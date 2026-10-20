// Copyright (C) 2024-2026 Tristan Stoltz / Luminous Dynamics
// SPDX-License-Identifier: AGPL-3.0-or-later

#![forbid(unsafe_code)]

//! Headless offscreen reference renderer for the Visual Genome WGSL shader.
//! It is a capture/profile utility, not a desktop compositor or boot dependency.
//! Frames intentionally replay the same scene inputs; this measures throughput,
//! not display cadence, v-sync, or the physical presentation path.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::process;
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};

const MANIFEST: &str = include_str!("../../../themes/visual-genome-v1.json");
const SHADER: &str = include_str!("../../../renderers/wgpu/shaders/mycelial_field.wgsl");
const BASELINE_SHADER: &str = include_str!("../fixtures/mycelial_field_pre_specialization.wgsl");
const MAX_WIDTH: u32 = 3840;
const MAX_HEIGHT: u32 = 2160;
const MAX_PIXELS: u64 = 8_294_400;
const MAX_FRAMES: u32 = 1800;
const ROW_ALIGNMENT: u32 = 256;

#[derive(Debug)]
struct Config {
    width: u32,
    height: u32,
    frames: u32,
    theme: String,
    motion: String,
    time: f32,
    output: PathBuf,
    backend: Backend,
    fallback: bool,
    neutral_palette: bool,
    shader_profile: ShaderProfile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShaderProfile { Baseline, Specialized }

impl ShaderProfile {
    fn id(self) -> &'static str {
        match self {
            Self::Baseline => "baseline-pre-specialization",
            Self::Specialized => "specialized-family-work",
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Backend { Vulkan, All }

#[derive(Clone, Copy, Debug)]
struct Palette<'a> {
    background: [f32; 4],
    primary: [f32; 4],
    secondary: [f32; 4],
    node: [f32; 4],
    convergence: [f32; 4],
    scene_family_index: u32,
    scene_family: &'a str,
}

fn main() {
    let config = match parse_args(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(Some(config)) => config,
        Ok(None) => return,
        Err(error) => {
            eprintln!("visual-genome-wgpu-reference: {error}");
            print_usage();
            process::exit(2);
        }
    };
    if let Err(error) = pollster::block_on(render(config)) {
        eprintln!("visual-genome-wgpu-reference: {error}");
        process::exit(1);
    }
}

async fn render(config: Config) -> Result<(), String> {
    let manifest: Value =
        serde_json::from_str(MANIFEST).map_err(|error| format!("invalid bundled manifest: {error}"))?;
    let parsed_palette = palette_for_theme(&manifest, &config.theme)?;
    let palette = if config.neutral_palette {
        neutral_benchmark_palette(parsed_palette)
    } else {
        parsed_palette
    };
    let motion_profile = &manifest["motion_profiles"][&config.motion];
    let motion = motion_profile["motion_scale"]
        .as_f64()
        .ok_or_else(|| format!("unknown motion profile '{}'", config.motion))? as f32;
    let particle_budget_scale = motion_profile["particle_budget_scale"]
        .as_f64()
        .ok_or_else(|| format!("motion profile '{}' has no particle budget", config.motion))? as f32;
    let crossfade_ms = motion_profile["crossfade_ms"]
        .as_u64()
        .ok_or_else(|| format!("motion profile '{}' has no crossfade duration", config.motion))?;
    if !motion.is_finite() || !(0.0..=1.0).contains(&motion)
        || !particle_budget_scale.is_finite()
        || !(0.0..=1.0).contains(&particle_budget_scale)
        || !(50..=2000).contains(&crossfade_ms)
    {
        return Err(format!("motion profile '{}' is outside the renderer's bounded contract", config.motion));
    }

    let setup_started = std::time::Instant::now();
    let backends = match config.backend {
        Backend::Vulkan => wgpu::Backends::VULKAN,
        Backend::All => wgpu::Backends::all(),
    };
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        ..wgpu::InstanceDescriptor::new_without_display_handle_from_env()
    });
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: config.fallback,
            compatible_surface: None,
            apply_limit_buckets: false,
        })
        .await
        .map_err(|error| format!("no matching adapter: {error}"))?;
    let info = adapter.get_info();
    let supports_timestamps = adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
    let required_features = adapter.features() & wgpu::Features::TIMESTAMP_QUERY;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("Visual Genome offscreen reference"),
            required_features,
            required_limits: adapter.limits(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
        })
        .await
        .map_err(|error| format!("device request failed: {error}"))?;

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Visual Genome sRGB target"),
        size: wgpu::Extent3d {
            width: config.width,
            height: config.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let shader_source = match config.shader_profile {
        ShaderProfile::Baseline => BASELINE_SHADER,
        ShaderProfile::Specialized => SHADER,
    };
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Visual Genome procedural filaments"),
        source: wgpu::ShaderSource::Wgsl(shader_source.into()),
    });
    let targets = [Some(wgpu::ColorTargetState {
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    })];
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Visual Genome offscreen pipeline"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &targets,
        }),
        multiview_mask: None,
        cache: None,
    });
    let uniform_values = uniform_values(
        config.width,
        config.height,
        config.time,
        motion,
        particle_budget_scale,
        palette,
    );
    let uniform_bytes = floats_as_le_bytes(&uniform_values);
    if uniform_values.len() != 28 || uniform_bytes.len() != 112 {
        return Err("host uniform layout must be exactly 28 float32 values (112 bytes)".to_string());
    }
    let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Visual Genome uniforms"),
        size: uniform_bytes.len() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&uniform_buffer, 0, &uniform_bytes);
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Visual Genome uniform bindings"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buffer.as_entire_binding(),
        }],
    });

    let unpadded_row_bytes = config.width.checked_mul(4).ok_or("row byte count overflow")?;
    let padded_row_bytes = align_up(unpadded_row_bytes, ROW_ALIGNMENT)?;
    let capture_buffer_size = u64::from(padded_row_bytes)
        .checked_mul(u64::from(config.height))
        .ok_or("capture size overflow")?;
    let capture_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Visual Genome pixel readback"),
        size: capture_buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let query_count = config.frames.checked_mul(2).ok_or("timestamp count overflow")?;
    let query_set = supports_timestamps.then(|| device.create_query_set(&wgpu::QuerySetDescriptor {
        label: Some("Visual Genome GPU timestamps"),
        ty: wgpu::QueryType::Timestamp,
        count: query_count,
    }));
    let query_size = u64::from(query_count) * std::mem::size_of::<u64>() as u64;
    let query_resolve = supports_timestamps.then(|| device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("GPU timestamp resolve"),
        size: query_size,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    }));
    let query_buffer = supports_timestamps.then(|| device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("GPU timestamp readback"),
        size: query_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    }));
    let setup_and_pipeline_ms = setup_started.elapsed().as_secs_f64() * 1000.0;

    let whole_started = std::time::Instant::now();
    let submit_started = std::time::Instant::now();
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Repeated offscreen render workload"),
    });
    for frame in 0..config.frames {
        let timestamp_writes = query_set.as_ref().map(|set| wgpu::RenderPassTimestampWrites {
            query_set: set,
            beginning_of_pass_write_index: Some(frame * 2),
            end_of_pass_write_index: Some(frame * 2 + 1),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Visual Genome fullscreen pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &capture_buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row_bytes),
                rows_per_image: Some(config.height),
            },
        },
        wgpu::Extent3d {
            width: config.width,
            height: config.height,
            depth_or_array_layers: 1,
        },
    );
    if let (Some(set), Some(resolve), Some(readback)) =
        (query_set.as_ref(), query_resolve.as_ref(), query_buffer.as_ref())
    {
        encoder.resolve_query_set(set, 0..query_count, resolve, 0);
        encoder.copy_buffer_to_buffer(resolve, 0, readback, 0, query_size);
    }
    let submission = queue.submit([encoder.finish()]);
    let cpu_encode_submit_ms = submit_started.elapsed().as_secs_f64() * 1000.0;

    let (capture_tx, capture_rx) = mpsc::channel();
    capture_buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
        let _ = capture_tx.send(result.map_err(|error| error.to_string()));
    });
    let timestamp_rx = query_buffer.as_ref().map(|buffer| {
        let (tx, rx) = mpsc::channel();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result.map_err(|error| error.to_string()));
        });
        rx
    });
    device.poll(wgpu::PollType::Wait {
        submission_index: Some(submission),
        timeout: Some(Duration::from_secs(120)),
    }).map_err(|error| format!("GPU completion failed: {error}"))?;
    capture_rx.recv_timeout(Duration::from_secs(5))
        .map_err(|error| format!("capture mapping callback timed out: {error}"))?
        .map_err(|error| format!("capture mapping failed: {error}"))?;
    if let Some(rx) = timestamp_rx {
        rx.recv_timeout(Duration::from_secs(5))
            .map_err(|error| format!("timestamp mapping callback timed out: {error}"))?
            .map_err(|error| format!("timestamp mapping failed: {error}"))?;
    }

    let ppm = {
        let mapped = capture_buffer.slice(..).get_mapped_range().map_err(|error| format!("could not access captured pixels: {error}"))?;
        let mut output = Vec::with_capacity(32 + (u64::from(config.width) * u64::from(config.height) * 3) as usize);
        output.extend_from_slice(format!("P6\n{} {}\n255\n", config.width, config.height).as_bytes());
        for y in 0..config.height as usize {
            let start = y * padded_row_bytes as usize;
            let row = &mapped[start..start + unpadded_row_bytes as usize];
            for rgba in row.chunks_exact(4) {
                output.extend_from_slice(&rgba[..3]);
            }
        }
        drop(mapped);
        output
    };
    capture_buffer.unmap();
    let header_len = format!("P6\n{} {}\n255\n", config.width, config.height).len();
    let capture_structure = capture_structure(&ppm[header_len..], config.width, config.height)?;
    fs::write(&config.output, &ppm).map_err(|error| format!("could not write PPM: {error}"))?;

    let gpu_ms = if let Some(buffer) = query_buffer.as_ref() {
        let mapped = buffer.slice(..).get_mapped_range().map_err(|error| format!("could not access GPU timestamps: {error}"))?;
        let period_ns = f64::from(queue.get_timestamp_period());
        let mut samples = Vec::with_capacity(config.frames as usize);
        for frame in 0..config.frames as usize {
            let offset = frame * 16;
            let start = u64::from_le_bytes(mapped[offset..offset + 8].try_into().expect("8 bytes"));
            let end = u64::from_le_bytes(mapped[offset + 8..offset + 16].try_into().expect("8 bytes"));
            let elapsed = end.wrapping_sub(start) as f64 * period_ns / 1_000_000.0;
            if elapsed.is_finite() && elapsed >= 0.0 {
                samples.push(elapsed);
            }
        }
        drop(mapped);
        buffer.unmap();
        samples
    } else {
        Vec::new()
    };
    let wall_ms = whole_started.elapsed().as_secs_f64() * 1000.0;
    let receipt = json!({
        "schema": "wgpu-offscreen-reference-v2",
        "source_commit": option_env!("VISUAL_SOURCE_COMMIT").unwrap_or("unavailable-local-build"),
        "build_commit": option_env!("VISUAL_BUILD_COMMIT").unwrap_or("unavailable-local-build"),
        "measurement": "replayed_offscreen_passes_one_command_submission",
        "shader_profile": config.shader_profile.id(),
        "scene_state_is_replayed_identically": true,
        "timestamp_query_supported": supports_timestamps,
        "timestamp_query_samples": gpu_ms.len(),
        "gpu_execution_ms": distribution(&gpu_ms),
        "cpu_record_and_submit_ms": cpu_encode_submit_ms,
        "end_to_end_render_and_readback_ms": wall_ms,
        "renderer_setup_and_pipeline_ms": setup_and_pipeline_ms,
        "adapter": {
            "name": info.name,
            "vendor_id": info.vendor,
            "device_id": info.device,
            "device_type": format!("{:?}", info.device_type),
            "driver": info.driver,
            "driver_info": info.driver_info,
            "backend": format!("{:?}", info.backend)
        },
        "resolution": {"width": config.width, "height": config.height},
        "frames_requested": config.frames,
        "theme": config.theme,
        "scene_family": palette.scene_family,
        "scene_family_index": palette.scene_family_index,
        "neutral_palette": config.neutral_palette,
        "motion_mode": config.motion,
        "motion_scale": motion,
        "particle_budget_scale": particle_budget_scale,
        "crossfade_ms": crossfade_ms,
        "crossfade_applied": false,
        "time_seconds": config.time,
        "output_format": "P6 PPM decoded from an Rgba8UnormSrgb offscreen texture",
        "capture_structure": capture_structure,
        "normative_wcag_conformance": false
    });
    println!("{}", serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?);
    Ok(())
}


