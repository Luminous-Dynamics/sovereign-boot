# Ouroboros adaptive sampling benchmark

**Status:** machine-readable exact-analytic workload contract. The fixtures are not performance results. The adaptive sampler is in draft [PR #57](https://github.com/Luminous-Dynamics/sovereign-boot/pull/57).

## Comparison question

At the same source field, coordinate frame, bounds, isovalue and quantization, how many unique samples can an interval-certified adaptive octree use while preserving every surface component and declared feature found by dense uniform controls?

The benchmark deliberately separates three propositions:

1. **Certificate correctness:** no cell is declared empty/inside when an independent denser evaluator finds a contrary witness.
2. **Sampling efficiency:** adaptive unique sample count relative to a matched dense grid.
3. **Reconstruction quality:** a later transition-cell mesher's surface, topology and feature metrics. The current sampler alone cannot make a mesh-quality claim.

## Fixture strata

`fixtures-v1.json` contains six exact analytic fields: enclosed sphere, axis-aligned box, rotated box, capsule, ring torus and capped cylinder. Their source samples start on a 2×2×2 lattice. This intentionally tests whether conservative intervals can discover enclosed components that sign-change-only refinement misses.

Three additional controls exercise exact-zero handling, an atomic sample-budget stop and balance-check budget exhaustion. These should retain unresolved status and explicit stop reasons rather than fabricate a complete tree.

Approximate Ouroboros face fields and mesh-derived SDFs are not in the exact-certificate stratum. They require separate, source-bound Lipschitz authorities before this algorithm may certify cells for them.

## Matched controls

Dense uniform sample counts:

| Resolution | Samples |
|---|---:|
| 32³ | 32,768 |
| 64³ | 262,144 |
| 128³ | 2,097,152 |

Use the identical field descriptor, bounds, coordinate frame, isovalue, sign convention and microunit quantization for the adaptive and uniform cases. The 128³ uniform grid is the dense reference control for witness/error checks; 32³ and 64³ are also useful efficiency baselines.

## Trial protocol

The fixture declares depths 4–8, sample budgets 4,096–65,536, leaf budgets 512–4,096, three balance-check budgets, two warmups and five repeats. Preserve every raw run. Report median and tail latency, not only the best run. Record CPU, memory, OS/kernel, Rust compiler/profile, source commit, exact configuration, source-grid digest and output receipt digest.

Independent dense witnesses must be generated from the decoded exact analytic field, not copied from the adaptive octree's own cache. For each `CertifiedInside` and `CertifiedOutside` leaf, sample a deterministic interior witness lattice and check the sign. Also compare connected components and thin-feature/gap witnesses at higher resolution. A finite witness set is a regression oracle, not a general mathematical proof; the actual proof comes from the verified exact field plus the conservative interval bound.

## Required metrics

Report sample reduction fraction, unique cache entries, cache hits, split count, leaf depth histogram, certified inside/outside/surface-possible fractions, budget-stopped leaf count, balance state, remaining-violation count or unknown sentinel, false certificate witnesses, missed components, feature/gap preservation, latency p50/p95/p99, peak RSS, working-payload estimate and canonical receipt bytes.

Do not combine these into one weighted leaderboard score. Sample reduction is only meaningful at matched surface/feature error and zero false certificates.

## Proposed target

Aim for at most 40% of the equivalent dense-grid sample count on feature-sparse exact fields, with zero false empty-space certificates against the independent dense witness protocol. Both are hypotheses, not results. A higher sample count can be the correct outcome when the field has more surface complexity or the declared error target is stricter.

## Interpretation and non-claims

- The octree's `CertifiedInside`/`CertifiedOutside` labels are conditional on `VerifiedExactField` reconstructing the complete canonical source grid and on the built-in exact primitive being 1-Lipschitz.
- Quantized samples are not exact real numbers; the one-microunit guard is part of the bound.
- A `BalanceStatus::Balanced` result means only face-neighbor 2:1 balance in this version.
- `BudgetStopped`, `SurfaceAtMaximumDepth` and `BalanceIncomplete` are incomplete outcomes, not quality passes.
- An adaptive cell tree is not a mesh. Watertightness, manifoldness, self-intersection, perceptual quality and rendering performance require separate downstream evidence.
- No measured sample reduction, runtime win, energy saving or SOTA superiority is currently claimed.