#!/usr/bin/env python3
"""Offline reference comparison: marching cubes vs classical finite-gradient dual contouring.

Research-only utility. It consumes one source-grounded, microunit-quantized
OUROBOROS face skin field. It is not production code, and the DC implementation
here is a deliberately simple QEF reference (not the SIGGRAPH 2026 method).
"""
from __future__ import annotations

import argparse
import hashlib
import json
import platform
import sys
import time
from importlib.metadata import PackageNotFoundError, version
from pathlib import Path

import numpy as np
import skimage
import trimesh
from skimage.measure import marching_cubes

BOUNDS_MIN = np.array([-0.18, -0.14, -0.16], dtype=np.float64)
BOUNDS_MAX = np.array([0.25, 0.14, 0.16], dtype=np.float64)
HEAD_CENTER = np.array([0.030, 0.0, 0.0], dtype=np.float64)
HEAD_RADII = np.array([0.120, 0.060, 0.110], dtype=np.float64)
NOSE_CENTER = np.array([0.160, 0.0, 0.005], dtype=np.float64)
NOSE_RADII = np.array([0.020, 0.014, 0.018], dtype=np.float64)
DISTANCE_SCALE = 1_000_000
CUBE_CORNERS = np.array(
    [
        [0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0],
        [0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1],
    ],
    dtype=np.int64,
)
CUBE_EDGES = (
    (0, 1), (1, 2), (2, 3), (3, 0),
    (4, 5), (5, 6), (6, 7), (7, 4),
    (0, 4), (1, 5), (2, 6), (3, 7),
)


def ellipsoid_distance_approx(points: np.ndarray, center: np.ndarray, radii: np.ndarray) -> np.ndarray:
    """IQ-style approximate ellipsoid field; not exact Euclidean SDF."""
    q = np.abs((points - center) / radii)
    k0 = np.linalg.norm(q, axis=-1)
    k1 = np.linalg.norm(q / radii, axis=-1)
    return np.divide(
        k0 * (k0 - 1.0),
        k1,
        out=np.full_like(k0, -float(np.min(radii))),
        where=k1 > 1.0e-15,
    )


def smooth_min(a: np.ndarray, b: np.ndarray, width: float) -> np.ndarray:
    if width == 0.0:
        return np.minimum(a, b)
    h = np.maximum(width - np.abs(a - b), 0.0) / width
    return np.minimum(a, b) - 0.25 * h * h * width


def evaluate_field(points: np.ndarray, width: float) -> np.ndarray:
    head = ellipsoid_distance_approx(points, HEAD_CENTER, HEAD_RADII)
    nose = ellipsoid_distance_approx(points, NOSE_CENTER, NOSE_RADII)
    return smooth_min(head, nose, width)


def canonical_grid(resolution: int, width: float) -> tuple[list[np.ndarray], np.ndarray, str]:
    axes = [
        np.linspace(BOUNDS_MIN[i], BOUNDS_MAX[i], resolution, dtype=np.float64)
        for i in range(3)
    ]
    points = np.stack(np.meshgrid(*axes, indexing="ij"), axis=-1)
    raw = evaluate_field(points, width)
    samples = np.rint(raw * DISTANCE_SCALE).astype("<i4")
    # First-axis-fastest linearization: i + nx * (j + ny * k).
    payload = samples.ravel(order="F").tobytes(order="C")
    return axes, samples, hashlib.sha256(payload).hexdigest()


def topology_report(vertices: np.ndarray, triangles: np.ndarray, width: float) -> dict:
    mesh = trimesh.Trimesh(vertices=vertices, faces=triangles, process=False)
    residual = np.abs(evaluate_field(vertices, width))
    triangle_count = int(len(triangles))
    vertex_count = int(len(vertices))
    index_count = triangle_count * 3
    geometry_bytes = vertex_count * 36 + index_count * 4
    return {
        "vertices": vertex_count,
        "triangles": triangle_count,
        "indices": index_count,
        "watertight": bool(mesh.is_watertight),
        "windingConsistent": bool(mesh.is_winding_consistent),
        "components": int(mesh.body_count),
        "eulerCharacteristic": int(mesh.euler_number),
        "fieldResidualP95": float(np.percentile(residual, 95)),
        "fieldResidualMaximum": float(np.max(residual)),
        "estimatedFaceBufferBytes": geometry_bytes,
        "budgetChecks": {
            "vertexLimit": vertex_count <= 50_000,
            "indexLimit": index_count <= 300_000,
            "geometryByteLimit": geometry_bytes <= 2_500_000,
        },
    }


