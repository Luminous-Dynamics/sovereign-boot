# Visual Renderer Benchmarks and Acceptance Targets

**Status:** Proposed targets, not measured results.
**Rule:** Record the baseline first. Never replace measured values with targets, and never report a queued or absent run as a pass.

## Measurement protocol

Every result must record:

- exact repository, commit SHA, dirty-tree state, theme-manifest version, renderer, and build profile;
- NixOS/kernel, compositor, GPU model, driver version, power profile, display resolution, refresh rate, and color profile;
- run duration, warm-up duration, sample count, median, p95, p99, maximum, and failure count;
- power-measurement source and methodology, where power is reported;
- raw trace or capture artifacts with hashes, plus the command needed to reproduce them.

Run at least five repetitions for performance measurements. A benchmark without the environment tuple and raw data is informational only.

## Targets

These are engineering targets for qualification, not claims about the current renderer.

| Area | Initial target | How to measure | Failure policy |
|---|---|---|---|
| CPU/DRM renderer, 1920×1080 | Sustain 30 FPS; p95 frame time ≤33.3 ms | Render loop timings and CPU profile | Degrade density; never delay boot |
| CPU/DRM renderer memory | ≤96 MiB peak RSS at 1920×1080 | Peak RSS under bounded maximum scene | Abort renderer safely, not boot |
| Desktop renderer, 1920×1080 | 60 FPS on the reference RTX 2070 Laptop GPU; p95 ≤16.7 ms and p99 ≤25 ms | Present timestamps + GPU/CPU traces | Lower quality tier automatically |
| Desktop renderer, 3840×2160 | At least 30 FPS; p95 ≤33.3 ms | Same protocol at 4K | Fall back to balanced/static mode |
| Desktop renderer GPU memory | ≤256 MiB attributable to the wallpaper process at 1080p | Driver process counters; include measurement caveats | Disable bloom/particles, then static |
| Desktop renderer CPU | ≤2% of one logical core at steady-state for a low-motion scene | 5-minute interval with wallpaper visible | Reduce simulation and particle update rate |
| Battery impact | ≤2 W incremental average in low-power mode, if hardware telemetry supports a valid comparison | Matched 30-minute A/B run: same brightness, scene, and workload | Low-power tier or pause animation |
| Theme switching | New first frame ≤250 ms after selection; no blank uncovered desktop | Timestamped repeated switching | Keep previous frame/static fallback |
| Display-manager handoff | Renderer releases DRM within 750 ms of stop request | VM/systemd timing plus hardware canary | Boot qualification blocked |
| Boot impact | No dependency on renderer success; service failure never prevents login or recovery | Inject missing device, crash, timeout, malformed state | Any login/recovery dependency is a blocker |
| Shutdown transition | Continuous theme-specific fade; no single full-screen white frame or abrupt white-to-black cut | Capture at ≥60 samples/s; inspect luminance series | Accessibility gate fails |
| CPU determinism | Identical capture hash for same build, theme, seed, fixed inputs, and resolution | Golden framebuffer test | Explain intentional versioned differences |
| Cross-backend appearance | SSIM ≥0.99 for approved reference scenes, after documented color conversion | CPU reference vs GPU output | Perceptual mismatch must be reviewed |
| Privacy | Zero network requests and zero identity-bearing values in captured lock screens | Network namespace trace + screenshot scan/review | Any leak blocks release |
| Resource bounds | Manifest limits are enforced even for malformed data | Property/fuzz tests | Reject invalid manifest, don't try to render |
| Accessibility | Reduced-motion path is effectively static; no rapid or full-frame flashing | Frame-difference analysis + manual review | Disable animated mode by default until fixed |
| Temporal scene audit | Zero sampled single-frame high-change transitions and zero opposing-transition triplets affecting ≥25% of the sampled raster; one-second rolling windows report no more than three large-area candidate events | Rust `motion_audit` plus `opposing_transition_audit` at fixed 30 Hz simulation steps; retain transition traces, rolling-window summary, and hashes | Fail the regression gate and review captures. The ≥25% proxy is relative to the whole sampled raster, not a 10° visual-field calculation. These are CPU-render regression checks, not WCAG conformance or physical-display measurements |

