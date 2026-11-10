#!/usr/bin/env python3
"""Analyze paired idle/active energy trials for Ouroboros and Linux wallpaper runtimes.

Standard-library only. This is offline research tooling, not production code.
Whole-system energy is computed from wall_total power samples minus a linearly
interpolated same-run idle baseline. CPU/package/GPU/video-engine domains are
retained as separate observations and are never summed into wall power.
"""
from __future__ import annotations

import argparse
import hashlib
import itertools
import json
import math
import random
import statistics
import sys
from collections import defaultdict
from pathlib import Path
from typing import Any

SCHEMA_VERSION = 1
DEFAULT_MIN_TRIALS = 10
DEFAULT_BOOTSTRAP_SAMPLES = 2000
DEFAULT_MAX_GAP_SECONDS = 2.0
RENDERER_ONLY = {"renderer_reported", "unknown"}


class TrialError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise TrialError(message)


def mean_or_zero(values: list[float]) -> float:
    return statistics.fmean(values) if values else 0.0


def quantile(values: list[float], q: float) -> float:
    if not values:
        raise TrialError("cannot calculate quantile of empty sample")
    ordered = sorted(values)
    position = (len(ordered) - 1) * q
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered[lower]
    fraction = position - lower
    return ordered[lower] * (1.0 - fraction) + ordered[upper] * fraction


def bootstrap_median_ci(
    values: list[float], *, samples: int, seed: int
) -> list[float] | None:
    if len(values) < 2:
        return None
    rng = random.Random(seed)
    medians = [
        statistics.median(rng.choices(values, k=len(values)))
        for _ in range(samples)
    ]
    return [quantile(medians, 0.025), quantile(medians, 0.975)]


def validate_trial(trial: dict[str, Any], line_number: int) -> None:
    prefix = f"line {line_number}"
    require(trial.get("schemaVersion") == SCHEMA_VERSION, f"{prefix}: unsupported schemaVersion")
    for key in ("trialId", "pairingBlockId"):
        require(isinstance(trial.get(key), str) and trial[key], f"{prefix}: {key} missing")
    for section in ("system", "workload", "host", "protocol", "frameOutcomes"):
        require(isinstance(trial.get(section), dict), f"{prefix}: missing object {section}")

    system = trial["system"]
    workload = trial["workload"]
    host = trial["host"]
    protocol = trial["protocol"]
    outcomes = trial["frameOutcomes"]

    for key in ("name", "version", "buildRevision"):
        require(isinstance(system.get(key), str) and system[key], f"{prefix}: system.{key} missing")
    for key in ("id", "colorSpace"):
        require(isinstance(workload.get(key), str) and workload[key], f"{prefix}: workload.{key} missing")
    require(workload["id"] in {f"W{i}" for i in range(8)}, f"{prefix}: unknown workload ID")
    for key in ("widthPx", "heightPx", "refreshMilliHz"):
        require(isinstance(workload.get(key), int) and workload[key] > 0, f"{prefix}: workload.{key} invalid")
    require(
        isinstance(host.get("fingerprintSha256"), str)
        and len(host["fingerprintSha256"]) == 64
        and all(ch in "0123456789abcdef" for ch in host["fingerprintSha256"]),
        f"{prefix}: host fingerprint missing or invalid",
    )
    for key in ("os", "kernel", "compositor", "gpu", "driver", "powerProfile", "displayMode"):
        require(isinstance(host.get(key), str) and host[key], f"{prefix}: host.{key} missing")
    for key in ("warmupSeconds", "activeDurationSeconds", "samplingRateMilliHz", "idleBeforeSeconds", "idleAfterSeconds"):
        require(isinstance(protocol.get(key), int) and protocol[key] >= 0, f"{prefix}: protocol.{key} invalid")
    require(protocol["activeDurationSeconds"] > 0, f"{prefix}: active duration must be positive")
    require(
        protocol.get("presentationSource") in {
            "wayland_presentation_time_v2", "vulkan_present_timing", "host_other", "unsupported"
        },
        f"{prefix}: invalid presentation source",
    )

    for key in ("presented", "discarded", "unknown"):
        require(isinstance(outcomes.get(key), int) and outcomes[key] >= 0, f"{prefix}: frameOutcomes.{key} invalid")
    provenance = outcomes.get("provenance")
    require(
        provenance in {
            "compositor_reported",
            "host_reported",
            "renderer_reported",
            "external_instrument",
            "unknown",
        },
        f"{prefix}: invalid presentation provenance",
    )
    if outcomes["presented"] > 0:
        require(
            provenance not in RENDERER_ONLY,
            f"{prefix}: renderer/unknown provenance cannot support a presented-frame count",
        )
        require(
            trial["protocol"]["presentationSource"] != "unsupported",
            f"{prefix}: unsupported presentation source cannot claim presented frames",
        )

    hashes = workload.get("assetHashes", {})
    require(isinstance(hashes, dict), f"{prefix}: workload.assetHashes must be an object")
    for asset_name, digest in hashes.items():
        require(
            isinstance(digest, str) and len(digest) == 64
            and all(ch in "0123456789abcdef" for ch in digest),
            f"{prefix}: invalid SHA-256 for asset {asset_name}",
        )