def marching_cubes_reference(samples: np.ndarray, resolution: int) -> tuple[np.ndarray, np.ndarray]:
    spacing = tuple((BOUNDS_MAX - BOUNDS_MIN) / (resolution - 1))
    scalar_field = samples.astype(np.float64) / DISTANCE_SCALE
    vertices, faces, _, _ = marching_cubes(
        scalar_field, level=0.0, spacing=spacing, method="lewiner", allow_degenerate=False
    )
    return vertices + BOUNDS_MIN, faces.astype(np.int64, copy=False)


def classical_dual_contouring_reference(
    samples: np.ndarray, axes: list[np.ndarray]
) -> tuple[np.ndarray, np.ndarray, int]:
    """Simple per-active-cell QEF DC with finite-difference sample gradients."""
    values = samples.astype(np.float64) / DISTANCE_SCALE
    spacing = np.array([(axis[-1] - axis[0]) / (len(axis) - 1) for axis in axes])
    gradients = np.gradient(values, *spacing, edge_order=1)
    nx, ny, nz = samples.shape

    corners = np.stack(
        [
            values[dx : dx + nx - 1, dy : dy + ny - 1, dz : dz + nz - 1]
            for dx, dy, dz in CUBE_CORNERS
        ],
        axis=-1,
    )
    active = (corners.min(axis=-1) < 0.0) & (corners.max(axis=-1) >= 0.0)
    active_cells = np.argwhere(active)
    cell_to_vertex = np.full((nx - 1, ny - 1, nz - 1), -1, dtype=np.int32)
    vertices: list[np.ndarray] = []

    for ci, cj, ck in active_cells:
        base = np.array([ci, cj, ck], dtype=np.int64)
        cell_values = corners[ci, cj, ck]
        intersections: list[np.ndarray] = []
        normals: list[np.ndarray] = []
        for a, b in CUBE_EDGES:
            va, vb = cell_values[a], cell_values[b]
            if (va < 0.0 <= vb) or (vb < 0.0 <= va):
                offset_a, offset_b = CUBE_CORNERS[a], CUBE_CORNERS[b]
                point_a = np.array([axes[t][base[t] + offset_a[t]] for t in range(3)])
                point_b = np.array([axes[t][base[t] + offset_b[t]] for t in range(3)])
                t = va / (va - vb)
                point = point_a + (point_b - point_a) * t
                grad_a = np.array([gradients[d][tuple(base + offset_a)] for d in range(3)])
                grad_b = np.array([gradients[d][tuple(base + offset_b)] for d in range(3)])
                normal = grad_a + (grad_b - grad_a) * t
                norm = np.linalg.norm(normal)
                normal = (point_b - point_a) / (np.linalg.norm(point_b - point_a) + 1.0e-12) if norm < 1.0e-12 else normal / norm
                intersections.append(point)
                normals.append(normal)

        if not intersections:
            continue
        point_array = np.asarray(intersections)
        normal_array = np.asarray(normals)
        rhs = np.einsum("ij,ij->i", normal_array, point_array)
        center = np.array([axes[t][base[t]] + 0.5 * spacing[t] for t in range(3)])
        regularization = 1.0e-6
        matrix = np.vstack([normal_array, np.sqrt(regularization) * np.eye(3)])
        target = np.concatenate([rhs, np.sqrt(regularization) * center])
        vertex = np.linalg.lstsq(matrix, target, rcond=None)[0]
        lower = np.array([axes[t][base[t]] for t in range(3)])
        vertex = np.clip(vertex, lower, lower + spacing)
        cell_to_vertex[ci, cj, ck] = len(vertices)
        vertices.append(vertex)

    quads: list[list[int]] = []

    # Each sign-changing primal edge is surrounded by four dual cells.
    for i in range(nx - 1):
        js, ks = np.where((samples[i, 1:-1, 1:-1] < 0) != (samples[i + 1, 1:-1, 1:-1] < 0))
        for jj, kk in zip(js, ks):
            j, k = jj + 1, kk + 1
            ids = [
                cell_to_vertex[i, j - 1, k - 1],
                cell_to_vertex[i, j, k - 1],
                cell_to_vertex[i, j, k],
                cell_to_vertex[i, j - 1, k],
            ]
            if min(ids) >= 0:
                if samples[i, j, k] >= 0:
                    ids.reverse()
                quads.append(ids)

    for j in range(ny - 1):
        ii, kk = np.where((samples[1:-1, j, 1:-1] < 0) != (samples[1:-1, j + 1, 1:-1] < 0))
        for iii, kkk in zip(ii, kk):
            i, k = iii + 1, kkk + 1
            ids = [
                cell_to_vertex[i - 1, j, k - 1],
                cell_to_vertex[i - 1, j, k],
                cell_to_vertex[i, j, k],
                cell_to_vertex[i, j, k - 1],
            ]
            if min(ids) >= 0:
                if samples[i, j, k] >= 0:
                    ids.reverse()
                quads.append(ids)

    for k in range(nz - 1):
        ii, jj = np.where((samples[1:-1, 1:-1, k] < 0) != (samples[1:-1, 1:-1, k + 1] < 0))
        for iii, jjj in zip(ii, jj):
            i, j = iii + 1, jjj + 1
            ids = [
                cell_to_vertex[i - 1, j - 1, k],
                cell_to_vertex[i, j - 1, k],
                cell_to_vertex[i, j, k],
                cell_to_vertex[i - 1, j, k],
            ]
            if min(ids) >= 0:
                if samples[i, j, k] >= 0:
                    ids.reverse()
                quads.append(ids)

    return np.asarray(vertices), np.asarray(quads, dtype=np.int64), int(len(active_cells))


