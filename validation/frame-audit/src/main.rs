// Copyright (C) 2024-2026 Tristan Stoltz / Luminous Dynamics
// SPDX-License-Identifier: AGPL-3.0-or-later
// Commercial licensing: see COMMERCIAL_LICENSE.md at repository root

//! Independent temporal regression audit for externally captured P6 PPM frames.
//!
//! This tool does not import the scene renderer. It analyzes the decoded pixels
//! supplied by the caller. It is a screen-raster proxy, not WCAG certification.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

const DEFAULT_FPS: u32 = 30;
const MAX_FPS: u32 = 240;
const MAX_FRAMES: usize = 1_800;
const MAX_FRAME_PIXELS: usize = 8_294_400; // UHD 4K
const MAX_TOTAL_SAMPLED_PIXELS: usize = 500_000_000;
const GENERAL_LUMINANCE_DELTA: f64 = 0.10;
const RED_CHROMATICITY_DELTA: f64 = 0.20;
const LARGE_AREA_RATIO: f64 = 0.25;
const MAX_FLASH_EVENTS_PER_SECOND: usize = 3;
const DEFAULT_MAX_GAP_MS: u64 = 100;
const ONE_SECOND_NS: u64 = 1_000_000_000;
const MAX_TIMESTAMP_FILE_BYTES: u64 = (MAX_FRAMES as u64) * 32;

#[derive(Debug)]
struct Config {
    fps: u32,
    paths: Vec<PathBuf>,
    timestamps: Option<PathBuf>,
    max_gap_ms: u64,
}

#[derive(Debug)]
struct Frame {
    width: usize,
    height: usize,
    pixels: Vec<u32>,
}

#[derive(Debug, Default)]
struct Summary {
    transitions_examined: usize,
    large_area_general_transitions: usize,
    large_area_red_transitions: usize,
    max_general_flash_area_ratio: f64,
    max_red_flash_area_ratio: f64,
    max_general_local_window_ratio: f64,
    max_red_local_window_ratio: f64,
}

