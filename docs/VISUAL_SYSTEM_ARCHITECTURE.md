# Luminous Visual Genome: wallpaper, lock-screen, and boot design

**Status:** RFC for shared manifest/desktop integration; CPU theme and scene support exists in this branch, but host deployment is not qualified.
**Scope:** Sovereign Boot renderer and future Symthaea/Mycelix desktop visuals.
**Principle:** One visual language; separate execution and trust boundaries.

## Why change the architecture?

The extracted framebuffer renderer remains the deterministic CPU reference. This branch introduces ten stable theme IDs, typed palettes, bounded theme-specific growth profiles, command-line selection, and ten distinct CPU scene families: mycelial network, solar canopy, cosmic cartography, ocean waves, epistemic lattice, quantum interference, aurora ribbons, graphite minimal, verdant recovery, and civic commons. Four explicit motion modes control the simulation clock, target cadence and exit transition; Reduced mode renders deterministic still compositions for all ten themes rather than merely slowing animated output. The aligned JSON manifest is design data, not yet dynamically loaded at runtime. A separate Naga validation crate now checks the WGSL shader without requiring a GPU. Plasma wallpaper and secure lock-screen integration remain separate milestones.

A source audit identified implementation gates that must be closed before enabling the newer state-aware boot module on a real host:

1. The renderer now accepts `--receipt`, `--lineage`, and `--handoff-path` as used by the NixOS module. Receipt and lineage files are only checked as nonempty, bounded regular files; the visual process never reads or logs their contents. The compositor marker triggers the same bounded convergence/fade path, with SIGTERM remaining the systemd enforcement path. The NixOS module's default `drmDevice = "auto"` now maps to numeric `/dev/dri/cardN` discovery: only character devices with canonical names are considered, devices are tried in numeric order, and cards without a usable connected display are skipped. The renderer's systemd unit is also restricted to AF_UNIX socket families, since the boot visuals require no IP networking. Unit tests cover symlink/size rejection, but the exact-head test run remains pending.
2. The module's default package selection attempts to load a Spore packaging file from the Symthaea input. That path did not resolve on the public Symthaea main branch inspected for this RFC. An explicit package override or a corrected pinned packaging path may avoid it, but default evaluation must be proven rather than assumed.
3. The extracted renderer package initially inherited workspace dependencies and pointed at a missing per-crate lockfile. This branch makes the renderer crate standalone, adds a lockfile derived from the exact renderer's upstream locked dependency graph, and changes the Nix derivation to run all targets. **These checks have not yet executed here.** The public repository's root workspace also includes spore-kernel, whose Cargo manifest references Symthaea path dependencies that are absent from this extraction; root-wide workspace builds remain unqualified.
4. The original renderer held a white convergence frame and then cleared abruptly to black. This branch replaces the full-screen flash with theme-colored convergence, a bounded per-pixel exit fade, and a 400 ms scene-entry crossfade that avoids switching from the intro to a fully drawn scene in one frame. A deterministic temporal audit records frame-to-frame luminance/channel changes and guards against large-area high-change regressions. The progress FIFO now uses nonblocking fixed-size reads, a 512-byte record limit, and a per-poll budget so a missing or hostile writer cannot stall early boot. These changes remain unverified until tests run successfully on the exact reviewed head.

These are observed source-level findings, not claims that every consumer configuration fails. The stateful boot module and the complete extracted workspace remain deployment blockers until their package and interface boundaries are repaired.

## The paradigm shift: one Visual Genome, multiple backends

Treat a theme as **a bounded, versioned visual specification**, not a collection of unrelated hard-coded effects. It describes the scene's palette, topology, motion, semantic meaning, privacy class, accessibility behaviour, and resource envelope. Backend adapters render that specification appropriately for each surface.

The contract should have these layers:

1. **Theme data:** stable IDs, validated palette values, motion curves, density limits, accessibility defaults, and allowed event classes.
2. **Pure Rust scene model:** deterministic state transitions and seeded procedural generation. It must be testable without a GPU, display server, user session, or network.
3. **Renderer adapters:** a simple CPU/DRM path for early boot and recovery; a high-quality desktop path using wgpu with its Vulkan backend; and an ordinary static-image path as the universal fallback.
4. **Surface adapters:** KDE Plasma wallpaper integration and lock-screen background integration. Any QML required by Plasma is thin host glue; scene logic, configuration, limits, and tests stay in Rust.
5. **Evidence:** versioned captures, reproducible benchmark settings, frame-time distributions, resource counters, and negative-control tests.