fn capture_structure(rgb: &[u8], width: u32, height: u32) -> Result<Value, String> {
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or("capture pixel count overflow")?;
    if pixels == 0 || pixels.checked_mul(3) != Some(rgb.len() as u64) {
        return Err("capture RGB byte count does not match dimensions".into());
    }

    let mut bins = HashSet::with_capacity(256);
    let mut min_luminance = f64::INFINITY;
    let mut max_luminance = f64::NEG_INFINITY;
    let mut mean = 0.0f64;
    let mut sum_squared_deltas = 0.0f64;
    let mut horizontal_delta_sum = 0.0f64;
    let mut horizontal_pairs = 0u64;
    let mut previous_luminance = 0.0f64;

    for index in 0..pixels as usize {
        let offset = index * 3;
        let red = rgb[offset];
        let green = rgb[offset + 1];
        let blue = rgb[offset + 2];
        let key = ((red >> 4) as u16) << 8
            | ((green >> 4) as u16) << 4
            | (blue >> 4) as u16;
        bins.insert(key);

        let r = f64::from(linear(red));
        let g = f64::from(linear(green));
        let b = f64::from(linear(blue));
        let luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        min_luminance = min_luminance.min(luminance);
        max_luminance = max_luminance.max(luminance);
        let sample_count = (index + 1) as f64;
        let delta = luminance - mean;
        mean += delta / sample_count;
        sum_squared_deltas += delta * (luminance - mean);

        if index % width as usize != 0 {
            horizontal_delta_sum += (luminance - previous_luminance).abs();
            horizontal_pairs += 1;
        }
        previous_luminance = luminance;
    }

    Ok(json!({
        "measurement": "captured_srgb_rgb_raster_structure_not_perceptual_quality",
        "quantized_color_bins": bins.len(),
        "luminance_min": min_luminance,
        "luminance_max": max_luminance,
        "luminance_mean": mean,
        "luminance_stddev": (sum_squared_deltas / pixels as f64).sqrt(),
        "mean_horizontal_luminance_delta": if horizontal_pairs == 0 { 0.0 } else { horizontal_delta_sum / horizontal_pairs as f64 }
    }))
}

