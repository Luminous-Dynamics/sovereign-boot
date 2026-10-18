// Copyright (C) 2024-2026 Tristan Stoltz / Luminous Dynamics
// SPDX-License-Identifier: AGPL-3.0-or-later
// Commercial licensing: see COMMERCIAL_LICENSE.md at repository root

//! Frame-triplet screen for opposing luminance changes and saturated-red transitions.
//!
//! This is a deterministic regression proxy. It does not implement the complete
//! WCAG algorithm (visual-field geometry, sliding one-second flash counting,
//! chromaticity edge cases, or physical-display capture).

use symthaea_quicken_fb::mycelium::MycelialNetwork;
use symthaea_quicken_fb::theme::{MotionMode, Theme};

const DEFAULT_WIDTH: u32 = 160;
const DEFAULT_HEIGHT: u32 = 90;
const DEFAULT_FRAMES: u32 = 60;
const MAX_WIDTH: u32 = 640;
const MAX_HEIGHT: u32 = 360;
const MAX_FRAMES: u32 = 1800;
const FRAME_INTERVAL_SECONDS: f32 = 1.0 / 30.0;
// At 30 Hz, 31 sample timestamps span one second including both endpoints.
const ONE_SECOND_WINDOW_SAMPLES: usize = 31;
const MAX_FLASH_EVENTS_PER_SECOND: usize = 3;
const GENERAL_LUMINANCE_DELTA: f64 = 0.10;
const RED_CHROMATICITY_DELTA: f64 = 0.20;
const LARGE_AREA_RATIO: f64 = 0.25;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct PairMetrics {
    general_flash_area_ratio: f64,
    red_flash_area_ratio: f64,
}

