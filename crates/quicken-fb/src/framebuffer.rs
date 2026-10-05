// Copyright (C) 2024-2026 Tristan Stoltz / Luminous Dynamics
// SPDX-License-Identifier: AGPL-3.0-or-later
// Commercial licensing: see COMMERCIAL_LICENSE.md at repository root
use drm::Device;
/// DRM/KMS framebuffer abstraction.
///
/// Opens a DRM device, finds a connected display, creates a dumb buffer at
/// native resolution, and maps it for direct pixel access on each frame.
/// No display server required — this runs on bare metal during NixOS installation.
use drm::buffer::Buffer;
use drm::control::connector::{Info as ConnectorInfo, State as ConnectorState};
use drm::control::crtc::Handle as CrtcHandle;
use drm::control::framebuffer::Handle as FbHandle;
use drm::control::{self, Device as ControlDevice, Mode, ResourceHandles};
use std::fs::{File, OpenOptions};
use std::os::unix::io::{AsFd, BorrowedFd};

/// A DRM card device wrapper implementing the drm traits.
struct Card(File);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl Device for Card {}
impl ControlDevice for Card {}

impl Card {
    fn open(path: &str) -> Result<Self, DrmError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| DrmError::DeviceOpen(path.to_string(), e))?;
        Ok(Card(file))
    }
}

/// Errors from framebuffer operations.
#[derive(Debug)]
pub enum DrmError {
    DeviceOpen(String, std::io::Error),
    NoConnector,
    NoConnectedDisplay(String),
    NoMode,
    NoEncoder,
    NoCrtc,
    ResourceQuery(std::io::Error),
    BufferCreate(std::io::Error),
    BufferMap(std::io::Error),
    FramebufferAdd(std::io::Error),
    ModeSetting(std::io::Error),
    CrtcQuery(std::io::Error),
}

impl std::fmt::Display for DrmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeviceOpen(path, e) => write!(f, "cannot open DRM device {path}: {e}"),
            Self::NoConnector => write!(f, "no connector available"),
            Self::NoConnectedDisplay(details) => write!(f, "no connected display found; {details}"),
            Self::NoMode => write!(f, "no display mode available"),
            Self::NoEncoder => write!(f, "no encoder for connector"),
            Self::NoCrtc => write!(f, "no CRTC available"),
            Self::ResourceQuery(e) => write!(f, "DRM resource query failed: {e}"),
            Self::BufferCreate(e) => write!(f, "dumb buffer creation failed: {e}"),
            Self::BufferMap(e) => write!(f, "dumb buffer map failed: {e}"),
            Self::FramebufferAdd(e) => write!(f, "framebuffer add failed: {e}"),
            Self::ModeSetting(e) => write!(f, "mode setting failed: {e}"),
            Self::CrtcQuery(e) => write!(f, "original CRTC state query failed: {e}"),
        }
    }
}

impl std::error::Error for DrmError {}

/// An active DRM framebuffer with mapped pixel memory.
pub struct DrmFramebuffer {
    card: Card,
    crtc: CrtcHandle,
    fb: FbHandle,
    /// Display width in pixels.
    pub width: u32,
    /// Display height in pixels.
    pub height: u32,
    /// Stride in bytes (may be > width * 4 due to alignment).
    pub stride: u32,
    /// The display mode being used.
    pub mode: Mode,
    /// Dumb buffer for cleanup and mapping.
    dumb_buffer: control::dumbbuffer::DumbBuffer,
    /// Original CRTC state for restore on drop.
    original_crtc: Option<control::crtc::Info>,
    /// Connectors originally driven by the selected CRTC.
    original_connectors: Vec<control::connector::Handle>,
}

/// Describe connector state without forcing a probe or changing modeset state.
fn connector_diagnostics(card: &Card, res: &ResourceHandles) -> String {
    let mut entries = Vec::new();
    for &conn_handle in res.connectors() {
        let conn = match card.get_connector(conn_handle, false) {
            Ok(c) => c,
            Err(_) => continue,
        };
        entries.push(format!(
            "{}-{}:{:?}:modes={}",
            conn.interface().as_str(),
            conn.interface_id(),
            conn.state(),
            conn.modes().len()
        ));
    }
    if entries.is_empty() {
        "card exposes no readable connectors".to_string()
    } else {
        format!("connectors=[{}]", entries.join(","))
    }
}

/// Non-mutating DRM hardware qualification result.
#[derive(Debug, Clone, Copy)]
pub struct DrmProbe {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
    pub connector_interface: &'static str,
    pub connector_interface_id: u32,
    pub crtc: CrtcHandle,
}

