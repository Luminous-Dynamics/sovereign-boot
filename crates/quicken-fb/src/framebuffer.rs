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
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::FileTypeExt;
use std::os::unix::io::{AsFd, BorrowedFd};
use std::path::PathBuf;

/// Maximum width or height accepted from a reported DRM mode.
const MAX_FRAMEBUFFER_DIMENSION: u32 = 16_384;
/// Bound the CPU output to about 144 MB at 32 bpp; DRM row-stride padding may add memory.
const MAX_FRAMEBUFFER_PIXELS: u64 = 36_000_000;

fn drm_card_index(file_name: &OsStr) -> Option<u32> {
    let name = file_name.to_str()?;
    let suffix = name.strip_prefix("card")?;
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) { return None; }
    suffix.parse().ok()
}

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

/// Copy a row-major frame into a possibly padded scanout buffer.
///
/// Returns false and leaves the destination unchanged if geometry or either
/// slice is insufficient. This also makes the stride path testable without DRM.
fn copy_frame_to_stride(
    dst: &mut [u32],
    src: &[u32],
    width: usize,
    height: usize,
    stride_pixels: usize,
) -> bool {
    if width == 0 || height == 0 || stride_pixels < width {
        return false;
    }
    let Some(source_len) = width.checked_mul(height) else {
        return false;
    };
    let Some(destination_len) = (height - 1)
        .checked_mul(stride_pixels)
        .and_then(|row_start| row_start.checked_add(width))
    else {
        return false;
    };
    if src.len() < source_len || dst.len() < destination_len {
        return false;
    }

    if stride_pixels == width {
        dst[..source_len].copy_from_slice(&src[..source_len]);
    } else {
        for y in 0..height {
            let source_start = y * width;
            let destination_start = y * stride_pixels;
            dst[destination_start..destination_start + width]
                .copy_from_slice(&src[source_start..source_start + width]);
        }
    }
    true
}

/// Errors from framebuffer operations.
#[derive(Debug)]
pub enum DrmError {
    DeviceOpen(String, std::io::Error),
    NoConnector,
    NoCardDevices,
    AutoDiscovery(std::io::Error),
    NoMode,
    NoEncoder,
    NoCrtc,
    InvalidModeDimensions { width: u32, height: u32 },
    ResourceQuery(std::io::Error),
    BufferCreate(std::io::Error),
    BufferMap(std::io::Error),
    FramebufferAdd(std::io::Error),
    ModeSetting(std::io::Error),
}

impl std::fmt::Display for DrmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeviceOpen(path, e) => write!(f, "cannot open DRM device {path}: {e}"),
            Self::NoConnector => write!(f, "no connected display found"),
            Self::NoCardDevices => write!(f, "no /dev/dri/cardN character devices found"),
            Self::AutoDiscovery(e) => write!(f, "cannot enumerate /dev/dri: {e}"),
            Self::NoMode => write!(f, "no display mode available"),
            Self::NoEncoder => write!(f, "no encoder for connector"),
            Self::NoCrtc => write!(f, "no CRTC available"),
            Self::InvalidModeDimensions { width, height } => write!(
                f,
                "display mode {width}x{height} exceeds the bounded framebuffer budget"
            ),
            Self::ResourceQuery(e) => write!(f, "DRM resource query failed: {e}"),
            Self::BufferCreate(e) => write!(f, "dumb buffer creation failed: {e}"),
            Self::BufferMap(e) => write!(f, "dumb buffer map failed: {e}"),
            Self::FramebufferAdd(e) => write!(f, "framebuffer add failed: {e}"),
            Self::ModeSetting(e) => write!(f, "mode setting failed: {e}"),
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
    /// Validated row-major pixel count for the CPU render buffer.
    pub pixel_count: usize,
    /// Stride in bytes (may be > width * 4 due to alignment).
    pub stride: u32,
    /// The display mode being used.
    pub mode: Mode,
    /// Dumb buffer for cleanup and mapping.
    dumb_buffer: control::dumbbuffer::DumbBuffer,
    /// Original CRTC state for restore on drop.
    original_crtc: Option<control::crtc::Info>,
}

