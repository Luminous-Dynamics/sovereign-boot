#!/usr/bin/env bash
# Produce a privacy-conscious, hash-linked evidence bundle for the independent
# frame auditor. The wrapper records provenance; it is not a WCAG certifier.
set -euo pipefail
umask 077
# P6 RGB at UHD 4K is about 24.9 MB including its short header.
MAX_INPUT_FRAME_BYTES=25000000
MAX_INPUT_TOTAL_BYTES=1600000000
MAX_INPUT_FRAMES=1800

usage() {
  cat <<'USAGE'
Usage:
  validation/capture-audit.sh --output-dir DIR --fps N --video RECORDING
  validation/capture-audit.sh --output-dir DIR --fps N --frames A.ppm B.ppm C.ppm [...]

Provide exactly one input mode. Frames must be ordered P6 PPM files. Video
frames are sampled by source PTS at the requested minimum cadence; neither path
resizes frames. Use an output folder outside the Git checkout; captures may be private.
USAGE
}

die() { printf 'capture-audit: %s\n' "$*" >&2; exit 2; }

invocation_dir="$(pwd -P)"
output_arg=""
fps="30"
video=""
frames=()
timestamps_file=""
max_gap_ms="100"
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
    --timestamps)
      (($# >= 2)) || die "--timestamps requires a file path"
      [[ -z "$timestamps_file" ]] || die "--timestamps may be supplied only once"
      timestamps_file="$2"
      shift 2
      ;;
    --max-gap-ms)
      (($# >= 2)) || die "--max-gap-ms requires a value"
      max_gap_ms="$2"
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
[[ "$mode" != "frames" || ${#frames[@]} -le "$MAX_INPUT_FRAMES" ]] || die "--frames exceeds the auditor's 1,800-frame limit"
[[ "$max_gap_ms" =~ ^[0-9]+$ && ${#max_gap_ms} -le 5 ]] || die "--max-gap-ms must be an integer from 1 to 60000"
((10#$max_gap_ms >= 1 && 10#$max_gap_ms <= 60000)) || die "--max-gap-ms must be an integer from 1 to 60000"
max_gap_ms=$((10#$max_gap_ms))
[[ "$mode" != "video" || -z "$timestamps_file" ]] || die "--timestamps is for PPM input; video mode derives timestamps from source PTS"

for command_name in git cargo rustc jq sha256sum realpath stat; do
  command -v "$command_name" >/dev/null 2>&1 || die "required command missing: $command_name"
done

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd -- "$script_dir/.." && pwd -P)"
cd "$repo_root"
[[ -z "$(git status --porcelain --untracked-files=normal)" ]] || die "checkout must be clean so receipts identify reviewed source exactly"

temporary_cargo_target=""
if [[ -z "${CARGO_TARGET_DIR:-}" ]]; then
  temporary_cargo_target="$(mktemp -d "${TMPDIR:-/tmp}/sovereign-capture-audit-target.XXXXXX")"
  export CARGO_TARGET_DIR="$temporary_cargo_target"
  trap 'if [[ -n "${temporary_cargo_target:-}" ]]; then rm -rf -- "$temporary_cargo_target"; fi' EXIT
fi

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
timestamp_sidecar_sha256=""
timestamp_sidecar_json="null"
source_media_json="null"
temporal_basis="nominal_fps"
timestamps_path=""
source_media_pts_preserved="false"
source_pts_origin_ns_json="null"
source_pts_time_base_json="null"
max_video_frames=0
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
    -show_entries 'stream=width,height,time_base,avg_frame_rate,r_frame_rate,pix_fmt,color_space,color_transfer,color_primaries:format=duration' \
    -of json "$video_path" > "$probe_path"
  source_media_json="$(cat "$probe_path")"
  video_width="$(jq -r '.streams[0].width // empty' "$probe_path")"
  video_height="$(jq -r '.streams[0].height // empty' "$probe_path")"
  [[ "$video_width" =~ ^[0-9]+$ && ${#video_width} -le 5 ]] || die "could not determine a bounded video width"
  [[ "$video_height" =~ ^[0-9]+$ && ${#video_height} -le 5 ]] || die "could not determine a bounded video height"
  video_pixels=$((video_width * video_height))
  ((video_pixels > 0 && video_pixels <= 8294400)) || die "video resolution exceeds the auditor's UHD 4K per-frame bound"
  max_video_frames=$((500000000 / video_pixels))
  ((max_video_frames <= 1800)) || max_video_frames=1800
  ((max_video_frames >= 3)) || die "video resolution leaves too little pixel-work budget for a three-frame audit"
  video_frame_limit=$((max_video_frames + 1))

  timestamps_path="$output_dir/presentation-timestamps-ns.txt"
  extraction_method="FFmpeg-selected source PTS converted through showinfo filter time base; relative to first selected frame"
  ffmpeg_log="$output_dir/.ffmpeg-extraction.log"
  pts_origin_file="$output_dir/.source-pts-origin-ns"
  ffmpeg -nostdin -hide_banner -loglevel info -i "$video_path" \
    -map 0:v:0 \
    -vf "select='isnan(prev_selected_t)+gte(t-prev_selected_t\,0.999/${fps})',showinfo" \
    -fps_mode passthrough -frames:v "$video_frame_limit" -start_number 1 \
    "$frame_dir/frame-%06d.ppm" 2> "$ffmpeg_log"

  time_base="$(sed -nE 's/.*config in time_base: ([0-9]+)\/([0-9]+),.*/\1 \2/p' "$ffmpeg_log" | head -n 1)"
  [[ -n "$time_base" ]] || die "FFmpeg did not report the selected-frame PTS time base"
  read -r pts_timebase_num pts_timebase_den <<< "$time_base"
  [[ "$pts_timebase_num" =~ ^[0-9]+$ && "$pts_timebase_den" =~ ^[0-9]+$ ]] || die "invalid FFmpeg PTS time base"
  ((pts_timebase_num > 0 && pts_timebase_den > 0)) || die "invalid zero-valued FFmpeg PTS time base"
  if ! grep -q 'showinfo.* n:.*pts:' "$ffmpeg_log"; then
    die "FFmpeg did not report presentation timestamps for selected frames"
  fi
  grep 'showinfo.* n:.*pts:' "$ffmpeg_log" \
    | sed -nE 's/.* n:[[:space:]]*[0-9]+ pts:[[:space:]]*(-?[0-9]+) pts_time:.*/\1/p' \
    | awk -v num="$pts_timebase_num" -v den="$pts_timebase_den" -v origin_path="$pts_origin_file" '
        NF {
          t = ($1 + 0) * num * 1000000000 / den
          if (!seen) {
            origin = t
            printf "%.0f\n", origin > origin_path
            close(origin_path)
            seen = 1
          }
          ns = t - origin
          if (ns < 0) exit 2
          printf "%.0f\n", ns
        }
        END { if (!seen) exit 3 }
      ' > "$timestamps_path"
  [[ -s "$timestamps_path" && -s "$pts_origin_file" ]] || die "could not generate a complete PTS sidecar"
  source_pts_origin_ns="$(cat "$pts_origin_file")"
  rm -f -- "$pts_origin_file"
  source_pts_origin_ns_json="$source_pts_origin_ns"
  source_pts_time_base_json="\"$time_base\""
  rm -f -- "$ffmpeg_log"
  temporal_basis="presentation_timestamp_ns"
  timestamp_sidecar_sha256="$(sha256sum -- "$timestamps_path" | awk '{print $1}')"
  timestamp_sidecar_json="\"$timestamp_sidecar_sha256\""
  source_media_pts_preserved="true"
else
  : > "$output_dir/input-frames.sha256"
  frame_index=0
  total_input_frame_bytes=0
  for frame in "${frames[@]}"; do
    frame_path="$frame"
    [[ "$frame_path" == /* ]] || frame_path="$invocation_dir/$frame_path"
    [[ -f "$frame_path" && ! -L "$frame_path" ]] || die "frame input must be a local regular, non-symlink file"
    frame_path="$(cd -- "$(dirname -- "$frame_path")" && pwd -P)/$(basename -- "$frame_path")"
    frame_size_bytes="$(stat -c '%s' -- "$frame_path")"
    [[ "$frame_size_bytes" =~ ^[0-9]+$ ]] || die "could not determine input frame size"
    ((frame_size_bytes <= MAX_INPUT_FRAME_BYTES)) || die "input frame exceeds 25,000,000-byte limit"
    total_input_frame_bytes=$((total_input_frame_bytes + frame_size_bytes))
    ((total_input_frame_bytes <= MAX_INPUT_TOTAL_BYTES)) || die "ordered PPM inputs exceed 1,600,000,000-byte aggregate limit"
    frame_index=$((frame_index + 1))
    printf -v frame_name 'frame-%06d.ppm' "$frame_index"
    cp -- "$frame_path" "$frame_dir/$frame_name"
    digest="$(sha256sum -- "$frame_path" | awk '{print $1}')"
    printf '%06d %s\n' "$frame_index" "$digest" >> "$output_dir/input-frames.sha256"
  done
  source_sequence_sha256="$(sha256sum -- "$output_dir/input-frames.sha256" | awk '{print $1}')"
  if [[ -n "$timestamps_file" ]]; then
    timestamps_source_path="$timestamps_file"
    [[ "$timestamps_source_path" == /* ]] || timestamps_source_path="$invocation_dir/$timestamps_source_path"
    [[ -f "$timestamps_source_path" && ! -L "$timestamps_source_path" ]] || die "timestamp sidecar must be a local regular, non-symlink file"
    timestamps_source_path="$(cd -- "$(dirname -- "$timestamps_source_path")" && pwd -P)/$(basename -- "$timestamps_source_path")"
    timestamps_path="$output_dir/presentation-timestamps-ns.txt"
    cp -- "$timestamps_source_path" "$timestamps_path"
    timestamp_sidecar_sha256="$(sha256sum -- "$timestamps_path" | awk '{print $1}')"
    timestamp_sidecar_json="\"$timestamp_sidecar_sha256\""
    temporal_basis="presentation_timestamp_ns"
    source_media_pts_preserved="false"
    source_pts_origin_ns_json="null"
    source_pts_time_base_json="null"
    extraction_method="ordered P6 PPM input; caller timestamp sidecar copied as supplied; no resizing"
  fi
fi

shopt -s nullglob
ppm_files=("$frame_dir"/frame-*.ppm)
((${#ppm_files[@]} >= 3)) || die "capture yielded fewer than three frames"
((${#ppm_files[@]} <= 1800)) || die "capture exceeds auditor's 1,800-frame limit"
if [[ "$mode" == "video" ]] && (( ${#ppm_files[@]} > max_video_frames )); then
  die "video exceeds the 500-million-pixel audit budget; shorten the recording or lower capture resolution"
fi
if [[ "$mode" == "video" ]]; then
  timestamp_count="$(wc -l < "$timestamps_path")"
  [[ "$timestamp_count" -eq "${#ppm_files[@]}" ]] || die "timestamp count does not match extracted frame count"
fi

(
  cd "$output_dir"
  sha256sum frames/frame-*.ppm > frames.sha256
  sha256sum --check frames.sha256 >/dev/null
)

if [[ -n "$timestamps_path" ]]; then
  cargo run --manifest-path validation/frame-audit/Cargo.toml --locked --release -- \
    --timestamps "$timestamps_path" --max-gap-ms "$max_gap_ms" "${ppm_files[@]}" > "$output_dir/audit.json"
  expected_audit_schema="captured-frame-audit-v4-timestamped"
else
  cargo run --manifest-path validation/frame-audit/Cargo.toml --locked --release -- \
    --fps "$fps" "${ppm_files[@]}" > "$output_dir/audit.json"
  expected_audit_schema="captured-frame-audit-v3"
fi

audit_schema="$(jq -r '.schema // empty' "$output_dir/audit.json")"
[[ "$audit_schema" == "$expected_audit_schema" ]] || die "unexpected auditor receipt schema"
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
  --arg temporal_basis "$temporal_basis" \
  --argjson timestamp_sidecar_sha256 "$timestamp_sidecar_json" \
  --argjson max_gap_ms "$max_gap_ms" \
  --argjson source_media_pts_preserved "$source_media_pts_preserved" \
  --argjson source_pts_origin_ns "$source_pts_origin_ns_json" \
  --argjson source_pts_time_base "$source_pts_time_base_json" \
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
    sample_fps_assumed: (if $temporal_basis == "nominal_fps" then $fps else null end),
    temporal_basis: $temporal_basis,
    declared_max_sample_gap_ms: $max_gap_ms,
    timestamp_sidecar_sha256: $timestamp_sidecar_sha256,
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
    timestamp_timeline_analyzed: ($temporal_basis == "presentation_timestamp_ns"),
    source_media_pts_preserved: $source_media_pts_preserved,
    source_pts_origin_ns: $source_pts_origin_ns,
    source_pts_time_base: $source_pts_time_base,
    normative_wcag_conformance: false,
    limitations: [
      (if $temporal_basis == "nominal_fps" then
        "The auditor assumes uniformly spaced samples at sample_fps_assumed."
       elif $source_media_pts_preserved then
        "Source PTS are converted through the recorded filter time base and normalized to source_pts_origin_ns; sampled frames can still miss intervening events."
       else
        "The auditor uses the caller-provided timestamp sidecar; a sampling-gap threshold pass does not prove all intervening display events were observed."
       end),
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
  [[ ! -f presentation-timestamps-ns.txt ]] || check_files+=(presentation-timestamps-ns.txt)
  sha256sum "${check_files[@]}" > evidence.sha256
  sha256sum --check evidence.sha256 >/dev/null
)
chmod -R go-rwx -- "$output_dir"
printf 'Evidence bundle created: %s\n' "$output_dir"
printf 'Reviewed source: %s\nBuild: %s\n' "$source_commit" "$build_commit"
printf 'Frames: %s at assumed %s Hz; audit is explicitly non-normative.\n' "${#ppm_files[@]}" "$fps"
