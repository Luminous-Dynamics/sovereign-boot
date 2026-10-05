// Copyright (C) 2024-2026 Tristan Stoltz / Luminous Dynamics
// SPDX-License-Identifier: AGPL-3.0-or-later
// Commercial licensing: see COMMERCIAL_LICENSE.md at repository root
//! Linux virtual-terminal handoff used only by the bounded physical canary.
//!
//! Direct KMS testing should run from the active text VT, not a graphical
//! terminal emulator. KDSETMODE(KD_GRAPHICS) prevents fbcon from concurrently
//! writing the same console while the canary owns the CRTC. The original VT
//! mode is restored after the DRM framebuffer has been dropped.

use std::fs::OpenOptions;
use std::os::fd::{AsRawFd, RawFd};
use std::path::Path;

pub struct VirtualTerminalGuard {
    _tty: std::fs::File,
    fd: RawFd,
    original_mode: nix::libc::c_int,
}

impl VirtualTerminalGuard {
    /// Enter graphics mode on the active real VT.
    ///
    /// Refuses pseudo-terminals and inactive VTs so a canary cannot be launched
    /// from Kitty, a Wayland/X11 terminal, or a different console.
    pub fn enter() -> Result<Self, String> {
        // The compositor must already have released the display before the
        // canary is permitted to take DRM ownership.
        match std::process::Command::new("systemctl")
            .args(["is-active", "--quiet", "display-manager.service"])
            .status()
        {
            Ok(status) if status.success() => {
                return Err("display-manager.service is still active; stop it before the canary".into());
            }
            Ok(_) => {}
            Err(e) => {
                return Err(format!("cannot verify display-manager.service state: {e}"));
            }
        }

        let stdin = std::fs::read_link("/proc/self/fd/0")
            .map_err(|e| format!("cannot identify stdin terminal: {e}"))?;
        let tty_name = tty_name(&stdin)
            .ok_or_else(|| format!("canary requires a real VT, got {}", stdin.display()))?;

        let active = std::fs::read_to_string("/sys/class/tty/tty0/active")
            .map_err(|e| format!("cannot identify active VT: {e}"))?
            .trim()
            .to_string();

        if active != tty_name {
            return Err(format!(
                "canary must run on the active VT: stdin={tty_name} active={active}"
            ));
        }

        let tty = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .map_err(|e| format!("cannot open controlling VT: {e}"))?;
        let fd = tty.as_raw_fd();

        let mut original_mode = 0;
        let rc = unsafe { nix::libc::ioctl(fd, nix::libc::KDGETMODE, &mut original_mode) };
        if rc < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }

        if original_mode != nix::libc::KD_TEXT {
            return Err(format!(
                "canary requires a text VT before takeover; current mode={original_mode}"
            ));
        }

        let rc = unsafe {
            nix::libc::ioctl(
                fd,
                nix::libc::KDSETMODE,
                nix::libc::KD_GRAPHICS as nix::libc::c_ulong,
            )
        };
        if rc < 0 {
            return Err(format!(
                "cannot enter VT graphics mode: {}",
                std::io::Error::last_os_error()
            ));
        }

        Ok(Self {
            _tty: tty,
            fd,
            original_mode,
        })
    }
}

impl Drop for VirtualTerminalGuard {
    fn drop(&mut self) {
        // Best-effort restoration: the DRM guard is dropped before this guard,
        // so CRTC ownership is returned before fbcon is re-enabled.
        let _ = unsafe {
            nix::libc::ioctl(
                self.fd,
                nix::libc::KDSETMODE,
                self.original_mode as nix::libc::c_ulong,
            )
        };
    }
}

fn tty_name(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?.to_string();
    let suffix = name.strip_prefix("tty")?;
    if suffix.is_empty() || !suffix.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(name)
}
