# Ouroboros × Waywallen renderer integration

This directory contains a **renderer-only plugin manifest template**. It is not installable until the `luminous-ouroboros-renderer` executable and its native dependencies have been packaged at the path declared by the manifest.

## Pinned upstream compatibility target

- Repository: [waywallen/waywallen](https://github.com/waywallen/waywallen)
- Reviewed source revision: `e2ed0b0adc6be6477c7bcc5015c76f4a1bb2b437`
- Upstream package version at that revision: `0.6.0`
- Renderer IPC protocol: v3 (protocol v3)
- Renderer spawn version: 10 (spawn version 10)
- Bridge library: MIT-licensed `waywallen-bridge` C ABI in the same upstream repository's `bridge/` directory

The released v0.5.0 tag is a separate compatibility target. Do not assume the moving `main` branch and v0.5.0 have identical bridge APIs or lifecycle behaviour. Re-pin this document and run the adapter conformance suite when changing the upstream commit.


### Current color-capability compatibility limit

The reviewed source contains a color-bit mapping discrepancy that must be resolved before advertising non-sRGB formats:

- The C bridge header `bridge/include/waywallen-bridge/protocol_bits.h` (blob `ed233605b52251d796c01ff26325fded298fab47`) assigns sRGB/BT.709/BT.2020 to bits 0/1/2.
- The daemon's Rust negotiator `src/wallframe/dma/negotiate.rs` (blob `1e24d22e7377ef195705c11462ef653363aa1fbd`) assigns sRGB/linear/BT.601/BT.709/BT.2020 to bits 0/1/2/3/4.
- The C bridge Vulkan pool currently advertises only sRGB + limited range + premultiplied alpha (blob `96bcf1726180be3312b9ef7fb1917ea27244ce0b`), so the initial default path is compatible.

Ouroboros therefore keeps its initial producer color declaration sRGB-only. Do not advertise BT.709, BT.2020, linear or BT.601 until the C and Rust definitions are reconciled with a cross-language IPC conformance test. Tracking issue: [sovereign-boot #22](https://github.com/Luminous-Dynamics/sovereign-boot/issues/22).

## Security boundary

The manifest intentionally omits `plugin.entry` and `plugin.entry_version`. Waywallen's Lua entry point executes inside the daemon with the user's privileges. Ouroboros scene logic belongs in the standalone renderer process, never in daemon-loaded Lua.

The renderer process should receive only:
- the selected scene-pack/resource path,
- validated typed settings,
- render-target/buffer negotiation messages,
- explicitly subscribed lifecycle or pointer events.

The process should not receive network, camera, input-device or general filesystem capabilities. Keep semantic simulation, checkpointing and asset validation inside the Luminous renderer/core boundary. The daemon remains the display and lifecycle host.

## Required implementation sequence

1. Validate this renderer-only manifest against the pinned Waywallen v0.6.0 registry. Confirm that no Lua entry is loaded and that the renderer definition is discovered with spawn version 10.
2. Implement a safe Rust wrapper over `waywallen-bridge`. Pin the exact upstream commit and record the C header/library hashes in the build evidence.
3. Complete the handshake and buffer lifecycle in protocol order:
   - Connect to the daemon Unix socket.
   - Receive `Init`; validate protocol v3 and spawn v10.
   - Send `Ready` with the selected DRM render-node identity, or (0,0) when unknown.
   - Send the producer-owned release sync-object timeline and format capabilities.
   - Receive negotiated buffer directives, allocate/apply the pool and send `BindBuffers`.
   - Submit frames with monotonically increasing sequence numbers, a per-frame acquire `SYNC_FD`, and a valid release-point value.
4. Implement render-target capability/result messages and validate revision ordering before supporting automatic display sizing. The pure protocol oracle now models this state machine, but the real adapter is not implemented.
5. First prove process lifecycle and buffer/fence correctness using a minimal clear-color frame. Only then add direct Vulkan rendering of the Ouroboros mesh.
6. Verify pause/covered behaviour, stop/restart, resize, hot-unplug and producer failure while preserving the CPU/static fallback.

## Synchronization is a correctness contract

A frame-ready message must carry a binary-semaphore-exported `SYNC_FD` acquire fence. The producer's release timeline is a DRM sync object; consumers signal the declared release point after their GPU work retires. Do not substitute OPAQUE_FD timeline exports, omit release points, reuse a slot before release, or invent a frame-ready shortcut.

Cross-GPU paths must be tested separately from same-device zero-copy paths. Unknown render-node identity must be treated conservatively, as the daemon protocol specifies.

## Packaging

The plugin must ship a newline-separated `files.txt`. Its binary path is relative to the plugin directory and mirrors the upstream glTF plugin's packaging layout. The initial manifest deliberately omits the special `resolution = -2` setting, because it requests the daemon's Auto resolution policy. Add Auto sizing only after the renderer implements render-target capabilities, revision ordering, exact pool-generation result checks and those paths have been qualified.