const RED_STATE_VECTOR_VALID: u8 = 1;
const RED_STATE_INVOLVES_RED: u8 = 2;
const RED_STATE_NEAR_BLACK_RED: u8 = 4;

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
struct LastLuminanceTransition {
    // Zero marks "no previous edge"; valid input frame indexes start at one.
    frame_index: u16,
    rising: bool,
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
struct LastRedTransition {
    // Compact fixed-size state keeps 4K audits within a bounded memory budget.
    frame_index: u16,
    flags: u8,
    dx: f32,
    dy: f32,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct WindowSummary {
    windows_examined: usize,
    max_events: usize,
    windows_over_three_events: usize,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("frame_audit: {error}");
        print_usage();
        process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let Some(config) = parse_args_from(&env::args().skip(1).collect::<Vec<_>>())? else {
        return Ok(());
    };
    if config.paths.len() < 3 || config.paths.len() > MAX_FRAMES {
        return Err(format!("provide 3..={MAX_FRAMES} ordered P6 PPM frames"));
    }

    let timestamped = config.timestamps.is_some();
    let timestamps_ns = match config.timestamps.as_ref() {
        Some(path) => read_timestamps_ns(path, config.paths.len())?,
        None => (0..config.paths.len())
            .map(|index| (index as u128 * ONE_SECOND_NS as u128 / config.fps as u128) as u64)
            .collect(),
    };
    let max_sample_gap_ns = timestamps_ns
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .max()
        .unwrap_or(0);
    let allowed_sample_gap_ns = config.max_gap_ms.saturating_mul(1_000_000);
    let sampling_gap_gate_passed = !timestamped || max_sample_gap_ns <= allowed_sample_gap_ns;

    let mut path_iter = config.paths.iter();
    let first_path = path_iter.next().expect("validated non-empty frames");
    let first = read_ppm(first_path)?;
    let width = first.width;
    let height = first.height;
    let (field_width, field_height) = visual_field_window_dimensions(width, height);
    let pixel_count = width
        .checked_mul(height)
        .ok_or("frame dimensions overflow")?;
    let total_sampled_pixels = pixel_count
        .checked_mul(config.paths.len())
        .ok_or("sample workload size overflow")?;
    if total_sampled_pixels > MAX_TOTAL_SAMPLED_PIXELS {
        return Err(format!(
            "workload exceeds {} total sampled pixels",
            MAX_TOTAL_SAMPLED_PIXELS
        ));
    }

    let mut previous = first;
    let mut last_luminance_transitions =
        vec![LastLuminanceTransition::default(); pixel_count];
    let mut last_red_transitions = vec![LastRedTransition::default(); pixel_count];
    // Reuse masks frame-to-frame to avoid two full-raster allocations per sample.
    let mut general_flash_mask = vec![0u8; pixel_count];
    let mut red_flash_mask = vec![0u8; pixel_count];
    let mut summary = Summary::default();
    let mut general_events = Vec::with_capacity(config.paths.len() - 1);
    let mut red_events = Vec::with_capacity(config.paths.len() - 1);
    let mut general_events_timed = vec![false; config.paths.len()];
    let mut red_events_timed = vec![false; config.paths.len()];

    for (frame_offset, path) in path_iter.enumerate() {
        let current = read_ppm(path)?;
        if current.width != width || current.height != height {
            return Err("all frames must have identical dimensions".to_string());
        }
        let frame_index = frame_offset + 1;
        let pair = analyze_transition_with_masks_timed(
            &previous.pixels,
            &current.pixels,
            width,
            height,
            frame_index,
            &timestamps_ns,
            ONE_SECOND_NS,
            &mut last_luminance_transitions,
            &mut last_red_transitions,
            &mut general_flash_mask,
            &mut red_flash_mask,
        );
        summary.transitions_examined += 1;
        summary.max_general_flash_area_ratio = summary
            .max_general_flash_area_ratio
            .max(pair.general_flash_area_ratio);
        summary.max_red_flash_area_ratio = summary
            .max_red_flash_area_ratio
            .max(pair.red_flash_area_ratio);
        summary.max_general_local_window_ratio = summary
            .max_general_local_window_ratio
            .max(pair.general_local_window_ratio);
        summary.max_red_local_window_ratio = summary
            .max_red_local_window_ratio
            .max(pair.red_local_window_ratio);
        let general_event = pair.general_local_window_ratio >= LARGE_AREA_RATIO;
        let red_event = pair.red_local_window_ratio >= LARGE_AREA_RATIO;
        general_events.push(general_event);
        red_events.push(red_event);
        general_events_timed[frame_index] = general_event;
        red_events_timed[frame_index] = red_event;
        if general_event {
            summary.large_area_general_transitions += 1;
        }
        if red_event {
            summary.large_area_red_transitions += 1;
        }
        previous = current;
    }

    if timestamped {
        let general_windows = summarize_one_second_windows_timed(&timestamps_ns, &general_events_timed);
        let red_windows = summarize_one_second_windows_timed(&timestamps_ns, &red_events_timed);
        println!(
            concat!(
                "{{\n",
                "  \"schema\": \"captured-frame-audit-v4-timestamped\",\n",
                "  \"source_commit\": \"{}\",\n",
                "  \"build_commit\": \"{}\",\n",
                "  \"source\": \"external_p6_ppm_frame_sequence\",\n",
                "  \"temporal_basis\": \"monotonic_presentation_timestamps_ns\",\n",
                "  \"frames_examined\": {},\n",
                "  \"resolution\": {{ \"width\": {}, \"height\": {} }},\n",
                "  \"timeline\": {{ \"start_ns\": {}, \"end_ns\": {}, \"observed_span_ns\": {}, \"max_sample_gap_ns\": {}, \"allowed_sample_gap_ns\": {}, \"sampling_gap_gate_passed\": {}, \"coverage_status\": \"{}\" }},\n",
                "  \"frame_transitions_examined\": {},\n",
                "  \"max_general_flash_area_ratio\": {:.6},\n",
                "  \"max_red_flash_area_ratio\": {:.6},\n",
                "  \"local_visual_field_proxy\": {{ \"method\": \"scaled_one_third_raster_window\", \"width\": {}, \"height\": {}, \"max_general_flash_area_ratio\": {:.6}, \"max_red_flash_area_ratio\": {:.6} }},\n",
                "  \"large_local_area_general_flash_transitions\": {},\n",
                "  \"large_local_area_red_flash_transitions\": {},\n",
                "  \"one_second_window_summary\": {{ \"basis\": \"presentation_timestamp_ns\", \"windows_examined\": {}, \"max_general_large_area_flash_events\": {}, \"general_windows_over_three_flash_events\": {}, \"max_red_large_area_flash_events\": {}, \"red_windows_over_three_flash_events\": {} }},\n",
                "  \"area_model\": \"sliding_one_third_local_raster_window_proxy\",\n",
                "  \"normative_wcag_conformance\": false,\n",
                "  \"thresholds\": {{ \"general_luminance_delta_ge\": {:.2}, \"red_chromaticity_delta_gt\": {:.2}, \"large_area_ratio_ge\": {:.2}, \"max_events_per_second\": {} }}\n",
                "}}"
            ),
            option_env!("VISUAL_SOURCE_COMMIT").unwrap_or("unavailable-local-build"),
            option_env!("VISUAL_BUILD_COMMIT").unwrap_or("unavailable-local-build"),
            config.paths.len(),
            width,
            height,
            timestamps_ns[0],
            *timestamps_ns.last().expect("timestamp count equals frame count"),
            timestamps_ns[timestamps_ns.len() - 1] - timestamps_ns[0],
            max_sample_gap_ns,
            allowed_sample_gap_ns,
            sampling_gap_gate_passed,
            if sampling_gap_gate_passed { "within_declared_gap_limit" } else { "incomplete_sampling_due_to_gap" },
            summary.transitions_examined,
            summary.max_general_flash_area_ratio,
            summary.max_red_flash_area_ratio,
            field_width,
            field_height,
            summary.max_general_local_window_ratio,
            summary.max_red_local_window_ratio,
            summary.large_area_general_transitions,
            summary.large_area_red_transitions,
            general_windows.windows_examined,
            general_windows.max_events,
            general_windows.windows_over_three_events,
            red_windows.max_events,
            red_windows.windows_over_three_events,
            GENERAL_LUMINANCE_DELTA,
            RED_CHROMATICITY_DELTA,
            LARGE_AREA_RATIO,
            MAX_FLASH_EVENTS_PER_SECOND,
        );
    } else {
    let general_windows = summarize_one_second_windows(&general_events, config.fps);
    let red_windows = summarize_one_second_windows(&red_events, config.fps);
    let window_samples = config.fps as usize + 1;

    println!(
        concat!(
            "{{\n",
            "  \"schema\": \"captured-frame-audit-v3\",\n",
            "  \"source_commit\": \"{}\",\n",
            "  \"build_commit\": \"{}\",\n",
            "  \"source\": \"external_p6_ppm_frame_sequence\",\n",
            "  \"fps_assumed\": {},\n",
            "  \"frames_examined\": {},\n",
            "  \"resolution\": {{ \"width\": {}, \"height\": {} }},\n",
            "  \"frame_transitions_examined\": {},\n",
            "  \"max_general_flash_area_ratio\": {:.6},\n",
            "  \"max_red_flash_area_ratio\": {:.6},\n",
            "  \"local_visual_field_proxy\": {{ \"method\": \"scaled_one_third_raster_window\", \"width\": {}, \"height\": {}, \"max_general_flash_area_ratio\": {:.6}, \"max_red_flash_area_ratio\": {:.6} }},\n",
            "  \"large_local_area_general_flash_transitions\": {},\n",
            "  \"large_local_area_red_flash_transitions\": {},\n",
            "  \"one_second_window_summary\": {{ \"sample_points_per_window\": {}, \"windows_examined\": {}, \"max_general_large_area_flash_events\": {}, \"general_windows_over_three_flash_events\": {}, \"max_red_large_area_flash_events\": {}, \"red_windows_over_three_flash_events\": {} }},\n",
            "  \"area_model\": \"sliding_one_third_local_raster_window_proxy\",\n",
            "  \"normative_wcag_conformance\": false,\n",
            "  \"thresholds\": {{ \"general_luminance_delta_ge\": {:.2}, \"red_chromaticity_delta_gt\": {:.2}, \"large_area_ratio_ge\": {:.2}, \"max_events_per_second\": {} }}\n",
            "}}"
        ),
        option_env!("VISUAL_SOURCE_COMMIT").unwrap_or("unavailable-local-build"),
        option_env!("VISUAL_BUILD_COMMIT").unwrap_or("unavailable-local-build"),
        config.fps,
        config.paths.len(),
        width,
        height,
        summary.transitions_examined,
        summary.max_general_flash_area_ratio,
        summary.max_red_flash_area_ratio,
        field_width,
        field_height,
        summary.max_general_local_window_ratio,
        summary.max_red_local_window_ratio,
        summary.large_area_general_transitions,
        summary.large_area_red_transitions,
        window_samples,
        general_windows.windows_examined,
        general_windows.max_events,
        general_windows.windows_over_three_events,
        red_windows.max_events,
        red_windows.windows_over_three_events,
        GENERAL_LUMINANCE_DELTA,
        RED_CHROMATICITY_DELTA,
        LARGE_AREA_RATIO,
        MAX_FLASH_EVENTS_PER_SECOND,
    );
    }
    Ok(())
}

fn parse_args_from(args: &[String]) -> Result<Option<Config>, String> {
    let mut fps = DEFAULT_FPS;
    let mut paths = Vec::new();
    let mut timestamps = None;
    let mut max_gap_ms = DEFAULT_MAX_GAP_MS;
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--fps" => {
                index += 1;
                let value = args.get(index).ok_or("--fps requires a value")?;
                fps = value
                    .parse::<u32>()
                    .map_err(|_| "--fps must be an integer between 1 and 240")?;
                if !(1..=MAX_FPS).contains(&fps) {
                    return Err("--fps must be an integer between 1 and 240".to_string());
                }
            }
            "--timestamps" => {
                index += 1;
                let value = args.get(index).ok_or("--timestamps requires a file path")?;
                if timestamps.is_some() {
                    return Err("--timestamps may be supplied only once".to_string());
                }
                timestamps = Some(PathBuf::from(value));
            }
            "--max-gap-ms" => {
                index += 1;
                let value = args.get(index).ok_or("--max-gap-ms requires a value")?;
                max_gap_ms = value.parse::<u64>()
                    .map_err(|_| "--max-gap-ms must be an integer from 1 to 60000")?;
                if !(1..=60_000).contains(&max_gap_ms) {
                    return Err("--max-gap-ms must be an integer from 1 to 60000".to_string());
                }
            }
            "--help" | "-h" => {
                print_usage();
                return Ok(None);
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown option: {value}"));
            }
            path => paths.push(PathBuf::from(path)),
        }
        index += 1;
    }
    if paths.len() < 3 {
        return Err("at least three ordered frame paths are required".to_string());
    }
    if paths.len() > MAX_FRAMES {
        return Err(format!("frame count exceeds {MAX_FRAMES}"));
    }
    Ok(Some(Config { fps, paths, timestamps, max_gap_ms }))
}

