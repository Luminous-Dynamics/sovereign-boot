// Copyright (C) 2024-2026 Tristan Stoltz / Luminous Dynamics
// SPDX-License-Identifier: AGPL-3.0-or-later
// Commercial licensing: see COMMERCIAL_LICENSE.md at repository root
/// Mycelial colonization boot animation for NixOS installation.
///
/// Runs on bare-metal DRM/KMS framebuffer — no display server, no GPU acceleration.
/// Renders procedural mycelial growth seeded by a public, non-secret visual seed.
///
/// Usage:
///   quicken-fb --theme mycelial-genesis
///   quicken-fb --theme cosmic-atlas --seed public-demo-v1
///   quicken-fb --theme graphite-focus --device /dev/dri/card1
///   quicken-fb --list-themes
///
/// Seeds affect visual variation only. Do not pass secrets or identity material.
///
/// Signal handling:
///   SIGTERM — clean exit (restore CRTC, unmap, fade to black)
///   SIGINT  — same as SIGTERM
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use symthaea_quicken_fb::framebuffer::DrmFramebuffer;
use symthaea_quicken_fb::mycelium::MycelialNetwork;
use symthaea_quicken_fb::progress::{ProgressEvent, ProgressMonitor};
use symthaea_quicken_fb::theme::{MotionMode, Theme, ThemeSurface};

/// Target frame rate for the animation.
const TARGET_FPS: u32 = 30;

/// State artifacts are presence-gated only; the renderer never reads their contents.
const MAX_BOOT_EVIDENCE_BYTES: u64 = 64 * 1024;

/// Global flag set by signal handler.
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// Time to sleep before the frame deadline, leaving a small scheduler-jitter margin.
/// None means the frame deadline has arrived or passed.
fn frame_wait_before_deadline(
    elapsed: Duration,
    frame_interval: Duration,
    scheduling_margin: Duration,
) -> Option<Duration> {
    if elapsed >= frame_interval {
        return None;
    }
    Some((frame_interval - elapsed).saturating_sub(scheduling_margin))
}

