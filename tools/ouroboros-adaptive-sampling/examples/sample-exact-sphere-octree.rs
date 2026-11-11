use std::{env, error::Error, fs};

use ouroboros_adaptive_sampling::{
    build_adaptive_octree, AdaptiveOctreeConfig, MAX_BALANCE_PAIR_CHECKS,
};
use ouroboros_scalar_grid::{
    AnalyticField, GridSpec, ScalarGrid, MAX_CANONICAL_BYTES, MAX_SAMPLES,
};

fn main() -> Result<(), Box<dyn Error>> {
    let output = env::args()
        .nth(1)
        .unwrap_or_else(|| "ouroboros-exact-sphere.ourooct".to_owned());

    // The 2x2x2 source lattice deliberately has all corners outside the sphere.
    // Conservative cell intervals must still discover the enclosed component.
    let source = ScalarGrid::sample_exact(
        GridSpec {
            minimum_microunits: [-2_000_000; 3],
            maximum_microunits: [2_000_000; 3],
            dimensions: [2, 2, 2],
            max_samples: MAX_SAMPLES,
            max_descriptor_bytes: 1_048_576,
            max_canonical_bytes: MAX_CANONICAL_BYTES,
            max_working_payload_bytes: 32 * 1024 * 1024,
        },
        AnalyticField::Sphere {
            center_microunits: [0, 0, 0],
            radius_microunits: 1_500_000,
        },
        0,
    )?;

    let config = AdaptiveOctreeConfig {
        max_depth: 5,
        max_unique_samples: 32_768,
        max_leaf_cells: 4_096,
        max_working_payload_bytes: 64 * 1024 * 1024,
        max_receipt_bytes: 32 * 1024 * 1024,
        max_balance_pair_checks: MAX_BALANCE_PAIR_CHECKS,
    };
    let report = build_adaptive_octree(&source, config)?;
    let bytes = report.canonical_bytes()?;
    fs::write(&output, &bytes)?;
    let metrics = report.metrics();

    println!(
        "{{\"schema\":\"OUROOCT1\",\"max_depth\":{},\"unique_samples\":{},\"sample_cache_hits\":{},\"leaves\":{},\"inside\":{},\"outside\":{},\"surface_possible\":{},\"budget_stopped\":{},\"balance_stopped\":{},\"balance_status\":\"{:?}\",\"remaining_2_to_1_violations\":{},\"working_payload_bytes\":{},\"receipt_bytes\":{},\"output\":\"{}\"}}",
        metrics.maximum_depth,
        metrics.unique_sample_count,
        metrics.sample_cache_hits,
        metrics.leaf_count,
        metrics.leaves_certified_inside,
        metrics.leaves_certified_outside,
        metrics.leaves_surface_possible,
        metrics.leaves_budget_stopped,
        metrics.leaves_balance_stopped,
        report.balance_status(),
        metrics.remaining_2_to_1_violations,
        metrics.working_payload_estimate_bytes,
        bytes.len(),
        output.replace('\\', "\\\\").replace('"', "\\\"")
    );
    Ok(())
}