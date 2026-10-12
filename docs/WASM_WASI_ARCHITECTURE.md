# WASM and WASI: the Portable Sovereign Visual Core

**Status:** first implementation slice on the hardening branch; compile/test qualification pending  
**Reviewed:** 2026-10-10

## Decision

The intended portability boundary is a platform-neutral Rust scene core that can be compiled to several targets—not a single WASM file that can seize any screen on any operating system.

The same visual state and deterministic rendering algorithm should work across:
- native Rust/Linux DRM/KMS for a narrowly controlled boot renderer;
- `wasm32-unknown-unknown` plus browser bindings for web/canvas hosts;
- `wasm32-wasip2` for a sandboxed WASI host;
- native mobile, TV, desktop, embedded or OEM applications that either link the Rust core or embed a suitable WASM runtime.

Each host still owns presentation, lifecycle, graphics access, input, power policy and privileged surfaces.

## Existing work we should reuse

Symthaea already has a separate browser/WASM path in `crates/domains/symthaea-spore`: a `wasm-bindgen` feature, exported engine APIs, and `build-wasm.sh` targeting `wasm32-unknown-unknown`. That is valuable precedent and reusable ecosystem knowledge, but Spore's reasoning/engine exports are not the same interface as Sovereign Boot's procedural visual scene. Do not add the entire Spore kernel as a dependency just to draw the boot animation.

References:
- Symthaea Spore crate: https://github.com/Luminous-Dynamics/symthaea/tree/main/crates/domains/symthaea-spore
- Spore browser bindings: https://github.com/Luminous-Dynamics/symthaea/blob/main/crates/domains/symthaea-spore/src/wasm_bindings.rs
- Spore browser build: https://github.com/Luminous-Dynamics/symthaea/blob/main/crates/domains/symthaea-spore/build-wasm.sh
- Spore Android native-library build: https://github.com/Luminous-Dynamics/symthaea/blob/main/crates/domains/symthaea-spore/build-android.sh
- Spore iOS native-library build: https://github.com/Luminous-Dynamics/symthaea/blob/main/crates/domains/symthaea-spore/build-ios.sh
- Spore browser WASM regression tests: https://github.com/Luminous-Dynamics/symthaea/blob/main/crates/domains/symthaea-spore/test-wasm.sh

## New package boundary

### `crates/visual-core` — the source of truth for scene behavior

Owns the color palette, seeded mycelial network, bounded growth state, pulse/contraction events and CPU pixel output. It has no DRM, window-system, browser or mobile dependency. The Linux renderer re-exports and consumes this crate, rather than maintaining a separate compiled copy of the simulation.

The first extraction preserves the existing algorithm to reduce migration risk. Its CPU framebuffer output is a compatibility backend, not necessarily the fastest path for a 4K TV or low-power phone. Future GPU renderers can consume the same scene state once the scene/command contract is formalized.

### `crates/visual-wasm` — browser ABI

Feature `web` exports a `VisualScene` through `wasm-bindgen`. Host code can create a seeded scene, advance it with bounded frame deltas, pulse or contract the scene, and request tightly packed RGBA8 bytes. Dimensions and per-frame timing/activity are bounded.

It deliberately does not access `window`, the DOM, canvas, clocks, storage, device APIs or display state from Rust. A starter host page and lifecycle adapter now live in `crates/visual-wasm/www`; the browser owns `requestAnimationFrame`, visibility changes, canvas upload, reduced-motion preferences, and suspend/resume behavior. `build-demo.sh` generates the JavaScript package into an ignored `www/pkg` directory. This is a runnable demo path, not yet a qualified wallpaper product or an assertion of compatibility across all browsers.

Build validation command used in CI:
`cargo +1.96.0 build -p sovereign-visual-wasm --target wasm32-unknown-unknown --features web --release --locked`

### `crates/visual-wasi` — WASI command-line smoke target

The WASI executable accepts bounded scene parameters and emits a PPM frame to standard output. It is useful for quick headless execution checks, but it does not show an image on screen, control a desktop, or implement a boot splash.

Build command:
`cargo +1.96.0 build -p sovereign-visual-wasi --target wasm32-wasip2 --release --locked`

### `crates/visual-component` — versioned WIT Component Model API

The WIT contract in `crates/visual-component/wit/visual.wit` exports a stateful scene resource with a `contract-version`, bounded `advance`, `pulse`, `contract`, dimensions, branch count and RGBA8 frame output. This gives compatible component hosts a typed interface and resource lifecycle instead of requiring a process/PPM convention.

The semantic limits are centralized in `crates/visual-core/src/contract.rs` and are shared by the browser, Component Model and WASI adapters: nonzero dimensions; a 4096-per-side ceiling; an 8,294,400-pixel budget; finite frame deltas in 0..=0.25 seconds; normalized activity in 0..=1; and finite contraction progress. The core exposes `SCENE_CONTRACT_VERSION = 1`; the browser ABI and WIT resource expose that version to the host, while the WASI CLI reports it through `--contract-version`. Adapters translate common validation errors into their own ABI's error shape rather than redefining the limits. Version 1 fixes input bounds, host-step semantics, row-major opaque RGBA8 layout, and the deterministic seeded simulation contract; changes that invalidate those guarantees require an explicit contract-version decision.