impl DrmFramebuffer {
    /// Open a DRM device at the given path (e.g., "/dev/dri/card0"),
    /// find the first connected display, set up a framebuffer.
    pub fn open(device_path: &str) -> Result<Self, DrmError> {
        let card = Card::open(device_path)?;

        // Query resources
        let res = card.resource_handles().map_err(DrmError::ResourceQuery)?;

        // Find first connected connector with a valid mode
        let (connector, mode) = Self::find_connected_display(&card, &res)?;

        // Find encoder + CRTC
        let encoder_handle = connector.current_encoder().ok_or(DrmError::NoEncoder)?;
        let encoder = card
            .get_encoder(encoder_handle)
            .map_err(DrmError::ResourceQuery)?;
        let crtc = encoder.crtc().ok_or(DrmError::NoCrtc)?;

        // Capture the original state before any modeset. Failing closed here
        // guarantees that every successful modeset has a restoration snapshot.
        let original_crtc = Some(card.get_crtc(crtc).map_err(DrmError::CrtcQuery)?);

        // Capture every connector currently attached to this CRTC. The legacy
        // SETCRTC restore must include connector attachment as well as
        // framebuffer/mode state.
        let mut original_connectors = Vec::new();
        for &connector_handle in res.connectors() {
            let connector = card
                .get_connector(connector_handle, false)
                .map_err(DrmError::ResourceQuery)?;
            if let Some(encoder_handle) = connector.current_encoder() {
                let encoder = card
                    .get_encoder(encoder_handle)
                    .map_err(DrmError::ResourceQuery)?;
                if encoder.crtc() == Some(crtc) {
                    original_connectors.push(connector_handle);
                }
            }
        }
        if original_connectors.is_empty() {
            return Err(DrmError::NoConnector);
        }

        let width = mode.size().0 as u32;
        let height = mode.size().1 as u32;

        // Create dumb buffer (32bpp XRGB8888)
        let db = card
            .create_dumb_buffer((width, height), drm::buffer::DrmFourcc::Xrgb8888, 32)
            .map_err(DrmError::BufferCreate)?;

        let stride = db.pitch();
        // Add framebuffer
        let fb = card
            .add_framebuffer(&db, 24, 32)
            .map_err(DrmError::FramebufferAdd)?;

        // Set the CRTC to display our framebuffer
        card.set_crtc(crtc, Some(fb), (0, 0), &[connector.handle()], Some(mode))
            .map_err(DrmError::ModeSetting)?;

        Ok(Self {
            card,
            crtc,
            fb,
            width,
            height,
            stride,
            mode,
            dumb_buffer: db,
            original_crtc,
            original_connectors,
        })
    }

    /// Find the first connected connector and its preferred mode.
    fn find_connected_display(
        card: &Card,
        res: &ResourceHandles,
    ) -> Result<(ConnectorInfo, Mode), DrmError> {
        for &conn_handle in res.connectors() {
            let conn = match card.get_connector(conn_handle, false) {
                Ok(c) => c,
                Err(_) => continue,
            };
            if conn.state() != ConnectorState::Connected {
                continue;
            }
            let modes = conn.modes().to_vec();
            if modes.is_empty() {
                continue;
            }
            // Prefer the first mode (usually the preferred/native resolution)
            let mode = modes
                .iter()
                .find(|m| m.mode_type().contains(control::ModeTypeFlags::PREFERRED))
                .unwrap_or(&modes[0])
                .clone();
            return Ok((conn, mode));
        }
        let details = connector_diagnostics(card, res);
        Err(DrmError::NoConnectedDisplay(details))
    }

    /// Stride in bytes.
    pub fn stride_bytes(&self) -> u32 {
        self.stride
    }

    /// Copy from a row-major u32 buffer (width*height) into the DRM dumb buffer.
    /// Maps the buffer, writes, and unmaps each frame.
    pub fn blit_from(&mut self, src: &[u32]) {
        let Ok(mut mapping) = self.card.map_dumb_buffer(&mut self.dumb_buffer) else {
            return;
        };

        let stride_pixels = self.stride as usize / 4;
        let w = self.width as usize;
        let h = self.height as usize;

        // XRGB8888 is a 4-byte pixel format. bytemuck performs the typed-view
        // alignment/length checks for us.
        let dst_bytes: &mut [u8] = &mut mapping;
        let dst: &mut [u32] = bytemuck::cast_slice_mut(dst_bytes);

        if stride_pixels == w {
            // Fast path: no padding
            let copy_len = (w * h).min(dst.len()).min(src.len());
            dst[..copy_len].copy_from_slice(&src[..copy_len]);
        } else {
            // Stride-aware copy
            for y in 0..h {
                let src_start = y * w;
                let dst_start = y * stride_pixels;
                let row_end = src_start + w;
                if row_end > src.len() || dst_start + w > dst.len() {
                    break;
                }
                dst[dst_start..dst_start + w].copy_from_slice(&src[src_start..row_end]);
            }
        }
        // mapping is dropped here, which flushes/unmaps
    }

    /// Probe a DRM device without creating a framebuffer or changing CRTC state.
    ///
    /// This is the recommended first physical-system test: it verifies that the
    /// device can be opened and that a connected display, usable mode, encoder,
    /// and CRTC are discoverable without modesetting the display.
    pub fn probe(device_path: &str) -> Result<DrmProbe, DrmError> {
        let card = Card::open(device_path)?;
        let res = card.resource_handles().map_err(DrmError::ResourceQuery)?;
        let (connector, mode) = Self::find_connected_display(&card, &res)?;
        let encoder_handle = connector.current_encoder().ok_or(DrmError::NoEncoder)?;
        let encoder = card
            .get_encoder(encoder_handle)
            .map_err(DrmError::ResourceQuery)?;
        let crtc = encoder.crtc().ok_or(DrmError::NoCrtc)?;
        Ok(DrmProbe {
            width: mode.size().0 as u32,
            height: mode.size().1 as u32,
            refresh_hz: mode.vrefresh(),
            connector_interface: connector.interface().as_str(),
            connector_interface_id: connector.interface_id(),
            crtc,
        })
    }
}

impl Drop for DrmFramebuffer {
    fn drop(&mut self) {
        // Restore original CRTC if we saved it
        if let Some(ref orig) = self.original_crtc {
            let _ = self.card.set_crtc(
                self.crtc,
                orig.framebuffer(),
                orig.position(),
                &self.original_connectors,
                orig.mode(),
            );
        }

        // Destroy framebuffer
        let _ = self.card.destroy_framebuffer(self.fb);
        // DumbBuffer is dropped automatically, which calls destroy_dumb_buffer
    }
}

