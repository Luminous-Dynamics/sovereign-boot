// Copyright (C) 2024-2026 Tristan Stoltz / Luminous Dynamics
// SPDX-License-Identifier: AGPL-3.0-or-later
// Commercial licensing: see COMMERCIAL_LICENSE.md at repository root

//! Reproducible CPU reference-renderer benchmark.
//!
//! Measures scene update + pixel rendering only. It intentionally excludes DRM
//! setup, framebuffer mapping/blitting, compositor scheduling, and GPU execution.
//! It emits machine-readable JSON so results can be retained as benchmark evidence.

use std::time::Instant;
use std::fs;

use symthaea_quicken_fb::mycelium::MycelialNetwork;
use symthaea_quicken_fb::theme::{MotionMode, Theme};

const DEFAULT_WIDTH: u32 = 1920;
const DEFAULT_HEIGHT: u32 = 1080;
const DEFAULT_FRAMES: u32 = 120;
const WARMUP_FRAMES: u32 = 10;
const MAX_FRAMES: u32 = 3600;
const MAX_WIDTH: u32 = 3840;
const MAX_HEIGHT: u32 = 2160;

fn main() {
    let (width, height, frames, theme, motion, snapshot_path) = match parse_args() {
        Ok(Some(values)) => values,
        Ok(None) => return,
        Err(message) => {
            eprintln!("render_bench: {message}");
            print_usage();
            std::process::exit(2);
        }
    };

    if width == 0 || height == 0 || width > MAX_WIDTH || height > MAX_HEIGHT {
        eprintln!(
            "render_bench: width/height must be within 1..={MAX_WIDTH} by 1..={MAX_HEIGHT}"
        );
        std::process::exit(2);
    }
    let pixels = (width as u64) * (height as u64);
    if pixels > (MAX_WIDTH as u64) * (MAX_HEIGHT as u64) {
        eprintln!("render_bench: pixel budget exceeded");
        std::process::exit(2);
    }
    if frames == 0 || frames > MAX_FRAMES {
        eprintln!("render_bench: frames must be within 1..={MAX_FRAMES}");
        std::process::exit(2);
    }

    let seed = "sovereign-visual-benchmark-v1";
    let mut scene = MycelialNetwork::new_with_theme(width, height, seed, theme);
    scene.set_motion_scale(motion.scale());
    scene.set_scene_entry_fade_duration(motion.crossfade_ms() as f32 / 1_000.0);
    scene.set_particle_budget_scale(motion.particle_budget_scale());
    if motion == MotionMode::Reduced {
        scene.elapsed = 8.0;
    }
    let mut buffer = vec![0u32; pixels as usize];

    for _ in 0..WARMUP_FRAMES {
        scene.grow(1.0 / 30.0, 0.75);
        scene.render(&mut buffer);
    }

    let mut samples_ns = Vec::with_capacity(frames as usize);
    for _ in 0..frames {
        let start = Instant::now();
        scene.grow(1.0 / 30.0, 0.75);
        scene.render(&mut buffer);
        samples_ns.push(start.elapsed().as_nanos());
    }

    samples_ns.sort_unstable();
    let p50_ns = percentile(&samples_ns, 50);
    let p95_ns = percentile(&samples_ns, 95);
    let p99_ns = percentile(&samples_ns, 99);
    let min_ns = samples_ns[0];
    let max_ns = samples_ns[samples_ns.len() - 1];
    let fps_at_p95 = if p95_ns == 0 { 0.0 } else { 1_000_000_000.0 / p95_ns as f64 };
    let framebuffer_hash = match framebuffer_blake3(width, height, &buffer) {
        Ok(hash) => hash,
        Err(error) => {
            eprintln!("render_bench: cannot hash framebuffer: {error}");
            std::process::exit(1);
        }
    };

    let visual = match visual_metrics(width, height, &buffer) {
        Ok(metrics) => metrics,
        Err(error) => {
            eprintln!("render_bench: cannot audit framebuffer: {error}");
            std::process::exit(1);
        }
    };

    println!(
        concat!(
            "{{\n",
            "  \"schema\": \"cpu-render-bench-v1\",\n",
            "  \"source_commit\": \"{}\",\n",
            "  \"build_commit\": \"{}\",\n",
            "  \"measurement\": \"scene_update_plus_cpu_pixel_render\",\n",
            "  \"theme\": \"{}\",\n",
            "  \"motion_mode\": \"{}\",\n",
            "  \"resolution\": {{ \"width\": {}, \"height\": {} }},\n",
            "  \"warmup_frames\": {},\n",
            "  \"measured_frames\": {},\n",
            "  \"branch_count_after_run\": {},\n",
            "  \"framebuffer_blake3\": \"{}\",\n",
            "  \"frame_time_ms\": {{ \"min\": {:.3}, \"p50\": {:.3}, \"p95\": {:.3}, \"p99\": {:.3}, \"max\": {:.3} }},\n",
            "  \"fps_equivalent_at_p95\": {:.2},\n",
            "  \"visual_quality\": {{ \"quantized_color_bins\": {}, \"luminance_min\": {:.3}, \"luminance_max\": {:.3}, \"luminance_mean\": {:.3}, \"luminance_stddev\": {:.3}, \"mean_adjacent_luminance_delta\": {:.3} }},\n",
            "  \"includes_drm_or_display_wait\": false,\n",
            "  \"includes_gpu\": false\n",
            "}}"
        ),
        option_env!("VISUAL_SOURCE_COMMIT").unwrap_or("unavailable-local-build"),
        option_env!("VISUAL_BUILD_COMMIT").unwrap_or("unavailable-local-build"),
        theme.id(),
        motion.id(),
        width,
        height,
        WARMUP_FRAMES,
        frames,
        scene.branches.len(),
        framebuffer_hash,
        min_ns as f64 / 1_000_000.0,
        p50_ns as f64 / 1_000_000.0,
        p95_ns as f64 / 1_000_000.0,
        p99_ns as f64 / 1_000_000.0,
        max_ns as f64 / 1_000_000.0,
        fps_at_p95,
        visual.quantized_color_bins,
        visual.luminance_min,
        visual.luminance_max,
        visual.luminance_mean,
        visual.luminance_stddev,
        visual.mean_adjacent_luminance_delta,
    );

    if let Some(path) = snapshot_path {
        match ppm_bytes(width, height, &buffer) {
            Ok(bytes) => {
                if let Err(error) = fs::write(&path, bytes) {
                    eprintln!("render_bench: failed to write snapshot {path}: {error}");
                    std::process::exit(1);
                }
            }
            Err(error) => {
                eprintln!("render_bench: snapshot refused: {error}");
                std::process::exit(1);
            }
        }
    }
}