The RTX 2070 Laptop GPU is the initial desktop reference only if the machine is available for local measurement. Do not claim that target passed from CI on a different device. GPU telemetry is vendor-dependent; when unavailable, label power and utilization unmeasured instead of estimating them.

## GPU CI throughput baseline

In addition to the ten low-resolution theme-identity captures, CI runs a 60-pass, 320×180 offscreen Vulkan workload on the installed Mesa software adapter. The receipt reports CPU command recording/submission, end-to-end readback, setup, and optional timestamp-query distributions with both the reviewed source-head SHA and the actual build SHA. This is a workload/receipt integration baseline, not a 1080p frame-rate claim and not a discrete-GPU measurement. Use the same command at increasing resolutions on the target GPU before applying the proposed desktop targets.

## Shader complexity and specialization

The offscreen WGSL reference shader uses ten manifest-owned scene-family IDs. The fragment shader computes fields inside the branch that consumes them: geometric scenes skip the generic multi-octave FBM substrate, while mycelial, cosmic, lattice, quantum and aurora families compute only their needed warped fields. This should reduce unused per-pixel work; a speedup must still be measured, not inferred from source inspection.

Qualification is an A/B capture and timestamp exercise first:

- Preserve an exact capture and hash for every family on the same software Vulkan adapter.
- Collect GPU timestamp samples where supported; keep CPU encode/submit and end-to-end readback time separate.
- Compare before/after on the same adapter, driver, resolution, power policy and frame count; do not infer a speedup across different CI machines or from CPU submission time.
- Proposed target: at least 20% p95 reduction for geometric families that no longer compute FBM, with exact pixel equality preferred where equations remain equivalent. The target is aspirational until same-adapter measurements exist.
- Do not use software-adapter timing as a product performance guarantee. Preserve it as a regression signal and use negative-control captures to catch blank or aliased output.

## Shader complexity and specialization A/B experiment

A benchmark-only fixture preserves the exact shader from before the family-work specialization. The offscreen reference CLI can run either `baseline` or `specialized` with the same theme, neutral palette, scene time, resolution, backend, and pass count. CI requires exact pixel equality for all ten families and records per-family GPU timestamp distributions when the adapter supports them. The report computes a p95 reduction only when both runs supply 60 valid GPU timestamp samples; otherwise the value is null. CPU submit and end-to-end readback times remain separate, and CI does not claim a speedup merely because a number is available.

## Required benchmark workloads

1. **Cold boot:** renderer enabled, renderer disabled, missing DRM device, renderer crash, repeated service restart, display-manager handoff, and safe-mode/recovery path.
2. **Scene complexity:** all ten CPU scenes at 640×360 smoke, 1080p, and 4K; minimum, median, and maximum branch density; steady state and peak animation. Same-seed fixed-step reruns must produce identical framebuffer hashes and PPM captures.
3. **Lifetime:** 60 minutes of visible rendering, 60 minutes covered/hidden, sleep/resume, display reconnect, and theme switching.
4. **Privacy:** locked display while notifications, activity changes, and Mycelix events are present; confirm no private content is captured or surfaced.
5. **Reproducibility:** regenerate all golden captures from a clean checkout and the pinned Nix build.
6. **Adversarial inputs:** unknown theme IDs, invalid color values, NaN/out-of-range motion factors, giant density values, unsupported schema versions, missing receipts, and malformed optional assets.

## Capture integrity: structure before aesthetics

The CPU benchmark emits a `visual_quality` diagnostic alongside each framebuffer hash: the count of occupied 4-bit-per-channel RGB bins (bounded to 4,096), minimum/maximum/mean luminance, luminance standard deviation, and mean adjacent-pixel luminance delta. CI requires at least two quantized color bins and nonzero luminance range/variation for every theme in both Cinematic and Reduced mode. This catches uniform/blank-output regressions that a deterministic hash alone cannot catch.