fn main() {
    let args = parse_args();

    // The same manifest owns theme availability on each surface. Reject a desktop-only
    // theme before reading boot evidence, opening DRM, or taking any display resources.
    if !args.theme.boot_allowed() {
        eprintln!(
            "quicken-fb: theme '{}' is not permitted on the boot surface",
            args.theme.id(),
        );
        std::process::exit(2);
    }

    // This process is decorative, so unavailable state evidence skips the
    // animation rather than holding up boot. Contents are deliberately not read.
    match (&args.receipt_path, &args.lineage_path) {
        (Some(receipt), Some(lineage)) => {
            if !validate_state_artifact(receipt) || !validate_state_artifact(lineage) {
                eprintln!("quicken-fb: boot evidence unavailable; skipping decorative renderer");
                return;
            }
        }
        (None, None) => {}
        _ => {
            eprintln!("quicken-fb: receipt and lineage must be supplied together; skipping renderer");
            return;
        }
    }

    // Install signal handlers
    install_signal_handlers();

    // Open DRM framebuffer
    let framebuffer_result = if args.device == "auto" {
        DrmFramebuffer::open_auto()
    } else {
        DrmFramebuffer::open(&args.device).map(|framebuffer| (framebuffer, args.device.clone()))
    };
    let (mut fb, active_device) = match framebuffer_result {
        Ok(result) => result,
        Err(error) => {
            eprintln!("quicken-fb: failed to open framebuffer: {error}");
            std::process::exit(1);
        }
    };

    eprintln!(
        "quicken-fb: display {}x{} @ {}Hz on {}",
        fb.width,
        fb.height,
        fb.mode.vrefresh(),
        active_device,
    );

    // Create mycelial network
    let mut network =
        MycelialNetwork::new_with_theme(fb.width, fb.height, &args.visual_seed, args.theme);
    network.set_motion_scale(args.motion.scale());
    network.set_scene_entry_fade_duration(args.motion.crossfade_ms() as f32 / 1_000.0);
    network.set_particle_budget_scale(args.motion.particle_budget_scale());
    let (contraction_duration, convergence_hold, fade_duration) =
        args.motion.transition_durations();

    // Create progress monitor
    let mut progress = ProgressMonitor::new(args.progress_pipe.as_deref());

    // Allocate render buffer (row-major, no stride padding)
    let mut render_buf = vec![0u32; fb.pixel_count];

    let frame_duration = Duration::from_millis(args.motion.frame_interval_ms());
    // SIGTERM handoff must stay below systemd's 750 ms stop deadline even when
    // low-power/reduced modes use a slower regular render cadence.
    let shutdown_frame_duration = Duration::from_nanos(1_000_000_000 / TARGET_FPS as u64);
    let start_time = Instant::now();
    let mut last_frame = Instant::now();

    // Animation state
    let mut completing = false;
    let mut contraction_start: Option<Instant> = None;

    // Main animation loop
    loop {
        let now = Instant::now();
        let _elapsed_total = now.duration_since(start_time).as_secs_f32();

        // Check for shutdown signal
        if SHUTDOWN.load(Ordering::Relaxed) {
            break;
        }

        // A compositor handoff marker is a second, graceful stop signal.
        // systemd SIGTERM remains the enforcement path if this process is wedged.
        if !completing {
            if let Some(path) = args.handoff_path.as_deref() {
                if Path::new(path).exists() {
                    completing = true;
                    contraction_start = Some(now);
                    if args.motion != MotionMode::Reduced {
                        network.pulse();
                    }
                }
            }
        }

        // Use the regular motion cadence during the scene, but use a bounded
        // 30 FPS cadence for the exit transition so even Reduced mode gets a
        // real multi-frame fade instead of skipping a 250 ms fade in one step.
        let current_frame_duration = if completing {
            shutdown_frame_duration
        } else {
            frame_duration
        };
        let elapsed_since_frame = now.duration_since(last_frame);
        let scheduling_margin = Duration::from_millis(1);
        if let Some(wait) = frame_wait_before_deadline(
            elapsed_since_frame,
            current_frame_duration,
            scheduling_margin,
        ) {
            // Sleep close to the frame deadline instead of waking every 500 us.
            // Recheck signals and actual elapsed time before rendering.
            if wait.is_zero() {
                std::thread::yield_now();
            } else {
                std::thread::sleep(wait);
            }
            continue;
        }
        let dt = elapsed_since_frame.as_secs_f32();
        last_frame = now;

        // Poll progress events
        let events = progress.poll();
        for event in &events {
            match event {
                ProgressEvent::DerivationComplete(name) => {
                    if args.motion != MotionMode::Reduced {
                        network.pulse();
                    }
                    eprintln!("quicken-fb: derivation complete: {name}");
                }
                ProgressEvent::PhaseChange(phase) => {
                    eprintln!("quicken-fb: phase: {phase}");
                }
                ProgressEvent::InstallComplete => {
                    if !completing {
                        completing = true;
                        contraction_start = Some(now);
                        if args.motion != MotionMode::Reduced {
                            network.pulse();
                        }
                        eprintln!("quicken-fb: installation complete — beginning final animation");
                    }
                }
                _ => {}
            }
        }

        // Handle final contraction animation
        if completing {
            if let Some(cs) = contraction_start {
                let t = now.duration_since(cs).as_secs_f32();
                if t < contraction_duration {
                    // Contraction phase: all threads pull toward center.
                    if contraction_duration > 0.0 {
                        network.contract(t / contraction_duration);
                    }
                    if args.motion != MotionMode::Reduced && (t * 4.0) as u32 % 2 == 0 {
                        network.pulse();
                    }
                } else if t < contraction_duration + convergence_hold {
                    // A short convergence hold; this is theme-colored, never a white flash.
                    if args.motion != MotionMode::Reduced {
                        network.contract(1.0);
                        network.fade_out(0.0);
                    }
                } else if t < contraction_duration + convergence_hold + fade_duration {
                    // Fade every pixel continuously to black, then release the display.
                    if args.motion != MotionMode::Reduced {
                        network.contract(1.0);
                    }
                    network.fade_out(
                        (t - contraction_duration - convergence_hold) / fade_duration
                    );
                } else {
                    // Done — exit
                    break;
                }
            }
        }

        // Grow the network
        let io_rate = progress.io_rate;
        network.grow(dt, io_rate);

        // Render
        network.render(&mut render_buf);

        // A failed mapping/copy must not turn the decorative animation into a
        // CPU-consuming loop that can no longer update the display.
        if !fb.blit_from(&render_buf) {
            eprintln!("quicken-fb: framebuffer write failed; stopping decorative renderer");
            break;
        }
    }

    // SIGTERM/SIGINT may be the compositor handoff path. Fade out over a
    // bounded number of frames instead of clearing the display abruptly.
    if SHUTDOWN.load(Ordering::Relaxed) {
        const SHUTDOWN_FADE_FRAMES: u32 = 6;
        let initial_fade = network.exit_fade.clamp(0.0, 1.0);
        for frame in 1..=SHUTDOWN_FADE_FRAMES {
            let t = frame as f32 / SHUTDOWN_FADE_FRAMES as f32;
            network.fade_out(initial_fade + (1.0 - initial_fade) * t);
            network.render(&mut render_buf);
            if !fb.blit_from(&render_buf) {
                eprintln!("quicken-fb: framebuffer write failed during shutdown fade");
                break;
            }
            std::thread::sleep(shutdown_frame_duration);
        }
    }

    // Clean exit — clear to black after the fade or normal completion.
    for pixel in render_buf.iter_mut() {
        *pixel = 0;
    }
    let _ = fb.blit_from(&render_buf);

    eprintln!("quicken-fb: clean exit");
}