/// Low-cost structure metrics for rejecting degenerate captures.
///
/// These are diagnostic signals, not a perceptual-quality score. Quantized
/// colors use 4 bits per RGB channel so memory stays bounded at 4,096 bins,
/// even when the frame is 4K. Luminance uses sRGB code values for stability.
#[derive(Clone, Copy, Debug, PartialEq)]
struct VisualMetrics {
    quantized_color_bins: usize,
    luminance_min: f64,
    luminance_max: f64,
    luminance_mean: f64,
    luminance_stddev: f64,
    mean_adjacent_luminance_delta: f64,
}

fn pixel_luminance(pixel: u32) -> f64 {
    let red = ((pixel >> 16) & 0xff) as f64;
    let green = ((pixel >> 8) & 0xff) as f64;
    let blue = (pixel & 0xff) as f64;
    0.2126 * red + 0.7152 * green + 0.0722 * blue
}

fn visual_metrics(width: u32, height: u32, pixels: &[u32]) -> Result<VisualMetrics, String> {
    let count = (width as usize)
        .checked_mul(height as usize)
        .ok_or("visual metric pixel count overflow")?;
    if width == 0 || height == 0 || pixels.len() < count {
        return Err("visual metric dimensions or framebuffer length are invalid".into());
    }

    let mut color_bins = [false; 4_096];
    let mut color_bin_count = 0usize;
    let mut luminance_min = f64::INFINITY;
    let mut luminance_max = f64::NEG_INFINITY;
    let mut luminance_sum = 0.0f64;
    let mut luminance_square_sum = 0.0f64;

    for &pixel in pixels.iter().take(count) {
        let red = ((pixel >> 16) & 0xff) as usize;
        let green = ((pixel >> 8) & 0xff) as usize;
        let blue = (pixel & 0xff) as usize;
        let bin = ((red >> 4) << 8) | ((green >> 4) << 4) | (blue >> 4);
        if !color_bins[bin] {
            color_bins[bin] = true;
            color_bin_count += 1;
        }

        let luminance = pixel_luminance(pixel);
        luminance_min = luminance_min.min(luminance);
        luminance_max = luminance_max.max(luminance);
        luminance_sum += luminance;
        luminance_square_sum += luminance * luminance;
    }

    let sample_count = count as f64;
    let luminance_mean = luminance_sum / sample_count;
    let luminance_variance = (luminance_square_sum / sample_count
        - luminance_mean * luminance_mean)
        .max(0.0);
    let luminance_stddev = luminance_variance.sqrt();

    let mut edge_delta_sum = 0.0f64;
    let mut edge_count = 0u64;
    for y in 0..height as usize {
        for x in 0..width as usize {
            let index = y * width as usize + x;
            let current = pixel_luminance(pixels[index]);
            if x + 1 < width as usize {
                edge_delta_sum += (current - pixel_luminance(pixels[index + 1])).abs();
                edge_count += 1;
            }
            if y + 1 < height as usize {
                edge_delta_sum +=
                    (current - pixel_luminance(pixels[index + width as usize])).abs();
                edge_count += 1;
            }
        }
    }

    Ok(VisualMetrics {
        quantized_color_bins: color_bin_count,
        luminance_min,
        luminance_max,
        luminance_mean,
        luminance_stddev,
        mean_adjacent_luminance_delta: if edge_count == 0 {
            0.0
        } else {
            edge_delta_sum / edge_count as f64
        },
    })
}

