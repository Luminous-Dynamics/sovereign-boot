# Ouroboros certified adaptive SDF sampling

**Status:** bounded Morton-ordered octree prototype over verified exact analytic fields. Source-reviewed; Rust compilation and tests have not run. This crate samples and classifies cells; it does not extract a mesh.

Tracking: [Issue #50](https://github.com/Luminous-Dynamics/sovereign-boot/issues/50)  
Canonical source: [PR #54 — OUROSGD1 scalar grid](https://github.com/Luminous-Dynamics/sovereign-boot/pull/54)  
Interval authority: [PR #55 — verified cell intervals](https://github.com/Luminous-Dynamics/sovereign-boot/pull/55)

## Paradigm shift: make sampling spend explainable

A uniform grid spends the same sampling budget in empty space and near the surface. Refining only where coarse edges change sign can miss a small closed component entirely contained in a cell whose corners all have the same sign. This implementation instead refines from conservative interval bounds over a verified exact SDF. A shared coordinate-keyed cache ensures each fixed-point location is sampled once. The octree is deterministic, Morton-ordered, and records why each leaf stopped.

## Exact-field authority is mandatory

Input is a canonical `ScalarGrid` (`OUROSGD1`). Before adaptive sampling, `VerifiedExactField::from_grid` requires the `ExactAnalyticSdf` label, decodes a supported `OUROFLD1` descriptor, and reproduces the entire source sample stream including the isovalue. It then exposes a fixed-point sampler constrained to the original bounds.

Supported built-in exact primitives include sphere, axis-aligned box, oriented box, capsule, ring torus, capped cylinder and plane. The mathematical distance functions are 1-Lipschitz. The stored grid is quantized signed-int32 microunits, so the cell interval includes a one-microunit quantization guard.

Approximate analytic fields, mesh-derived fields and unknown sources are rejected by this adaptive sampler. In particular, the Ouroboros head+nose IQ-style ellipsoid estimator is not assigned an L=1 certificate. It remains a separate uncertified application track until a conservative bound is justified.

## Conservative cell interval

For the eight corner samples `s_i` and a conservative upper bound `D_C` on the cell diagonal:

```text
lower(C) = max(s_i) - D_C - 1 microunit
upper(C) = min(s_i) + D_C + 1 microunit
```

The 1-Lipschitz bound gives:

- `CertifiedOutside` only when `lower(C) > 0`.
- `CertifiedInside` only when `upper(C) < 0`.
- `SurfacePossible` otherwise.

Strict inequalities ensure exact-zero lattice points remain unresolved rather than being treated as empty space. Cell diagonal and per-axis widths are rounded upward using checked integer arithmetic.

## Adaptive algorithm

1. Verify the source grid and acquire exact-field sampling authority.
2. Sample the root cell corners into a coordinate-keyed `BTreeMap<[i64; 3], i32>`.
3. Classify the root interval.
4. Split each `SurfacePossible` leaf into eight children until it is certified, reaches the declared maximum depth, or a resource budget stops it.
5. Each split is atomic: all 27 points in its 3×3×3 subdivision stencil, eight child classifications, and sample/leaf/working-payload/receipt budgets are checked before the parent, cache or leaf partition is mutated.
6. Enforce face-neighbor 2:1 balance by splitting the coarser leaf. If a split or the bounded pair-check oracle cannot complete, preserve the relevant leaves and report `BalanceIncomplete` or a leaf-level stop reason.

Leaf keys are ordered by a depth-normalized Morton code, so different levels share a spatially coherent canonical order. The coordinate cache is ordered by fixed-point XYZ, not by insertion order.

The domain must have at least one microunit per maximum-depth tick. The span need not divide evenly by `2^max_depth`: tick coordinates are floored, while each cell extent and its interval diagonal are rounded upward. The root domain endpoints remain exact.

2:1 balancing in this version covers **face-adjacent** leaves only. Edge-only and corner-only adjacency are not covered. Balance is a precondition for a later transition-cell mesher, not a guarantee that a mesh will be manifold.

## Budget and outcome semantics

| Setting | Default |
|---|---:|
| Maximum depth | 8 |
| Unique samples | 65,536 |
| Leaf cells | 4,096 |
| Estimated working payload | 64 MiB |
| Canonical receipt | 32 MiB |
| Pairwise balance checks | 67,108,864 |

Global hard limits cap depth at 18, leaves at 4,096, working payload at 128 MiB, receipt at 64 MiB and pair checks at 67,108,864. The face-neighbor oracle is currently pairwise and quadratic but has a fixed comparison budget. If that budget is exhausted, the report uses `u64::MAX` as an unknown-violation sentinel and says `BalanceIncomplete`; it never reports a false zero.

A failed split does not partially modify the tree. The parent remains a `BudgetStopped` leaf with its original `SurfacePossible` classification. Balance-stopped leaves use `BalanceStopped`. A tree can be balanced while some surface leaves are budget-stopped; these are separate claims.

The working-payload estimate includes conservative per-entry cache and leaf overhead, the canonical source-grid copy, two separately retained descriptor copies, and the canonical receipt buffer that coexists with the tree during serialization. It is still a payload estimate, not an operating-system RSS cap; allocator metadata and process code/stack are excluded. The canonical receipt embeds the exact source grid and ordered sample/leaf records. It identifies bytes; hashing, signing and build provenance are external.

## Canonical output

`OUROOCT1` binds the exact `OUROSGD1` source bytes, source descriptor, isovalue and bounds; every sampled microunit coordinate/value pair; each leaf's Morton key, tick bounds, interval classification, sign-edge mask and stop reason; balance status and violation count/sentinel; sample/cache metrics; and all budget parameters.

`verify(source_grid)` deterministically rebuilds the tree and rejects a mutated cache, leaf, metric or balance outcome. This receipt is evidence of the adaptive-sampling computation, not a signature and not a mesh-quality certificate.

## Authored adversarial tests

The current source contains 14 authored tests:

- An enclosed sphere with all eight root corners outside is still discovered.
- Shared edge/face samples are evaluated once and recorded as cache hits.
- A sample budget stops a split atomically and preserves the parent.
- Odd fixed-point spans use conservative upward-rounded cell widths.
- Domains smaller than one maximum-depth tick are rejected.
- The estimated canonical receipt length matches actual serialization.
- Morton keys sort spatially across different depths.
- Balance-check exhaustion is explicit and does not become zero violations.
- Budget-stopped surface cells remain unresolved.
- An approximate-field label cannot acquire exact sampling authority.
- Exact-zero plane samples remain `SurfacePossible`.
- Completed trees satisfy the declared face-neighbor 2:1 check.
- Initial working-payload and receipt budgets reject before tree sampling allocations.
- Repeated builds produce byte-identical receipts; mutated reports are rejected.

These 14 tests are authored, not compiled or run. No Rust test or CI pass is claimed.

## Benchmark contract

Compare to dense uniform exact-grid controls at 32³, 64³ and 128³ with the same field descriptor, bounds, coordinate frame, isovalue and quantization. Report unique adaptive samples versus dense sample count, cache-hit ratio, split count, leaves by depth, certified inside/outside/surface-possible fractions, budget stops, 2:1 balance distribution, false-empty certificate count against a denser independent evaluator, missed-component rate, thin-feature/gap preservation, latency p50/p95/p99, peak RSS and receipt bytes.

Proposed target hypothesis: on feature-sparse exact fields, reach the same declared surface/feature error with at most 40% of dense uniform samples and zero false empty-space certificates. This is a target, not a measured result.

Start with the exact sphere, box, rotated box, capsule, torus, capped-cylinder and plane fixtures. Add fixed-seed rotations, half-cell translations, sub-voxel features/gaps and multiple components. Keep the approximate Ouroboros face field in a separate application stratum.

## Follow-on

After this sampler is qualified: add semantic feature-risk triggers (refinement hints, never proofs), replace the quadratic neighbor scan with a bounded spatial face index, implement transition-cell extraction, and compare the adaptive pipeline against uniform controls, marching cubes, classical QEF dual contouring and maintained SIGGRAPH 2026 DCSDFD on identical exact samples.

No mesh, watertightness, manifoldness, self-intersection, visual identity, renderer-performance, energy or SOTA-superiority claim is made by this sampler.