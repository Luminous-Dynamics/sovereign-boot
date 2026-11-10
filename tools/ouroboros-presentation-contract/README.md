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

The crate emits canonical little-endian byte strings with versioned magic values. Hash those exact bytes using the chosen project digest at the integration boundary. This avoids coupling the conformance oracle to a cryptographic implementation; the content digest is not itself an authentication claim.

## Important limitations

This is a first oracle, not a production runtime contract. Candidate cost fields are planning estimates, not measured GPU/CPU telemetry. It does not yet implement actual host capability discovery, live Wayland/Vulkan/WGPU calls, temporal receipt aggregation, signatures, independent hardware capture or visual-perception scoring. Candidate cost values remain estimates; observed render/presentation telemetry is a separate evidence stream. Frame receipt canonical encoding is versioned as `OUROFRM2` after adding protocol-preserving presentation observations. The exact-head Rust compile/test result is pending; see the associated PR rather than treating authored tests as passing evidence.
