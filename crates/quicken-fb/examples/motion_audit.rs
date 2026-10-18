// Copyright (C) 2024-2026 Tristan Stoltz / Luminous Dynamics
// SPDX-License-Identifier: AGPL-3.0-or-later
// Commercial licensing: see COMMERCIAL_LICENSE.md at repository root

//! Deterministic temporal audit of the CPU reference renderer.
//!
//! This reports per-transition luminance and channel-change diagnostics. The
//! wide-area threshold is a conservative engineering heuristic, not a WCAG
//! flash-conformance algorithm or a perceptual-safety guarantee.

use blake3::Hasher;
use symthaea_quicken_fb::mycelium::MycelialNetwork;
use symthaea_quicken_fb::theme::{MotionMode, Theme};

const DEFAULT_WIDTH: u32 = 160;
const DEFAULT_HEIGHT: u32 = 90;
const DEFAULT_FRAMES: u32 = 60;
const MAX_WIDTH: u32 = 640;
const MAX_HEIGHT: u32 = 360;
const MAX_FRAMES: u32 = 1_800;
const FRAME_INTERVAL_SECONDS: f32 = 1.0 / 30.0;
const CHANGED_CHANNEL_DELTA: u32 = 3;
const LARGE_CHANNEL_DELTA: u32 = 80;
const LARGE_RELATIVE_LUMINANCE_DELTA: f64 = 0.10;
const LARGE_AREA_RATIO: f64 = 0.25;

#[derive(Clone, Copy, Debug, PartialEq)]
struct TransitionMetrics {
    frame: u32,
    mean_luminance_delta: f64,
    max_pixel_luminance_delta: f64,
    changed_pixel_ratio: f64,
    high_change_pixel_ratio: f64,
    large_area_high_change: bool,
}

#[derive(Debug)]
struct AuditSummary {
    sequence_blake3: String,
    motion_observed: bool,
    zero_change_transitions: usize,
    large_area_high_change_transitions: usize,
    max_mean_luminance_delta: f64,
    max_pixel_luminance_delta: f64,
    max_changed_pixel_ratio: f64,
    max_high_change_pixel_ratio: f64,
}

