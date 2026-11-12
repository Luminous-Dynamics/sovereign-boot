import struct
import tempfile
import unittest
from pathlib import Path

import numpy as np

import compare_classical_to_dcsdfd as compare


def make_face_descriptor():
    descriptor = bytearray(b"OUROFLD1")
    descriptor.extend(struct.pack("<H", 1))
    descriptor.extend(bytes((1, 1, 1, 1)))
    descriptor.extend(struct.pack(
        "<H3q3q", 1, 30_000, 0, 0, 120_000, 60_000, 110_000
    ))
    descriptor.extend(struct.pack(
        "<H3q3q", 4, 160_000, 0, 5_000, 20_000, 14_000, 18_000
    ))
    descriptor.append(0)
    descriptor.extend(struct.pack("<q", 0))
    assert len(descriptor) == 123
    return bytes(descriptor)


def make_grid():
    fixture = b"ouroboros-face-skin-v1"
    descriptor = make_face_descriptor()
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
    def write_reference_run_fixture(self, run_dir, vertices, quads):
        np.save(run_dir / "vertices.npy", vertices)
        np.save(run_dir / "quads.npy", quads)
        manifest = {
            "schema_version": 1,
            "reference": {
                "repository": "sgsellan/gpytoolbox",
                "commit": "d6e6ae3b5ff9200e687ca19c178ff35d65c1f810",
                "source_mode": "fixed-seed-patched",
                "source_file": "src/cpp/dual_contouring_of_signed_distance_data/contouring.cpp",
                "source_file_sha256": "a" * 64,
                "patch_sha256": "71b1983492b906935b95588368bdf7c11a87a47ed4ae0fa8694ea5d2f95386ee",
            },
            "native_binary": {
                "extension_path": "/pinned/gpytoolbox/_dcsdd_cpp_impl.so",
                "extension_sha256": "b" * 64,
                "extension_inside_reference_checkout": True,
                "binary_source_binding": "path-and-binary-digest-recorded; build provenance is not independently attested",
            },
            "parameters": {"isovalue": 0.0},
            "input": {
                "magic": "OUROGRD1",
                "grid_sha256": "c" * 64,
                "field_descriptor_sha256": "d" * 64,
                "resampled": False,
                "field_reevaluated": False,
            },
            "result": {
                "vertices_file": "vertices.npy",
                "quads_file": "quads.npy",
                "vertices_sha256": compare.stable_array_hash(vertices, np),
                "quads_sha256": compare.stable_array_hash(quads, np),
                "vertex_count": len(vertices),
                "quad_count": len(quads),
            },
        }
        (run_dir / "run-v1.json").write_text(
            __import__("json").dumps(manifest), encoding="utf-8"
        )
        return manifest

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

    def test_deterministic_surface_sampling_is_zero_for_identical_triangle_surfaces(self):
        vertices = np.array([
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        ])
        triangles = np.array([[0, 1, 2]], dtype=np.int64)
        result = compare.symmetric_surface_sampled_distance(
            vertices, triangles, vertices.copy(), triangles.copy(), 4096, np
        )
        self.assertEqual(result["sample_count_per_surface"], 4096)
        self.assertEqual(result["symmetric_max_sample_distance"], 0.0)
        self.assertEqual(result["reference_degenerate_triangles_excluded"], 0)

    def test_deterministic_surface_sampling_detects_rigid_translation(self):
        vertices = np.array([
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        ])
        triangles = np.array([[0, 1, 2]], dtype=np.int64)
        shifted = vertices + np.array([0.0, 0.0, 0.25])
        result = compare.symmetric_surface_sampled_distance(
            vertices, triangles, shifted, triangles, 4096, np
        )
        self.assertAlmostEqual(result["symmetric_max_sample_distance"], 0.25)
        self.assertAlmostEqual(result["candidate_to_reference"]["mean"], 0.25)

    def test_surface_sampler_excludes_degenerate_faces_but_requires_positive_area(self):
        vertices = np.array([
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [2.0, 2.0, 2.0],
        ])
        triangles = np.array([[0, 1, 2], [0, 0, 3]], dtype=np.int64)
        points, degenerate = compare.deterministic_surface_samples(
            vertices, triangles, 256, np
        )
        self.assertEqual(points.shape, (256, 3))
        self.assertEqual(degenerate, 1)
        with self.assertRaises(compare.ComparisonError):
            compare.deterministic_surface_samples(
                vertices, np.array([[0, 0, 0]], dtype=np.int64), 256, np
            )

    def test_surface_sampling_is_deterministic_across_repeated_calls(self):
        vertices = np.array([
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.2],
            [0.0, 1.0, 0.0],
        ])
        triangles = np.array([[0, 1, 2], [0, 2, 3]], dtype=np.int64)
        first, first_degenerate = compare.deterministic_surface_samples(
            vertices, triangles, 2048, np
        )
        second, second_degenerate = compare.deterministic_surface_samples(
            vertices, triangles, 2048, np
        )
        np.testing.assert_array_equal(first, second)
        self.assertEqual(first_degenerate, second_degenerate)

    def test_reference_run_rejects_nonzero_isovalue_and_unbound_inputs(self):
        run_dir = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: __import__("shutil").rmtree(run_dir, ignore_errors=True))
        vertices = np.array([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]])
        quads = np.array([[0, 1, 2, 0]], dtype=np.int64)
        np.save(run_dir / "vertices.npy", vertices)
        np.save(run_dir / "quads.npy", quads)
        manifest = {
            "schema_version": 1,
            "reference": {
                "repository": "sgsellan/gpytoolbox",
                "commit": "d6e6ae3b5ff9200e687ca19c178ff35d65c1f810",
                "source_mode": "fixed-seed-patched",
                "source_file": "src/cpp/dual_contouring_of_signed_distance_data/contouring.cpp",
                "source_file_sha256": "a" * 64,
                "patch_sha256": "71b1983492b906935b95588368bdf7c11a87a47ed4ae0fa8694ea5d2f95386ee",
            },
            "native_binary": {
                "extension_path": "/pinned/gpytoolbox/_dcsdd_cpp_impl.so",
                "extension_sha256": "b" * 64,
                "extension_inside_reference_checkout": True,
                "binary_source_binding": "path-and-binary-digest-recorded; build provenance is not independently attested",
            },
            "parameters": {"isovalue": 1.0},
            "input": {
                "magic": "OUROGRD1",
                "grid_sha256": "c" * 64,
                "field_descriptor_sha256": "d" * 64,
                "resampled": False,
                "field_reevaluated": False,
            },
            "result": {
                "vertices_file": "vertices.npy", "quads_file": "quads.npy",
                "vertices_sha256": compare.stable_array_hash(vertices, np),
                "quads_sha256": compare.stable_array_hash(quads, np),
                "vertex_count": len(vertices), "quad_count": len(quads),
            },
        }
        (run_dir / "run-v1.json").write_text(__import__("json").dumps(manifest), encoding="utf-8")
        with self.assertRaises(compare.ComparisonError):
            compare.load_reference_run(run_dir, np)
        manifest["parameters"]["isovalue"] = 0.0
        manifest["input"]["resampled"] = True
        (run_dir / "run-v1.json").write_text(__import__("json").dumps(manifest), encoding="utf-8")
        with self.assertRaises(compare.ComparisonError):
            compare.load_reference_run(run_dir, np)

    def test_reference_run_requires_loaded_native_binary_digest(self):
        run_dir = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: __import__("shutil").rmtree(run_dir, ignore_errors=True))
        vertices = np.array([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]])
        quads = np.array([[0, 1, 2, 0]], dtype=np.int64)
        np.save(run_dir / "vertices.npy", vertices)
        np.save(run_dir / "quads.npy", quads)
        manifest = {
            "schema_version": 1,
            "reference": {
                "repository": "sgsellan/gpytoolbox",
                "commit": "d6e6ae3b5ff9200e687ca19c178ff35d65c1f810",
                "source_mode": "fixed-seed-patched",
                "source_file": "src/cpp/dual_contouring_of_signed_distance_data/contouring.cpp",
                "source_file_sha256": "a" * 64,
                "patch_sha256": "71b1983492b906935b95588368bdf7c11a87a47ed4ae0fa8694ea5d2f95386ee",
            },
            "native_binary": {
                "extension_path": "/pinned/gpytoolbox/_dcsdd_cpp_impl.so",
                "extension_sha256": "b" * 64,
                "extension_inside_reference_checkout": True,
                "binary_source_binding": "path-and-binary-digest-recorded; build provenance is not independently attested",
            },
            "parameters": {"isovalue": 0.0},
            "input": {
                "magic": "OUROGRD1",
                "grid_sha256": "c" * 64,
                "field_descriptor_sha256": "d" * 64,
                "resampled": False,
                "field_reevaluated": False,
            },
            "result": {
                "vertices_file": "vertices.npy", "quads_file": "quads.npy",
                "vertices_sha256": compare.stable_array_hash(vertices, np),
                "quads_sha256": compare.stable_array_hash(quads, np),
                "vertex_count": len(vertices), "quad_count": len(quads),
            },
        }
        manifest.pop("native_binary")
        (run_dir / "run-v1.json").write_text(__import__("json").dumps(manifest), encoding="utf-8")
        with self.assertRaisesRegex(compare.ComparisonError, "native extension digest"):
            compare.load_reference_run(run_dir, np)
        manifest["native_binary"] = {
            "extension_path": "/pinned/gpytoolbox/_dcsdd_cpp_impl.so",
            "extension_sha256": "not-a-sha256",
            "extension_inside_reference_checkout": True,
            "binary_source_binding": "path-and-binary-digest-recorded; build provenance is not independently attested",
        }
        (run_dir / "run-v1.json").write_text(__import__("json").dumps(manifest), encoding="utf-8")
        with self.assertRaisesRegex(compare.ComparisonError, "native extension digest"):
            compare.load_reference_run(run_dir, np)

    def test_reference_run_rejects_float32_vertices_before_dtype_normalization(self):
        run_dir = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: __import__("shutil").rmtree(run_dir, ignore_errors=True))
        vertices = np.array(
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            dtype=np.float32,
        )
        quads = np.array([[0, 1, 2, 0]], dtype=np.int64)
        self.write_reference_run_fixture(run_dir, vertices, quads)
        with self.assertRaisesRegex(compare.ComparisonError, "float64 storage"):
            compare.load_reference_run(run_dir, np)

    def test_reference_run_rejects_float_quad_indices_before_integer_cast(self):
        run_dir = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: __import__("shutil").rmtree(run_dir, ignore_errors=True))
        vertices = np.array(
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            dtype=np.float64,
        )
        quads = np.array([[0.0, 1.0, 2.0, 0.0]], dtype=np.float64)
        self.write_reference_run_fixture(run_dir, vertices, quads)
        with self.assertRaisesRegex(compare.ComparisonError, "signed int64"):
            compare.load_reference_run(run_dir, np)

    def test_reference_run_rejects_artifact_path_escape(self):
        run_dir = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: __import__("shutil").rmtree(run_dir.parent, ignore_errors=True))
        vertices = np.array(
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            dtype=np.float64,
        )
        quads = np.array([[0, 1, 2, 0]], dtype=np.int64)
        manifest = self.write_reference_run_fixture(run_dir, vertices, quads)
        outside = run_dir.parent / "outside.npy"
        np.save(outside, vertices)
        manifest["result"]["vertices_file"] = "../outside.npy"
        (run_dir / "run-v1.json").write_text(
            __import__("json").dumps(manifest), encoding="utf-8"
        )
        with self.assertRaisesRegex(compare.ComparisonError, "simple basename"):
            compare.load_reference_run(run_dir, np)

    def test_reference_input_binding_includes_descriptor_geometry_and_grid_metadata(self):
        grid_path = self.write_bytes(make_grid())
        grid = compare.parse_ourogrd1(grid_path, np)
        input_record = {
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
        compare.validate_reference_input_binding({"input": input_record}, grid)
        input_record["field_descriptor_sha256"] = "0" * 64
        with self.assertRaisesRegex(compare.ComparisonError, "field_descriptor_sha256"):
            compare.validate_reference_input_binding({"input": input_record}, grid)

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
