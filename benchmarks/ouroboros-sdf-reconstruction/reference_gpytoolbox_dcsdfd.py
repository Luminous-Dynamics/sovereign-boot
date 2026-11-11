#!/usr/bin/env python3
"""Offline Gpytoolbox DCSDFD oracle for canonical Ouroboros OUROGRD1 grids.

This adapter is deliberately an interchange/reference tool, not production runtime code.
It reads already-sampled int32 values and never calls the source field or resamples it.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import struct
import sys
import time
from importlib.metadata import PackageNotFoundError, version
from pathlib import Path

import numpy as np
import gpytoolbox as gpy

GRID_MAGIC = b"OUROGRD1"
FIELD_MAGIC = b"OUROFLD1"
GRID_SCHEMA_VERSION = 1
FIELD_SCHEMA_VERSION = 1
EXPECTED_FIXTURE = b"ouroboros-face-skin-v1"
EXPECTED_FIELD_DESCRIPTOR_BYTES = 123
EXPECTED_DISTANCE_QUANTIZATION = 1_000_000


class GridFormatError(ValueError):
    pass


def take(data: bytes, offset: int, size: int, label: str) -> tuple[bytes, int]:
    end = offset + size
    if size < 0 or end > len(data):
        raise GridFormatError(f"truncated {label} at byte {offset}")
    return data[offset:end], end


def parse_grid(path: Path) -> tuple[dict, np.ndarray, np.ndarray]:
    raw = path.read_bytes()
    if len(raw) < 8 or raw[:8] != GRID_MAGIC:
        raise GridFormatError("expected OUROGRD1 magic")
    offset = 8
    schema_raw, offset = take(raw, offset, 2, "grid schema")
    schema = struct.unpack("<H", schema_raw)[0]
    if schema != GRID_SCHEMA_VERSION:
        raise GridFormatError(f"unsupported grid schema {schema}")
    fixture_len_raw, offset = take(raw, offset, 2, "fixture length")
    fixture_len = struct.unpack("<H", fixture_len_raw)[0]
    fixture_raw, offset = take(raw, offset, fixture_len, "fixture ID")
    if fixture_raw != EXPECTED_FIXTURE:
        raise GridFormatError(f"unexpected fixture ID {fixture_raw!r}")
    descriptor_len_raw, offset = take(raw, offset, 4, "field descriptor length")
    descriptor_len = struct.unpack("<I", descriptor_len_raw)[0]
    if descriptor_len != EXPECTED_FIELD_DESCRIPTOR_BYTES:
        raise GridFormatError(f"unexpected field descriptor size {descriptor_len}")
    descriptor, offset = take(raw, offset, descriptor_len, "field descriptor")
    if descriptor[:8] != FIELD_MAGIC:
        raise GridFormatError("invalid field descriptor magic")
    field_schema = struct.unpack_from("<H", descriptor, 8)[0]
    if field_schema != FIELD_SCHEMA_VERSION:
        raise GridFormatError(f"unsupported field schema {field_schema}")
    if descriptor[10:14] != bytes((1, 1, 1, 1)):
        raise GridFormatError("unsupported field frame/sign/units/estimator identifiers")
    if descriptor[13] != 1:
        raise GridFormatError("this adapter only accepts the IQ ellipsoid approximate-field fixture")
    dims_raw, offset = take(raw, offset, 12, "grid dimensions")
    dims = struct.unpack("<III", dims_raw)
    if any(n < 2 for n in dims):
        raise GridFormatError(f"invalid grid dimensions {dims}")
    axis_layout, offset = take(raw, offset, 4, "axis/layout IDs")
    if axis_layout != bytes((1, 2, 3, 1)):
        raise GridFormatError(f"unsupported axis/layout IDs {list(axis_layout)}")
    bounds_raw, offset = take(raw, offset, 48, "grid bounds")
    bounds_fixed = struct.unpack("<6q", bounds_raw)
    bounds = tuple(value / 1_000_000.0 for value in bounds_fixed)
    minimum = np.asarray(bounds[:3], dtype=np.float64)
    maximum = np.asarray(bounds[3:], dtype=np.float64)
    if np.any(maximum <= minimum):
        raise GridFormatError("invalid grid bounds")
    quant_raw, offset = take(raw, offset, 8, "distance quantization")
    quantization = struct.unpack("<q", quant_raw)[0]
    if quantization != EXPECTED_DISTANCE_QUANTIZATION:
        raise GridFormatError(f"unsupported distance quantization {quantization}")
    count_raw, offset = take(raw, offset, 8, "sample count")
    sample_count = struct.unpack("<Q", count_raw)[0]
    expected_count = int(dims[0]) * int(dims[1]) * int(dims[2])
    if sample_count != expected_count:
        raise GridFormatError(f"sample count {sample_count} != dimensions product {expected_count}")
    sample_bytes, offset = take(raw, offset, sample_count * 4, "sample vector")
    if offset != len(raw):
        raise GridFormatError(f"unexpected trailing bytes: {len(raw) - offset}")
    samples_i32 = np.frombuffer(sample_bytes, dtype="<i4").copy()
    sdf = samples_i32.astype(np.float64) / float(quantization)
    nx, ny, nz = dims
    x = minimum[0] + (maximum[0] - minimum[0]) * np.arange(nx, dtype=np.float64) / (nx - 1)
    y = minimum[1] + (maximum[1] - minimum[1]) * np.arange(ny, dtype=np.float64) / (ny - 1)
    z = minimum[2] + (maximum[2] - minimum[2]) * np.arange(nz, dtype=np.float64) / (nz - 1)
    xx, yy, zz = np.meshgrid(x, y, z, indexing="ij")
    grid_vertices = np.stack(
        (xx.ravel(order="F"), yy.ravel(order="F"), zz.ravel(order="F")),
        axis=-1,
    )
    metadata = {
        "input_format": "OUROGRD1",
        "grid_schema": schema,
        "field_schema": field_schema,
        "fixture_id": fixture_raw.decode("utf-8"),
        "field_semantics": "approximate-analytic-field-iq-ellipsoid-union",
        "estimator_id": int(descriptor[13]),
        "axis_layout_ids": list(axis_layout),
        "dimensions": list(dims),
        "minimum": minimum.tolist(),
        "maximum": maximum.tolist(),
        "sample_count": int(sample_count),
        "distance_quantization": quantization,
        "grid_sha256": hashlib.sha256(raw).hexdigest(),
        "sample_vector_sha256": hashlib.sha256(sample_bytes).hexdigest(),
    }
    return metadata, sdf, grid_vertices


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("grid", type=Path, help="canonical OUROGRD1 input")
    parser.add_argument("output", type=Path, help="output NPZ path")
    parser.add_argument("--reference-revision", required=True, help="pinned Gpytoolbox commit/revision")
    parser.add_argument("--outer-iters", type=int, default=100)
    parser.add_argument("--inner-iters", type=int, default=100)
    parser.add_argument("--batch-size", type=int, default=200000)
    parser.add_argument("--hermite-update", action=argparse.BooleanOptionalAction, default=True)
    parser.add_argument("--mu", type=float, default=0.1)
    parser.add_argument("--dc-weight", type=float, default=0.02)
    parser.add_argument("--svd-threshold", type=float, default=0.01)
    parser.add_argument("--new-hermite-pos-weight", type=float, default=0.2)
    parser.add_argument("--new-face-pos-weight", type=float, default=0.2)
    parser.add_argument("--new-hermite-normal-weight", type=float, default=0.2)
    args = parser.parse_args()
    if args.outer_iters < 0 or args.inner_iters < 0 or args.batch_size <= 0:
        parser.error("outer/inner iterations must be nonnegative and batch size must be positive")
    try:
        pkg_version = version("gpytoolbox")
    except PackageNotFoundError:
        pkg_version = "unknown"
    metadata, sdf, grid_vertices = parse_grid(args.grid)
    start = time.perf_counter_ns()
    vertices, quads = gpy.dual_contouring_of_signed_distance_data(
        sdf,
        grid_vertices,
        *metadata["dimensions"],
        isovalue=0.0,
        outer_iters=args.outer_iters,
        inner_iters=args.inner_iters,
        batch_size=args.batch_size,
        hermite_update=args.hermite_update,
        mu=args.mu,
        dc_weight=args.dc_weight,
        svd_threshold=args.svd_threshold,
        new_hermite_pos_weight=args.new_hermite_pos_weight,
        new_face_pos_weight=args.new_face_pos_weight,
        new_hermite_normal_weight=args.new_hermite_normal_weight,
        verbose=False,
    )
    elapsed_ns = time.perf_counter_ns() - start
    vertices = np.asarray(vertices, dtype="<f8", order="C")
    quads = np.asarray(quads, dtype="<i8", order="C")
    if vertices.ndim != 2 or vertices.shape[1] != 3:
        raise RuntimeError(f"reference returned invalid vertex array shape {vertices.shape}")
    if quads.ndim != 2 or quads.shape[1] != 4:
        raise RuntimeError(f"reference returned invalid quad array shape {quads.shape}")
    payload = (
        struct.pack("<QQ", vertices.shape[0], quads.shape[0])
        + vertices.tobytes(order="C")
        + quads.tobytes(order="C")
    )
    output_digest = hashlib.sha256(payload).hexdigest()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    np.savez_compressed(args.output, vertices=vertices, quads=quads)
    report = {
        **metadata,
        "reference": "gpytoolbox.dual_contouring_of_signed_distance_data",
        "gpytoolbox_version": pkg_version,
        "reference_revision": args.reference_revision,
        "parameters": {
            "outer_iters": args.outer_iters,
            "inner_iters": args.inner_iters,
            "batch_size": args.batch_size,
            "hermite_update": args.hermite_update,
            "mu": args.mu,
            "dc_weight": args.dc_weight,
            "svd_threshold": args.svd_threshold,
            "new_hermite_pos_weight": args.new_hermite_pos_weight,
            "new_face_pos_weight": args.new_face_pos_weight,
            "new_hermite_normal_weight": args.new_hermite_normal_weight,
        },
        "vertex_count": int(vertices.shape[0]),
        "quad_count": int(quads.shape[0]),
        "elapsed_ns": int(elapsed_ns),
        "canonical_array_payload_sha256": output_digest,
        "output_npz": str(args.output),
        "note": "Reference adapter dequantizes only canonical OUROGRD1 samples; no source field evaluation or resampling occurs.",
        "non_claims": [
            "no Rust/reference parity result is produced by this adapter alone",
            "no self-intersection status is inferred",
            "no geometry or energy superiority is claimed",
            "the source field is approximate, not an exact Euclidean SDF",
        ],
    }
    report_path = args.output.with_suffix(args.output.suffix + ".json")
    report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(report, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (GridFormatError, OSError, RuntimeError, ValueError) as exc:
        print(f"reference adapter failed: {exc}", file=sys.stderr)
        raise SystemExit(2)
