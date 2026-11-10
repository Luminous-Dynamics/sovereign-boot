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
- `CapabilitySnapshot` contains explicitly ordered candidate plans and a revision.
- `negotiate` deterministically selects the first feasible candidate, records soft degradation, or returns an explicit infeasible decision.
- `verify_quality_decision` recomputes negotiation from the original request, capability snapshot and policy version, then compares canonical decision bytes; altered candidate selection, degradation or stale capability revision is rejected. A CPU-only candidate can advertise zero GPU time/memory and remain feasible under a zero-GPU budget.
- `FrameReceipt` binds the entity/revision/state/decision roots, backend build digest, target format/extent, pool generation, terminal status, evidence level, optional session clock ID and presentation observation. The session clock is recorded even for Submitted/Discarded/Unknown receipts when known, and must equal the clock inside any Presented observation. Its Wayland v2 adapter combines split 32-bit timestamp/refresh-sequence fields with checked arithmetic and retains refresh interval, output identity when available and source flags. Zero refresh/sequence values remain valid.
- Wayland `hw_clock`, `hw_completion` and `zero_copy` bits remain compositor-reported flags; they do not upgrade the receipt's evidence level to independent hardware observation. The Wayland adapter requires exactly `CompositorFeedback`; it cannot claim `HardwareObserved` or `ExternalInstrument` without a separate evidence source. Presented requires a validated observation. Discarded is source-bound but has no presentation timestamp.
- `SessionBinding` and `FrameSequenceGuard` pin every frame to one entity ID/revision, backend build, negotiated decision, session, target format/extent, buffer generation and (when known) the connection's presentation clock ID. The first observed clock is locked for the session. Reconfiguration requires a strictly newer pool generation; frame sequence remains monotonic.
- `PresentationFeedbackLedger` registers host-owned surface commits and deduplicates identical terminal feedback for each frame sequence. Presented and Discarded are mutually exclusive; unknown, out-of-order, conflicting or late/unregistered feedback is rejected. The ledger is bounded and retains terminal records for the session lifetime to prevent duplicate callback rebinding.
- `semantic_lod` adds semantic-distortion-bounded LOD. Stable IDs cover head silhouette, left/right eyes, nose and mouth. A canonical profile specifies required/optional status, minimum projected diameter, per-feature screen-error ceiling and integer importance weight. The profile requires a head silhouette and at least two facial anchors; missing optional features incur a nonzero declared penalty.
- `negotiate_semantic_lod` filters candidates through the existing hard CPU/GPU/frame/geometry/Reduced-Motion constraints and per-feature semantic constraints. It then returns a deterministic **Pareto frontier** over 11 minimization objectives: weighted semantic distortion, vertices, indices, geometry bytes, frame time, CPU time, GPU time, CPU memory, GPU memory, global silhouette error and inverse projected feature diameter. The selected default is the lowest semantic distortion on that frontier; ties use resource preference rank then candidate ID.
- The canonical semantic decision binds the resource request, capability snapshot, semantic profile, all candidate feature observations, selected candidate and full Pareto set. `verify_semantic_lod_decision` recomputes the decision from frozen inputs. This is a measurable structural proxy, not a proof of human identity recognition.
- `semantic_lod` adds semantic-distortion-bounded candidate selection. It requires a canonical per-feature profile for head silhouette, left/right eyes, nose and mouth; required feature presence/projected diameter/screen error are hard constraints, optional missing features receive an explicit penalty, and candidates must also pass the existing CPU/GPU/resource limits. It minimizes weighted integer screen-space distortion, then uses preference rank and candidate ID as deterministic tie-breaks. The decision binds the resource request, capability snapshot, semantic profile and every candidate observation; `verify_semantic_lod_decision` recomputes it from those frozen inputs.

The crate emits canonical little-endian byte strings with versioned magic values. Hash those exact bytes using the chosen project digest at the integration boundary. This avoids coupling the conformance oracle to a cryptographic implementation; the content digest is not itself an authentication claim.

## Related research and differentiation

Semantic-aware LOD is active prior art, not a novel claim by Ouroboros. An IEEE Access paper published 28 September 2026, *Semantic-Aware Level-of-Detail Allocation for Energy-Efficient Real-Time Rendering*, uses expert-derived AHP weights, NSGA-II over a discrete LOD space, and knee-point selection. In a Unity URP virtual-museum scene, it reports a telemetry-derived mean GPU-power reduction of 11.24% over five paired runs, with SSIM 0.897 and LPIPS 0.069 across 15 viewpoints. That is a valuable baseline, but its LOD assignment is scene-object/shader class based and fixed at scene load; its authors state that SSIM/LPIPS are image-level proxies, not semantic comprehension, and dynamic reassignment is future work. See [IEEE Access paper](https://doi.org/10.1109/ACCESS.2026.3737822).

The Ouroboros extension is deliberately narrower and more stringent for a persistent face/entity:
- individual identity anchors (left/right eye, nose, head silhouette) are hard constraints, not only weights in an aggregate semantic score;
- optional features (e.g. tiny mouth detail) have explicit missing-feature penalties;
- the planner evaluates per-feature projected error and exact host resource bounds;
- the full nondominated candidate frontier is exposed, instead of hiding energy/fidelity/resource tradeoffs behind one opaque score;
- the decision is canonical and independently recomputable, and can be linked to a frame receipt and compositor-presentation evidence.

A 2025 *Computer Graphics Forum* benchmark of speech-driven 3D facial animation found objective/subjective metric rankings do not always align and may not generalize across datasets. Therefore Ouroboros must not treat its feature-error objective as a surrogate for human recognition; the blinded cross-form study remains a separate acceptance gate. See [the benchmark study](https://doi.org/10.1111/cgf.70073).

## Important limitations

This is a first oracle, not a production runtime contract. Candidate cost fields and semantic screen-space observations are host/renderer planning inputs, not independently measured GPU/CPU telemetry or a proof of human recognition. It does not yet implement actual host capability discovery, live Wayland/Vulkan/WGPU calls, temporal receipt aggregation, signatures, independent hardware capture or blinded visual-perception scoring. Candidate cost values remain estimates; observed render/presentation telemetry is a separate evidence stream. Frame receipt canonical encoding is versioned as `OUROFRM2` after adding protocol-preserving presentation observations. The exact-head Rust compile/test result is pending; see the associated PR rather than treating authored tests as passing evidence.
