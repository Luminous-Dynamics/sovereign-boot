# Ouroboros industry performance and energy benchmark

**Status:** benchmark design and versioned workload manifest only. No comparative host trials have been run.

Tracking: [Issue #38](https://github.com/Luminous-Dynamics/sovereign-boot/issues/38)  
Source baseline: PR #15 head `d5a7f877d71757db49e63ad1b1582a661f6e4ef2`  
Workload manifest: [`workloads-v1.json`](workloads-v1.json)

## Industry snapshot as of 2026-11-10

The desktop wallpaper category has moved quickly. Our comparison should reflect actual late-2026 capabilities, not stale competitor assumptions.

| Runtime | Documented strengths | Where Ouroboros should compare |
|---|---|---|
| Waywallen 0.6.0 (8 Nov 2026) | Dynamic wallpaper manager, plugin discovery, hardware video, glTF scene rendering, per-display management and integrations across KDE/GNOME/wlroots/COSMIC | Desktop integration, display lifecycle, glTF path, multi-output and presentation overhead |
| Fresco 1.1.53 (28 Oct 2026) | Video/GIF/image/slideshow/playlist, VA-API/NVDEC, power-saving, pause policies, multi-monitor and widgets | Hardware decode, marginal energy, hidden/visible power-state behavior |
| OpenWallpaper | WASM programmable scenes, Wallpaper Engine scene conversion, media, multi-monitor and auto-pause; auto-pause documented as desktop-environment-dependent | Programmable scene startup, WASM portability, scene runtime and pause behavior |
| Wallpaper Engine KDE active continuation | Plasma 6/Qt 6 scene/web/video playback with a custom Vulkan scene renderer | 3D scene rendering, memory and presentation cadence on a matched asset |
| Ouroboros | Persistent semantic entity, C1 morphing, portable checkpoints, semantic landmarks and content-addressed evidence; CPU reference renderer only | Cross-form identity, state portability, semantic LOD and proof-bearing presentation |

Sources:
- [Waywallen 0.6.0 release history](https://flathubupdates.popey.com/app/org.waywallen.waywallen) and [upstream repository](https://github.com/waywallen/waywallen)
- [Waywallen display adapter](https://github.com/waywallen/waywallen-display)
- [Fresco implementation and performance section](https://github.com/DibbayajyotiRoy/Fresco)
- [OpenWallpaper](https://openwallpaper.org/)
- [Wallpaper Engine KDE active continuation](https://github.com/CaptSilver/wallpaper-engine-kde-plugin)

Pin exact release commits and driver/runtime versions at test time. Feature descriptions change; this snapshot is not a compatibility qualification.

## External power reference: Fresco

Fresco documents measurements by an external contributor using `turbostat` on an Intel N150, Deepin 25 and VA-API, with two runs per setting:

| Video | Power mode | GPU power | Total package power |
|---|---|---:|---:|
| 1080p60 | Full quality | 1.37 W | 6.00 W |
| 1080p60 | Reduced (default) | 0.63 W | 4.03 W |
| 4K60 | Full quality | 2.77 W | 7.94 W |
| 4K60 | Reduced (default) | 1.60 W | 5.95 W |
| 4K60 | Minimum | 0.99 W | 4.97 W |

Fresco explicitly caveats that:
- These are package-power measurements while playing, not marginal wallpaper cost, because there was no same-run idle baseline.
- The data is from one Intel N150 system and two runs per level.
- Hardware decode moves work to a video engine; it does not make playback free.

Therefore these numbers are a useful external reference but **not** an absolute target for another host. Our suite must measure same-host active-minus-idle marginal energy, repeat more trials and preserve meter uncertainty.

## Why this suite is different

Do not optimize or rank by average FPS, process CPU percentage or renderer submission counts alone.

Our primary resource metric should be:

`joules per compositor-presented frame`

with an honest paired idle baseline and a separate visual/identity quality constraint.

A 60 FPS renderer that submits 60 frames but has 20 discarded frames is not a 60-presented-FPS result. A low CPU figure that simply moves work to a GPU/video-engine is not an energy result. A visually degraded quality setting is not an efficiency win unless the perceptual/semantic constraint is still met.

Publish Pareto frontiers for:
- actual presentation smoothness;
- marginal energy and joules per presented frame;
- CPU/GPU/video memory;
- geometry and semantic error;
- multi-output scaling;
- startup, pause/resume and lifecycle reliability.

Do not publish a single weighted leaderboard score.

## Matched workloads

The JSON manifest defines W0–W7:
- static compositor baseline;
- same video bitstreams at 1080p60 and 4K60;
- deterministic 2D parallax;
- seeded particle/shader scene;
- same glTF animated character;
- Ouroboros persistent identity, C1 retarget and checkpoint workload;
- hidden/occluded/resume states;
- multi-output mixed-refresh and hotplug.

Only compare feature intersections. If a product does not support a workload, record `unsupported` with a reason. Do not substitute a different scene and call it matched.

Every asset must be frozen and SHA-256 hashed before trial runs. Scene parameters, camera, lighting, animation/event trace, color space, output mode and refresh rate must be identical.

## Method

- Warm up for 5 minutes; measure for 15 minutes.
- Run at least 10 paired trials per setting initially.
- Randomize or use ABBA paired order to reduce thermal/background drift.
- Capture an idle baseline on the same host immediately before/after active trials.
- Use a wall power meter for whole-system power; use RAPL/turbostat and GPU counters only for documented, non-overlapping domains.
- Record compositor feedback for presentation cadence when available; distinguish Unknown/unsupported from Presented.
- Record raw samples at 10 Hz where the instrument supports it.
- Report median, p50/p95/p99/p99.9 and confidence intervals across trials. Preserve each raw trial.
- Fix and record compositor, kernel, GPU/driver, firmware, output mode, refresh/VRR, power profile, temperature and background load.
- Separate presentation feedback from renderer/host timing and separate human perception from image similarity.

## Proposed first acceptance targets

These are targets to qualify, not achieved results:

| Area | Initial target |
|---|---|
| Resource and geometry budget violations | 0 |
| Stale session/pool/decision receipt accepted | 0 |
| 1080p60 and 4K60 | p99 presentation interval within the declared cadence while workload remains inside its negotiated budget |
| 120 Hz | Separate 8.33 ms qualification profile |
| Hidden/occluded transition | Enter explicit low-power/paused state within 1 s |
| Resume | Valid presented frame within 250 ms after a defined host wake/surface-ready point |
| Hidden-state marginal power | Statistically indistinguishable from same-host static baseline within instrument resolution |
| Internal energy improvement | ≥20% lower joules per presented frame versus Ouroboros' own unoptimized reference on one matched workload, without violating identity/visual quality bounds |
| Blinded cross-form recognition | ≥90%, with a pre-registered protocol and confidence interval |
| Checkpoint continuation | 0 semantic-state divergence inside the declared same-build compatibility contract |

Do not set a universal absolute watt goal from Fresco's Intel N150 result. Qualify each host class separately.

## How to beat the category

The opportunity is not to clone every wallpaper-management feature. Waywallen and the KDE/Fresco/OpenWallpaper ecosystem already cover substantial integration and playback breadth.

Ouroboros should aim to be the first *evidence-bearing persistent visual entity runtime*:

1. Entity identity survives renderer/process/device changes.
2. Quality negotiation preserves semantic identity under hard budgets.
3. LOD reduces non-essential detail before identity-critical features.
4. Presentation feedback distinguishes rendered/submitted/presented/discarded.
5. The runtime learns a host's actual cost envelope without changing hard constraints.
6. Checkpoints and visual evidence make cross-renderer continuity independently reproducible.

The last point is our intended differentiator. It is not yet an industry-leading result.

## Non-claims

No industry comparison has been executed. No claim is made that Ouroboros is more efficient, faster, more visually compelling or more portable than Waywallen, Fresco, OpenWallpaper or Wallpaper Engine. The competitor data above is source-reported and must be independently reproduced under matched host/workload conditions.
