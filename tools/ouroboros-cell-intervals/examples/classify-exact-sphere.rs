use std::{env, error::Error, fs};

use ouroboros_cell_intervals::{classify, IntervalLimits};
use ouroboros_scalar_grid::{AnalyticField, GridSpec, MAX_CANONICAL_BYTES, MAX_SAMPLES};

fn main() -> Result<(), Box<dyn Error>> {
    let output = env::args()
        .nth(1)
        .unwrap_or_else(|| "ouroboros-sphere-intervals.ouroiv".to_owned());
    let spec = GridSpec {
        minimum_microunits: [-2_000_000; 3],
        maximum_microunits: [2_000_000; 3],
        dimensions: [65, 65, 65],
        max_samples: MAX_SAMPLES,
        max_descriptor_bytes: 1_048_576,
        max_canonical_bytes: MAX_CANONICAL_BYTES,
        max_working_payload_bytes: 32 * 1024 * 1024,
    };
    let grid = ouroboros_scalar_grid::ScalarGrid::sample_exact(
        spec,
        AnalyticField::Sphere {
            center_microunits: [0, 0, 0],
            radius_microunits: 1_500_000,
        },
        0,
    )?;
    let report = classify(&grid, IntervalLimits::default())?;
    let bytes = report.canonical_bytes()?;
    fs::write(&output, &bytes)?;
    let metrics = report.metrics();
    println!(
        "{{\"schema\":\"OUROIV1\",\"grid\":[65,65,65],\"cells\":{},\"inside\":{},\"outside\":{},\"surface_possible\":{},\"uncertified\":{},\"lipschitz_q32\":{},\"verified_samples\":{},\"adjacent_checks\":{},\"receipt_bytes\":{},\"output\":\"{}\"}}",
        metrics.total_cells,
        metrics.certified_inside_cells,
        metrics.certified_outside_cells,
        metrics.surface_possible_cells,
        metrics.uncertified_cells,
        report.policy().lipschitz_q32,
        metrics.verified_source_samples,
        metrics.adjacent_sample_checks,
        bytes.len(),
        output.replace('\\', "\\\\").replace('"', "\\\"")
    );
    Ok(())
}
