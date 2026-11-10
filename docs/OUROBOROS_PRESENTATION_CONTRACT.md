# Ouroboros Presentation Contract (proposal)

**Status:** research proposal, opened as [issue #29](https://github.com/Luminous-Dynamics/sovereign-boot/issues/29). This document defines an implementation direction; it does not claim that a presentation contract, GPU backend, hardware attestation or cross-device conformance suite exists yet.

## 1. Executive proposal

Move Ouroboros from a visual runtime toward a **persistent visual entity platform**.

A visual entity is not its mesh, shader, pixels, renderer process, desktop surface or device. It is a stable identity plus a versioned semantic contract and portable state. Each renderer is an interchangeable, separately qualified expression of that entity.

The crucial missing boundary is presentation quality. The current body and face planners select geometry under host-provided budgets. They do not yet negotiate a complete presentation policy or prove that a submitted frame was displayed. A renderer can successfully build a frame and still miss its deadline, submit an obsolete buffer, lose the surface, or have the compositor discard the update.

The proposed contract therefore connects five distinct layers:

1. **Identity** — which persistent entity is being expressed.
2. **State** — where that entity is in its semantic and temporal evolution.
3. **Representation** — which versioned semantic geometry and assets express it.
4. **Presentation session** — what a particular host/output/backend can deliver under negotiated constraints.
5. **Evidence** — what was requested, selected, rendered, submitted, presented and independently observed.

This is an interoperability and assurance layer, not a replacement for Wayland, Waywallen, Vulkan, WGPU, glTF or compositor integrations.

## 2. Identity must not be a content hash

A common design error is to use the digest of a scene file or mesh as the entity's identity. That makes every legitimate asset update look like a new being. The inverse error—using one fixed ID without tracking its revisions—makes it impossible to distinguish old and new representations.

Keep these values separate:

| Identifier | Meaning | Changes when |
|---|---|---|
| `entity_id` | Stable opaque identity of the persistent entity | Never during ordinary revisions |
| `entity_revision_root` | Digest of the canonical semantic descriptor and referenced immutable assets | Semantic contract or asset content changes |
| `instance_id` | One running/installed instance of an entity | A new instance is created |
| `session_id` | One host/output/backend presentation session | Session or output is recreated |
| `state_digest` | Digest of one canonical state/checkpoint | Entity state changes |
| `plan_digest` | Digest of the selected quality plan | Negotiated inputs or plan changes |
| `frame_id` | Monotonic session-local frame identity | Each frame submission |

An entity fork should receive a new `entity_id` and may carry a parent/lineage reference. A normal upgrade retains the entity ID and changes the revision root. Runtime state and output capabilities are never part of the entity revision root.

### 2.1 Canonical descriptor (proposed)

The first implementation should define a versioned Rust type, not immediately commit to a new general-purpose JSON asset format:

```text
OuroborosEntityDescriptor {
    schema_version,
    entity_id: [u8; 16],
    semantic_contract_version,
    required_feature_ids: sorted stable IDs,
    landmark_schema_versions: sorted versions,
    representation_roots: sorted content digests,
    parent_revision_root: optional digest,
    entity_revision_root: digest of canonical descriptor body,
}
```

The 128-bit ID is opaque and assigned once; it is not recomputed from visual pixels, geometry or a mutable revision. Canonical serialization must specify byte order, field order, length limits, unknown-version handling and duplicate-ID rejection. The root must not recursively include itself.

A portable checkpoint should be wrapped by an entity snapshot envelope that binds `entity_id`, `entity_revision_root`, checkpoint schema/digest and event cursor. Keep `OUROCKP1` as its own state format; do not silently insert host or renderer identity into it.

### 2.2 Identity invariants

- A renderer change must not change `entity_id`.
- A resolution, display, compositor, device or operating-system change must not change `entity_id`.
- A style/material update may change the revision root without changing `entity_id`.
- Restoring a checkpoint must not silently restore device handles, file descriptors, permissions, output identifiers or host capabilities.
- A schema-incompatible revision must be rejected or explicitly migrated; never silently reinterpret old state.
- A fork is explicit lineage, not an accidental consequence of a different digest.

## 3. Quality is a constrained vector, not one preset

Economy/Balanced/Cinematic is useful for geometry selection, but it compresses several independent dimensions into one label. A mobile renderer may have a strong GPU and weak CPU; a desktop may have abundant compute but strict power/thermal limits; an output may require Reduced Motion while retaining high image fidelity.

Do not collapse all of this into a weighted scalar named `quality`. Negotiate a vector of independently inspectable constraints:

| Dimension | Request / objective | Example observation |
|---|---|---|
| Semantic fidelity | Stable root, landmark IDs, attachment error bounds | Root/landmark drift |
| Geometry fidelity | Projected silhouette error, feature pixel floor, LOD ceiling | Actual LOD, silhouette error |
| Temporal fidelity | Cadence, deadline, frame age and stale-frame policy | p50/p95/p99/p99.9; late/discarded frames |
| Compute | CPU/GPU time or work ceilings | Host CPU time; GPU timestamps where supported |
| Memory | CPU/GPU memory and buffer ceilings | Peak RSS and reported/observed GPU allocations |
| Energy/thermal | Power/energy envelope and thermal response policy | Wall joules; domain-specific energy counters |
| Color/presentation | Color encoding, range, alpha, extent, sync requirements | Actual negotiated target properties |
| Accessibility | Reduced Motion, flash/frequency and contrast constraints | Policy mode and independent audit result |
| Availability | Pause/restore/reconfigure deadline and fallback policy | Recovery latency and valid-frame result |

### 3.1 Hard constraints and soft objectives

A request contains two different classes:

- **Hard constraints:** MUST NOT be violated. If no feasible plan exists, return `Infeasible` and use an explicit fallback.
- **Soft objectives:** preferences that may be traded, but every degradation must be named and recorded.

The preference order itself is explicit and versioned. Do not let an opaque adaptive heuristic silently trade accessibility or semantic identity for resolution or effects.

Every numeric wire field should use a defined unit and bounded integer representation (for example nanoseconds, bytes, millihertz, microunits or fixed-point normalized error). Avoid platform-dependent floating-point values in canonical decision bytes.

### 3.2 Deterministic negotiation

The pure core receives a bounded request, a host capability snapshot and a versioned policy. It does not query the GPU, display, thermal sensors, power supply or compositor.

Proposed shape:

```text
QualityRequest {
    request_schema,
    hard_constraints,
    ordered_soft_objectives,
    motion_accessibility_policy,
    maximum_resource_budget,
}

CapabilitySnapshot {
    capability_revision,
    backend_id,
    supported_target_formats,
    supported_sync_modes,
    supported_color_encodings,
    supported_geometry_features,
    resource_limits,
}

QualityDecision {
    policy_version,
    request_digest,
    capability_snapshot_digest,
    selected_geometry_plans,
    selected_temporal_policy,
    selected_color/presentation_mode,
    resource_reservations,
    degraded_objectives: [{ objective_id, reason }],
    infeasible_constraints: [],
    decision_digest,
}
```

The same canonical request, capability snapshot and policy version must produce byte-identical decisions. A stale capability revision invalidates the decision. A host must not treat an acknowledgement as proof that a render-target reconfiguration succeeded: the published buffer extent, format, pool generation and synchronization contract must be checked against the request.

No feasible plan means no partial success. The host may request a different policy, choose the CPU/static fallback, or keep the last-known-good frame, but it must record which path it took.

## 4. Delivery receipts: rendered is not presented

Add an independently versioned receipt layer. Do not mutate the `OUROEVB4` canonical bytes in place.

A delivery receipt should bind at least:

- `entity_id`, entity revision root, state/checkpoint digest and replay receipt;
- request, capability snapshot and selected-plan digests;
- renderer/backend ID, executable/build digest and relevant driver/runtime versions;
- session-local frame sequence, render-target extent/format, pool generation and buffer/slot identity;
- render-start/render-finish timestamps and clock domain;
- CPU/GPU duration and memory observations when genuinely available;
- submission result and compositor presentation feedback when available;
- discarded, superseded, stale, missed-deadline or unknown outcomes;
- evidence-source level and capture/attestation references.

Use separate lifecycle events or monotonic status transitions:

```text
Planned -> Rendered -> Submitted -> Presented
                     -> Discarded
                     -> Unknown
```

This is not a claim that the user's eyes saw the frame. `Presented` means the named presentation source reported the surface update as displayed. An independent hardware clock/completion flag is stronger evidence than renderer self-report, and should be represented as such rather than flattened into one boolean.

### 4.1 Evidence-source levels

| Level | Source | Permitted claim |
|---|---|---|
| R0 | Renderer self-report | Renderer says it rendered/submitted a frame |
| R1 | Host process observation | Host observed the process, target and resource/timing data |
| R2 | Compositor presentation feedback | Compositor reported presented or discarded, with clock/output metadata |
| R3 | Driver/hardware signal | Hardware-backed completion/timing/zero-copy property where supported |
| R4 | Independent instrument | External wall-power or capture instrument corroborated a measurement |

A missing observation remains `Unknown`. A timeout is not a success. A receipt at R0 cannot claim R2/R3. A powercap counter is not whole-device energy unless its domain is documented; wall-meter measurements should be used for whole-system energy claims.

### 4.2 Existing protocol primitives

Wayland's presentation-time protocol already distinguishes a content update that was displayed from one discarded because it was superseded or its surface was destroyed. Its feedback can report presentation-clock, refresh and output metadata, and flags such as hardware clock, hardware completion and zero-copy. Ouroboros should preserve these distinctions rather than inventing an approximation.

Reference: [Wayland presentation-time protocol](https://wayland.app/protocols/presentation-time).

For Vulkan, query supported presentation timing/synchronization extensions and report absence explicitly. `VK_KHR_present_wait` and related mechanisms do not provide identical guarantees to every display timing extension; no universal GPU timestamp or present-completion feature should be assumed.

References: [Vulkan Window System Integration](https://docs.vulkan.org/spec/latest/chapters/VK_KHR_surface/wsi.html), [VK_EXT_present_timing proposal](https://docs.vulkan.org/features/latest/features/proposals/VK_EXT_present_timing.html), [WebGPU timestamp-query and device timeline](https://www.w3.org/TR/webgpu/).

## 5. Evidence graph and trust boundary

The current `OUROEVB4` root is useful: it content-addresses replay, checkpoint, landmarks, geometry plans/meshes and the CPU reference image. It is not signed or authenticated. Preserve this distinction.

The next evidence graph should link:

```text
EntityDescriptor
   | entity_id + revision_root
   v
EntitySnapshot / Checkpoint / Replay
   | state_digest
   v
Semantic landmarks + body/face geometry + asset roots
   | representation receipts
   v
QualityRequest + CapabilitySnapshot
   | decision_digest
   v
Renderer build + target generation + frame receipt
   | presentation feedback
   v
Host/Compositor/Hardware observation
   | attestation subject
   v
Signed evidence envelope / transparency record (optional, separately qualified)
```

Use a standard authenticated attestation structure instead of a bespoke signature protocol. The in-toto Attestation Framework separates predicate, statement, envelope and bundle; the statement binds metadata to immutable subject digests and the envelope handles authentication/serialization. Evaluate a versioned in-toto/DSSE envelope for host/build and presentation attestations, while keeping the portable core dependency-light. The attesting host and its key must be identified; a renderer signing its own claim does not independently prove the host's behavior.

Reference: [in-toto Attestation Framework specification](https://github.com/in-toto/attestation/blob/main/spec/README.md).

Trust must remain layered:

- BLAKE3/content addressing: integrity and deterministic identity of bytes.
- Reproducible build/source evidence: correspondence to reviewed source/toolchain.
- Host/compositor/hardware observation: evidence about execution and presentation.
- Signature/transparency: attribution and tamper-evident publication.
- Human evaluation: perceptual identity, comfort and visual quality.

None substitutes for the others.

## 6. Asset interoperability: build on glTF, do not replace it

Use established interchange where it fits. Khronos describes glTF as a runtime-independent 3D asset delivery format with scenes, nodes, meshes, materials, textures, skins and animations. It is explicitly not a runtime behavior specification; it does not define animation playback, pause/resume or persistence semantics.

That is precisely why Ouroboros should place a semantic entity/state contract around interoperable asset references rather than inventing another mesh container or trying to make glTF own entity identity.

References: [Khronos glTF registry](https://registry.khronos.org/glTF/), [glTF 2.0 specification](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html).

The semantic landmark/feature IDs remain the stable bridge. A glTF node or mesh index is not an Ouroboros identity anchor unless a versioned mapping binds it to the semantic contract.

## 7. Benchmark programme

### 7.1 Correctness and negotiation

Start with a dependency-light Rust oracle and deterministic fixtures. It can be developed and tested locally without a GPU, compositor or CI runner.

| Test class | Required result |
|---|---|
| Stable identity | Same entity ID across renderer, resolution, device and ordinary asset revision |
| Explicit fork | New ID plus explicit parent lineage |
| Canonical negotiation | Identical inputs/policy => identical decision bytes |
| Hard constraints | Zero resource/semantic/accessibility overcommit |
| Infeasible request | Explicit failure, no partial pass-like result |
| Budget monotonicity | More permissive budget never yields a worse choice under fixed objective ordering |
| Revision fencing | Stale capability/config/pool generations rejected |
| Receipt ordering | Duplicate/reordered/missing frame IDs detected |
| Delivery truthfulness | R0 never promoted to R2/R3 without corresponding evidence |
| Recovery | Snapshot restore binds entity/revision and preserves continuation semantics |
| Negative controls | Truncation, trailing bytes, overflow, unknown schema, invalid enum and false status all rejected |

### 7.2 Perceptual and cross-renderer parity

Exact image digests are appropriate for same-build deterministic CPU replay. They are not the universal correctness criterion across Vulkan/WGPU/CPU because rasterization, color conversion and shader execution can differ.

Measure separately:

- semantic anchor and landmark position/basis error;
- silhouette intersection-over-union and symmetric Chamfer/Hausdorff distance;
- topology, manifoldness, winding, seam and occlusion defects;
- color difference after an explicit color-space/transfer conversion;
- frame-to-frame temporal continuity and presentation timing;
- blinded human cross-form identity and motion-quality results.

Keep the existing >=90% cross-form identity match as an experimental target, with per-transition confusion matrices and confidence intervals. It is not a measured result or a proxy for consciousness, personality or human-like identity.

### 7.3 Performance, power and lifecycle

Use named hosts and repeated matched trials. The current benchmark script's five-trial approach is a good baseline; preserve raw JSONL, exact source/build SHA, machine, CPU governor, kernel/toolchain, display topology, driver/compositor, power profile, thermal state and SHA-256 manifest.

Report p50/p95/p99/p99.9, median-of-trials and spread for:
- scene step/retarget;
- geometry and LOD planning;
- checkpoint/replay;
- CPU/GPU render;
- submission-to-presentation latency;
- dropped, late, superseded and stale frame fraction;
- process CPU, GPU utilization/time, CPU RSS and GPU allocation;
- energy per displayed frame and Wh/hour;
- pause, hidden, occluded, disconnected, hotplug and resume paths.

For Linux powercap, record the exact energy domain and its `energy_uj`/range semantics; do not present one package counter as whole-system power. Use an external wall meter for whole-system comparisons whenever feasible.

Reference: [Linux kernel Power Capping Framework](https://docs.kernel.org/power/powercap/powercap.html).

### 7.4 Candidate targets versus measured results

Keep these as initial targets, not established performance:

| Metric | Candidate target | Qualification |
|---|---:|---|
| 1080p desktop | 60 FPS | Named GPU/driver/compositor; repeated trials |
| 1080p p95 frame time | <=16.7 ms | Report p99 and misses too |
| 4K desktop | 30 FPS | Same scene/quality envelope |
| GPU memory at 1080p | <=256 MiB | Read actual allocation telemetry |
| Hidden/occluded work | Near zero after pause is confirmed | Measure CPU, GPU and energy |
| Resume to valid frame | <=250 ms | Separate process restart from surface resume |
| Checkpoint continuation | Zero same-build divergence | Also report cross-architecture tolerance separately |
| Hard-budget overcommit | Zero | Every test and runtime decision |
| Cross-form identity matching | >=90% | Blinded study; per-pair confidence intervals |

Do not create a numeric energy target until the first complete named-host and wall-meter baseline exists. A relative percentage without a matched baseline is marketing, not a benchmark.

## 8. Industry comparison and positioning

| System | Publicly documented strengths | Ouroboros response |
|---|---|---|
| Waywallen | Renderer process/lifecycle protocol, DMA-BUF/fence integration and multiple desktop display integrations | Integrate and qualify it; do not recreate the shell |
| we-layerd | Per-output renderer isolation, DMA-BUF with shared-memory fallback, output hotplug, frame callbacks, per-output settings and automation | Reuse these lifecycle ideas; add entity and delivery evidence |
| Fresco | Hardware video decode, multi-monitor playback, pause policies, power-saving scaling, desktop widgets and broad desktop coverage | Do not compete on media-library breadth; prove procedural state continuity and measured energy |
| OpenWallpaper | WASM programmable scenes, video/GIF, multi-monitor and covered-window pause behavior | Aim for portable state across hosts and renderers, not just portable code |
| Wallpaper Engine for KDE | Vulkan scene renderer, particles, post-processing, SceneScript/audio/text support | Match a bounded scene fixture; differentiate with semantic receipts and cross-backend conformance |

References: [Waywallen](https://github.com/waywallen/waywallen), [we-layerd](https://github.com/Aromatic05/we-layerd), [Fresco](https://github.com/DibbayajyotiRoy/Fresco), [OpenWallpaper](https://openwallpaper.org/), [Wallpaper Engine for KDE](https://github.com/CaptSilver/wallpaper-engine-kde-plugin).

The competitive strategy is therefore not a feature-count race:

1. Integrate with existing surface hosts instead of replacing them.
2. Make entity identity and state portable across those hosts.
3. Make quality negotiation deterministic and multidimensional.
4. Make delivered output measurable and evidence-bearing.
5. Publish matched benchmarks and disclose non-comparable workloads.

## 9. Execution plan that does not wait for GPU or CI runners

### Phase A — pure contract (CPU-only)

- Add entity descriptor and identity/revision/instance/session separation.
- Add quality request, capability snapshot, deterministic plan and degradation reason types.
- Add canonical bytes/digests and hostile/monotonicity fixtures.
- Add frame lifecycle and evidence-level enums; reject impossible status promotion.
- Keep new schemas versioned independently from `OUROCKP1`, `OUROLND1`, `OUROFAC1` and `OUROEVB4`.

### Phase B — evidence and conformance

- Add a verifier that checks receipt ancestry, monotonic frame IDs, resource limits and observation levels.
- Add fixture traces for output resize, stale pool generation, superseded frame, compositor discard, pause/restore, Reduced Motion and renderer crash.
- Define an in-toto/DSSE host-attestation mapping as an optional boundary adapter, not a core dependency.

### Phase C — live host adapters

- Waywallen: clear-color proof, then real DMA-BUF/fence lifecycle, then direct Vulkan.
- WGPU: same semantic contract, geometry, plan and evidence ancestry.
- Wayland: capture presentation-time feedback and preserve clock/output/flags.
- CPU reference: retain as deterministic oracle and offline fallback.

### Phase D — measured competition

- Capture the first named x86_64 and ARM64 baselines.
- Match a fixed procedural workload against the closest available scene runtimes; record when no fair equivalent exists.
- Profile top CPU/GPU/energy contributors, optimize only after bottlenecks are measured.
- Run blinded identity/motion tests with pre-registered analysis.

## 10. Decision

Start with the pure identity and negotiated-presentation contract. It gives Ouroboros a new technical centre of gravity without waiting for GPU hardware, Waywallen adapter code or queued CI.

The first proof should be small and falsifiable: **one stable entity ID, two different renderer capability snapshots, two deterministic quality decisions, and a receipt verifier that refuses to call an unpresented or stale frame successful.**

That is a stronger foundation for a persistent visual being than another rendering effect or a larger mesh.