/// Parsed command-line arguments.
struct Args {
    visual_seed: String,
    progress_pipe: Option<String>,
    device: String,
    theme: Theme,
    motion: MotionMode,
    receipt_path: Option<String>,
    lineage_path: Option<String>,
    handoff_path: Option<String>,
}

/// Minimal argument parser to keep the early-boot binary small and dependency-free.
fn parse_args() -> Args {
    let args: Vec<String> = std::env::args().collect();
    let mut visual_seed = "symthaea-visual-seed-v1".to_string();
    let mut progress_pipe = None;
    let mut device = "/dev/dri/card0".to_string();
    let mut theme = Theme::MycelialGenesis;
    let mut motion = MotionMode::Cinematic;
    let mut receipt_path = None;
    let mut lineage_path = None;
    let mut handoff_path = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--seed" | "--genesis-phrase" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("quicken-fb: --seed requires a value");
                    std::process::exit(2);
                }
                visual_seed = args[i].clone();
            }
            "--theme" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("quicken-fb: --theme requires a stable theme ID");
                    print_usage();
                    std::process::exit(2);
                }
                let id = &args[i];
                theme = match Theme::from_id(id) {
                    Some(theme) => theme,
                    None => {
                        eprintln!("quicken-fb: unknown theme ID: {id}");
                        print_usage();
                        std::process::exit(2);
                    }
                };
            }
            "--list-themes" => {
                for theme in Theme::ALL {
                    println!("{}", theme.id());
                }
                std::process::exit(0);
            }
            "--list-theme-scenes" => {
                for theme in Theme::ALL {
                    println!("{} {}", theme.id(), theme.scene_kind().id());
                }
                std::process::exit(0);
            }
            "--list-theme-contracts" => {
                for theme in Theme::ALL {
                    let palette = theme.palette();
                    let growth = theme.growth_profile();
                    let (max_branches, max_particles) = theme.density_caps();
                    let (growth_motion, pulse_motion, parallax_motion) = theme.motion_tuning_values();
                    println!(
                        concat!(
                            "{}\t{}\t",
                            "#{:02x}{:02x}{:02x}\t",
                            "#{:02x}{:02x}{:02x}\t",
                            "#{:02x}{:02x}{:02x}\t",
                            "#{:02x}{:02x}{:02x}\t",
                            "#{:02x}{:02x}{:02x}\t",
                            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}"
                        ),
                        theme.id(),
                        theme.scene_kind().id(),
                        palette.background.0, palette.background.1, palette.background.2,
                        palette.primary.0, palette.primary.1, palette.primary.2,
                        palette.secondary.0, palette.secondary.1, palette.secondary.2,
                        palette.node.0, palette.node.1, palette.node.2,
                        palette.convergence.0, palette.convergence.1, palette.convergence.2,
                        growth.initial_branch_min,
                        growth.initial_branch_variants,
                        growth.branch_length_scale,
                        growth.growth_speed,
                        growth.min_fork_angle_degrees,
                        growth.max_fork_angle_degrees,
                        growth.min_children,
                        growth.max_children,
                        growth.pulse_gain,
                        max_branches,
                        max_particles,
                        growth_motion,
                        pulse_motion,
                        parallax_motion,
                        theme.supports_surface(ThemeSurface::Boot),
                        theme.supports_surface(ThemeSurface::Desktop),
                        theme.supports_surface(ThemeSurface::LockScreen)
                    );
                }
                std::process::exit(0);
            }
            "--list-motion-modes" => {
                for mode in MotionMode::ALL {
                    let (contraction, hold, fade) = mode.transition_durations();
                    println!(
                        "{} {} {} {} {} {} {} {}",
                        mode.id(),
                        mode.scale(),
                        mode.particle_budget_scale(),
                        mode.frame_interval_ms(),
                        contraction,
                        hold,
                        fade,
                        mode.crossfade_ms()
                    );
                }
                std::process::exit(0);
            }
            "--motion" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("quicken-fb: --motion requires reduced, low-power, balanced, or cinematic");
                    print_usage();
                    std::process::exit(2);
                }
                let id = &args[i];
                motion = match MotionMode::from_id(id) {
                    Some(mode) => mode,
                    None => {
                        eprintln!("quicken-fb: unknown motion mode: {id}");
                        print_usage();
                        std::process::exit(2);
                    }
                };
            }
            "--progress-pipe" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("quicken-fb: --progress-pipe requires a path");
                    std::process::exit(2);
                }
                progress_pipe = Some(args[i].clone());
            }
            "--device" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("quicken-fb: --device requires a path");
                    std::process::exit(2);
                }
                device = args[i].clone();
            }
            "--receipt" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("quicken-fb: --receipt requires a path");
                    std::process::exit(2);
                }
                receipt_path = Some(args[i].clone());
            }
            "--lineage" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("quicken-fb: --lineage requires a path");
                    std::process::exit(2);
                }
                lineage_path = Some(args[i].clone());
            }
            "--handoff-path" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("quicken-fb: --handoff-path requires a path");
                    std::process::exit(2);
                }
                handoff_path = Some(args[i].clone());
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other => {
                eprintln!("quicken-fb: unknown argument: {other}");
                print_usage();
                std::process::exit(2);
            }
        }
        i += 1;
    }

    // Bound command-line input. The seed is for visible variation only, never a secret.
    if visual_seed.is_empty() || visual_seed.len() > 256 {
        eprintln!("quicken-fb: --seed must contain 1..=256 bytes");
        std::process::exit(2);
    }

    if receipt_path.is_some() != lineage_path.is_some() {
        eprintln!("quicken-fb: --receipt and --lineage must be supplied together");
        std::process::exit(2);
    }

    Args {
        visual_seed,
        progress_pipe,
        device,
        theme,
        motion,
        receipt_path,
        lineage_path,
        handoff_path,
    }
}

