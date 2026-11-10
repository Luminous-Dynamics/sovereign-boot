# Ouroboros and the Luminous Visual Runtime

**Status:** portable scene state, C1-continuous bounded Hermite morph retargeting, deterministic CPU rasterization, a bounded 3D tube mesh, host-budgeted LOD, versioned semantic face landmarks, portable checkpoints and a sealed replay/checkpoint/landmark/mesh/image evidence bundle are implemented on `feat/ouroboros-identity-core-v1`. Tests are authored; exact-head CI has exposed formatting and a Nix VM canary failure, and the latest fixes are awaiting their next run. No WGPU desktop renderer or Waywallen adapter is claimed by this document.

## 1. Product thesis

The project should not be a clone of a wallpaper catalogue. It should be a **semantic living-environment runtime**: one persistent scene identity, one versioned scene contract, and several independently qualified render/presentation adapters.

The signature scene is Ouroboros: one recognizable being expressed through four related states:

1. **Nature** — roots, mycelium, leaves, rivers, growth and organic surface detail.
2. **Humanity** — facial expression, relational gestures, anatomical and sculptural cues.
3. **Digital** — information pathways, circuit-like filaments, particles and computational geometry.
4. **Enlightenment** — translucent/light-integrated forms that combine rather than erase the other three.

The entity must remain recognizably the same across every transition. The ring topology and head anchor are invariant; surface materials, secondary geometry and lighting can change. Transitions are semantic blends, not hard scene cuts.

## 2. What this first implementation does

The new `crates/visual-core/src/ouroboros.rs` module provides:

- Four stable form IDs and independent material-expression weights.
- Bounded cubic-Hermite form transitions with zero terminal velocity.
- Mid-transition retargeting carries the instantaneous derivative into the new segment, preserving C1 continuity rather than merely preserving position.
- One invariant head anchor shared by all four forms.
- Bounded body-centerline sampling with an explicit maximum sample count.
- Bounded yaw/pitch driven only by a normalized host-supplied attention target.
- Four motion policies; Reduced freezes weights, transition progress and velocity, and ignores further time advancement.
- Validation of configuration, attention targets, sample ranges and frame delta.
- No OS, display, camera, filesystem or network authority.

The additional `crates/visual-core/src/ouroboros_render.rs` module now produces deterministic opaque RGBA8 reference images: a continuous ring, expression-specific leaf/node/circuit/ray detail, and a persistent stylized face whose gaze changes from the bounded host pose. It includes fixtures for opaque output, four distinct form captures, identical-input replay, Reduced-mode invariance, dimension rejection, and gaze-driven pixel changes.

This is still a **stylized 2D CPU reference**, not finished art. It does not yet produce a volumetric mesh, sculpted facial geometry, physically based materials, advanced lighting, a WGPU renderer, a wallpaper surface, or a screensaver. Its tests are authored, not yet executed on the exact PR head, and no screenshots have received human visual review.

A third core module, `crates/visual-core/src/ouroboros_trace.rs`, defines a versioned, bounded, canonical little-endian event trace. It captures initial configuration/form, host time steps, target-form changes, normalized attention targets and motion-policy changes. Replaying the trace produces:
- A BLAKE3 digest of the canonical trace bytes.
- A BLAKE3 digest of the **complete internal scene state**, including pending attention and transition/frozen state—not merely the visible frame.
- The final renderer-facing scene, usable by the CPU reference renderer.


A fifth core API adds a bounded portable scene checkpoint (`OUROCKP1`) independently of the event-trace and internal-state digest schemas. The canonical checkpoint captures configuration, current/target form state, independent Hermite channel durations, elapsed time, transition progress, all four C1 start velocities, normalized attention, head pose, motion policy and optional Reduced-mode frozen weights/progress/velocity. It returns a BLAKE3 receipt bound to the checkpoint schema and scene-contract version. Restore rejects malformed or unsupported inputs before returning a scene. It does not serialize renderer/device handles, display identities, process IDs, permissions or other host capabilities.

Checkpoint source and tests are authored, not yet exact-head qualified. The current target is <=256 bytes, p95 encode/decode <=100 microseconds over 1,000 in-process round trips, and zero same-build state/frame divergence over 1,000 fixed steps after restore.


A sixth core module, `crates/visual-core/src/ouroboros_evidence.rs`, seals a reproducible CPU evidence run. Given a validated event trace, a host-budgeted mesh plan and image dimensions, it:
- Replays the canonical trace and retains the exact trace bytes and replay receipt.
- Encodes the final semantic scene checkpoint, including C1 velocity and motion-policy state.
- Generates and validates both the planned body mesh and landmark-driven face mesh, binding separate canonical geometry receipts.
- Renders the deterministic CPU reference image and retains its RGBA8 bytes and pixel receipt.
- Encodes the sub-receipts, scene-contract versions, requested/selected LOD and exact resource estimate into a canonical `OUROEVB3` metadata bundle, including the semantic face-landmark receipt and the complete host-provided budget inputs; BLAKE3 over that bundle is the evidence root. The sealer recomputes the deterministic LOD plan from the replayed scene and those exact budget inputs before accepting the evidence.

The returned artifact includes trace bytes, checkpoint bytes, semantic landmarks, face geometry, body geometry, pixels, bundle bytes and receipts. It is a content-addressed evidence root, **not a digital signature or authenticated provenance**. Build/commit identity, GPU driver, host measurements, compositor presentation and physical hardware evidence must be attached by a separately qualified host/attestation layer. Exact-head evidence-sealer tests are authored but not yet qualified.

