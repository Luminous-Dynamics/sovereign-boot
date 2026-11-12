#!/usr/bin/env python3
"""Compare a classical OURODCQ1 artifact to a pinned DCSDFD reference run.

Reported vertex-site distances are not surface-to-surface Hausdorff distances.
Topology metrics are lightweight structural indicators, not a replacement for
ouroboros-mesh-audit or the independent CGAL oracle.
"""
from __future__ import annotations

import argparse
import json
import math
import struct
import sys
from pathlib import Path
from typing import Any

from reference_adapter import (
    MAX_INPUT_BYTES,
    build_grid_vertices,
    parse_ourogrd1,
    sha256_bytes,
    stable_array_hash,
)


MAX_RECEIPT_BYTES = 32 * 1024 * 1024
DC_MAGIC = b"OURODCQ1"
DC_SCHEMA = 1
DC_CONFIG_BYTES = 37
POSITION_SCALE = 1_000_000_000.0
MAX_REFERENCE_ARRAY_BYTES = 512 * 1024 * 1024
MAX_REFERENCE_VERTICES = 2_000_000
MAX_REFERENCE_QUADS = 2_000_000
MAX_TOPOLOGY_VERTICES = 1_000_000
MAX_TOPOLOGY_TRIANGLES = 200_000


class ComparisonError(ValueError):
    pass


def take_u16(data: bytes, offset: int) -> tuple[int, int]:
    if offset + 2 > len(data):
        raise ComparisonError("truncated OURODCQ1 u16")
    return struct.unpack_from("<H", data, offset)[0], offset + 2


def take_u32(data: bytes, offset: int) -> tuple[int, int]:
    if offset + 4 > len(data):
        raise ComparisonError("truncated OURODCQ1 u32")
    return struct.unpack_from("<I", data, offset)[0], offset + 4


def take_u64(data: bytes, offset: int) -> tuple[int, int]:
    if offset + 8 > len(data):
        raise ComparisonError("truncated OURODCQ1 u64")
    return struct.unpack_from("<Q", data, offset)[0], offset + 8