def analyze_trial(
    trial: dict[str, Any],
    *,
    line_number: int,
    max_gap_seconds: float,
) -> dict[str, Any]:
    validate_trial(trial, line_number)
    prefix = f"line {line_number}"

    samples = trial.get("powerSamples")
    require(isinstance(samples, list), f"{prefix}: powerSamples missing")
    wall = [sample for sample in samples if sample.get("domain") == "wall_total"]
    require(len(wall) >= 6, f"{prefix}: need at least 6 wall_total samples")

    wall.sort(key=lambda sample: sample.get("timestampMonotonicNs", -1))
    phases: dict[str, list[dict[str, Any]]] = {
        "idle_before": [],
        "active": [],
        "idle_after": [],
    }
    last_timestamp = -1
    domain_boundary: str | None = None
    instrument: str | None = None
    for sample in wall:
        timestamp = sample.get("timestampMonotonicNs")
        phase = sample.get("phase")
        power = sample.get("powerMilliwatts")
        uncertainty = sample.get("uncertaintyMilliwatts")
        require(isinstance(timestamp, int) and timestamp > last_timestamp, f"{prefix}: wall timestamps not strictly increasing")
        require(phase in phases, f"{prefix}: invalid power sample phase")
        require(isinstance(power, int) and power >= 0, f"{prefix}: invalid powerMilliwatts")
        require(isinstance(uncertainty, int) and uncertainty >= 0, f"{prefix}: invalid uncertaintyMilliwatts")
        require(isinstance(sample.get("instrument"), str) and sample["instrument"], f"{prefix}: missing power instrument")
        require(isinstance(sample.get("domainBoundary"), str) and sample["domainBoundary"], f"{prefix}: power domainBoundary required")
        if domain_boundary is None:
            domain_boundary = sample["domainBoundary"]
            instrument = sample["instrument"]
        require(sample["domainBoundary"] == domain_boundary, f"{prefix}: mixed wall_total domain boundaries")
        require(sample["instrument"] == instrument, f"{prefix}: mixed wall_total instruments")
        last_timestamp = timestamp
        phases[phase].append(sample)

    for phase, values in phases.items():
        require(len(values) >= 2, f"{prefix}: phase {phase} needs at least two wall_total samples")

    ordered_phases = [sample["phase"] for sample in wall]
    phase_order = {"idle_before": 0, "active": 1, "idle_after": 2}
    require(
        all(phase_order[a] <= phase_order[b] for a, b in zip(ordered_phases, ordered_phases[1:])),
        f"{prefix}: wall_total phases are not ordered idle_before -> active -> idle_after",
    )

    active = phases["active"]
    active_times = [sample["timestampMonotonicNs"] for sample in active]
    active_seconds = [(timestamp - active_times[0]) / 1_000_000_000 for timestamp in active_times]
    require(active_seconds[-1] > 0, f"{prefix}: active power window has zero duration")
    require(
        abs(active_seconds[-1] - trial["protocol"]["activeDurationSeconds"]) <= 2.0,
        f"{prefix}: active sample window differs from declared activeDurationSeconds by more than 2 seconds",
    )
    gaps = [b - a for a, b in zip(active_seconds, active_seconds[1:])]
    require(max(gaps) <= max_gap_seconds, f"{prefix}: active wall power sampling gap exceeds limit")

    idle_before_power = statistics.fmean(s["powerMilliwatts"] for s in phases["idle_before"])
    idle_after_power = statistics.fmean(s["powerMilliwatts"] for s in phases["idle_after"])
    idle_before_uncertainty = statistics.fmean(s["uncertaintyMilliwatts"] for s in phases["idle_before"])
    idle_after_uncertainty = statistics.fmean(s["uncertaintyMilliwatts"] for s in phases["idle_after"])

    baseline: list[float] = []
    baseline_uncertainty: list[float] = []
    for elapsed in active_seconds:
        fraction = elapsed / active_seconds[-1]
        baseline.append(
            idle_before_power * (1.0 - fraction) + idle_after_power * fraction
        )
        baseline_uncertainty.append(
            idle_before_uncertainty * (1.0 - fraction)
            + idle_after_uncertainty * fraction
        )

    marginal_power_mw = [
        float(sample["powerMilliwatts"]) - baseline_power
        for sample, baseline_power in zip(active, baseline)
    ]
    # Trapezoidal integration of the active-minus-interpolated-idle power curve.
    marginal_energy_j = 0.0
    conservative_uncertainty_j = 0.0
    for index in range(len(active) - 1):
        duration_s = (active_times[index + 1] - active_times[index]) / 1_000_000_000
        marginal_energy_j += (
            marginal_power_mw[index] + marginal_power_mw[index + 1]
        ) * 0.5 * duration_s / 1000.0
        endpoint_uncertainty_mw = (
            active[index]["uncertaintyMilliwatts"]
            + baseline_uncertainty[index]
            + active[index + 1]["uncertaintyMilliwatts"]
            + baseline_uncertainty[index + 1]
        ) * 0.5
        conservative_uncertainty_j += endpoint_uncertainty_mw * duration_s / 1000.0

    # Report other measured domains separately; do not add package/GPU/video
    # counters to wall_total because device-domain boundaries may overlap.
    domain_summary: dict[str, Any] = {}
    other_domains = sorted({sample["domain"] for sample in samples if sample.get("domain") != "wall_total"})
    for domain in other_domains:
        domain_samples = [sample for sample in samples if sample.get("domain") == domain]
        boundaries = {sample.get("domainBoundary") for sample in domain_samples}
        instruments = {sample.get("instrument") for sample in domain_samples}
        require(len(boundaries) == 1 and len(instruments) == 1, f"{prefix}: mixed source/boundary within domain {domain}")
        domain_summary[domain] = {
            "instrument": next(iter(instruments)),
            "domainBoundary": next(iter(boundaries)),
            "meanPowerMilliwattsByPhase": {
                phase: (
                    statistics.fmean(
                        sample["powerMilliwatts"]
                        for sample in domain_samples
                        if sample.get("phase") == phase
                    )
                    if any(sample.get("phase") == phase for sample in domain_samples)
                    else None
                )
                for phase in ("idle_before", "active", "idle_after")
            },
        }

    outcomes = trial["frameOutcomes"]
    presented = outcomes["presented"]
    total_terminal = presented + outcomes["discarded"]
    return {
        "trialId": trial["trialId"],
        "pairingBlockId": trial["pairingBlockId"],
        "system": trial["system"]["name"],
        "version": trial["system"]["version"],
        "buildRevision": trial["system"]["buildRevision"],
        "workloadId": trial["workload"]["id"],
        "hostFingerprintSha256": trial["host"]["fingerprintSha256"],
        "widthPx": trial["workload"]["widthPx"],
        "heightPx": trial["workload"]["heightPx"],
        "refreshMilliHz": trial["workload"]["refreshMilliHz"],
        "assetHashes": trial["workload"].get("assetHashes", {}),
        "presentationProvenance": outcomes["provenance"],
        "presentedFrames": presented,
        "discardedFrames": outcomes["discarded"],
        "unknownFrameOutcomes": outcomes["unknown"],
        "activeWindowSeconds": active_seconds[-1],
        "activeWallPowerMeanW": statistics.fmean(s["powerMilliwatts"] for s in active) / 1000.0,
        "idleBeforeMeanW": idle_before_power / 1000.0,
        "idleAfterMeanW": idle_after_power / 1000.0,
        "marginalEnergyJ": marginal_energy_j,
        "conservativeMeterUncertaintyJ": conservative_uncertainty_j,
        "joulesPerPresentedFrame": (
            marginal_energy_j / presented if presented > 0 else None
        ),
        "discardedRatio": (
            outcomes["discarded"] / total_terminal if total_terminal > 0 else None
        ),
        "presentationUnknownRatio": (
            outcomes["unknown"] / (total_terminal + outcomes["unknown"])
            if (total_terminal + outcomes["unknown"]) > 0 else None
        ),
        "negativeMarginalEnergy": marginal_energy_j < 0,
        "wallPowerInstrument": instrument,
        "wallPowerDomainBoundary": domain_boundary,
        "nonWallPowerSamplesRetainedButNotSummed": sum(
            1 for sample in samples if sample.get("domain") != "wall_total"
        ),
        "otherPowerDomainSummaries": domain_summary,
    }


