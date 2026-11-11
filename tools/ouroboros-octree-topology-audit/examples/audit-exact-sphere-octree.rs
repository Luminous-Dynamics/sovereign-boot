use std::{env, error::Error, fs};

use ouroboros_adaptive_sampling::{
    build_adaptive_octree, AdaptiveOctreeConfig, MAX_BALANCE_PAIR_CHECKS,
};
use ouroboros_octree_topology_audit::{
    audit_octree_topology, TopologyAuditConfig,
};
use ouroboros_scalar_grid::{
    AnalyticField, GridSpec, ScalarGrid, MAX_CANONICAL_BYTES, MAX_SAMPLES,
};

fn main() -> Result<(), Box<dyn Error>> {
    let output = env::args()
        .nth(1)
        .unwrap_or_else(|| "ouroboros-exact-sphere-topology.ourocf".to_owned());
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
    let tree = build_adaptive_octree(
        &source,
        AdaptiveOctreeConfig {
            max_depth: 5,
            max_unique_samples: 32_768,
            max_leaf_cells: 4_096,
            max_working_payload_bytes: 64 * 1024 * 1024,
            max_receipt_bytes: 32 * 1024 * 1024,
            max_balance_pair_checks: MAX_BALANCE_PAIR_CHECKS,
        },
    )?;
    let audit = audit_octree_topology(&source, &tree, TopologyAuditConfig::default())?;
    let bytes = audit.canonical_bytes(&source, &tree)?;
    fs::write(&output, &bytes)?;
    let metrics = audit.metrics();
    println!(
        "{{\"schema\":\"OUROTCF1\",\"readiness\":\"{:?}\",\"leaves\":{},\"surface_possible\":{},\"contourable_surface\":{},\"surface_without_crossings\":{},\"face_neighbors\":{},\"edge_neighbors\":{},\"corner_neighbors\":{},\"face_violations\":{},\"edge_violations\":{},\"corner_violations\":{},\"zero_corner_surface_leaves\":{},\"pair_comparisons\":{},\"canonical_bytes\":{},\"output\":\"{}\"}}",
        audit.readiness(),
        metrics.leaf_count,
        metrics.surface_possible_leaf_count,
        metrics.contourable_surface_leaf_count,
        metrics.surface_possible_without_crossing_count,
        metrics.face_neighbor_pairs,
        metrics.edge_neighbor_pairs,
        metrics.corner_neighbor_pairs,
        metrics.face_two_to_one_violations,
        metrics.edge_two_to_one_violations,
        metrics.corner_two_to_one_violations,
        metrics.surface_possible_with_zero_corner_count,
        metrics.pair_comparisons_completed,
        bytes.len(),
        output.replace('\\', "\\\\").replace('"', "\\\"")
    );
    Ok(())
}