The WIT resource constructor defensively normalizes dimensions to the shared dimension and pixel budget because WIT constructors cannot return a `Result`. Hosts must query the effective width/height; other adapters can reject invalid sizes. All frame presentation stays on the host side. The component has no display or input imports and must not be granted device capabilities simply to render a scene.

The branch also includes `tools/component-host-smoke`, a Wasmtime 49.0.2 host that attempts actual instantiation and exercises the exported resource methods while registering no host imports. CI runs this after the component build. Until an exact-head run passes, this is a test implementation—not runtime qualification evidence. The helper's generated lockfile is currently surfaced as a short-retention CI artifact so its resolved dependency graph can be pinned before calling the host test reproducible.

Build command:
`cargo +1.96.0 build -p sovereign-visual-component --target wasm32-wasip2 --release --locked`

The WASI CLI target remains a smoke target; the WIT component is the reusable guest interface. They are different artifacts with different intended uses. Component host execution and interface inspection should become explicit qualification gates after the compile and unit-test gates are green.

## Typed settings and fixed-step browser API

The core now exposes typed `SceneSettings` / `ScenePalette` for numeric Scene Pack seeds, with validation before construction. The profile carries both `branch_limit` (scene request) and `resource_max_branches` (independent host ceiling); construction rejects invalid/inconsistent values and the renderer enforces the scene cap before child allocation. It also applies palette roles, growth rate, depth limits, tick-scheduled pulses, deterministic drift and a modeled renderer-allocation estimate. Fixed-tick batches are bounded to `min(fixed_step_hz, 120)`; hosts must pause/reset on suspend rather than catch up without a bound.

The browser adapter adds a configured constructor accepting a uint32 seed, bounded settings, and six RGB palette triplets, plus `advance_ticks`. The existing phrase-based constructor and dt-based method remain for compatibility. The committed browser smoke script now contains a settings-based deterministic replay fixture and negative checks.

The WIT Component Model now carries the same typed settings record (numeric seed, palette roles, growth/depth limits, tick frequency, pulse period, drift and memory budget) through an atomic resource reconfiguration method, plus bounded fixed-tick advancement. A resource cannot mix the legacy variable-delta and fixed-tick host APIs; attempted mode switches return errors. The deterministic canvas-to-substrate static gradient is also exposed to the browser and WIT as a non-mutating fallback with bounded brightness. Rust fixtures compare configured RGBA against the native core, while browser/WIT/Wasmtime fixtures check replay, fallback endpoints and mode/budget rejection. These tests still need exact-head execution; they are not a claim that Wasmtime qualification has passed.

The core now generates Scene Pack v1's deterministic canvas-to-substrate gradient as an opaque RGBA8 frame, and both browser WASM and WIT expose it as a non-mutating static-fallback operation with brightness bounds. This is the fallback renderer primitive, not a loader or presentation-selection system. The project still does not parse Scene Pack JSON, interpret presentation variants/safe regions, enforce lifecycle across hosts, or prove a whole-process memory limit. Do not call the overall product Scene Pack v1 compatible until those gaps are implemented and qualified.

## Scene Pack validation boundary

A dedicated `sovereign-visual-pack` crate now parses the pinned Scene Pack v1 manifest outside the real-time renderer. It rejects duplicate JSON keys, unknown fields, malformed identifiers/versions/colors, unsafe asset paths, invalid ranges/safe regions, contradictory scene/host branch budgets, and unconsented inputs. It returns an immutable typed `ValidatedScenePack` and can instantiate the shared core settings.

The parser does not open asset paths or call operating-system APIs. Asset verification is delegated through an explicit `AssetHashProvider` so the host can enforce pack-root containment (including symlink checks) and compute SHA-256 under its own policy. This is the correct trust boundary for browser/native hosts and keeps the WASM/WASI guest capability-denied.

The WASI CLI accepts `--scene-pack-stdin` and renders the centered-network `boot` profile or gradient-only `staticFallback`; unsupported compositions, safe regions, inputs, required capabilities and packs with assets fail closed. The capability-denied Wasmtime qualification harness uses the same parser before adapting typed settings into WIT. The browser host packages the pinned manifest, fetches it from its own origin, and supplies its bounded bytes to the guest's `createFromScenePack` API. The browser and WASI adapters currently support boot + static fallback only, while desktop/idle/locked edge-biased variants are rejected because safe-region/composition mapping is not implemented. This is implementation presence, not a qualification pass: exact-head CI is pending, and the parser still needs a positive/negative corpus generated from the authoritative Draft 2020-12 schema before claiming complete schema conformance. General host asset resolution and OS lifecycle/presentation adapters remain follow-on work.

## WASM and WASI are complementary, not interchangeable

