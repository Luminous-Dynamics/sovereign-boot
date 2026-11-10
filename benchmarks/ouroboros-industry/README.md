# Ouroboros industry performance and energy benchmark

**Status:** benchmark design and versioned workload manifest only. No comparative host trials have been run.

Tracking: [Issue #38](https://github.com/Luminous-Dynamics/sovereign-boot/issues/38)  
Source baseline: PR #15 head `d5a7f877d71757db49e63ad1b1582a661f6e4ef2`  
Workload manifest: [`workloads-v1.json`](workloads-v1.json)

## Industry snapshot as of 2026-11-10 (late-day release refresh)

The desktop wallpaper category has moved quickly. Our comparison should reflect actual late-2026 capabilities, not stale competitor assumptions.

| Runtime | Documented strengths | Where Ouroboros should compare |
|---|---|---|
| Waywallen 0.6.0 (upstream 8 Nov; Flathub package updated 9 Nov 2026) | Dynamic wallpaper manager, plugin discovery, hardware video, glTF scene rendering, per-display management and integrations across KDE/GNOME/wlroots/COSMIC | Desktop integration, display lifecycle, glTF path, multi-output and presentation overhead |
| Fresco 1.1.54 (1 Nov 2026; latest release found) | Video/GIF/image/slideshow/playlist, VA-API/NVDEC, power-saving, pause policies, multi-monitor and widgets; 1.1.54 fixed GNOME X11 wallpaper window stacking and relaxed several Deepin package dependencies | Hardware decode, marginal energy, hidden/visible power-state behavior |
| OpenWallpaper | WASM programmable scenes, Wallpaper Engine scene conversion, media, multi-monitor and auto-pause; auto-pause documented as desktop-environment-dependent | Programmable scene startup, WASM portability, scene runtime and pause behavior |
| Wallpaper Engine KDE active continuation | Plasma 6/Qt 6 scene/web/video playback with a custom Vulkan scene renderer | 3D scene rendering, memory and presentation cadence on a matched asset |
| Ouroboros | Persistent semantic entity, C1 morphing, portable checkpoints, semantic landmarks and content-addressed evidence; CPU reference renderer only | Cross-form identity, state portability, semantic LOD and proof-bearing presentation |

Sources:
- [Waywallen 0.6.0 release history and Flathub publication](https://flathubupdates.popey.com/app/org.waywallen.waywallen) and [upstream repository](https://github.com/waywallen/waywallen)
- [Waywallen display adapter](https://github.com/waywallen/waywallen-display)
- [Fresco 1.1.54 release history](https://github.com/DibbayajyotiRoy/Fresco/releases) and [implementation/performance section](https://github.com/DibbayajyotiRoy/Fresco)
- [OpenWallpaper](https://openwallpaper.org/)
- [Wallpaper Engine KDE active continuation](https://github.com/CaptSilver/wallpaper-engine-kde-plugin)

Pin exact release commits and driver/runtime versions at test time. Feature descriptions change; this snapshot is not a compatibility qualification.

## External power reference: Fresco

Fresco 1.1.53's performance section documents measurements by an external contributor using `turbostat` on an Intel N150, Deepin 25 and VA-API, with two runs per setting. Fresco 1.1.54 (released 1 Nov 2026) is newer; the measurement table below is still the 1.1.53-published reference, not a claim that 1.1.54 re-ran the experiment:

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

## Raw-trial schema and paired energy analyzer

The suite now includes an executable, standard-library-only analysis path:

- `trial-v1.schema.json`: one complete trial record per JSONL line. It binds the system/build, workload/assets, host fingerprint, warm-up/active/idle protocol, presentation outcome provenance and timestamped power samples.
- `analyze-energy.py`: validates trial provenance and computes paired active-minus-idle marginal energy, joules per actually presented frame, conservative meter uncertainty, per-domain summaries, bootstrap intervals across repeated trials, and matched A/B differences for shared pairing-block IDs.

Run the analyzer's deterministic synthetic self-test:

```sh
python benchmarks/ouroboros-industry/analyze-energy.py --self-test
```

This self-test uses **synthetic fixture values only** and prints a message explicitly saying it is not a measured result.

Analyze real frozen trials:

```sh
python benchmarks/ouroboros-industry/analyze-energy.py trials-v1.jsonl \
  --min-trials 10 --bootstrap-samples 2000 \
  --output energy-analysis-v1.json
```

### Energy accounting

For the `wall_total` domain, the analyzer calculates:

`marginal_energy_j = integral(active_wall_power - interpolated_idle_baseline)`

The idle baseline interpolates between the means of the same trial's `idle_before` and `idle_after` phases. It integrates the active phase using trapezoidal integration. Per-sample meter uncertainty is propagated as a conservative bound; instrument calibration and other systematic uncertainty still need to be documented in the raw trial notes.

`joules_per_presented_frame = marginal_energy_j / compositor_or_host_evidenced_presented_frames`

Renderer-reported or unknown provenance cannot justify a nonzero presented-frame count. When presentation is unavailable, J/presented-frame remains null and unknown outcomes are reported separately. Discarded frames are never counted as presented.

CPU package, CPU core, GPU, video-engine and VRAM readings are summarized separately. They are **not added** to `wall_total`, because their measurement boundaries can overlap. The raw samples remain in the JSONL input and output metadata identifies the instrument/domain boundary.

The analyzer defaults to 10 repeated trials before marking a group `meets_initial_trial_count`; smaller groups remain `insufficient_trials`. Bootstrap intervals describe run-to-run variation for the sampled trials and do not replace calibration uncertainty, randomized paired design or an external wall meter.

The trial schema is a data contract, not a proof that the measurement instrument is correctly installed. Preserve the raw JSONL, meter calibration record, asset hashes, source revisions and instrument setup with every published result.

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
