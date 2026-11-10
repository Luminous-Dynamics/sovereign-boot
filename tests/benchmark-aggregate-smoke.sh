#!/usr/bin/env bash
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
cd "$root"
command -v jq >/dev/null 2>&1 || { echo "jq is required" >&2; exit 2; }

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
workloads='[
  "checkpoint_encode_decode",
  "cpu_reference_capture_320",
  "face_mesh_build_validate",
  "loop_control",
  "mesh_build_validate",
  "mesh_lod_planning",
  "sealed_evidence_capture_128",
  "scene_advance_60hz",
  "scene_retarget_1hz_at_60fps",
  "semantic_landmarks",
  "trace_decode_replay"
]'

for trial in 1 2; do
  jq -n -c --argjson trial "$trial" --argjson workloads "$workloads" '
    ({type:"metadata", schema:"luminous-ouroboros-cpu-benchmark-v2", source_rev:"fixture", profile:"release"})
    ,
    ($workloads | map({
      type:"measurement",
      name:.,
      iterations:3,
      p50_ns:(50 + $trial * 10),
      mean_ns:(75 + $trial * 10),
      p95_ns:(100 + $trial * 20),
      p99_ns:(200 + $trial * 20)
    }))
  ' > "$tmp/trial-$trial.jsonl"
done

jq -s --argjson expected_trials 2 -f benchmarks/ouroboros/aggregate.jq \
  "$tmp/trial-1.jsonl" "$tmp/trial-2.jsonl" > "$tmp/aggregate.json"

jq -e '
  length == 11
  and all(.[]; .trial_count == 2)
  and all(.[]; .median_trial_p50_ns == 65)
  and all(.[]; .median_trial_p95_ns == 130)
  and all(.[]; .median_trial_p99_ns == 230)
  and all(.[]; .p95_trial_spread_ns == 20)
' "$tmp/aggregate.json" > /dev/null

echo "ouroboros-benchmark-aggregate: PASS (11 workloads, 2 independent trials)"
