// Copyright (C) 2024-2026 Tristan Stoltz / Luminous Dynamics
// SPDX-License-Identifier: AGPL-3.0-or-later
// Commercial licensing: see COMMERCIAL_LICENSE.md at repository root
/// Installation progress monitoring.
///
/// Reads events from a named pipe (FIFO) written by the installer, or
/// falls back to polling /proc/diskstats for disk I/O rate estimation.
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read};
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};
use std::path::Path;
use std::time::{Duration, Instant};

/// Maximum bytes accepted into a single progress event.
const MAX_PROGRESS_LINE_BYTES: usize = 512;
/// Bound time spent draining a continuously-written FIFO on any one frame.
const MAX_PROGRESS_BYTES_PER_POLL: usize = 4096;

/// Events emitted by the progress monitor.
#[derive(Debug, Clone)]
pub enum ProgressEvent {
    /// Disk write observed (cumulative bytes).
    DiskWrite(u64),
    /// A Nix derivation has finished building.
    DerivationComplete(String),
    /// Installation phase change (e.g., "partitioning", "formatting", "installing", "bootloader").
    PhaseChange(String),
    /// Installation is complete — begin final animation.
    InstallComplete,
    /// I/O rate estimate (bytes per second).
    IoRate(f64),
}

/// Progress monitor that reads from a named pipe.
pub struct ProgressMonitor {
    reader: Option<File>,
    /// Fallback: poll /proc/diskstats
    last_diskstats_bytes: u64,
    last_diskstats_time: Instant,
    /// Normalized I/O rate (0.0 to 1.0).
    pub io_rate: f32,
    /// Whether installation is complete.
    pub complete: bool,
    line_buf: String,
    /// Discard an oversized or non-ASCII record until the next newline.
    discarding_line: bool,
}

impl ProgressMonitor {
    /// Create a monitor reading from the given named pipe path.
    /// If the pipe doesn't exist or can't be opened, falls back to diskstats polling.
    pub fn new(pipe_path: Option<&str>) -> Self {
        let reader = pipe_path.and_then(|path| {
            let p = Path::new(path);
            // Refuse symlinks and non-FIFOs. This path is opened by an early-boot
            // process; don't follow a configured FIFO path through a symlink.
            let meta = std::fs::symlink_metadata(p).ok()?;
            if !meta.file_type().is_fifo() {
                return None;
            }

            // O_NONBLOCK is essential here: a reader-only FIFO open can otherwise
            // hang boot while waiting for a writer that might never appear.
            OpenOptions::new()
                .read(true)
                .custom_flags(nix::libc::O_NONBLOCK | nix::libc::O_CLOEXEC)
                .open(p)
                .ok()
        });

        Self {
            reader,
            last_diskstats_bytes: Self::read_diskstats_bytes(),
            last_diskstats_time: Instant::now(),
            io_rate: 0.0,
            complete: false,
            line_buf: String::with_capacity(256),
            discarding_line: false,
        }
    }

