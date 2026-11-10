use std::{env, error::Error, fs};

use ouroboros_dual_contouring::{extract, DualContourConfig};
use ouroboros_sdf_lab::{FaceSkinField, GridSpec, UnionMode, MAX_CANONICAL_BYTES, MAX_SAMPLES};

fn main() -> Result<(), Box<dyn Error>> {
    let output = env::args()
        .nth(1)
        .unwrap_or_else(|| "ouroboros-face-skin-dc32.ourodc".to_owned());
    let spec = GridSpec {
        minimum: [-0.18, -0.14, -0.16],
        maximum: [0.25, 0.14, 0.16],
        dimensions: [32, 32, 32],
        max_samples: MAX_SAMPLES,
        max_canonical_bytes: MAX_CANONICAL_BYTES,
        max_working_payload_bytes: 32 * 1024 * 1024,
    };
    let field = FaceSkinField::source_fixture(UnionMode::PolynomialSmooth {
        width: 0.004,
    })?;
    let sampled = spec.sample(field)?;
    let mesh = extract(&sampled, DualContourConfig::default())?;
    let bytes = mesh.canonical_bytes()?;
    fs::write(&output, &bytes)?;
    let metrics = mesh.metrics();
    let audit = mesh.audit_report();

    println!(
        "{{\"schema\":\"OURODCQ1\",\"grid\":[32,32,32],\"active_cells\":{},\"crossing_edges\":{},\"vertices\":{},\"quads\":{},\"triangles\":{},\"qef_p50_residual_nano\":{},\"qef_p95_residual_nano\":{},\"geometry_bytes\":{},\"topology\":\"{:?}\",\"closed\":\"{:?}\",\"orientation\":\"{:?}\",\"self_intersections\":\"{:?}\",\"canonical_bytes\":{},\"output\":\"{}\"}}",
        metrics.active_cell_count,
        metrics.sign_changing_edge_count,
        mesh.vertices().len(),
        mesh.quads().len(),
        mesh.triangles().len(),
        metrics.qef_residual_p50_nanounits,
        metrics.qef_residual_p95_nanounits,
        metrics.geometry_bytes,
        audit.checks.topology_manifold,
        audit.checks.closed_surface,
        audit.checks.orientation_consistency,
        audit.checks.self_intersections,
        bytes.len(),
        output.replace('\\', "\\\\").replace('"', "\\\"")
    );
    Ok(())
}