fn print_usage() {
    eprintln!(
        "Usage: cargo run --manifest-path validation/frame-audit/Cargo.toml --release -- \
         [--fps 30] [--timestamps timestamps-ns.txt [--max-gap-ms 100]] \
         frame-000001.ppm frame-000002.ppm frame-000003.ppm ..."
    );
}


fn read_timestamps_ns(path: &Path, expected_count: usize) -> Result<Vec<u64>, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("cannot stat timestamp sidecar: {error}"))?;
    if metadata.len() > MAX_TIMESTAMP_FILE_BYTES {
        return Err("timestamp sidecar exceeds its byte bound".to_string());
    }
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("cannot read UTF-8 timestamp sidecar: {error}"))?;
    parse_timestamps_ns(&contents, expected_count)
}

fn parse_timestamps_ns(contents: &str, expected_count: usize) -> Result<Vec<u64>, String> {
    if expected_count < 3 || expected_count > MAX_FRAMES {
        return Err("timestamp count target is outside the accepted frame bound".to_string());
    }
    if contents.len() as u64 > MAX_TIMESTAMP_FILE_BYTES {
        return Err("timestamp sidecar exceeds its byte bound".to_string());
    }
    let mut timestamps: Vec<u64> = Vec::with_capacity(expected_count);
    for (line_index, line) in contents.lines().enumerate() {
        let value = line.trim();
        if value.is_empty() {
            continue;
        }
        if !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(format!("timestamp line {} must be unsigned integer nanoseconds", line_index + 1));
        }
        let timestamp = value
            .parse::<u64>()
            .map_err(|_| format!("timestamp line {} is outside the u64 range", line_index + 1))?;
        if let Some(previous) = timestamps.last() {
            if timestamp <= *previous {
                return Err(format!("timestamps must be strictly increasing (line {})", line_index + 1));
            }
        }
        timestamps.push(timestamp);
        if timestamps.len() > expected_count {
            return Err("timestamp count does not match frame count".to_string());
        }
    }
    if timestamps.len() != expected_count {
        return Err(format!(
            "timestamp count {} does not match frame count {}",
            timestamps.len(), expected_count
        ));
    }
    Ok(timestamps)
}