def parse_ourodcq1(path: Path, np: Any) -> dict[str, Any]:
    raw = path.read_bytes()
    if len(raw) > MAX_RECEIPT_BYTES:
        raise ComparisonError("OURODCQ1 exceeds the 32 MiB receipt limit")
    if len(raw) < 14 or raw[:8] != DC_MAGIC:
        raise ComparisonError("unsupported DC receipt magic")
    offset = 8
    schema, offset = take_u16(raw, offset)
    if schema != DC_SCHEMA:
        raise ComparisonError(f"unsupported OURODCQ1 schema {schema}")
    source_len, offset = take_u32(raw, offset)
    if source_len > MAX_INPUT_BYTES or offset + source_len > len(raw):
        raise ComparisonError("invalid embedded OUROGRD1 source length")
    source_grid = raw[offset : offset + source_len]
    offset += source_len
    if offset + DC_CONFIG_BYTES > len(raw):
        raise ComparisonError("truncated dual-contouring policy bytes")
    config_bytes = raw[offset : offset + DC_CONFIG_BYTES]
    offset += DC_CONFIG_BYTES

    vertex_count, offset = take_u32(raw, offset)
    vertex_bytes = vertex_count * 40
    if vertex_count == 0 or offset + vertex_bytes > len(raw):
        raise ComparisonError("invalid or truncated DC vertex array")
    vertices_i64 = np.empty((vertex_count, 3), dtype=np.int64)
    for index in range(vertex_count):
        values = struct.unpack_from("<3q3iI", raw, offset)
        vertices_i64[index, :] = values[:3]
        offset += 40
    vertices = vertices_i64.astype(np.float64) / POSITION_SCALE

    quad_count, offset = take_u32(raw, offset)
    quad_index_count = quad_count * 4
    quad_bytes = quad_index_count * 4
    if quad_count == 0 or offset + quad_bytes > len(raw):
        raise ComparisonError("invalid or truncated DC quad array")
    quads = np.frombuffer(raw, dtype="<u4", count=quad_index_count, offset=offset)
    quads = quads.astype(np.int64).reshape((quad_count, 4))
    offset += quad_bytes

    triangle_count, offset = take_u32(raw, offset)
    triangle_index_count = triangle_count * 3
    triangle_bytes = triangle_index_count * 4
    if triangle_count != quad_count * 2 or offset + triangle_bytes > len(raw):
        raise ComparisonError("invalid or truncated DC triangle view")
    triangles = np.frombuffer(raw, dtype="<u4", count=triangle_index_count, offset=offset)
    triangles = triangles.astype(np.int64).reshape((triangle_count, 3))
    offset += triangle_bytes

    # Metrics: 5 u32 counters, 5 u64 fields, then audit length + opaque audit.
    metrics_offset = offset
    if offset + 5 * 4 + 5 * 8 + 4 > len(raw):
        raise ComparisonError("truncated DC metrics/audit header")
    metrics = struct.unpack_from("<5I5Q", raw, offset)
    offset += 5 * 4 + 5 * 8
    audit_len, offset = take_u32(raw, offset)
    if offset + audit_len != len(raw):
        raise ComparisonError("DC audit payload length mismatch or trailing bytes")
    audit_bytes = raw[offset : offset + audit_len]

    if quads.size and (quads.min() < 0 or quads.max() >= vertex_count):
        raise ComparisonError("DC quad contains an out-of-range vertex index")
    if triangles.size and (triangles.min() < 0 or triangles.max() >= vertex_count):
        raise ComparisonError("DC triangle contains an out-of-range vertex index")
    if not np.isfinite(vertices).all():
        raise ComparisonError("DC receipt contains non-finite positions")

    return {
        "receipt_sha256": sha256_bytes(raw),
        "embedded_grid_sha256": sha256_bytes(source_grid),
        "source_grid_bytes": source_grid,
        "config_sha256": sha256_bytes(config_bytes),
        "vertices": vertices,
        "quads": quads,
        "triangles": triangles,
        "metrics": {
            "active_cell_count": metrics[0],
            "sign_changing_edge_count": metrics[1],
            "qef_centroid_fallback_count": metrics[2],
            "qef_clamped_vertex_count": metrics[3],
            "finite_difference_gradient_fallback_count": metrics[4],
            "qef_residual_p50_nanounits": metrics[5],
            "qef_residual_p95_nanounits": metrics[6],
            "qef_residual_max_nanounits": metrics[7],
            "geometry_bytes": metrics[8],
            "working_payload_estimate_bytes": metrics[9],
        },
        "audit_receipt_sha256": sha256_bytes(audit_bytes),
        "audit_receipt_bytes": len(audit_bytes),
    }


def triangulate_quads(quads: Any, np: Any):
    if len(quads) == 0:
        return np.empty((0, 3), dtype=np.int64)
    return np.vstack((quads[:, [0, 1, 2]], quads[:, [0, 2, 3]])).astype(np.int64)


def union_find_components(face_count: int, edge_uses: dict):
    parent = list(range(face_count))
    rank = [0] * face_count

    def find(value: int) -> int:
        while parent[value] != value:
            parent[value] = parent[parent[value]]
            value = parent[value]
        return value

    def union(a: int, b: int) -> None:
        ra, rb = find(a), find(b)
        if ra == rb:
            return
        if rank[ra] < rank[rb]:
            ra, rb = rb, ra
        parent[rb] = ra
        if rank[ra] == rank[rb]:
            rank[ra] += 1

    for uses in edge_uses.values():
        if len(uses) >= 2:
            first = uses[0][0]
            for face_id, _direction in uses[1:]:
                union(first, face_id)
    return len({find(i) for i in range(face_count)})


