# Ouroboros adaptive octree topology qualification benchmark

**Status:** fixture and protocol definition, not a measured benchmark report. The audit implementation is in a draft PR stacked on adaptive sampler PR #57.

Implementation: [Issue #59](https://github.com/Luminous-Dynamics/sovereign-boot/issues/59)
Adaptive sampler: [PR #57](https://github.com/Luminous-Dynamics/sovereign-boot/pull/57)
Generic source grid: [PR #54](https://github.com/Luminous-Dynamics/sovereign-boot/pull/54)

## Why this is a separate gate

A regular octree can be complete and face-neighbor 2:1 balanced while still presenting edge/corner neighborhood configurations that a future transition mesher cannot safely interpret. A leaf may also have an interval classification of SurfacePossible while having no sign-changing corner edge at the chosen depth. Such a cell is unresolved, not empty and not directly contourable.

This audit independently checks the partition, Morton geometry, source sample-cache consistency, sign masks and all 26-neighborhood contacts. It does not emit a mesh.

## Benchmark strata

Use the six exact analytic cases from the adaptive sampler suite: sphere, box, oriented box, capsule, torus and capped cylinder. Keep approximate analytic and mesh-derived data separate because the current interval authority certifies only exact built-in analytic descriptors. Never transfer the exact certificate to approximate face IQ ellipsoids or mesh-derived fields without new evidence.

In addition to source-field trials, the JSON corpus contains synthetic face/edge/corner contact controls. Those are unit-scale structural fixtures, not geometry-performance comparisons.

## Metrics

Report face/edge/corner neighbor counts, 2:1 violation counts, maximum level-gap per contact type, unresolved surface-possible leaves, surface crossing edge counts, partition volume equality, pair comparisons, unique samples, leaf count, canonical bytes, and latency/RSS on named hardware.

The implementation has a bounded all-pairs scan with a hard maximum of 8.4 million comparisons. On a leaf count above the configured pair budget, it does not reinterpret unknown as zero: uncomputed neighbor and violation counters are u64::MAX and readiness is AuditIncomplete.

## Acceptance

A future transition mesher may only start from ReadyForTransitionTemplateEvaluation. This status requires:
- complete prefix-free Morton partition with exact root-volume equality;
- cached corner samples and 12-edge masks exactly reproduce the stored leaf classifications;
- no source-tree budget or balance incompleteness;
- no SurfacePossible leaf with zero crossing edge mask or exact-zero corner ambiguity;
- completed face, edge and corner neighborhood scan, with no depth gap greater than one.

This is not a claim that a mesh exists, or that the future mesh will be crack-free, manifold, watertight or intersection-free. Those require separate transition-template proofs and independent mesh/geometric audits.

## Performance protocol

For each fixture, use a fixed host/build and repeat 25 measured audits after 3 warmups. Report p50/p95/p99/max, pair comparisons, leaf counts and source receipt sizes. Do not compare different grids or field parameters. No latency target is declared until the first named-host Rust baseline is available.

## Current non-claims

Ten Rust unit tests are authored. They have not been compiled/run in this environment, and no CI pass is claimed. No transition mesh, self-intersection, perceptual, renderer or energy result is claimed.