# Ouroboros Pareto negotiation benchmark

**Status:** machine-readable synthetic comparison corpus. It is not a measured performance report. The fixtures are designed to exercise the policies implemented in `tools/ouroboros-presentation-contract`.

Tracking: [Issue #36](https://github.com/Luminous-Dynamics/sovereign-boot/issues/36)  
Implementation: [Draft PR #30](https://github.com/Luminous-Dynamics/sovereign-boot/pull/30)

## Why this benchmark exists

A renderer quality planner needs more than a preset and more than a single host-ranked first-feasible candidate. The existing first-feasible selector is retained as a baseline. The new Pareto path first filters hard constraints, finds nondominated candidates across independent resource and perceptual objectives, and lets host preference select only among the resulting frontier.

The benchmark makes this comparison explicit without inventing a weighted scalar. It also records when conservative interval comparisons cannot be made because a host has not supplied uncertainty bounds.

## Policy modes

- **PointEstimate:** uses nominal declared candidate estimates. This is useful for testing the selection algorithm, but the estimates remain planning inputs, not measurements.
- **ConservativeIntervals:** requires an absolute uncertainty bound for every objective dimension. A candidate A dominates candidate B only when A's worst-case costs are no greater than B's best-case costs in every minimized dimension and A's worst-case feature size is no smaller than B's best-case feature size. Overlapping intervals remain incomparable. Missing uncertainty remains Indeterminate, never zero.
- **EpsilonGrid:** uses an additive bucket width in the native unit of each metric. The full vector of buckets determines equivalence and dominance; an equivalent bucket vector resolves by stable candidate ID. Epsilon values are policy tolerances, not confidence bounds or objective weights.

The exact policy and decision inputs are included in `OUROPOL2` and `OUROPDE2` canonical bytes. Capability snapshot array order is non-semantic; `OUROCAP2` canonical bytes sort candidates by stable ID.

## Objective vector

| Direction | Objective | Unit |
|---|---|---|
| Minimize | Vertex count | vertices |
| Minimize | Index count | indices |
| Minimize | Geometry bytes | bytes |
| Minimize | Frame-time estimate | ns |
| Minimize | CPU-time estimate | ns |
| Minimize | GPU-time estimate | ns |
| Minimize | CPU memory estimate | bytes |
| Minimize | GPU memory estimate | bytes |
| Minimize | Silhouette error | microunits |
| Maximize | Feature diameter | millipixels |

Target format and animation mode are categorical compatibility boundaries: candidates with different values are not compared against each other. Reduced Motion remains a hard constraint. Energy is deliberately absent because no qualified, source-bound energy measurement exists in the candidate contract. Do not fill it with zero.

## Fixture cases

`fixtures-v1.json` covers:
1. Resource/perceptual trade-off with two frontier members.
2. Strict dominance with one stable dominator.
3. Hard-budget rejection before Pareto comparison.
4. Missing uncertainty as Indeterminate, not zero.
5. Overlapping uncertainty intervals that remain incomparable.
6. Epsilon-grid equivalence and stable-ID tie-breaking.
7. A larger candidate universe that contracts the frontier by adding a feasible dominator.

Expected dispositions, frontier IDs, selected IDs, dominator explanations and hard rejection reasons are included per fixture.

## Metrics and report contract

Every run should report:
- feasible / infeasible / indeterminate candidate counts;
- frontier count and frontier fraction;
- dominated fraction and dominant reason;
- hard rejections by constraint dimension;
- chosen candidate and whether the first-feasible baseline would have selected a dominated candidate;
- byte-identical capability/decision receipts under candidate input permutations;
- verifier agreement after recomputing from request/capability/policy bytes.

The benchmark must **not** require frontier size to be monotonic when the candidate set grows or hard constraints relax. The invariant is feasible-set inclusion under relaxed hard constraints. A newly added feasible alternative can dominate a previous frontier candidate and reduce frontier size.

Performance runs should use 1, 4, 16, 64 and 128 candidates, ten warmups and at least fifty measured trials on named hosts, reporting p50/p95/p99/max decision latency and host/build metadata. No latency or throughput target is set until a reproducible Rust baseline exists.

## Interpreting the first-feasible comparison

The host-rank baseline is not a truth oracle; it represents the host's declared preference. A useful non-scalar differentiator is the **dominated-baseline-selection rate**: how often the first-feasible host rank chooses a candidate that the same candidate set and request show to be dominated by another feasible candidate.

Do not call this utility regret. If an evaluator needs a scalar utility or regret measure, that evaluator must publish its objective weights, constraints, calibration data and sensitivity analysis separately.

## Verification and limitations

- These fixtures are synthetic and machine-readable.
- The Rust code has 54 authored unit tests, but the current environment has no Rust toolchain; tests have not been compiled or executed.
- No performance, quality, power or industry-superiority claim is made.
- Candidate estimates and uncertainty margins are host-supplied planning data; no independent measurement calibration exists yet.