def aggregate_trials(
    trials: list[dict[str, Any]],
    *,
    minimum_trials: int,
    bootstrap_samples: int,
) -> list[dict[str, Any]]:
    groups: dict[tuple[Any, ...], list[dict[str, Any]]] = defaultdict(list)
    for trial in trials:
        key = (
            trial["system"],
            trial["version"],
            trial["workloadId"],
            trial["hostFingerprintSha256"],
            trial["widthPx"],
            trial["heightPx"],
            trial["refreshMilliHz"],
            json.dumps(trial["assetHashes"], sort_keys=True),
        )
        groups[key].append(trial)

    summaries = []
    for key, records in sorted(groups.items(), key=lambda item: tuple(str(x) for x in item[0])):
        energy_values = [record["marginalEnergyJ"] for record in records]
        jpf_values = [
            record["joulesPerPresentedFrame"]
            for record in records
            if record["joulesPerPresentedFrame"] is not None
        ]
        seed_material = "|".join(str(value) for value in key).encode("utf-8")
        seed = int.from_bytes(hashlib.sha256(seed_material).digest()[:8], "little")
        summaries.append({
            "system": key[0],
            "version": key[1],
            "workloadId": key[2],
            "hostFingerprintSha256": key[3],
            "widthPx": key[4],
            "heightPx": key[5],
            "refreshMilliHz": key[6],
            "trialCount": len(records),
            "qualificationStatus": (
                "meets_initial_trial_count"
                if len(records) >= minimum_trials else "insufficient_trials"
            ),
            "medianMarginalEnergyJ": statistics.median(energy_values),
            "marginalEnergyMedian95PercentileBootstrapCI": bootstrap_median_ci(
                energy_values, samples=bootstrap_samples, seed=seed
            ),
            "medianJoulesPerPresentedFrame": (
                statistics.median(jpf_values) if jpf_values else None
            ),
            "joulesPerPresentedFrameMedian95PercentileBootstrapCI": bootstrap_median_ci(
                jpf_values, samples=bootstrap_samples, seed=seed ^ 0x9E3779B97F4A7C15
            ),
            "medianDiscardedRatio": statistics.median(
                [record["discardedRatio"] for record in records if record["discardedRatio"] is not None]
            ) if any(record["discardedRatio"] is not None for record in records) else None,
            "unknownOutcomeTrials": sum(
                1 for record in records if record["unknownFrameOutcomes"] > 0
            ),
            "negativeMarginalEnergyTrials": sum(
                1 for record in records if record["negativeMarginalEnergy"]
            ),
        })
    return summaries


