# Ouroboros SDF reconstruction benchmark corpus

**Status:** fixture design / schema proposal. No canonical grids have been sampled and no extraction comparison has been run.

Tracking: [Issue #51](https://github.com/Luminous-Dynamics/sovereign-boot/issues/51)  
Classical control: [PR #47](https://github.com/Luminous-Dynamics/sovereign-boot/pull/47)  
Maintained DCSDFD reference: [Gpytoolbox API](https://gpytoolbox.org/latest/dual_contouring_of_signed_distance_data/)  
Paper: [SIGGRAPH 2026 — Dual Contouring of Signed Distance Data](https://doi.org/10.1145/3799902.3811116)

## Why the corpus is split by field semantics

The current Ouroboros face fixture uses an IQ-style approximate ellipsoid distance estimator and a polynomial smooth minimum. It is a useful application-level field, but it is not an exact Euclidean SDF. It must not be used as the only accuracy oracle.

The manifest therefore distinguishes:
- exact analytic primitives, where point-to-surface ground truth is known;
- exact analytic SDFs with controlled sample noise;
- approximate sharp/smooth compositions, where the declared field's zero-set is ground truth but exact distance is not assumed;
- the existing approximate face field, used for semantic/identity-specific evaluation.

A signed zero level set does not automatically make a function a true signed-distance function.

## Fixtures

The `fixtures-v1.json` file currently specifies 12 fixtures: sphere; axis-aligned and rotated box; capsule; torus; sub-voxel capsule at 0.5/1/1.5/2 voxel feature scales; two separated spheres; plane/boundary control; sharp min-composite; polynomial smooth-union; Ouroboros head+nose application field; controlled noisy box.

The initial grid matrix is 32³, 64³ and 128³. 256³ is optional and requires a separate resource qualification.

## Shared input rule

The proposed generic `OUROSGD1` contract in Issue #51 must bind field descriptor and semantic class (exact / approximate / mesh-derived / unknown); coordinate frame, units, axis order and first-axis-fastest layout; bounds, dimensions, isovalue and sample spacing; sign and quantization conventions; ordered int32 samples; source mesh digest or exact analytic parameters; noise realization/seed where applicable.

Each extractor receives the same canonical grid bytes. The reference adapter may convert quantized samples to float64 for a library API only by deterministic dequantization of those samples; it must not call the analytic source field to obtain a second set of values.

The current `OUROGRD1` face fixture remains frozen and valid for its present schema. Do not silently reinterpret its descriptor as a generic field.

## Comparators

1. Marching Cubes reference.
2. Ouroboros classical Hermite/QEF control (PR #47).
3. Pinned maintained Gpytoolbox SIGGRAPH 2026 DCSDFD implementation.
4. Future Ouroboros Rust DCSDFD implementation (Issue #48).

For each comparator, record exact version/commit, options, input digest, output digest, host/build metadata and failure class.

## Measurements

Geometry: symmetric Chamfer, Hausdorff p95/p99/max, edge-feature Chamfer, normal angular error, silhouette IoU and semantic landmark error.

Topology: edge and vertex-link manifoldness, watertightness, winding, boundary loops, Euler characteristic, face flips, degenerates and self-intersection pairs.

Operations: p50/p95/p99 runtime, peak RSS, allocation count/bytes, output size and iterative energy/termination trace.

Report per fixture and resolution; do not collapse results into one weighted score. Plot accuracy/noise/resolution curves and the nondominated method frontier.

## Acceptance gates and honest status

- Identical input grid digest for every method in a matched comparison.
- Zero invalid indices, non-finite coordinates or silently truncated geometry.
- DCSDFD port parity targets (median vertex distance ≤0.1 voxel, p95 ≤0.5 voxel) are hypotheses for non-degenerate exact-SDF fixtures, not current results.
- Topology and self-intersection are separate. A watertight combinatorial mesh is not assumed to be intersection-free.
- Face identity recognition is a separate blinded human evaluation, not inferred from Chamfer or silhouette.
- No runtime, power or superiority claim until named-host repeated trials exist.

## Current state

The fixture manifest is committed. The generic grid schema, grid samplers, reference adapter and comparator outputs remain future work under Issue #51. No test/CI or benchmark result is claimed.