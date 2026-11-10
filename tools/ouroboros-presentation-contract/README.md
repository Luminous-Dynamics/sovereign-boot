# Ouroboros presentation contract oracle

A dependency-free Rust prototype for the identity and negotiated-presentation rules proposed in [issue #29](https://github.com/Luminous-Dynamics/sovereign-boot/issues/29).

## Run

From the repository root:

```sh
cargo test --manifest-path tools/ouroboros-presentation-contract/Cargo.toml
```

The crate is intentionally isolated from the repository's production Cargo workspace and scene-core dependency graph. Its own workspace and lockfile make the standalone test command self-contained. It uses only the Rust standard library and does not query a device, render a frame, open a socket, inspect the desktop or sign evidence.

## Current prototype

- `EntityDescriptor` separates stable `EntityId` from mutable representation roots. Its canonical bytes are the revision input; the resulting digest is not the entity ID.
- `QualityRequest` separates hard bounds from soft visual preferences, including independent CPU/GPU time and memory ceilings.
- `CapabilitySnapshot` contains candidate plans and a revision. Candidate IDs and host preference ranks must be unique, but input order is not semantic; canonical `OUROCAP2` bytes sort by stable candidate ID. The existing first-feasible baseline selects by `(preference_rank, candidate_id)`.
- `negotiate` deterministically selects the first feasible host-ranked candidate, records soft degradation, or returns an explicit infeasible decision. `OURODEC2` records the revised permutation-invariant capability encoding.
- `negotiate_pareto` is a separate explicit policy path: hard-filter candidates first, calculate nondominated alternatives across geometry/compute/memory/feature/silhouette dimensions, then let host preference select only among the frontier. Every candidate receives a reproducible status: Infeasible, Indeterminate, Dominated, Frontier or Selected, with hard rejection flags or a stable dominator/reason.
- `DominanceMode::PointEstimate` compares declared nominal estimates. `ConservativeIntervals` only compares candidates with declared absolute uncertainty bounds; it compares worst-case cost upper bounds to other candidates' best-case lower bounds and reverses the direction for feature diameter. Missing uncertainty remains Indeterminate, never zero. These are engineering bounds supplied by the host, not automatic statistical confidence intervals.
- `DominanceMode::EpsilonGrid` compares objective buckets using a separately declared positive epsilon for every metric in its own units (vertices, indices, bytes, nanoseconds, memory, feature diameter and silhouette error). Candidates within the same complete bucket vector are equivalent for that policy and resolve by stable candidate ID. Epsilon widths are policy parameters, not uncertainty and not weights.
- The policy does not use a hidden weighted scalar. The selected candidate is the lowest host preference rank within the nondominated frontier, with candidate ID as the deterministic tie-break. The full frontier and dominated/infeasible explanations are embedded in `OUROPDE2` decision bytes. Policy parameters are bound by `OUROPOL2`. EpsilonGrid uses fixed positive bucket widths for each metric; changing any width changes the canonical policy bytes.
- `verify_quality_decision` recomputes negotiation from the original request, capability snapshot and policy version, then compares canonical decision bytes; altered candidate selection, degradation or stale capability revision is rejected. A CPU-only candidate can advertise zero GPU time/memory and remain feasible under a zero-GPU budget.
- `FrameReceipt` binds the entity/revision/state/decision roots, backend build digest, target format/extent, pool generation, terminal status, evidence level, optional session clock ID and presentation observation. The session clock is recorded even for Submitted/Discarded/Unknown receipts when known, and must equal the clock inside any Presented observation. Its Wayland v2 adapter combines split 32-bit timestamp/refresh-sequence fields with checked arithmetic and retains refresh interval, output identity when available and source flags. Zero refresh/sequence values remain valid.
- Wayland `hw_clock`, `hw_completion` and `zero_copy` bits remain compositor-reported flags; they do not upgrade the receipt's evidence level to independent hardware observation. Feedback for an unregistered frame is rejected before mutating the session's learned clock state. The Wayland adapter requires exactly `CompositorFeedback`; it cannot claim `HardwareObserved` or `ExternalInstrument` without a separate evidence source. Presented requires a validated observation. Discarded is source-bound but has no presentation timestamp.
- `SessionBinding` and `FrameSequenceGuard` pin every frame to one entity ID/revision, backend build, negotiated decision, session, target format/extent, buffer generation and (when known) the connection's presentation clock ID. The first observed clock is locked for the session. Reconfiguration requires a strictly newer pool generation; frame sequence remains monotonic. `PresentationFeedbackLedger` is explicitly session-bound and has a monotonic retirement watermark: completed frame outcomes can be pruned after the host has consumed them, while late callbacks for retired sequences are rejected. This lets a long-lived 60 Hz session recycle a bounded ledger instead of permanently exhausting its capacity.
- `PresentationFeedbackLedger` registers host-owned surface commits and deduplicates identical terminal feedback for each frame sequence. Presented and Discarded are mutually exclusive; unknown, out-of-order, conflicting or late/unregistered feedback is rejected. The ledger is bounded; after the host consumes terminal records it may explicitly retire them, and the monotonic retirement watermark rejects late callbacks so settled entries can be pruned safely.

Capability snapshots are permutation-invariant: candidate IDs and host preference ranks must be unique, canonical `OUROCAP2` bytes sort by candidate ID, and the first-feasible baseline selects by rank then ID. The capability/decision encodings were bumped to schema v2 because the candidate record now includes an optional uncertainty vector. The crate emits canonical little-endian byte strings with versioned magic values. Hash those exact bytes using the chosen project digest at the integration boundary. This avoids coupling the conformance oracle to a cryptographic implementation; the content digest is not itself an authentication claim.

## Important limitations

This is a first oracle, not a production runtime contract. Candidate cost fields are planning estimates, not measured GPU/CPU telemetry. Pareto covers only the dimensions presently modeled; energy remains unknown and is not silently treated as zero. It does not yet implement actual host capability discovery, live Wayland/Vulkan/WGPU calls, temporal receipt aggregation, signatures, independent hardware capture or visual-perception scoring. Candidate cost values remain estimates; observed render/presentation telemetry is a separate evidence stream. Frame receipt canonical encoding is versioned as `OUROFRM2` after adding protocol-preserving presentation observations. Capability and baseline decision encodings are `OUROCAP2` and `OURODEC2`; the separate Pareto policy and result are `OUROPOL1` and `OUROPDE1`. The exact-head Rust compile/test result is pending; see the associated PR rather than treating authored tests as passing evidence.


## Pareto adversarial corpus

The current source contains 54 authored unit tests. The Pareto additions cover:
- A resource/perceptual trade-off where both alternatives remain nondominated.
- Strict dominance and a stable dominator explanation.
- Hard-budget filtering before dominance.
- Capability and decision byte invariance under candidate-array permutation.
- Identical metric-vector tie resolution by candidate ID rather than insertion order or host rank.
- Conservative interval dominance and the Indeterminate outcome when uncertainty is missing.
- Overlapping uncertainty intervals that remain incomparable.
- No-candidate-with-required-uncertainty as an explicit result.
- Feasible-set preservation under a relaxed hard ceiling.
- A newly added feasible candidate that dominates and removes previous frontier members.
- Epsilon-grid equivalence, stable-ID tie-breaking, per-metric positive epsilon validation and policy-byte binding.
- Decision recomputation and mutation rejection.

The 54 tests are authored, not run; no compile or CI pass is claimed.
