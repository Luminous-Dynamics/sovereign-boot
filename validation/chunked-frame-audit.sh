#!/usr/bin/env bash
# Bounded timestamp-aware chunking for long P6 PPM sequences.
# Each later chunk carries a one-second timestamp overlap plus a predecessor
# frame, and subtracts a prefix receipt to avoid double-counting the overlap.
set -euo pipefail
umask 077

MAX_FRAMES_TOTAL=10000
MAX_FRAME_BYTES=25000000
MAX_TOTAL_INPUT_BYTES=50000000000
MAX_TIMESTAMP_FILE_BYTES=300000
DEFAULT_CHUNK_FRAMES=60
ONE_SECOND_NS=1000000000

usage() {
  cat <<'USAGE'
Usage:
  validation/chunked-frame-audit.sh --output-dir DIR --timestamps FILE \
    [--max-gap-ms 100] [--chunk-frames 60] \
    --frames FRAME-000001.ppm FRAME-000002.ppm FRAME-000003.ppm [...]

The timestamp sidecar contains one strictly increasing unsigned nanosecond
timestamp per input frame. Original PPM files remain caller-owned; the evidence
bundle records their ordered hashes instead of copying the entire corpus.
USAGE
}

die() { printf 'chunked-frame-audit: %s\n' "$*" >&2; exit 2; }

invocation_dir="$(pwd -P)"
output_arg=""
timestamps_arg=""
max_gap_ms="100"
chunk_frames="$DEFAULT_CHUNK_FRAMES"
frames=()

