#!/usr/bin/env python3
"""Unit tests for the source-bound Component Model evidence collector."""
from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("collect-evidence.py")
SPEC = importlib.util.spec_from_file_location("component_evidence_collector", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
collector = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(collector)


class EvidenceCollectorTests(unittest.TestCase):
    def test_complete_runtime_success_record_has_no_missing_markers(self) -> None:
        log = " ".join(collector.REQUIRED_MARKERS)
        self.assertEqual([], collector.missing_required_markers(log))

    def test_each_required_assertion_marker_is_mandatory(self) -> None:
        for missing in collector.REQUIRED_MARKERS:
            with self.subTest(marker=missing):
                log = " ".join(m for m in collector.REQUIRED_MARKERS if m != missing)
                self.assertIn(missing, collector.missing_required_markers(log))

    def test_source_file_record_has_byte_count_and_sha256(self) -> None:
        record = collector.file_record("tests/fixtures/first-germination.scene.json")
        self.assertEqual("tests/fixtures/first-germination.scene.json", record["path"])
        self.assertGreater(record["bytes"], 0)
        self.assertRegex(record["sha256"], r"^[0-9a-f]{64}$")

    def test_source_path_cannot_escape_repository(self) -> None:
        with self.assertRaises(ValueError):
            collector.file_record("../../../../etc/passwd")

    def test_source_path_must_exist(self) -> None:
        with self.assertRaises(ValueError):
            collector.file_record("does-not-exist-for-evidence-test.file")


if __name__ == "__main__":
    unittest.main(verbosity=2)
