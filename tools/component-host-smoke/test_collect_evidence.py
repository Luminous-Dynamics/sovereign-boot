#!/usr/bin/env python3
"""Unit tests for the source-bound Component Model evidence collector."""
from __future__ import annotations

import contextlib
import hashlib
import importlib.util
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

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
        prefix = collector.REQUIRED_MARKERS[0]
        for missing in collector.REQUIRED_MARKERS:
            with self.subTest(marker=missing):
                if missing == prefix:
                    # Without the receipt prefix, the collector must reject the
                    # whole line as a receipt rather than treating log fragments as one.
                    log = " ".join(m for m in collector.REQUIRED_MARKERS[1:])
                    self.assertIn(
                        "exactly one component_runtime receipt line",
                        collector.missing_required_markers(log),
                    )
                else:
                    log = " ".join(
                        m for m in collector.REQUIRED_MARKERS if m != missing
                    )
                    self.assertIn(missing, collector.missing_required_markers(log))

    def test_markers_split_across_lines_do_not_form_a_receipt(self) -> None:
        midpoint = len(collector.REQUIRED_MARKERS) // 2
        first = " ".join(collector.REQUIRED_MARKERS[:midpoint])
        second = " ".join(collector.REQUIRED_MARKERS[midpoint:])
        self.assertIn(
            "exactly one component_runtime receipt line",
            collector.missing_required_markers(first + "\\n" + second),
        )

    def test_duplicate_receipt_lines_are_rejected(self) -> None:
        line = " ".join(collector.REQUIRED_MARKERS)
        self.assertIn(
            "exactly one component_runtime receipt line",
            collector.missing_required_markers(line + "\\n" + line),
        )

    def test_generated_host_lock_is_artifact_not_committed_source(self) -> None:
        self.assertNotIn("tools/component-host-smoke/Cargo.lock", collector.SOURCE_PATHS)

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

    def test_missing_assertion_prevents_pass_receipt(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            log = root / "runtime.log"
            output = root / "qualification.json"
            log.write_text(" ".join(collector.REQUIRED_MARKERS[:-1]), encoding="utf-8")
            argv = [
                str(SCRIPT),
                "--component", str(root / "component.wasm"),
                "--fixture", str(collector.ROOT / "tests/fixtures/first-germination.scene.json"),
                "--lockfile", str(root / "Cargo.lock"),
                "--runtime-log", str(log),
                "--rustc-version", "rustc test-version",
                "--output", str(output),
            ]
            with patch.object(sys, "argv", argv), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(2, collector.main())
            self.assertFalse(output.exists(), "failed assertions must not produce a pass receipt")

    def test_complete_record_binds_receipt_to_artifact_hashes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            component = root / "component.wasm"
            lockfile = root / "Cargo.lock"
            log = root / "runtime.log"
            output = root / "qualification.json"
            component_bytes = b"synthetic component bytes for collector unit test"
            lock_bytes = b"synthetic host lock for collector unit test"
            component.write_bytes(component_bytes)
            lockfile.write_bytes(lock_bytes)
            log.write_text(" ".join(collector.REQUIRED_MARKERS), encoding="utf-8")
            fixture = collector.ROOT / "tests/fixtures/first-germination.scene.json"
            argv = [
                str(SCRIPT),
                "--component", str(component),
                "--fixture", str(fixture),
                "--lockfile", str(lockfile),
                "--runtime-log", str(log),
                "--rustc-version", "rustc test-version",
                "--output", str(output),
            ]
            mock_records = lambda relative: {"path": relative, "bytes": 1, "sha256": "a" * 64}
            with (
                patch.object(sys, "argv", argv),
                patch.object(collector, "git_output", side_effect=["commit-test", "tree-test"]),
                patch.object(collector, "file_record", side_effect=mock_records),
                contextlib.redirect_stdout(io.StringIO()),
            ):
                self.assertEqual(0, collector.main())
            evidence = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual("pass", evidence["result"])
            self.assertEqual("commit-test", evidence["source"]["commit"])
            self.assertEqual("tree-test", evidence["source"]["tree"])
            self.assertEqual(
                hashlib.sha256(component_bytes).hexdigest(),
                evidence["artifacts"]["component"]["sha256"],
            )
            self.assertEqual(
                hashlib.sha256(lock_bytes).hexdigest(),
                evidence["artifacts"]["host_lockfile"]["sha256"],
            )
            self.assertFalse(evidence["scope"]["physical_boot"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
