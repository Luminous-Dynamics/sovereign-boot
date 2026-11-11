# Ouroboros conservative cell-interval oracle

**Status:** first uniform-cell classification oracle over canonical OUROSGD1 samples. Source-reviewed; not compiled or executed. It is not yet an adaptive octree sampler or an adaptive mesher.

Tracking: [Issue #50](https://github.com/Luminous-Dynamics/sovereign-boot/issues/50)  
Input contract: [PR #54 — generic scalar grid](https://github.com/Luminous-Dynamics/sovereign-boot/pull/54)  
Exact field corpus: `tools/ouroboros-scalar-grid`

## Why this is a separate algorithmic step

A naive adaptive sampler refines cells near observed sign changes. That can miss a small closed component entirely contained inside a coarse cell when all eight corners have the same sign. A conservative interval classifier instead asks whether the entire cell can be proven inside or outside under a verified field bound.

For a 1-Lipschitz field (f), each sampled corner (p_i), each point (x) in a cell (C), and the cell's farthest-corner distance (D_C):

[
f(x) ge f(p_i)-D_C
qquad
f(x) le f(p_i)+D_C
]

The intersection of these valid bounds gives:

[
L_C = max_i f(p_i)-D_C,
qquad
U_C = min_i f(p_i)+D_C
]

This implementation includes a one-microunit guard for nearest-integer distance quantization and floating-point sample evaluation. It classifies a cell as:
- `CertifiedOutside` only if (L_C>0);
- `CertifiedInside` only if (U_C<0);
- `SurfacePossible` otherwise;
- `UncertifiedField` if no built-in exact-analytic Lipschitz authority exists.

The strict inequalities matter. Exact-zero samples cannot be classified as empty space.

## What qualifies for a certificate

A declared field semantic label is not enough.

For `ExactAnalyticSdf`, the classifier:
1. Decodes and validates the exact `OUROFLD1` descriptor (sphere, axis-aligned box, oriented box, capsule, ring torus, capped cylinder or plane).
2. Re-evaluates every grid vertex from that descriptor and verifies the exact quantized sample stream, including the isovalue.
3. Verifies every adjacent sample pair along all three lattice axes is consistent with the 1-Lipschitz bound plus quantization allowance.
4. Computes the maximum cell diagonal with integer ceiling arithmetic.
5. Classifies cells using the verified unit-Lipschitz bound and exact canonical samples.

If a caller forges `ExactAnalyticSdf` while changing any sample, step 2 rejects the grid. Unknown descriptor schemas and malformed descriptors fail closed.

Approximate analytic fields, mesh-derived SDFs and unknown sources are not assigned (L=1). They receive `UncertifiedField` records with sign-change diagnostics only. The current Ouroboros IQ-style face field therefore does not receive false mathematical certificates.

The certificate binds the exact canonical OUROSGD1 bytes, decoded field descriptor, isovalue, sample data, classification policy, all per-cell bounds and counts. It is a reproducible content record, not a signature or build-provenance attestation.

## Resource ceilings

| Resource | Default maximum |
|---|---:|
| Input samples | scalar-grid's 2,097,152 sample ceiling |
| Classified cells | 2,048,383 (the cells in a maximum 128³ sample grid) |
| Receipt | 96 MiB |
| Estimated simultaneous payload | 192 MiB |

The working estimate includes the input sample vector, canonical source bytes, in-memory cell records and a second source-plus-record copy while emitting the receipt. It excludes allocator metadata, code pages and process RSS. It is a preflight payload ceiling, not an OS-enforced memory cap.

## Tests authored

The current source contains 12 authored tests:
- exact sphere returns inside/outside/surface-possible classes;
- exact-zero lattice vertices remain SurfacePossible;
- dense 4×4×4 interior probes find no false inside/outside certificate in every certified cell;
- approximate source semantics never receive exact certificates;
- forged exact semantics with modified samples are rejected;
- malformed or unsupported descriptors fail closed;
- adjacent sample Lipschitz consistency rejects discontinuous sample sets;
- all seven built-in exact primitive descriptors decode and reproduce their sample streams;
- canonical receipt is byte-deterministic and recomputable;
- first-axis-fastest cell order is deterministic;
- cell-count, receipt and working-payload limits reject before classification allocation.

These 12 tests are authored, not run. The current environment has no Rust toolchain; no compilation or passing CI claim is made.

## Current limitations

This release classifies the cells of an existing uniform grid. It does not create new samples, split cells, maintain a Morton octree, balance 2:1 neighbors, build transition cells or emit a mesh. It is the evidence-producing predicate needed before those operations can safely spend an adaptive sample budget.

It is also not a proof for arbitrary external distance fields. To support mesh-derived or approximate fields, the next step is a separately justified Lipschitz bound with its own source provenance and validation. Empirical gradient estimates must never be relabelled as a mathematical bound.

## Next experiment

Use the `SurfacePossible` set as the refinement frontier. Four machine-readable cases live in `fixtures/cell-intervals-v1.json`: exact sphere, exact-zero plane, approximate-field and tampered-sample controls. For a future octree sampler:
- cache canonical shared sample coordinates by field/coordinate-frame/point/policy key;
- refine only cells whose conservative interval still contains zero or whose semantic features are at risk;
- keep 2:1 balance as a topology precondition, not as an implicit guarantee of extraction;
- report sample count, cache hits, depth histogram, certified volume, unresolved cells and budget-stopped cells;
- compare against 32³/64³/128³ uniform controls at equal symmetric surface/feature error.

The ≤40% dense-sample target and zero false-empty-cell target in Issue #50 remain hypotheses, not measured results.
