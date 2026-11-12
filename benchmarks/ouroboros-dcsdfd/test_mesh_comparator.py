import struct
import tempfile
import unittest
from pathlib import Path

import numpy as np

import compare_classical_to_dcsdfd as compare


def make_grid():
    fixture = b"compare-grid"
    descriptor = b"descriptor-v1"
    bounds = (0, 0, 0, 1_000_000, 2_000_000, 3_000_000)
    # Exact sampled field S=x on a [0,1] x [0,2] x [0,3] grid.
    samples = [0, 1_000_000, 0, 1_000_000, 0, 1_000_000, 0, 1_000_000]
    out = bytearray(b"OUROGRD1")
    out.extend(struct.pack("<H", 1))
    out.extend(struct.pack("<H", len(fixture)))
    out.extend(fixture)
    out.extend(struct.pack("<I", len(descriptor)))
    out.extend(descriptor)
    out.extend(struct.pack("<III", 2, 2, 2))
    out.extend(bytes((1, 2, 3, 1)))
    out.extend(struct.pack("<6q", *bounds))
    out.extend(struct.pack("<q", 1_000_000))
    out.extend(struct.pack("<Q", 8))
    for sample in samples:
        out.extend(struct.pack("<i", sample))
    return bytes(out)


def make_dc_receipt(source_grid):
    out = bytearray(b"OURODCQ1")
    out.extend(struct.pack("<H", 1))
    out.extend(struct.pack("<I", len(source_grid)))
    out.extend(source_grid)
    out.extend(bytes(37))  # canonical DualContourConfig bytes
    vertices = [
        (200_000_000, 200_000_000, 500_000_000),
        (800_000_000, 200_000_000, 500_000_000),
        (800_000_000, 800_000_000, 500_000_000),
        (200_000_000, 800_000_000, 500_000_000),
    ]
    out.extend(struct.pack("<I", len(vertices)))
    for x, y, z in vertices:
        out.extend(struct.pack("<3q3iI", x, y, z, 0, 0, 1_000_000_000, 9))
    quads = [(0, 1, 2, 3)]
    out.extend(struct.pack("<I", len(quads)))
    for face in quads:
        out.extend(struct.pack("<4I", *face))
    triangles = [(0, 1, 2), (0, 2, 3)]
    out.extend(struct.pack("<I", len(triangles)))
    for face in triangles:
        out.extend(struct.pack("<3I", *face))
    out.extend(struct.pack("<5I5Q", 4, 4, 0, 0, 0, 0, 0, 0, 160, 1024))
    out.extend(struct.pack("<I", 0))  # empty audit payload for parser unit test
    return bytes(out)


class DualContourReceiptTests(unittest.TestCase):
    def write_bytes(self, data):
        handle = tempfile.NamedTemporaryFile(delete=False)
        handle.write(data)
        handle.close()
        path = Path(handle.name)
        self.addCleanup(path.unlink, missing_ok=True)
        return path

    def test_ourodcq1_parser_recovers_exact_source_digest_and_mesh_arrays(self):
        grid = make_grid()
        receipt = self.write_bytes(make_dc_receipt(grid))
        mesh = compare.parse_ourodcq1(receipt, np)
        self.assertEqual(mesh["embedded_grid_sha256"], compare.sha256_bytes(grid))
        self.assertEqual(mesh["vertices"].shape, (4, 3))
        self.assertEqual(mesh["quads"].tolist(), [[0, 1, 2, 3]])
        self.assertEqual(mesh["triangles"].tolist(), [[0, 1, 2], [0, 2, 3]])
        self.assertEqual(mesh["metrics"]["active_cell_count"], 4)

    def test_ourodcq1_parser_rejects_trailing_bytes(self):
        data = make_dc_receipt(make_grid()) + b"x"
        with self.assertRaises(compare.ComparisonError):
            compare.parse_ourodcq1(self.write_bytes(data), np)

    def test_basic_topology_summary_counts_boundary_edges_and_components(self):
        vertices = np.array([
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ])
        triangles = np.array([[0, 1, 2], [0, 2, 3]], dtype=np.int64)
        report = compare.topology_summary(vertices, triangles, np, 1e-20)
        self.assertEqual(report["boundary_edge_count"], 4)
        self.assertEqual(report["nonmanifold_edge_count"], 0)
        self.assertEqual(report["orientation_conflict_edge_count"], 0)
        self.assertEqual(report["edge_connected_face_component_count"], 1)
        self.assertEqual(report["zero_or_near_zero_area_triangle_count"], 0)

    def test_topology_summary_exposes_same_direction_shared_edge(self):
        vertices = np.array([
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
        ])
        triangles = np.array([[0, 1, 2], [0, 1, 3]], dtype=np.int64)
        report = compare.topology_summary(vertices, triangles, np, 1e-20)
        self.assertEqual(report["orientation_conflict_edge_count"], 1)

    def test_trilinear_residual_is_to_quantized_grid_not_claimed_ground_truth(self):
        grid_path = self.write_bytes(make_grid())
        grid = compare.parse_ourogrd1(grid_path, np)
        points = np.array([[0.5, 0.5, 0.5], [0.25, 1.0, 1.5]])
        result = compare.sampled_grid_residual(points, grid, np)
        self.assertEqual(result["inside_grid_vertex_count"], 2)
        self.assertEqual(result["outside_grid_vertex_count"], 0)
        self.assertAlmostEqual(result["absolute_residual"]["p50"], 0.375)
        self.assertIn("not exact-SDF error", result["definition"])


if __name__ == "__main__":
    unittest.main()