The boot renderer must remain useful without Vulkan. The desktop renderer may use Vulkan compute and fragment shaders for fields, bloom, soft volumetrics, smooth trails, particles, and multiscale topology, but GPU failure must downgrade to the CPU or static theme without affecting boot, login, lock, or recovery.

## Ten initial themes

Theme names should describe a visual mood and information design—not make unverified claims about the machine's state.

| ID | Art direction | Visual language | Best use |
|---|---|---|---|
| mycelial-genesis | Living network awakening | Filament growth, branching nodes, soft leaf light | Signature boot and wallpaper |
| solar-commons | Solarpunk abundance | Warm gold, living green, terracotta, slow radiance | Mycelix community / daily desktop |
| cosmic-atlas | Deep-space cartography | Star fields, orbital arcs, nebula layers, sparse labels | Sol-Atlas / research desktop |
| oceanic-intelligence | Bioluminescent ocean | Fluid fields, caustic light, drifting plankton-like particles | Relaxed animated wallpaper |
| prism-of-evidence | Epistemic lattice | Clean graph edges, provenance paths, spectral but restrained highlights | Symthaea research / developer session |
| quantum-garden | Abstract possibility field | Interference-like waves and branching phase patterns, explicitly artistic | Experimental desktop |
| aurora-sanctuary | Quiet atmospheric | Slow aurora sheets, low contrast motion, dark resting areas | Low-power and reduced-motion defaults |
| graphite-focus | Minimal precision | Charcoal, fine line-work, tiny controlled accents, almost static | Coding and productivity |
| verdant-recovery | Calm resilience | Muted green-blue, simple concentric growth, no bright flash | Recovery, rollback, troubleshooting |
| civic-dawn | Shared flourishing | Multiple organic clusters linked without hierarchy, warm horizon light | Mycelix / Commons identity |

Current implementation status: all ten IDs have a palette, bounded growth profile, and distinct CPU scene family, with deterministic capture and hash support in the benchmark. The JSON manifest is aligned design data but is not yet the runtime source of truth; generated captures still require human quality review.

## Three distinct experience contracts

### 1. Boot sequence: concise, factual, fail-open

Use a cinematic but restrained sequence: dark substrate, a small signal, bounded network growth, a short convergence, and a gentle dissolve into the display manager. It must not turn boot into a splash screen that can hold login hostage.

- Show only milestones that are actually observed and safe to disclose. Never invent progress percentages or imply a service is healthy merely because an animation phase finished.
- Keep network access at zero and read no personal or Mycelix identity data. If a seed is needed, use a public/non-secret installation visual ID or a user-selected seed—not a secret phrase exposed in process arguments.
- No full-screen white flash. Support reduced motion, low luminance, and a static fallback.
- Bound service start and stop time; test GPU/display-manager handoff and missing DRM devices. Animation failure must not be a boot failure.
- Never make an animation or visual asset part of the recovery path's required dependency set.

### 2. Desktop wallpaper: high-fidelity, resource-aware

Use wgpu with Vulkan as the first high-quality renderer, not raw Vulkan as a first step. It gives a Rust-oriented abstraction while preserving a Vulkan-capable backend; measure before considering an ash/raw-Vulkan implementation. Avoid adopting a full game engine solely to draw a desktop background unless its lifecycle integration proves simpler.

- Use GPU shaders for glow, multi-scale density fields, particles, and efficient compositing; keep simulation inputs deterministic and bounded.
- Provide quality tiers: static, low-power, balanced, and cinematic.
- Pause or reduce work when covered, minimized, on battery, under thermal pressure, or when reduced motion is requested.
- Do not subscribe to raw journals, filenames, keystrokes, or personal activity to drive effects. A later live-data visualization must use explicit opt-in, an allowlisted event schema, aggregation, and no identity-bearing data by default.