def paired_comparisons(trials: list[dict[str, Any]]) -> list[dict[str, Any]]:
    blocks: dict[tuple[str, str, str, int, int, int], list[dict[str, Any]]] = defaultdict(list)
    for trial in trials:
        key = (
            trial["pairingBlockId"],
            trial["workloadId"],
            trial["hostFingerprintSha256"],
            trial["widthPx"],
            trial["heightPx"],
            trial["refreshMilliHz"],
        )
        blocks[key].append(trial)

    comparisons = []
    for key, records in sorted(blocks.items(), key=lambda item: tuple(str(x) for x in item[0])):
        by_system: dict[str, list[dict[str, Any]]] = defaultdict(list)
        for record in records:
            by_system[record["system"]].append(record)
        if len(by_system) != 2 or any(len(values) != 1 for values in by_system.values()):
            continue
        system_names = sorted(by_system)
        left = by_system[system_names[0]][0]
        right = by_system[system_names[1]][0]
        if left["assetHashes"] != right["assetHashes"]:
            continue
        comparisons.append({
            "pairingBlockId": key[0],
            "workloadId": key[1],
            "hostFingerprintSha256": key[2],
            "systemA": system_names[0],
            "systemB": system_names[1],
            "deltaMarginalEnergyJ_BminusA": (
                by_system[system_names[1]][0]["marginalEnergyJ"]
                - by_system[system_names[0]][0]["marginalEnergyJ"]
            ),
            "deltaJoulesPerPresentedFrame_BminusA": (
                right["joulesPerPresentedFrame"] - left["joulesPerPresentedFrame"]
                if left["joulesPerPresentedFrame"] is not None
                and right["joulesPerPresentedFrame"] is not None else None
            ),
        })
    return comparisons