fn elapsed_within_window(previous_ns: u64, current_ns: u64, window_ns: u64) -> bool {
    current_ns >= previous_ns && current_ns - previous_ns <= window_ns
}

fn summarize_one_second_windows_timed(timestamps_ns: &[u64], events: &[bool]) -> WindowSummary {
    if timestamps_ns.len() != events.len() || timestamps_ns.len() < 2 {
        return WindowSummary::default();
    }
    let mut result = WindowSummary::default();
    let mut left = 0usize;
    let mut event_count = 0usize;
    for right in 0..timestamps_ns.len() {
        if events[right] {
            event_count += 1;
        }
        while timestamps_ns[right] - timestamps_ns[left] > ONE_SECOND_NS {
            if events[left] {
                event_count -= 1;
            }
            left += 1;
        }
        if timestamps_ns[right] - timestamps_ns[0] >= ONE_SECOND_NS {
            result.windows_examined += 1;
            result.max_events = result.max_events.max(event_count);
            if event_count > MAX_FLASH_EVENTS_PER_SECOND {
                result.windows_over_three_events += 1;
            }
        }
    }
    result
}

fn read_ppm(path: &Path) -> Result<Frame, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("cannot stat an input frame: {error}"))?;
    let max_bytes = MAX_FRAME_PIXELS
        .checked_mul(3)
        .and_then(|value| value.checked_add(1_024))
        .ok_or("frame byte bound overflow")?;
    if metadata.len() as u128 > max_bytes as u128 {
        return Err("input PPM exceeds the per-frame byte bound".to_string());
    }
    let bytes = fs::read(path).map_err(|error| format!("cannot read an input frame: {error}"))?;
    parse_ppm_bytes(&bytes)
}

fn parse_ppm_bytes(bytes: &[u8]) -> Result<Frame, String> {
    let mut cursor = 0usize;
    if next_token(bytes, &mut cursor)? != b"P6" {
        return Err("only binary P6 PPM input is supported".to_string());
    }
    let width = parse_positive_usize(next_token(bytes, &mut cursor)?, "width")?;
    let height = parse_positive_usize(next_token(bytes, &mut cursor)?, "height")?;
    let max_value = parse_positive_usize(next_token(bytes, &mut cursor)?, "maxval")?;
    if max_value != 255 {
        return Err("P6 PPM maxval must be 255".to_string());
    }
    if cursor >= bytes.len() || !bytes[cursor].is_ascii_whitespace() {
        return Err("P6 header must end with whitespace before raster data".to_string());
    }
    let separator = bytes[cursor];
    cursor += 1;
    if separator == b'\r' && bytes.get(cursor) == Some(&b'\n') {
        cursor += 1;
    }

    let pixel_count = width.checked_mul(height).ok_or("frame dimensions overflow")?;
    if pixel_count > MAX_FRAME_PIXELS {
        return Err(format!("frame exceeds {MAX_FRAME_PIXELS} pixels"));
    }
    let raster_len = pixel_count.checked_mul(3).ok_or("raster size overflow")?;
    if bytes.len().saturating_sub(cursor) != raster_len {
        return Err("P6 raster byte count does not match its dimensions".to_string());
    }
    let mut pixels = Vec::with_capacity(pixel_count);
    for rgb in bytes[cursor..].chunks_exact(3) {
        pixels.push(((rgb[0] as u32) << 16) | ((rgb[1] as u32) << 8) | rgb[2] as u32);
    }
    Ok(Frame {
        width,
        height,
        pixels,
    })
}

fn next_token<'a>(bytes: &'a [u8], cursor: &mut usize) -> Result<&'a [u8], String> {
    loop {
        while *cursor < bytes.len() && bytes[*cursor].is_ascii_whitespace() {
            *cursor += 1;
        }
        if *cursor < bytes.len() && bytes[*cursor] == b'#' {
            while *cursor < bytes.len() && bytes[*cursor] != b'\n' {
                *cursor += 1;
            }
        } else {
            break;
        }
    }
    let start = *cursor;
    while *cursor < bytes.len()
        && !bytes[*cursor].is_ascii_whitespace()
        && bytes[*cursor] != b'#'
    {
        *cursor += 1;
    }
    if start == *cursor {
        return Err("unexpected end of P6 header".to_string());
    }
    Ok(&bytes[start..*cursor])
}

fn parse_positive_usize(value: &[u8], name: &str) -> Result<usize, String> {
    let text = std::str::from_utf8(value).map_err(|_| format!("invalid {name} token"))?;
    let parsed = text
        .parse::<usize>()
        .map_err(|_| format!("invalid {name} value"))?;
    if parsed == 0 {
        return Err(format!("{name} must be positive"));
    }
    Ok(parsed)
}

