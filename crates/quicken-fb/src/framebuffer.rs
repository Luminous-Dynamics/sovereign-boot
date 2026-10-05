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
    RestoreVerification(String),
    SourceBufferTooSmall { expected: usize, actual: usize },
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
            Self::CrtcQuery(e) => write!(f, "CRTC state query failed: {e}"),
            Self::RestoreVerification(details) => write!(f, "CRTC restoration verification failed: {details}"),
            Self::SourceBufferTooSmall { expected, actual } => write!(f, "render buffer too small: expected {expected} pixels, got {actual}"),
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
    restored: bool,
}

fn select_crtc(
    card: &Card,
    res: &ResourceHandles,
    connector: &ConnectorInfo,
) -> Result<(CrtcHandle, &'static str), DrmError> {
    if let Some(encoder_handle) = connector.current_encoder() {
        let encoder = card
            .get_encoder(encoder_handle)
            .map_err(DrmError::ResourceQuery)?;
        if let Some(crtc) = encoder.crtc() {
            return Ok((crtc, "current"));
        }
    }

    for &encoder_handle in connector.encoders() {
        let encoder = card
            .get_encoder(encoder_handle)
            .map_err(DrmError::ResourceQuery)?;
        for crtc in res.filter_crtcs(encoder.possible_crtcs()) {
            if connected_connectors_for_crtc(card, res, crtc)?.is_empty() {
                return Ok((crtc, "free-compatible"));
            }
        }
    }

    Err(DrmError::NoCrtc)
}