DCSDFD_REFERENCE = {
    "package": "gpytoolbox",
    "version": "0.3.8",
    "sourceCommit": "b6e2c2b4330b0b7c0dcc5c84f6282b8fa6080ebe",
    "parameters": {
        "isovalue": 0.0,
        "outer_iters": 100,
        "inner_iters": 100,
        "batch_size": 200000,
        "hermite_update": True,
        "mu": 0.1,
        "dc_weight": 0.02,
        "svd_threshold": 0.01,
        "new_hermite_pos_weight": 0.2,
        "new_face_pos_weight": 0.2,
        "new_hermite_normal_weight": 0.2,
        "verbose": False,
    },
}


def maintained_dcsdfd_reference(
    samples: np.ndarray, axes: list[np.ndarray]
) -> tuple[np.ndarray, np.ndarray]:
    """Call the maintained, version-pinned SIGGRAPH 2026 DCSDFD API.

    S is the same int32-microunit sample vector used by the other methods,
    converted to float64 only at the API boundary. GV is assembled with the
    exact same first-axis-fastest order: i + nx * (j + ny * k).
    """
    try:
        installed = version("gpytoolbox")
    except PackageNotFoundError as exc:
        raise RuntimeError(
            "Install the pinned reference with "
            "'python -m pip install gpytoolbox==0.3.8' before using --include-dcsdfd"
        ) from exc
    if installed != DCSDFD_REFERENCE["version"]:
        raise RuntimeError(
            f"DCSDFD reference requires gpytoolbox=={DCSDFD_REFERENCE['version']}; "
            f"found {installed}"
        )
    import gpytoolbox as gpy

    dimensions = samples.shape
    if len(dimensions) != 3 or len(set(dimensions)) != 1:
        raise ValueError("DCSDFD adapter currently expects a cubic 3D grid")
    S = samples.ravel(order="F").astype(np.float64) / DISTANCE_SCALE
    coordinate_grids = np.meshgrid(*axes, indexing="ij")
    GV = np.column_stack(
        [coordinate.ravel(order="F") for coordinate in coordinate_grids]
    ).astype(np.float64, copy=False)
    if len(S) != int(np.prod(dimensions)) or GV.shape != (len(S), 3):
        raise ValueError("DCSDFD input flattening does not match the canonical grid")

    vertices, quads = gpy.dual_contouring_of_signed_distance_data(
        S,
        GV,
        int(dimensions[0]),
        int(dimensions[1]),
        int(dimensions[2]),
        **DCSDFD_REFERENCE["parameters"],
    )
    vertices = np.asarray(vertices, dtype=np.float64)
    quads = np.asarray(quads, dtype=np.int64)
    if vertices.ndim != 2 or vertices.shape[1] != 3:
        raise ValueError("maintained DCSDFD returned vertices other than N x 3")
    if quads.ndim != 2 or quads.shape[1] != 4:
        raise ValueError("maintained DCSDFD returned faces other than quad M x 4")
    if len(quads) and (quads.min() < 0 or quads.max() >= len(vertices)):
        raise ValueError("maintained DCSDFD returned an out-of-range face index")
    return vertices, quads