fn srgb_to_linear(channel: u8) -> f64 {
    let value = channel as f64 / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn luminance(pixel: u32) -> f64 {
    let red = srgb_to_linear(((pixel >> 16) & 0xff) as u8);
    let green = srgb_to_linear(((pixel >> 8) & 0xff) as u8);
    let blue = srgb_to_linear((pixel & 0xff) as u8);
    0.2126 * red + 0.7152 * green + 0.0722 * blue
}

fn chromaticity_uv(pixel: u32) -> Option<(f64, f64)> {
    if near_black(pixel) {
        // Low-signal chromaticity is treated as indeterminate by this proxy.
        return None;
    }
    let red = srgb_to_linear(((pixel >> 16) & 0xff) as u8);
    let green = srgb_to_linear(((pixel >> 8) & 0xff) as u8);
    let blue = srgb_to_linear((pixel & 0xff) as u8);
    let x = 0.4124564 * red + 0.3575761 * green + 0.1804375 * blue;
    let y = 0.2126729 * red + 0.7151522 * green + 0.0721750 * blue;
    let z = 0.0193339 * red + 0.1191920 * green + 0.9503041 * blue;
    let denominator = x + 15.0 * y + 3.0 * z;
    if denominator <= 1e-12 {
        None
    } else {
        Some((4.0 * x / denominator, 9.0 * y / denominator))
    }
}

fn red_fraction(pixel: u32) -> f64 {
    let red = ((pixel >> 16) & 0xff) as f64;
    let green = ((pixel >> 8) & 0xff) as f64;
    let blue = (pixel & 0xff) as f64;
    let total = red + green + blue;
    if total <= f64::EPSILON {
        0.0
    } else {
        red / total
    }
}

fn near_black(pixel: u32) -> bool {
    let red = (pixel >> 16) & 0xff;
    let green = (pixel >> 8) & 0xff;
    let blue = pixel & 0xff;
    red.max(green).max(blue) <= 1
}

fn uv_distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

fn saturated_red_transition(before: u32, after: u32) -> bool {
    let before_red = red_fraction(before) >= 0.80;
    let after_red = red_fraction(after) >= 0.80;
    if !before_red && !after_red {
        return false;
    }
    match (chromaticity_uv(before), chromaticity_uv(after)) {
        (Some(a), Some(b)) => uv_distance(a, b) > RED_CHROMATICITY_DELTA,
        (None, Some(_)) => near_black(before) && after_red,
        (Some(_), None) => before_red && near_black(after),
        (None, None) => false,
    }
}

#[derive(Debug, Default, PartialEq)]
struct PairMetrics {
    general_flash_area_ratio: f64,
    red_flash_area_ratio: f64,
    general_local_window_ratio: f64,
    red_local_window_ratio: f64,
}

// Pair significant opposing edges for the same pixel even when a bright or
// saturated-red plateau spans multiple sampled frames. This is only a proxy.
fn analyze_transition(
    before: &[u32],
    after: &[u32],
    width: usize,
    height: usize,
    frame_index: usize,
    fps: u32,
    last_luminance: &mut [LastLuminanceTransition],
    last_red: &mut [LastRedTransition],
) -> PairMetrics {
    let mut general_flash_mask = vec![0u8; before.len()];
    let mut red_flash_mask = vec![0u8; before.len()];
    analyze_transition_with_masks(
        before,
        after,
        width,
        height,
        frame_index,
        fps,
        last_luminance,
        last_red,
        &mut general_flash_mask,
        &mut red_flash_mask,
    )
}

fn analyze_transition_with_masks(
    before: &[u32],
    after: &[u32],
    width: usize,
    height: usize,
    frame_index: usize,
    fps: u32,
    last_luminance: &mut [LastLuminanceTransition],
    last_red: &mut [LastRedTransition],
    general_flash_mask: &mut [u8],
    red_flash_mask: &mut [u8],
) -> PairMetrics {
    let timestamps_ns: Vec<u64> = (0..=frame_index)
        .map(|index| (index as u128 * ONE_SECOND_NS as u128 / fps as u128) as u64)
        .collect();
    analyze_transition_with_masks_timed(
        before, after, width, height, frame_index, &timestamps_ns, ONE_SECOND_NS,
        last_luminance, last_red, general_flash_mask, red_flash_mask,
    )
}

fn analyze_transition_with_masks_timed(
    before: &[u32],
    after: &[u32],
    width: usize,
    height: usize,
    frame_index: usize,
    timestamps_ns: &[u64],
    max_pair_interval_ns: u64,
    last_luminance: &mut [LastLuminanceTransition],
    last_red: &mut [LastRedTransition],
    general_flash_mask: &mut [u8],
    red_flash_mask: &mut [u8],
) -> PairMetrics {
    assert_eq!(before.len(), after.len());
    assert_eq!(before.len(), width * height);
    assert_eq!(before.len(), last_luminance.len());
    assert_eq!(before.len(), last_red.len());
    assert_eq!(before.len(), general_flash_mask.len());
    assert_eq!(before.len(), red_flash_mask.len());
    assert!(!before.is_empty());
    assert!(frame_index < timestamps_ns.len());

    general_flash_mask.fill(0);
    red_flash_mask.fill(0);
    let mut general_flash_pixels = 0usize;
    let mut red_flash_pixels = 0usize;

    for (index, (&first, &second)) in before.iter().zip(after).enumerate() {
        let l0 = luminance(first);
        let l1 = luminance(second);
        let delta = l1 - l0;
        if delta.abs() >= GENERAL_LUMINANCE_DELTA && l0.min(l1) < 0.80 {
            let current = LastLuminanceTransition {
                frame_index: frame_index as u16,
                rising: delta > 0.0,
            };
            let previous = last_luminance[index];
            if previous.frame_index != 0 {
                let previous_index = previous.frame_index as usize;
                let within_window = previous_index < timestamps_ns.len()
                    && elapsed_within_window(
                        timestamps_ns[previous_index],
                        timestamps_ns[frame_index],
                        max_pair_interval_ns,
                    );
                if within_window && previous.rising != current.rising {
                    general_flash_pixels += 1;
                    general_flash_mask[index] = 1;
                }
            }
            last_luminance[index] = current;
        }

        if saturated_red_transition(first, second) {
            let first_uv = chromaticity_uv(first);
            let second_uv = chromaticity_uv(second);
            let vector = match (first_uv, second_uv) {
                (Some(a), Some(b)) => Some((b.0 - a.0, b.1 - a.1)),
                _ => None,
            };
            let first_is_red = red_fraction(first) >= 0.80;
            let second_is_red = red_fraction(second) >= 0.80;
            let mut flags = 0u8;
            if vector.is_some() {
                flags |= RED_STATE_VECTOR_VALID;
            }
            if first_is_red || second_is_red {
                flags |= RED_STATE_INVOLVES_RED;
            }
            if (near_black(first) && second_is_red) || (near_black(second) && first_is_red) {
                flags |= RED_STATE_NEAR_BLACK_RED;
            }
            let (dx, dy) = vector.unwrap_or((0.0, 0.0));
            let current = LastRedTransition {
                frame_index: frame_index as u16,
                flags,
                dx: dx as f32,
                dy: dy as f32,
            };
            let previous = last_red[index];
            if previous.frame_index != 0 {
                let previous_index = previous.frame_index as usize;
                let within_window = previous_index < timestamps_ns.len()
                    && elapsed_within_window(
                        timestamps_ns[previous_index],
                        timestamps_ns[frame_index],
                        max_pair_interval_ns,
                    );
                let previous_vector_valid = previous.flags & RED_STATE_VECTOR_VALID != 0;
                let current_vector_valid = current.flags & RED_STATE_VECTOR_VALID != 0;
                let opposing = if previous_vector_valid && current_vector_valid {
                    previous.dx * current.dx + previous.dy * current.dy < 0.0
                } else {
                    previous.flags & RED_STATE_NEAR_BLACK_RED != 0
                        && current.flags & RED_STATE_NEAR_BLACK_RED != 0
                };
                let involves_red = (previous.flags | current.flags) & RED_STATE_INVOLVES_RED != 0;
                if within_window && opposing && involves_red {
                    red_flash_pixels += 1;
                    red_flash_mask[index] = 1;
                }
            }
            last_red[index] = current;
        }
    }

    let count = before.len() as f64;
    PairMetrics {
        general_flash_area_ratio: general_flash_pixels as f64 / count,
        red_flash_area_ratio: red_flash_pixels as f64 / count,
        general_local_window_ratio: max_visual_field_ratio(&general_flash_mask, width, height),
        red_local_window_ratio: max_visual_field_ratio(&red_flash_mask, width, height),
    }
}

fn visual_field_window_dimensions(width: usize, height: usize) -> (usize, usize) {
    // W3C's published 1024x768 approximation is 341x256; scaled each
    // dimension by one third, this remains a geometry proxy for other rasters.
    ((width / 3).max(1), (height / 3).max(1))
}

fn max_visual_field_ratio(mask: &[u8], width: usize, height: usize) -> f64 {
    assert_eq!(mask.len(), width * height);
    if width == 0 || height == 0 {
        return 0.0;
    }
    let (window_width, window_height) = visual_field_window_dimensions(width, height);
    let window_area = window_width * window_height;
    let mut column_counts = vec![0usize; width];

    for y in 0..window_height {
        for x in 0..width {
            column_counts[x] += usize::from(mask[y * width + x] != 0);
        }
    }

    let mut max_count = 0usize;
    for top in 0..=height - window_height {
        if top > 0 {
            let removed_row = top - 1;
            let added_row = top + window_height - 1;
            for x in 0..width {
                column_counts[x] -= usize::from(mask[removed_row * width + x] != 0);
                column_counts[x] += usize::from(mask[added_row * width + x] != 0);
            }
        }

        let mut count: usize = column_counts[..window_width].iter().sum();
        max_count = max_count.max(count);
        for left in 1..=width - window_width {
            count -= column_counts[left - 1];
            count += column_counts[left + window_width - 1];
            max_count = max_count.max(count);
        }
    }

    max_count as f64 / window_area as f64
}

fn summarize_one_second_windows(events: &[bool], fps: u32) -> WindowSummary {
    let window_samples = fps as usize + 1;
    if events.len() < window_samples {
        return WindowSummary::default();
    }
    let mut result = WindowSummary::default();
    let mut event_count = 0usize;
    for end in 0..events.len() {
        if events[end] {
            event_count += 1;
        }
        if end >= window_samples && events[end - window_samples] {
            event_count -= 1;
        }
        if end + 1 >= window_samples {
            result.windows_examined += 1;
            result.max_events = result.max_events.max(event_count);
            if event_count > MAX_FLASH_EVENTS_PER_SECOND {
                result.windows_over_three_events += 1;
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_external_p6_ppm_rgb_pixels() {
        let mut bytes = b"P6\n2 1\n255\n".to_vec();
        bytes.extend_from_slice(&[0, 0, 0, 255, 255, 255]);
        let frame = parse_ppm_bytes(&bytes).expect("valid PPM");
        assert_eq!(frame.width, 2);
        assert_eq!(frame.height, 1);
        assert_eq!(frame.pixels, vec![0x000000, 0xffffff]);
    }

    #[test]
    fn rejects_raster_size_mismatch() {
        let bytes = b"P6\n2 1\n255\n\x00\x00\x00";
        assert!(parse_ppm_bytes(bytes).is_err());
    }

    #[test]
    fn detects_black_white_black_opposing_luminance_transitions() {
        let black = [0x000000; 100];
        let white = [0xffffff; 100];
        let mut lum = vec![LastLuminanceTransition::default(); 100];
        let mut red = vec![LastRedTransition::default(); 100];
        assert_eq!(
            analyze_transition(&black, &white, 10, 10, 1, 30, &mut lum, &mut red).general_flash_area_ratio,
            0.0
        );
        assert_eq!(
            analyze_transition(&white, &black, 10, 10, 2, 30, &mut lum, &mut red).general_flash_area_ratio,
            1.0
        );
    }

    #[test]
    fn monotonic_luminance_transition_is_not_a_flash() {
        let black = [0x000000; 100];
        let white = [0xffffff; 100];
        let mut lum = vec![LastLuminanceTransition::default(); 100];
        let mut red = vec![LastRedTransition::default(); 100];
        let _ = analyze_transition(&black, &white, 10, 10, 1, 30, &mut lum, &mut red);
        let result = analyze_transition(&white, &white, 10, 10, 2, 30, &mut lum, &mut red);
        assert_eq!(result.general_flash_area_ratio, 0.0);
        assert_eq!(result.general_local_window_ratio, 0.0);
    }

    #[test]
    fn flash_masks_are_cleared_between_transitions() {
        let black = [0x000000; 100];
        let white = [0xffffff; 100];
        let mut lum = vec![LastLuminanceTransition::default(); 100];
        let mut red = vec![LastRedTransition::default(); 100];
        let mut general_mask = vec![0u8; 100];
        let mut red_mask = vec![0u8; 100];
        let _ = analyze_transition_with_masks(&black, &white, 10, 10, 1, 30, &mut lum, &mut red, &mut general_mask, &mut red_mask);
        assert!(general_mask.iter().all(|x| *x == 0));
        let _ = analyze_transition_with_masks(&white, &black, 10, 10, 2, 30, &mut lum, &mut red, &mut general_mask, &mut red_mask);
        assert!(general_mask.iter().all(|x| *x == 1));
        let _ = analyze_transition_with_masks(&black, &black, 10, 10, 3, 30, &mut lum, &mut red, &mut general_mask, &mut red_mask);
        assert!(general_mask.iter().all(|x| *x == 0));
    }

    #[test]
    fn detects_flash_when_bright_plateau_spans_multiple_frames() {
        let black = [0x000000; 100];
        let white = [0xffffff; 100];
        let mut lum = vec![LastLuminanceTransition::default(); 100];
        let mut red = vec![LastRedTransition::default(); 100];
        let _ = analyze_transition(&black, &white, 10, 10, 1, 30, &mut lum, &mut red);
        let _ = analyze_transition(&white, &white, 10, 10, 2, 30, &mut lum, &mut red);
        let result = analyze_transition(&white, &black, 10, 10, 3, 30, &mut lum, &mut red);
        assert_eq!(result.general_flash_area_ratio, 1.0);
    }

    #[test]
    fn ignores_opposing_luminance_edges_outside_one_second() {
        let black = [0x000000; 100];
        let white = [0xffffff; 100];
        let mut lum = vec![LastLuminanceTransition::default(); 100];
        let mut red = vec![LastRedTransition::default(); 100];
        let _ = analyze_transition(&black, &white, 10, 10, 1, 2, &mut lum, &mut red);
        let result = analyze_transition(&white, &black, 10, 10, 4, 2, &mut lum, &mut red);
        assert_eq!(result.general_flash_area_ratio, 0.0);
    }

    #[test]
    fn detects_white_red_white_saturated_red_transition_pair() {
        let white = [0xffffff; 100];
        let red = [0xff0000; 100];
        let mut lum = vec![LastLuminanceTransition::default(); 100];
        let mut red_state = vec![LastRedTransition::default(); 100];
        let _ = analyze_transition(&white, &red, 10, 10, 1, 30, &mut lum, &mut red_state);
        let result = analyze_transition(&red, &white, 10, 10, 2, 30, &mut lum, &mut red_state);
        assert_eq!(result.red_flash_area_ratio, 1.0);
    }



    #[test]
    fn near_black_to_saturated_red_is_flagged_conservatively() {
        let near_black = [0x0001_0101; 100];
        let red = [0x00ff_0000; 100];
        let mut lum = vec![LastLuminanceTransition::default(); 100];
        let mut red_state = vec![LastRedTransition::default(); 100];
        let _ = analyze_transition(&near_black, &red, 10, 10, 1, 30, &mut lum, &mut red_state);
        let result = analyze_transition(&red, &near_black, 10, 10, 2, 30, &mut lum, &mut red_state);
        assert_eq!(result.red_flash_area_ratio, 1.0);
    }

    #[test]
    fn per_pixel_transition_state_remains_compact() {
        assert_eq!(std::mem::size_of::<LastLuminanceTransition>(), 4);
        assert_eq!(std::mem::size_of::<LastRedTransition>(), 12);
    }

    #[test]
    fn local_window_ratio_is_zero_for_empty_and_one_for_full_mask() {
        let width = 12usize;
        let height = 12usize;
        assert_eq!(max_visual_field_ratio(&vec![0u8; width * height], width, height), 0.0);
        assert_eq!(max_visual_field_ratio(&vec![1u8; width * height], width, height), 1.0);
    }

    #[test]
    fn local_window_finds_flash_patch_at_bottom_right_boundary() {
        let width = 12usize;
        let height = 12usize;
        let mut mask = vec![0u8; width * height];
        for y in 10..12 {
            for x in 10..12 {
                mask[y * width + x] = 1;
            }
        }
        let ratio = max_visual_field_ratio(&mask, width, height);
        assert!((ratio - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn local_visual_field_matches_published_reference_approximation() {
        assert_eq!(visual_field_window_dimensions(1024, 768), (341, 256));
    }

    #[test]
    fn local_window_catches_concentrated_flash_below_global_area_gate() {
        let width = 12usize;
        let height = 12usize;
        let mut mask = vec![0u8; width * height];
        for y in 1..3 {
            for x in 1..3 {
                mask[y * width + x] = 1;
            }
        }
        let global_ratio = 4.0 / (width * height) as f64;
        let local_ratio = max_visual_field_ratio(&mask, width, height);
        assert!(global_ratio < LARGE_AREA_RATIO);
        assert!((local_ratio - LARGE_AREA_RATIO).abs() < f64::EPSILON);
    }

    #[test]
    fn rolling_window_flags_four_candidate_events_and_slides() {
        let mut events = vec![false; 32];
        events[0] = true;
        events[10] = true;
        events[20] = true;
        events[30] = true;
        let summary = summarize_one_second_windows(&events, 30);
        assert_eq!(summary.windows_examined, 2);
        assert_eq!(summary.max_events, 4);
        assert_eq!(summary.windows_over_three_events, 1);
    }

    #[test]
    fn subsecond_capture_does_not_claim_complete_windows() {
        assert_eq!(
            summarize_one_second_windows(&vec![true; 30], 30),
            WindowSummary::default()
        );
    }
    #[test]
    fn timestamp_sidecar_requires_exact_count_and_strictly_increasing_values() {
        assert_eq!(
            parse_timestamps_ns("100\n200\n350\n", 3).expect("valid timeline"),
            vec![100, 200, 350]
        );
        assert!(parse_timestamps_ns("100\n100\n200\n", 3).is_err());
        assert!(parse_timestamps_ns("100\n200\n", 3).is_err());
        assert!(parse_timestamps_ns("100\n200\n300\n400\n", 3).is_err());
        assert!(parse_timestamps_ns("100\n-2\n300\n", 3).is_err());
    }

    #[test]
    fn opposing_edges_use_elapsed_time_not_frame_distance() {
        assert!(elapsed_within_window(10, 1_000_000_010, ONE_SECOND_NS));
        assert!(!elapsed_within_window(10, 1_000_000_011, ONE_SECOND_NS));
        assert!(!elapsed_within_window(20, 10, ONE_SECOND_NS));
    }

    #[test]
    fn timestamp_windows_count_events_inside_and_outside_one_second() {
        let inside_times = vec![0, 200_000_000, 400_000_000, 600_000_000, 800_000_000, 1_000_000_000];
        let mut inside_events = vec![false; inside_times.len()];
        inside_events[0] = true;
        inside_events[1] = true;
        inside_events[2] = true;
        inside_events[3] = true;
        let inside = summarize_one_second_windows_timed(&inside_times, &inside_events);
        assert_eq!(inside.max_events, 4);
        assert!(inside.windows_examined > 0);

        let outside_times = vec![0, 500_000_000, 1_000_000_001];
        let outside_events = vec![true, false, true];
        let outside = summarize_one_second_windows_timed(&outside_times, &outside_events);
        assert_eq!(outside.max_events, 1);
    }

    #[test]
    fn timestamp_windows_do_not_claim_full_window_before_one_second() {
        let times = vec![5_000_000_000, 5_100_000_000, 5_900_000_000];
        let events = vec![true, true, true];
        assert_eq!(
            summarize_one_second_windows_timed(&times, &events),
            WindowSummary::default()
        );
    }

    fn brute_force_timestamp_windows(timestamps_ns: &[u64], events: &[bool]) -> WindowSummary {
        if timestamps_ns.len() != events.len() || timestamps_ns.len() < 2 {
            return WindowSummary::default();
        }
        let mut result = WindowSummary::default();
        for right in 0..timestamps_ns.len() {
            if timestamps_ns[right] - timestamps_ns[0] < ONE_SECOND_NS {
                continue;
            }
            result.windows_examined += 1;
            let window_start = timestamps_ns[right].saturating_sub(ONE_SECOND_NS);
            let event_count = (0..=right)
                .filter(|&left| {
                    timestamps_ns[left] >= window_start
                        && timestamps_ns[left] <= timestamps_ns[right]
                        && events[left]
                })
                .count();
            result.max_events = result.max_events.max(event_count);
            if event_count > MAX_FLASH_EVENTS_PER_SECOND {
                result.windows_over_three_events += 1;
            }
        }
        result
    }

    #[test]
    fn optimized_timestamp_windows_match_independent_oracle_exhaustively() {
        // Exhaust every event mask for short timelines across uniform frame periods,
        // jittered presentation times, and sample gaps both below and above one second.
        // Alternate the timeline origin to catch accidental absolute-time assumptions,
        // including arithmetic near the u64 limit.
        let gap_patterns: &[&[u64]] = &[
            &[100_000_000],
            &[333_333_333],
            &[1, 999_999_999, 1, 1_000_000_001],
            &[450_000_000, 550_000_000, 100_000_000, 900_000_000],
        ];
        let mut cases = 0usize;
        for (pattern_index, gaps) in gap_patterns.iter().enumerate() {
            for len in 2..=9usize {
                let event_masks = 1usize << len;
                for mask in 0..event_masks {
                    let base = if (pattern_index + len) % 2 == 0 {
                        0
                    } else {
                        u64::MAX - 20_000_000_000
                    };
                    let mut timestamps_ns = Vec::with_capacity(len);
                    timestamps_ns.push(base);
                    for index in 1..len {
                        let next = timestamps_ns[index - 1]
                            .checked_add(gaps[(index - 1) % gaps.len()])
                            .expect("test timeline remains within u64 bounds");
                        timestamps_ns.push(next);
                    }
                    let events: Vec<bool> = (0..len).map(|index| mask & (1usize << index) != 0).collect();
                    let optimized = summarize_one_second_windows_timed(&timestamps_ns, &events);
                    let oracle = brute_force_timestamp_windows(&timestamps_ns, &events);
                    assert_eq!(
                        optimized, oracle,
                        "timestamp window mismatch: pattern={pattern_index} len={len} mask={mask} timeline={timestamps_ns:?} events={events:?}"
                    );
                    cases += 1;
                }
            }
        }
        assert_eq!(cases, 4 * (4 + 8 + 16 + 32 + 64 + 128 + 256 + 512));
    }

}
