#!/usr/bin/env python3
"""Matched Gpytoolbox DCSDFD reference runner for canonical OUROGRD1 files.

This is an offline research adapter. It does not evaluate the field or create a new
SDF; it reconstructs grid positions and float64 sample values from the canonical
integer input. It records the exact upstream source commit and RNG mode.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.metadata
import json
import os
import platform
import resource
import struct
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

MAGIC = b"OUROGRD1"
REFERENCE_COMMIT = "d6e6ae3b5ff9200e687ca19c178ff35d65c1f810"
RNG_PATCH_SEED_LINE = "std::mt19937 gen(0x4F55524FU);"
UPSTREAM_RANDOM_LINE = "std::mt19937 gen(rd());"
SOURCE_RELATIVE_PATH = Path(
    "src/cpp/dual_contouring_of_signed_distance_data/contouring.cpp"
)
MAX_INPUT_BYTES = 64 * 1024 * 1024
MAX_SAMPLES = 2_097_152


class InputError(ValueError):
    pass


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def read_u16(buf: bytes, offset: int) -> tuple[int, int]:
    if offset + 2 > len(buf):
        raise InputError("truncated u16")
    return struct.unpack_from("<H", buf, offset)[0], offset + 2


def read_u32(buf: bytes, offset: int) -> tuple[int, int]:
    if offset + 4 > len(buf):
        raise InputError("truncated u32")
    return struct.unpack_from("<I", buf, offset)[0], offset + 4


def read_i64(buf: bytes, offset: int) -> tuple[int, int]:
    if offset + 8 > len(buf):
        raise InputError("truncated i64")
    return struct.unpack_from("<q", buf, offset)[0], offset + 8


def read_u64(buf: bytes, offset: int) -> tuple[int, int]:
    if offset + 8 > len(buf):
        raise InputError("truncated u64")
    return struct.unpack_from("<Q", buf, offset)[0], offset + 8


def parse_ourogrd1(path: Path, np: Any) -> dict[str, Any]:
    file_size = path.stat().st_size
    if file_size > MAX_INPUT_BYTES:
        raise InputError(f"input exceeds {MAX_INPUT_BYTES} bytes")
    raw = path.read_bytes()
    if len(raw) != file_size:
        raise InputError("input length changed while reading")
    if len(raw) < 8 or raw[:8] != MAGIC:
        raise InputError("unsupported input magic; expected OUROGRD1")

    offset = 8
    schema, offset = read_u16(raw, offset)
    if schema != 1:
        raise InputError(f"unsupported OUROGRD1 schema {schema}")

    fixture_len, offset = read_u16(raw, offset)
    if fixture_len == 0 or offset + fixture_len > len(raw):
        raise InputError("invalid fixture-id length")
    try:
        fixture_id = raw[offset : offset + fixture_len].decode("utf-8")
    except UnicodeDecodeError as exc:
        raise InputError("fixture ID is not UTF-8") from exc
    offset += fixture_len

    descriptor_len, offset = read_u32(raw, offset)
    if descriptor_len == 0 or offset + descriptor_len > len(raw):
        raise InputError("invalid field-descriptor length")
    descriptor = raw[offset : offset + descriptor_len]
    offset += descriptor_len

    if offset + 12 + 4 > len(raw):
        raise InputError("truncated dimensions/layout")
    nx, ny, nz = struct.unpack_from("<III", raw, offset)
    offset += 12
    axis_layout = tuple(raw[offset : offset + 4])
    offset += 4
    if axis_layout != (1, 2, 3, 1):
        raise InputError(f"unsupported axis/layout marker {axis_layout}")
    if min(nx, ny, nz) < 2:
        raise InputError("each grid dimension must be at least 2")

    bounds_fixed = []
    for _ in range(6):
        value, offset = read_i64(raw, offset)
        bounds_fixed.append(value)
    distance_scale, offset = read_i64(raw, offset)
    sample_count, offset = read_u64(raw, offset)

    expected_count = nx * ny * nz
    if expected_count > MAX_SAMPLES:
        raise InputError(f"grid has {expected_count} samples; limit is {MAX_SAMPLES}")
    if sample_count != expected_count:
        raise InputError(
            f"sample count mismatch: header={sample_count}, dimensions={expected_count}"
        )
    if distance_scale <= 0:
        raise InputError("distance quantization scale must be positive")
    sample_bytes = sample_count * 4
    if offset + sample_bytes != len(raw):
        raise InputError("OUROGRD1 length does not match header and sample count")

    # The first axis varies fastest: row i + nx*(j + ny*k).
    samples_i32 = np.frombuffer(raw, dtype="<i4", count=sample_count, offset=offset)
    samples = samples_i32.astype(np.float64)
    samples /= float(distance_scale)

    # Bounds are microunit integers, matching the Rust grid-coordinate function.
    minimum = [bounds_fixed[i] / 1_000_000.0 for i in range(3)]
    maximum = [bounds_fixed[i + 3] / 1_000_000.0 for i in range(3)]

    return {
        "raw": raw,
        "grid_sha256": sha256_bytes(raw),
        "fixture_id": fixture_id,
        "descriptor_sha256": sha256_bytes(descriptor),
        "dimensions": (nx, ny, nz),
        "minimum": minimum,
        "maximum": maximum,
        "distance_scale": distance_scale,
        "samples": samples,
        "sample_count": int(sample_count),
    }


def build_grid_vertices(grid: dict[str, Any], np: Any):
    nx, ny, nz = grid["dimensions"]
    minimum = grid["minimum"]
    maximum = grid["maximum"]

    # Match Rust's arithmetic order: min + (max-min)*index/(count-1).
    xs = minimum[0] + (maximum[0] - minimum[0]) * np.arange(nx, dtype=np.float64) / (nx - 1)
    ys = minimum[1] + (maximum[1] - minimum[1]) * np.arange(ny, dtype=np.float64) / (ny - 1)
    zs = minimum[2] + (maximum[2] - minimum[2]) * np.arange(nz, dtype=np.float64) / (nz - 1)

    count = nx * ny * nz
    vertices = np.empty((count, 3), dtype=np.float64)
    cursor = 0
    for k in range(nz):
        for j in range(ny):
            stop = cursor + nx
            vertices[cursor:stop, 0] = xs
            vertices[cursor:stop, 1] = ys[j]
            vertices[cursor:stop, 2] = zs[k]
            cursor = stop
    if cursor != count:
        raise InputError("internal first-axis-fastest grid writer mismatch")
    return vertices


def stable_array_hash(array: Any, np: Any) -> str:
    arr = np.asarray(array)
    if arr.dtype.kind == "f":
        canonical = np.ascontiguousarray(arr, dtype="<f8")
    elif arr.dtype.kind in "iu":
        canonical = np.ascontiguousarray(arr, dtype="<i8")
    else:
        raise TypeError(f"unsupported result dtype: {arr.dtype}")
    prefix = json.dumps(
        {"dtype": canonical.dtype.str, "shape": list(canonical.shape)},
        sort_keys=True,
        separators=(",", ":"),
    ).encode("ascii")
    return sha256_bytes(prefix + b"\0" + canonical.tobytes(order="C"))


def source_identity(root: Path, mode: str) -> dict[str, Any]:
    source = root / SOURCE_RELATIVE_PATH
    if not source.is_file():
        raise InputError(f"missing reference source: {source}")
    source_bytes = source.read_bytes()
    source_text = source_bytes.decode("utf-8")
    if mode == "upstream-unmodified":
        if UPSTREAM_RANDOM_LINE not in source_text or "std::random_device rd;" not in source_text:
            raise InputError("source does not match upstream-unmodified RNG implementation")
        if RNG_PATCH_SEED_LINE in source_text:
            raise InputError("fixed-seed patch detected in upstream-unmodified mode")
    elif mode == "fixed-seed-patched":
        if RNG_PATCH_SEED_LINE not in source_text:
            raise InputError("fixed-seed patch line not found in reference source")
        if "std::random_device rd;" in source_text or UPSTREAM_RANDOM_LINE in source_text:
            raise InputError("upstream random_device seed remains in patched source")
    else:
        raise InputError(f"unknown source mode: {mode}")

    rev = subprocess.run(
        ["git", "-C", str(root), "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if rev != REFERENCE_COMMIT:
        raise InputError(
            f"reference checkout mismatch: expected {REFERENCE_COMMIT}, got {rev}"
        )
    return {
        "repository": "sgsellan/gpytoolbox",
        "commit": rev,
        "source_mode": mode,
        "source_file": str(SOURCE_RELATIVE_PATH),
        "source_file_sha256": sha256_bytes(source_bytes),
        "rng_policy": (
            "std::random_device" if mode == "upstream-unmodified"
            else "std::mt19937(seed=0x4F55524F)"
        ),
    }


def peak_rss_bytes() -> int | None:
    try:
        usage = resource.getrusage(resource.RUSAGE_SELF)
        raw = int(usage.ru_maxrss)
        # Linux reports KiB; macOS reports bytes.
        return raw * 1024 if sys.platform.startswith("linux") else raw
    except (AttributeError, OSError, ValueError):
        return None


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--grid", required=True, type=Path, help="canonical OUROGRD1 file")
    parser.add_argument("--gpytoolbox-root", required=True, type=Path)
    parser.add_argument(
        "--mode",
        required=True,
        choices=("upstream-unmodified", "fixed-seed-patched"),
    )
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--threads", type=int, default=1)
    parser.add_argument("--outer-iters", type=int, default=100)
    parser.add_argument("--inner-iters", type=int, default=100)
    parser.add_argument("--batch-size", type=int, default=200000)
    parser.add_argument("--hermite-update", choices=("on", "off"), default="on")
    parser.add_argument("--mu", type=float, default=0.1)
    parser.add_argument("--dc-weight", type=float, default=0.02)
    parser.add_argument("--svd-threshold", type=float, default=0.01)
    parser.add_argument("--new-hermite-pos-weight", type=float, default=0.2)
    parser.add_argument("--new-face-pos-weight", type=float, default=0.2)
    parser.add_argument("--new-hermite-normal-weight", type=float, default=0.2)
    parser.add_argument("--isovalue", type=float, default=0.0)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.threads < 1 or args.outer_iters < 0 or args.inner_iters < 0 or args.batch_size < 1:
        raise SystemExit("invalid thread/iteration/batch-size parameter")
    for name, value in (
        ("mu", args.mu),
        ("dc_weight", args.dc_weight),
        ("svd_threshold", args.svd_threshold),
        ("new_hermite_pos_weight", args.new_hermite_pos_weight),
        ("new_face_pos_weight", args.new_face_pos_weight),
        ("new_hermite_normal_weight", args.new_hermite_normal_weight),
        ("isovalue", args.isovalue),
    ):
        if not isinstance(value, (int, float)) or not float("-inf") < float(value) < float("inf"):
            raise SystemExit(f"{name} must be finite")

    root = args.gpytoolbox_root.resolve()
    identity = source_identity(root, args.mode)

    # Configure thread caps before importing NumPy/Gpytoolbox native modules.
    os.environ["OMP_NUM_THREADS"] = str(args.threads)
    os.environ["OMP_DYNAMIC"] = "FALSE"
    os.environ["MKL_NUM_THREADS"] = str(args.threads)
    os.environ["OPENBLAS_NUM_THREADS"] = str(args.threads)
    sys.path.insert(0, str(root / "src"))
    sys.path.insert(0, str(root))

    try:
        import numpy as np
        import gpytoolbox as gpy
    except ImportError as exc:
        raise SystemExit(
            "Gpytoolbox and its compiled gpytoolbox_bindings extension must be installed "
            "from the pinned source checkout"
        ) from exc

    grid = parse_ourogrd1(args.grid, np)
    gv = build_grid_vertices(grid, np)
    if gv.shape != (grid["sample_count"], 3):
        raise InputError("constructed GV shape differs from the canonical sample count")

    params = {
        "isovalue": float(args.isovalue),
        "outer_iters": int(args.outer_iters),
        "inner_iters": int(args.inner_iters),
        "batch_size": int(args.batch_size),
        "hermite_update": args.hermite_update == "on",
        "mu": float(args.mu),
        "dc_weight": float(args.dc_weight),
        "svd_threshold": float(args.svd_threshold),
        "new_hermite_pos_weight": float(args.new_hermite_pos_weight),
        "new_face_pos_weight": float(args.new_face_pos_weight),
        "new_hermite_normal_weight": float(args.new_hermite_normal_weight),
        "verbose": False,
    }

    started = time.perf_counter_ns()
    vertices, quads = gpy.dual_contouring_of_signed_distance_data(
        grid["samples"], gv, *grid["dimensions"], **params
    )
    elapsed_ns = time.perf_counter_ns() - started

    vertices = np.asarray(vertices, dtype=np.float64)
    quads = np.asarray(quads)
    if vertices.ndim != 2 or vertices.shape[1] != 3:
        raise InputError(f"reference returned invalid vertex shape {vertices.shape}")
    if quads.ndim != 2 or quads.shape[1] != 4:
        raise InputError(f"reference returned invalid quad shape {quads.shape}")
    if not np.isfinite(vertices).all():
        raise InputError("reference returned non-finite vertex coordinates")
    if quads.size and (
        quads.min() < 0 or quads.max() >= len(vertices)
    ):
        raise InputError("reference returned out-of-range quad index")

    args.output_dir.mkdir(parents=True, exist_ok=True)
    vertices_path = args.output_dir / "vertices.npy"
    quads_path = args.output_dir / "quads.npy"
    np.save(vertices_path, vertices, allow_pickle=False)
    np.save(quads_path, quads.astype(np.int64, copy=False), allow_pickle=False)

    try:
        package_version = importlib.metadata.version("gpytoolbox")
    except importlib.metadata.PackageNotFoundError:
        package_version = getattr(gpy, "__version__", "unknown")

    manifest = {
        "schema_version": 1,
        "benchmark_id": "ouroboros-dcsdfd-matched-grid-v1",
        "timestamp_utc": datetime.now(timezone.utc).isoformat(),
        "reference": identity,
        "gpytoolbox_version": package_version,
        "python_version": sys.version,
        "numpy_version": np.__version__,
        "host": {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "processor": platform.processor(),
            "logical_cpu_count": os.cpu_count(),
            "threads_requested": args.threads,
        },
        "input": {
            "path": str(args.grid.resolve()),
            "magic": "OUROGRD1",
            "grid_sha256": grid["grid_sha256"],
            "fixture_id": grid["fixture_id"],
            "field_descriptor_sha256": grid["descriptor_sha256"],
            "dimensions": list(grid["dimensions"]),
            "minimum": grid["minimum"],
            "maximum": grid["maximum"],
            "distance_scale": grid["distance_scale"],
            "sample_count": grid["sample_count"],
            "layout": "first-axis-fastest",
            "resampled": False,
            "field_reevaluated": False,
        },
        "parameters": params,
        "result": {
            "vertex_count": int(len(vertices)),
            "quad_count": int(len(quads)),
            "vertices_sha256": stable_array_hash(vertices, np),
            "quads_sha256": stable_array_hash(quads, np),
            "vertices_file": vertices_path.name,
            "quads_file": quads_path.name,
            "elapsed_ns": int(elapsed_ns),
            "peak_rss_bytes_process_high_water": peak_rss_bytes(),
        },
        "qualification": {
            "status": "reference-output-only",
            "topology_audit": "not-run",
            "self_intersection": "NotEvaluated",
            "perceptual_quality": "not-evaluated",
            "energy": "not-measured",
        },
    }
    out_manifest = args.output_dir / "run-v1.json"
    out_manifest.write_text(
        json.dumps(manifest, sort_keys=True, indent=2) + "\n",
        encoding="utf-8",
    )
    print(json.dumps({
        "run_manifest": str(out_manifest),
        "grid_sha256": grid["grid_sha256"],
        "vertex_count": len(vertices),
        "quad_count": len(quads),
        "elapsed_ns": elapsed_ns,
        "source_mode": args.mode,
        "vertices_sha256": manifest["result"]["vertices_sha256"],
        "quads_sha256": manifest["result"]["quads_sha256"],
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (InputError, OSError, subprocess.CalledProcessError) as exc:
        raise SystemExit(f"ouroboros DCSDFD adapter: {exc}") from exc