fn main() {
    let (width, height, frames, theme, motion) = match parse_args() {
        Ok(Some(args)) => args,
        Ok(None) => return,
        Err(error) => {
            eprintln!("motion_audit: {error}");
            print_usage();
            std::process::exit(2);
        }
    };

    if width == 0 || height == 0 || width > MAX_WIDTH || height > MAX_HEIGHT {
        eprintln!(
            "motion_audit: resolution must be within 1..={MAX_WIDTH} by 1..={MAX_HEIGHT}"
        );
        std::process::exit(2);
    }
    if frames < 2 || frames > MAX_FRAMES {
        eprintln!("motion_audit: frames must be within 2..={MAX_FRAMES}");
        std::process::exit(2);
    }

    let mut scene = MycelialNetwork::new_with_theme(
        width,
        height,
        "sovereign-motion-audit-v1",
        theme,
    );
    scene.set_motion_scale(motion.scale());
    scene.set_scene_entry_fade_duration(motion.crossfade_ms() as f32 / 1_000.0);
    scene.set_particle_budget_scale(motion.particle_budget_scale());
    if motion == MotionMode::Reduced {
        // Use the established timepoint for dedicated static compositions.
        scene.elapsed = 8.0;
    }

    let pixel_count = (width as usize) * (height as usize);
    let mut current = vec![0u32; pixel_count];
    let mut previous = vec![0u32; pixel_count];
    let mut sequence_hasher = Hasher::new();
    let mut transitions = Vec::with_capacity((frames - 1) as usize);
    let mut summary = AuditSummary {
        sequence_blake3: String::new(),
        motion_observed: false,
        zero_change_transitions: 0,
        large_area_high_change_transitions: 0,
        max_mean_luminance_delta: 0.0,
        max_pixel_luminance_delta: 0.0,
        max_changed_pixel_ratio: 0.0,
        max_high_change_pixel_ratio: 0.0,
    };

    for frame in 0..frames {
        scene.grow(FRAME_INTERVAL_SECONDS, 0.75);
        scene.render(&mut current);
        hash_frame_rgb(&mut sequence_hasher, &current);

        if frame > 0 {
            let metrics = analyze_transition(frame + 1, &previous, &current);
            summary.motion_observed |= metrics.changed_pixel_ratio > 0.0;
            if metrics.changed_pixel_ratio == 0.0 {
                summary.zero_change_transitions += 1;
            }
            if metrics.large_area_high_change {
                summary.large_area_high_change_transitions += 1;
            }
            summary.max_mean_luminance_delta = summary
                .max_mean_luminance_delta
                .max(metrics.mean_luminance_delta);
            summary.max_pixel_luminance_delta = summary
                .max_pixel_luminance_delta
                .max(metrics.max_pixel_luminance_delta);
            summary.max_changed_pixel_ratio = summary
                .max_changed_pixel_ratio
                .max(metrics.changed_pixel_ratio);
            summary.max_high_change_pixel_ratio = summary
                .max_high_change_pixel_ratio
                .max(metrics.high_change_pixel_ratio);
            transitions.push(metrics);
        }

        std::mem::swap(&mut current, &mut previous);
    }

    summary.sequence_blake3 = sequence_hasher.finalize().to_hex().to_string();
    println!(
        concat!(
            "{{\n",
            "  \"schema\": \"motion-audit-v1\",\n",
            "  \"source_commit\": \"{}\",\n",
            "  \"build_commit\": \"{}\",\n",
            "  \"measurement\": \"cpu_reference_frame_sequence\",\n",
            "  \"theme\": \"{}\",\n",
            "  \"motion_mode\": \"{}\",\n",
            "  \"resolution\": {{ \"width\": {}, \"height\": {} }},\n",
            "  \"frames_requested\": {},\n",
            "  \"sample_interval_seconds\": {:.6},\n",
            "  \"frame_transitions\": {},\n",
            "  \"sequence_blake3\": \"{}\",\n",
            "  \"motion_observed\": {},\n",
            "  \"zero_change_transitions\": {},\n",
            "  \"large_area_high_change_transitions\": {},\n",
            "  \"max_mean_luminance_delta\": {:.6},\n",
            "  \"max_pixel_luminance_delta\": {:.6},\n",
            "  \"max_changed_pixel_ratio\": {:.6},\n",
            "  \"max_high_change_pixel_ratio\": {:.6},\n",
            "  \"thresholds\": {{ \"changed_channel_delta_gt\": {}, \"large_channel_delta_ge\": {}, \"large_relative_luminance_delta_ge\": {:.3}, \"large_area_ratio_ge\": {:.2} }},\n",
            "  \"normative_flash_conformance\": false,\n",
            "  \"transitions\": ["
        ),
        option_env!("VISUAL_SOURCE_COMMIT").unwrap_or("unavailable-local-build"),
        option_env!("VISUAL_BUILD_COMMIT").unwrap_or("unavailable-local-build"),
        theme.id(),
        motion.id(),
        width,
        height,
        frames,
        FRAME_INTERVAL_SECONDS,
        transitions.len(),
        summary.sequence_blake3,
        summary.motion_observed,
        summary.zero_change_transitions,
        summary.large_area_high_change_transitions,
        summary.max_mean_luminance_delta,
        summary.max_pixel_luminance_delta,
        summary.max_changed_pixel_ratio,
        summary.max_high_change_pixel_ratio,
        CHANGED_CHANNEL_DELTA,
        LARGE_CHANNEL_DELTA,
        LARGE_RELATIVE_LUMINANCE_DELTA,
        LARGE_AREA_RATIO,
    );
    for (index, transition) in transitions.iter().enumerate() {
        if index > 0 {
            print!(",");
        }
        print!(
            "{{\"frame\":{},\"mean_luminance_delta\":{:.6},\"max_pixel_luminance_delta\":{:.6},\"changed_pixel_ratio\":{:.6},\"high_change_pixel_ratio\":{:.6},\"large_area_high_change\":{}}}",
            transition.frame,
            transition.mean_luminance_delta,
            transition.max_pixel_luminance_delta,
            transition.changed_pixel_ratio,
            transition.high_change_pixel_ratio,
            transition.large_area_high_change,
        );
    }
    println!("]\n}}");
}