fn palette_for_theme<'a>(manifest: &'a Value, id: &str) -> Result<Palette<'a>, String> {
    if manifest["schema_version"].as_u64() != Some(1) {
        return Err("unsupported manifest schema".into());
    }
    let themes = manifest["themes"].as_array().ok_or("manifest has no themes array")?;
    let entry = themes.iter().find(|t| t["id"].as_str() == Some(id))
        .ok_or_else(|| format!("unknown theme '{id}'"))?;
    let p = &entry["palette"];
    let scene_family = entry["scene_family"].as_str().ok_or("missing scene family")?;
    let scene_family_index = u32::try_from(
        entry["shader_variant"].as_u64().ok_or("missing shader variant")?
    ).map_err(|_| "shader variant must fit in u32")?;
    if scene_family_index > 9 {
        return Err("shader variant must be between 0 and 9".into());
    }
    Ok(Palette {
        background: parse_color(p["background"].as_str().ok_or("missing background palette")?, 1.0)?,
        primary: parse_color(p["primary"].as_str().ok_or("missing primary palette")?, 1.0)?,
        secondary: parse_color(p["secondary"].as_str().ok_or("missing secondary palette")?, 1.0)?,
        node: parse_color(p["node"].as_str().ok_or("missing node palette")?, 1.0)?,
        convergence: parse_color(p["convergence"].as_str().ok_or("missing convergence palette")?, 0.0)?,
        scene_family_index,
        scene_family,
    })
}

