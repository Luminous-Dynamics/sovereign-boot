# Ouroboros bounded classical dual-contouring extractor

**Status:** fixed-grid CPU research implementation, source-reviewed but not compiled or executed. This is a classical QEF dual-contouring baseline, not the SIGGRAPH 2026 discrete-SDF method and not an adaptive/octree implementation.

Tracking: [Issue #46](https://github.com/Luminous-Dynamics/sovereign-boot/issues/46)  
SDF input: [Issue #34](https://github.com/Luminous-Dynamics/sovereign-boot/issues/34) / [Draft PR #35](https://github.com/Luminous-Dynamics/sovereign-boot/pull/35)  
Mesh audit: [Issue #44](https://github.com/Luminous-Dynamics/sovereign-boot/issues/44) / [Draft PR #45](https://github.com/Luminous-Dynamics/sovereign-boot/pull/45)

## Build and run

The crate is an isolated workspace with local path dependencies only:

```sh
cargo test --manifest-path tools/ouroboros-dual-contouring/Cargo.toml
cargo run --release --manifest-path tools/ouroboros-dual-contouring/Cargo.toml \
  --example extract-face-skin -- /tmp/ouroboros-face-skin-dc32.ourodc
sha256sum /tmp/ouroboros-face-skin-dc32.ourodc
```

The example samples the source-derived `OUROLND1`/ `OUROFAC1` head+nose skin field at 32³, extracts a quad mesh, creates the fixed-diagonal triangle view, runs the independent semantic mesh audit and writes the canonical artifact. SHA-256 is an external checking digest, not a signature or provenance attestation.

## Input and algorithm

- Input is an already sampled `SampledGrid` from `ouroboros-sdf-lab`. The extractor never re-evaluates the field and never resamples it.
- It uses the SDF lab's negative-inside, zero-on-surface, positive-outside convention and first-axis-fastest indexing.
- The active-cell criterion is a mixture of negative and nonnegative corner samples.
- Crossings along the 12 cube edges use linear interpolation of the fixed-point sampled values.
- Hermite normals are central finite differences at interior grid vertices and one-sided differences at the sample-grid boundary.
- A zero/ill-conditioned interpolated normal uses the crossed grid edge's inside-to-outside axis direction. This fallback is counted separately from QEF centroid fallback.
- One regularized 3×3 QEF is solved per active cell. Partial-pivot Gaussian elimination uses a deterministic pivot rule. Singular systems fall back to the Hermite intersection centroid. Solutions are clamped to their source cell and clamping is reported.
- Every sign-changing interior grid edge connects its four adjacent active-cell vertices into one quad. Winding is oriented from negative/inside toward positive/outside.
- A deterministic triangle view splits each quad along diagonal (0,2). The quad mesh remains available and canonical.
- A sign-changing edge that touches any grid boundary returns `SurfaceTouchesGridBoundary`; a missing adjacent cell vertex returns an explicit error instead of emitting a partial surface.
- Exact-zero samples are treated as outside for sign classification, but a zero endpoint on a sign-changing edge returns `ExactZeroCrossing`. There is no hidden epsilon perturbation or symbolic tie-break in this first implementation.

## Semantic provenance

The first skin-union field is made from two source primitives: head shell (feature ID 1) and nose (feature ID 4). Every output vertex carries the coarse composite contributor mask `0b01001` and the triangle view uses face region 1 (head-skin composite contract). This is **global source provenance**, not a claim that the extractor has reconstructed local head-vs-nose ownership at every vertex. Local feature attribution is a follow-on requirement.

## Resource ceilings

| Resource | Default maximum |
|---|---:|
| Active cells | 2,097,152 |
| Output vertices | 50,000 |
| Output quads | 62,500 |
| Geometry payload (vertex + quad + triangle data) | 8 MiB |
| Estimated simultaneous working payload | 128 MiB |
| Canonical source-plus-output receipt | 32 MiB |

The working estimate includes the canonical input-grid bytes, sample vector payload, cell-to-vertex map, maximum output buffers, QEF residual vector and an estimate for the independent audit's edge/vertex workspace. It excludes allocator metadata, stack, code pages and total process RSS; it is a preflight payload budget, not a hard operating-system memory cap.

The canonical receipt embeds the full source `OUROGRD1` byte string. It then binds the algorithm/schema, all QEF and resource parameters, fixed-point vertices/normals, quads, fixed-diagonal triangles, QEF diagnostics and the independent audit receipt. It returns bytes, not a digest or signature; callers hash/sign externally.

## Independent audit, not one validity bit

The extractor invokes `ouroboros-mesh-audit` on its triangle view and stores the result. The audit separately reports buffer validity, edge/vertex manifold topology, closure, orientation, computed zero-area/near-degeneracy, semantic provenance and per-component topology. It deliberately reports self-intersections as **NotEvaluated**. The extraction function may return a mesh with failed audit statuses; callers must inspect them rather than equating extraction success with geometric validity.

## Tests authored

The current source contains 14 authored unit tests:
- source-grounded 32³ face extraction produces vertices/quads/triangles;
- repeat extraction emits identical canonical bytes;
- fixed diagonal (0,2) triangulation;
- independent audit's individual dimension statuses and self-intersection NotEvaluated;
- fixed 37-byte extractor policy serialization and threshold binding;
- the canonical result embeds the exact source-grid bytes;
- output vertex hard-limit enforcement;
- cuts on each of the six grid boundaries fail instead of yielding partial geometry;
- exact-zero crossing ambiguity fails closed;
- singular QEF uses centroid fallback;
- too-small grids reject before cell allocation.

These 14 tests are authored, not run. The current execution environment did not provide `rustc`, `cargo` or `rustup`; no compile or passing CI claim is made.

## Comparison plan

All extractors must consume the identical canonical `OUROGRD1` bytes:
1. Existing separate closed volumes with explicit occlusion.
2. Marching cubes reference baseline.
3. This fixed-grid QEF dual-contouring baseline.
4. SIGGRAPH 2026 discrete-SDF dual contouring using the maintained Gpytoolbox implementation as an offline oracle.

Compare topology/audit results, self-intersection oracle findings, silhouette IoU, symmetric Chamfer/Hausdorff, landmark error, QEF residual distribution, vertex/face/byte counts, peak RSS and named-host p50/p95 build times. Do not compare different input fields or call the classical QEF implementation equivalent to the SIGGRAPH method.

## Non-claims

This is not adaptive dual contouring, not the SIGGRAPH 2026 method, not exact-SDF reconstruction, not a self-intersection solver, not a mesh repair operation, not a completed facial model, and not evidence of human likeness, perceptual quality, GPU performance or energy efficiency.
