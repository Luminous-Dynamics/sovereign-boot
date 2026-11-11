# Ouroboros semantic face SDF research benchmark

**Status:** source-grounded exploratory reference only. This is not a production Rust mesher, a qualified watertight face, or an implementation of the SIGGRAPH 2026 discrete-SDF dual-contouring method.

Tracking: [Issue #31](https://github.com/Luminous-Dynamics/sovereign-boot/issues/31)  
Source baseline: PR #15 head `d5a7f877d71757db49e63ad1b1582a661f6e4ef2`

## Why this benchmark exists

The current `OUROFAC1` is five independently closed ellipsoid regions. The regions have stable semantic IDs and landmark attachments, but separately closed volumes do not automatically form a visually coherent facial surface. Issue #28 requires intersection, occlusion, manifold and self-intersection qualification before a fused-surface claim.

The experiment asks a narrower question first:

> Should the head shell and nose become one skin-bearing surface, while eyes and mouth remain separately addressable anatomy?

The current `OUROFAC1` source geometry supplies an unusually good initial fixture. `fixtures-v1.json` records its exact local-frame feature IDs, centers, radii, source commit, coordinate convention, field estimator, composition operations, grid dimensions and resource ceilings.

## Semantic fixture

The coordinate order is **forward, lateral, up** in the `OUROLND1` face basis. The scene major radius is normalized to 1.0.

| Feature | Region ID | Local center (F,L,U) | Radii (F,L,U) | Proposed role |
|---|---:|---|---|---|
| Head shell | 1 | (0.030, 0, 0) | (0.120, 0.060, 0.110) | Skin-union candidate |
| Left eye | 2 | (0.060, -0.050, 0.020) | (0.016, 0.018, 0.014) | Independent attached anatomy |
| Right eye | 3 | (0.060, 0.050, 0.020) | (0.016, 0.018, 0.014) | Independent attached anatomy |
| Nose | 4 | (0.160, 0, 0.005) | (0.020, 0.014, 0.018) | Skin-union candidate |
| Mouth | 5 | (0.140, 0, -0.035) | (0.008, 0.035, 0.006) | Surface patch/cavity candidate |

The values are taken from `crates/visual-core/src/ouroboros.rs` and `crates/visual-core/src/ouroboros_face.rs` at the pinned feature-branch commit. They are not newly invented facial proportions.

## Exploratory implementation and measured results

The initial CPU-only sanity experiment used:

- Python 3.13.5
- NumPy 2.3.5
- scikit-image 0.26.0 `marching_cubes`
- Trimesh 4.11.1
- x86_64 Linux kernel 6.18.57, virtualized Intel Xeon Platinum 8573C, 3 logical CPUs

The field uses an IQ-style ellipsoid distance approximation. It is **not an exact Euclidean signed distance**. Head and nose were combined using sharp `min` and polynomial smooth-`min`. The output mesh was extracted with scikit-image marching cubes and checked with Trimesh.

### Smooth-union sweep

The table below uses smooth-union width `k = 0.004` in normalized major-radius units. Field residual is measured by evaluating the same approximate composite field at extracted vertices; it is not a Hausdorff distance to an exact surface.

| Grid | Vertices | Triangles | p95 field residual | Max field residual | Watertight | Components | Euler |
|---|---:|---:|---:|---:|---|---:|---:|
| 64³ | 5,786 | 11,568 | 3.44e-5 | 9.58e-4 | Yes | 1 | 2 |
| 96³ | 13,264 | 26,524 | 1.52e-5 | 5.91e-4 | Yes | 1 | 2 |
| 128³ | 23,728 | 47,452 | 8.76e-6 | 3.03e-4 | Yes | 1 | 2 |
| 192³ | 53,690 | 107,376 | 4.38e-6 | 1.59e-4 | Yes | 1 | 2 |

All four meshes were also reported by Trimesh as winding-consistent positive volumes. These checks do not establish absence of self-intersections; no self-intersection test was run.

### Resource-budget implication

The current production face mesh uses 36 bytes per vertex and 4 bytes per index, with hard ceilings of 50,000 vertices, 300,000 indices and 2,500,000 geometry bytes.

The production index buffer contains **three u32 indices per triangle**. The earlier estimate incorrectly charged only one index per triangle; the table below corrects that accounting.

| Grid | Vertices | Triangles | Indices | Estimated bytes | All current limits |
|---|---:|---:|---:|---:|---|
| 64³ | 5,786 | 11,568 | 34,704 | 347,112 | Fits |
| 96³ | 13,264 | 26,524 | 79,572 | 795,792 | Fits |
| 128³ | 23,728 | 47,452 | 142,356 | 1,423,632 | Fits |
| 192³ | 53,690 | 107,376 | 322,128 | 3,221,352 | **Fails all three** |

The 192³ case exceeds the 50,000 vertex ceiling, the 300,000 index ceiling and the 2.5 MB geometry-byte ceiling. The 128³ case remains under all three. This is exactly why negotiated geometry quality must retain independent, explicit limits rather than one aggregate budget.

The full five-width / three-grid extraction matrix, including all three timing trials per case, is recorded in [`sweep-results-v1.json`](sweep-results-v1.json).

### Intersection/occlusion clue

The separate primitive meshes were sampled at their vertices, and each vertex was tested against the head-shell approximate field. The fraction of sampled surface vertices inside the head shell was:

| Region | Approx. fraction of sampled surface vertices inside head shell |
|---|---:|
| Nose | 25.8% |
| Left eye | 69.1% |
| Right eye | 69.1% |
| Mouth | 44.4% |

This is a **surface-vertex fraction**, not a volume-overlap fraction, visible-pixel fraction or perceptual occlusion score. It demonstrates substantial geometric intersection in the current parameterization and justifies an explicit visible-surface policy. It does not, by itself, tell us whether the visible face looks correct.

The earlier arbitrary-scale experiment was superseded. The fixture and the issue were updated to use the actual pinned `OUROLND1`/`OUROFAC1` source geometry.

## Candidate algorithm matrix

Hold the semantic input field fixed when comparing extraction algorithms.

1. **Existing separate volumes:** retain the five closed regions; introduce deterministic draw order, depth/occlusion and socket/patch semantics.
2. **Marching cubes:** establish a straightforward sampled-SDF baseline.
3. **Classical dual contouring:** estimate gradients from neighboring SDF samples, solve the conventional per-cell vertex placement problem, and record feature preservation/topology.
4. **SIGGRAPH 2026 discrete-SDF dual contouring:** evaluate the authors' maintained Gpytoolbox reference implementation as an offline comparison after reviewing license and platform reproducibility.
5. **Rust-first implementation:** only port or reimplement after the reference corpus identifies a material advantage and provides adversarial fixtures.

### Same-sample extractor comparison (initial 64³ probe)

A separate CPU-only probe now supplies the **same first-axis-fastest signed-int32 microunit field bytes** to two different extractors at 64³, smooth-union width 0.004. The sample SHA-256 is `0ce6069a45f415734f8649f9ef2fe9fb0aa2b269ea69b2ababbd50ca4910853c`.

| Metric | Lewiner marching cubes | Simple finite-gradient QEF dual contouring |
|---|---:|---:|
| Vertices | 5,786 | 5,788 |
| Output quads | 0 | 5,786 |
| Triangles after fixed 0–2 quad triangulation | 11,568 | 11,572 |
| Trimesh watertight / winding-consistent | Yes / Yes | Yes / Yes |
| Connected components / Euler | 1 / 2 | 1 / 2 |
| p95 residual to the same approximate continuous field | 3.45e-5 | 4.56e-4 |
| Maximum residual | 9.57e-4 | 3.47e-3 |
| One observed extraction time | 10.64 ms | 1,074.45 ms |
| Estimated face bytes after triangle conversion | 347,112 | 347,232 |

The conventional DC reference emits quads; the table triangulates them only to run the common Trimesh topology checks. Its QEF uses finite-difference gradients and a small cell-center regularizer, clamps the solution to the cell, then builds dual faces around sign-changing primal edges.

**Interpretation is deliberately limited.** Both meshes pass these basic topology diagnostics on this fixture, but this simple QEF implementation has higher field residual. Its Python-loop extraction time is not comparable with scikit-image's compiled Cython marching-cubes implementation. These results do not establish that marching cubes is superior to optimized dual contouring, and they do not test the SIGGRAPH 2026 algorithm. See [`extractor-comparison-v1.json`](extractor-comparison-v1.json).

Reproduce with the pinned research environment (not a production dependency):

```sh
python benchmarks/ouroboros-face-sdf/compare-extractors-reference.py \
  --resolution 64 --smooth-width 0.004 --trials 3 \
  --output extractor-comparison-local.json
```

Install research-only dependencies: NumPy 2.3.5, scikit-image 0.26.0 and Trimesh 4.11.1. The script labels itself an offline Python reference; it is not invoked by production Cargo or runtime code.

### Maintained API contract and reproducibility

The maintained Gpytoolbox API accepts `S` (one signed-distance value per grid vertex), `GV` (one 3D position per grid vertex), and `nx, ny, nz`. Its documented flattening rule is `i + nx * (j + ny * k)`: the first axis is fastest. The canonical Ouroboros sampling order is therefore forward-fastest, then lateral, then up.

The function returns vertex positions `V` and quadrilateral faces `F`. For a fair comparison, pin and record:
- `isovalue` (default 0.0);
- `outer_iters` / `inner_iters` (default 100 each);
- `batch_size` (default 200000);
- `hermite_update` (default true);
- `mu` (0.1), `dc_weight` (0.02), `svd_threshold` (0.01);
- new Hermite/face/normal blend weights (0.2 each);
- the Gpytoolbox package and source revision.

Do not use package defaults as an unexplained baseline. The reference method is iterative; report all settings and run repeated trials. Also keep its returned quad topology separate from triangle-only baselines unless the quad-to-triangle conversion is explicitly specified and included in metrics.

### SIGGRAPH 2026 research

*Dual Contouring of Signed Distance Data* (Carrera, Wang, Batty, Stein and Sellán, SIGGRAPH 2026) reconstructs sharp features from regularly sampled SDF values without requiring gradient information or arbitrary SDF queries.

- [Paper and project page](https://gatc.cs.columbia.edu/projects/dual-contouring-of-signed-distance-data.html)
- [Maintained Gpytoolbox API](https://gpytoolbox.org/latest/dual_contouring_of_signed_distance_data/)
- [Archived research implementation](https://github.com/xianacarrera/dual-contouring-of-signed-distance-data)

The authors explicitly direct users away from the archived research code and toward the maintained Gpytoolbox implementation. Treat that implementation as an offline reference, not a production dependency. The original research repository reports macOS-only testing.

## Geometry policy hypothesis

The initial architecture hypothesis is deliberately heterogeneous:

- **Head shell + nose/snout:** candidate for one skin-bearing analytic union and extracted surface.
- **Eyes:** remain separate semantic surfaces with explicit socket/occlusion rules and stable feature IDs.
- **Mouth:** represent as a surface patch, crease or cavity; do not force it to be a closed ellipsoid.
- **Landmarks:** preserve `OUROLND1` as the authority; all extracted geometry must carry recoverable feature/region provenance.

A single universal Boolean union is not the goal. The correct topology differs by semantic role.

## Required qualification before production use

- Exact source-derived fixtures at neutral and maximum valid yaw/pitch.
- 64³, 96³, 128³, 192³ and 256³ grids; smooth widths 0, 0.002, 0.004, 0.006 and 0.010.
- Degenerate triangles, edge incidence, winding, connected components, Euler characteristic and independent self-intersection checks.
- Signed-field residual, symmetric Chamfer/Hausdorff against a high-resolution analytic reference and silhouette IoU.
- Landmark position error, region-ID recovery, attachment continuity and explicit occlusion review.
- Hard vertex/index/byte budgets at all LODs.
- Repeated named-host Rust trials with raw JSONL and source/build hashes.
- Human review for silhouette, expression legibility, visual comfort and identity continuity.

Do not set timing regression thresholds until a named Rust build and normal run-to-run variance exist. Do not claim a unified watertight face based only on the initial marching-cubes topology checks.

## Reproduction boundary

The initial measurements were generated by an offline Python reference experiment. A runnable Rust-first lab and machine-verifiable raw JSONL are future deliverables. No Python dependency is introduced into production. No SIGGRAPH 2026 method implementation, production mesher or GPU renderer is included by this documentation/fixture commit.


## Maintained SIGGRAPH 2026 reference adapter

The current benchmark now pins the **maintained** Gpytoolbox implementation, rather than the archived author reproduction repository:
- Gpytoolbox release: `v0.3.8`, published 2026-10-06.
- Exact Git tag commit: `b6e2c2b4330b0b7c0dcc5c84f6282b8fa6080ebe`.
- API: `dual_contouring_of_signed_distance_data`.
- API source at the tag: https://github.com/sgsellan/gpytoolbox/blob/v0.3.8/src/gpytoolbox/dual_contouring_of_signed_distance_data.py
- Configuration: [`dcsdd-reference-config-v1.json`](dcsdd-reference-config-v1.json).

The configuration pins all method parameters to the maintained API defaults: 100 outer iterations, 100 inner iterations, batch size 200,000, Hermite updates enabled, `mu=0.1`, `dc_weight=0.02`, `svd_threshold=0.01`, and the three update blend weights at 0.2. It also pins the CPython 3.13 manylinux wheel SHA-256.

### Run the maintained reference

Install the pinned package and the existing research dependencies:

```sh
python -m pip install gpytoolbox==0.3.8
python benchmarks/ouroboros-face-sdf/compare-extractors-reference.py \
  --resolution 64 --smooth-width 0.004 --trials 10 \
  --include-dcsdfd --dcsdfd-warmups 3 --dcsdfd-trials 10 \
  --output /tmp/ouroboros-dcsdd-64.json
```

The opt-in adapter:
- Converts the same signed-int32 microunit sample vector to float64 only at the Gpytoolbox API boundary.
- Constructs `GV` with each coordinate component flattened in Fortran order, matching `i + nx * (j + ny * k)`.
- Passes the pinned outer/inner iteration and QEF/Hermite parameters.
- Retains DCSDFD's quads as the primary output and creates a separate fixed-diagonal triangle view only for the common Trimesh topology diagnostics.
- Reports p50/p95/p99/max timing from the requested measured trials, output counts, quad-buffer estimate and field residual.

**Important input-provenance limitation:** this Python harness regenerates the canonical fixture sample vector from the pinned analytic field definition and checks its little-endian int32 microunit payload hash against the recorded sample SHA-256. It does not yet parse the Rust `OUROGRD1` container directly. The exact sample vector/hash and grid coordinates are held constant across the extractors, but direct Rust-container ingestion remains a follow-on integration step.

The current face field uses the IQ-style approximate ellipsoid distance, not an exact Euclidean SDF. Therefore this fixture supports application-specific comparison, but it cannot be the sole accuracy oracle. The benchmark plan in [Issue #48](https://github.com/Luminous-Dynamics/sovereign-boot/issues/48) keeps exact analytic SDFs, mesh-derived SDFs and the face fixture as separate strata.

No maintained DCSDFD output has yet been measured by this adapter. The existing results above remain marching-cubes and a simple classical QEF reference. The Rust QEF crate in [PR #47](https://github.com/Luminous-Dynamics/sovereign-boot/pull/47) remains a control baseline, not a DCSDFD port.


## Published SIGGRAPH 2026 reference metrics

The paper's reported ABC CAD values and noise-study values are transcribed separately in [`dcsdd-paper-baseline-v1.json`](dcsdd-paper-baseline-v1.json). They are published reference data, not Ouroboros measurements.

For the paper's 55-shape ABC subset, DCSDFD reports (Chamfer ×10^-3 / Hausdorff ×10^-2 / edge-Chamfer / SDF energy ×10^-5 / vertices):
- 50³: 2.958 / 2.970 / 0.03787 / 1.074 / 3.4k
- 100³: 0.7788 / 1.221 / 0.02622 / 0.05776 / 14k
- 150³: 0.4432 / 0.7037 / 0.01684 / 0.241 / 32k
- 200³: 0.4576 / 0.6741 / 0.01599 / 0.4359 / 57k

The paper reports a 100³ noise study across ten shapes at 0.1%, 0.5% and 1.0%. At 1.0% noise, DCSDFD's reported Chamfer / Hausdorff / edge-Chamfer are 10.79 ×10^-3 / 8.729 ×10^-2 / 0.1138. This is why we keep noise robustness as a separate acceptance dimension rather than inferring it from clean data.

Paper details that should be reproduced:
- 55 randomly selected ABC CAD shapes, 50³/100³/150³/200³.
- Ten smooth-only shape controls; the paper reports mixed results and performance roughly on par with the strongest prior methods.
- Ten-shape, 100³ noise sweep at 0.1%, 0.5%, 1.0%.
- A random rotation is applied for each reported result to control grid alignment.
- 100 maximum inner and outer iterations and 200,000 maximum random SDF sample batch.
- At high resolutions, the paper discusses narrow-band SDF samples or randomized per-outer-iteration batches.

The raw ABC shape IDs/hashes, per-shape rotations, exact noise scaling details and full Table 3 transcription are not yet captured in the JSON. These gaps are explicitly listed; do not pretend this is a complete reproduction dataset.
