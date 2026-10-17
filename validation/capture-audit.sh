#!/usr/bin/env bash
# Produce a privacy-conscious, hash-linked evidence bundle for the independent
# frame auditor. The wrapper records provenance; it is not a WCAG certifier.
set -euo pipefail
umask 077

usage() {
  cat <<'USAGE'
Usage:
  validation/capture-audit.sh --output-dir DIR --fps N --video RECORDING
  validation/capture-audit.sh --output-dir DIR --fps N --frames A.ppm B.ppm C.ppm [...]

Provide exactly one input mode. Frames must be ordered P6 PPM files. Video
frames are sampled uniformly by FFmpeg without resizing. Use an output folder
outside the Git checkout; evidence images may contain private screen content.
USAGE
}

die() { printf 'capture-audit: %s\n' "$*" >&2; exit 2; }

invocation_dir="$(pwd -P)"
output_arg=""
fps="30"
video=""
frames=()
mode=""

while (($#)); do
  case "$1" in
    --output-dir)
      (($# >= 2)) || die "--output-dir requires a path"
      output_arg="$2"
      shift 2
      ;;
    --fps)
      (($# >= 2)) || die "--fps requires an integer"
      fps="$2"
      shift 2
      ;;
    --video)
      (($# >= 2)) || die "--video requires a file path"
      [[ -z "$mode" ]] || die "choose either --video or --frames, not both"
      mode="video"
      video="$2"
      shift 2
      ;;
    --frames)
      [[ -z "$mode" ]] || die "choose either --video or --frames, not both"
      mode="frames"
      shift
      while (($#)) && [[ "$1" != --* ]]; do
        frames+=("$1")
        shift
      done
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      usage >&2
      die "unknown argument: $1"
      ;;
  esac
done

[[ -n "$output_arg" ]] || die "--output-dir is required"
[[ "$fps" =~ ^[0-9]+$ ]] || die "--fps must be an integer between 1 and 240"
[[ ${#fps} -le 3 ]] || die "--fps must be an integer between 1 and 240"
((10#$fps >= 1 && 10#$fps <= 240)) || die "--fps must be an integer between 1 and 240"
fps=$((10#$fps))
[[ "$mode" == "video" || "$mode" == "frames" ]] || die "one input mode is required"
[[ "$mode" != "frames" || ${#frames[@]} -ge 3 ]] || die "--frames requires at least three ordered PPM files"

for command_name in git cargo rustc jq sha256sum realpath; do
  command -v "$command_name" >/dev/null 2>&1 || die "required command missing: $command_name"
done

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd -- "$script_dir/.." && pwd -P)"
cd "$repo_root"
[[ -z "$(git status --porcelain --untracked-files=normal)" ]] || die "checkout must be clean so receipts identify reviewed source exactly"

source_commit="${VISUAL_SOURCE_COMMIT:-$(git rev-parse HEAD)}"
build_commit="${VISUAL_BUILD_COMMIT:-$(git rev-parse HEAD)}"
[[ "$source_commit" =~ ^[0-9a-f]{40}$ ]] || die "VISUAL_SOURCE_COMMIT must be a full 40-character SHA"
[[ "$build_commit" =~ ^[0-9a-f]{40}$ ]] || die "VISUAL_BUILD_COMMIT must be a full 40-character SHA"
export VISUAL_SOURCE_COMMIT="$source_commit" VISUAL_BUILD_COMMIT="$build_commit"

case "$output_arg" in
  /*) output_dir="$output_arg" ;;
  *) output_dir="$invocation_dir/$output_arg" ;;
esac
output_dir="$(realpath -m -- "$output_dir")"
case "$output_dir/" in
  "$repo_root/"*) die "output directory must be outside the Git checkout" ;;
esac
mkdir -p -- "$output_dir"
[[ -z "$(find "$output_dir" -mindepth 1 -maxdepth 1 -print -quit)" ]] || die "output directory must be empty"
chmod 700 -- "$output_dir"

frame_dir="$output_dir/frames"
mkdir -m 700 -- "$frame_dir"
bundle_created_utc="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
cargo_version="$(cargo --version)"
rustc_version="$(rustc --version)"
ffmpeg_version="not-used"
ffprobe_version="not-used"
source_video_sha256=""
source_sequence_sha256=""
source_media_json="null"
extraction_method="ordered P6 PPM input; copied without resizing"

if [[ "$mode" == "video" ]]; then
  command -v ffmpeg >/dev/null 2>&1 || die "video mode requires ffmpeg"
  command -v ffprobe >/dev/null 2>&1 || die "video mode requires ffprobe"
  [[ -f "$video" ]] || [[ -f "$invocation_dir/$video" ]] || die "video input must be a local regular file"
  video_path="$video"
  [[ "$video_path" == /* ]] || video_path="$invocation_dir/$video_path"
  video_path="$(cd -- "$(dirname -- "$video_path")" && pwd -P)/$(basename -- "$video_path")"
  source_video_sha256="$(sha256sum -- "$video_path" | awk '{print $1}')"
  ffmpeg_version="$(ffmpeg -version | sed -n '1p')"
  ffprobe_version="$(ffprobe -version | sed -n '1p')"
  probe_path="$output_dir/source-media-probe.json"
  ffprobe -v error -select_streams v:0 \
    -show_entries 'stream=width,height,avg_frame_rate,r_frame_rate,pix_fmt,color_space,color_transfer,color_primaries:format=duration' \
    -of json "$video_path" > "$probe_path"
  source_media_json="$(cat "$probe_path")"
  extraction_method="ffmpeg fps=${fps} filter; CFR samples; no resizing; original timestamps are not passed to the audit"
  ffmpeg -nostdin -hide_banner -v error -i "$video_path" \
    -vf "fps=${fps}" -fps_mode cfr -start_number 1 \
    "$frame_dir/frame-%06d.ppm"
else
  : > "$output_dir/input-frames.sha256"
  frame_index=0
  for frame in "${frames[@]}"; do
    [[ -f "$frame" ]] || [[ -f "$invocation_dir/$frame" ]] || die "frame input must be a local regular file"
    frame_path="$frame"
    [[ "$frame_path" == /* ]] || frame_path="$invocation_dir/$frame_path"
    frame_path="$(cd -- "$(dirname -- "$frame_path")" && pwd -P)/$(basename -- "$frame_path")"
    frame_index=$((frame_index + 1))
    printf -v frame_name 'frame-%06d.ppm' "$frame_index"
    cp -- "$frame_path" "$frame_dir/$frame_name"
    digest="$(sha256sum -- "$frame_path" | awk '{print $1}')"
    printf '%06d %s\n' "$frame_index" "$digest" >> "$output_dir/input-frames.sha256"
  done
  source_sequence_sha256="$(sha256sum -- "$output_dir/input-frames.sha256" | awk '{print $1}')"
fi

shopt -s nullglob
ppm_files=("$frame_dir"/frame-*.ppm)
((${#ppm_files[@]} >= 3)) || die "capture yielded fewer than three frames"
((${#ppm_files[@]} <= 1800)) || die "capture exceeds auditor's 1,800-frame limit"

(
  cd "$output_dir"
  sha256sum frames/frame-*.ppm > frames.sha256
  sha256sum --check frames.sha256 >/dev/null
)

cargo run --manifest-path validation/frame-audit/Cargo.toml --locked --release -- \
  --fps "$fps" "${ppm_files[@]}" > "$output_dir/audit.json"

audit_schema="$(jq -r '.schema // empty' "$output_dir/audit.json")"
[[ "$audit_schema" == "captured-frame-audit-v3" ]] || die "unexpected auditor receipt schema"
jq -e --arg source "$source_commit" --arg build "$build_commit" \
  '.source_commit == $source and .build_commit == $build and .normative_wcag_conformance == false' \
  "$output_dir/audit.json" >/dev/null || die "auditor receipt provenance/schema validation failed"

frame_manifest_sha256="$(sha256sum -- "$output_dir/frames.sha256" | awk '{print $1}')"
audit_sha256="$(sha256sum -- "$output_dir/audit.json" | awk '{print $1}')"
input_sequence_json="null"
[[ -z "$source_sequence_sha256" ]] || input_sequence_json="\"$source_sequence_sha256\""
source_video_json="null"
[[ -z "$source_video_sha256" ]] || source_video_json="\"$source_video_sha256\""

jq -n \
  --arg source_commit "$source_commit" \
  --arg build_commit "$build_commit" \
  --arg bundle_created_utc "$bundle_created_utc" \
  --arg input_kind "$mode" \
  --arg extraction_method "$extraction_method" \
  --arg cargo_version "$cargo_version" \
  --arg rustc_version "$rustc_version" \
  --arg ffmpeg_version "$ffmpeg_version" \
  --arg ffprobe_version "$ffprobe_version" \
  --argjson fps "$fps" \
  --argjson frame_count "${#ppm_files[@]}" \
  --arg frame_manifest_sha256 "$frame_manifest_sha256" \
  --arg audit_sha256 "$audit_sha256" \
  --argjson source_video_sha256 "$source_video_json" \
  --argjson source_sequence_sha256 "$input_sequence_json" \
  --argjson source_media "$source_media_json" \
  --slurpfile audit "$output_dir/audit.json" \
  '{
    schema: "visual-capture-provenance-v1",
    source_commit: $source_commit,
    build_commit: $build_commit,
    bundle_created_at_utc: $bundle_created_utc,
    input_kind: $input_kind,
    source_video_sha256: $source_video_sha256,
    source_frame_sequence_sha256: $source_sequence_sha256,
    source_media: $source_media,
    sample_fps_assumed: $fps,
    frame_count: $frame_count,
    resolution: $audit[0].resolution,
    extraction_method: $extraction_method,
    tools: {
      cargo: $cargo_version,
      rustc: $rustc_version,
      ffmpeg: $ffmpeg_version,
      ffprobe: $ffprobe_version
    },
    frame_hash_manifest_sha256: $frame_manifest_sha256,
    audit_receipt_sha256: $audit_sha256,
    source_paths_recorded: false,
    original_frame_timestamps_analyzed: false,
    normative_wcag_conformance: false,
    limitations: [
      "The auditor assumes uniformly spaced samples at sample_fps_assumed.",
      "Video frame extraction can omit, duplicate, resample, color-convert, or miss display events.",
      "A passing proxy receipt does not establish WCAG conformance or physical-display safety.",
      "Keep the original recording and capture/display details with this local evidence bundle."
    ]
  }' > "$output_dir/capture-manifest.json"

(
  cd "$output_dir"
  check_files=(audit.json capture-manifest.json frames.sha256)
  [[ ! -f input-frames.sha256 ]] || check_files+=(input-frames.sha256)
  [[ ! -f source-media-probe.json ]] || check_files+=(source-media-probe.json)
  sha256sum "${check_files[@]}" > evidence.sha256
  sha256sum --check evidence.sha256 >/dev/null
)
chmod -R go-rwx -- "$output_dir"
printf 'Evidence bundle created: %s\n' "$output_dir"
printf 'Reviewed source: %s\nBuild: %s\n' "$source_commit" "$build_commit"
printf 'Frames: %s at assumed %s Hz; audit is explicitly non-normative.\n' "${#ppm_files[@]}" "$fps"