impl DrmFramebuffer {
    /// Find the first numeric DRM card device with a connected display.
    pub fn open_auto() -> Result<(Self, String), DrmError> {
        let entries = fs::read_dir("/dev/dri").map_err(DrmError::AutoDiscovery)?;
        let mut candidates: Vec<(u32, PathBuf)> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let index = drm_card_index(&entry.file_name())?;
                let path = entry.path();
                let metadata = fs::symlink_metadata(&path).ok()?;
                if !metadata.file_type().is_char_device() { return None; }
                Some((index, path))
            })
            .collect();
        candidates.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        if candidates.is_empty() { return Err(DrmError::NoCardDevices); }
        let mut last_error = None;
        for (_, path) in candidates {
            let device = path.to_string_lossy().into_owned();
            match Self::open(&device) {
                Ok(framebuffer) => return Ok((framebuffer, device)),
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or(DrmError::NoCardDevices))
    }

    /// Validate a display mode before allocating its DRM or CPU pixel buffers.
    fn checked_pixel_count(width: u32, height: u32) -> Result<usize, DrmError> {
        let invalid = DrmError::InvalidModeDimensions { width, height };
        let pixels = u64::from(width)
            .checked_mul(u64::from(height))
            .ok_or(invalid)?;
        if width == 0
            || height == 0
            || width > MAX_FRAMEBUFFER_DIMENSION
            || height > MAX_FRAMEBUFFER_DIMENSION
            || pixels > MAX_FRAMEBUFFER_PIXELS
        {
            return Err(DrmError::InvalidModeDimensions { width, height });
        }
        usize::try_from(pixels)
            .map_err(|_| DrmError::InvalidModeDimensions { width, height })
    }

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

        // Save original CRTC for restoration
        let original_crtc = card.get_crtc(crtc).ok();

        let width = mode.size().0 as u32;
        let height = mode.size().1 as u32;
        // Validate before asking the DRM driver for a potentially large allocation.
        let pixel_count = Self::checked_pixel_count(width, height)?;

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
            pixel_count,
            stride,
            mode,
            dumb_buffer: db,
            original_crtc,
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
        Err(DrmError::NoConnector)
    }

    /// Stride in bytes.
    pub fn stride_bytes(&self) -> u32 {
        self.stride
    }

    /// Copy from a row-major u32 buffer (width*height) into the DRM dumb buffer.
    /// Maps the buffer, writes, and unmaps each frame.
    /// Copy a complete frame to scanout. False means the optional renderer
    /// should stop rather than spin while writes are failing.
    pub fn blit_from(&mut self, src: &[u32]) -> bool {
        let Ok(mut mapping) = self.card.map_dumb_buffer(&mut self.dumb_buffer) else {
            return false;
        };

        let stride_pixels = self.stride as usize / 4;
        let w = self.width as usize;
        let h = self.height as usize;

        // Reinterpret the u8 mapping as u32 slice
        let dst_bytes: &mut [u8] = &mut mapping;
        // SAFETY: XRGB8888 is 4-byte aligned, DRM guarantees alignment.
        let dst: &mut [u32] = unsafe {
            std::slice::from_raw_parts_mut(dst_bytes.as_mut_ptr() as *mut u32, dst_bytes.len() / 4)
        };

        // Reject an undersized mapping instead of displaying a partially copied frame.
        let complete = copy_frame_to_stride(dst, src, w, h, stride_pixels);
        // mapping is dropped here, which flushes/unmaps
        complete
    }

    /// Request a page flip (non-blocking). Returns immediately.
    pub fn page_flip(&self) -> Result<(), DrmError> {
        // For dumb buffers with a single FB, we just do a set_crtc.
        // True page-flipping with double-buffering would require two FBs.
        // For a boot animation at ~30fps, direct writes are fine.
        Ok(())
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
                &[],
                orig.mode(),
            );
        }

        // Destroy framebuffer
        let _ = self.card.destroy_framebuffer(self.fb);
        // DumbBuffer is dropped automatically, which calls destroy_dumb_buffer
    }
}

// SAFETY: The dumb buffer is tied to the Card file descriptor lifetime.
// Access is single-threaded (animation loop).
unsafe impl Send for DrmFramebuffer {}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tight_stride_copies_a_complete_frame() {
        let src = [1, 2, 3, 4, 5, 6];
        let mut dst = [0; 6];
        assert!(copy_frame_to_stride(&mut dst, &src, 3, 2, 3));
        assert_eq!(dst, src);
    }

    #[test]
    fn padded_stride_preserves_row_padding() {
        let src = [1, 2, 3, 4];
        let mut dst = [0xdead_beefu32; 8];
        assert!(copy_frame_to_stride(&mut dst, &src, 2, 2, 4));
        assert_eq!(dst, [1, 2, 0xdead_beef, 0xdead_beef, 3, 4, 0xdead_beef, 0xdead_beef]);
    }

    #[test]
    fn undersized_buffers_are_rejected_without_partial_writes() {
        let src = [1, 2, 3, 4];
        let mut dst = [0xdead_beefu32; 5];
        assert!(!copy_frame_to_stride(&mut dst, &src, 2, 2, 4));
        assert_eq!(dst, [0xdead_beef; 5]);
        let mut full_dst = [0xdead_beefu32; 6];
        assert!(!copy_frame_to_stride(&mut full_dst, &src[..3], 2, 2, 3));
        assert_eq!(full_dst, [0xdead_beef; 6]);
    }

    #[test]
    fn invalid_stride_and_overflowing_geometry_are_rejected() {
        let mut dst = [];
        let src = [];
        assert!(!copy_frame_to_stride(&mut dst, &src, 2, 1, 1));
        assert!(!copy_frame_to_stride(&mut dst, &src, usize::MAX, 2, usize::MAX));
    }

    #[test]
    fn accepts_bounded_hd_4k_and_8k_modes() {
        assert_eq!(DrmFramebuffer::checked_pixel_count(1920, 1080).unwrap(), 2_073_600);
        assert_eq!(DrmFramebuffer::checked_pixel_count(3840, 2160).unwrap(), 8_294_400);
        assert_eq!(DrmFramebuffer::checked_pixel_count(7680, 4320).unwrap(), 33_177_600);
        assert_eq!(DrmFramebuffer::checked_pixel_count(8192, 4320).unwrap(), 35_389_440);
    }

    #[test]
    fn rejects_zero_axes_and_oversized_dimensions() {
        assert!(DrmFramebuffer::checked_pixel_count(0, 1080).is_err());
        assert!(DrmFramebuffer::checked_pixel_count(1920, 0).is_err());
        assert!(DrmFramebuffer::checked_pixel_count(MAX_FRAMEBUFFER_DIMENSION + 1, 1).is_err());
        assert!(DrmFramebuffer::checked_pixel_count(16_384, 2_198).is_err());
    }
}


#[cfg(test)]
mod device_discovery_tests {
    use super::drm_card_index;
    use std::ffi::OsStr;

    #[test]
    fn parses_only_canonical_numeric_card_names() {
        assert_eq!(drm_card_index(OsStr::new("card0")), Some(0));
        assert_eq!(drm_card_index(OsStr::new("card12")), Some(12));
        for name in ["", "card", "cardx", "card-1", "card2foo", "renderD128", "by-path"] {
            assert_eq!(drm_card_index(OsStr::new(name)), None, "unexpected candidate: {name}");
        }
    }
}
