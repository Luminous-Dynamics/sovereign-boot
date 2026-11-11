# Ouroboros signed/unsigned surface-extraction benchmark

**Status:** benchmark contract and paper snapshots only. No Ouroboros extraction result is reported here.

Tracking: [Issue #64](https://github.com/Luminous-Dynamics/sovereign-boot/issues/64)  
Signed adaptive tree: [PR #57](https://github.com/Luminous-Dynamics/sovereign-boot/pull/57)  
Topology readiness: [PR #61](https://github.com/Luminous-Dynamics/sovereign-boot/pull/61)  
Adaptive transition mesher proposal: [Issue #62](https://github.com/Luminous-Dynamics/sovereign-boot/issues/62)  
Unsigned-distance track: [Issue #60](https://github.com/Luminous-Dynamics/sovereign-boot/issues/60)

## Why a single leaderboard is invalid

The literature contains methods with materially different input authorities:

- DCSDFD consumes discrete **signed** distance samples on a regular grid and does not require arbitrary field evaluation or external gradients.
- The power-diagram adaptive signed-SDF method uses SDF values **and gradients**, with incremental regular-Delaunay updates and additional field queries.
- McGrids uses a continuous SDF evaluator with Monte-Carlo-driven adaptive-grid sampling and midpoint refinement.
- DCx and VoroUDF consume **unsigned** distance fields, but with different assumptions: DCx uses neural/ground-truth UDFs and expanded-cube templates; VoroUDF optimizes movable seeds with tangent/repulsion terms.
- SpUDD targets finite **discrete unsigned** distance samples.

These methods must be compared within compatible field/topology strata. A signed-solid extraction score cannot be ranked directly against an unsigned method designed to preserve open or non-manifold surfaces.

## Two required fairness views

1. **Frozen-sample track:** every method receives the same canonical source samples, field descriptor, bounds, coordinate frame, quantization and isovalue. No extra gradient, projection or neural query may be silently added. If an algorithm cannot consume this input, mark it unsupported for this track.
2. **Method-appropriate information track:** supply every input a method requires, including gradients or learned-field inference where applicable, but count the additional information and computation explicitly.

Every trial separates:
- unique distance-value samples and evaluator calls;
- gradient/normal queries;
- surface projections, ray samples and sample-to-seed assignments;
- adaptive site insertions, local updates and Delaunay/mesh work;
- output construction and validation.

Do not treat gradients as free. Do not handicap a method that requires gradients by withholding them in its method-appropriate track.

## Reference method registry

The machine-readable registry is `methods-v1.json`. It distinguishes:
- published method / paper snapshot;
- public implementation verified;
- implementation pinned to an immutable commit;
- implementation not located or not yet independently qualified.

Notable current references:
- Gpytoolbox maintained DCSDFD source pinned to commit `d6e6ae3b5ff9200e687ca19c178ff35d65c1f810`.
- Official McGrids source pinned to `6c24f0968c82b00472130303a1a532b15626dbb1`.
- Official DCx source pinned to `68ab8d8a60be93f8fa139f2a17721ba461bc297d`.
- VoroUDF and SpUDD are paper snapshots here; no official implementation was verified in this review.
- The power-diagram adaptive SDF method is a paper snapshot here; no official code repository was verified in this review.

### McGrids query-accounting caveat

The public McGrids Python wrapper increments its internal `query_count` in `__sdf__()`, but the adaptive Monte-Carlo sampling loop calls `self.sdf_func(sample_points)` directly, bypassing that counter. Therefore the wrapper's internal counter is not an authoritative total for that code path. Benchmark adapters must wrap the field callable at the outer boundary and count every invocation/point, while preserving the method-reported counter separately as a diagnostic.

This is a source-inspection finding, not a claim that the paper's published results are incorrect. The paper's query-count snapshots are preserved separately and must not be recomputed from the wrapper's incomplete counter.

## Metric contract

Use explicitly named metrics with units and normalization:
- symmetric L2 Chamfer;
- bidirectional and maximum Hausdorff;
- edge/feature Chamfer;
- landmark displacement;
- normal-angle error;
- source-field residual;
- component precision/recall;
- Euler characteristic difference;
- boundary-loop count and boundary-curve Chamfer;
- non-manifold edge/vertex incidence and non-manifold-curve Chamfer;
- thin-sheet/gap recall;
- combinatorial mesh audit;
- independent self-intersection result.

A metric record must bind its implementation digest, normalization convention, point/sample count and random seed. Paper-reported values remain in `published-baselines-v1.json` with their original dataset, scale and units. Never normalize them into one artificial cross-paper score.

## Acceptance targets (hypotheses, not results)

- **Input equality:** byte-identical canonical source samples for every compatible method in a frozen-sample comparison.
- **Accounting:** zero uncounted evaluator calls in the external wrapper; method-reported counts retained separately.
- **Topology:** zero cracks or combinatorial non-manifold violations for accepted closed-manifold analytic fixtures once adaptive transition extraction exists.
- **Open/UDF cases:** preserve valid boundary loops and non-manifold structures where that is the source topology; do not impose closed-manifold assumptions.
- **Geometric quality:** report Chamfer/Hausdorff and feature/edge Chamfer at matched output complexity.
- **Information efficiency:** report quality vs distance-value, gradient, projection and neural inference budgets separately.
- **Determinism:** identical source, algorithm build, parameters and deterministic mode produce identical canonical output.
- **Unknowns:** timeouts, unsupported cases and incomplete audits never become numeric zero or a successful quality result.

No absolute latency target or SOTA-superiority claim is set before named-host baselines exist.

## Raw trial format

- `trial-v1.schema.json` defines one machine-readable trial record suitable for JSONL.
- `example-trial-v1.json` contains deliberately synthetic placeholder hashes and an Incomplete status. It is schema documentation, not a measured result.
- `published-baselines-v1.json` records published values separately from executable trial records.

Aggregation is permitted only when source geometry, field semantics, coordinate frame, metric definitions, normalization, sample budget and host/build pins match.