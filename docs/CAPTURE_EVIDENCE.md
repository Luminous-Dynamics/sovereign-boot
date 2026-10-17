# Reproducible display-capture evidence

**Purpose:** create a reproducible provenance bundle for the independent P6 PPM frame auditor. It is a regression-evidence workflow, not a WCAG certification tool.

## Run against a local recording

Use a locally stored MP4 or other FFmpeg-readable recording. The output directory must be outside the Git checkout and empty; the tool refuses a dirty checkout so a receipt cannot silently claim to describe a different source tree.

    validation/capture-audit.sh \
      --video /path/to/local-display-recording.mp4 \
      --fps 30 \
      --output-dir /tmp/sovereign-boot-capture-evidence

For frames already extracted from a recording, preserve their order explicitly:

    validation/capture-audit.sh \
      --frames /tmp/frames/frame-000001.ppm /tmp/frames/frame-000002.ppm /tmp/frames/frame-000003.ppm \
      --fps 30 \
      --output-dir /tmp/sovereign-boot-capture-evidence

The input is restricted to local regular files. Video mode hashes the original recording and records selected ffprobe stream metadata; it extracts frames uniformly at the requested integer FPS without resizing. Frame mode hashes the original ordered PPM inputs. The tool never writes source paths or source filenames into the manifest. Evidence is created with owner-only permissions because captures may show private desktop or lock-screen content.

## Evidence bundle

- **capture-manifest.json:** schema version, source/build commit SHAs, bundle creation time (not source-media capture time), input type, source hashes, sample cadence, dimensions, tool versions, hashes of the frame manifest and audit receipt, plus explicit limitations.
- **audit.json:** the independent Rust auditor's captured-frame-audit-v3 receipt.
- **frames/:** normalized, ordered P6 PPM files actually examined.
- **frames.sha256:** hashes for every audited frame.
- **input-frames.sha256:** source PPM hashes and order when frame-sequence mode is used.
- **source-media-probe.json:** selected video metadata in video mode.
- **evidence.sha256:** checksums for the evidence manifests/receipts; the frame manifest separately authenticates the frame bytes.

Verify the bundle after transfer:

    cd /tmp/sovereign-boot-capture-evidence
    sha256sum --check evidence.sha256
    sha256sum --check frames.sha256

The original video itself is not copied into the bundle; its SHA-256 is recorded in capture-manifest.json. Keep the recording separately, along with display model/refresh rate, capture method, OS/compositor, color-management settings, and any camera/display geometry notes. Do not publish real lock-screen recordings or raw captures without a privacy review.

## Interpretation limits

The frame-audit executable receives a single assumed FPS and ordered frames, not the original timestamp of each source frame. Variable-frame-rate timestamps, dropped or duplicated frames, camera exposure/rolling shutter, codec artifacts, display scaling, color conversion, and events between samples may change what it observes. Preserve the original source and never present the bundle as proof that no unobserved event occurred.

W3C's [WCAG 2.2 understanding of SC 2.3.1](https://www.w3.org/WAI/WCAG22/Understanding/three-flashes-or-below-threshold) describes flashes as opposing transitions and evaluates frequency over any one-second period. Its local visual-field area criteria are not equivalent to a percentage of the full raster. This repository's analyzer deliberately reports normative_wcag_conformance: false and remains a regression proxy; use a validated independent analyzer and appropriate real-display measurements for any stronger claim.

## Next qualification milestones

1. Add timestamp-aware sampling so variable-frame-rate captures can preserve presentation timestamps instead of relying only on a nominal FPS.
2. Compare at least one monitor/camera capture and one OS-level screen capture; document what each path does and does not observe.
3. Add known-positive, known-negative, red-transition, bright-plateau, and between-sample adversarial fixtures; retain their raw frames and expected results.
4. Record end-to-end boot/desktop lifecycle traces separately from shader time: process start, first presented frame, compositor handoff, stop request, last display access, and fallback completion.
5. Keep CPU submission, GPU execution timestamps, compositor/presentation timing, and physical display observations as separate measurements.

## References

- [W3C WCAG 2.2 SC 2.3.1: Three Flashes or Below Threshold](https://www.w3.org/WAI/WCAG22/Understanding/three-flashes-or-below-threshold)
- [Khronos Vulkan timestamp-query sample](https://docs.vulkan.org/samples/latest/samples/api/timestamp_queries/README.html)
- [Vulkan Guide: Profiling](https://docs.vulkan.org/guide/latest/profiling.html)