def analyze_records(
    records: list[dict[str, Any]],
    *,
    minimum_trials: int = DEFAULT_MIN_TRIALS,
    bootstrap_samples: int = DEFAULT_BOOTSTRAP_SAMPLES,
    max_gap_seconds: float = DEFAULT_MAX_GAP_SECONDS,
) -> dict[str, Any]:
    trial_results = [
        analyze_trial(record, line_number=index + 1, max_gap_seconds=max_gap_seconds)
        for index, record in enumerate(records)
    ]
    return {
        "schemaVersion": 1,
        "status": "analysis_only_not_qualification",
        "metricDefinitions": {
            "marginalEnergyJ": "Trapezoidal integral of wall_total active power minus linearly interpolated same-trial idle-before/idle-after baseline.",
            "joulesPerPresentedFrame": "Marginal wall energy divided by compositor/host-evidenced presented frame count; never uses submitted or renderer-only counts.",
            "meterUncertainty": "Conservative bound from active-sample and interpolated idle-sample per-point uncertainty; does not include all calibration/systematic uncertainty.",
            "powerDomains": "Only wall_total contributes to whole-system energy. CPU/package/GPU/video_engine domains are retained as separate samples and never summed."
        },
        "minimumTrialsPerGroup": minimum_trials,
        "bootstrapSamples": bootstrap_samples,
        "maxActiveSamplingGapSeconds": max_gap_seconds,
        "trialResults": trial_results,
        "groupSummaries": aggregate_trials(
            trial_results,
            minimum_trials=minimum_trials,
            bootstrap_samples=bootstrap_samples,
        ),
        "pairedComparisons": paired_comparisons(trial_results),
    }