fn connected_connectors_for_crtc(
    card: &Card,
    res: &ResourceHandles,
    crtc: CrtcHandle,
) -> Result<Vec<control::connector::Handle>, DrmError> {
    let mut connectors = Vec::new();
    for &connector_handle in res.connectors() {
        let connector = card
            .get_connector(connector_handle, false)
            .map_err(DrmError::ResourceQuery)?;
        if let Some(encoder_handle) = connector.current_encoder() {
            let encoder = card
                .get_encoder(encoder_handle)
                .map_err(DrmError::ResourceQuery)?;
            if encoder.crtc() == Some(crtc) {
                connectors.push(connector_handle);
            }
        }
    }
    Ok(connectors)
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

/// Evidence emitted after the original scanout state is independently verified.
#[derive(Debug, Clone, Copy)]
pub struct DrmRestoreReceipt {
    pub crtc: CrtcHandle,
    pub connector_count: usize,
    pub framebuffer: Option<FbHandle>,
    pub mode_width: u32,
    pub mode_height: u32,
    pub refresh_hz: u32,
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
    pub selection_source: &'static str,
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

        // Prefer the connector's currently active CRTC, but when the
        // compositor has released KMS there may be no current encoder. In that
        // early-boot state select a compatible CRTC that is presently free.
        let (crtc, _selection_source) = select_crtc(&card, &res, &connector)?;

        // Capture the original state before any modeset. Failing closed here
        // guarantees that every successful modeset has a restoration snapshot.
        let original_crtc = Some(card.get_crtc(crtc).map_err(DrmError::CrtcQuery)?);

        // Capture every connector currently attached to this CRTC. The legacy
        // SETCRTC restore must include connector attachment as well as
        // framebuffer/mode state.
        let original_connectors = connected_connectors_for_crtc(&card, &res, crtc)?
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
            restored: false,
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
    pub fn blit_from(&mut self, src: &[u32]) -> Result<(), DrmError> {
        let expected = (self.width as usize)
            .checked_mul(self.height as usize)
            .ok_or(DrmError::SourceBufferTooSmall {
                expected: usize::MAX,
                actual: src.len(),
            })?;
        if src.len() != expected {
            return Err(DrmError::SourceBufferTooSmall {
                expected,
                actual: src.len(),
            });
        }

        let mut mapping = self
            .card
            .map_dumb_buffer(&mut self.dumb_buffer)
            .map_err(DrmError::BufferMap)?;

        let stride_pixels = self.stride as usize / 4;
        let w = self.width as usize;
        let h = self.height as usize;

        // XRGB8888 is a 4-byte pixel format. bytemuck performs the typed-view
        // alignment/length checks for us.
        let dst_bytes: &mut [u8] = &mut mapping;
        let dst: &mut [u32] = bytemuck::cast_slice_mut(dst_bytes);

        if stride_pixels < w || dst.len() < stride_pixels.saturating_mul(h) {
            return Err(DrmError::BufferMap(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "DRM dumb buffer mapping is smaller than its declared stride/height",
            )));
        }

        if stride_pixels == w {
            // Fast path: no padding
            dst[..expected].copy_from_slice(src);
        } else {
            // Stride-aware copy
            for y in 0..h {
                let src_start = y * w;
                let dst_start = y * stride_pixels;
                dst[dst_start..dst_start + w].copy_from_slice(&src[src_start..src_start + w]);
            }
        }
        // mapping is dropped here, which flushes/unmaps
        Ok(())
    }

    /// Restore the CRTC and connector topology captured before our modeset.
    ///
    /// This is the authoritative restoration path. It performs the legacy SETCRTC,
    /// re-queries the CRTC, and independently reconstructs the connector set before
    /// reporting success. Drop calls the same method as a final best-effort fallback.
    pub fn restore(&mut self) -> Result<DrmRestoreReceipt, DrmError> {
        if self.restored {
            return Ok(self.restore_receipt());
        }

        let original = self
            .original_crtc
            .as_ref()
            .ok_or(DrmError::CrtcQuery(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no original CRTC snapshot available",
            )))?;

        self.card
            .set_crtc(
                self.crtc,
                original.framebuffer(),
                original.position(),
                &self.original_connectors,
                original.mode(),
            )
            .map_err(DrmError::ModeSetting)?;

        let observed = self.card.get_crtc(self.crtc).map_err(DrmError::CrtcQuery)?;
        if observed.framebuffer() != original.framebuffer()
            || observed.position() != original.position()
            || observed.mode() != original.mode()
        {
            return Err(DrmError::RestoreVerification(format!(
                "CRTC state mismatch: expected fb={:?} pos={:?} mode={:?}, observed fb={:?} pos={:?} mode={:?}",
                original.framebuffer(),
                original.position(),
                original.mode(),
                observed.framebuffer(),
                observed.position(),
                observed.mode(),
            )));
        }

        let resources = self
            .card
            .resource_handles()
            .map_err(DrmError::ResourceQuery)?;
        let observed_connectors = connected_connectors_for_crtc(&self.card, &resources, self.crtc)?;
        if observed_connectors != self.original_connectors {
            return Err(DrmError::RestoreVerification(format!(
                "connector topology mismatch: expected {:?}, observed {:?}",
                self.original_connectors, observed_connectors
            )));
        }

        self.restored = true;
        Ok(self.restore_receipt())
    }

    fn restore_receipt(&self) -> DrmRestoreReceipt {
        let (mode_width, mode_height, refresh_hz) = self
            .original_crtc
            .as_ref()
            .and_then(|crtc| crtc.mode())
            .map(|mode| (mode.size().0 as u32, mode.size().1 as u32, mode.vrefresh()))
            .unwrap_or((0, 0, 0));
        DrmRestoreReceipt {
            crtc: self.crtc,
            connector_count: self.original_connectors.len(),
            framebuffer: self.original_crtc.as_ref().and_then(|crtc| crtc.framebuffer()),
            mode_width,
            mode_height,
            refresh_hz,
        }
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
        let (crtc, selection_source) = select_crtc(&card, &res, &connector)?;
        Ok(DrmProbe {
            width: mode.size().0 as u32,
            height: mode.size().1 as u32,
            refresh_hz: mode.vrefresh(),
            connector_interface: connector.interface().as_str(),
            connector_interface_id: connector.interface_id(),
            crtc,
            selection_source,
        })
    }
}

impl Drop for DrmFramebuffer {
    fn drop(&mut self) {
        if !self.restored {
            let _ = self.restore();
        }

        // Destroy framebuffer only after restoration has been attempted.
        let _ = self.card.destroy_framebuffer(self.fb);
        // DumbBuffer is dropped automatically, which calls destroy_dumb_buffer.
    }
}