/// Hash pixel channels in a stable RGB byte order, excluding stride/padding.
fn framebuffer_blake3(width: u32, height: u32, pixels: &[u32]) -> Result<String, String> {


    let count = (width as usize)
        .checked_mul(height as usize)
        .ok_or("framebuffer pixel count overflow")?;
    if width == 0 || height == 0 || pixels.len() < count {
        return Err("framebuffer dimensions or length are invalid".into());
    }

    let mut hasher = blake3::Hasher::new();
    for pixel in pixels.iter().take(count) {
        hasher.update(&[
            ((pixel >> 16) & 0xff) as u8,
            ((pixel >> 8) & 0xff) as u8,
            (pixel & 0xff) as u8,
        ]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn ppm_bytes(width: u32, height: u32, pixels: &[u32]) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err("snapshot dimensions must be positive".into());
    }
    let count = (width as usize)
        .checked_mul(height as usize)
        .ok_or("snapshot pixel count overflow")?;
    if pixels.len() < count {
        return Err("snapshot framebuffer is smaller than width × height".into());
    }

    let header = format!("P6\n{width} {height}\n255\n");
    let mut output = Vec::with_capacity(header.len() + count * 3);
    output.extend_from_slice(header.as_bytes());
    for pixel in pixels.iter().take(count) {
        output.push(((pixel >> 16) & 0xff) as u8);
        output.push(((pixel >> 8) & 0xff) as u8);
        output.push((pixel & 0xff) as u8);
    }
    Ok(output)
}

fn percentile(sorted: &[u128], percentile: usize) -> u128 {
    debug_assert!(!sorted.is_empty());
    let last = sorted.len() - 1;
    let index = (last * percentile).div_ceil(100);
    sorted[index.min(last)]
}

fn parse_args() -> Result<Option<(u32, u32, u32, Theme, MotionMode, Option<String>)>, String> {
    let args: Vec<String> = std::env::args().collect();
    parse_args_from(&args)
}

fn parse_args_from(args: &[String]) -> Result<Option<(u32, u32, u32, Theme, MotionMode, Option<String>)>, String> {
    let mut width = DEFAULT_WIDTH;
    let mut height = DEFAULT_HEIGHT;
    let mut frames = DEFAULT_FRAMES;
    let mut theme = Theme::MycelialGenesis;
    let mut motion = MotionMode::Cinematic;
    let mut snapshot_path = None;
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "--width" => {
                i += 1;
                width = parse_u32_arg(&args, i, "--width")?;
            }
            "--height" => {
                i += 1;
                height = parse_u32_arg(&args, i, "--height")?;
            }
            "--frames" => {
                i += 1;
                frames = parse_u32_arg(&args, i, "--frames")?;
            }
            "--theme" => {
                i += 1;
                let id = args.get(i).ok_or("--theme requires a stable theme ID")?;
                theme = Theme::from_id(id)
                    .ok_or_else(|| format!("unknown theme ID: {id}"))?;
            }
            "--motion" => {
                i += 1;
                let id = args.get(i).ok_or("--motion requires a stable motion-mode ID")?;
                motion = MotionMode::from_id(id)
                    .ok_or_else(|| format!("unknown motion mode: {id}"))?;
            }
            "--snapshot" => {
                i += 1;
                let path = args.get(i).ok_or("--snapshot requires an output path")?;
                if path.is_empty() {
                    return Err("--snapshot path must not be empty".into());
                }
                snapshot_path = Some(path.clone());
            }
            "--help" | "-h" => {
                print_usage();
                return Ok(None);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
        i += 1;
    }

    Ok(Some((width, height, frames, theme, motion, snapshot_path)))
}

fn parse_u32_arg(args: &[String], index: usize, name: &str) -> Result<u32, String> {
    args.get(index)
        .ok_or_else(|| format!("{name} requires a value"))?
        .parse::<u32>()
        .map_err(|_| format!("{name} must be a positive integer"))
}

fn print_usage() {
    eprintln!(
        "Usage: cargo run --release --example render_bench -- [OPTIONS]\n  --theme <ID>   Theme ID (default: mycelial-genesis)\n  --motion <ID>  reduced|low-power|balanced|cinematic\n  --width <N>    Width, up to 3840 (default: 1920)\n  --height <N>   Height, up to 2160 (default: 1080)\n  --frames <N>   Measured frames, 1..=3600 (default: 120)\n  --snapshot <P> Optional P6 PPM capture path\n  --help         Show this help"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_is_bounded_and_uses_sorted_samples() {
        let values = [10, 20, 30, 40, 50];
        assert_eq!(percentile(&values, 0), 10);
        assert_eq!(percentile(&values, 50), 30);
        assert_eq!(percentile(&values, 95), 50);
        assert_eq!(percentile(&values, 100), 50);
    }

    #[test]
    fn visual_metrics_detect_a_uniform_frame() {
        let pixels = [0x0012_3456; 4];
        let metrics = visual_metrics(2, 2, &pixels).expect("valid frame");
        assert_eq!(metrics.quantized_color_bins, 1);
        assert_eq!(metrics.luminance_min, metrics.luminance_max);
        assert_eq!(metrics.luminance_stddev, 0.0);
        assert_eq!(metrics.mean_adjacent_luminance_delta, 0.0);
    }

    #[test]
    fn visual_metrics_measure_spatial_contrast_with_bounded_color_storage() {
        let pixels = [0x0000_0000, 0x00ff_ffff, 0x00ff_0000, 0x0000_00ff];
        let metrics = visual_metrics(2, 2, &pixels).expect("valid frame");
        assert!(metrics.quantized_color_bins >= 3);
        assert!(metrics.luminance_min < metrics.luminance_max);
        assert!(metrics.luminance_stddev > 0.0);
        assert!(metrics.mean_adjacent_luminance_delta > 0.0);
    }

    #[test]
    fn visual_metrics_reject_invalid_dimensions_or_short_buffer() {
        assert!(visual_metrics(0, 1, &[0]).is_err());
        assert!(visual_metrics(2, 2, &[0]).is_err());
    }

    #[test]
    fn parser_accepts_a_known_theme() {
        let args = vec![
            "render_bench".to_string(),
            "--theme".to_string(),
            "cosmic-atlas".to_string(),
        ];
        let parsed = parse_args_from(&args)
            .expect("valid arguments")
            .expect("help was not requested");
        assert_eq!(parsed.3, Theme::CosmicAtlas);
        assert_eq!(parsed.4, MotionMode::Cinematic);
    }

    #[test]
    fn parser_accepts_reduced_motion_mode() {
        let args = vec![
            "render_bench".to_string(),
            "--motion".to_string(),
            "reduced".to_string(),
        ];
        let parsed = parse_args_from(&args)
            .expect("valid arguments")
            .expect("help was not requested");
        assert_eq!(parsed.4, MotionMode::Reduced);
    }

    #[test]
    fn ppm_capture_has_a_valid_header_and_rgb_order() {
        let bytes = ppm_bytes(1, 1, &[0x0012_3456]).expect("valid one-pixel frame");
        assert!(bytes.starts_with(b"P6\n1 1\n255\n"));
        assert_eq!(&bytes[11..], &[0x12, 0x34, 0x56]);
    }

    #[test]
    fn ppm_capture_rejects_short_framebuffers() {
        assert!(ppm_bytes(2, 2, &[0]).is_err());
    }

    #[test]
    fn framebuffer_hash_is_deterministic_and_dimension_checked() {
        let pixels = [0x0012_3456, 0x00ab_cdef];
        let first = framebuffer_blake3(2, 1, &pixels).expect("valid framebuffer");
        let second = framebuffer_blake3(2, 1, &pixels).expect("same framebuffer");
        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
        assert!(framebuffer_blake3(3, 1, &pixels).is_err());
    }
}