These values are **structural diagnostics, not a perceptual-quality score**. They do not measure semantic composition, accessibility, color-vision suitability, artistic coherence, or motion comfort. Preserve the image artifacts and perform human review; later GPU qualification should use color-managed CPU/GPU captures plus documented perceptual comparison rather than exact cross-backend hashes.

Motion safety needs its own temporal audit. The W3C's [WCAG 2.2 Success Criterion 2.3.1](https://www.w3.org/WAI/WCAG22/Understanding/three-flashes-or-below-threshold.html) gives a useful conservative reference for flash frequency, and its [reduced-motion technique](https://www.w3.org/WAI/WCAG22/Techniques/css/C39) illustrates how an explicit user preference should change non-essential animation. These are design references here, not a claim that a desktop wallpaper is itself a conforming web page. Final-frame hashes cannot establish flash safety; release qualification still needs a time-sequenced luminance/area audit and visual review.

The in-branch Rust tool samples the CPU reference scene at a fixed 30 Hz simulation interval and records per-transition mean/max relative-luminance delta, changed-pixel ratio, high-change area ratio, and a sequence hash:

```bash
bench_dir="$(mktemp -d)"
trap 'rm -rf "$bench_dir"' EXIT
mkdir -p "$bench_dir/quicken-fb"
cp -a crates/quicken-fb/. "$bench_dir/quicken-fb/"
cargo run --manifest-path "$bench_dir/quicken-fb/Cargo.toml" --locked --release --example motion_audit -- \
  --theme oceanic-intelligence --motion cinematic --frames 180 \
  --width 160 --height 90 > /tmp/ocean-motion-audit.json
```

Run the paired-transition proxy in the same isolated package:

```bash
bench_dir="$(mktemp -d)"
trap 'rm -rf "$bench_dir"' EXIT
mkdir -p "$bench_dir/quicken-fb"
cp -a crates/quicken-fb/. "$bench_dir/quicken-fb/"
cargo run --manifest-path "$bench_dir/quicken-fb/Cargo.toml" --locked --release --example opposing_transition_audit -- \
  --theme oceanic-intelligence --motion cinematic --frames 180 \
  --width 160 --height 90 > /tmp/ocean-opposing-audit.json
```

CI runs it across all ten themes in both Reduced and Cinematic modes. Reduced must produce 59 unchanged transitions in a 60-frame trace; Cinematic must show motion; repeated sequence hashes must match; and no sampled transition may have a high-change condition covering 25% or more of the sampled frame. This 25% full-raster threshold is a deliberately simple **engineering alarm**, not a conservative substitute for the WCAG flash test: its spatial calculation uses a 10-degree visual-field region, not the whole raster. WCAG's general-flash evaluation involves opposing transitions, frequency windows, a defined visual-field area and relative-luminance conditions; saturated-red flashes need a separate chromaticity analysis. The auditor now also computes every complete rolling one-second window over 31 frame-triplet sample timestamps (30 Hz, including both endpoints). It reports the maximum number of sampled large-area candidate events in any such window, and how many windows exceed three events, separately for general-luminance and saturated-red candidates. CI keeps the stricter per-triplet zero-large-area-event gate and also validates the rolling-window summary schema. A shorter-than-one-second trace reports zero complete windows rather than implying coverage that was not sampled.

The current renderer-side auditor still does not implement the complete normative spatial/frequency procedure and does not capture the physical display. Its 25% gate measures the full low-resolution raster, not the WCAG-defined 10-degree visual-field area; sampling at 30 Hz may also miss faster display behaviour. Therefore a passing receipt is a regression screen, never a compliance certificate.

### Independent audit of captured frames

Each manifest theme now has a stable `shader_variant` integer (0–9) beside its `scene_family`. The GPU host consumes this declared value directly; CI validates full coverage and uniqueness, while unit tests pin the mapping between named families and shader ABI. Keep this mapping versioned: reassigning an index changes rendered scene semantics and requires new golden captures.

