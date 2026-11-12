# Portable Visual Runtime: Contract, Capabilities, and Conformance

**Status:** design contract proposal; implementation/qualification is tracked separately  
**Scope:** Sovereign Boot's reusable visual engine and its browser, WASI, native, and privileged-surface adapters  
**Non-goal:** claiming that one binary can override every operating system surface

## Design thesis

Make the *scene* portable, not the operating system. A single versioned visual contract should let a browser, a WASI component host, a native app, a TV app, or a narrowly scoped boot renderer consume the same scene semantics while each host owns presentation, lifecycle, device access, accessibility, and policy.

The architecture has four separable layers:

1. **Scene semantics** — seeded state, bounded simulation steps, semantic events, versioned settings, and observable state.
2. **Render plan** — backend-neutral draw operations or scene data, independent of a particular windowing system or GPU API.
3. **Renderer backend** — CPU RGBA reference renderer first; optional GPU backends later. Every backend must preserve the declared visual semantics or explicitly declare a lower-fidelity profile.
4. **Host adapter** — browser canvas, native application surface, Android wallpaper service, TV application, Linux desktop surface, or boot-only DRM/KMS adapter. It owns scheduling and permissions.

The existing `visual-core`, `visual-wasm`, `visual-component`, and `visual-wasi` crates are the first implementation slice. This document proposes the next contract layer; it does not claim every layer is implemented.

## Normative invariants

- **Version every public contract.** Scene semantics, serialized settings, render-plan format, and host capabilities are distinct versioned surfaces. A crate version is not a substitute for a scene-contract version.
- **No ambient authority in the scene engine.** The portable core must not open displays, inspect the OS, read arbitrary files, access the network, query user identity, or control lock/login/boot state.
- **Host authority is explicit.** The host grants only the capabilities it needs. Missing capabilities produce a declared fallback, not an implicit privilege escalation.
- **Bound every resource.** Validate dimensions, total pixels, frame delta, input list lengths, event queue size, and serialized state size before allocation or execution. Reject or normalize only where the ABI cannot return construction errors, and expose the effective normalized values.
- **Separate simulation from presentation.** The host schedules frames and decides whether a surface is visible. A hidden, suspended, battery-constrained, thermal-constrained, or reduced-motion surface must not keep simulating at full rate.
- **Preserve a reference renderer.** CPU rendering and fixed test fixtures remain the oracle for backend conformance. GPU paths are optimizations, not new sources of scene truth.
- **Never silently weaken a profile.** If a host cannot meet a requested quality, motion, color, or frame-rate profile, report the negotiated profile and use an explicit fallback.
- **Keep privileged lifecycle outside the visual component.** Boot progression, authentication, recovery, secure login, and display restoration are controlled by trusted native code, never by a guest visual module.

## Implemented settings boundary on the hardening branch

The first typed settings slice now exists in `crates/visual-core/src/settings.rs` and `crates/visual-core/src/mycelium.rs`. The legacy phrase-based constructor is retained; the additive `with_settings(width, height, seed: u32, settings)` constructor validates dimensions, the requested branch ceiling, the independent `resource_max_branches` ceiling, their relationship, depth, growth rate, tick frequency, pulse period, drift amplitude, opaque RGB palette semantics, and the modeled renderer-allocation estimate before allocation.

The configured renderer enforces its branch/depth limits during growth, maps all six palette roles to the CPU renderer, applies growth-rate scaling and deterministic positional drift, and schedules periodic pulses by integer simulation ticks. Fixed-step batches are bounded by `min(fixed_step_hz, 120)`, so no call advances more than one simulated second. Supported browser/WIT host APIs lock each scene to either variable-delta or fixed-tick stepping and reject attempts to mix them. The legacy low-level variable-delta `grow` primitive retains its historical minimum crawl at zero activity; the new configured fixed-tick path treats zero activity as no growth.

Both the browser ABI and WIT Component Model now accept typed settings; the WIT resource reconfiguration is atomic on validation failure. Browser smoke fixtures, Rust unit fixtures, and the Wasmtime host harness cover replay, bounds, branch budget, settings rejection and resource cleanup. These are implemented test definitions, not pass claims: exact-head execution remains required.

The deterministic canvas-to-substrate static gradient renderer is implemented in core and exposed through browser WASM and WIT with brightness validation and non-mutating behavior. The remaining interoperability boundary is explicit: there is no Scene Pack JSON loader here yet; presentation-variant selection, safe regions, visibility/suspend policy, fallback activation, and whole-process memory accounting remain adapter/loader work. See `SCENE_PACK_INTEROPERABILITY.md` for the field-by-field mapping and acceptance gates.