The CPU renderer also exposes `render_reference_artifact`, which binds RGBA bytes to width, height, scene-contract version and a BLAKE3 pixel digest. These receipts make a run inspectable and replayable; they do not establish cross-architecture floating-point parity, GPU parity, visual quality or authenticity without a separately verified source/build and capture process.

The fourth core module, `crates/visual-core/src/ouroboros_mesh.rs`, converts the semantic centerline into a bounded closed tube mesh:
- Stable ring/tube tessellation with explicit resource ceilings (ring 32–2048, tube 6–64 segments).
- A deterministic low-twist frame projected from the scene's bounded centerline tangent.
- Unit normals, a closed periodic centerline, duplicated UV seam vertices with byte-identical positions/normals, and stable indices across every semantic form.
- An explicit outward triangle winding contract validated for every triangle, plus exact canonical index topology and hostile-public-data bounds checks.
- Canonical little-endian mesh serialization and a BLAKE3 receipt binding the scene-contract version, tessellation, vertices, normals, UVs, form weights and indices.

The body mesh is a geometry artifact, not a final character model or shader. It has no GPU upload path, shadowing or physically based lighting. Its test coverage is authored but not yet qualified on this exact PR head.


A seventh core contract adds `OuroborosScene::landmarks()`: one versioned semantic head/face attachment set in normalized scene coordinates. It carries the exact invariant head root, an orthonormal forward/lateral/up basis derived from bounded yaw/pitch, and stable left-eye, right-eye, nose-tip, mouth-center, crown and chin points. Facial proportions scale with the scene major radius; semantic form weights never move or reorder the root or attachment IDs. `OuroborosLandmarks::validate` checks schema and scene-contract versions, finite coordinates, exact head-root identity, an orthonormal right-handed basis bound to the scene pose, a bounded facial attachment envelope and bilateral eye symmetry. `canonical_bytes` emits the versioned `OUROLND1` representation and its BLAKE3 receipt is independently versioned from mesh, checkpoint and trace schemas. Adversarial tests mutate schema, root, basis, pose, eye symmetry and attachment bounds; exact-head qualification is pending.

The landmark set is a renderer-neutral attachment contract, not a sculpted model or full skeletal rig. The deterministic CPU reference renderer now projects its eyes, pupils, nose, mouth and cheek accents directly from this contract; its duplicate yaw/pitch face transform has been removed. A future WGPU/Vulkan renderer must consume the same `OUROLND1` attachment map and include its receipt in cross-backend evidence. This does not claim photorealism, facial expression quality or human identity; those remain visual-art and perceptual-evaluation work.

### Landmark-driven 3D head and face geometry

The new `crates/visual-core/src/ouroboros_face.rs` module adds a separate parametric face artifact, `OuroborosFaceMesh`, generated from `OuroborosLandmarks` rather than renderer-private offsets. Its first profile contains five closed indexed ellipsoid regions with stable IDs: head shell, left eye, right eye, nose and mouth. Eye volumes are centred on the exact eye landmarks; the nose volume is constructed so its forward tip meets the nose-tip landmark; the mouth region is centred on the mouth attachment. All five regions use the same landmark-derived forward/lateral/up basis, so bounded pose changes rotate the face while leaving the root fixed.

The face mesh has its own bounded latitude/longitude tessellation, explicit vertex/index/byte estimates, checked reservations, finite/unit-normal/UV validation, canonical index topology, outward winding checks and exact UV seam checks. Its current hard limits are 50,000 vertices, 300,000 indices and 2.5 MB of canonical vertex/index storage. Validation regenerates canonical geometry from the supplied landmarks and config and compares every vertex/index, so mutations of public geometry vectors cannot retain validity by changing only counts or indices. The canonical `OUROFAC1` receipt binds the schema, scene contract, landmark digest, tessellation and all vertex/index bytes.