The separate `validation/frame-audit` utility consumes ordered external P6 PPM frames and deliberately imports no scene-renderer code. It therefore can check decoded GPU captures or frames extracted from a screen recording without trusting the renderer's own metrics. It retains the previous frame and compact per-pixel last-transition metadata rather than storing the whole video. The two event masks are preallocated once and reused across frames to avoid a pair of full-raster heap allocations on every sample, and pairs significant opposing luminance or saturated-red edges across non-adjacent frames when their separation is no more than one second. A bright plateau lasting several sampled frames is no longer silently ignored.

For a captured video, extract evenly spaced frames without resizing and run the independent check:

```bash
ffmpeg -i display-capture.mp4 -vf fps=30 /tmp/visual-frame-%05d.ppm
cargo test --manifest-path validation/frame-audit/Cargo.toml --locked --all-targets
cargo run --manifest-path validation/frame-audit/Cargo.toml --locked --release -- \
  --fps 30 /tmp/visual-frame-*.ppm > /tmp/captured-frame-audit.json
```

The audit bounds each frame to UHD 4K and the total workload to 500 million sampled pixels. It requires equal frame dimensions and emits a v3 JSON receipt with the assumed sampling rate, transition metrics, rolling one-second windows, and a non-normative flag. Video frame duplication, dropped samples, compression, color management, display capture and scaling can all affect results. Use the actual spacing of extracted samples; the video's nominal frame rate is not necessarily the monitor refresh rate. Preserve the original video, extraction command, capture/display details, frame hashes and receipt together.

The spatial screen reports the maximum candidate-flash ratio in a sliding local raster window scaled to approximately one third of the frame width and height. This resembles the W3C's published 341×256 approximation at 1024×768 rather than dividing by full-screen area alone. Tests assert that reference scale and include a concentrated four-pixel patch that is below 25% of a 12×12 raster but reaches 25% inside a 4×4 local window. Per-pixel history uses compact fixed-size state (4 bytes for luminance edges and 12 bytes for red-colour edges); tests pin these layouts to prevent an accidental jump in audit memory usage.

**This remains a regression proxy, not WCAG conformance.** The scaled raster window is not the actual physical 10-degree visual field for every monitor/viewing distance; sampling can miss events between frames; and red-colour handling is conservative, not independently certified. Use a validated analyser and actual display captures for any stronger claim.

## Timestamp semantics and evidence completeness

The external frame auditor now has two explicitly different timing modes:

- **Legacy nominal-FPS mode:** schema captured-frame-audit-v3; frame-index spacing is interpreted using the declared FPS. Keep this path for existing reproducible fixtures.
- **Timestamp-sidecar mode:** schema captured-frame-audit-v4-timestamped; each input frame has exactly one strictly increasing unsigned integer timestamp in nanoseconds. Opposing-edge pairing and rolling one-second event windows use elapsed timestamp time. The source/build SHAs remain embedded in the receipt.

The CLI takes the sidecar with --timestamps timestamps-ns.txt. --max-gap-ms declares a maximum allowed adjacent sample gap (default 100 ms). A trace with a larger gap emits coverage_status: incomplete_sampling_due_to_gap; its observed event counts must not be interpreted as a complete scan of the unobserved interval. This threshold is an engineering coverage alarm, not proof that every shorter interval was adequately sampled. Both timestamp and legacy modes continue to report normative_wcag_conformance: false.

### New qualification targets