def synthetic_trial(index: int) -> dict[str, Any]:
    base_time = 0
    samples = []
    for timestamp, phase, power in [
        (0, "idle_before", 5_000),
        (2_000_000_000, "idle_before", 5_000),
        (10_000_000_000, "active", 5_200),
        (11_000_000_000, "active", 5_300),
        (12_000_000_000, "active", 5_400),
        (13_000_000_000, "idle_after", 5_200),
        (15_000_000_000, "idle_after", 5_200),
    ]:
        samples.append({
            "timestampMonotonicNs": timestamp,
            "phase": phase,
            "domain": "wall_total",
            "powerMilliwatts": power,
            "uncertaintyMilliwatts": 10,
            "instrument": "synthetic-meter",
            "domainBoundary": "whole-system-ac-input",
        })
    # Retain an independent GPU-domain observation; analyzer must not add it.
    samples.append({
        "timestampMonotonicNs": 10_000_000_000,
        "phase": "active",
        "domain": "gpu",
        "powerMilliwatts": 600,
        "uncertaintyMilliwatts": 20,
        "instrument": "synthetic-gpu-telemetry",
        "domainBoundary": "gpu-domain-only",
    })
    return {
        "schemaVersion": 1,
        "trialId": f"synthetic-{index}",
        "pairingBlockId": f"pair-{index}",
        "system": {"name": "synthetic-runtime", "version": "0", "buildRevision": "synthetic"},
        "workload": {
            "id": "W2",
            "assetHashes": {"scene": "a" * 64},
            "widthPx": 1920,
            "heightPx": 1080,
            "refreshMilliHz": 60_000,
            "colorSpace": "sRGB",
        },
        "host": {
            "fingerprintSha256": "b" * 64,
            "os": "synthetic",
            "kernel": "synthetic",
            "compositor": "synthetic",
            "gpu": "synthetic",
            "driver": "synthetic",
            "powerProfile": "synthetic",
            "displayMode": "1920x1080@60",
        },
        "protocol": {
            "warmupSeconds": 300,
            "activeDurationSeconds": 2,
            "samplingRateMilliHz": 1_000,
            "idleBeforeSeconds": 2,
            "idleAfterSeconds": 2,
            "presentationSource": "wayland_presentation_time_v2",
        },
        "powerSamples": samples,
        "frameOutcomes": {
            "presented": 100,
            "discarded": 0,
            "unknown": 0,
            "provenance": "compositor_reported",
        },
    }


def self_test() -> int:
    records = [synthetic_trial(index) for index in range(10)]
    result = analyze_records(records, minimum_trials=10, bootstrap_samples=100)
    first = result["trialResults"][0]
    assert abs(first["marginalEnergyJ"] - 0.4) < 1.0e-9, first
    assert abs(first["joulesPerPresentedFrame"] - 0.004) < 1.0e-12, first
    assert abs(first["conservativeMeterUncertaintyJ"] - 0.04) < 1.0e-9, first
    assert first["nonWallPowerSamplesRetainedButNotSummed"] == 1, first
    assert result["groupSummaries"][0]["qualificationStatus"] == "meets_initial_trial_count"
    print("self-test: PASS (synthetic only; no measured performance result)")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", nargs="?", type=Path, help="JSONL file with one complete trial record per line")
    parser.add_argument("--output", type=Path, help="write analysis JSON to this path")
    parser.add_argument("--min-trials", type=int, default=DEFAULT_MIN_TRIALS)
    parser.add_argument("--bootstrap-samples", type=int, default=DEFAULT_BOOTSTRAP_SAMPLES)
    parser.add_argument("--max-gap-seconds", type=float, default=DEFAULT_MAX_GAP_SECONDS)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        return self_test()
    if args.input is None:
        parser.error("input JSONL path is required unless --self-test is used")
    if args.min_trials < 2 or args.bootstrap_samples < 100 or args.max_gap_seconds <= 0:
        parser.error("min-trials >=2, bootstrap-samples >=100 and max-gap-seconds >0 are required")

    records = []
    try:
        with args.input.open("r", encoding="utf-8") as handle:
            for line_number, line in enumerate(handle, start=1):
                if not line.strip():
                    continue
                try:
                    records.append(json.loads(line))
                except json.JSONDecodeError as error:
                    raise TrialError(f"line {line_number}: invalid JSON: {error}") from error
        result = analyze_records(
            records,
            minimum_trials=args.min_trials,
            bootstrap_samples=args.bootstrap_samples,
            max_gap_seconds=args.max_gap_seconds,
        )
        encoded = json.dumps(result, indent=2, sort_keys=True) + "\n"
        if args.output:
            args.output.write_text(encoded, encoding="utf-8")
        else:
            sys.stdout.write(encoded)
        return 0
    except (OSError, TrialError, KeyError, TypeError) as error:
        print(f"energy analysis failed: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