def topology_summary(vertices: Any, triangles: Any, np: Any, area_epsilon: float) -> dict[str, Any]:
    if len(vertices) > MAX_TOPOLOGY_VERTICES or len(triangles) > MAX_TOPOLOGY_TRIANGLES:
        raise ComparisonError(
            "lightweight topology summary exceeds its explicit vertex/triangle work limit"
        )
    if triangles.ndim != 2 or triangles.shape[1] != 3:
        raise ComparisonError("triangle array must have shape (n,3)")
    if triangles.size and (triangles.min() < 0 or triangles.max() >= len(vertices)):
        raise ComparisonError("triangle index out of range")

    edge_uses: dict[tuple[int, int], list[tuple[int, int]]] = {}
    used_vertices = set()
    repeated_index_faces = 0
    zero_area_faces = 0
    for face_id, face in enumerate(triangles):
        a, b, c = (int(face[0]), int(face[1]), int(face[2]))
        if len({a, b, c}) != 3:
            repeated_index_faces += 1
        else:
            cross = np.cross(vertices[b] - vertices[a], vertices[c] - vertices[a])
            if not np.isfinite(cross).all() or float(np.linalg.norm(cross)) <= area_epsilon:
                zero_area_faces += 1
        used_vertices.update((a, b, c))
        for first, second in ((a, b), (b, c), (c, a)):
            key = (first, second) if first < second else (second, first)
            direction = 1 if first < second else -1
            edge_uses.setdefault(key, []).append((face_id, direction))

    incidence = [len(uses) for uses in edge_uses.values()]
    boundary_edges = sum(1 for count in incidence if count == 1)
    nonmanifold_edges = sum(1 for count in incidence if count > 2)
    orientation_conflicts = sum(
        1 for uses in edge_uses.values()
        if len(uses) == 2 and uses[0][1] == uses[1][1]
    )
    return {
        "vertex_count": int(len(vertices)),
        "triangle_count": int(len(triangles)),
        "edge_count": int(len(edge_uses)),
        "boundary_edge_count": int(boundary_edges),
        "nonmanifold_edge_count": int(nonmanifold_edges),
        "orientation_conflict_edge_count": int(orientation_conflicts),
        "repeated_index_triangle_count": int(repeated_index_faces),
        "zero_or_near_zero_area_triangle_count": int(zero_area_faces),
        "isolated_vertex_count": int(len(vertices) - len(used_vertices)),
        "edge_connected_face_component_count": (
            int(union_find_components(len(triangles), edge_uses)) if len(triangles) else 0
        ),
        "self_intersection": "NotEvaluated",
        "qualification": "edge-incidence indicators only; not a full vertex-link/manifold audit",
    }


def distance_summary(values: Any, np: Any) -> dict[str, float]:
    if len(values) == 0:
        raise ComparisonError("cannot summarize an empty vertex set")
    return {
        "p50": float(np.percentile(values, 50)),
        "p95": float(np.percentile(values, 95)),
        "p99": float(np.percentile(values, 99)),
        "max": float(np.max(values)),
        "mean": float(np.mean(values)),
    }


def symmetric_vertex_site_distance(reference: Any, candidate: Any, np: Any) -> dict[str, Any]:
    try:
        from scipy.spatial import cKDTree
    except ImportError as exc:
        raise ComparisonError("SciPy is required for nearest-vertex comparisons") from exc
    if len(reference) == 0 or len(candidate) == 0:
        raise ComparisonError("both meshes must contain at least one vertex")
    ref_tree = cKDTree(reference)
    candidate_tree = cKDTree(candidate)
    candidate_to_reference = ref_tree.query(candidate, k=1, workers=1)[0]
    reference_to_candidate = candidate_tree.query(reference, k=1, workers=1)[0]
    return {
        "definition": "nearest vertex-site Euclidean distance; not surface Chamfer/Hausdorff",
        "candidate_to_reference": distance_summary(candidate_to_reference, np),
        "reference_to_candidate": distance_summary(reference_to_candidate, np),
        "symmetric_max": float(max(candidate_to_reference.max(), reference_to_candidate.max())),
    }


def halton_values(count: int, base: int, np: Any):
    if count < 1 or base < 2:
        raise ComparisonError("Halton count/base must be positive")
    indices = np.arange(1, count + 1, dtype=np.int64)
    values = np.zeros(count, dtype=np.float64)
    factor = 1.0
    remaining = indices.copy()
    while np.any(remaining):
        factor /= base
        values += factor * (remaining % base)
        remaining //= base
    return values


