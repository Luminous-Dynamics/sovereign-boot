// Copyright (C) 2024-2026 Tristan Stoltz / Luminous Dynamics
// SPDX-License-Identifier: AGPL-3.0-or-later
// Commercial licensing: see COMMERCIAL_LICENSE.md at repository root

//! Growth-only benchmark for the bounded procedural branch model.
//!
//! Unlike render_bench, this example does not rasterize pixels or talk to DRM.
//! It records update costs over branch-count intervals, then measures capped
//! steady state separately so those workloads are not conflated.

use std::time::Instant;

use symthaea_quicken_fb::mycelium::{MAX_BRANCHES, MycelialNetwork};
use symthaea_quicken_fb::theme::Theme;

const DEFAULT_STEPS: u32 = 3_000;
const MAX_STEPS: u32 = 10_000;
const SATURATED_SAMPLES: usize = 128;
const TARGETS: [usize; 4] = [512, 2_048, 4_096, MAX_BRANCHES];

#[derive(Debug)]
struct Checkpoint {
    target_branches: usize,
    interval_start_branches: usize,
    actual_branches: usize,
    interval_start_step: u32,
    step: u32,
    sample_count: usize,
    p50_ns: u128,
    p95_ns: u128,
    max_ns: u128,
}

fn main() {
    let (steps, theme) = match parse_args() {
        Ok(Some(values)) => values,
        Ok(None) => return,
        Err(error) => {
            eprintln!("growth_bench: {error}");
            print_usage();
            std::process::exit(2);
        }
    };

    if steps == 0 || steps > MAX_STEPS {
        eprintln!("growth_bench: steps must be within 1..={MAX_STEPS}");
        std::process::exit(2);
    }

    let mut network = MycelialNetwork::new_with_theme(
        1920,
        1080,
        "sovereign-growth-benchmark-v1",
        theme,
    );

    let mut checkpoints = Vec::with_capacity(TARGETS.len());
    let mut next_target = 0usize;
    let mut sample_bucket = Vec::<u128>::with_capacity(128);
    let mut interval_start_branches = network.branches.len();
    let mut interval_start_step = 0u32;
    let mut steps_executed = 0u32;

    for step in 1..=steps {
        let start = Instant::now();
        network.grow(0.1, 0.75);
        let frame_ns = start.elapsed().as_nanos();
        steps_executed = step;
        sample_bucket.push(frame_ns);
        let branch_count = network.branches.len();

        // Each checkpoint summarizes the update samples since the previous
        // threshold, and records the exact branch-count/step interval.
        while next_target < TARGETS.len() && branch_count >= TARGETS[next_target] {
            if sample_bucket.is_empty() {
                // A single update may cross multiple thresholds; record its
                // measured time rather than fabricating a zero-length sample.
                sample_bucket.push(frame_ns);
            }
            sample_bucket.sort_unstable();
            checkpoints.push(Checkpoint {
                target_branches: TARGETS[next_target],
                interval_start_branches,
                actual_branches: branch_count,
                interval_start_step,
                step,
                sample_count: sample_bucket.len(),
                p50_ns: percentile(&sample_bucket, 50),
                p95_ns: percentile(&sample_bucket, 95),
                max_ns: sample_bucket[sample_bucket.len() - 1],
            });
            sample_bucket.clear();
            interval_start_branches = branch_count;
            interval_start_step = step;
            next_target += 1;
        }

        if next_target == TARGETS.len() {
            break;
        }
    }

    let checkpoint_json = checkpoints
        .iter()
        .map(|checkpoint| {
            format!(
                concat!(
                    "{{\"checkpoint_branches\":{},",
                    "\"interval_start_branches\":{},",
                    "\"actual_branches\":{},",
                    "\"interval_start_step\":{},",
                    "\"step\":{},",
                    "\"sample_count\":{},",
                    "\"update_time_ms\":{{\"p50\":{:.4},\"p95\":{:.4},\"max\":{:.4}}}}}"
                ),
                checkpoint.target_branches,
                checkpoint.interval_start_branches,
                checkpoint.actual_branches,
                checkpoint.interval_start_step,
                checkpoint.step,
                checkpoint.sample_count,
                checkpoint.p50_ns as f64 / 1_000_000.0,
                checkpoint.p95_ns as f64 / 1_000_000.0,
                checkpoint.max_ns as f64 / 1_000_000.0,
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    // Measure steady state only once the hard branch cap has been reached.
    // This is separate from the update distribution during growth to the cap.
    let mut saturated_samples = Vec::<u128>::with_capacity(SATURATED_SAMPLES);
    if next_target == TARGETS.len() && network.branches.len() == MAX_BRANCHES {
        for _ in 0..SATURATED_SAMPLES {
            let start = Instant::now();
            network.grow(0.1, 0.75);
            saturated_samples.push(start.elapsed().as_nanos());
        }
        saturated_samples.sort_unstable();
    }

    let saturated_json = if saturated_samples.is_empty() {
        "null".to_string()
    } else {
        format!(
            concat!(
                "{{\"sample_count\":{},",
                "\"update_time_ms\":{{\"p50\":{:.4},\"p95\":{:.4},\"max\":{:.4}}}}}"
            ),
            saturated_samples.len(),
            percentile(&saturated_samples, 50) as f64 / 1_000_000.0,
            percentile(&saturated_samples, 95) as f64 / 1_000_000.0,
            saturated_samples[saturated_samples.len() - 1] as f64 / 1_000_000.0,
        )
    };

    println!(
        concat!(
            "{{\n",
            "  \"schema\":\"cpu-growth-bench-v1\",\n",
            "  \"source_commit\":\"{}\",\n",
            "  \"build_commit\":\"{}\",\n",
            "  \"measurement\":\"simulation_update_only\",\n",
            "  \"theme\":\"{}\",\n",
            "  \"seed\":\"sovereign-growth-benchmark-v1\",\n",
            "  \"branch_cap\":{},\n",
            "  \"steps_requested\":{},\n",
            "  \"steps_executed\":{},\n",
            "  \"final_branch_count\":{},\n",
            "  \"all_targets_reached\":{},\n",
            "  \"checkpoints\":[{}],\n",
            "  \"cap_steady_state\":{},\n",
            "  \"includes_pixel_rendering\":false,\n",
            "  \"includes_gpu\":false,\n",
            "  \"includes_drm\":false\n",
            "}}"
        ),
        option_env!("VISUAL_SOURCE_COMMIT").unwrap_or("unavailable-local-build"),
        option_env!("VISUAL_BUILD_COMMIT").unwrap_or("unavailable-local-build"),
        theme.id(),
        MAX_BRANCHES,
        steps,
        steps_executed,
        network.branches.len(),
        next_target == TARGETS.len(),
        checkpoint_json,
        saturated_json,
    );
}

fn percentile(sorted: &[u128], percentile: usize) -> u128 {
    debug_assert!(!sorted.is_empty());
    let last = sorted.len() - 1;
    let index = (last * percentile).div_ceil(100);
    sorted[index.min(last)]
}

fn parse_args() -> Result<Option<(u32, Theme)>, String> {
    let args: Vec<String> = std::env::args().collect();
    parse_args_from(&args)
}

fn parse_args_from(args: &[String]) -> Result<Option<(u32, Theme)>, String> {
    let mut steps = DEFAULT_STEPS;
    let mut theme = Theme::MycelialGenesis;
    let mut i = 1usize;

    while i < args.len() {
        match args[i].as_str() {
            "--steps" => {
                i += 1;
                steps = args
                    .get(i)
                    .ok_or("--steps requires a value")?
                    .parse::<u32>()
                    .map_err(|_| "--steps must be a positive integer")?;
            }
            "--theme" => {
                i += 1;
                let id = args.get(i).ok_or("--theme requires a stable theme ID")?;
                theme = Theme::from_id(id)
                    .ok_or_else(|| format!("unknown theme ID: {id}"))?;
            }
            "--help" | "-h" => {
                print_usage();
                return Ok(None);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
        i += 1;
    }

    Ok(Some((steps, theme)))
}

fn print_usage() {
    eprintln!(
        "Usage: cargo run --release --example growth_bench -- [OPTIONS]\\n  --theme <ID>   Theme ID (default: mycelial-genesis)\\n  --steps <N>    Simulation updates, 1..=10000 (default: 3000)\\n  --help         Show this help"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_thresholds_match_branch_cap() {
        assert_eq!(TARGETS, [512, 2_048, 4_096, MAX_BRANCHES]);
        assert_eq!(TARGETS[3], MAX_BRANCHES);
        assert_eq!(SATURATED_SAMPLES, 128);
    }

    #[test]
    fn percentile_is_bounded_and_uses_sorted_samples() {
        let samples = [10, 20, 30, 40, 50];
        assert_eq!(percentile(&samples, 0), 10);
        assert_eq!(percentile(&samples, 50), 30);
        assert_eq!(percentile(&samples, 95), 50);
        assert_eq!(percentile(&samples, 100), 50);
    }

    #[test]
    fn parser_accepts_theme_and_step_budget() {
        let args = vec![
            "growth_bench".to_string(),
            "--theme".to_string(),
            "mycelial-genesis".to_string(),
            "--steps".to_string(),
            "4096".to_string(),
        ];
        let parsed = parse_args_from(&args)
            .expect("valid arguments")
            .expect("help not requested");
        assert_eq!(parsed, (4096, Theme::MycelialGenesis));
    }

    #[test]
    fn parser_rejects_unknown_theme() {
        let args = vec![
            "growth_bench".to_string(),
            "--theme".to_string(),
            "unknown".to_string(),
        ];
        assert!(parse_args_from(&args).is_err());
    }
}
