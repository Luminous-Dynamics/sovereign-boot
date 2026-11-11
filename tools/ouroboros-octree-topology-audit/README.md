# Ouroboros adaptive octree topology audit

**Status:** independent structural audit and transition-readiness gate. It does not emit geometry and is not a mesh-validity certificate.

Tracking: [Issue #59](https://github.com/Luminous-Dynamics/sovereign-boot/issues/59)  
Input tree: [PR #57 — certified adaptive SDF octree sampling](https://github.com/Luminous-Dynamics/sovereign-boot/pull/57)  
Generic source grid: [PR #54](https://github.com/Luminous-Dynamics/sovereign-boot/pull/54)

## Build and run

```sh
cargo test --manifest-path tools/ouroboros-octree-topology-audit/Cargo.toml
cargo run --release --manifest-path tools/ouroboros-octree-topology-audit/Cargo.toml \
  --example audit-exact-sphere-octree -- /tmp/ouroboros-exact-sphere-topology.ourocf
sha256sum /tmp/ouroboros-exact-sphere-topology.ourocf
```

The example constructs the same exact sphere and coarse 2×2×2 source lattice as PR #57, builds the adaptive octree, independently audits it, and writes the canonical `OUROTCF1` certificate. The tree receipt embeds the complete `OUROSGD1` source grid. The topology certificate embeds the complete `OUROOCT1` tree receipt. Hashing/signing remains external.

## What is independently checked

1. **Morton path geometry:** decode every octant digit and prove the leaf's maximum-depth tick origin and span match its `(level, path_code)`.
2. **Complete partition:** reject ancestor/descendant overlap, require every leaf within the root, and verify the dyadic leaf-volume sum equals the root volume. Under the dyadic Morton model, prefix-free leaves plus volume equality prove a complete, non-overlapping partition.
3. **Sample-cache parity:** recompute all eight leaf-corner coordinates using the sampler's floor-to-microunit mapping; each corner must exist in the shared sample cache.
4. **Classification parity:** recalculate every leaf's 12-bit sign-changing-edge mask and corner min/max from the cached samples. Compare all values to the stored classification and check signed interval bounds/status consistency.
5. **Stop-state consistency:** a surface-pending leaf must be below max depth; a max-depth surface leaf must say so; budget/balance stops retain their explicit reason. A balance-stopped **certified** neighbor is allowed because balancing may need to split certified coarse cells.
6. **Full 26-neighborhood balance:** compare leaf pairs and classify contacts as face, edge or corner. Report pair counts, maximum level gaps and 2:1 violations separately for each contact type. A tree that only passes face-neighbor 2:1 does not automatically pass edge/corner balance.

## Readiness semantics

| Result | Meaning |
|---|---|
| `AuditIncomplete` | The bounded pair scan could not complete. Neighbor counts/violations are `u64::MAX` (unknown), never zero. |
| `TreeIncomplete` | Budget/balance stops, unresolved refinement or incomplete source-tree balancing remain. |
| `SurfaceUnresolved` | A max-depth SurfacePossible leaf has no sign-changing edge, or the full 26-neighborhood scan finds a depth-gap violation, or an isovalue-zero corner remains ambiguous for the extractor. |
| `ReadyForTransitionTemplateEvaluation` | Complete partition, exact cached-corner/classification parity, no unresolved surface leaf and no face/edge/corner 2:1 violations. |
| `NoSurface` | The complete tree contains no SurfacePossible leaf. This is not a mesh candidate. |

**Important:** `ReadyForTransitionTemplateEvaluation` is only permission to evaluate a future transition-case implementation. It does not guarantee cracks will be absent, a mesh will be manifold/watertight, triangle intersections are absent, or identity/perceptual quality is preserved. The next mesher must still prove its own local transition templates and run the independent mesh audit. Self-intersection remains NotEvaluated until a qualified geometric oracle is attached.

## Resource and evidence bounds

Default caps:
- 4,096 leaves inherited from the sampler.
- 8,400,000 pair comparisons maximum (all pairs of 4,096 leaves fit below this bound).
- 128 MiB estimated working payload by default, with a 256 MiB crate ceiling.
- 32 MiB canonical receipt by default, with a 64 MiB crate ceiling.

The working estimate includes two source-tree workspaces during deterministic report verification, the source tree receipt, leaf/order vectors and audit structures. It remains a payload estimate, not an OS-enforced RSS cap.

The certificate uses fixed-width fields and exact canonical-length accounting. It binds the full `OUROOCT1` source receipt, policy/schema versions, readiness, checks and metrics. It does not authenticate the source build or sampling process.

## Authored adversarial tests

The 11 authored tests cover:
- independent leaf partition and cache/sign-mask verification on an exact sphere;
- separate face/edge/corner contact classification;
- a corner-only 2:1 violation that face-only balance would miss;
- a maximum-depth SurfacePossible leaf with zero crossing mask;
- exact-zero isovalue corners blocking transition readiness;
- exact canonical receipt-length accounting;
- pair-budget exhaustion as Unknown/Incomplete, not zero;
- no-surface trees reported separately;
- forged Morton-path origin rejection;
- byte-deterministic audits and receipt verification.

These tests are authored, not compiled or executed. No Rust compile/test or CI pass is claimed.

## Next step: transition-cell meshing

The next issue should build a bounded table-driven transition extractor over this qualified tree. It must:
- enumerate all sign-changing primal edges and their incident leaf rings across 1:1 and 2:1 face/edge/corner neighborhoods;
- use explicit transition templates with checked local orientation and no duplicate/zero-area output;
- fail closed when a neighborhood is unsupported or incomplete;
- emit the mesh plus a receipt binding `OUROTCF1`, the exact sample map, template-set version, vertices/faces and independent topology audit;
- compare against uniform-grid marching cubes, classical uniform QEF dual contouring and the maintained SIGGRAPH 2026 DCSDFD method on the same exact source samples.

No transition mesh, crack-free result, manifold guarantee, self-intersection test, renderer benefit or SOTA improvement is claimed by this audit crate.
