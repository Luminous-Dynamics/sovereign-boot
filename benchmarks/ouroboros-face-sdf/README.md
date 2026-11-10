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

| Grid | Estimated production-layout bytes | Existing vertex ceiling |
|---|---:|---|
| 64³ | 254,568 | Fits |
| 96³ | 583,600 | Fits |
| 128³ | 1,044,016 | Fits |
| 192³ | 2,362,344 | **Fails vertex ceiling** |

The 192³ case is below the byte ceiling but exceeds the independent vertex ceiling. This is exactly why negotiated geometry quality must retain separate, explicit limits rather than one aggregate budget.

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