## Scene Pack v1 loading

The separate `sovereign-visual-pack` crate is now the strict manifest boundary: bounded JSON input, duplicate-key rejection, unknown-field rejection, typed version/range checks, safe-relative-path validation, presentation/safe-region checks, consent invariants and dual branch-budget validation. `ValidatedScenePack` preserves host-facing metadata, exposes immutable typed renderer settings, and can instantiate the same core scene used by the other adapters.

The parser intentionally has no filesystem authority. The capability-denied Wasmtime qualification harness, the WASI CLI, and the browser demo now use it directly. The browser host fetches the pinned same-origin manifest and supplies its bytes; the guest does not fetch URLs or open paths. The browser and WASI adapters accept centered-network boot and gradient-only static fallback. They reject asset-bearing packs because no `AssetHashProvider` is wired, and refuse required capabilities, inputs, safe regions or unsupported compositions rather than ignoring them.

A trusted host must explicitly supply an `AssetHashProvider` whose implementation safely resolves an asset under the pack root (including symlink checks) and computes SHA-256; the parser fails closed on resolver errors or digest mismatch. This is not yet a complete schema-conformance claim. Exact-head CI and a schema-generated positive/negative conformance corpus remain prerequisites.

## Determinism and reproducibility

The seed alone is not a complete reproducibility contract. The full replay input is:

- scene contract version;
- seed encoding and normalization rules;
- initial dimensions and scene settings;
- ordered semantic events;
- ordered simulation steps and exact input values;
- render backend/profile version when pixel output is compared.

Qualification should distinguish these levels:

1. **Same-target replay:** same artifact, same replay input, same output.
2. **Cross-target semantic parity:** same externally observable scene state and declared behavior.
3. **Cross-target pixel parity:** exact RGBA byte equality for named target/runtime combinations and fixtures.
4. **Backend visual equivalence:** measured image differences within a documented tolerance; this is not byte parity.

Only level 3 should be called byte-for-byte parity, and only for the targets actually tested. Do not infer parity for untested architectures, compiler flags, runtime versions, or GPU drivers. If future cross-target replay requires stronger guarantees, consider fixed-point simulation or explicitly specified quantization rather than assuming floating-point equivalence.

## Capability negotiation

Hosts should declare a small capability document before creating a scene. The initial conceptual fields are:

- `contract_versions`: versions supported by host and guest;
- `max_width`, `max_height`, and `max_pixels`;
- `render_modes`: reference CPU, host GPU, or other named backends;
- `motion_modes`: full, reduced, static;
- `lifecycle`: visibility, suspend/resume, teardown signals available;
- `resource_budget`: memory ceiling and target frame-time budget;
- `surface_kind`: browser content, app surface, desktop integration, or boot-only;
- `host_permissions`: explicit, auditable capabilities; empty by default.

Negotiation is deterministic: choose the highest mutually supported contract version and a profile that fits all declared limits. If no valid profile exists, fail closed with a typed incompatibility result. Do not let the guest request additional authority after creation.

This capability document is a design sketch, not yet a published WIT interface. It should be implemented only after the existing WIT and browser APIs are reviewed together to avoid two competing sources of truth.

## Scene Pack interoperability

The current engine is not yet a Scene Pack v1 consumer. See [`SCENE_PACK_INTEROPERABILITY.md`](SCENE_PACK_INTEROPERABILITY.md) for the exact field mapping, numeric-seed contract proposal, fixed-step semantics, resource-budget rules, additive API strategy, and fail-closed acceptance gates. This integration takes precedence over adding another renderer backend.

## Graphics backend decision

The architecture decision record is in [`GRAPHICS_BACKEND_DECISION.md`](GRAPHICS_BACKEND_DECISION.md). It keeps CPU RGBA as the reference, treats wgpu/native and browser-WebGPU as optional future renderers, and leaves wasi:webgpu and wasi-gfx behind a future adapter boundary while their surface/runtime contracts continue to evolve.

## Proposed portable render-plan boundary

The current RGBA frame API is a valuable compatibility and test oracle, but full-frame copies scale poorly on high-resolution and constrained devices. Introduce a render-plan boundary only after profiling proves it is needed.

A future render plan may contain bounded, versioned primitives such as:
- clear/background;
- path or polyline with explicit coordinate space;
- filled/stroked geometry;
- gradient or palette reference;
- opacity and blend mode from a deliberately small allowlist;
- semantic animation parameters rather than host-clock reads.

Do not expose arbitrary shader source, native handles, file paths, or host callbacks in the portable plan. Each command must have bounded counts and finite numeric inputs. Unknown mandatory commands fail closed; unknown optional metadata can be ignored only if the schema explicitly permits it.