This is a first **five-volume parametric face mesh**, not a fused watertight sculpt, production anatomy, expression/blendshape rig, skin/hair material system or photorealism claim. Eye and mouth volumes are separate closed surfaces that may intersect the head shell; the next art/geometry pass must review proportions, surface attachments and visual occlusion. Exact-head geometry tests and visual review remain pending. Host-requested face LOD and requested-vs-selected face budget reporting are the next separate task in [issue #27](https://github.com/Luminous-Dynamics/sovereign-boot/issues/27).


The mesh API also exposes deterministic host-budget planning. The host supplies projected ring/tube pixel radii plus maximum vertex, index and byte budgets. Requested ring tessellation rounds up to the next power-of-two needed to target <=0.75 px centerline spacing (capped at 2048); tube cross-section demand uses its own quantized ladder. The core then selects a deterministic feasible tessellation tier (Economy, Balanced or Cinematic) using the two ladders and exact resource estimates. The returned plan includes both the requested and selected ring/tube segments, so a host can see where the shared budget traded cross-section detail for silhouette resolution. If even the minimum mesh cannot fit, it returns `BudgetTooSmall`; the host must choose a CPU/static fallback. This keeps display/GPU discovery out of the scene core while allowing predictable resolution/power scaling. The same budget input produces the same plan, and the plan reports exact counts and estimated geometry bytes.

**Motion-quality boundary:** the source now uses bounded cubic-Hermite segments independently for each of the four semantic form channels. A retarget carries each channel's current derivative into its own segment and uses a zero-velocity terminal condition. Position and velocity evaluation use f64 intermediates to reduce boundary cancellation; only when a channel is exactly at 0 or 1 and its derivative points outward is that derivative projected into the interval's tangent cone, because preserving it would mathematically violate the bound. Each channel's duration is shortened independently until its analytic cubic extrema remain inside the validated weight range, so a near-boundary residual cannot force the whole entity to snap. Reported transition progress tracks the slowest active channel. No per-frame allocation is introduced. Reduced mode freezes and restores the complete kinematic state. Tests are authored for all 64 initial/target/retarget triples, finite-difference derivative agreement, repeated deterministic adversarial chains, freeze/resume and zero terminal velocity; exact-head test evidence and visual review are pending. Acceleration continuity (C2) is not claimed.

## 3. Architecture

Keep scene semantics portable and host capabilities separate.

| Layer | Ownership | Current boundary |
|---|---|---|
| Scene identity and state | Rust visual core | Ouroboros v0.1 state model now added |
| Scene pack / visual contract | Versioned data + Rust validation | Existing Scene Pack work is a separate integration track |
| Reference renderer | Rust CPU RGBA | Mycelial scene plus stylized Ouroboros CPU reference rasterizer; content-addressed capture receipts authored, golden artifact review pending |
| Geometry | Rust CPU indexed mesh | Closed Ouroboros tube mesh, invariant topology, deterministic UV seams, normals, hostile-input validation and mesh receipt authored; exact-head geometry tests pending |
| Quality planning | Host-provided projected size and resource ceilings | Deterministic Economy/Balanced/Cinematic mesh planner with exact vertex/index/byte estimates; exact-head budget tests pending |
| Replay/evidence | Canonical Rust event trace + BLAKE3 | Versioned input trace, full internal state digest and `OUROEVB2` evidence root binding checkpoint, face-landmark, mesh, image and complete LOD budget receipts; exact-head tests pending |
| Persistence | Canonical Rust checkpoint + BLAKE3 | Bounded `OUROCKP1` checkpoint API, active/Reduced restore tests and evidence-bundle integration authored; exact-head qualification pending |
| Semantic identity rig | Rust scene landmark contract | Versioned `OUROLND1` invariant face root/basis/attachment points and evidence receipt authored; exact-head landmark tests and visual review pending |
| Desktop renderer | Rust + wgpu | Headless reference path exists on the separate Visual Genome branch; not a desktop presentation adapter |
| Boot adapter | Rust DRM/KMS + NixOS service | Existing renderer, VM/hardware qualification remains separate |
| Desktop host adapter | Waywallen renderer subprocess + pinned bridge; Waywallen Display owns Plasma/Wayland presentation | Not implemented |
| Screen-saver adapter | User-session lifecycle | Not implemented |
| Lock-screen background | Existing compositor/lock UI only | Not implemented; separate security qualification required |

The scene core never decides whether a display is visible, whether a lock is active, or whether it may acquire a device. The host owns the frame clock, visibility/suspend policy, permissions, display lifecycle, and fallback selection.

### Desktop integration decision

**First target: a Waywallen external renderer plugin, not a new Plasma wallpaper plugin.**

The reviewed Waywallen source already has the presentation stack we would otherwise need to own:
- Rust daemon with an external renderer subprocess registry.
- Plugin manifests that declare renderer executable, wallpaper types and bounded typed settings.
- A versioned renderer IPC contract (protocol v3 / spawn v10 at the reviewed upstream revision).
- `waywallen-bridge`, a MIT-licensed C ABI which owns the DMA-BUF buffer pool, modifier negotiation and release-timeline bookkeeping while exposing a Vulkan `VkImage` to the renderer.
- Per-frame acquire `SYNC_FD` transfer; buffer release is tracked with the daemon-consumer release timeline.
- Separate display clients for Plasma 6 / GNOME and layer-shell environments.

The `waywallen-display` library is a **consumer/presentation client**. The Luminous producer should implement Waywallen's renderer subprocess protocol and use `waywallen-bridge`; it should not pretend that the display library is a generic frame-producer API.


A separate, dependency-free Rust test oracle now lives in `tools/waywallen-protocol-contract`. It models the protocol-v3 / spawn-v10 state transitions and fail-closed rules for Init/Ready, release-syncobj, FormatCaps flattening, supported format/extent, buffer-pool generations, slot/plane descriptor counts and per-frame acquire-fence/sequence/release-point invariants. CI runs this oracle independently of the production workspace. It does **not** implement Unix sockets, SCM_RIGHTS, Vulkan, DMA-BUF allocation or kernel fence signalling; the actual adapter must map real events into these checked transitions and separately qualify descriptor/fence ownership.


The first integration should be a separate Rust executable, `luminous-ouroboros-renderer`, plus a small Waywallen plugin manifest. Keep the semantic simulation, deterministic CPU reference, scene validation, and eventual Vulkan rendering in Luminous code. Use Waywallen for per-display surfaces, compositor lifecycle, buffer import and presentation.

Important security boundary: Waywallen's README explicitly warns that plugin Lua entry points execute inside the daemon with the user's privileges. First test a renderer-only manifest with no Lua entry. If a catalogue/source script is unavoidable, keep it first-party, minimal, reviewed, offline and read-only; do not put scene logic or asset downloading in Lua. The renderer itself should be a separate process launched inside a systemd user scope with no IP network, no camera/input device permissions, read-only scene packs and only an allowlisted DRM render node plus the Waywallen Unix IPC socket.

Implementation order:
1. Pin an exact Waywallen source/bridge revision and confirm the renderer-only manifest path.
2. Wrap the C bridge ABI in a small safe Rust module, preserving protocol/spawn-version checks and FD ownership.
3. Make a smoke renderer negotiate buffers, publish a deterministic test frame, observe release points, and shut down cleanly.
4. Render the CPU Ouroboros reference into an initial Vulkan transfer target to prove the complete DMA-BUF + `SYNC_FD` pipeline.
5. Replace the staging upload with direct Ash/Vulkan rendering into the bridge-owned `VkImage`; keep the CPU renderer as a static fallback and test oracle.
6. Test visibility/pause, display hotplug, resolution changes, process kill/restart, sleep/resume and multi-monitor behavior in the disposable KDE VM.
7. Only consider a direct Plasma WallpaperItem plugin if the Waywallen protocol/packaging path proves inadequate. Keep lock-screen and early-boot adapters completely separate.

The Waywallen renderer protocol is versioned and can change. Record the exact daemon revision, bridge ABI version and spawn version in every evidence bundle; do not treat the current v10 protocol as a timeless contract.

## 4. Industry comparison and differentiation

The Linux desktop-wallpaper field is no longer just a set of video-loop tools. Current competitors offer live scene runtimes, output workers, battery-aware policies, audio/widget surfaces, wallpaper catalogs and even lock-screen presentation. The following review is dated **2026-11-09**; it distinguishes official release information from current upstream source and from claims we have not independently benchmarked.

| Product | Current strengths that Ouroboros must respect | Our response |
|---|---|---|
| **Wallpaper Engine (official + community Linux renderers)** | Official app remains Windows/Android. A maintained Plasma 6 fork runs Scene/Web/Video through a custom Vulkan renderer and documents particles, post-processing, SceneScript, audio reactivity and text layers; other Wayland projects run Workshop scenes too. These are third-party ports, not official Linux support. | Do not claim to be the first Linux Vulkan scene renderer, first Workshop-compatible renderer or exclusive 3D wallpaper path. Win on original portable semantic entities, C1 continuity, durable checkpoints, cross-host identity and evidence-rooted qualification. |
| **Waywallen** | Mature Linux display integration; a daemon-managed external renderer process; declarative plugin manifests; v3 IPC with DMA-BUF/modifier and synchronization negotiation; hardware video decode. Its released v0.5.0 added smart playlists, automatic display sizing, dedicated lock-screen display role and battery-aware playback rules; current upstream source is 0.6.0 and must be pinned separately. | Use Waywallen as the first host rather than rebuilding desktop-shell surfaces. Implement only the producer side; keep the Ouroboros scene and renderer outside the daemon. |
| **Fresco** | v1.1.54 (released 2026-11-01) supports video/GIF/image/slideshows, per-display wallpapers, day/night schedules, battery/fullscreen pause, widgets and lock-screen presentations on selected desktops. Its README reports Intel N150/Deepin 25/VA-API package-power measurements (two runs per mode): 1080p60 Full 1.37 W GPU / 6.00 W package vs Reduced 0.63 W / 4.03 W; 4K60 Full 2.77 W / 7.94 W vs Reduced 1.60 W / 5.95 W. The README explicitly says these are total package readings without a matched idle baseline, not marginal wallpaper overhead. | Treat battery policy, covered-window pause, output hotplug and lock-screen integration as table stakes. The published Fresco numbers become an external reference, not an Ouroboros comparison until we repeat the same workload/machine with a matched static/idle baseline and capture methodology. Our distinction must be semantic identity/replay, bounded state, explicit authority boundaries and evidence-backed quality—not the existence of Reduced mode alone. |
| **we-layerd** | Native Wallpaper Engine scene/video/web runtime for Wayland; independent output workers; hotplug reconciliation by stable output identity; DMA-BUF when formats/modifiers permit and shared-memory fallback otherwise; bounded in-flight buffers and frame-callback pacing; per-output focus/fullscreen/power automation. | Match lifecycle, per-output failure isolation, resource bounds and actual frame pacing. Reuse the host bridge instead of inventing a second compositor transport. |
| **OpenWallpaper** | Programmable WASM scenes with AOT/native execution, Wallpaper Engine scene conversion, video/GIF playback, audio-spectrum inputs and per-monitor targeting. Its automatic-pause coverage is compositor-dependent (the project documents Hyprland as the fully supported case in the reviewed README). | Match scene portability and multi-monitor behavior, then differentiate through replayable semantic scene state, checkpoint portability and bounded, reviewable host permissions. |
| **Luminous Visual Runtime** | Portable Rust scene state, CPU reference rasterizer, C1 per-channel morphs, bounded closed mesh, replay receipts, versioned scene checkpoints and deterministic host-budget LOD are now source implementations; tests/adapter remain under exact-head qualification. | Aim to become the evidence-qualified semantic-entity layer that can run inside several hosts, not another monolithic wallpaper manager. |

### Where we can actually outperform

There is no valid current claim that Ouroboros is faster, lower-power or visually better than these products: we have not run an apples-to-apples test on a shared machine, compositor, GPU driver, scene/media workload or display topology. Current competitors already document Vulkan scene rendering, per-output workers, DMA-BUF/shared-memory fallbacks, multi-display automation and explicit performance modes. The near-term win is to outperform them on *persistent semantic identity, continuity and independently reproducible evidence*, then measure whether that architecture also improves resource and lifecycle outcomes.

1. **Identity as a first-class invariant.** Competitors primarily manage media playback or wallpaper scenes. Ouroboros is one semantic entity whose identity must persist across forms, render backends, process restarts and display handoffs. We measure that with landmark/topology continuity, blinded cross-form recognition and checkpoint restore equivalence.
2. **Evidence-carrying rendering.** Every replay, checkpoint, mesh and image is tied to canonical inputs and a digest. Extend this to the adapter's exact upstream commit, protocol/spawn version, bridge ABI/header hashes, negotiated format/modifier, pool generation and raw presentation evidence. A screenshot or FPS counter alone is not a qualification.
3. **Budget negotiation rather than hidden quality degradation.** The host provides projected size and hard ceilings; the plan reports requested versus selected tessellation and exact estimates. Extend the same contract to frame rate, GPU memory, CPU time, thermal/power state and hidden-state behavior.
4. **Authority separation.** The semantic scene has no camera, network, filesystem or display authority. The renderer is a process boundary. Lock-screen presentation, device access and boot remain distinct capabilities with independent security gates.
5. **Deterministic degradation.** When a GPU allocation, compositor surface or host budget fails, the entity should descend to a bounded CPU/static representation without losing identity or corrupting its checkpoint. Competitors' media fallbacks are useful baselines; our claim must be tested with deliberate fault injection.

The current upstream anchors used for this comparison:
- Fresco README power table: Intel N150, Deepin 25, VA-API, two runs per mode; values above are project-reported total package power. The README warns that no idle baseline was recorded, so marginal wallpaper power is unknown.
- we-layerd documents per-output isolated workers, stable output hotplug, DMA-BUF zero-copy where possible, shared-memory fallback, frame callbacks and bounded in-flight buffers.
- Waywallen source pin: `e2ed0b0adc6be6477c7bcc5015c76f4a1bb2b437` (0.6.0 source; released v0.5.0 is not the same revision).
- Fresco release page: v1.1.54, published 2026-11-01.
- we-layerd current README/source: native Wayland runtime and output-worker architecture.
- OpenWallpaper project documentation: WASM/AOT scene runtime and compositor-dependent auto-pause.

### What we should not claim

- A Rust scene core is not a full wallpaper engine.
- A headless WGPU capture is not compositor presentation or physical-GPU performance evidence.
- A Plasma plugin is not a secure lock-screen adapter.
- A passing motion heuristic is not WCAG certification.
- A VM test is not physical-display, GPU-driver, suspend/resume or power evidence.

## 5. Benchmarks and acceptance targets

Targets below are proposed qualification targets, not results. Every measurement must be tied to repository/commit, build profile, NixOS/kernel, compositor, GPU/driver, resolution, refresh rate, power state, run duration and raw artifact hashes.

### Desktop performance

| Metric | Initial target | Qualification method |
|---|---:|---|
| 1080p desktop | 60 FPS; p95 frame time <=16.7 ms | Presented-frame timestamps on the reference RTX 2070 Laptop, not offscreen submit time |
| 4K desktop | 30 FPS; p95 <=33.3 ms | Same physical GPU/driver tuple at 3840x2160 |
| Wallpaper GPU memory | <=256 MiB at 1080p | Process and driver accounting; state exclusions explicitly |
| Steady CPU | <=2% of one logical core in low-motion mode | Five-minute visible workload after warm-up |
| Low-power increment | <=2 W average, only when valid telemetry exists | Matched 30-minute A/B run; otherwise report unmeasured |
| First scene after selection | <=250 ms to a valid frame | Timestamped repeated theme changes |
| Hidden/covered pause | Stop simulation and rendering within 1 second | Compositor visibility injection and process/GPU counters |
| Resume | First valid frame <=250 ms after visibility returns | Repeated cover/uncover and monitor reconnect tests |
| Stability | 60-minute visible and 60-minute hidden runs without unbounded memory growth | RSS/GPU-memory traces and scene-state receipts |

### Power comparison baseline

Fresco's published Intel N150 figures are useful as a *reference point*, not a head-to-head result. To compare responsibly, run the same 1080p60 and 4K60 clips on the same machine with identical codec, VA-API/NVDEC/driver path, refresh, brightness and output topology. Record (a) static-desktop idle, (b) video wallpaper Full, (c) video wallpaper Reduced, and (d) Ouroboros static/low-motion/balanced. Report GPU/video-engine power and total package power separately, run at least five repeated 30-minute windows after thermal stabilization, and preserve raw `turbostat`/driver telemetry. If the same telemetry is not available, mark the comparison `not comparable`; do not subtract Fresco's package figures from an unmeasured Ouroboros result.

### CPU qualification harness

The release-mode benchmark harness is `crates/visual-core/examples/ouroboros-benchmark.rs`. Run it with:

```sh
BENCH_SOURCE_REV="$(git rev-parse HEAD)" \
BENCH_CPU_MODEL="exact CPU model" \
BENCH_CPU_GOVERNOR="performance / powersave / schedutil" \
BENCH_POWER_PROFILE="AC balanced / AC performance / battery" \
BENCH_KERNEL="$(uname -r)" \
BENCH_RAM_MIB="installed RAM in MiB" \
BENCH_RUST_TOOLCHAIN="$(rustc +1.96.0 -V)" \
BENCH_GPU_MODEL="exact GPU model or not-applicable" \
BENCH_GPU_DRIVER="exact driver version or not-applicable" \
BENCH_COMPOSITOR="exact compositor/version or not-applicable" \
cargo +1.96.0 run --release --locked -p sovereign-visual-core \
  --example ouroboros-benchmark -- --iterations 250 --warmup 20 \
  > ouroboros-benchmark.jsonl
```

It emits newline-delimited JSON using schema `luminous-ouroboros-cpu-benchmark-v2`: one metadata row followed by one row per operation. Current measurements include loop-control overhead, **ordinary 60 Hz C1 stepping**, a separate **1 Hz retarget-under-60 Hz stepping** workload, semantic landmark generation, checkpoint encode/decode, trace decode/replay, LOD planning, body-mesh build+validation, face-mesh build+validation, 320×320 CPU capture and 128×128 sealed evidence capture. This separation prevents retarget-command cost from being mislabeled as ordinary per-frame cost. Each measurement reports min/mean/p50/p95/p99/max nanoseconds and operations per second. The metadata row records schema, source revision, debug/release profile, OS, architecture, CPU model, CPU governor, power profile, kernel, RAM, Rust toolchain, optional GPU/driver/compositor identifiers, logical CPU count, scene-contract version, trace length, body-mesh plan and face-mesh counts.

For named-host baselines, use `bash scripts/benchmark-ouroboros-linux.sh`. It builds the release harness once, then launches five independent trials by default (250 warm-up / 1,000 measured operations each). The driver rejects a dirty source tree unless explicitly overridden, validates every trial against the exact source SHA and all eleven workload names, computes medians of each trial's p50/p95/p99/mean (not pooled samples), reports run-to-run p95 spread, and writes raw JSONL, a host manifest and SHA-256 sums under the ignored `target/benchmarks/ouroboros/` tree. `tests/benchmark-aggregate-smoke.sh` exercises aggregation independently with two synthetic trials. The scripts are source-implemented; until their exact-head CI smoke and a named-host run pass, no performance result is claimed.

The harness is CPU-only. It does **not** measure Vulkan/WGPU submission, GPU memory, compositor-presented frame pacing, power or lock-screen behavior. Those measurements require the later native adapter and a separately identified device/compositor. A benchmark JSONL file is not a pass/fail qualification by itself; preserve raw output with the exact source/build/host tuple and compare confidence intervals over repeated runs. No performance numbers are claimed until the harness has actually run on a named host.

### Comparative qualification protocol

Do not compare products from marketing claims or different machines. Freeze one matrix and capture the raw evidence for every runtime. Use the following dated software baselines:

| Runtime | Baseline revision/date | Workload class to compare |
|---|---|---|
| Waywallen | v0.5.0 release (2026-10-20); separate source pin 0.6.0 commit in the adapter README | Same video file, multi-monitor lifecycle, renderer pause/restart, and the Ouroboros producer when integrated |
| Fresco | v1.1.54 release (2026-11-01) | Same video/image playback, per-monitor pause, performance mode, battery pause and lock-screen behavior on supported desktops |
| we-layerd | Current main source reviewed 2026-11-09 | Same Wallpaper Engine scene/video/web workloads where assets are licensed and available; output worker isolation, DMA-BUF/shared-memory behavior and frame pacing |
| OpenWallpaper | Current official project docs reviewed 2026-11-09 | WASM scene, AOT runtime, multi-output targeting and compositor-specific pause |
| Ouroboros | Exact PR/release commit plus a pinned adapter/runtime host | Same media workload for playback parity plus a dedicated semantic-entity workload for identity, transition, checkpoint and replay |

**Test controls:** same laptop/GPU/driver/kernel/compositor where the runtime supports it; same output resolution/refresh; AC and battery as separate conditions; same 1080p60 H.264 clip for media parity; identical covered/hidden durations; same display hotplug sequence. Repeat each run at least five times after a documented warm-up, randomize product order and preserve logs and raw timestamps. Record exact app versions and settings. A runtime that does not support the same compositor or workload is marked *not comparable*, never assigned a zero or inferred result.

**Measure separately:** CPU time per process; GPU engine utilization and memory; frames produced, submitted and actually presented; dropped/late frames; time-to-first-valid-presented-frame; hidden-pause latency; resume latency; monitor hotplug recovery; renderer crash recovery; total system power from a wall meter where possible. Package/CPU telemetry is useful diagnostic evidence but is not interchangeable with wall power. Fresco publishes an Intel N150 package-power table and explicitly notes that it lacks a matched idle baseline, so it cannot be treated as a direct marginal wallpaper cost or compared numerically to the RTX 2070 laptop without repeating the measurement.

**The first target is parity, not a fabricated SOTA win:** no worse than the best supported baseline on presented-frame stability, hidden pause, memory growth and recovery for the common video workload; then demonstrate an attributable advantage in semantic identity, C1 continuity, reproducible restore and bounded host-budget degradation. Report median, p95/p99, confidence intervals, hardware/software tuple and every unsupported scenario.

### Multi-display and lifecycle qualification

| Scenario | Acceptance target | Required evidence |
|---|---|---|
| 1 display → 2/3 displays | Every display reaches its selected mode; scene simulation is shared when semantics/settings are identical | Per-output present traces, pool generations, total memory and CPU/GPU use |
| Output hotplug | No stale output identity, stale frame or leaked pool; reconnect <=1 second after host reports the output | Stable output IDs, buffer-generation receipts and process/resource snapshots |
| Mixed GPU / hybrid graphics | Unknown or cross-GPU topology takes a conservative path; no assumed zero-copy | DRM node identities, negotiated path, modifier, plane counts and fence traces |
| Covered/fullscreen/hidden | Stop simulation/render work within 1 second, while preserving checkpoint state | Host visibility events, frame counters and CPU/GPU counters |
| Sleep/resume | First valid presented frame <=250 ms after the compositor reports a usable target; no stale DMA-BUF or FD reuse | Before/after generation IDs, present timestamps and descriptor lifecycle logs |
| Renderer crash/restart | Static/CPU fallback remains opaque; restart recovers from a validated checkpoint or known initial state | Fault injection, checkpoint digest, process exit/restart trace and first-present receipt |
| Display resize | Re-negotiate target and pool; never write beyond the negotiated extent | Render-target revision, pool generation and memory bounds |

### Ouroboros identity and morph quality

| Metric | Acceptance target |
|---|---|
| Head anchor | Exactly invariant in the scene contract across all four forms |
| Retarget continuity (C0) | Zero instantaneous jump in all four form weights |
| Retarget velocity continuity (C1) | Analytic derivative jump <=1e-6 normalized weight units/s in exact-head tests; finite-difference target <=1e-4 on captured 30/60 Hz trajectories |
| Retarget boundedness | Zero excursions outside [0, 1] over all channels and repeated retarget chains |
| Channel independence | A short duration for one near-boundary channel must not shorten other active channels; measure per-channel completion times |
| Body topology | Same ordered centerline parameterization and sample IDs across all forms |
| Pose bounds | Yaw and pitch always remain inside validated configuration limits |
| Reduced mode | No frame/state change from elapsed time or attention-target updates over a 60-second replay |
| Deterministic replay | Same build, seed/configuration, ordered input events and fixed-step sequence produce the same scene-state and CPU reference output |
| Transition quality | No geometry inversion, NaN, C0/C1 discontinuity or unintended topology change; C2/acceleration jumps reported separately; human-reviewed motion at 30/60 Hz |
| Perceptual identity | Blinded reviewers correctly match the same Ouroboros across all four forms at a proposed >=90% rate; report sample size and confidence interval |

The identity metric is intentionally not just a pixel similarity score: materials are supposed to change substantially. We should measure persistent landmarks, silhouette/topology continuity and human recognition separately.

### Reproducible visual evidence

- One evidence bundle must bind the event trace, final internal-state digest, portable checkpoint, semantic face-landmark receipt, mesh receipt, pixel receipt, full host budget and selected/requested LOD.
- All embedded scene-contract and artifact schema versions must be explicit.
- Re-sealing identical trace, plan, image dimensions and same-build inputs must produce identical sub-artifacts and bundle root.
- A changed trace, pixel, mesh, checkpoint or plan must change the corresponding sub-receipt and evidence root.
- The `OUROEVB2` evidence bundle is content-addressed, not signed. Authenticity requires an independently verified build/source attestation and a trusted capture path.
- Landmark schema version and canonical digest must be embedded in every backend comparison; adapters must not silently replace the semantic eye/nose/mouth/root attachment map.
- CPU reference, GPU output and compositor-presented captures remain distinct backends; never treat equal dimensions or a shared scene contract as proof of pixel parity.

### Persistence and handoff

- Active C1 trajectories must resume with identical per-channel values, velocities and subsequent state on the same build/target.
- Reduced checkpoints must remain frozen until the host explicitly resumes the entity.
- Current canonical schema sizes are 121 bytes for an active checkpoint and 157 bytes for a Reduced checkpoint; the hard format ceiling remains 256 bytes.
- p95 encode/decode target <=100 microseconds for 1,000 in-process operations on the reference laptop CPU; actual latency remains unmeasured.
- Invalid, oversized, unknown-version, out-of-range and trailing-byte snapshots are rejected before use.
- Checkpoints contain semantic state only. Renderer handles, file paths, DMA-BUFs, FDs, display identity and permissions are never persisted.

### Host-budgeted deterministic LOD

- The host—not the scene core—supplies projected pixel size and vertex/index/byte ceilings.
- Every plan must disclose requested versus selected ring/tube segments, exact geometry counts and estimated bytes.
- Plans must never exceed a declared ceiling; budgets below the minimum viable mesh must fail closed and select a CPU/static host fallback.
- Identical inputs must produce identical plan fields; increasing projected ring size with other inputs fixed must not reduce selected ring segments.
- Extend geometry budgets into a scene-wide budget: simulation CPU time, mesh bytes, material/shader count, texture bytes, render-target pixels and maximum frame rate. The first release should not add adaptive behavior until the host can explain every downgrade.

### Accessibility, safety and privacy

- Reduced mode must be static immediately; no black intro, background simulation or pointer-follow movement.
- Low Power, Balanced and Cinematic must have explicit, bounded budgets.
- Avoid rapid full-frame luminance changes; retain time-stamped capture audits. These are regression screens, not certification.
- Default attention source is a host-provided logical target; no camera or face detection permission.
- Zero renderer IP-network requests and zero remote asset dependencies.
- Lock-screen scenes must never introduce an input surface, click-through path, identity/presence data, notifications or a replacement authentication flow.
- GPU/backend failure must return to a static opaque fallback; it must never expose the desktop during lock transitions.
- Rendering must not delay login, lock, unlock, shutdown, recovery or boot.

### Platform qualification

1. Native renderer unit/replay tests.
2. Headless CPU/WGPU reference captures.
3. Disposable KDE Plasma 6 VM: plugin lifecycle, display resize, monitor hotplug, sleep/resume, renderer kill/restart and fallback.
4. Physical RTX 2070 laptop: presented-frame latency, power, GPU memory, display reconnect and long-duration thermals.
5. Separate lock-screen and physical boot qualification; never infer either from wallpaper success.

## 6. Paradigm shifts worth pursuing

### A. From wallpaper to persistent visual entity

The scene is not a video loop. It has a stable identity, semantic state, a deterministic simulation clock, and explicit events. The same entity can be rendered with different fidelity on the boot path, desktop, browser and future devices.

### B. From device-owned effects to portable scene contracts

Define versioned semantic state, render-plan capabilities, resource budgets and adapter permissions. A host negotiates what it can safely present; unsupported fields fail closed instead of being silently ignored.

### C. From always-running animation to visibility-aware computation

The host controls visibility and idle state. When hidden, suspended, locked or on battery, the scene reduces or stops work. A static representation is a first-class output, not an error screen.

### D. From opaque plugins to evidence-carrying scenes

Each scene pack should bind version, seed, configuration, declared assets, hashes, budgets and required host capabilities. The runtime can report which profile it actually negotiated and preserve reproducible captures.

### E. From aesthetic themes to semantic environments

Ouroboros is a first exemplar. Future scenes could embody civic commons, ecological recovery, scientific exploration, collaborative work and musical expression while retaining shared accessibility and resource contracts.

## 7. Execution roadmap

| Priority | Deliverable | Exit condition |
|---|---|---|
| P0 | Qualify this Ouroboros core slice | Exact-head formatting, compile, unit tests, frozen/resume/retarget tests and deterministic replay/capture receipts |
| P0 | Qualify C1 morph retargeting | **Source implemented** with bounded cubic-Hermite segments and analytic extrema checks; exact-head tests, finite-difference captures and human motion review pending |
| P0 | Resolve portable visual-core/Scene Pack integration | Versioned contract and positive/negative fixtures, no ambiguous settings authority |
| P1 | CPU reference renderer for Ouroboros | **Source implemented** with pixel receipts; exact-head tests, saved golden artifacts and human visual review remain pending |
| P1 | Deterministic Ouroboros tube mesh | **Source implemented** with topology/mesh receipts; exact-head geometry tests and visual review pending |
| P1 | Host-budgeted deterministic LOD | **Source implemented** with Economy/Balanced/Cinematic selection and hard budget checks; exact-head planning tests pending |
| P1 | Portable Ouroboros checkpoint | **Source implemented** with `OUROCKP1` schema and active/Reduced restore tests; exact-head tests and p95 encode/decode measurements pending |
| P1 | Semantic face landmark contract | **Source implemented** with `OUROLND1`, invariant root, orthonormal pose basis and six face attachments; exact-head tests and visual review pending |
| P1 | Landmark-driven 3D face mesh | **Source implemented** with five closed parametric regions and `OUROFAC1`; exact-head geometry tests, attachment/occlusion visual review and perceptual evaluation pending |
| P1 | Sealed visual-evidence bundle | **Source implemented** with `OUROEVB3` binding trace, checkpoint, landmarks, face mesh, body mesh, CPU pixels and complete LOD budget; exact-head tests and cross-backend comparison pending |
| P1 | CPU benchmark harness | **Source implemented** with JSONL p50/p95/p99 reports for scene, landmarks, checkpoint, replay, LOD, body mesh, face mesh, CPU capture and evidence sealing; exact-head build pending |
| P1 | Named-host benchmark capture | **Source implemented** with five independent release-process trials, strict workload/source checks, median-of-trial percentiles, spread and SHA-256 evidence; script/aggregation CI smoke pending and no named-host result measured |
| P1 | WGPU Ouroboros renderer | Same semantic fixtures, GPU shader validation, reference comparisons and named-device performance evidence |
| P1 | Waywallen bridge smoke renderer | Exact protocol handshake, buffer negotiation, DMA-BUF publication, SYNC_FD and release-point lifecycle |
| P2 | Sandboxed Ouroboros Waywallen plugin | Per-user plugin install, typed scene settings, renderer process sandbox, static fallback and daemon lifecycle |
| P2 | Waywallen idle/screen-saver integration | Session idle integration, hidden-state pause, reduced-motion and power policy |
| P3 | Lock-screen background adapter | Independent security review and lock/unlock transition capture |
| P3 | Sovereign Boot visual handoff | VM handoff gates, then separate physical canary evidence |

The new branch is stacked on the open standalone visual-core hardening PR. It is deliberately not based on the default branch because the portable core it extends is not yet merged. The new module's tests have not yet been run successfully on this exact branch head; no qualification claim is made until exact-head checks complete.

## References

- KDE Plasma wallpaper plugin and per-user location: https://develop.kde.org/docs/plasma/
- KDE Plasma WallpaperItem API: https://api.kde.org/qml-org-kde-plasma-plasmoid-wallpaperitem.html
- wgpu cross-platform Rust graphics API: https://wgpu.rs/doc/wgpu/
- OpenWallpaper: https://openwallpaper.org/
- Waywallen source reviewed at commit `e2ed0b0adc6be6477c7bcc5015c76f4a1bb2b437` (package version 0.6.0, renderer IPC v3, spawn v10); the v0.5.0 release is a separate compatibility target
- Official Wallpaper Engine platform support FAQ: https://help.wallpaperengine.io/en/functionality/linuxmacos.html
- Community Wallpaper Engine for KDE Vulkan renderer: https://github.com/CaptSilver/wallpaper-engine-kde-plugin
- we-layerd Wayland runtime and architecture: https://github.com/Aromatic05/we-layerd
- Fresco Linux wallpaper runtime and performance-mode notes: https://github.com/DibbayajyotiRoy/Fresco
- Sovereign Boot visual benchmarks: https://github.com/Luminous-Dynamics/sovereign-boot/blob/research/visual-genome-v1/docs/VISUAL_BENCHMARKS.md