// Fixed linear-light colors isolate scene geometry from theme palette differences.
fn neutral_benchmark_palette<'a>(mut palette: Palette<'a>) -> Palette<'a> {
    palette.background = [0.0033, 0.0052, 0.0123, 1.0];
    palette.primary = [0.1620, 0.7519, 0.8796, 1.0];
    palette.secondary = [0.0273, 0.1221, 0.2462, 1.0];
    palette.node = [1.0, 0.6105, 0.2159, 1.0];
    palette.convergence = [0.0, 0.0, 0.0, 0.0];
    palette
}

fn parse_color(text: &str, alpha: f32) -> Result<[f32; 4], String> {
    let b = text.as_bytes();
    if b.len() != 7 || b[0] != b'#' { return Err("palette color must be #RRGGBB".into()); }
    let parse = |part: &[u8]| -> Result<u8, String> {
        let digits = std::str::from_utf8(part).map_err(|_| "palette color isn't ASCII")?;
        u8::from_str_radix(digits, 16).map_err(|_| "palette color contains non-hex digits".into())
    };
    let r = parse(&b[1..3])?;
    let g = parse(&b[3..5])?;
    let blue = parse(&b[5..7])?;
    Ok([linear(r), linear(g), linear(blue), alpha])
}

fn linear(channel: u8) -> f32 {
    let v = f32::from(channel) / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn uniform_values(
    w: u32,
    h: u32,
    time: f32,
    motion: f32,
    particle_budget_scale: f32,
    p: Palette,
) -> Vec<f32> {
    let mut v = Vec::with_capacity(28);
    v.extend([w as f32, h as f32, time, motion]);
    v.extend(p.background);
    v.extend(p.primary);
    v.extend(p.secondary);
    v.extend(p.node);
    v.extend(p.convergence);
    // Keep the 112-byte vec4-aligned ABI; y carries the particle budget.
    v.extend([p.scene_family_index as f32, particle_budget_scale, 0.0, 0.0]);
    v
}

fn floats_as_le_bytes(values: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for value in values { bytes.extend_from_slice(&value.to_le_bytes()); }
    bytes
}

fn align_up(value: u32, alignment: u32) -> Result<u32, String> {
    if alignment == 0 { return Err("zero alignment".into()); }
    let v = value.checked_add(alignment - 1).ok_or("alignment overflow")?;
    Ok(v / alignment * alignment)
}

fn distribution(samples: &[f64]) -> Value {
    if samples.is_empty() {
        return json!({"samples":0,"p50_ms":null,"p95_ms":null,"p99_ms":null,"max_ms":null});
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    json!({
        "samples": sorted.len(),
        "p50_ms": percentile(&sorted, 0.50),
        "p95_ms": percentile(&sorted, 0.95),
        "p99_ms": percentile(&sorted, 0.99),
        "max_ms": sorted.last().copied()
    })
}

fn percentile(sorted: &[f64], q: f64) -> Option<f64> {
    if sorted.is_empty() || !q.is_finite() || !(0.0..=1.0).contains(&q) { return None; }
    let index = ((sorted.len() as f64 * q).ceil() as usize).saturating_sub(1).min(sorted.len()-1);
    Some(sorted[index])
}

fn parse_args(args: &[String]) -> Result<Option<Config>, String> {
    let mut c = Config {
        width: 640, height: 360, frames: 120,
        theme: "mycelial-genesis".into(), motion: "balanced".into(), time: 1.0,
        output: PathBuf::new(), backend: Backend::Vulkan, fallback: false,
        neutral_palette: false, shader_profile: ShaderProfile::Specialized
    };
    let mut has_output = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--width" => { i+=1; c.width=parse_int(args,i,"--width")?; }
            "--height" => { i+=1; c.height=parse_int(args,i,"--height")?; }
            "--frames" => { i+=1; c.frames=parse_int(args,i,"--frames")?; }
            "--theme" => { i+=1; c.theme=args.get(i).ok_or("--theme requires an ID")?.clone(); }
            "--motion" => { i+=1; c.motion=args.get(i).ok_or("--motion requires a mode")?.clone(); }
            "--time" => {
                i+=1; c.time=args.get(i).ok_or("--time requires seconds")?.parse().map_err(|_| "invalid --time")?;
            }
            "--output" => { i+=1; c.output=PathBuf::from(args.get(i).ok_or("--output requires a path")?); has_output=true; }
            "--backend" => {
                i+=1;
                c.backend=match args.get(i).map(String::as_str) {
                    Some("vulkan")=>Backend::Vulkan, Some("all")=>Backend::All,
                    _=>return Err("--backend must be vulkan or all".into())
                };
            }
            "--force-fallback" => c.fallback=true,
            "--neutral-palette" => c.neutral_palette=true,
            "--shader-profile" => {
                i+=1;
                c.shader_profile=match args.get(i).map(String::as_str) {
                    Some("baseline") => ShaderProfile::Baseline,
                    Some("specialized") => ShaderProfile::Specialized,
                    _ => return Err("--shader-profile must be baseline or specialized".into())
                };
            }
            "--help" | "-h" => { print_usage(); return Ok(None); }
            unknown if unknown.starts_with('-') => return Err(format!("unknown option {unknown}")),
            value => return Err(format!("unexpected argument {value}")),
        }
        i+=1;
    }
    if c.width==0||c.width>MAX_WIDTH||c.height==0||c.height>MAX_HEIGHT {
        return Err(format!("dimensions must be within 1..={MAX_WIDTH} x 1..={MAX_HEIGHT}"));
    }
    if u64::from(c.width)*u64::from(c.height)>MAX_PIXELS { return Err("pixel budget exceeded".into()); }
    if c.frames==0||c.frames>MAX_FRAMES { return Err(format!("frames must be 1..={MAX_FRAMES}")); }
    if !c.time.is_finite()||!(0.0..=86_400.0).contains(&c.time) { return Err("--time must be 0..=86400".into()); }
    if !has_output { return Err("--output is required".into()); }
    let manifest: Value = serde_json::from_str(MANIFEST).map_err(|e| e.to_string())?;
    palette_for_theme(&manifest, &c.theme)?;
    if manifest["motion_profiles"][&c.motion]["motion_scale"].as_f64().is_none() {
        return Err(format!("unknown motion mode '{}'",c.motion));
    }
    Ok(Some(c))
}