| Check | Acceptance target | Failure behavior |
|---|---|---|
| Timestamp format | Exactly one timestamp per frame; integer nanoseconds; no negative, duplicate, decreasing, malformed, missing, or extra entries | Reject the audit with non-zero exit |
| Pair timing | Opposing edges ≤1,000,000,000 ns apart can pair; edges beyond one second cannot pair | Deterministic boundary fixtures fail on any divergence |
| Rolling windows | Event counts use any observed one-second timestamp interval, including irregular sample spacing | Positive/negative boundary fixtures must match expected counts |
| Gap reporting | Every sequence with a gap above --max-gap-ms is labeled incomplete_sampling_due_to_gap | Incomplete data may never be represented as complete coverage |
| Determinism | Identical pixel bytes, timestamp sidecar, options, and build produce identical audit metrics | Different metrics or event summaries block qualification |
| Provenance | Receipt binds source/build commits; capture manifest binds timestamp-sidecar hash, ordered frame-hash manifest, and receipt hash | Missing or mismatched evidence blocks release |
| Schema compatibility | Legacy FPS receipts remain v3; timestamp receipts use v4; mode must be visible in the receipt | A receipt must never hide which timing basis was used |
| Resource bounds | Retain maximum frame, count, timestamp-file, and sampled-pixel limits; report truncation/gap status explicitly | No partial run can be mistaken for a clean audit |

### Performance baseline before optimization

Measure the timestamped and legacy modes separately on the same named reference machine. Use at least five repetitions and report frame count, resolution, total pixel work, timeline span, max sample gap, elapsed audit time, peak RSS, and p50/p95/p99 per-transition time. Do not compare a timestamped full-resolution capture against a low-resolution nominal-FPS run as if the difference were a code speedup.

Proposed performance targets to validate after collecting the initial baseline:

- 1080p: sustain 30 sampled frames/second with per-transition p95 ≤33.3 ms; peak RSS ≤256 MiB.
- 4K: sustain 10 sampled frames/second as an initial target; peak RSS ≤512 MiB.
- Repeated run: zero audit-metric divergence for identical frame/timestamp inputs.
- Gap negative control: zero cases where a large timestamp gap produces a complete-coverage status.

These are target budgets, not current measured performance results. The current 500-million-pixel workload cap permits only about 241 frames at 1080p and 60 at 4K; longer traces require a separately qualified chunked/resumable mode with state preserved across chunk boundaries. Do not remove the existing cap merely to make the longer-duration target appear satisfied.

## Paradigm shift: qualify a visual system, not an isolated shader

The proposed unit of design is a **visual contract**: intent, composition features, palette, motion envelope, state meaning, resource budget, deterministic seed, reduced-motion representation, and safe fallback. Themes should not be ten names wired to one hidden generic scene. Nor should an attractive screenshot be accepted as proof of good timing, accessibility, power use, or safe lifecycle integration.

Qualify five independent properties instead of collapsing them into a single visual score:

1. **Semantic controllability.** A declared theme/family selector must create its intended composition. Test the ten current families under a neutral palette so colour alone cannot make them appear distinct. Later, collect blinded owner-review ratings of theme identity; start with at least 80% correct family identification and record uncertainty rather than turning that target into a claim about universal perception.
2. **Bounded expressivity.** The same seed and fixed inputs reproduce exact pixels on the same backend; changing only the declared seed changes the scene within its budget; Reduced ignores time and renders its intentional still composition. Do not confuse randomness with creativity or require pixel equality across unlike backends.
3. **Temporal safety as a time-series property.** Keep source PTS, sampling gaps, transition history, and one-second windows in timestamp space. Test both monolithic and segmented analysis against the same corpus. The PPM chunker is a first prototype using overlap/prefix deltas, not a serialized continuation of per-pixel state; it remains unqualified until it matches the monolithic oracle and malformed-input negative controls pass.
4. **Operational isolation.** The desktop may use the GPU; early boot and recovery must not depend on GPU discovery, shader compilation, remote assets, or the decorative process succeeding. Measure first useful frame, stop-to-DRM-release, fallback completion, crash recovery, and privacy separately.
5. **Resource truth.** Report unique source pixels separately from actual processed pixel work (including repeated overlap), compile time separately from audit time, CPU submission separately from GPU execution, and frame rendering separately from presentation. Missing telemetry is null/unmeasured, not an estimate.

### Chunked-evidence acceptance targets