### 3. Lock screen: privacy before spectacle

Integrate as a background renderer behind the desktop environment's existing lock screen; do not implement a replacement authentication or input path.

- No notifications, names, conversation text, trust scores, file paths, presence, or private network state.
- No network access, no click-through, no input capture, and no authentication logic.
- If the renderer crashes or times out, the desktop lock screen stays opaque and secure.
- Provide a static dark fallback, readable system prompts, reduced motion, and no rapid/high-contrast transitions.
- Test screenshots of the locked display to confirm that private desktop content never peeks through during transitions.

## Graphics plan

### Tier 0 — correctness and authored look
Keep a CPU reference renderer for boot and snapshot tests. Add a typed Theme enum and Palette, then move all hard-coded colors and convergence effects behind it. Keep algorithms bounded and testable without hardware.

### Tier 1 — shared scene model
Separate simulation from pixels: scene time, theme palette, event input, and output primitives are independent. Deterministic seeds and fixed time steps enable the same scene to be reviewed on CPU and GPU.

### Tier 2 — desktop GPU
Use wgpu/Vulkan for a desktop-only renderer. Start with a full-screen shader and a bounded storage buffer for network edges/particles. Prefer instancing and coarse field textures over CPU-generated thousands of individual line rasterizations. Add antialiasing, subtle bloom, layered depth, and smooth trails only within performance budgets.

### Tier 3 — coherent ecosystem
Use the same theme IDs and palette semantics across boot, desktop, lock screen, website demos, and optional Mycelix visualisations. Each surface gets an adapter and its own permissions; the shared visual language does not imply shared access to private data.

### Tier 4 — adaptive visual computing
Adapt quality to the available frame budget, power mode, temperature, refresh rate, and visibility. Adaptation should affect graphical fidelity—not truthfulness, security policy, or the reported state of the system. Deterministic capture mode freezes the adaptive controller for tests.

## Non-negotiable boundaries

- Themes are declarative, schema-validated data; no arbitrary executable theme scripts.
- Boot and lock-screen renderers never fetch remote assets or use network-driven shaders.
- User-level wallpaper processes run without elevated privileges. A boot renderer's privileged DRM access is the exception and must stay tiny, bounded, and separately audited.
- A theme cannot change boot order, recovery decisions, authorization, or Mycelix trust rules.
- No secret in a command-line argument. Use a file descriptor or a narrowly permissioned file if a secret is ever genuinely necessary; for visuals, it should not be.
- Static image mode must be available everywhere.
- Generated imagery and status indications must be distinguishable from factual telemetry. The scene may be symbolic; any claimed system status must be evidence-backed.

## Initial acceptance gates

The first theme-engine PR is ready to merge only after:

1. The CLI rejects unknown theme IDs; manifest validation checks schema version, unique IDs, bounded growth values, and Rust/JSON ID parity. When the JSON becomes runtime input, malformed colors, durations, density, and schema versions must be rejected before rendering.
2. Same renderer build + theme version + seed + fixed time inputs produce identical CPU capture hashes.
3. Every theme has a capture in the same resolution, timepoint, and color-management profile.
4. Reduced-motion mode changes the actual animation path, not merely a label.
5. Manifest shader variants are unique and cover the full 0–9 ABI; GPU receipts must report the declared variant for each named scene family.
5. The end sequence has no full-screen white flash and fades to black continuously.
6. Renderer failure, missing DRM, SIGTERM, malformed theme, and missing optional assets all take a bounded fail-open path.
7. Boot service/package/CLI contracts agree exactly and are tested against the selected pinned package.
8. Benchmarks report measured distributions from a named machine and driver. Queued, absent, or mismatched-head CI is not a pass.

## References

- [Sovereign Boot module](../nix/modules/sovereign-boot.nix)
- [Framebuffer renderer entrypoint](../crates/quicken-fb/src/main.rs)
- [Procedural mycelium model](../crates/quicken-fb/src/mycelium.rs)
- [Current palette](../crates/quicken-fb/src/color.rs)
- [wgpu documentation](https://wgpu.rs/)
- [Vulkan Guide](https://docs.vulkan.org/guide/latest/)
