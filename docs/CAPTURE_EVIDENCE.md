# Reproducible display-capture evidence

**Purpose:** create a reproducible provenance bundle for the independent P6 PPM frame auditor. It is a regression-evidence workflow, not a WCAG certification tool.

## Run against a local recording

Use a locally stored MP4 or other FFmpeg-readable recording. Video frames are selected at the requested minimum temporal cadence using source PTS; this is not a guarantee that every source frame or every display event is captured. The output directory must be outside the Git checkout and empty; the tool refuses a dirty checkout so a receipt cannot silently claim to describe a different source tree.

    validation/capture-audit.sh \
      --video /path/to/local-display-recording.mp4 \
      --fps 30 \
      --output-dir /tmp/sovereign-boot-capture-evidence

For frames already extracted from a recording, preserve their order explicitly:

    validation/capture-audit.sh \
      --frames /tmp/frames/frame-000001.ppm /tmp/frames/frame-000002.ppm /tmp/frames/frame-000003.ppm \
      --fps 30 \
      --output-dir /tmp/sovereign-boot-capture-evidence

The input is restricted to local regular files. PPM inputs are rejected before copying if there are more than 1,800 frames, any frame exceeds 25,000,000 bytes, or aggregate input bytes exceed 1,600,000,000 bytes. Symlinked PPM frames and timestamp sidecars are rejected. Video mode hashes the original recording, records selected ffprobe metadata, and uses FFmpeg select/showinfo to sample by elapsed source presentation time. Each selected frame's integer PTS is converted by the dependency-free Rust pts-to-ns helper using checked i128 integer arithmetic through the filter-reported time base. Relative nanoseconds are emitted without floating-point rounding; the original origin PTS ticks, exact origin-nanosecond decimal string, and time base are recorded in the manifest (the legacy numeric origin field is retained for compatibility). Extraction is bounded by the UHD 4K per-frame limit, a maximum of 1,800 frames, and a 500-million-sampled-pixel work budget; higher-workload clips are rejected rather than silently truncated. Frame mode hashes the ordered PPM inputs and accepts an optional timestamp sidecar through `--timestamps`; that sidecar is copied byte-for-byte and does not imply source-media PTS provenance. `--max-gap-ms` sets the sampling-gap alarm (default 100 ms).

The tool never writes source paths or source filenames into the manifest. Evidence is created with owner-only permissions because captures may show private desktop or lock-screen content.

## Evidence bundle

- **capture-manifest.json:** schema version, source/build commit SHAs, bundle creation time (not source-media capture time), input type, source hashes, sample cadence, dimensions, tool versions, hashes of the frame manifest and audit receipt, plus explicit limitations.
- **audit.json:** the independent Rust auditor's v3 nominal-FPS receipt or v4 timestamped receipt, depending on input mode.
- **frames/:** normalized, ordered P6 PPM files actually examined.
- **frames.sha256:** hashes for every audited frame.
- **presentation-timestamps-ns.txt:** the normalized nanosecond timestamp series used by timestamp mode; for video it is derived from selected source PTS and the recorded filter time base.
- **input-frames.sha256:** source PPM hashes and order when frame-sequence mode is used.
- **source-media-probe.json:** selected video metadata in video mode.
- **evidence.sha256:** checksums for the evidence manifests/receipts; the frame manifest separately authenticates the frame bytes.

Verify the bundle after transfer:

    cd /tmp/sovereign-boot-capture-evidence
    sha256sum --check evidence.sha256
    sha256sum --check frames.sha256

The original video itself is not copied into the bundle; its SHA-256 is recorded in capture-manifest.json. In video mode, the normalized PTS sidecar, source PTS origin, and filter time base are recorded and hashed. For PPM-sequence mode, the caller-provided sidecar is copied byte-for-byte and hashed; the manifest does not assert that it came from original media. Keep the recording separately, along with display model/refresh rate, capture method, OS/compositor, color-management settings, and any camera/display geometry notes. Do not publish real lock-screen recordings or raw captures without a privacy review.

## Long-duration chunked capture audit

The initial bounded chunking path for pre-extracted ordered P6 PPM sequences is validation/chunked-frame-audit.sh. It is qualification-gated and is not yet a replacement for the single-shot path. It keeps the existing per-audit 500,000,000-sampled-pixel bound and accepts up to 10,000 caller-owned frames under an aggregate input-byte cap; every chunk is independently checked by the same Rust auditor.

