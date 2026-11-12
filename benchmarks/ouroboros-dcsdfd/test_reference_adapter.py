import struct
import tempfile
import unittest
from pathlib import Path

import numpy as np

import reference_adapter as adapter


def make_face_descriptor(head_region=1, nose_region=4, union_mode=0, union_width=0):
    descriptor = bytearray(b"OUROFLD1")
    descriptor.extend(struct.pack("<H", 1))
    descriptor.extend(bytes((1, 1, 1, 1)))
    descriptor.extend(struct.pack(
        "<H3q3q", head_region, 30_000, 0, 0, 120_000, 60_000, 110_000
    ))
    descriptor.extend(struct.pack(
        "<H3q3q", nose_region, 160_000, 0, 5_000, 20_000, 14_000, 18_000
    ))
    descriptor.append(union_mode)
    descriptor.extend(struct.pack("<q", union_width))
    assert len(descriptor) == 123
    return bytes(descriptor)


def make_grid(
    dimensions=(2, 2, 2),
    axes=(1, 2, 3, 1),
    samples=None,
    declared_count=None,
    descriptor=None,
    distance_scale=1_000_000,
):
    nx, ny, nz = dimensions
    count = nx * ny * nz
    if samples is None:
        samples = list(range(-count, 0))
    if declared_count is None:
        declared_count = count
    fixture = b"ouroboros-face-skin-v1"
    if descriptor is None:
        descriptor = make_face_descriptor()
    bounds = (0, 0, 0, 1_000_000, 2_000_000, 3_000_000)
    payload = bytearray(b"OUROGRD1")
    payload.extend(struct.pack("<H", 1))
    payload.extend(struct.pack("<H", len(fixture)))
    payload.extend(fixture)
    payload.extend(struct.pack("<I", len(descriptor)))
    payload.extend(descriptor)
    payload.extend(struct.pack("<III", nx, ny, nz))
    payload.extend(bytes(axes))
    payload.extend(struct.pack("<6q", *bounds))
    payload.extend(struct.pack("<q", distance_scale))
    payload.extend(struct.pack("<Q", declared_count))
    for sample in samples:
        payload.extend(struct.pack("<i", sample))
    return bytes(payload)