    /// Poll for new events. Non-blocking: returns events available right now.
    pub fn poll(&mut self) -> Vec<ProgressEvent> {
        let mut events = Vec::new();

        if self.reader.is_some() {
            // Fixed-size reads plus a per-poll byte budget keep an untrusted or
            // continuously-writing FIFO from stalling the render loop.
            let mut chunk = [0u8; 512];
            let mut bytes_read = 0usize;
            let mut remove_reader = false;

            while bytes_read < MAX_PROGRESS_BYTES_PER_POLL {
                let remaining = MAX_PROGRESS_BYTES_PER_POLL - bytes_read;
                let read_len = chunk.len().min(remaining);
                let result = match self.reader.as_mut() {
                    Some(reader) => reader.read(&mut chunk[..read_len]),
                    None => break,
                };

                match result {
                    Ok(0) => break,
                    Ok(count) => {
                        bytes_read += count;
                        for &byte in &chunk[..count] {
                            if byte == b'\n' {
                                if !self.discarding_line {
                                    let line = self.line_buf.trim();
                                    if let Some(event) = Self::parse_line(line) {
                                        if matches!(event, ProgressEvent::InstallComplete) {
                                            self.complete = true;
                                        }
                                        events.push(event);
                                    }
                                }
                                self.line_buf.clear();
                                self.discarding_line = false;
                                continue;
                            }

                            if self.discarding_line {
                                continue;
                            }

                            let allowed_ascii = byte.is_ascii()
                                && (!byte.is_ascii_control() || byte == b'\r' || byte == b'\t');
                            if !allowed_ascii || self.line_buf.len() >= MAX_PROGRESS_LINE_BYTES {
                                self.line_buf.clear();
                                self.discarding_line = true;
                            } else {
                                self.line_buf.push(byte as char);
                            }
                        }
                    }
                    Err(ref error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(ref error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(_) => {
                        remove_reader = true;
                        break;
                    }
                }
            }

            if remove_reader {
                self.reader = None;
                self.line_buf.clear();
                self.discarding_line = false;
            }
        }

        // Always poll diskstats for I/O rate
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_diskstats_time);
        if elapsed >= Duration::from_millis(250) {
            let current_bytes = Self::read_diskstats_bytes();
            let delta = current_bytes.saturating_sub(self.last_diskstats_bytes);
            let bytes_per_sec = delta as f64 / elapsed.as_secs_f64();

            // Normalize: 0 = idle, 1.0 = ~200MB/s (fast NVMe write)
            self.io_rate = (bytes_per_sec / 200_000_000.0).min(1.0) as f32;

            if delta > 0 {
                events.push(ProgressEvent::IoRate(bytes_per_sec));
                events.push(ProgressEvent::DiskWrite(current_bytes));
            }

            self.last_diskstats_bytes = current_bytes;
            self.last_diskstats_time = now;
        }

        events
    }

    /// Parse a line from the progress pipe.
    /// Protocol:
    ///   DRV:<derivation-name>
    ///   PHASE:<phase-name>
    ///   COMPLETE
    ///   WRITE:<bytes>
    fn parse_line(line: &str) -> Option<ProgressEvent> {
        if line.is_empty() {
            return None;
        }
        if let Some(drv) = line.strip_prefix("DRV:") {
            return Some(ProgressEvent::DerivationComplete(drv.trim().to_string()));
        }
        if let Some(phase) = line.strip_prefix("PHASE:") {
            return Some(ProgressEvent::PhaseChange(phase.trim().to_string()));
        }
        if line == "COMPLETE" {
            return Some(ProgressEvent::InstallComplete);
        }
        if let Some(bytes_str) = line.strip_prefix("WRITE:") {
            if let Ok(bytes) = bytes_str.trim().parse::<u64>() {
                return Some(ProgressEvent::DiskWrite(bytes));
            }
        }
        None
    }

    /// Read total sector writes from /proc/diskstats (all block devices).
    /// Returns approximate bytes written.
    fn read_diskstats_bytes() -> u64 {
        let mut total: u64 = 0;
        let path = Path::new("/proc/diskstats");
        if !path.exists() {
            return 0;
        }
        let Ok(file) = File::open(path) else {
            return 0;
        };
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let Ok(line) = line else { continue };
            let fields: Vec<&str> = line.split_whitespace().collect();
            // Field 9 (0-indexed) is sectors written
            if fields.len() >= 10 {
                let name = fields[2];
                // Only count whole-disk devices, skip partitions
                if name.starts_with("sd") || name.starts_with("nvme") || name.starts_with("vd") {
                    if let Ok(sectors) = fields[9].parse::<u64>() {
                        total += sectors * 512; // sectors are 512 bytes
                    }
                }
            }
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_derivation() {
        let ev = ProgressMonitor::parse_line("DRV:nixos-system-luminous-24.11");
        assert!(
            matches!(ev, Some(ProgressEvent::DerivationComplete(ref s)) if s == "nixos-system-luminous-24.11")
        );
    }

    #[test]
    fn test_parse_phase() {
        let ev = ProgressMonitor::parse_line("PHASE:formatting");
        assert!(matches!(ev, Some(ProgressEvent::PhaseChange(ref s)) if s == "formatting"));
    }

    #[test]
    fn test_parse_complete() {
        let ev = ProgressMonitor::parse_line("COMPLETE");
        assert!(matches!(ev, Some(ProgressEvent::InstallComplete)));
    }

    #[test]
    fn test_parse_write() {
        let ev = ProgressMonitor::parse_line("WRITE:1048576");
        assert!(matches!(ev, Some(ProgressEvent::DiskWrite(1048576))));
    }

    #[test]
    fn test_parse_empty() {
        assert!(ProgressMonitor::parse_line("").is_none());
    }

    #[test]
    fn test_parse_unknown() {
        assert!(ProgressMonitor::parse_line("GARBAGE:stuff").is_none());
    }

    #[test]
    fn test_monitor_creation_no_pipe() {
        let mon = ProgressMonitor::new(None);
        assert!(!mon.complete);
        assert!(mon.reader.is_none());
    }

    #[test]
    fn test_monitor_creation_nonexistent_pipe() {
        let mon = ProgressMonitor::new(Some("/nonexistent/pipe"));
        assert!(mon.reader.is_none());
    }

    #[test]
    fn progress_monitor_refuses_regular_file_and_symlink() {
        use std::os::unix::fs::symlink;
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "quicken-progress-path-{}-{nonce}",
            std::process::id()
        ));
        let regular = base.with_extension("txt");
        let link = base.with_extension("link");
        std::fs::write(&regular, b"not a fifo").expect("write regular file");
        symlink(&regular, &link).expect("make symlink");

        assert!(ProgressMonitor::new(regular.to_str()).reader.is_none());
        assert!(ProgressMonitor::new(link.to_str()).reader.is_none());

        let _ = std::fs::remove_file(regular);
        let _ = std::fs::remove_file(link);
    }

    #[test]
    fn fifo_without_writer_does_not_block_and_oversized_line_is_discarded() {
        use std::io::Write;
        use std::time::{SystemTime, UNIX_EPOCH};

        assert_eq!(MAX_PROGRESS_LINE_BYTES, 512);
        assert_eq!(MAX_PROGRESS_BYTES_PER_POLL, 4096);

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "quicken-progress-{}-{nonce}.fifo",
            std::process::id()
        ));
        nix::unistd::mkfifo(
            &path,
            nix::sys::stat::Mode::from_bits_truncate(0o600),
        )
        .expect("create test FIFO");

        // This construction must return before any writer has opened the FIFO.
        let mut monitor = ProgressMonitor::new(Some(path.to_str().expect("UTF-8 temp path")));
        assert!(monitor.reader.is_some());

        let mut writer = OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open writer after nonblocking reader");
        let oversized = format!("DRV:{}\n", "x".repeat(MAX_PROGRESS_LINE_BYTES + 32));
        writer
            .write_all(format!("{oversized}COMPLETE\n").as_bytes())
            .expect("write bounded test stream");
        drop(writer);

        let events = monitor.poll();
        assert!(monitor.complete, "valid event after oversized line was lost");
        assert!(
            !events.iter().any(|event| matches!(event, ProgressEvent::DerivationComplete(_))),
            "oversized FIFO record must be discarded"
        );

        std::fs::remove_file(path).expect("remove test FIFO");
    }
}