while (($#)); do
  case "$1" in
    --output-dir)
      (($# >= 2)) || die "--output-dir requires a path"
      output_arg="$2"
      shift 2
      ;;
    --timestamps)
      (($# >= 2)) || die "--timestamps requires a path"
      timestamps_arg="$2"
      shift 2
      ;;
    --max-gap-ms)
      (($# >= 2)) || die "--max-gap-ms requires a value"
      max_gap_ms="$2"
      shift 2
      ;;
    --chunk-frames)
      (($# >= 2)) || die "--chunk-frames requires a value"
      chunk_frames="$2"
      shift 2
      ;;
    --frames)
      shift
      while (($#)); do
        [[ "$1" != --* ]] || die "--frames must be the final option"
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
[[ -n "$timestamps_arg" ]] || die "--timestamps is required"
((${#frames[@]} >= 3)) || die "--frames requires at least three ordered PPM files"
((${#frames[@]} <= MAX_FRAMES_TOTAL)) || die "frame count exceeds the $MAX_FRAMES_TOTAL-frame extended-mode bound"
[[ "$max_gap_ms" =~ ^[0-9]+$ && ${#max_gap_ms} -le 5 ]] || die "--max-gap-ms must be an integer from 1 to 60000"
((10#$max_gap_ms >= 1 && 10#$max_gap_ms <= 60000)) || die "--max-gap-ms must be an integer from 1 to 60000"
max_gap_ms=$((10#$max_gap_ms))
[[ "$chunk_frames" =~ ^[0-9]+$ && ${#chunk_frames} -le 4 ]] || die "--chunk-frames must be an integer from 3 to 1800"
((10#$chunk_frames >= 3 && 10#$chunk_frames <= 1800)) || die "--chunk-frames must be an integer from 3 to 1800"
chunk_frames=$((10#$chunk_frames))

for command_name in git cargo rustc jq sha256sum realpath stat; do
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
[[ -z "$(find "$output_dir" -mindepth 1 -maxdepth 1 -print -quit 2>/dev/null || true)" ]] || die "output directory must be empty"
mkdir -p -- "$output_dir"
chmod 700 -- "$output_dir"
mkdir -m 700 -- "$output_dir/chunks"

timestamps_path="$timestamps_arg"
[[ "$timestamps_path" == /* ]] || timestamps_path="$invocation_dir/$timestamps_path"
[[ -f "$timestamps_path" && ! -L "$timestamps_path" ]] || die "timestamp sidecar must be a local regular, non-symlink file"
timestamps_path="$(cd -- "$(dirname -- "$timestamps_path")" && pwd -P)/$(basename -- "$timestamps_path")"
timestamp_bytes="$(stat -c '%s' -- "$timestamps_path")"
((timestamp_bytes <= MAX_TIMESTAMP_FILE_BYTES)) || die "timestamp sidecar exceeds the $MAX_TIMESTAMP_FILE_BYTES-byte bound"
mapfile -t raw_timestamps < "$timestamps_path"
((${#raw_timestamps[@]} == ${#frames[@]})) || die "timestamp count does not match frame count"

declare -a timestamps=()
previous_timestamp=-1
: > "$output_dir/presentation-timestamps-ns.txt"
for raw_timestamp in "${raw_timestamps[@]}"; do
  [[ "$raw_timestamp" =~ ^[0-9]+$ ]] || die "every timestamp line must be unsigned integer nanoseconds"
  [[ ${#raw_timestamp} -le 18 ]] || die "timestamp exceeds the 18-digit planner bound; normalize to a clip-relative timeline"
  normalized_timestamp="$raw_timestamp"
  while [[ "$normalized_timestamp" == 0* && ${#normalized_timestamp} -gt 1 ]]; do
    normalized_timestamp="${normalized_timestamp#0}"
  done
  timestamp_value=$((10#$normalized_timestamp))
  ((timestamp_value > previous_timestamp)) || die "timestamps must be strictly increasing"
  timestamps+=("$timestamp_value")
  printf '%s\n' "$timestamp_value" >> "$output_dir/presentation-timestamps-ns.txt"
  previous_timestamp="$timestamp_value"
done
cp -- "$timestamps_path" "$output_dir/source-timestamps.txt"
source_timestamp_sha256="$(sha256sum -- "$output_dir/source-timestamps.txt" | awk '{print $1}')"
analyzed_timestamp_sha256="$(sha256sum -- "$output_dir/presentation-timestamps-ns.txt" | awk '{print $1}')"

declare -a normalized_frames=()
declare -a frame_hashes=()
total_input_bytes=0
: > "$output_dir/input-frames.manifest"
for ((frame_index = 0; frame_index < ${#frames[@]}; frame_index++)); do
  frame_path="${frames[frame_index]}"
  [[ "$frame_path" == /* ]] || frame_path="$invocation_dir/$frame_path"
  [[ -f "$frame_path" && ! -L "$frame_path" ]] || die "frame input must be a local regular, non-symlink file"
  frame_path="$(cd -- "$(dirname -- "$frame_path")" && pwd -P)/$(basename -- "$frame_path")"
  frame_size="$(stat -c '%s' -- "$frame_path")"
  [[ "$frame_size" =~ ^[0-9]+$ ]] || die "could not determine input frame size"
  ((frame_size <= MAX_FRAME_BYTES)) || die "input frame exceeds $MAX_FRAME_BYTES bytes"
  total_input_bytes=$((total_input_bytes + frame_size))
  ((total_input_bytes <= MAX_TOTAL_INPUT_BYTES)) || die "input frame corpus exceeds $MAX_TOTAL_INPUT_BYTES aggregate bytes"
  frame_hash="$(sha256sum -- "$frame_path" | awk '{print $1}')"
  normalized_frames+=("$frame_path")
  frame_hashes+=("$frame_hash")
  printf '%08d %s\n' "$frame_index" "$frame_hash" >> "$output_dir/input-frames.manifest"
done
frame_manifest_sha256="$(sha256sum -- "$output_dir/input-frames.manifest" | awk '{print $1}')"

temporary_cargo_target=""
if [[ -z "${CARGO_TARGET_DIR:-}" ]]; then
  temporary_cargo_target="$(mktemp -d "${TMPDIR:-/tmp}/sovereign-chunked-frame-audit-target.XXXXXX")"
  export CARGO_TARGET_DIR="$temporary_cargo_target"
  trap 'if [[ -n "${temporary_cargo_target:-}" ]]; then rm -rf -- "$temporary_cargo_target"; fi' EXIT
fi
cargo build --manifest-path validation/frame-audit/Cargo.toml --locked --release
auditor_bin="$CARGO_TARGET_DIR/release/sovereign-frame-audit"
[[ -x "$auditor_bin" ]] || die "compiled independent frame auditor not found"
audit_started_epoch_ns="$(date +%s%N)"

run_audit() {
  local start_index="$1"
  local end_index="$2"
  local output_receipt="$3"
  local sidecar_path="$4"
  local count="$((end_index - start_index))"
  local index
  local -a chunk_paths=()
  : > "$sidecar_path"
  for ((index = start_index; index < end_index; index++)); do
    printf '%s\n' "${timestamps[index]}" >> "$sidecar_path"
  done
  chunk_paths=("${normalized_frames[@]:$start_index:$count}")
  "$auditor_bin" --timestamps "$sidecar_path" --max-gap-ms "$max_gap_ms" "${chunk_paths[@]}" > "$output_receipt"
  jq -e --arg source "$source_commit" --arg build "$build_commit" --argjson frames "$count" '
    .schema == "captured-frame-audit-v4-timestamped"
    and .source_commit == $source and .build_commit == $build
    and .frames_examined == $frames
    and .normative_wcag_conformance == false
  ' "$output_receipt" >/dev/null || die "chunk receipt validation failed"
}

records_path="$output_dir/chunk-records.jsonl"
: > "$records_path"
frame_count="${#normalized_frames[@]}"
emit_start=0
chunk_index=0
while ((emit_start < frame_count)); do
  if ((emit_start == 0)); then
    chunk_start=0
  else
    boundary_ns="${timestamps[emit_start]}"
    threshold_ns=$((boundary_ns - ONE_SECOND_NS))
    first_in_window="$emit_start"
    while ((first_in_window > 0)); do
      candidate_index=$((first_in_window - 1))
      candidate_ns="${timestamps[candidate_index]}"
      if ((candidate_ns >= threshold_ns)); then
        first_in_window="$candidate_index"
      else
        break
      fi
    done
    chunk_start="$first_in_window"
    if ((chunk_start > 0)); then chunk_start=$((chunk_start - 1)); fi
    prefix_length=$((emit_start - chunk_start))
    ((prefix_length >= 3)) || die "fewer than three samples cover the required overlap; densify capture or use a monolithic audit"
  fi

  overlap_length=$((emit_start - chunk_start))
  new_capacity=$((chunk_frames - overlap_length))
  ((new_capacity >= 1)) || die "chunk limit cannot hold the required overlap; increase --chunk-frames or lower sample cadence"
  chunk_end=$((emit_start + new_capacity))
  ((chunk_end <= frame_count)) || chunk_end="$frame_count"
  chunk_length=$((chunk_end - chunk_start))
  ((chunk_length >= 3)) || die "derived chunk is shorter than the three-frame minimum"

  printf -v chunk_name '%05d' "$chunk_index"
  full_receipt="$output_dir/chunks/full-$chunk_name.json"
  full_sidecar="$temporary_cargo_target/timestamps-full-$chunk_name.txt"
  run_audit "$chunk_start" "$chunk_end" "$full_receipt" "$full_sidecar"
  full_hash="$(sha256sum -- "$full_receipt" | awk '{print $1}')"

  prefix_hash="null"
  prefix_receipt=""
  if ((emit_start > 0)); then
    prefix_receipt="$output_dir/chunks/prefix-$chunk_name.json"
    prefix_sidecar="$temporary_cargo_target/timestamps-prefix-$chunk_name.txt"
    run_audit "$chunk_start" "$emit_start" "$prefix_receipt" "$prefix_sidecar"
    prefix_hash="$(sha256sum -- "$prefix_receipt" | awk '{print $1}')"
    jq -cn \
      --argjson chunk_index "$chunk_index" \
      --argjson chunk_start_index "$chunk_start" \
      --argjson emit_start_index "$emit_start" \
      --argjson emit_end_index_exclusive "$chunk_end" \
      --arg full_receipt_sha256 "$full_hash" \
      --arg prefix_receipt_sha256 "$prefix_hash" \
      --slurpfile full "$full_receipt" \
      --slurpfile prefix "$prefix_receipt" \
      '{
        chunk_index:$chunk_index,
        chunk_start_index:$chunk_start_index,
        emit_start_index:$emit_start_index,
        emit_end_index_exclusive:$emit_end_index_exclusive,
        full_receipt_sha256:$full_receipt_sha256,
        prefix_receipt_sha256:$prefix_receipt_sha256,
        full:$full[0],
        prefix:$prefix[0]
      }' >> "$records_path"
  else
    jq -cn \
      --argjson chunk_index "$chunk_index" \
      --argjson chunk_start_index "$chunk_start" \
      --argjson emit_start_index "$emit_start" \
      --argjson emit_end_index_exclusive "$chunk_end" \
      --arg full_receipt_sha256 "$full_hash" \
      --slurpfile full "$full_receipt" \
      '{
        chunk_index:$chunk_index,
        chunk_start_index:$chunk_start_index,
        emit_start_index:$emit_start_index,
        emit_end_index_exclusive:$emit_end_index_exclusive,
        full_receipt_sha256:$full_receipt_sha256,
        prefix_receipt_sha256:null,
        full:$full[0],
        prefix:null
      }' >> "$records_path"
  fi

  emit_start="$chunk_end"
  chunk_index=$((chunk_index + 1))
done
chunk_records_sha256="$(sha256sum -- "$records_path" | awk '{print $1}')"

jq -n \
  --slurpfile records "$records_path" \
  --arg source_commit "$source_commit" \
  --arg build_commit "$build_commit" \
  --arg frame_manifest_sha256 "$frame_manifest_sha256" \
  --arg source_timestamp_sha256 "$source_timestamp_sha256" \
  --arg analyzed_timestamp_sha256 "$analyzed_timestamp_sha256" \
  --arg chunk_records_sha256 "$chunk_records_sha256" \
  --argjson frame_count "$frame_count" \
  --argjson max_gap_ms "$max_gap_ms" \
  --argjson chunk_frame_limit "$chunk_frames" \
  '
    ($records) as $runs
    | [$runs[].full] as $fulls
    | ($runs | map(.full.frame_transitions_examined - (.prefix.frame_transitions_examined // 0))) as $transition_deltas
    | ($runs | map(.full.large_local_area_general_flash_transitions - (.prefix.large_local_area_general_flash_transitions // 0))) as $general_deltas
    | ($runs | map(.full.large_local_area_red_flash_transitions - (.prefix.large_local_area_red_flash_transitions // 0))) as $red_deltas
    | ($runs | map(.full.one_second_window_summary.windows_examined - (.prefix.one_second_window_summary.windows_examined // 0))) as $window_deltas
    | ($runs | map(.full.one_second_window_summary.general_windows_over_three_flash_events - (.prefix.one_second_window_summary.general_windows_over_three_flash_events // 0))) as $general_window_deltas
    | ($runs | map(.full.one_second_window_summary.red_windows_over_three_flash_events - (.prefix.one_second_window_summary.red_windows_over_three_flash_events // 0))) as $red_window_deltas
    | ([ $fulls[].timeline.max_sample_gap_ns ] | max) as $max_gap_ns
    | ($frame_count * $fulls[0].resolution.width * $fulls[0].resolution.height) as $unique_source_pixel_work
    | ([$runs[] | ((.full.frames_examined + (.prefix.frames_examined // 0)) * .full.resolution.width * .full.resolution.height)] | add) as $audit_pixel_work
    | ([$fulls[] | (.frames_examined * .resolution.width * .resolution.height)] | max) as $max_chunk_pixel_work
    | {
        schema:"captured-frame-audit-v5-chunked",
        source_commit:$source_commit,
        build_commit:$build_commit,
        source:"ordered_external_p6_ppm_frame_sequence",
        temporal_basis:"monotonic_presentation_timestamps_ns",
        chunking_method:"one_second_timestamp_overlap_plus_predecessor_frame_with_prefix_delta_accounting",
        frames_examined:$frame_count,
        frame_transitions_examined:($transition_deltas | add),
        unique_source_sampled_pixels:$unique_source_pixel_work,
        audit_sampled_pixels_processed:$audit_pixel_work,
        redundant_overlap_pixels:($audit_pixel_work - $unique_source_pixel_work),
        max_chunk_sampled_pixels:$max_chunk_pixel_work,
        resolution:$fulls[0].resolution,
        timeline:{
          start_ns:$fulls[0].timeline.start_ns,
          end_ns:$fulls[-1].timeline.end_ns,
          observed_span_ns:($fulls[-1].timeline.end_ns - $fulls[0].timeline.start_ns),
          max_sample_gap_ns:$max_gap_ns,
          allowed_sample_gap_ns:($max_gap_ms * 1000000),
          sampling_gap_gate_passed:($max_gap_ns <= ($max_gap_ms * 1000000)),
          coverage_status:(if $max_gap_ns <= ($max_gap_ms * 1000000) then "within_declared_gap_limit" else "incomplete_sampling_due_to_gap" end)
        },
        max_general_flash_area_ratio:([$fulls[].max_general_flash_area_ratio] | max),
        max_red_flash_area_ratio:([$fulls[].max_red_flash_area_ratio] | max),
        local_visual_field_proxy:{
          method:"scaled_one_third_local_raster_window",
          width:$fulls[0].local_visual_field_proxy.width,
          height:$fulls[0].local_visual_field_proxy.height,
          max_general_flash_area_ratio:([$fulls[].local_visual_field_proxy.max_general_flash_area_ratio] | max),
          max_red_flash_area_ratio:([$fulls[].local_visual_field_proxy.max_red_flash_area_ratio] | max)
        },
        large_local_area_general_flash_transitions:($general_deltas | add),
        large_local_area_red_flash_transitions:($red_deltas | add),
        one_second_window_summary:{
          basis:"presentation_timestamp_ns",
          windows_examined:($window_deltas | add),
          max_general_large_area_flash_events:([$fulls[].one_second_window_summary.max_general_large_area_flash_events] | max),
          general_windows_over_three_flash_events:($general_window_deltas | add),
          max_red_large_area_flash_events:([$fulls[].one_second_window_summary.max_red_large_area_flash_events] | max),
          red_windows_over_three_flash_events:($red_window_deltas | add)
        },
        chunked_accounting:{
          chunk_count:($runs | length),
          chunk_frame_limit:$chunk_frame_limit,
          transition_count_matches_full_sequence:(($transition_deltas | add) == ($frame_count - 1)),
          all_prefix_deltas_nonnegative:(([ $transition_deltas[], $general_deltas[], $red_deltas[], $window_deltas[], $general_window_deltas[], $red_window_deltas[] ] | all(. >= 0)))
        },
        input_frame_manifest_sha256:$frame_manifest_sha256,
        source_timestamp_sidecar_sha256:$source_timestamp_sha256,
        analyzed_timestamp_sidecar_sha256:$analyzed_timestamp_sha256,
        chunk_record_manifest_sha256:$chunk_records_sha256,
        normative_wcag_conformance:false
      }
  ' > "$output_dir/audit.json"

jq -e '
  .schema == "captured-frame-audit-v5-chunked"
  and .normative_wcag_conformance == false
  and .chunked_accounting.transition_count_matches_full_sequence == true
  and .chunked_accounting.all_prefix_deltas_nonnegative == true
' "$output_dir/audit.json" >/dev/null || die "merged chunk accounting did not reconcile; refusing a pass-like summary"

for ((frame_index = 0; frame_index < ${#normalized_frames[@]}; frame_index++)); do
  current_hash="$(sha256sum -- "${normalized_frames[frame_index]}" | awk '{print $1}')"
  [[ "$current_hash" == "${frame_hashes[frame_index]}" ]] || die "input frame changed during audit at ordered index $frame_index"
done
[[ "$(sha256sum -- "$timestamps_path" | awk '{print $1}')" == "$(sha256sum -- "$output_dir/source-timestamps.txt" | awk '{print $1}')" ]] || die "timestamp sidecar changed during audit"

audit_finished_epoch_ns="$(date +%s%N)"
elapsed_wall_ms=$(((audit_finished_epoch_ns - audit_started_epoch_ns) / 1000000))
((elapsed_wall_ms >= 0)) || die "wall clock moved backwards; benchmark duration is invalid"
audit_sha256="$(sha256sum -- "$output_dir/audit.json" | awk '{print $1}')"
jq -n \
  --arg source_commit "$source_commit" \
  --arg build_commit "$build_commit" \
  --arg frame_manifest_sha256 "$frame_manifest_sha256" \
  --arg source_timestamp_sha256 "$source_timestamp_sha256" \
  --arg analyzed_timestamp_sha256 "$analyzed_timestamp_sha256" \
  --arg chunk_records_sha256 "$chunk_records_sha256" \
  --arg audit_receipt_sha256 "$audit_sha256" \
  --argjson frame_count "$frame_count" \
  --argjson chunk_count "$chunk_index" \
  --argjson chunk_frame_limit "$chunk_frames" \
  --argjson elapsed_wall_ms "$elapsed_wall_ms" \
  --slurpfile audit "$output_dir/audit.json" \
  '{
    schema:"chunked-capture-benchmark-v1",
    source_commit:$source_commit,
    build_commit:$build_commit,
    measurement:"shell_wall_clock_including_chunk_audits_and_final_input_rehash_excluding_cargo_build",
    frame_count:$frame_count,
    chunk_count:$chunk_count,
    chunk_frame_limit:$chunk_frame_limit,
    unique_source_sampled_pixels:$audit[0].unique_source_sampled_pixels,
    audit_sampled_pixels_processed:$audit[0].audit_sampled_pixels_processed,
    redundant_overlap_pixels:$audit[0].redundant_overlap_pixels,
    max_chunk_sampled_pixels:$audit[0].max_chunk_sampled_pixels,
    elapsed_wall_ms:$elapsed_wall_ms,
    input_frame_manifest_sha256:$frame_manifest_sha256,
    source_timestamp_sidecar_sha256:$source_timestamp_sha256,
    analyzed_timestamp_sidecar_sha256:$analyzed_timestamp_sha256,
    chunk_record_manifest_sha256:$chunk_records_sha256,
    audit_receipt_sha256:$audit_receipt_sha256
  }' > "$output_dir/benchmark.json"
benchmark_sha256="$(sha256sum -- "$output_dir/benchmark.json" | awk '{print $1}')"
jq -n \
  --arg source_commit "$source_commit" \
  --arg build_commit "$build_commit" \
  --arg source_timestamp_sha256 "$source_timestamp_sha256" \
  --arg analyzed_timestamp_sha256 "$analyzed_timestamp_sha256" \
  --arg frame_manifest_sha256 "$frame_manifest_sha256" \
  --arg chunk_records_sha256 "$chunk_records_sha256" \
  --arg audit_sha256 "$audit_sha256" \
  --arg benchmark_sha256 "$benchmark_sha256" \
  --arg created_utc "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
  --argjson frame_count "$frame_count" \
  --argjson chunk_count "$chunk_index" \
  --argjson chunk_frames "$chunk_frames" \
  --argjson max_gap_ms "$max_gap_ms" \
  '{
    schema:"chunked-capture-provenance-v1",
    source_commit:$source_commit,
    build_commit:$build_commit,
    bundle_created_at_utc:$created_utc,
    input_kind:"ordered_ppm_sequence",
    frame_count:$frame_count,
    chunk_count:$chunk_count,
    chunk_frame_limit:$chunk_frames,
    temporal_basis:"presentation_timestamp_ns",
    declared_max_sample_gap_ms:$max_gap_ms,
    source_timestamp_sidecar_sha256:$source_timestamp_sha256,
    analyzed_timestamp_sidecar_sha256:$analyzed_timestamp_sha256,
    input_frame_manifest_sha256:$frame_manifest_sha256,
    chunk_record_manifest_sha256:$chunk_records_sha256,
    audit_receipt_sha256:$audit_sha256,
    benchmark_receipt_sha256:$benchmark_sha256,
    source_paths_recorded:false,
    normative_wcag_conformance:false,
    limitations:[
      "The source PPM corpus is not copied into the evidence bundle; retain it and verify input-frames.manifest against the original files.",
      "One-second overlap plus a predecessor frame preserves observed opposing-edge history only when the timestamp gap contract is satisfied.",
      "A sampling-gap threshold pass does not prove all intervening display events were observed.",
      "This regression proxy is not a normative WCAG conformance test or a physical-display measurement."
    ]
  }' > "$output_dir/capture-manifest.json"

(
  cd "$output_dir"
  check_files=(audit.json benchmark.json capture-manifest.json chunk-records.jsonl input-frames.manifest source-timestamps.txt presentation-timestamps-ns.txt)
  while IFS= read -r receipt; do check_files+=("$receipt"); done < <(find chunks -maxdepth 1 -type f -name '*.json' -print | sort)
  sha256sum "${check_files[@]}" > evidence.sha256
  sha256sum --check evidence.sha256 >/dev/null
)
chmod -R go-rwx -- "$output_dir"
printf 'Chunked audit evidence created: %s\n' "$output_dir"
printf 'Frames: %s; chunks: %s; chunk limit: %s\n' "$frame_count" "$chunk_index" "$chunk_frames"
printf 'Reviewed source: %s\nBuild: %s\n' "$source_commit" "$build_commit"