def deterministic_surface_samples(
    vertices: Any,
    triangles: Any,
    sample_count: int,
    np: Any,
) -> tuple[Any, int]:
    vertices = np.asarray(vertices, dtype=np.float64)
    triangles = np.asarray(triangles, dtype=np.int64)
    if sample_count < 1:
        raise ComparisonError("surface sample count must be positive")
    if vertices.ndim != 2 or vertices.shape[1] != 3:
        raise ComparisonError("surface vertices must have shape (n,3)")
    if triangles.ndim != 2 or triangles.shape[1] != 3 or len(triangles) == 0:
        raise ComparisonError("surface triangles must have shape (m,3) with m > 0")
    if not np.isfinite(vertices).all():
        raise ComparisonError("surface contains non-finite vertices")
    if triangles.min() < 0 or triangles.max() >= len(vertices):
        raise ComparisonError("surface triangle index out of range")

    tri_points = vertices[triangles]
    cross = np.cross(
        tri_points[:, 1] - tri_points[:, 0],
        tri_points[:, 2] - tri_points[:, 0],
    )
    twice_area = np.linalg.norm(cross, axis=1)
    if not np.isfinite(twice_area).all():
        raise ComparisonError("surface triangle area is non-finite")
    valid = twice_area > 0.0
    degenerate_count = int((~valid).sum())
    if not np.any(valid):
        raise ComparisonError("surface has no positive-area triangles")
    valid_triangles = triangles[valid]
    valid_areas = twice_area[valid] * 0.5
    cumulative_area = np.cumsum(valid_areas, dtype=np.float64)
    total_area = float(cumulative_area[-1])
    if not math.isfinite(total_area) or total_area <= 0.0:
        raise ComparisonError("surface has invalid total area")

    face_selector = halton_values(sample_count, 2, np) * total_area
    face_ids = np.searchsorted(cumulative_area, face_selector, side="right")
    face_ids = np.minimum(face_ids, len(valid_triangles) - 1)
    selected = vertices[valid_triangles[face_ids]]

    # Uniform area measure inside each selected triangle:
    # (1-sqrt(u), sqrt(u)*(1-v), sqrt(u)*v).
    root_u = np.sqrt(halton_values(sample_count, 3, np))
    v = halton_values(sample_count, 5, np)
    w0 = 1.0 - root_u
    w1 = root_u * (1.0 - v)
    w2 = root_u * v
    samples = (
        selected[:, 0] * w0[:, None]
        + selected[:, 1] * w1[:, None]
        + selected[:, 2] * w2[:, None]
    )
    if not np.isfinite(samples).all():
        raise ComparisonError("deterministic surface sampler generated non-finite points")
    return samples, degenerate_count


def symmetric_surface_sampled_distance(
    reference_vertices: Any,
    reference_triangles: Any,
    candidate_vertices: Any,
    candidate_triangles: Any,
    sample_count: int,
    np: Any,
) -> dict[str, Any]:
    try:
        from scipy.spatial import cKDTree
    except ImportError as exc:
        raise ComparisonError("SciPy is required for surface-sample nearest-neighbour comparisons") from exc
    reference_samples, reference_degenerate = deterministic_surface_samples(
        reference_vertices, reference_triangles, sample_count, np
    )
    candidate_samples, candidate_degenerate = deterministic_surface_samples(
        candidate_vertices, candidate_triangles, sample_count, np
    )
    reference_tree = cKDTree(reference_samples)
    candidate_tree = cKDTree(candidate_samples)
    candidate_to_reference = reference_tree.query(candidate_samples, k=1, workers=1)[0]
    reference_to_candidate = candidate_tree.query(reference_samples, k=1, workers=1)[0]
    return {
        "definition": (
            "symmetric nearest-neighbour distance between deterministic Halton, "
            "area-weighted triangle-surface samples; an estimator, not exact Chamfer or Hausdorff"
        ),
        "sampling": "Halton bases 2/3/5; area-weighted face selection; uniform barycentric samples",
        "sample_count_per_surface": int(sample_count),
        "reference_degenerate_triangles_excluded": reference_degenerate,
        "candidate_degenerate_triangles_excluded": candidate_degenerate,
        "candidate_to_reference": distance_summary(candidate_to_reference, np),
        "reference_to_candidate": distance_summary(reference_to_candidate, np),
        "symmetric_max_sample_distance": float(
            max(candidate_to_reference.max(), reference_to_candidate.max())
        ),
    }