fn parse_int(args: &[String], i: usize, name: &str) -> Result<u32,String> {
    args.get(i).ok_or_else(|| format!("{name} requires a value"))?.parse().map_err(|_| format!("{name} must be an unsigned integer"))
}

fn print_usage() {
    eprintln!("Usage: wgpu-reference --output capture.ppm [--backend vulkan|all] [--force-fallback] [--neutral-palette] [--shader-profile baseline|specialized] [--width N] [--height N] [--frames N] [--theme ID] [--motion MODE] [--time SECONDS]");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_structure_rejects_wrong_raster_length() {
        assert!(capture_structure(&[0, 0, 0], 2, 1).is_err());
    }

    #[test]
    fn capture_structure_rejects_uniform_image_as_non_structural() {
        let rgb = [16u8, 24, 32].repeat(8);
        let metrics = capture_structure(&rgb, 4, 2).unwrap();
        assert_eq!(metrics["quantized_color_bins"].as_u64(), Some(1));
        assert_eq!(metrics["luminance_min"], metrics["luminance_max"]);
        assert_eq!(metrics["luminance_stddev"].as_f64(), Some(0.0));
    }

    #[test]
    fn capture_structure_counts_distinct_bins_and_luminance_range() {
        let rgb = [0, 0, 0, 255, 255, 255];
        let metrics = capture_structure(&rgb, 2, 1).unwrap();
        assert_eq!(metrics["quantized_color_bins"].as_u64(), Some(2));
        assert!(metrics["luminance_max"].as_f64().unwrap() > metrics["luminance_min"].as_f64().unwrap());
        assert!(metrics["luminance_stddev"].as_f64().unwrap() > 0.0);
    }

    #[test]
    fn converts_srgb_to_linear_light() {
        assert_eq!(linear(0), 0.0);
        assert_eq!(linear(255), 1.0);
        assert!((linear(128)-0.21586).abs()<0.0001);
    }

    #[test]
    fn maps_manifest_theme_palettes_and_motion() {
        let m: Value = serde_json::from_str(MANIFEST).unwrap();
        let p=palette_for_theme(&m,"mycelial-genesis").unwrap();
        assert_eq!(p.background[3],1.0);
        assert_eq!(p.convergence[3],0.0);
        assert_eq!(m["motion_profiles"]["reduced"]["motion_scale"].as_f64(),Some(0.0));
        assert_eq!(m["motion_profiles"]["reduced"]["particle_budget_scale"].as_f64(),Some(0.0));
        assert_eq!(m["motion_profiles"]["low-power"]["particle_budget_scale"].as_f64(),Some(0.25));
        assert_eq!(m["motion_profiles"]["balanced"]["particle_budget_scale"].as_f64(),Some(0.6));
        assert_eq!(m["motion_profiles"]["cinematic"]["particle_budget_scale"].as_f64(),Some(1.0));
    }

    #[test]
    fn budget_uniform_preserves_layout_and_is_bounded_by_manifest_profiles() {
        let manifest: Value = serde_json::from_str(MANIFEST).unwrap();
        for (mode, expected) in [
            ("reduced", 0.0),
            ("low-power", 0.25),
            ("balanced", 0.6),
            ("cinematic", 1.0),
        ] {
            let actual = manifest["motion_profiles"][mode]["particle_budget_scale"]
                .as_f64().expect("budget scale") as f32;
            assert_eq!(actual, expected);
            let palette = palette_for_theme(&manifest, "cosmic-atlas").unwrap();
            let values = uniform_values(160, 90, 0.0, 0.0, actual, palette);
            assert_eq!(values.len(), 28);
            assert_eq!(values[25], expected);
            assert_eq!(floats_as_le_bytes(&values).len(), 112);
        }
    }

    #[test]
    fn unknown_theme_is_rejected() {
        let m: Value = serde_json::from_str(MANIFEST).unwrap();
        assert!(palette_for_theme(&m,"unknown-theme").is_err());
    }

    #[test]
    fn uniform_layout_is_exactly_one_hundred_twelve_bytes() {
        let m: Value = serde_json::from_str(MANIFEST).unwrap();
        let p=palette_for_theme(&m,"mycelial-genesis").unwrap();
        let vals=uniform_values(640,360,0.5,0.65,0.25,p);
        assert_eq!(vals.len(),28);
        assert_eq!(floats_as_le_bytes(&vals).len(),112);
        assert_eq!(vals[24], p.scene_family_index as f32);
        assert_eq!(vals[25], 0.25);
    }

    #[test]
    fn all_manifest_scene_families_map_to_distinct_stable_shader_variants() {
        let m: Value = serde_json::from_str(MANIFEST).unwrap();
        let themes=m["themes"].as_array().unwrap();
        let mut variants=HashSet::new();
        for theme in themes {
            let id=theme["id"].as_str().unwrap();
            let family=theme["scene_family"].as_str().unwrap();
            let expected_variant: u32 = match family {
                "mycelial-network" => 0,
                "solar-canopy" => 1,
                "cosmic-cartography" => 2,
                "ocean-waves" => 3,
                "epistemic-lattice" => 4,
                "quantum-interference" => 5,
                "aurora-ribbons" => 6,
                "graphite-minimal" => 7,
                "verdant-recovery" => 8,
                "civic-commons" => 9,
                other => panic!("unrecognized family in fixture: {other}"),
            };
            let palette=palette_for_theme(&m,id).unwrap();
            assert_eq!(palette.scene_family,family);
            assert_eq!(palette.scene_family_index,theme["shader_variant"].as_u64().unwrap() as u32);
            assert_eq!(palette.scene_family_index,expected_variant);
            assert!(variants.insert(palette.scene_family_index), "duplicate variant for {family}");
        }
        assert_eq!(themes.len(),10);
        assert_eq!(variants.len(),10);
    }

    #[test]
    fn neutral_palette_equalizes_colors_without_erasing_family_ids() {
        let manifest: Value = serde_json::from_str(MANIFEST).unwrap();
        let themes = manifest["themes"].as_array().unwrap();
        let first_id = themes[0]["id"].as_str().unwrap();
        let baseline = neutral_benchmark_palette(palette_for_theme(&manifest, first_id).unwrap());
        for theme in themes {
            let id = theme["id"].as_str().unwrap();
            let original = palette_for_theme(&manifest, id).unwrap();
            let neutral = neutral_benchmark_palette(original);
            assert_eq!(neutral.background, baseline.background);
            assert_eq!(neutral.primary, baseline.primary);
            assert_eq!(neutral.secondary, baseline.secondary);
            assert_eq!(neutral.node, baseline.node);
            assert_eq!(neutral.convergence, baseline.convergence);
            assert_eq!(neutral.scene_family_index, original.scene_family_index);
            assert_eq!(neutral.scene_family, original.scene_family);
        }
    }

    #[test]
    fn neutral_palette_cli_flag_is_parsed() {
        let args=vec!["--output".into(),"capture.ppm".into(),"--neutral-palette".into()];
        let config=parse_args(&args).unwrap().unwrap();
        assert!(config.neutral_palette);
    }

    #[test]
    fn shader_profile_ids_are_stable_for_receipts() {
        assert_eq!(ShaderProfile::Baseline.id(), "baseline-pre-specialization");
        assert_eq!(ShaderProfile::Specialized.id(), "specialized-family-work");
    }

    #[test]
    fn shader_profile_defaults_to_specialized_and_accepts_baseline() {
        let default_args=vec!["--output".into(),"capture.ppm".into()];
        assert_eq!(parse_args(&default_args).unwrap().unwrap().shader_profile, ShaderProfile::Specialized);
        let baseline_args=vec![
            "--output".into(),"capture.ppm".into(),
            "--shader-profile".into(),"baseline".into()
        ];
        assert_eq!(parse_args(&baseline_args).unwrap().unwrap().shader_profile, ShaderProfile::Baseline);
    }

    #[test]
    fn unknown_shader_profile_is_rejected_before_adapter_creation() {
        let args=vec![
            "--output".into(),"capture.ppm".into(),
            "--shader-profile".into(),"other".into()
        ];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn row_alignment_is_checked_and_padded() {
        assert_eq!(align_up(256,256).unwrap(),256);
        assert_eq!(align_up(257,256).unwrap(),512);
        assert!(align_up(u32::MAX,256).is_err());
    }

    #[test]
    fn nearest_rank_percentiles_are_stable() {
        assert_eq!(percentile(&[1.0,2.0,3.0,4.0],0.5),Some(2.0));
        assert_eq!(percentile(&[1.0,2.0,3.0,4.0],0.95),Some(4.0));
        assert_eq!(percentile(&[],0.95),None);
    }

    #[test]
    fn cli_rejects_oversized_width_without_device_creation() {
        let args=vec!["--output".into(),"capture.ppm".into(),"--width".into(),"9000".into()];
        assert!(parse_args(&args).is_err());
    }
}