The CPU RGBA renderer remains the golden reference. A GPU backend is accepted only after deterministic fixtures, resource-budget checks, and visual-difference tests are published. Zero-copy buffers or shared memory are optimization details negotiated by the host, never assumed by the guest.

## ABI and lifecycle rules

- Constructors that cannot return errors must normalize dimensions using the shared core policy and expose effective dimensions immediately.
- Methods with invalid inputs return stable, documented errors. Error text is for diagnostics; callers should not parse English strings as machine codes.
- Every host owns frame scheduling. Simulation deltas are bounded; suspend/resume gaps are handled by pause/reset policy, not by one giant catch-up step.
- Scene resources are explicitly disposed when the surface is torn down. No background work survives teardown unless a separately versioned host capability permits it.
- Static-frame mode must be a first-class output, not a special-case crash path.
- Host input is reduced to semantic events. Raw keystrokes, touch coordinates, system notifications, and user identity are not forwarded unless the specific application needs them and documents why.
- Accessibility and reduced-motion preferences are host policy and must be honored without granting the guest access to unrelated platform APIs.

## Qualification matrix

Do not mark a target as supported merely because its artifact compiles.

| Gate | Evidence required | What it does not prove |
|---|---|---|
| Core unit tests | exact-head test result and fixtures | browser or component execution |
| Browser WASM smoke | named Node/runtime, bindings, native fixture comparison | real browser lifecycle or GPU behavior |
| Real-browser test | named browser versions, canvas output, visibility/reduced-motion handling | mobile wallpaper or desktop integration |
| WASI component execution | named runtime version, successful instantiation, resource lifecycle, frame dimensions/bytes | access to a display or OS surface |
| Native adapter test | named OS/API, teardown and failure-path evidence | other OS versions or OEM variants |
| Performance profile | repeatable measurements at 720p, 1080p, and representative high-resolution workloads | thermal/battery behavior on every device |
| Physical privileged-surface test | exact device/firmware, recovery path, evidence capture | universal boot/lock-screen compatibility |

Each evidence record should include source commit, artifact digest, toolchain/runtime versions, command, exit status, test fixture IDs, and an explicit non-claims list. A queued run is not a pass; a stale-head run cannot qualify a newer commit.

## Implementation sequence

1. **Freeze and review the existing v1 scene contract.** Ensure the Rust limits, WIT API, browser ABI, CLI, and docs agree. Add boundary fixtures for dimensions, non-finite values, seed handling, and RGBA layout.
2. **Run the actual component in a named runtime.** The branch now includes `tools/component-host-smoke`, an isolated Wasmtime 49.0.2 host that binds to the production WIT world and exercises construction, bounded stepping, invalid-input rejection, rendering, seeded replay, dimension normalization, and resource disposal. Its linker intentionally provides no imports, so unexpected host/WASI requirements fail instantiation. CI execution is pending until the exact branch-head run finishes successfully; the generated host lockfile is temporarily uploaded as an artifact for review and pinning.
3. **Create a machine-readable conformance fixture format.** Store replay inputs and expected reference outputs with schema version and hashes; keep golden fixtures small and deterministic.
4. **Measure before optimizing.** Capture CPU time, peak memory, bytes copied, and frame latency at 720p, 1080p, and 4K-ish workloads under the declared pixel budget.
5. **Add a render-plan prototype only if measurements justify it.** Preserve RGBA reference output and compare the new backend against it.
6. **Add host adapters by product and permission model.** Browser content, desktop wallpaper, mobile wallpaper, TV app, and boot splash are separate adapters with separate acceptance gates.

## Upstream basis

- WebAssembly specifies a portable execution model, while imported APIs are provided by the host: https://webassembly.org/docs/portability/
- WIT defines typed component interfaces and versioned packages: https://github.com/WebAssembly/component-model/blob/main/design/mvp/WIT.md
- WASI uses modular interfaces and host-specific capability sets rather than assuming every host exposes every API: https://github.com/WebAssembly/WASI/blob/main/docs/DesignPrinciples.md
- WASI 0.3 adds async, streams, and futures; runtime/toolchain support must be qualified before adopting newer interfaces: https://wasi.dev/roadmap

## Current evidence boundary

A real Wasmtime host harness and CI invocation now exist on the hardening branch, but implementation presence is not a passing result. Until an exact-head CI run succeeds, do not claim that the component has instantiated or completed the fixture. The harness does not establish browser lifecycle, GPU performance, native/mobile/TV integration, or qualification of any privileged surface. Those claims require separate exact-head artifacts and recorded evidence.
