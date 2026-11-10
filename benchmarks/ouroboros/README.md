# Ouroboros named-host benchmark capture

Run from a clean checkout on the machine being qualified:

\`\`\`bash
scripts/benchmark-ouroboros-linux.sh
\`\`\`

Defaults: five independent process trials, each with 250 warm-up and 1,000 measured operations per workload. The script builds the Rust 1.96 release harness once, then runs the executable directly for each trial.

Optional controls:

\`\`\`bash
scripts/benchmark-ouroboros-linux.sh --trials 7 --warmup 500 --iterations 2000
scripts/benchmark-ouroboros-linux.sh --output-dir /tmp/ouroboros-baseline
\`\`\`

A dirty source tree is rejected by default. \`--allow-dirty\` is available for investigation, but the output manifest marks it non-comparable. Do not use dirty runs for regression baselines.

Each output directory contains:

- \`trial-XX.jsonl\`: raw per-process metadata and all ten workload measurements.
- \`manifest.json\`: exact source SHA, source cleanliness, CPU/kernel/toolchain/power metadata and explicit limitations.
- \`aggregate.json\`: median of each trial's p50/p95/p99/mean, plus min/max p95 and spread. It does not pool samples across runs.
- \`SHA256SUMS\`: hashes over every raw JSONL and summary file.

## Comparison protocol

Keep each host's evidence separate. For each repeated comparison, use the same source SHA, Rust toolchain, build profile, iteration/warm-up counts and host power policy. Close or record background workloads. Record AC/battery state, thermal state, ambient conditions, monitor topology/resolution/refresh and whether the compositor is running. The built-in harness captures CPU/reference operations only; the GPU, driver and compositor fields are contextual metadata and do not imply those components were measured.

Compare matching workloads only. A CPU reference capture is not comparable to a competitor's GPU-presented frame. Report p50/p95/p99, between-trial spread and the loop-control baseline. Set regression gates only after enough matched runs establish natural variance.

## Current competitive target

The initial goal is not to beat video playback on throughput. It is to demonstrate a reproducible, resource-bounded, persistent semantic scene with deterministic replay, low per-frame CPU cost, low idle work, checkpoint recovery and explainable LOD. External comparisons must use equivalent scenes, settings, resolution, presentation path and lifecycle policy. Missing equivalence is reported as \`not comparable\`.