| Gate | Target before qualification | Negative control |
|---|---|---|
| Segment equivalence | Same event counts, per-window summaries, area maxima, and transition count as monolithic analysis for identical ordered frames/timestamps | Put an opposing transition across a segment boundary; deliberately remove/duplicate a chunk or mismatch timestamps |
| Window coverage | Every one-second window is counted once; overlap does not inflate counts | Compare the complete window-summary object to the monolithic oracle |
| Gap semantics | A gap within the configured limit remains within policy; larger gap is explicitly incomplete | Introduce a 300 ms gap with a 150 ms declared limit |
| Input integrity | SHA-256 manifest binds order; re-hash detects source mutation; source/build commits are recorded | Duplicate/backward/missing timestamp and changed input fail closed |
| Work reporting | Report unique pixels, actual pixel-work, redundant overlap, chunk limits, wall time, coverage status | Resource-bound failure never produces a clean zero-event pass |
| Long capture | At least 60 seconds of 1920×1080 at 30 samples/s (1,800 frames), preserving one-second semantics across every boundary | Replay a known flash pattern before/across/after boundaries; all receipts reconcile |
| Memory | Peak RSS ≤256 MiB at 1080p and ≤512 MiB at 4K on a named machine | Oversized frame, byte, frame-count, or pixel-work budget must reject visibly |

Do not treat the 60-second target as achieved by increasing a frame limit. This branch's first chunked PPM implementation re-reads overlapping frames and subtracts prefix counters; it still needs measured equivalence, resource benchmarks, video-input support, and eventually a serializable auditor state if overlap reprocessing is too expensive. Report p50/p95/p99/max and peak RSS over at least five repetitions on a fixed named machine. Keep CI fixtures small and deterministic; put 60-second/4K performance work on a dedicated runner or local qualification machine with raw artifacts and exact SHAs.

### Evidence hierarchy

- **Unit/property tests** prove local math and parser invariants.
- **Synthetic frame corpus** proves known-positive and known-negative temporal behaviour under controlled timestamps.
- **Chunked-versus-monolithic oracle** detects segmentation bugs; it does not establish that the input captured every real display event.
- **Offscreen GPU captures** establish shader compilation, pixel determinism, and same-adapter throughput only.
- **OS/compositor and physical-display captures** establish additional lifecycle/timing facts but still require correct instrumentation and a documented capture chain.
- **Owner visual review** evaluates composition and meaning; structural colour-bin and luminance statistics are only blank/degenerate-image alarms.

