# Ouroboros DCSDFD matched reference harness

**Status:** source and workload harness. No reference or Rust benchmark has been executed in this environment.

Tracking: [Issue #70](https://github.com/Luminous-Dynamics/sovereign-boot/issues/70)  
Classical Rust baseline: [PR #67](https://github.com/Luminous-Dynamics/sovereign-boot/pull/67)  
Faithful Rust port: [Issue #48](https://github.com/Luminous-Dynamics/sovereign-boot/issues/48)

## Reference pin

- Gpytoolbox commit: \`d6e6ae3b5ff9200e687ca19c178ff35d65c1f810\` (9 November 2026).
- API: \`dual_contouring_of_signed_distance_data\`.
- Paper: Carrera et al., SIGGRAPH 2026, [DOI 10.1145/3799902.3811116](https://doi.org/10.1145/3799902.3811116).

The maintained method is not classical one-pass dual contouring. It runs initial cell/Hermite/QEF construction, then outer loops that assign discrete-SDF sphere constraints, compute cell-face intersections, optionally update Hermite data, and solve per-cell inner optimization problems. It exposes outer/inner iteration counts, SVD threshold, Hermite/face blending weights, regularization and batch size.

## Critical reproducibility issue

The pinned upstream source seeds its far-grid-sample shuffle with \`std::random_device\` and \`std::mt19937\`. If a requested sphere batch is smaller than the full sample grid and there are far samples, the exact selected constraint subset can vary. Even when the full far sample set fits, shuffle order can vary. This matters because the method subsequently sorts sphere constraints by radius and performs floating-point optimization.

Two reference modes are kept separate:

1. **Upstream-unmodified:** use the repository source exactly as pinned; run at least five repeats and report output/metric variability.
2. **Fixed-seed-patched:** apply \`determinize-gpytoolbox.patch\`, which changes only the RNG seed constructor to \`std::mt19937(0x4F55524FU)\`. This is a labelled research variant, not official upstream behavior. The adapter verifies the exact source line before running.

Do not set \`batch_size=1\` to avoid this: it changes the available discrete-SDF constraint set and is not an equivalent determinization. The fixed-seed mode sets thread caps before native imports, but that does not prove full runtime determinism. Repeated output-hash comparisons remain mandatory.

Patch SHA-256: \`71b1983492b906935b95588368bdf7c11a87a47ed4ae0fa8694ea5d2f95386ee\`.

## Input equivalence

The adapter accepts a canonical \`OUROGRD1\` file from the SDF lab. It validates the header, field descriptor, grid layout, bounds, scale, dimensions and exact sample count. It reconstructs the same first-axis-fastest \`GV\` positions using the canonical bounds and converts signed int32 microunit samples directly to float64.

The adapter does **not** call the face-field evaluator and does **not** resample the field. It records the complete input SHA-256 and field-descriptor digest in the run manifest. The current face fixture remains an approximate IQ-style ellipsoid distance, not an exact Euclidean SDF.

## Prepare a pinned reference checkout

Follow the Gpytoolbox installation/build instructions at the pinned commit. The compiled extension \`gpytoolbox_bindings\` must be available to Python. Verify the checkout:

\`\`\`sh
git checkout d6e6ae3b5ff9200e687ca19c178ff35d65c1f810
git rev-parse HEAD
\`\`\`

Upstream-unmodified mode:

\`\`\`sh
python benchmarks/ouroboros-dcsdfd/reference_adapter.py \\
  --grid /path/to/face-smooth-32.ourogrid \\
  --gpytoolbox-root /path/to/gpytoolbox \\
  --mode upstream-unmodified \\
  --output-dir results/upstream/smooth-32/run-01
\`\`\`

Fixed-seed-patched mode (working tree must still report the pinned commit):

\`\`\`sh
git -C /path/to/gpytoolbox apply /path/to/sovereign-boot/benchmarks/ouroboros-dcsdfd/determinize-gpytoolbox.patch
python benchmarks/ouroboros-dcsdfd/reference_adapter.py \\
  --grid /path/to/face-smooth-32.ourogrid \\
  --gpytoolbox-root /path/to/gpytoolbox \\
  --mode fixed-seed-patched \\
  --threads 1 \\
  --output-dir results/seeded/smooth-32/run-01
\`\`\`

The adapter saves \`vertices.npy\`, \`quads.npy\` and \`run-v1.json\`. The JSON binds input/source hashes, options, versions, host, elapsed time, process RSS high-water, result counts and geometry-array hashes. It also records the loaded Python module hash and compiled `_dcsdd_cpp_impl` binary path/hash. The upstream-unmodified mode requires a clean tracked checkout; fixed-seed mode permits exactly one unstaged source-file change matching the recorded patch. The binary digest is recorded, but build provenance is not independently attested—the manifest says so explicitly. This does not claim topology, self-intersection, visual quality or energy results.

## Workload and parameters

\`workloads-v1.json\` records the face fixture bounds, sharp/smooth unions, 32³/64³/128³ sizes, upstream defaults and stage presets (1/5/20/100 outer iterations). Use the same parameter set for the Rust baseline and reference whenever a meaningful intersection exists.

Early characterization: 5 upstream-unmodified repeats and 10 fixed-seed-patched paired runs per workload. Confirmatory claims require at least 30 matched runs and an explicit statistical plan.

## Comparison metrics

- Input SHA, fixture/descriptor hash, exact dimensions/bounds and coordinate layout.
- Vertex/quad/triangle counts, surface components, boundary edges, manifold links, orientation and degeneracy.
- Symmetric vertex-site nearest-neighbour distance p50/p95/p99/max (tessellation-sensitive diagnostic).
- Deterministic area-weighted triangle-surface sample distance using a fixed Halton sequence (reported with sample count and both directions). This is an estimator, not exact Chamfer/Hausdorff; repeat at multiple sample counts to assess convergence.
- Surface-to-discrete-SDF residual (and analytic ground-truth residual only for exact SDF fixtures).
- Silhouette IoU, semantic landmark error, head contour and feature-line drift.
- DCSDFD iteration counts, energy and displacement trace only when captured from an explicitly instrumented reference; otherwise unavailable.
- Runtime, peak process RSS, geometry bytes and scaling with grid resolution.

Never compare only triangle count or final vertex distance. A mismatch should be localized to cell classification, Hermite construction, initial QEF, sphere assignment, face intersections, inner solve, Hermite update, outer loop, connectivity or triangulation. The current public adapter does not claim all internal traces are exposed.

## Current scope and non-claims

This is a matched reference harness, not a Rust port. It does not prove the classical Rust baseline is equivalent to DCSDFD, and does not show speed/quality/energy superiority. The fixed-seed source is not official upstream. No benchmark has been run. The face fixture is approximate; a broader benchmark still needs exact analytic sphere/box/capsule/torus grids and pinned mesh-derived SDF grids as separate strata.


## Comparator hardening

The comparator now:
- validates the reference manifest schema, pinned upstream commit, source mode, finite isovalue, output-array hashes/counts/shapes, finite vertices and quad index ranges;
- requires isovalue zero when comparing against the current OURODCQ1 classical extractor;
- verifies the exact embedded OUROGRD1 digest against the supplied grid and the DCSDFD run manifest;
- checks that the Rust triangle view is exactly the declared fixed (0,2) triangulation of its quad array;
- computes both vertex-site distances and deterministic area-weighted triangle-surface sample distances;
- labels the surface-sample metric as an estimator, not an exact Hausdorff guarantee.

The surface sampler excludes zero-area triangles from its sampling distribution and reports how many were excluded. The independent Rust mesh auditor remains the full topology/vertex-link audit; this Python summary does not replace it.

The reference-run loader now requires float64 vertex storage and signed-int64 quad-index storage before any dtype conversion, rejects archive/symlink/path-escape artifacts, caps individual NPY payload sizes and verifies the descriptor digest, bounds, dimensions, scale and first-axis-fastest metadata against the exact input grid. This prevents a float32 vertex file or fractional quad index array from being silently normalized and accepted.

The lightweight Python edge-incidence topology summary has an explicit maximum of 1,000,000 vertices and 200,000 triangles. Larger meshes fail with a clear limit error rather than creating an unbounded Python dictionary/union-find workspace. This summary is not the full Rust audit; the aggregation must mark this diagnostic incomplete/unsupported when the limit is exceeded.

Current source contains 12 authored reference-adapter tests and 17 authored comparator tests. They are source-inspected but have **not** been run in this environment. No reference extraction, mesh comparison or performance trial is claimed.