Example:

    validation/chunked-frame-audit.sh \
      --output-dir /tmp/chunked-frame-audit-evidence \
      --timestamps /tmp/presentation-timestamps-ns.txt \
      --max-gap-ms 100 --chunk-frames 60 \
      --frames /tmp/frames/frame-000001.ppm /tmp/frames/frame-000002.ppm /tmp/frames/frame-000003.ppm

Each later chunk starts at the earliest observed sample within one second of its first newly emitted sample, plus one earlier predecessor. The wrapper audits both the full chunk and the prefix ending immediately before new samples, then subtracts prefix counters so overlapped transitions/windows are not double-counted. It checks nonnegative deltas and requires the merged transition count to equal frame_count - 1. The receipt binds ordered input hashes, source and normalized timestamp hashes, per-chunk full/prefix receipts, unique source pixels, actual pixel work including overlap reprocessing, redundant overlap work, chunk-audit elapsed wall time (excluding compilation), and an evidence checksum. Original frame paths are not written to the evidence bundle; the PPM corpus remains caller-owned and must be retained separately.

Large sample gaps remain explicitly incomplete_sampling_due_to_gap; a green-looking event count is not a complete result when the coverage gate fails. The workflow compares a deterministic chunked run to the monolithic timestamped receipt, including temporal-window summaries, and runs a gap negative control. This first version only chunks pre-extracted PPM files. It does not yet serialize/restore the auditor's per-pixel state, stream source video, or qualify a 60-second 1080p/30 capture under a named machine/RSS benchmark. It remains a non-normative visual regression proxy, not WCAG certification.
## Interpretation limits

For ordered PPM frames, supply a timestamp sidecar:

    validation/capture-audit.sh \
      --frames /tmp/frame-000001.ppm /tmp/frame-000002.ppm /tmp/frame-000003.ppm \
      --timestamps /tmp/timestamps-ns.txt --max-gap-ms 100 \
      --output-dir /tmp/sovereign-boot-capture-evidence

Timestamp mode emits schema `captured-frame-audit-v4-timestamped`. It pairs opposing transitions using elapsed nanoseconds and evaluates one-second windows against presentation timestamps. A frame gap above the declared threshold (100 ms by default) is explicitly reported as `incomplete_sampling_due_to_gap`; observed event counts must not be interpreted as complete when that gate fails. Timestamps are presumed to use a single monotonic time base; do not mix wall-clock timestamps with media-relative PTS.

The legacy nominal-FPS mode remains schema v3 for reproducibility. Timestamp mode (including video-derived source PTS) uses schema v4. Timestamp mode improves time accounting but does not recover events that happen between sampled frames. Camera exposure/rolling shutter, codec artifacts, display scaling, color conversion, and physical-display differences may change what is observed. Preserve the original source and never present the bundle as proof that no unobserved event occurred.

W3C's [WCAG 2.2 understanding of SC 2.3.1](https://www.w3.org/WAI/WCAG22/Understanding/three-flashes-or-below-threshold) describes flashes as opposing transitions and evaluates frequency over any one-second period. Its local visual-field area criteria are not equivalent to a percentage of the full raster. This repository's analyzer deliberately reports normative_wcag_conformance: false and remains a regression proxy; use a validated independent analyzer and appropriate real-display measurements for any stronger claim.

## Next qualification milestones

1. Expand video tests to variable-frame-rate, missing-PTS, discontinuous-timestamp, and time-base-changing fixtures; retain the PTS origin and filter time base in provenance.
2. Compare at least one monitor/camera capture and one OS-level screen capture; document what each path does and does not observe.
3. Add known-positive, known-negative, red-transition, bright-plateau, and between-sample adversarial fixtures; retain their raw frames and expected results.
4. Record end-to-end boot/desktop lifecycle traces separately from shader time: process start, first presented frame, compositor handoff, stop request, last display access, and fallback completion.
5. Keep CPU submission, GPU execution timestamps, compositor/presentation timing, and physical display observations as separate measurements.

## References

- [W3C WCAG 2.2 SC 2.3.1: Three Flashes or Below Threshold](https://www.w3.org/WAI/WCAG22/Understanding/three-flashes-or-below-threshold)
- [Khronos Vulkan timestamp-query sample](https://docs.vulkan.org/samples/latest/samples/api/timestamp_queries/README.html)
- [Vulkan Guide: Profiling](https://docs.vulkan.org/guide/latest/profiling.html)