Flash-frequency guidance should be read from the [W3C WCAG 2.2 SC 2.3.1 explanation](https://www.w3.org/WAI/WCAG22/Understanding/three-flashes-or-below-threshold.html). Our Rust auditor is deliberately non-normative; a passing receipt is not certification, and a sampling-gap pass is not proof that no unobserved event occurred.
## Visual Genome manifest parity

The JSON manifest is still not loaded at early-boot runtime. To avoid silently divergent duplicate definitions, qualification now asks the typed Rust theme inventory to emit a stable TSV contract and diffs it against the normalized manifest. The parity contract includes all ten stable IDs, scene-family identifiers, all five RGB palette roles, and all nine growth-profile parameters per theme. This is stronger than checking the ID inventory alone, but it is a consistency gate, not yet a true single source of truth.

The next architectural step is build-time generation: make the JSON manifest the human-edited design source, generate boot-safe Rust constants from the bounded schema, and validate the generated artifact deterministically. Early boot must continue to compile palette/geometry constants into the executable; it should not parse JSON, allocate general-purpose schema objects, or access the desktop shader or any network asset. The generator acceptance gate should require: byte-reproducible generation; diff-clean generated code; manifest rejection for unknown fields/schema versions, duplicate IDs, invalid colors and out-of-range budgets; complete typed-contract coverage; and no new early-boot runtime dependency.
## Human-quality evaluation

Use a small, blinded study with identical display hardware and lighting. Ask participants to rank coherence, beauty, legibility, motion comfort, and perceived system-state clarity separately.

Initial goals:

- median aesthetic/coherence rating ≥4/5;
- at least 90% correctly distinguish a factual status indicator from purely decorative motion;
- at least 90% of participants identify the reduced-motion setting and report that it changes the scene;
- no unresolved reports of lock-screen privacy exposure or disorienting full-screen flashes.

Record sample size and questions; these goals are internal design targets, not scientific generalizations.

## Baseline work to execute before optimization

Every benchmark and audit JSON receipt embeds both `source_commit` (the reviewed PR head) and `build_commit` (the actual commit checked out by the runner, which is often GitHub's synthetic merge commit on pull requests). CI injects both SHA values and rejects receipts whose values differ. A local invocation without these environment variables labels them `unavailable-local-build`; such a receipt is not exact-head CI evidence.

The reproducible entry point is the **isolated renderer crate**, matching CI. The root workspace still includes spore-kernel path dependencies to modules absent from this extraction, so do not use the root workspace as evidence that the renderer itself fails or passes.

```bash
bench_dir="$(mktemp -d)"
trap 'rm -rf "$bench_dir"' EXIT
mkdir -p "$bench_dir/quicken-fb"
cp -a crates/quicken-fb/. "$bench_dir/quicken-fb/"
cargo run --manifest-path "$bench_dir/quicken-fb/Cargo.toml" --locked --release --example render_bench -- \
  --theme cosmic-atlas --width 1920 --height 1080 --frames 120 \
  --snapshot /tmp/cosmic-atlas.ppm
```

It measures CPU scene update plus pixel rendering only, accepts `--motion cinematic` or `--motion reduced`, and optionally emits a deterministic P6 PPM capture from the same final frame buffer. Reduced mode freezes the scene clock and selects the dedicated still composition. It does not include DRM mapping/blitting, actual display timing, GPU work, or power; do not label its output as full-screen FPS or a graphics-card result. Keep the JSON record, PPM capture, SHA-256, source commit and host tuple together for visual/performance review.

The growth-only benchmark measures simulation updates at four branch-count thresholds without drawing pixels:

```bash
bench_dir="$(mktemp -d)"
trap 'rm -rf "$bench_dir"' EXIT
mkdir -p "$bench_dir/quicken-fb"
cp -a crates/quicken-fb/. "$bench_dir/quicken-fb/"
cargo run --manifest-path "$bench_dir/quicken-fb/Cargo.toml" --locked --release --example growth_bench -- --steps 3000 --theme mycelial-genesis
```

It reports p50/p95/max for the measured update interval leading to each 512, 2,048, 4,096 and 8,192-branch checkpoint, including the observed start/end branch counts and step range. Once the 8,192 cap is reached, it also records 128 separate saturated steady-state updates. Qualification requires all thresholds plus the capped steady-state sample to be present; these receipts remain separate from pixel-render timings.

- Capture the current CPU renderer at fixed timepoints and resolutions.
- Add counters for frame time, branch count, allocations, and dropped frames behind an opt-in benchmark mode; do not log user data.
- Extend the temporal audit with opposing-transition windows and saturated-red chromaticity screening, then compare its findings against actual high-refresh-rate display captures; keep the current fixed-step proxy explicitly non-normative.
- Compare growth-only receipts against the parent revision and track p95 update cost at 512, 2,048, 4,096, and 8,192 branches. The in-branch spatial index preserves the previous <1 px intersection deduplication rule while avoiding repeated whole-graph scans; keep a negative-control workload for pathological density.
- Crossfade full scene detail over the theme background across a bounded 400 ms simulation-time interval beginning at the actual one-second scene boundary; regression tests cover the exact start, midpoint, and completion.
- Keep the final fade so rendered luminance transitions continuously to black.
- Require every manifest theme's GPU reference capture to have a distinct exact pixel hash on the same adapter/resolution/time point; then review geometry and palette quality separately. This is an identity regression guard, not a perceptual-quality metric.
- Compare static, CPU, and Vulkan-backed desktop paths at identical scene states.
- Store a compact benchmark receipt containing commit SHA, manifest version, machine tuple, command, result summary, and artifact hashes.

## GPU execution-time qualification protocol

The WGSL shader is still exploratory; no executable wgpu/Vulkan desktop renderer or GPU performance result is currently qualified. When the renderer exists, do not infer execution time from CPU command-encoding or queue-submission latency. Those are different measurements.

### Instrumentation contract

1. Query the adapter's supported features and enable wgpu::Features::TIMESTAMP_QUERY only when the adapter advertises support. For timestamps written inside a pass or encoder, request the corresponding additional feature only when supported. Record unsupported timestamp queries as gpu_timestamps_available: false; never silently substitute CPU timing for GPU time.
2. Use a ring of timestamp query pairs. Place a start/end timestamp around measured render work, resolve query sets into a buffer, and read results asynchronously after GPU completion. Avoid a blocking map or device-wide wait on every frame, since instrumentation must not become a frame-pacing mechanism.
3. Convert timestamp deltas using Queue::get_timestamp_period() and report elapsed nanoseconds. Reject invalid, wrapped, unavailable, or incomplete samples rather than manufacturing durations.
4. Produce separate distributions for simulation/update CPU time, CPU encoding/submission time, GPU execution time, and end-to-end frame/present timing. The GPU timestamp interval does not include every part of display scan-out or physical presentation latency.
5. Pair every receipt with the exact commit, renderer and shader hashes, OS/kernel, adapter/device ID, driver, backend, enabled features, resolution, refresh rate, theme, seed, motion mode, power state, warm-up count, sample count, and artifact SHA-256. Omit sensitive machine identifiers if receipts will be published outside the engineering team.

The API behavior and feature-gating requirements are documented in the [wgpu::Features::TIMESTAMP_QUERY reference](https://docs.rs/wgpu/latest/wgpu/struct.Features.html) and [wgpu::QueryType::Timestamp](https://docs.rs/wgpu/latest/wgpu/enum.QueryType.html). The feature is not guaranteed, so the runtime must test support before requesting it.

### Qualification workload

For each GPU backend and target device:

- Warm up for at least 120 frames; gather at least 1,800 measured frames in each trial, with three trials per scene/resolution. Preserve individual trial receipts rather than pooling away outliers.
- Test all ten themes at 640×360, 1920×1080, and 3840×2160; include Reduced, Low-power, and Cinematic modes, plus minimum/median/maximum branch density.
- Report p50/p95/p99/max for CPU update, CPU command recording/submission, GPU render execution, and end-to-end frame timing. Report dropped/missed frames separately; a low GPU duration does not compensate for stalls elsewhere.
- Record whether timestamp queries were supported and actually used. If not, use a documented external GPU profiler or mark GPU execution time unmeasured. Do not infer GPU time from an FPS counter alone.

Initial target budgets (not achieved measurements): at 1080p/60 Hz, total frame p95 ≤16.7 ms and p99 ≤25 ms, with GPU-render p95 ≤12 ms as a sub-budget to leave room for CPU work and presentation; at 4K, sustain ≥30 FPS with total frame p95 ≤33.3 ms. Track power only where trustworthy device telemetry is available, and label it unmeasured otherwise.

### Accessibility sampling rule

The external-frame audit is a regression proxy, not the official WCAG algorithm. W3C describes a flash as two opposing transitions and evaluates flash frequency within a one-second interval; the reference visual-field estimate is a 341×256 pixel region at 1024×768, not an arbitrary full-screen percentage ([W3C SC 2.3.1 guidance](https://www.w3.org/WAI/WCAG22/Understanding/three-flashes-or-below-threshold.html)). The local raster window added here improves on the full-frame-only alarm, but does not model actual viewing distance or all physical display behavior. Qualification still requires a validated independent analyzer and captured output.

## Release gate

A visual release may be called qualified only when focused tests and benchmark artifacts refer to the exact reviewed commit. A package build demonstrates that a derivation built; it does not prove service handoff, privacy, visual quality, Vulkan availability, or successful boot.
