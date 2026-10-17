# wgpu / Vulkan desktop renderer prototype

**Status: shader prototype, with a separate validation-only headless wgpu reference renderer and GPU benchmark under active qualification.** The reference utility can render offscreen captures and report optional hardware GPU timestamps. It is not yet integrated into a desktop compositor, wallpaper, lock-screen adapter, or boot path, and its current-head compile/render checks must pass before its results are treated as qualified.

The first shader, `shaders/mycelial_field.wgsl`, now selects one of ten stable procedural scene families from the same theme manifest used by the CPU renderer: mycelial network, solar canopy, cosmic cartography, ocean waves, epistemic lattice, quantum interference, aurora ribbons, graphite minimal, verdant recovery, and civic commons. It uses a single aligned uniform buffer and a full-screen triangle; it loads no images, does not read system state, and makes no network requests. The scene-family index is part of the host/shader ABI and has parity tests against all manifest themes.


## Executable headless reference path

A separate validation-only crate now lives at `validation/wgpu-reference`. It loads this shader with wgpu, reads palettes and motion scales from the v1 JSON manifest, renders to an offscreen sRGB texture, exports a P6 PPM capture, and returns a machine-readable timing receipt. It is not a window, wallpaper plugin, lock-screen adapter, or boot dependency.

Example (the backend must exist on the selected host):

```bash
cargo run --manifest-path validation/wgpu-reference/Cargo.toml --release -- \
  --backend vulkan --theme mycelial-genesis --motion reduced --time 0 \
  --width 1920 --height 1080 --frames 1800 --output /tmp/visual-genome.ppm
```

The receipt separates CPU command-recording/submission time, offscreen render-plus-readback wall time, and GPU execution samples when `TIMESTAMP_QUERY` is supported. GPU timings are `null` when unsupported; CPU submit time is never relabelled as GPU time. Replayed frames share identical scene inputs, so the frame loop is a throughput/profile workload, not animation cadence or display-presentation latency. Archive host, adapter, driver, backend, dimensions, scene inputs, and exact lockfile with results. Performance remains unqualified until exact-head CI and target-hardware runs finish.

CI uses Mesa's headless Vulkan software adapter to render all ten theme palettes, rejects structurally flat captures, and compares an exact-repeat capture. This exercises every manifest palette through the executable shader path and demonstrates reproducibility on that adapter only; it does not establish performance or visual quality on the laptop GPU or a physical monitor.

## Intended architecture

- **Host API:** Rust + wgpu. Request the Vulkan backend when available; select a compatible backend only under an explicit policy.
- **Shader:** WGSL, validated by the wgpu/Naga pipeline at startup or build time.
- **Fallback:** CPU theme renderer and then static background. A shader or driver error must not prevent desktop startup.
- **Input contract:** resolution, monotonic scene time, motion scale, and five theme colors. Theme values are visual-only, not trust or machine-state claims.
- **Color contract:** convert manifest sRGB colors to linear-light RGB before upload. The convergence uniform uses RGB for the final tint and its alpha component as the per-frame convergence weight.
- **Process boundary:** run at user privilege in the desktop session. Do not reuse the privileged early-boot process for wallpapers, lock screens, or interactive content.

## Next integration steps

1. Add a separate desktop-renderer crate with wgpu as an optional dependency; do not add wgpu to the DRM/boot binary.
2. Run `cargo run --manifest-path validation/wgsl-check/Cargo.toml --locked -- renderers/wgpu/shaders/mycelial_field.wgsl` to parse and semantically validate WGSL without creating a GPU device; then add an offscreen wgpu pipeline-compilation test.
3. Render a reference CPU scene and GPU scene with identical theme/seed/time inputs; archive captures and compare perceptual error after color-space conversion.
4. Add frame-time, GPU-time (when available), memory, dropped-frame, and power measurements.
5. Wire KDE Plasma wallpaper support only after lifecycle tests show that rendering pauses or reduces work when hidden and exits cleanly at logout.
6. Do not use the Wallpaper Engine plugin as a lock-screen implementation: upstream forks document that locking is not supported. The lock screen should keep the desktop environment's authentication overlay and use only a background adapter with static fallback.

This prototype intentionally does not declare its visual quality, shader compilation, or performance qualified until those gates are executed on the exact commit.