def sampled_grid_residual(vertices: Any, grid: dict[str, Any], np: Any) -> dict[str, Any]:
    nx, ny, nz = grid["dimensions"]
    minimum = np.asarray(grid["minimum"], dtype=np.float64)
    maximum = np.asarray(grid["maximum"], dtype=np.float64)
    samples = grid["samples"].reshape(-1)
    points = np.asarray(vertices, dtype=np.float64)

    inside = np.all((points >= minimum) & (points <= maximum), axis=1)
    pts = points[inside]
    if len(pts) == 0:
        return {
            "definition": "absolute trilinear interpolation of canonical quantized grid samples",
            "inside_grid_vertex_count": 0,
            "outside_grid_vertex_count": int(len(points)),
            "absolute_residual": None,
        }

    dims = np.asarray([nx, ny, nz], dtype=np.float64)
    coords = (pts - minimum) / (maximum - minimum) * (dims - 1.0)
    lower = np.floor(coords).astype(np.int64)
    upper = np.minimum(lower + 1, (dims.astype(np.int64) - 1))
    t = coords - lower

    def value(i, j, k):
        index = i + nx * (j + ny * k)
        return samples[index]

    i0, j0, k0 = lower[:, 0], lower[:, 1], lower[:, 2]
    i1, j1, k1 = upper[:, 0], upper[:, 1], upper[:, 2]
    tx, ty, tz = t[:, 0], t[:, 1], t[:, 2]
    c000 = value(i0, j0, k0)
    c100 = value(i1, j0, k0)
    c010 = value(i0, j1, k0)
    c110 = value(i1, j1, k0)
    c001 = value(i0, j0, k1)
    c101 = value(i1, j0, k1)
    c011 = value(i0, j1, k1)
    c111 = value(i1, j1, k1)

    c00 = c000 * (1.0 - tx) + c100 * tx
    c10 = c010 * (1.0 - tx) + c110 * tx
    c01 = c001 * (1.0 - tx) + c101 * tx
    c11 = c011 * (1.0 - tx) + c111 * tx
    c0 = c00 * (1.0 - ty) + c10 * ty
    c1 = c01 * (1.0 - ty) + c11 * ty
    interpolated = c0 * (1.0 - tz) + c1 * tz
    residuals = np.abs(interpolated)
    return {
        "definition": "absolute trilinear interpolation of canonical quantized grid samples; not exact-SDF error",
        "inside_grid_vertex_count": int(len(pts)),
        "outside_grid_vertex_count": int(len(points) - len(pts)),
        "absolute_residual": distance_summary(residuals, np),
    }


def safe_reference_array_path(run_dir: Path, filename: Any, label: str) -> Path:
    if (
        not isinstance(filename, str)
        or not filename
        or filename in (".", "..")
        or Path(filename).name != filename
    ):
        raise ComparisonError(f"reference {label} filename must be a simple basename")
    root = run_dir.resolve(strict=True)
    path = root / filename
    if path.is_symlink():
        raise ComparisonError(f"reference {label} file must not be a symlink")
    try:
        resolved = path.resolve(strict=True)
    except OSError as exc:
        raise ComparisonError(f"reference {label} file is missing or unreadable") from exc
    try:
        resolved.relative_to(root)
    except ValueError as exc:
        raise ComparisonError(f"reference {label} file escapes its run directory") from exc
    if not resolved.is_file():
        raise ComparisonError(f"reference {label} path is not a regular file")
    if resolved.stat().st_size > MAX_REFERENCE_ARRAY_BYTES:
        raise ComparisonError(f"reference {label} file exceeds the array byte limit")
    return resolved