def time_trials(function, trials: int) -> tuple[object, list[float]]:
    outputs = []
    elapsed_ms = []
    for _ in range(trials):
        start = time.perf_counter()
        outputs.append(function())
        elapsed_ms.append((time.perf_counter() - start) * 1000.0)
    return outputs[-1], elapsed_ms


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--resolution", type=int, default=64)
    parser.add_argument("--smooth-width", type=float, default=0.004)
    parser.add_argument("--trials", type=int, default=3)
    parser.add_argument(
        "--include-dcsdfd",
        action="store_true",
        help="also run the maintained gpytoolbox==0.3.8 SIGGRAPH 2026 DCSDFD reference",
    )
    parser.add_argument("--dcsdfd-warmups", type=int, default=3)
    parser.add_argument("--dcsdfd-trials", type=int, default=10)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.resolution < 8 or args.resolution > 128 or args.trials < 1:
        parser.error("resolution must be 8..128 and trials must be >= 1")
    if args.dcsdfd_warmups < 0 or args.dcsdfd_trials < 1:
        parser.error("DCSDFD warmups must be >= 0 and DCSDFD trials must be >= 1")
    if args.smooth_width < 0.0 or args.smooth_width > 0.05:
        parser.error("smooth width must be 0..0.05")

    axes, samples, grid_sha256 = canonical_grid(args.resolution, args.smooth_width)
    (mc_mesh, mc_times) = time_trials(
        lambda: marching_cubes_reference(samples, args.resolution), args.trials
    )
    mc_vertices, mc_triangles = mc_mesh
    mc_result = topology_report(mc_vertices, mc_triangles, args.smooth_width)
    mc_result.update({
        "algorithm": "Lewiner marching cubes (scikit-image)",
        "trialTimesMs": mc_times,
        "medianTimeMs": float(np.median(mc_times)),
    })

    (dc_mesh, dc_times) = time_trials(
        lambda: classical_dual_contouring_reference(samples, axes), args.trials
    )
    dc_vertices, dc_quads, active_cells = dc_mesh
    if len(dc_quads):
        dc_triangles = np.vstack([dc_quads[:, [0, 1, 2]], dc_quads[:, [0, 2, 3]]])
    else:
        dc_triangles = np.empty((0, 3), dtype=np.int64)
    dc_result = topology_report(dc_vertices, dc_triangles, args.smooth_width)
    dc_result.update({
        "algorithm": "simple finite-gradient per-cell QEF dual contouring (research reference)",
        "quadsBeforeTriangulation": int(len(dc_quads)),
        "activeCells": active_cells,
        "quadTriangulation": "fixed diagonal 0-2",
        "trialTimesMs": dc_times,
        "medianTimeMs": float(np.median(dc_times)),
    })

    dcsdfd_result = None
    if args.include_dcsdfd:
        for _ in range(args.dcsdfd_warmups):
            maintained_dcsdfd_reference(samples, axes)
        (dcsdfd_mesh, dcsdfd_times) = time_trials(
            lambda: maintained_dcsdfd_reference(samples, axes),
            args.dcsdfd_trials,
        )
        dcsdfd_vertices, dcsdfd_quads = dcsdfd_mesh
        if len(dcsdfd_quads):
            dcsdfd_triangles = np.vstack([
                dcsdfd_quads[:, [0, 1, 2]],
                dcsdfd_quads[:, [0, 2, 3]],
            ])
        else:
            dcsdfd_triangles = np.empty((0, 3), dtype=np.int64)
        dcsdfd_result = topology_report(
            dcsdfd_vertices, dcsdfd_triangles, args.smooth_width
        )
        dcsdfd_result.update({
            "algorithm": "maintained SIGGRAPH 2026 DCSDFD (gpytoolbox)",
            "referenceVersion": DCSDFD_REFERENCE["version"],
            "referenceSourceCommit": DCSDFD_REFERENCE["sourceCommit"],
            "parameters": DCSDFD_REFERENCE["parameters"],
            "quads": int(len(dcsdfd_quads)),
            "quadIndexCount": int(len(dcsdfd_quads) * 4),
            "estimatedQuadBufferBytes": int(
                len(dcsdfd_vertices) * 36 + len(dcsdfd_quads) * 16
            ),
            "triangulationForCommonTopologyChecks": "fixed diagonal 0-2 only",
            "trialTimesMs": dcsdfd_times,
            "medianTimeMs": float(np.median(dcsdfd_times)),
            "p95TimeMs": float(np.percentile(dcsdfd_times, 95)),
            "p99TimeMs": float(np.percentile(dcsdfd_times, 99)),
            "maximumTimeMs": float(np.max(dcsdfd_times)),
        })

    report = {
        "schemaVersion": 1,
        "status": "exploratory-python-reference-not-qualified",
        "fixtureId": "ouroboros-face-semantic-sdf-v1",
        "field": {
            "estimator": "IQ-style approximate ellipsoid distance",
            "exactEuclideanSdf": False,
            "smoothUnionWidth": args.smooth_width,
            "bounds": [BOUNDS_MIN.tolist(), BOUNDS_MAX.tolist()],
            "distanceQuantization": DISTANCE_SCALE,
            "sampleOrder": "i + nx * (j + ny * k)",
            "samplesSha256": grid_sha256,
        },
        "grid": [args.resolution] * 3,
        "environment": {
            "python": platform.python_version(),
            "numpy": np.__version__,
            "scikitImage": skimage.__version__,
            "trimesh": trimesh.__version__,
            "platform": platform.platform(),
        },
        "referenceConfiguration": {
            "id": "gpytoolbox-dcsdd-v0.3.8",
            "included": bool(args.include_dcsdfd),
            "sourceCommit": DCSDFD_REFERENCE["sourceCommit"],
            "parameters": DCSDFD_REFERENCE["parameters"],
            "warmupRuns": args.dcsdfd_warmups if args.include_dcsdfd else None,
            "measuredTrials": args.dcsdfd_trials if args.include_dcsdfd else None,
            "sampleConversion": "int32 microunits -> float64 / 1_000_000 at API boundary",
            "gridVertexOrder": "i + nx * (j + ny * k)",
        },
        "methods": [mc_result, dc_result] + ([dcsdfd_result] if dcsdfd_result is not None else []),
        "limitations": [
            "Both methods receive the same canonical int32 microunit sample grid.",
            "The DC implementation is a simple finite-difference QEF reference, not the SIGGRAPH 2026 algorithm.",
            "The maintained DCSDFD reference is optional and requires the exact gpytoolbox 0.3.8 package; its canonical package/source pin is recorded in dcsdd-reference-config-v1.json.",
            "The timing comparison crosses a compiled scikit-image extractor and a Python-loop QEF implementation; timings are not an algorithmic performance comparison.",
            "The residual is evaluated against the same approximate field, not exact Euclidean geometry.",
            "No self-intersection or independent manifold test is run.",
            "A quad mesh is triangulated on a fixed diagonal only for common Trimesh topology checks.",
            "No production Rust, GPU or human perceptual qualification is included.",
        ],
    }
    encoded = json.dumps(report, indent=2) + "\n"
    if args.output:
        args.output.write_text(encoded, encoding="utf-8")
    else:
        print(encoded, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
