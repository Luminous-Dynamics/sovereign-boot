# Ouroboros and the Luminous Visual Runtime

**Status:** portable scene state, C1-continuous bounded Hermite morph retargeting, deterministic CPU rasterization, a bounded 3D tube mesh, canonical event traces and content-addressed replay/image/mesh receipts are implemented on `feat/ouroboros-identity-core-v1`. Tests are authored but have not yet been executed successfully on this exact branch head. No WGPU desktop renderer or Waywallen adapter is claimed by this document.

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

The CPU renderer also exposes `render_reference_artifact`, which binds RGBA bytes to width, height, scene-contract version and a BLAKE3 pixel digest. These receipts make a run inspectable and replayable; they do not establish cross-architecture floating-point parity, GPU parity, visual quality or authenticity without a separately verified source/build and capture process.

The fourth core module, `crates/visual-core/src/ouroboros_mesh.rs`, converts the semantic centerline into a bounded closed tube mesh:
- Stable ring/tube tessellation with explicit resource ceilings (ring 32–2048, tube 6–64 segments).
- A deterministic low-twist frame projected from the scene's bounded centerline tangent.
- Unit normals, a closed periodic centerline, duplicated UV seam vertices with byte-identical positions/normals, and stable indices across every semantic form.
- An explicit outward triangle winding contract validated for every triangle, plus exact canonical index topology and hostile-public-data bounds checks.
- Canonical little-endian mesh serialization and a BLAKE3 receipt binding the scene-contract version, tessellation, vertices, normals, UVs, form weights and indices.

The mesh is a geometry artifact, not a final character model or shader. It has no face sculpt, material graph, animation rig, GPU upload path, shadowing or physically based lighting. Its test coverage is authored but not yet qualified on this exact PR head.


The mesh API also exposes deterministic host-budget planning. The host supplies projected ring/tube pixel radii plus maximum vertex, index and byte budgets. The core selects a deterministic feasible tessellation tier (Economy, Balanced or Cinematic) using a quantized ring/tube ladder and exact resource estimates. The returned plan includes both the requested and selected ring/tube segments, so a host can see where the shared budget traded cross-section detail for silhouette resolution. If even the minimum mesh cannot fit, it returns `BudgetTooSmall`; the host must choose a CPU/static fallback. This keeps display/GPU discovery out of the scene core while allowing predictable resolution/power scaling. The same budget input produces the same plan, and the plan reports exact counts and estimated geometry bytes.

**Motion-quality boundary:** the source now uses bounded cubic-Hermite segments. A retarget carries the current form-weight derivative into the new segment and uses a zero-velocity terminal condition. Candidate segment duration is shortened until analytic cubic extrema remain inside the validated weight range; no per-frame allocation is introduced. Reduced mode freezes and restores the kinematic state. C1 tests cover all 64 initial/target/retarget triples, finite-difference derivative agreement, repeated retarget bounds, freeze/resume and zero terminal velocity. These tests are authored but not yet qualified on the exact PR head; C1 is an implementation claim awaiting exact-head test evidence and visual review. Acceleration continuity (C2) is not claimed.

## 3. Architecture

Keep scene semantics portable and host capabilities separate.

| Layer | Ownership | Current boundary |
|---|---|---|
| Scene identity and state | Rust visual core | Ouroboros v0.1 state model now added |
| Scene pack / visual contract | Versioned data + Rust validation | Existing Scene Pack work is a separate integration track |
| Reference renderer | Rust CPU RGBA | Mycelial scene plus stylized Ouroboros CPU reference rasterizer; content-addressed capture receipts authored, golden artifact review pending |
| Geometry | Rust CPU indexed mesh | Closed Ouroboros tube mesh, invariant topology, deterministic UV seams, normals, hostile-input validation and mesh receipt authored; exact-head geometry tests pending |
| Quality planning | Host-provided projected size and resource ceilings | Deterministic Economy/Balanced/Cinematic mesh planner with exact vertex/index/byte estimates; exact-head budget tests pending |
| Replay/evidence | Canonical Rust event trace + BLAKE3 | Versioned input trace and full internal state digest authored; exact-head tests/replay receipts pending |
| Persistence | Canonical Rust checkpoint + BLAKE3 | Bounded `OUROCKP1` checkpoint API and active/Reduced restore tests authored; exact-head qualification pending |
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

The competitive set has moved beyond simple video wallpaper players:

| Product | Publicly documented strengths | Opportunity for Luminous |
|---|---|---|
| Wallpaper Engine | Large Workshop catalogue, editor for images/video/web/apps, mature Windows and Android ecosystem | Do not compete on catalogue size first. Build a portable, open, Nix-native semantic scene runtime and original living entities. |
| OpenWallpaper | Programmable WASM scenes, Wallpaper Engine scene conversion, video/GIF, audio-spectrum input, multi-monitor support and several Wayland environments | Match the important runtime basics: scene API, multi-monitor lifecycle, pause when hidden and video/static fallback. Differentiate through deterministic scene contracts, trust boundaries and continuity with boot. |
| Waywallen | KDE/GNOME integration, layer-shell desktops, hardware video decoding, plugins and external Wallpaper Engine Scene/Web integration | Treat desktop-shell integration, input routing, auto-pause and renderer health as table stakes. Differentiate through shared Rust scene semantics and evidence-backed accessibility/resource behavior. |
| Luminous Visual Runtime | Portable Rust state model, typed bounded settings, CPU reference, browser/WASI slices and separate boot renderer | Build a persistent semantic entity model, reproducible scene evidence and a strict separation between visual capability and host authority. |

Wallpaper Engine's publisher lists Windows 10/11 and Android, and its support FAQ says Linux/macOS are not currently supported; third-party KDE/Plasma Scene/Web/Video integrations nevertheless exist and must be treated as real Linux competitors. OpenWallpaper and Waywallen are therefore the more relevant native Linux runtime comparators, not proof that the Linux market is empty.

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

### Persistence and handoff

- Active C1 trajectories must resume with identical per-channel values, velocities and subsequent state on the same build/target.
- Reduced checkpoints must remain frozen until the host explicitly resumes the entity.
- Snapshot size target <=256 bytes; p95 encode/decode <=100 microseconds for 1,000 in-process operations on the reference laptop CPU.
- Invalid, oversized, unknown-version, out-of-range and trailing-byte snapshots are rejected before use.
- Checkpoints contain semantic state only. Renderer handles, file paths, DMA-BUFs, FDs, display identity and permissions are never persisted.

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
- Waywallen: https://waywallen.github.io/ (v0.5.0, released 2026-10-20, reviewed for the renderer/desktop architecture)
- Wallpaper Engine platform support FAQ: https://help.wallpaperengine.io/en/functionality/linuxmacos.html
- Sovereign Boot visual benchmarks: https://github.com/Luminous-Dynamics/sovereign-boot/blob/research/visual-genome-v1/docs/VISUAL_BENCHMARKS.md