/// Require a nonempty bounded regular file without reading its contents.
fn validate_state_artifact(path: &str) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    metadata.file_type().is_file()
        && metadata.len() > 0
        && metadata.len() <= MAX_BOOT_EVIDENCE_BYTES
}

fn print_usage() {
    eprintln!("Usage: quicken-fb [OPTIONS]\n  --theme <ID>             Theme ID (default: mycelial-genesis)\n  --list-themes            List supported theme IDs and exit\n  --list-theme-scenes     List theme-to-scene mappings\n  --list-theme-contracts   Print palettes, budgets and surface permissions as TSV\n  --list-motion-modes    List motion budgets and transition timings\n  --motion <MODE>          reduced|low-power|balanced|cinematic\n  --seed <VALUE>           Public visual seed, not a secret (max 256 bytes)\n  --genesis-phrase <VALUE> Legacy alias for --seed\n  --receipt <PATH>         Optional boot-state artifact (presence only)\n  --lineage <PATH>         Optional lineage artifact (presence only)\n  --handoff-path <PATH>    Exit when the compositor marker appears\n  --progress-pipe <PATH>  Optional installer progress FIFO\n  --device <PATH>         DRM device (default: /dev/dri/card0)\n  --help                  Show this help");
}

/// Install SIGTERM and SIGINT handlers for clean shutdown.
fn install_signal_handlers() {
    // Use a simple atomic flag approach — safe for signal handlers
    unsafe {
        nix::libc::signal(
            nix::libc::SIGTERM,
            signal_handler as nix::libc::sighandler_t,
        );
        nix::libc::signal(nix::libc::SIGINT, signal_handler as nix::libc::sighandler_t);
    }
}