- `wasm32-unknown-unknown` is a minimal WebAssembly target typically paired with JavaScript bindings in a browser. Web APIs are supplied by the browser host, not by WASI.
- `wasm32-wasip2` targets the WASI Preview 2 / Component Model environment. A runtime must implement the interfaces the component imports and grant appropriate capabilities.
- WASI does not, by itself, standardize arbitrary access to a window, display, keyboard, secure login surface, TV compositor, or firmware framebuffer. The host has to provide those operations through an API/component contract or a native adapter.
- WASM portability means portable computation under a compatible runtime and imported interfaces. It does not mean every OS installs a WASM runtime, every TV app store permits one, or every system-owned screen can be replaced.

We should use the stable target/runtime combination with the widest practical implementation support, version the component interfaces, and only move to newer WASI interface versions when our supported runtimes can be qualified. Do not tie core scene correctness to a specific WASI preview revision.

## Runtime host contract

Any interactive adapter should drive a bounded step cycle:

1. Create a scene with a schema version, dimensions, seed and approved visual settings.
2. Advance it only while the surface is visible/active, passing a bounded time delta and normalized activity.
3. Pause or lower quality on hide, battery saver, thermal pressure, suspend, display-off or reduced-motion requests.
4. Obtain a frame/scene update and present it through the host-owned surface.
5. Dispose the scene and release buffers on teardown.
6. Keep OS lock/unlock, authentication, boot progression and recovery outside the renderer.

The existing animation algorithm uses a pixel buffer, so frame transfer can be a memory/time cost on small devices. Keep a static-frame fallback; measure 720p/1080p/4K before picking defaults; introduce GPU rendering only after identical scene semantics and image-regression fixtures are stable. Do not allocate at panel native resolution blindly.

## What this makes possible—and what still needs native work

- **Browser:** directly plausible through wasm-bindgen; still needs JS packaging, canvas host, accessibility and browser-version testing.
- **Linux desktop:** can reuse visual-core natively or use a WASM runtime; an actual wallpaper/plugin/splash must integrate with the desktop/boot lifecycle.
- **Android:** can embed WASM or link native Rust into a Kotlin host; Android `WallpaperService`, lifecycle/battery management, and packaging remain native work.
- **iOS/iPadOS:** can reuse scene computation via Rust or a supported WASM engine, but cannot gain unrestricted always-running wallpaper or arbitrary lock-screen capabilities from WASM.
- **Windows/macOS:** can reuse computation through a native Rust library or WASM runtime; shells, screen savers, sign-in and lock surfaces need their own supported integrations.
- **TV OSes:** app-level visualization may be feasible. Whether a WASM runtime can be shipped and whether a screensaver can be installed are separate questions for each TV OS, model, launcher and store policy.
- **Firmware and pre-OS:** ordinary WASI is not a replacement for a bootloader/UEFI framebuffer API. This requires a different trusted boot integration and vendor/firmware permissions.

## Next contract layer

The next-stage design for deterministic replay, explicit capability negotiation, a future backend-neutral render plan, lifecycle rules, and target-specific conformance evidence is documented in [`PORTABLE_VISUAL_RUNTIME.md`](PORTABLE_VISUAL_RUNTIME.md). It is a proposal and qualification plan, not a claim that all of those layers have shipped. Keep the existing RGBA renderer as the reference oracle; do not introduce a second public contract until the WIT and browser interfaces have been reviewed together.

## Qualification requirements

Do not label these targets supported until CI and test evidence establish:
- native core unit tests and deterministic output fixtures;
- byte-for-byte parity between a native Rust reference frame and browser-WASM output for the same seeded scene;
- browser-target compilation, pinned binding generation, Node WebAssembly runtime smoke, and host-script syntax checks;
- WASI-target compilation and execution under a named runtime;
- explicit bounds and invalid-input tests;
- identical defined scene semantics across native, browser-WASM and WASI;
- resource/time measurements at representative resolutions;
- lifecycle handling in a real browser/app host; and
- separate target-specific evidence for each desktop, mobile, TV, boot or privileged surface.

The CI workflow now compiles the browser-WASM adapter, generates pinned wasm-bindgen bindings, runs a Node WebAssembly runtime smoke test over seeded RGBA output, syntax-checks the committed browser host, and compiles the WASI CLI and WIT component. Until exact-head workflow runs are green, those builds/tests remain **pending**, not pass. A Node smoke test is not a real-browser test, and a component build does not prove that every runtime can load it or that a host has integrated its frames. Browser presentation and host execution tests are separate gates; so are desktop, phone, TV, login, lock or firmware surfaces.

## Upstream technical references

- WebAssembly portability and host-defined imports: https://webassembly.org/docs/portability/
- Rust minimal browser-oriented target: https://doc.rust-lang.org/stable/rustc/platform-support/wasm32-unknown-unknown.html
- Rust WASI Preview 2 target: https://doc.rust-lang.org/rustc/platform-support/wasm32-wasip2.html
- WASI specifications and versioning: https://github.com/WebAssembly/WASI
- Wasmtime's runtime and Component Model support: https://docs.wasmtime.dev/introduction.html
- wasi:webgpu (GPU API proposal; display/windowing is explicitly out of scope): https://github.com/WebAssembly/wasi-webgpu