class CanonicalGridReaderTests(unittest.TestCase):
    def write_fixture(self, payload):
        temp = tempfile.NamedTemporaryFile(delete=False)
        temp.write(payload)
        temp.close()
        self.addCleanup(Path(temp.name).unlink, missing_ok=True)
        return Path(temp.name)

    def test_valid_grid_preserves_first_axis_fastest_samples_and_bounds(self):
        payload = make_grid(samples=[-1_000_000, -500_000, 0, 500_000,
                                     1_000_000, 1_500_000, 2_000_000, 2_500_000])
        path = self.write_fixture(payload)
        grid = adapter.parse_ourogrd1(path, np)
        self.assertEqual(grid["dimensions"], (2, 2, 2))
        self.assertEqual(grid["samples"].tolist(), [-1.0, -0.5, 0.0, 0.5, 1.0, 1.5, 2.0, 2.5])
        self.assertEqual(grid["minimum"], [0.0, 0.0, 0.0])
        self.assertEqual(grid["maximum"], [1.0, 2.0, 3.0])
        self.assertEqual(grid["fixture_id"], "ouroboros-face-skin-v1")
        self.assertEqual(grid["field"]["field_semantics"], "approximate-analytic-ellipsoid-field")
        self.assertEqual(grid["field"]["union_mode"], "sharp")
        self.assertEqual(grid["grid_sha256"], adapter.sha256_bytes(payload))

    def test_grid_vertex_order_matches_i_plus_nx_times_j_plus_ny_times_k(self):
        payload = make_grid()
        grid = adapter.parse_ourogrd1(self.write_fixture(payload), np)
        vertices = adapter.build_grid_vertices(grid, np)
        expected = np.array([
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 2.0, 0.0],
            [1.0, 2.0, 0.0],
            [0.0, 0.0, 3.0],
            [1.0, 0.0, 3.0],
            [0.0, 2.0, 3.0],
            [1.0, 2.0, 3.0],
        ])
        np.testing.assert_array_equal(vertices, expected)

    def test_wrong_magic_is_rejected(self):
        payload = bytearray(make_grid())
        payload[0:8] = b"NOTGRID!"
        with self.assertRaises(adapter.InputError):
            adapter.parse_ourogrd1(self.write_fixture(bytes(payload)), np)

    def test_wrong_axis_or_layout_marker_is_rejected(self):
        payload = make_grid(axes=(1, 2, 3, 0))
        with self.assertRaises(adapter.InputError):
            adapter.parse_ourogrd1(self.write_fixture(payload), np)

    def test_wrong_field_magic_and_descriptor_length_are_rejected(self):
        descriptor = bytearray(make_face_descriptor())
        descriptor[0:8] = b"NOTFIELD"
        payload = make_grid(descriptor=bytes(descriptor))
        with self.assertRaises(adapter.InputError):
            adapter.parse_ourogrd1(self.write_fixture(payload), np)
        payload = make_grid(descriptor=b"short")
        with self.assertRaises(adapter.InputError):
            adapter.parse_ourogrd1(self.write_fixture(payload), np)

    def test_wrong_semantic_regions_and_union_parameters_are_rejected(self):
        with self.assertRaises(adapter.InputError):
            adapter.parse_ourogrd1(
                self.write_fixture(make_grid(descriptor=make_face_descriptor(head_region=2))),
                np,
            )
        with self.assertRaises(adapter.InputError):
            adapter.parse_ourogrd1(
                self.write_fixture(make_grid(descriptor=make_face_descriptor(union_mode=0, union_width=1))),
                np,
            )
        with self.assertRaises(adapter.InputError):
            adapter.parse_ourogrd1(
                self.write_fixture(make_grid(descriptor=make_face_descriptor(union_mode=1, union_width=0))),
                np,
            )

    def test_noncanonical_distance_scale_and_reversed_bounds_are_rejected(self):
        with self.assertRaises(adapter.InputError):
            adapter.parse_ourogrd1(
                self.write_fixture(make_grid(distance_scale=1000)),
                np,
            )
        payload = bytearray(make_grid())
        # First-axis maximum is the first i64 after the three minimum bounds.
        bounds_offset = 8 + 2 + 2 + len(b"ouroboros-face-skin-v1") + 4 + 123 + 12 + 4
        struct.pack_into("<q", payload, bounds_offset + 24, -1)
        with self.assertRaises(adapter.InputError):
            adapter.parse_ourogrd1(self.write_fixture(bytes(payload)), np)

    def test_sample_count_mismatch_is_rejected_before_sample_decode(self):
        payload = make_grid(declared_count=7)
        with self.assertRaises(adapter.InputError):
            adapter.parse_ourogrd1(self.write_fixture(payload), np)

    def test_oversized_grid_is_rejected_before_sample_decode(self):
        dimensions = (129, 129, 129)
        payload = make_grid(dimensions=dimensions, samples=[], declared_count=129**3)
        with self.assertRaises(adapter.InputError):
            adapter.parse_ourogrd1(self.write_fixture(payload), np)

    def test_array_digest_binds_shape_and_dtype(self):
        a = np.array([[1.0, 2.0]], dtype=np.float64)
        b = np.array([1.0, 2.0], dtype=np.float64)
        self.assertNotEqual(adapter.stable_array_hash(a, np), adapter.stable_array_hash(b, np))

    def test_fixed_seed_diff_accepts_exact_rng_only_change(self):
        diff = """diff --git a/src.cpp b/src.cpp
--- a/src.cpp
+++ b/src.cpp
@@ -1,5 +1,4 @@
-        std::random_device rd;
-        std::mt19937 gen(rd());
+        std::mt19937 gen(0x4F55524FU); // fixed seed for Ouroboros differential trials
"""
        self.assertTrue(adapter.fixed_seed_diff_matches(diff))

    def test_fixed_seed_diff_rejects_extra_or_altered_source_changes(self):
        diff = """diff --git a/src.cpp b/src.cpp
--- a/src.cpp
+++ b/src.cpp
@@ -1,6 +1,5 @@
-        std::random_device rd;
-        std::mt19937 gen(rd());
+        std::mt19937 gen(1U);
+        // unrelated alteration
"""
        self.assertFalse(adapter.fixed_seed_diff_matches(diff))


if __name__ == "__main__":
    unittest.main()
