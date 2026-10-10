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

## New package boundary

### `crates/visual-core` — the source of truth for scene behavior

Owns the color palette, seeded mycelial network, bounded growth state, pulse/contraction events and CPU pixel output. It has no DRM, window-system, browser or mobile dependency. The Linux renderer re-exports and consumes this crate, rather than maintaining a separate compiled copy of the simulation.

The first extraction preserves the existing algorithm to reduce migration risk. Its CPU framebuffer output is a compatibility backend, not necessarily the fastest path for a 4K TV or low-power phone. Future GPU renderers can consume the same scene state once the scene/command contract is formalized.

### `crates/visual-wasm` — browser ABI

Feature `web` exports a `VisualScene` through `wasm-bindgen`. Host code can create a seeded scene, advance it with bounded frame deltas, pulse or contract the scene, and request tightly packed RGBA8 bytes. Dimensions and per-frame timing/activity are bounded.

It deliberately does not access `window`, the DOM, canvas, clocks, storage, device APIs or display state from Rust. A browser host owns `requestAnimationFrame`, visibility changes, canvas upload, reduced-motion preferences, and suspend/resume behavior. A generated wasm-bindgen JS package and user-facing page/wallpaper integration are still required before calling this a browser product.

Build validation command used in CI:
`cargo +1.96.0 build -p sovereign-visual-wasm --target wasm32-unknown-unknown --features web --release --locked`

### `crates/visual-wasi` — WASI execution proof

The first WASI executable takes bounded scene parameters and emits one PPM image to standard output. This proves the same Rust simulation can be compiled against a WASI target without requiring Linux DRM.

It is intentionally a headless smoke target. It does not show an image on screen, control a desktop, implement a boot splash, or expose a stable typed component interface to other programs yet.

Build validation command used in CI:
`cargo +1.96.0 build -p sovereign-visual-wasi --target wasm32-wasip2 --release --locked`

A natural next step is to define a versioned WIT interface for a reusable Component Model guest (create scene, advance, emit frame or scene commands). That should follow API review of memory copies, resource lifetime, bounded calls, error variants, pixel format, and frame transport. The current WASI PPM executable is not to be marketed as that finished component.

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

## Qualification requirements

Do not label these targets supported until CI and test evidence establish:
- native core unit tests and deterministic output hashes/fixtures;
- browser-target compilation and binding generation;
- WASI-target compilation and execution under a named runtime;
- explicit bounds and invalid-input tests;
- identical defined scene semantics across native, browser-WASM and WASI;
- resource/time measurements at representative resolutions;
- lifecycle handling in a real browser/app host; and
- separate target-specific evidence for each desktop, mobile, TV, boot or privileged surface.

The CI workflow now compiles both browser-WASM and WASI targets. Until those exact-head workflow runs are green, target compilation must remain **pending**, not pass. Even after they pass, this demonstrates compilation—not actual browser presentation or support for desktop, phone, TV, login, lock or firmware surfaces.

## Upstream technical references

- WebAssembly portability and host-defined imports: https://webassembly.org/docs/portability/
- Rust minimal browser-oriented target: https://doc.rust-lang.org/stable/rustc/platform-support/wasm32-unknown-unknown.html
- Rust WASI Preview 2 target: https://doc.rust-lang.org/rustc/platform-support/wasm32-wasip2.html
- WASI specifications and versioning: https://github.com/WebAssembly/WASI
- Wasmtime's runtime and Component Model support: https://docs.wasmtime.dev/introduction.html
- wasi:webgpu (GPU API proposal; display/windowing is explicitly out of scope): https://github.com/WebAssembly/wasi-webgpu