extern "C" fn signal_handler(_sig: std::ffi::c_int) {
    SHUTDOWN.store(true, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn frame_wait_sleeps_to_within_margin_of_deadline() {
        assert_eq!(
            frame_wait_before_deadline(
                Duration::from_millis(10),
                Duration::from_millis(33),
                Duration::from_millis(1),
            ),
            Some(Duration::from_millis(22)),
        );
    }

    #[test]
    fn frame_wait_yields_near_deadline_and_stops_waiting_after_deadline() {
        assert_eq!(
            frame_wait_before_deadline(
                Duration::from_millis(32),
                Duration::from_millis(33),
                Duration::from_millis(1),
            ),
            Some(Duration::ZERO),
        );
        assert_eq!(
            frame_wait_before_deadline(
                Duration::from_millis(33),
                Duration::from_millis(33),
                Duration::from_millis(1),
            ),
            None,
        );
        assert_eq!(
            frame_wait_before_deadline(
                Duration::from_millis(250),
                Duration::from_millis(67),
                Duration::from_millis(1),
            ),
            None,
        );
    }

    #[test]
    fn boot_state_artifact_check_rejects_symlinks_and_oversized_files() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "quicken-state-artifact-{}-{nonce}",
            std::process::id()
        ));
        let valid = base.with_extension("json");
        let oversized = base.with_extension("large");
        let link = base.with_extension("link");

        fs::write(&valid, b"{\"schema\":\"test\"}").expect("write valid artifact");
        assert!(validate_state_artifact(valid.to_str().expect("UTF-8 temp path")));

        fs::write(
            &oversized,
            vec![b'x'; MAX_BOOT_EVIDENCE_BYTES as usize + 1],
        )
        .expect("write oversized artifact");
        assert!(!validate_state_artifact(
            oversized.to_str().expect("UTF-8 temp path")
        ));

        symlink(&valid, &link).expect("create symlink");
        assert!(!validate_state_artifact(
            link.to_str().expect("UTF-8 temp path")
        ));

        let _ = fs::remove_file(valid);
        let _ = fs::remove_file(oversized);
        let _ = fs::remove_file(link);
    }
}