#[derive(Debug, Default)]
struct Summary {
    pair_count: usize,
    large_area_general_flash_pairs: usize,
    large_area_red_flash_pairs: usize,
    max_general_flash_area_ratio: f64,
    max_red_flash_area_ratio: f64,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct WindowSummary {
    windows_examined: usize,
    max_events: usize,
    windows_over_three_events: usize,
}

fn main() {
    let (width, height, frames, theme, motion) = match parse_args() {
        Ok(Some(args)) => args,
        Ok(None) => return,
        Err(error) => {
            eprintln!("opposing_transition_audit: {error}");
            print_usage();
            std::process::exit(2);
        }
    };
    if width == 0 || height == 0 || width > MAX_WIDTH || height > MAX_HEIGHT {
        eprintln!("opposing_transition_audit: resolution is outside the bounded range");
        std::process::exit(2);
    }
    if frames < 3 || frames > MAX_FRAMES {
        eprintln!("opposing_transition_audit: frames must be within 3..=1800");
        std::process::exit(2);
    }

    let mut scene = MycelialNetwork::new_with_theme(
        width,
        height,
        "sovereign-opposing-transition-audit-v1",
        theme,
    );
    scene.set_motion_scale(motion.scale());
    scene.set_scene_entry_fade_duration(motion.crossfade_ms() as f32 / 1_000.0);
    scene.set_particle_budget_scale(motion.particle_budget_scale());
    if motion == MotionMode::Reduced {
        scene.elapsed = 8.0;
    }

    let pixels = width as usize * height as usize;
    let mut current = vec![0u32; pixels];
    let mut previous = vec![0u32; pixels];
    let mut previous_previous = vec![0u32; pixels];
    let mut summary = Summary::default();
    let mut general_flash_events = Vec::with_capacity((frames - 2) as usize);
    let mut red_flash_events = Vec::with_capacity((frames - 2) as usize);

    for frame in 0..frames {
        scene.grow(FRAME_INTERVAL_SECONDS, 0.75);
        scene.render(&mut current);

        if frame >= 2 {
            let pair = analyze_pair(&previous_previous, &previous, &current);
            summary.pair_count += 1;
            summary.max_general_flash_area_ratio = summary
                .max_general_flash_area_ratio
                .max(pair.general_flash_area_ratio);
            summary.max_red_flash_area_ratio = summary
                .max_red_flash_area_ratio
                .max(pair.red_flash_area_ratio);

            let general_area_event = pair.general_flash_area_ratio >= LARGE_AREA_RATIO;
            let red_area_event = pair.red_flash_area_ratio >= LARGE_AREA_RATIO;
            general_flash_events.push(general_area_event);
            red_flash_events.push(red_area_event);
            if general_area_event {
                summary.large_area_general_flash_pairs += 1;
            }
            if red_area_event {
                summary.large_area_red_flash_pairs += 1;
            }
        }

        // O(1) buffer rotation retains only the previous two frames.
        std::mem::swap(&mut previous_previous, &mut previous);
        std::mem::swap(&mut previous, &mut current);
    }

    let general_windows = summarize_one_second_windows(&general_flash_events);
    let red_windows = summarize_one_second_windows(&red_flash_events);

    println!(
        concat!(
            "{{\n",
            "  \"schema\": \"opposing-transition-audit-v1\",\n",
            "  \"source_commit\": \"{}\",\n",
            "  \"build_commit\": \"{}\",\n",
            "  \"measurement\": \"cpu_reference_frame_triplets\",\n",
            "  \"theme\": \"{}\",\n",
            "  \"motion_mode\": \"{}\",\n",
            "  \"resolution\": {{ \"width\": {}, \"height\": {} }},\n",
            "  \"frames_requested\": {},\n",
            "  \"frame_triplets_examined\": {},\n",
            "  \"max_general_flash_area_ratio\": {:.6},\n",
            "  \"max_red_flash_area_ratio\": {:.6},\n",
            "  \"large_area_general_flash_pairs\": {},\n",
            "  \"large_area_red_flash_pairs\": {},\n",
            "  \"one_second_window_summary\": {{ \"sample_points_per_window\": {}, \"windows_examined\": {}, \"max_general_large_area_flash_events\": {}, \"general_windows_over_three_flash_events\": {}, \"max_red_large_area_flash_events\": {}, \"red_windows_over_three_flash_events\": {} }},\n",
            "  \"thresholds\": {{ \"general_luminance_delta_ge\": {:.2}, \"red_chromaticity_delta_gt\": {:.2}, \"large_area_ratio_ge\": {:.2} }},\n",
            "  \"normative_wcag_conformance\": false\n",
            "}}"
        ),
        option_env!("VISUAL_SOURCE_COMMIT").unwrap_or("unavailable-local-build"),
        option_env!("VISUAL_BUILD_COMMIT").unwrap_or("unavailable-local-build"),
        theme.id(),
        motion.id(),
        width,
        height,
        frames,
        summary.pair_count,
        summary.max_general_flash_area_ratio,
        summary.max_red_flash_area_ratio,
        summary.large_area_general_flash_pairs,
        summary.large_area_red_flash_pairs,
        ONE_SECOND_WINDOW_SAMPLES,
        general_windows.windows_examined,
        general_windows.max_events,
        general_windows.windows_over_three_events,
        red_windows.max_events,
        red_windows.windows_over_three_events,
        GENERAL_LUMINANCE_DELTA,
        RED_CHROMATICITY_DELTA,
        LARGE_AREA_RATIO,
    );
}

fn srgb_to_linear(value: f64) -> f64 {
    let value = value / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn luminance(pixel: u32) -> f64 {
    let red = srgb_to_linear(((pixel >> 16) & 0xff) as f64);
    let green = srgb_to_linear(((pixel >> 8) & 0xff) as f64);
    let blue = srgb_to_linear((pixel & 0xff) as f64);
    0.2126 * red + 0.7152 * green + 0.0722 * blue
}

fn chromaticity_uv(pixel: u32) -> Option<(f64, f64)> {
    if near_black(pixel) {
        // Low-signal chromaticity is treated as indeterminate by this proxy.
        return None;
    }
    let red = srgb_to_linear(((pixel >> 16) & 0xff) as f64);
    let green = srgb_to_linear(((pixel >> 8) & 0xff) as f64);
    let blue = srgb_to_linear((pixel & 0xff) as f64);
    let x = 0.4124564 * red + 0.3575761 * green + 0.1804375 * blue;
    let y = 0.2126729 * red + 0.7151522 * green + 0.0721750 * blue;
    let z = 0.0193339 * red + 0.1191920 * green + 0.9503041 * blue;
    let denominator = x + 15.0 * y + 3.0 * z;
    if denominator <= 1e-12 {
        // CIE chromaticity is undefined for black.
        return None;
    }
    Some((4.0 * x / denominator, 9.0 * y / denominator))
}

fn red_fraction(pixel: u32) -> f64 {
    let red = ((pixel >> 16) & 0xff) as f64;
    let green = ((pixel >> 8) & 0xff) as f64;
    let blue = (pixel & 0xff) as f64;
    let total = red + green + blue;
    if total <= f64::EPSILON { 0.0 } else { red / total }
}

fn uv_distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

fn near_black(pixel: u32) -> bool {
    let red = (pixel >> 16) & 0xff;
    let green = (pixel >> 8) & 0xff;
    let blue = pixel & 0xff;
    red.max(green).max(blue) <= 1
}

fn saturated_red_transition(before: u32, after: u32) -> bool {
    let before_red = red_fraction(before) >= 0.80;
    let after_red = red_fraction(after) >= 0.80;
    if !before_red && !after_red {
        return false;
    }
    match (chromaticity_uv(before), chromaticity_uv(after)) {
        (Some(a), Some(b)) => uv_distance(a, b) > RED_CHROMATICITY_DELTA,
        // Exact/near black has undefined or numerically unstable chromaticity.
        // Treat a saturated-red transition to near-black as a conservative
        // alarm rather than pretending that black has a valid u'v' coordinate.
        (None, Some(_)) => near_black(before) && after_red,
        (Some(_), None) => before_red && near_black(after),
        (None, None) => false,
    }
}

fn analyze_pair(before: &[u32], middle: &[u32], after: &[u32]) -> PairMetrics {
    assert_eq!(before.len(), middle.len(), "frame dimensions must match");
    assert_eq!(middle.len(), after.len(), "frame dimensions must match");
    assert!(!before.is_empty(), "frames must contain pixels");

    let mut general_flash_pixels = 0usize;
    let mut red_flash_pixels = 0usize;
    for ((&first, &second), &third) in before.iter().zip(middle).zip(after) {
        let l0 = luminance(first);
        let l1 = luminance(second);
        let l2 = luminance(third);
        let d0 = l1 - l0;
        let d1 = l2 - l1;
        if d0 * d1 < 0.0
            && d0.abs() >= GENERAL_LUMINANCE_DELTA
            && d1.abs() >= GENERAL_LUMINANCE_DELTA
            && l0.min(l1) < 0.80
            && l1.min(l2) < 0.80
        {
            general_flash_pixels += 1;
        }

        let red_first = saturated_red_transition(first, second);
        let red_second = saturated_red_transition(second, third);
        if let (Some(uv0), Some(uv1), Some(uv2)) = (
            chromaticity_uv(first),
            chromaticity_uv(second),
            chromaticity_uv(third),
        ) {
            let first_vector = (uv1.0 - uv0.0, uv1.1 - uv0.1);
            let second_vector = (uv2.0 - uv1.0, uv2.1 - uv1.1);
            let opposing = first_vector.0 * second_vector.0
                + first_vector.1 * second_vector.1 < 0.0;
            if opposing && (red_first || red_second) {
                red_flash_pixels += 1;
            }
        } else if red_first && red_second {
            // With a black endpoint u'v' is undefined; two opposing red/black
            // transitions are still a conservative saturated-red alarm.
            red_flash_pixels += 1;
        }
    }

    let count = before.len() as f64;
    PairMetrics {
        general_flash_area_ratio: general_flash_pixels as f64 / count,
        red_flash_area_ratio: red_flash_pixels as f64 / count,
    }
}

fn summarize_one_second_windows(events: &[bool]) -> WindowSummary {
    if events.len() < ONE_SECOND_WINDOW_SAMPLES {
        return WindowSummary::default();
    }

    let mut result = WindowSummary::default();
    let mut event_count = 0usize;
    for end in 0..events.len() {
        if events[end] {
            event_count += 1;
        }
        if end >= ONE_SECOND_WINDOW_SAMPLES && events[end - ONE_SECOND_WINDOW_SAMPLES] {
            event_count -= 1;
        }
        if end + 1 >= ONE_SECOND_WINDOW_SAMPLES {
            result.windows_examined += 1;
            result.max_events = result.max_events.max(event_count);
            if event_count > MAX_FLASH_EVENTS_PER_SECOND {
                result.windows_over_three_events += 1;
            }
        }
    }
    result
}

fn parse_args() -> Result<Option<(u32, u32, u32, Theme, MotionMode)>, String> {
    parse_args_from(&std::env::args().collect::<Vec<_>>())
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
                theme = Theme::from_id(value).ok_or_else(|| format!("unknown theme: {value}"))?;
            }
            "--motion" => {
                index += 1;
                let value = args.get(index).ok_or("--motion requires a stable motion mode")?;
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
        "Usage: cargo run --release --example opposing_transition_audit -- [OPTIONS]\n  --theme <ID>   Stable theme ID\n  --motion <ID>  reduced|low-power|balanced|cinematic\n  --frames <N>   Frames, 3..=1800\n  --width <N>    Width, up to 640\n  --height <N>   Height, up to 360"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn general_flash_requires_two_opposing_luminance_transitions() {
        let black = [0x0000_0000; 100];
        let white = [0x00ff_ffff; 100];
        let flash = analyze_pair(&black, &white, &black);
        assert_eq!(flash.general_flash_area_ratio, 1.0);

        let monotonic = analyze_pair(&black, &white, &white);
        assert_eq!(monotonic.general_flash_area_ratio, 0.0);
    }

    #[test]
    fn saturated_red_screen_detects_white_red_white_pair() {
        let white = [0x00ff_ffff; 100];
        let red = [0x00ff_0000; 100];
        let flash = analyze_pair(&white, &red, &white);
        assert_eq!(flash.red_flash_area_ratio, 1.0);
    }

    #[test]
    fn saturated_red_black_pair_is_flagged_conservatively() {
        let black = [0x0000_0000; 100];
        let red = [0x00ff_0000; 100];
        let flash = analyze_pair(&black, &red, &black);
        assert_eq!(flash.red_flash_area_ratio, 1.0);
    }

    #[test]
    fn near_black_to_saturated_red_is_flagged_conservatively() {
        let near_black = [0x0001_0101; 100];
        let red = [0x00ff_0000; 100];
        let flash = analyze_pair(&near_black, &red, &near_black);
        assert_eq!(flash.red_flash_area_ratio, 1.0);
    }

    #[test]
    fn isolated_opposing_flash_is_below_large_area_gate() {
        let black = [0x0000_0000; 100];
        let mut middle = black;
        middle[42] = 0x00ff_ffff;
        let metrics = analyze_pair(&black, &middle, &black);
        assert_eq!(metrics.general_flash_area_ratio, 0.01);
        assert!(metrics.general_flash_area_ratio < LARGE_AREA_RATIO);
    }

    #[test]
    fn one_second_window_counts_three_candidate_events_without_alarm() {
        let mut events = vec![false; ONE_SECOND_WINDOW_SAMPLES];
        events[0] = true;
        events[10] = true;
        events[20] = true;

        let result = summarize_one_second_windows(&events);
        assert_eq!(result.windows_examined, 1);
        assert_eq!(result.max_events, 3);
        assert_eq!(result.windows_over_three_events, 0);
    }

    #[test]
    fn rolling_one_second_window_flags_four_candidate_events_and_slides() {
        let mut events = vec![false; ONE_SECOND_WINDOW_SAMPLES + 1];
        events[0] = true;
        events[10] = true;
        events[20] = true;
        events[30] = true;

        let result = summarize_one_second_windows(&events);
        assert_eq!(result.windows_examined, 2);
        assert_eq!(result.max_events, 4);
        assert_eq!(result.windows_over_three_events, 1);
    }

    #[test]
    fn partial_subsecond_trace_does_not_claim_a_complete_window() {
        let events = vec![true; ONE_SECOND_WINDOW_SAMPLES - 1];
        assert_eq!(summarize_one_second_windows(&events), WindowSummary::default());
    }

    #[test]
    fn parser_accepts_theme_motion_and_frame_limit() {
        let args = vec![
            "opposing_transition_audit".to_string(),
            "--theme".to_string(),
            "oceanic-intelligence".to_string(),
            "--motion".to_string(),
            "reduced".to_string(),
            "--frames".to_string(),
            "61".to_string(),
        ];
        let parsed = parse_args_from(&args).expect("valid").expect("not help");
        assert_eq!(parsed, (160, 90, 61, Theme::OceanicIntelligence, MotionMode::Reduced));
    }

    #[test]
    fn parser_rejects_unknown_motion_mode() {
        let args = vec![
            "opposing_transition_audit".to_string(),
            "--motion".to_string(),
            "unknown".to_string(),
        ];
        assert!(parse_args_from(&args).is_err());
    }
}
