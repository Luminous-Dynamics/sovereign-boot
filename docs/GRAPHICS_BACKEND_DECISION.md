# Graphics Backend Decision for the Portable Visual Runtime

**Status:** architecture decision record; the future GPU work is not implemented  
**Decision:** keep scene semantics independent from both rasterization and the host presentation surface.

## The principle

Treat these as four different contracts, each with its own version and evidence:

1. **Scene semantics:** what the visual scene means and how its state evolves.
2. **Render backend:** how the scene becomes pixels or drawing commands.
3. **Presentation surface:** where frames are presented and how resize/input/swapchain events are handled.
4. **Host policy:** when the scene may run, which capabilities it receives, and how suspend, reduced motion, battery, thermal pressure, and teardown are enforced.

Do not collapse these into a universal "display API." The host OS, browser, application shell, or boot environment owns the surface and its authority.

## Decision

- **Keep the CPU RGBA renderer as the reference backend.** It is the portable correctness oracle and the fallback for environments without GPU access. Preserve seeded fixtures and compare backends against this reference.
- **Keep the current browser path simple.** The wasm32-unknown-unknown + wasm-bindgen scene API remains browser-compatible and asks the browser host to present frames. Optimize its canvas transfer only after measuring real browser performance.
- **Treat GPU rendering as an optional backend, not a new source of truth.** If profiling justifies it, evaluate wgpu for native backends and its browser-WebGPU backend. Backend/device differences mean we must not presume byte-identical output just because the same Rust API is used.
- **Do not make the portable core depend on wasi:webgpu yet.** The official proposal remains Phase 2. Its stated non-goal is window/screen presentation, so it does not provide the full capability required by our product surfaces. Keep any future WASI GPU adapter behind an optional, versioned interface.
- **Track wasi-gfx as an experimental integration candidate, not as a foundational dependency.** Its ecosystem explores window/surface, framebuffer, graphics-context and WebGPU integration outside the browser. Its maintainers have also described a deliberate separation between WASI's long-stability interfaces and rapidly evolving graphics/surface APIs. That is an argument for an adapter boundary, not a reason to bind the core to a moving proposal.
- **Keep Linux DRM/KMS as a specialized native boot adapter.** It is not an API for websites, phones, TV apps, desktop wallpaper managers or generic WASI components. Boot ownership, display restoration, and recovery stay under trusted native control.

## Backend and surface comparison

| Path | Role now | Portability/evidence boundary |
|---|---|---|
| CPU RGBA from visual-core | Reference/fallback | Deterministic fixtures and exact byte comparison are possible for named toolchains/targets |
| Browser WASM + canvas host | First interactive host | Browser owns canvas, scheduling, visibility, reduced-motion policy and presentation |
| Native Rust + wgpu | Candidate optimization | Use only after measurement; native graphics API and driver are host dependencies |
| Browser WebGPU via wgpu | Candidate optimization | Target-specific API path; compare image results against the CPU reference |
| wasi:webgpu | Monitor, do not require | Phase 2 proposal; GPU API is not a universal window/display API |
| wasi-gfx package family | Experimental future adapter | Useful research path for a host-provided surface, but evolving contracts must remain optional |
| DRM/KMS | Existing specialized boot path | Linux-specific, privileged, recovery-sensitive and separately qualified |

## Conformance levels must stay distinct

1. **Replay determinism:** identical complete inputs give the same defined scene state on a named artifact/runtime.
2. **Byte-identical pixels:** RGBA bytes match exactly for specifically named backend/runtime/build combinations.
3. **Visual equivalence:** output difference stays inside a documented threshold with fixed image fixtures.
4. **Presentation correctness:** the host displays the result correctly under its lifecycle and surface contract.

Passing one level does not imply passing the next. In particular, a WASM build or successful GPU device request does not prove that a window can be opened or that a privileged system surface can be replaced.

## When to introduce a render-plan interface

Do not prematurely build a large scene graph or arbitrary shader language. First measure the current CPU pipeline at representative dimensions (720p, 1080p, and a bounded high-resolution case). Capture frame time, peak memory, bytes copied, and latency. If the bottleneck is full-frame transfer, prototype a *small, bounded, versioned* command vocabulary and keep the current RGBA renderer as the golden backend.

A render plan must not contain host window handles, native pointers, arbitrary filesystem paths, raw OS events, arbitrary shader source, or implicit callbacks. Unknown required commands fail closed. Optional extensions require declared negotiation and bounded resource use.

## Adoption gates for a future GPU backend

- Same scene-contract version and replay fixtures as the CPU reference.
- Explicit negotiated backend/profile and resource budget.
- Bounded allocations and finite command parameters.
- Exact-head, version-pinned build and runtime evidence.
- Image-difference fixtures plus a documented tolerance where exact pixels are not realistic.
- Graceful CPU/static fallback when no compatible adapter exists.
- Separate tests for surface creation, resize, suspension, reduced motion, teardown, and device loss.

No GPU backend is considered supported merely because it compiles or can acquire an adapter.

## Research references

- [WASI WebGPU proposal](https://github.com/WebAssembly/wasi-webgpu) — Phase 2, GPU API scope excludes window/screen display.
- [WASI proposal phase table](https://github.com/WebAssembly/WASI/blob/main/docs/Proposals.md) — WebGPU remains Phase 2 in the current table.
- [The Future of wasi-gfx and wasi:webgpu](https://wasi-gfx.dev/blog/posts/future-of-wasi-gfx/) — June 7, 2026 design discussion about the stability goals of core WASI versus fast-evolving graphics/surface interfaces.
- [wgpu backend documentation](https://docs.rs/wgpu/latest/wasm32-unknown-unknown/wgpu/struct.Backends.html) — native and browser-WebGPU backends are distinct target paths.

## Immediate implementation order

1. Finish exact-head Wasmtime Component Model host qualification.
2. Pin the host harness dependency lockfile.
3. Establish repeatable CPU/browser timing and memory baselines.
4. Only then prototype a render-plan abstraction or GPU backend if the measured gains justify another contract.