fn srgb_to_linear(value: f64) -> f64 {
    let value = value / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// Approximate sRGB relative luminance, normalized to 0..=1.
fn relative_luminance(pixel: u32) -> f64 {
    let red = srgb_to_linear(((pixel >> 16) & 0xff) as f64);
    let green = srgb_to_linear(((pixel >> 8) & 0xff) as f64);
    let blue = srgb_to_linear((pixel & 0xff) as f64);
    0.2126 * red + 0.7152 * green + 0.0722 * blue
}

fn analyze_transition(frame: u32, before: &[u32], after: &[u32]) -> TransitionMetrics {
    assert_eq!(before.len(), after.len(), "frame dimensions must match");
    assert!(!before.is_empty(), "frames must contain pixels");

    let mut luminance_delta_sum = 0.0;
    let mut max_luminance_delta: f64 = 0.0;
    let mut changed_pixels = 0usize;
    let mut high_change_pixels = 0usize;

    for (&old, &new) in before.iter().zip(after) {
        let old_luminance = relative_luminance(old);
        let new_luminance = relative_luminance(new);
        let luminance_delta = (new_luminance - old_luminance).abs();
        luminance_delta_sum += luminance_delta;
        max_luminance_delta = max_luminance_delta.max(luminance_delta);

        let red_delta = ((new >> 16) & 0xff).abs_diff((old >> 16) & 0xff);
        let green_delta = ((new >> 8) & 0xff).abs_diff((old >> 8) & 0xff);
        let blue_delta = (new & 0xff).abs_diff(old & 0xff);
        let max_channel_delta = red_delta.max(green_delta).max(blue_delta);
        if max_channel_delta > CHANGED_CHANNEL_DELTA {
            changed_pixels += 1;
        }

        let darker_luminance = old_luminance.min(new_luminance);
        if (luminance_delta >= LARGE_RELATIVE_LUMINANCE_DELTA && darker_luminance < 0.80)
            || max_channel_delta >= LARGE_CHANNEL_DELTA
        {
            high_change_pixels += 1;
        }
    }

    let total = before.len() as f64;
    let changed_pixel_ratio = changed_pixels as f64 / total;
    let high_change_pixel_ratio = high_change_pixels as f64 / total;

    TransitionMetrics {
        frame,
        mean_luminance_delta: luminance_delta_sum / total,
        max_pixel_luminance_delta: max_luminance_delta,
        changed_pixel_ratio,
        high_change_pixel_ratio,
        large_area_high_change: high_change_pixel_ratio >= LARGE_AREA_RATIO,
    }
}

fn hash_frame_rgb(hasher: &mut Hasher, pixels: &[u32]) {
    for pixel in pixels {
        hasher.update(&[
            ((pixel >> 16) & 0xff) as u8,
            ((pixel >> 8) & 0xff) as u8,
            (pixel & 0xff) as u8,
        ]);
    }
}

fn parse_args() -> Result<Option<(u32, u32, u32, Theme, MotionMode)>, String> {
    let args = std::env::args().collect::<Vec<_>>();
    parse_args_from(&args)
}

fn parse_args_from(args: &[String]) -> Result<Option<(u32, u32, u32, Theme, MotionMode)>, String> {
    let mut width = DEFAULT_WIDTH;
    let mut height = DEFAULT_HEIGHT;
    let mut frames = DEFAULT_FRAMES;
    let mut theme = Theme::MycelialGenesis;
    let mut motion = MotionMode::Cinematic;
    let mut index = 1;

    while index < args.len() {
        match args[index].as_str() {
            "--width" => {
                index += 1;
                width = parse_value(args, index, "--width")?;
            }
            "--height" => {
                index += 1;
                height = parse_value(args, index, "--height")?;
            }
            "--frames" => {
                index += 1;
                frames = parse_value(args, index, "--frames")?;
            }
            "--theme" => {
                index += 1;
                let value = args.get(index).ok_or("--theme requires a stable theme ID")?;
                theme = Theme::from_id(value).ok_or_else(|| format!("unknown theme ID: {value}"))?;
            }
            "--motion" => {
                index += 1;
                let value = args.get(index).ok_or("--motion requires a stable motion-mode ID")?;
                motion = MotionMode::from_id(value)
                    .ok_or_else(|| format!("unknown motion mode: {value}"))?;
            }
            "--help" | "-h" => {
                print_usage();
                return Ok(None);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
        index += 1;
    }
    Ok(Some((width, height, frames, theme, motion)))
}

fn parse_value(args: &[String], index: usize, name: &str) -> Result<u32, String> {
    args.get(index)
        .ok_or_else(|| format!("{name} requires a value"))?
        .parse::<u32>()
        .map_err(|_| format!("{name} must be a positive integer"))
}

fn print_usage() {
    eprintln!(
        "Usage: cargo run --release --example motion_audit -- [OPTIONS]\n  --theme <ID>   Theme ID (default: mycelial-genesis)\n  --motion <ID>  reduced|low-power|balanced|cinematic (default: cinematic)\n  --frames <N>   Frames, 2..=1800 (default: 60)\n  --width <N>    Width, up to 640 (default: 160)\n  --height <N>   Height, up to 360 (default: 90)\n  --help         Show this help"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdr_relative_luminance_maps_black_and_white_to_unit_interval_endpoints() {
        assert_eq!(relative_luminance(0x0000_0000), 0.0);
        assert!((relative_luminance(0x00ff_ffff) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn identical_frames_have_no_motion_or_large_change() {
        let frame = [0x0020_3040; 4];
        let metrics = analyze_transition(2, &frame, &frame);
        assert_eq!(metrics.changed_pixel_ratio, 0.0);
        assert_eq!(metrics.high_change_pixel_ratio, 0.0);
        assert!(!metrics.large_area_high_change);
    }

    #[test]
    fn full_frame_black_to_white_is_flagged_as_large_area_change() {
        let before = [0x0000_0000; 100];
        let after = [0x00ff_ffff; 100];
        let metrics = analyze_transition(2, &before, &after);
        assert_eq!(metrics.high_change_pixel_ratio, 1.0);
        assert!(metrics.large_area_high_change);
    }

    #[test]
    fn isolated_change_does_not_claim_large_area_change() {
        let before = [0x0000_0000; 100];
        let mut after = before;
        after[42] = 0x00ff_ffff;
        let metrics = analyze_transition(2, &before, &after);
        assert!(!metrics.large_area_high_change);
        assert_eq!(metrics.high_change_pixel_ratio, 0.01);
    }

    #[test]
    fn parser_accepts_theme_motion_and_bounds() {
        let args = vec![
            "motion_audit".to_string(),
            "--theme".to_string(),
            "cosmic-atlas".to_string(),
            "--motion".to_string(),
            "reduced".to_string(),
            "--frames".to_string(),
            "61".to_string(),
        ];
        let parsed = parse_args_from(&args)
            .expect("valid args")
            .expect("not help");
        assert_eq!(parsed, (160, 90, 61, Theme::CosmicAtlas, MotionMode::Reduced));
    }

    #[test]
    fn parser_rejects_unknown_mode() {
        let args = vec![
            "motion_audit".to_string(),
            "--motion".to_string(),
            "unknown".to_string(),
        ];
        assert!(parse_args_from(&args).is_err());
    }
}