def validate_reference_input_binding(reference_manifest: dict[str, Any], grid: dict[str, Any]) -> None:
    input_record = reference_manifest.get("input") if isinstance(reference_manifest, dict) else None
    if not isinstance(input_record, dict):
        raise ComparisonError("reference manifest is missing a structured input record")
    expected = {
        "magic": "OUROGRD1",
        "grid_sha256": grid["grid_sha256"],
        "field_descriptor_sha256": grid["descriptor_sha256"],
        "fixture_id": grid["fixture_id"],
        "dimensions": list(grid["dimensions"]),
        "minimum": grid["minimum"],
        "maximum": grid["maximum"],
        "distance_scale": grid["distance_scale"],
        "sample_count": grid["sample_count"],
        "layout": "first-axis-fastest",
        "resampled": False,
        "field_reevaluated": False,
    }
    for key, value in expected.items():
        if input_record.get(key) != value:
            raise ComparisonError(f"reference input binding mismatch for {key}")


def load_reference_run(run_dir: Path, np: Any):
    manifest_path = run_dir / "run-v1.json"
    if not manifest_path.is_file():
        raise ComparisonError(f"missing reference manifest {manifest_path}")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if not isinstance(manifest, dict) or type(manifest.get("schema_version")) is not int or manifest.get("schema_version") != 1:
        raise ComparisonError("unsupported DCSDFD reference manifest schema")
    reference = manifest.get("reference")
    if not isinstance(reference, dict):
        raise ComparisonError("reference manifest is missing its source identity record")
    if reference.get("repository") != "sgsellan/gpytoolbox":
        raise ComparisonError("reference manifest does not identify sgsellan/gpytoolbox")
    if reference.get("commit") != "d6e6ae3b5ff9200e687ca19c178ff35d65c1f810":
        raise ComparisonError("reference manifest is not pinned to the declared Gpytoolbox commit")
    if reference.get("source_mode") not in ("upstream-unmodified", "fixed-seed-patched"):
        raise ComparisonError("reference manifest has an unknown source mode")
    source_digest = reference.get("source_file_sha256")
    if not isinstance(source_digest, str) or len(source_digest) != 64 or any(ch not in "0123456789abcdef" for ch in source_digest):
        raise ComparisonError("reference manifest is missing a valid pinned source-file digest")
    if reference.get("source_file") != "src/cpp/dual_contouring_of_signed_distance_data/contouring.cpp":
        raise ComparisonError("reference manifest has an unexpected source-file path")
    if reference["source_mode"] == "fixed-seed-patched":
        if reference.get("patch_sha256") != "71b1983492b906935b95588368bdf7c11a87a47ed4ae0fa8694ea5d2f95386ee":
            raise ComparisonError("fixed-seed reference manifest has an unexpected patch digest")
    elif reference.get("patch_sha256") is not None:
        raise ComparisonError("upstream-unmodified reference must not claim a patch digest")

    native = manifest.get("native_binary")
    if not isinstance(native, dict):
        raise ComparisonError("reference manifest is missing its native binary record")
    extension_digest = native.get("extension_sha256")
    if not isinstance(extension_digest, str) or len(extension_digest) != 64 or any(ch not in "0123456789abcdef" for ch in extension_digest):
        raise ComparisonError("reference manifest is missing the loaded native extension digest")
    if not isinstance(native.get("extension_path"), str) or not native["extension_path"]:
        raise ComparisonError("reference manifest is missing the loaded native extension path")
    if native.get("binary_source_binding") != (
        "path-and-binary-digest-recorded; build provenance is not independently attested"
    ):
        raise ComparisonError("reference manifest has an unknown native binary provenance label")
    if native.get("extension_inside_reference_checkout") not in (True, False):
        raise ComparisonError("reference manifest is missing native extension checkout-location status")

    parameters = manifest.get("parameters")
    if not isinstance(parameters, dict):
        raise ComparisonError("reference manifest is missing its parameter record")
    isovalue = parameters.get("isovalue")
    if isinstance(isovalue, bool) or not isinstance(isovalue, (int, float)) or not math.isfinite(float(isovalue)):
        raise ComparisonError("reference manifest is missing a finite isovalue")
    if float(isovalue) != 0.0:
        raise ComparisonError("classical OURODCQ1 comparison requires the zero isovalue")
    input_record = manifest.get("input")
    if not isinstance(input_record, dict):
        raise ComparisonError("reference manifest is missing its input record")
    if input_record.get("magic") != "OUROGRD1":
        raise ComparisonError("reference manifest does not identify OUROGRD1 input")
    grid_digest = input_record.get("grid_sha256")
    descriptor_digest = input_record.get("field_descriptor_sha256")
    for label, digest in (("grid", grid_digest), ("field descriptor", descriptor_digest)):
        if not isinstance(digest, str) or len(digest) != 64 or any(ch not in "0123456789abcdef" for ch in digest):
            raise ComparisonError(f"reference manifest is missing a valid {label} digest")
    if input_record.get("resampled") is not False or input_record.get("field_reevaluated") is not False:
        raise ComparisonError("reference manifest does not attest frozen, non-resampled input")

    result_record = manifest.get("result")
    if not isinstance(result_record, dict):
        raise ComparisonError("reference manifest is missing its result record")
    vertices_path = safe_reference_array_path(
        run_dir, result_record.get("vertices_file"), "vertex-array"
    )
    quads_path = safe_reference_array_path(
        run_dir, result_record.get("quads_file"), "quad-array"
    )
    vertices_raw = np.load(vertices_path, allow_pickle=False)
    quads_raw = np.load(quads_path, allow_pickle=False)
    if not isinstance(vertices_raw, np.ndarray) or not isinstance(quads_raw, np.ndarray):
        raise ComparisonError("reference outputs must be plain NPY arrays, not archives")
    if vertices_raw.dtype.kind != "f" or vertices_raw.dtype.itemsize != 8:
        raise ComparisonError("reference vertex array must preserve float64 storage")
    if quads_raw.dtype.kind != "i" or quads_raw.dtype.itemsize != 8:
        raise ComparisonError("reference quad array must preserve signed int64 index storage")
    if vertices_raw.ndim != 2 or vertices_raw.shape[1] != 3 or len(vertices_raw) == 0:
        raise ComparisonError("reference vertex array has an invalid shape or is empty")
    if quads_raw.ndim != 2 or quads_raw.shape[1] != 4 or len(quads_raw) == 0:
        raise ComparisonError("reference quad array has an invalid shape or is empty")
    if len(vertices_raw) > MAX_REFERENCE_VERTICES or len(quads_raw) > MAX_REFERENCE_QUADS:
        raise ComparisonError("reference output exceeds the vertex/quad count limits")
    vertices = np.asarray(vertices_raw, dtype=np.float64)
    quads = np.asarray(quads_raw, dtype=np.int64)
    if not np.isfinite(vertices).all():
        raise ComparisonError("reference mesh contains non-finite vertices")
    if quads.min() < 0 or quads.max() >= len(vertices):
        raise ComparisonError("reference quad index out of range")
    vertices_digest = result_record.get("vertices_sha256")
    quads_digest = result_record.get("quads_sha256")
    for label, digest in (("vertex", vertices_digest), ("quad", quads_digest)):
        if (
            not isinstance(digest, str)
            or len(digest) != 64
            or any(ch not in "0123456789abcdef" for ch in digest)
        ):
            raise ComparisonError(f"reference manifest has an invalid {label}-array digest")
    if stable_array_hash(vertices, np) != vertices_digest:
        raise ComparisonError("reference vertex-array digest does not match run manifest")
    if stable_array_hash(quads, np) != quads_digest:
        raise ComparisonError("reference quad-array digest does not match run manifest")
    if len(vertices) != result_record.get("vertex_count"):
        raise ComparisonError("reference vertex count does not match run manifest")
    if len(quads) != result_record.get("quad_count"):
        raise ComparisonError("reference quad count does not match run manifest")
    return manifest, vertices, quads


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference-run", required=True, type=Path)
    parser.add_argument("--candidate-ourodcq1", required=True, type=Path)
    parser.add_argument("--grid", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--area-epsilon", type=float, default=1e-20)
    parser.add_argument("--surface-samples", type=int, default=16384)
    args = parser.parse_args()

    import numpy as np

    reference_manifest, ref_vertices, ref_quads = load_reference_run(args.reference_run, np)
    candidate = parse_ourodcq1(args.candidate_ourodcq1, np)
    if not math.isfinite(args.area_epsilon) or args.area_epsilon < 0:
        raise ComparisonError("area-epsilon must be finite and nonnegative")
    if args.surface_samples < 256 or args.surface_samples > 1_000_000:
        raise ComparisonError("surface-samples must be in [256, 1000000]")

    grid = parse_ourogrd1(args.grid, np)
    grid_sha = grid["grid_sha256"]
    validate_reference_input_binding(reference_manifest, grid)
    if candidate["embedded_grid_sha256"] != grid_sha:
        raise ComparisonError("OURODCQ1 embedded grid differs from reference input")

    ref_triangles = triangulate_quads(ref_quads, np)
    candidate_expected_triangles = triangulate_quads(candidate["quads"], np)
    if not np.array_equal(candidate["triangles"], candidate_expected_triangles):
        raise ComparisonError("OURODCQ1 triangle view differs from its declared fixed (0,2) quad diagonal")
    ref_topology = topology_summary(ref_vertices, ref_triangles, np, args.area_epsilon)
    cand_topology = topology_summary(
        candidate["vertices"], candidate["triangles"], np, args.area_epsilon
    )
    comparison = {
        "schema_version": 1,
        "input": {
            "grid_sha256": grid_sha,
            "fixture_id": grid["fixture_id"],
            "dimensions": list(grid["dimensions"]),
            "minimum": grid["minimum"],
            "maximum": grid["maximum"],
        },
        "reference": {
            "source": reference_manifest["reference"],
            "mode": reference_manifest["reference"]["source_mode"],
            "vertex_count": int(len(ref_vertices)),
            "quad_count": int(len(ref_quads)),
            "topology_indicators": ref_topology,
            "sampled_grid_vertex_residual": sampled_grid_residual(ref_vertices, grid, np),
        },
        "candidate": {
            "kind": "classical-rust-OURODCQ1",
            "receipt_sha256": candidate["receipt_sha256"],
            "config_sha256": candidate["config_sha256"],
            "metrics": candidate["metrics"],
            "embedded_audit_receipt_sha256": candidate["audit_receipt_sha256"],
            "embedded_audit_receipt_bytes": candidate["audit_receipt_bytes"],
            "vertex_count": int(len(candidate["vertices"])),
            "quad_count": int(len(candidate["quads"])),
            "topology_indicators": cand_topology,
            "sampled_grid_vertex_residual": sampled_grid_residual(candidate["vertices"], grid, np),
        },
        "geometry_comparison": {
            "symmetric_vertex_site_distance": symmetric_vertex_site_distance(
                ref_vertices, candidate["vertices"], np
            ),
            "symmetric_surface_sampled_distance": symmetric_surface_sampled_distance(
                ref_vertices, ref_triangles,
                candidate["vertices"], candidate["triangles"],
                args.surface_samples, np,
            ),
        },
        "limitations": [
            "nearest vertex-site distance is not surface-to-surface Chamfer or Hausdorff distance",
            "surface-sampled distance is a deterministic area-weighted Monte Carlo/quasi-Monte-Carlo estimate, not exact Hausdorff distance",
            "topology metrics count edge incidences and do not replace vertex-link/manifold audit",
            "self-intersection remains NotEvaluated until matching CGAL evidence is attached",
            "sampled-grid residual is an interpolated residual to the quantized grid, not exact geometric ground truth",
            "perceptual identity, silhouette and energy are not evaluated",
        ],
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(comparison, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({
        "output": str(args.output),
        "grid_sha256": grid_sha,
        "vertex_site_distance": comparison["geometry_comparison"]["symmetric_vertex_site_distance"]["symmetric_max"],
        "surface_sampled_max_distance": comparison["geometry_comparison"]["symmetric_surface_sampled_distance"]["symmetric_max_sample_distance"],
        "reference_vertices": len(ref_vertices),
        "candidate_vertices": len(candidate["vertices"]),
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ComparisonError, OSError, KeyError, ValueError) as exc:
        raise SystemExit(f"ouroboros mesh comparison: {exc}") from exc
