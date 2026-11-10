#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/benchmark-ouroboros-linux.sh [options]

Builds the pinned Rust release benchmark once, then records independent JSONL
trials and a median-of-trial percentile summary. No display or GPU API is used.

Options:
  --iterations N    Measured operations per workload per trial (default: 1000)
  --warmup N        Warm-up operations per workload per trial (default: 250)
  --trials N        Independent process trials (default: 5)
  --output-dir DIR  Evidence output directory
  --allow-dirty     Permit a dirty source tree; manifest marks it non-comparable
  -h, --help        Show this help
USAGE
}

iterations=1000
warmup=250
trials=5
output_dir=""
allow_dirty=0

while (($#)); do
  case "$1" in
    --iterations)
      (($# >= 2)) || { echo "--iterations requires a value" >&2; exit 2; }
      iterations="$2"; shift 2 ;;
    --warmup)
      (($# >= 2)) || { echo "--warmup requires a value" >&2; exit 2; }
      warmup="$2"; shift 2 ;;
    --trials)
      (($# >= 2)) || { echo "--trials requires a value" >&2; exit 2; }
      trials="$2"; shift 2 ;;
    --output-dir)
      (($# >= 2)) || { echo "--output-dir requires a value" >&2; exit 2; }
      output_dir="$2"; shift 2 ;;
    --allow-dirty)
      allow_dirty=1; shift ;;
    -h|--help)
      usage; exit 0 ;;
    *)
      echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
  esac
done

for value in "$iterations" "$warmup" "$trials"; do
  [[ "$value" =~ ^[0-9]+$ ]] || { echo "iteration, warmup and trial counts must be integers" >&2; exit 2; }
done
((iterations >= 1 && iterations <= 100000)) || { echo "--iterations must be in 1..=100000" >&2; exit 2; }
((warmup <= 100000)) || { echo "--warmup must be <=100000" >&2; exit 2; }
((trials >= 2 && trials <= 25)) || { echo "--trials must be in 2..=25 for a comparative baseline" >&2; exit 2; }

command -v jq >/dev/null 2>&1 || { echo "jq is required to validate and aggregate benchmark JSONL" >&2; exit 2; }
command -v sha256sum >/dev/null 2>&1 || { echo "sha256sum is required to seal raw benchmark artifacts" >&2; exit 2; }
command -v cargo >/dev/null 2>&1 || { echo "cargo is required" >&2; exit 2; }

root="$(git rev-parse --show-toplevel)"
cd "$root"
source_rev="$(git rev-parse HEAD)"
dirty=0
if [[ -n "$(git status --porcelain --untracked-files=normal)" ]]; then
  dirty=1
fi
if ((dirty && allow_dirty == 0)); then
  echo "source tree is dirty; commit the benchmark source or pass --allow-dirty (non-comparable)" >&2
  exit 2
fi

if [[ -z "$output_dir" ]]; then
  output_dir="$root/artifacts/benchmarks/ouroboros/$source_rev"
elif [[ "$output_dir" != /* ]]; then
  output_dir="$root/$output_dir"
fi
mkdir -p "$output_dir"
output_dir="$(cd "$output_dir" && pwd)"

cpu_model="\${BENCH_CPU_MODEL:-$(awk -F: '/model name[[:space:]]*:/ {sub(/^[[:space:]]+/, "", $2); print $2; exit}' /proc/cpuinfo 2>/dev/null || true)}"
[[ -n "$cpu_model" ]] || cpu_model="not-provided"
cpu_governor="\${BENCH_CPU_GOVERNOR:-$(find /sys/devices/system/cpu -path '*/cpufreq/scaling_governor' -type f -print0 2>/dev/null | xargs -0 -r cat | sort -u | paste -sd, -)}"
[[ -n "$cpu_governor" ]] || cpu_governor="not-provided"
power_profile="\${BENCH_POWER_PROFILE:-}"
if [[ -z "$power_profile" ]] && command -v powerprofilesctl >/dev/null 2>&1; then
  power_profile="$(powerprofilesctl get 2>/dev/null || true)"
fi
[[ -n "$power_profile" ]] || power_profile="not-provided"
kernel="\${BENCH_KERNEL:-$(uname -srmo)}"
ram_mib="\${BENCH_RAM_MIB:-$(awk '/MemTotal:/ {print int($2 / 1024); exit}' /proc/meminfo 2>/dev/null || true)}"
[[ -n "$ram_mib" ]] || ram_mib="not-provided"
rust_toolchain="\${BENCH_RUST_TOOLCHAIN:-$(rustc +1.96.0 --version)}"
compositor="\${BENCH_COMPOSITOR:-\${XDG_CURRENT_DESKTOP:-\${XDG_SESSION_TYPE:-not-provided}}}"
gpu_model="\${BENCH_GPU_MODEL:-not-provided}"
gpu_driver="\${BENCH_GPU_DRIVER:-not-provided}"
if [[ "$gpu_model" == "not-provided" ]] && command -v nvidia-smi >/dev/null 2>&1; then
  gpu_model="$(nvidia-smi --query-gpu=name --format=csv,noheader 2>/dev/null | paste -sd';' - || true)"
  gpu_driver="\${BENCH_GPU_DRIVER:-$(nvidia-smi --query-gpu=driver_version --format=csv,noheader 2>/dev/null | paste -sd';' - || true)}"
  [[ -n "$gpu_model" ]] || gpu_model="not-provided"
  [[ -n "$gpu_driver" ]] || gpu_driver="not-provided"
fi

target_dir="$(cargo +1.96.0 metadata --format-version 1 --no-deps | jq -r '.target_directory')"
benchmark_bin="$target_dir/release/examples/ouroboros-benchmark"

echo "[1/3] Building exact-head release benchmark ($source_rev)"
cargo +1.96.0 build --release --locked -p sovereign-visual-core --example ouroboros-benchmark
[[ -x "$benchmark_bin" ]] || { echo "benchmark executable not found: $benchmark_bin" >&2; exit 1; }

echo "[2/3] Recording $trials independent trials ($warmup warm-up, $iterations measured)"
trial_files=()
for ((trial = 1; trial <= trials; trial++)); do
  trial_name="$(printf 'trial-%02d.jsonl' "$trial")"
  trial_path="$output_dir/$trial_name"
  echo "  trial $trial/$trials -> $trial_name"
  BENCH_SOURCE_REV="$source_rev" \
  BENCH_CPU_MODEL="$cpu_model" \
  BENCH_CPU_GOVERNOR="$cpu_governor" \
  BENCH_POWER_PROFILE="$power_profile" \
  BENCH_KERNEL="$kernel" \
  BENCH_RAM_MIB="$ram_mib" \
  BENCH_RUST_TOOLCHAIN="$rust_toolchain" \
  BENCH_GPU_MODEL="$gpu_model" \
  BENCH_GPU_DRIVER="$gpu_driver" \
  BENCH_COMPOSITOR="$compositor" \
    "$benchmark_bin" --iterations "$iterations" --warmup "$warmup" > "$trial_path"

  jq -e -s --argjson expected "$iterations" '
    length == 11 and
    .[0].type == "metadata" and
    .[0].schema == "luminous-ouroboros-cpu-benchmark-v2" and
    (.[1:] | all(.[]; .type == "measurement" and .iterations == $expected and .p95_ns >= .p50_ns and .p99_ns >= .p95_ns))
  ' "$trial_path" >/dev/null
  trial_files+=("$trial_path")
done

echo "[3/3] Aggregating independent trial percentiles and hashing raw evidence"
jq -n \
  --arg source_rev "$source_rev" \
  --arg dirty "$dirty" \
  --arg os_id "$(awk -F= '/^ID=/ {gsub(/"/, "", $2); print $2; exit}' /etc/os-release 2>/dev/null || echo unknown)" \
  --arg kernel "$kernel" \
  --arg cpu_model "$cpu_model" \
  --arg cpu_governor "$cpu_governor" \
  --arg power_profile "$power_profile" \
  --arg ram_mib "$ram_mib" \
  --arg rust_toolchain "$rust_toolchain" \
  --arg gpu_model "$gpu_model" \
  --arg gpu_driver "$gpu_driver" \
  --arg compositor "$compositor" \
  --argjson iterations "$iterations" \
  --argjson warmup "$warmup" \
  --argjson trials "$trials" \
  '{
    schema: "luminous-ouroboros-benchmark-trial-manifest-v1",
    source_rev: $source_rev,
    dirty_source_tree: ($dirty == "1"),
    comparable_baseline: ($dirty == "0"),
    os_id: $os_id,
    kernel: $kernel,
    cpu_model: $cpu_model,
    cpu_governor: $cpu_governor,
    power_profile: $power_profile,
    ram_mib: $ram_mib,
    rust_toolchain: $rust_toolchain,
    gpu_model: $gpu_model,
    gpu_driver: $gpu_driver,
    compositor_or_session: $compositor,
    iterations_per_workload: $iterations,
    warmup_per_workload: $warmup,
    independent_trials: $trials,
    notes: [
      "CPU/reference operations only; no GPU presentation or power claim.",
      "Record AC/battery state, thermal state, background load and display topology separately.",
      "Aggregate values are medians of per-trial percentiles, not pooled samples."
    ]
  }' > "$output_dir/manifest.json"

jq -s '
  def median:
    sort as $values
    | ($values | length) as $count
    | if $count == 0 then null
      elif ($count % 2) == 1 then $values[($count / 2 | floor)]
      else (($values[($count / 2 | floor) - 1] + $values[($count / 2 | floor)]) / 2)
      end;
  [ .[] | select(.type == "measurement") ]
  | group_by(.name)
  | map({
      workload: .[0].name,
      trial_count: length,
      median_trial_p50_ns: ([.[].p50_ns] | median),
      median_trial_p95_ns: ([.[].p95_ns] | median),
      median_trial_p99_ns: ([.[].p99_ns] | median),
      median_trial_mean_ns: ([.[].mean_ns] | median),
      minimum_observed_trial_p95_ns: ([.[].p95_ns] | min),
      maximum_observed_trial_p95_ns: ([.[].p95_ns] | max),
      p95_trial_spread_ns: (([.[].p95_ns] | max) - ([.[].p95_ns] | min))
    })
' "\${trial_files[@]}" > "$output_dir/aggregate.json"

(
  cd "$output_dir"
  sha256sum manifest.json aggregate.json trial-*.jsonl > SHA256SUMS
)

echo "Benchmark evidence written to: $output_dir"
echo "Source revision: $source_rev (dirty=$dirty)"
