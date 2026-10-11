#!/usr/bin/env python3
"""Unit tests for the source-bound Component Model evidence collector."""
from __future__ import annotations

import contextlib
import hashlib
import importlib.util
import io
import json
import os
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

PACKET_SCRIPT = Path(__file__).with_name("assemble-packet.py")
PACKET_SPEC = importlib.util.spec_from_file_location("component_evidence_packet", PACKET_SCRIPT)
assert PACKET_SPEC is not None and PACKET_SPEC.loader is not None
packet = importlib.util.module_from_spec(PACKET_SPEC)
PACKET_SPEC.loader.exec_module(packet)


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
        missing = collector.missing_required_markers(first + "\n" + second)
        # Only the first line is eligible as the receipt; assertions present
        # solely on the continuation line must still be reported missing.
        self.assertTrue(missing)
        for marker in collector.REQUIRED_MARKERS[midpoint:]:
            self.assertIn(marker, missing)

    def test_marker_substring_cannot_satisfy_required_assertion(self) -> None:
        line = " ".join(collector.REQUIRED_MARKERS).replace(
            "branch_budget=pass", "branch_budget=pass-extra"
        )
        self.assertIn(
            "branch_budget=pass",
            collector.missing_required_markers(line),
        )

    def test_lockfile_is_resolved_before_receipt_tests(self) -> None:
        workflow = (collector.ROOT / ".github/workflows/component-runtime-qualification.yml").read_text(
            encoding="utf-8"
        )
        self.assertLess(
            workflow.index("- name: Resolve isolated host dependency graph"),
            workflow.index("- name: Test evidence receipt validation"),
        )
        self.assertIn("run: python3 -B tools/component-host-smoke/test_collect_evidence.py", workflow)
        self.assertIn("if: always()", workflow)
        upload = workflow.split("- name: Upload runtime evidence and lockfile", 1)[1]
        self.assertIn("path: ${{ runner.temp }}/component-evidence/", upload)

    def test_failure_packet_records_stage_outcomes_and_diagnostics(self) -> None:
        workflow = (collector.ROOT / ".github/workflows/component-runtime-qualification.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn('id: runtime', workflow)
        self.assertIn('id: receipt', workflow)
        self.assertIn('if: always()', workflow)
        self.assertIn('packet-status.json', workflow)
        self.assertIn('runtime.stderr.log', workflow)
        self.assertIn('collector-stderr.log', workflow)
        self.assertIn('"qualification_result": "$QUALIFICATION_RESULT"', workflow)
        self.assertIn('"receipt_present": $RECEIPT_PRESENT', workflow)
        self.assertIn('if [ "$RECEIPT_OUTCOME" = "success" ] && [ "$RECEIPT_PRESENT" = true ]; then', workflow)
        self.assertIn('"runtime_log": "$(digest_if_file "$RUNNER_TEMP/component-runtime.log")"', workflow)
        self.assertIn('"collector_stderr": "$(digest_if_file "$RUNNER_TEMP/component-evidence/collector-stderr.log")"', workflow)

    def test_packet_pass_requires_all_steps_and_a_valid_receipt(self) -> None:
        passed = {name: "success" for name in packet.REQUIRED_STEPS}
        self.assertEqual(
            "pass",
            packet.qualification_result(passed, receipt_present=True, receipt_valid=True),
        )
        self.assertEqual(
            "failure",
            packet.qualification_result(passed, receipt_present=False, receipt_valid=False),
        )
        self.assertEqual(
            "failure",
            packet.qualification_result(passed, receipt_present=True, receipt_valid=False),
        )
        self.assertEqual(
            "failure",
            packet.qualification_result(dict(passed, build="failure"), receipt_present=True, receipt_valid=True),
        )
        self.assertEqual(
            "cancelled",
            packet.qualification_result(dict(passed, runtime="cancelled"), receipt_present=False, receipt_valid=False),
        )
        self.assertEqual(
            "not_issued",
            packet.qualification_result(dict(passed, receipt="skipped"), receipt_present=False, receipt_valid=False),
        )

    def test_corrupt_receipt_writes_failure_packet_instead_of_crashing(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root = base / "repo"
            runner_temp = base / "runner-temp"
            evidence = runner_temp / "component-evidence"
            (root / "tools/component-host-smoke").mkdir(parents=True)
            (root / "target/wasm32-wasip2/release").mkdir(parents=True)
            (root / "tests/fixtures").mkdir(parents=True)
            evidence.mkdir(parents=True)
            (root / "tools/component-host-smoke/Cargo.lock").write_bytes(b"host lockfile")
            (root / "target/wasm32-wasip2/release/sovereign_visual_component.wasm").write_bytes(b"component")
            (root / "tests/fixtures/first-germination.scene.json").write_bytes(b"fixture")
            (runner_temp / "component-runtime.log").write_text("runtime log", encoding="utf-8")
            (runner_temp / "component-runtime-stderr.log").write_text("", encoding="utf-8")
            (evidence / "qualification.json").write_text("{not-json", encoding="utf-8")
            (evidence / "collector-stderr.log").write_text("bad receipt", encoding="utf-8")
            variables = {
                "RUNNER_TEMP": str(runner_temp),
                "EXPECTED_COMMIT": "a" * 40,
                "GITHUB_RUN_ID": "12345",
                "GITHUB_RUN_ATTEMPT": "1",
                "GITHUB_WORKFLOW_REF": "Luminous-Dynamics/sovereign-boot/.github/workflows/component-runtime-qualification.yml@refs/pull/1/merge",
                "RUNNER_OS": "Linux",
                "TOOLCHAIN_OUTCOME": "success",
                "LOCKFILE_OUTCOME": "success",
                "RECEIPT_TESTS_OUTCOME": "success",
                "FORMAT_OUTCOME": "success",
                "BUILD_OUTCOME": "success",
                "RUNTIME_OUTCOME": "success",
                "RECEIPT_OUTCOME": "success",
            }
            with (
                patch.object(packet, "ROOT", root),
                patch.dict(os.environ, variables, clear=True),
                contextlib.redirect_stdout(io.StringIO()),
            ):
                self.assertEqual(1, packet.main())
            status = json.loads((evidence / "packet-status.json").read_text(encoding="utf-8"))
            self.assertEqual("failure", status["qualification_result"])
            self.assertEqual("invalid", status["receipt_validation"])
            self.assertTrue(status["receipt_present"])
            self.assertIn("unreadable JSON", status["receipt_validation_error"])

    def test_complete_pass_packet_copies_and_validates_all_runtime_artifacts(self) -> None:
        expected_commit = "e" * 40
        expected_tree = "f" * 40
        workflow_ref = "Luminous-Dynamics/sovereign-boot/.github/workflows/component-runtime-qualification.yml@refs/pull/1/merge"
        runtime_log = " ".join(packet.REQUIRED_RUNTIME_MARKERS) + "\n"
        component_bytes = b"synthetic wasm component"
        lockfile_bytes = b"synthetic host lockfile"
        fixture_bytes = b"synthetic pinned fixture"
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root = base / "repo"
            runner_temp = base / "runner-temp"
            evidence = runner_temp / "component-evidence"
            for relative in packet.REQUIRED_SOURCE_PATHS:
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(("source:" + relative).encode("utf-8"))
            (root / "tools/component-host-smoke").mkdir(parents=True, exist_ok=True)
            (root / "target/wasm32-wasip2/release").mkdir(parents=True)
            (root / "tests/fixtures").mkdir(parents=True, exist_ok=True)
            (root / "tools/component-host-smoke/Cargo.lock").write_bytes(lockfile_bytes)
            (root / "target/wasm32-wasip2/release/sovereign_visual_component.wasm").write_bytes(component_bytes)
            (root / "tests/fixtures/first-germination.scene.json").write_bytes(fixture_bytes)
            runner_temp.mkdir(parents=True, exist_ok=True)
            (runner_temp / "component-runtime.log").write_text(runtime_log, encoding="utf-8")
            (runner_temp / "component-runtime-stderr.log").write_text("", encoding="utf-8")
            evidence.mkdir(parents=True, exist_ok=True)
            (evidence / "collector-stderr.log").write_text("", encoding="utf-8")

            source_records = []
            for relative in sorted(packet.REQUIRED_SOURCE_PATHS):
                path = root / relative
                content = path.read_bytes()
                source_records.append({
                    "path": relative,
                    "bytes": len(content),
                    "sha256": hashlib.sha256(content).hexdigest(),
                })
            receipt = {
                "schema": "luminous-sovereign-visual-component-runtime-evidence-v1",
                "result": "pass",
                "source": {
                    "commit": expected_commit,
                    "expected_commit": expected_commit,
                    "tree": expected_tree,
                    "files": source_records,
                },
                "execution": {
                    "workflow": "Component Model runtime qualification",
                    "run_id": "12345",
                    "run_attempt": "1",
                    "workflow_ref": workflow_ref,
                    "runner_os": "Linux",
                    "runtime_log_sha256": hashlib.sha256(runtime_log.encode("utf-8")).hexdigest(),
                    "required_runtime_markers": list(packet.REQUIRED_RUNTIME_MARKERS),
                },
                "artifacts": {
                    "component": {
                        "path": "target/wasm32-wasip2/release/sovereign_visual_component.wasm",
                        "bytes": len(component_bytes),
                        "sha256": hashlib.sha256(component_bytes).hexdigest(),
                    },
                    "host_lockfile": {
                        "path": "tools/component-host-smoke/Cargo.lock",
                        "bytes": len(lockfile_bytes),
                        "sha256": hashlib.sha256(lockfile_bytes).hexdigest(),
                    },
                    "scene_pack_fixture": {
                        "path": "tests/fixtures/first-germination.scene.json",
                        "bytes": len(fixture_bytes),
                        "sha256": hashlib.sha256(fixture_bytes).hexdigest(),
                    },
                },
                "scope": dict(packet.REQUIRED_SCOPE),
            }
            (evidence / "qualification.json").write_text(
                json.dumps(receipt), encoding="utf-8"
            )
            variables = {
                "RUNNER_TEMP": str(runner_temp),
                "EXPECTED_COMMIT": expected_commit,
                "GITHUB_RUN_ID": "12345",
                "GITHUB_RUN_ATTEMPT": "1",
                "GITHUB_WORKFLOW_REF": workflow_ref,
                "RUNNER_OS": "Linux",
                "TOOLCHAIN_OUTCOME": "success",
                "LOCKFILE_OUTCOME": "success",
                "RECEIPT_TESTS_OUTCOME": "success",
                "FORMAT_OUTCOME": "success",
                "BUILD_OUTCOME": "success",
                "RUNTIME_OUTCOME": "success",
                "RECEIPT_OUTCOME": "success",
            }

            def fake_git(args, **kwargs):
                revision = args[-1]
                if revision == "HEAD":
                    return type("ProcessResult", (), {"stdout": expected_commit + "\n"})()
                if revision == "HEAD^{tree}":
                    return type("ProcessResult", (), {"stdout": expected_tree + "\n"})()
                raise AssertionError("unexpected git command: " + repr(args))

            with (
                patch.object(packet, "ROOT", root),
                patch.dict(os.environ, variables, clear=True),
                patch.object(packet.subprocess, "run", side_effect=fake_git),
                contextlib.redirect_stdout(io.StringIO()),
            ):
                self.assertEqual(0, packet.main())

            status = json.loads((evidence / "packet-status.json").read_text(encoding="utf-8"))
            self.assertEqual("pass", status["qualification_result"])
            self.assertEqual("valid", status["receipt_validation"])
            self.assertEqual(expected_commit, status["checkout_commit"])
            self.assertEqual(expected_tree, status["checkout_tree"])
            self.assertEqual(
                hashlib.sha256(component_bytes).hexdigest(),
                status["sha256"]["component"],
            )
            self.assertEqual(
                component_bytes,
                (evidence / "sovereign_visual_component.wasm").read_bytes(),
            )
            self.assertEqual(
                lockfile_bytes,
                (evidence / "component-host-Cargo.lock").read_bytes(),
            )

    def test_packet_receipt_validation_binds_source_context_runtime_and_artifacts(self) -> None:
        expected_commit = "e" * 40
        workflow_ref = "Luminous-Dynamics/sovereign-boot/.github/workflows/component-runtime-qualification.yml@refs/pull/1/merge"
        runtime_log = " ".join(packet.REQUIRED_RUNTIME_MARKERS)
        hashes = {
            "component": "a" * 64,
            "host_lockfile": "b" * 64,
            "scene_pack_fixture": "c" * 64,
            "runtime_log": hashlib.sha256(runtime_log.encode("utf-8")).hexdigest(),
        }
        sizes = {"component": 10, "host_lockfile": 20, "scene_pack_fixture": 30}
        with tempfile.TemporaryDirectory() as directory:
            source_root = Path(directory)
            source_records = []
            for relative in sorted(packet.REQUIRED_SOURCE_PATHS):
                path = source_root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                content = ("source:" + relative).encode("utf-8")
                path.write_bytes(content)
                source_records.append({
                    "path": relative,
                    "bytes": len(content),
                    "sha256": hashlib.sha256(content).hexdigest(),
                })
            receipt = {
                "schema": "luminous-sovereign-visual-component-runtime-evidence-v1",
                "result": "pass",
                "source": {
                    "commit": expected_commit,
                    "expected_commit": expected_commit,
                    "tree": "f" * 40,
                    "files": source_records,
                },
                "execution": {
                    "workflow": "Component Model runtime qualification",
                    "run_id": "12345",
                    "run_attempt": "1",
                    "workflow_ref": workflow_ref,
                    "runner_os": "Linux",
                    "runtime_log_sha256": hashes["runtime_log"],
                    "required_runtime_markers": list(packet.REQUIRED_RUNTIME_MARKERS),
                },
                "artifacts": {
                    "component": {"sha256": hashes["component"], "bytes": sizes["component"]},
                    "host_lockfile": {"sha256": hashes["host_lockfile"], "bytes": sizes["host_lockfile"]},
                    "scene_pack_fixture": {"sha256": hashes["scene_pack_fixture"], "bytes": sizes["scene_pack_fixture"]},
                },
                "scope": dict(packet.REQUIRED_SCOPE),
            }
            kwargs = {
                "expected_commit": expected_commit,
                "run_id": "12345",
                "run_attempt": "1",
                "workflow_ref": workflow_ref,
                "runner_os": "Linux",
                "hashes": hashes,
                "sizes": sizes,
                "source_root": source_root,
                "runtime_log_text": runtime_log,
                "checkout_commit": expected_commit,
                "checkout_tree": "f" * 40,
            }
            valid, reason = packet.validate_receipt_document(receipt, **kwargs)
            self.assertTrue(valid, reason)

            module_root = source_root / "crates/visual-core/src/lib.rs"
            original_module_root = module_root.read_bytes()
            added_module = "pub mod omitted_from_evidence;\n".encode("utf-8")
            module_root.write_bytes(original_module_root + b"\n" + added_module)
            (module_root.parent / "omitted_from_evidence.rs").write_bytes(b"pub fn omitted() {}\n")
            inventory_with_omission = []
            for record in source_records:
                if record["path"] == "crates/visual-core/src/lib.rs":
                    content = module_root.read_bytes()
                    inventory_with_omission.append({
                        "path": record["path"],
                        "bytes": len(content),
                        "sha256": hashlib.sha256(content).hexdigest(),
                    })
                else:
                    inventory_with_omission.append(record)
            receipt_with_omitted_module = dict(
                receipt,
                source=dict(receipt["source"], files=inventory_with_omission),
            )
            self.assertFalse(
                packet.validate_receipt_document(receipt_with_omitted_module, **kwargs)[0]
            )
            module_root.write_bytes(original_module_root)
            (module_root.parent / "omitted_from_evidence.rs").unlink()

            changed_path = source_root / ".gitignore"
            changed_path.write_bytes(b"post-receipt modification")
            self.assertFalse(packet.validate_receipt_document(receipt, **kwargs)[0])
            changed_path.write_bytes(b"source:.gitignore")
            wrong_hashes = dict(hashes, component="0" * 64)
            self.assertFalse(
                packet.validate_receipt_document(receipt, **dict(kwargs, hashes=wrong_hashes))[0]
            )
            wrong_context = dict(receipt, execution=dict(receipt["execution"], run_id="99999"))
            self.assertFalse(packet.validate_receipt_document(wrong_context, **kwargs)[0])
            malformed_run = dict(receipt, execution=dict(receipt["execution"], run_id="not-numeric"))
            self.assertFalse(
                packet.validate_receipt_document(malformed_run, **dict(kwargs, run_id="not-numeric"))[0]
            )
            malformed_attempt = dict(receipt, execution=dict(receipt["execution"], run_attempt="0"))
            self.assertFalse(
                packet.validate_receipt_document(malformed_attempt, **dict(kwargs, run_attempt="0"))[0]
            )
            self.assertRegex(workflow_ref, r"/\.github/workflows/component-runtime-qualification\.yml@[^@\s]+$")
            malformed_workflow = dict(receipt, execution=dict(receipt["execution"], workflow_ref="arbitrary"))
            self.assertFalse(
                packet.validate_receipt_document(malformed_workflow, **dict(kwargs, workflow_ref="arbitrary"))[0]
            )
            windows_runner = dict(receipt, execution=dict(receipt["execution"], runner_os="Windows"))
            self.assertFalse(
                packet.validate_receipt_document(windows_runner, **dict(kwargs, runner_os="Windows"))[0]
            )
            self.assertFalse(packet.validate_receipt_document(receipt, **dict(kwargs, checkout_commit="f" * 40))[0])
            self.assertFalse(packet.validate_receipt_document(receipt, **dict(kwargs, checkout_tree="0" * 40))[0])
            self.assertFalse(
                packet.validate_receipt_document(receipt, **dict(kwargs, runtime_log_text="component_runtime=wasmtime-49.0.2"))[0]
            )
            bad_scope = dict(receipt, scope=dict(receipt["scope"], physical_boot=True))
            self.assertFalse(packet.validate_receipt_document(bad_scope, **kwargs)[0])
            duplicate_inventory = dict(receipt, source=dict(receipt["source"], files=source_records + [source_records[0]]))
            self.assertFalse(packet.validate_receipt_document(duplicate_inventory, **kwargs)[0])


    def test_duplicate_receipt_lines_are_rejected(self) -> None:
        line = " ".join(collector.REQUIRED_MARKERS)
        self.assertIn(
            "exactly one component_runtime receipt line",
            collector.missing_required_markers(line + "\n" + line),
        )

    def test_source_inventory_binds_guest_collector_and_workflow(self) -> None:
        for relative in (
            ".gitignore",
            ".github/workflows/component-runtime-qualification.yml",
            "crates/visual-component/src/lib.rs",
            "crates/visual-component/wit/visual.wit",
            "crates/visual-core/src/mycelium.rs",
            "crates/visual-pack/src/lib.rs",
            "tests/fixtures/first-germination.scene.json",
            "tools/component-host-smoke/assemble-packet.py",
            "tools/component-host-smoke/collect-evidence.py",
            "tools/component-host-smoke/test_collect_evidence.py",
        ):
            self.assertIn(relative, collector.SOURCE_PATHS)

    def test_generated_host_lock_is_artifact_not_committed_source(self) -> None:
        self.assertNotIn("tools/component-host-smoke/Cargo.lock", collector.SOURCE_PATHS)

    def test_github_actions_context_requires_workflow_receipt_provenance(self) -> None:
        valid = {
            "GITHUB_ACTIONS": "true",
            "GITHUB_WORKFLOW": "Component Model runtime qualification",
            "GITHUB_RUN_ID": "38126517666",
            "GITHUB_RUN_ATTEMPT": "1",
            "GITHUB_WORKFLOW_REF": "Luminous-Dynamics/sovereign-boot/.github/workflows/component-runtime-qualification.yml@refs/pull/1/merge",
            "RUNNER_OS": "Linux",
        }
        with patch.dict(os.environ, valid, clear=True):
            context = collector.require_github_actions_context()
        self.assertEqual("38126517666", context["run_id"])
        self.assertEqual("Linux", context["runner_os"])

        with patch.dict(os.environ, {}, clear=True):
            with self.assertRaisesRegex(ValueError, "only be issued inside GitHub Actions"):
                collector.require_github_actions_context()
        invalid = dict(valid, GITHUB_WORKFLOW_REF="refs/pull/1/merge")
        with patch.dict(os.environ, invalid, clear=True):
            with self.assertRaisesRegex(ValueError, "workflow reference"):
                collector.require_github_actions_context()
        invalid = dict(valid, RUNNER_OS="Windows")
        with patch.dict(os.environ, invalid, clear=True):
            with self.assertRaisesRegex(ValueError, "Linux hosted runner"):
                collector.require_github_actions_context()

    def test_checkout_cleanliness_allows_only_generated_host_lockfile(self) -> None:
        with patch.object(
            collector,
            "git_output",
            return_value="?? tools/component-host-smoke/Cargo.lock",
        ):
            collector.require_clean_checkout()

        for status in (
            " M crates/visual-core/src/lib.rs",
            "?? tools/component-host-smoke/unexpected.rs",
            " M .github/workflows/component-runtime-qualification.yml",
        ):
            with self.subTest(status=status):
                with patch.object(collector, "git_output", return_value=status):
                    with self.assertRaisesRegex(ValueError, "checkout contains changes"):
                        collector.require_clean_checkout()

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
                "--expected-commit", "a" * 40,
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
            root = Path(directory) / "repo"
            root.mkdir()
            component = root / "target/wasm32-wasip2/release/sovereign_visual_component.wasm"
            lockfile = root / "tools/component-host-smoke/Cargo.lock"
            log = root / "runtime.log"
            output = root / "qualification.json"
            fixture = root / "tests/fixtures/first-germination.scene.json"
            component.parent.mkdir(parents=True)
            lockfile.parent.mkdir(parents=True)
            fixture.parent.mkdir(parents=True)
            component_bytes = b"synthetic component bytes for collector unit test"
            lock_bytes = b"synthetic host lock for collector unit test"
            component.write_bytes(component_bytes)
            lockfile.write_bytes(lock_bytes)
            fixture.write_bytes(b"pinned fixture bytes for collector unit test")
            log.write_text(" ".join(collector.REQUIRED_MARKERS), encoding="utf-8")
            expected_commit = "a" * 40
            argv = [
                str(SCRIPT),
                "--expected-commit", expected_commit,
                "--component", str(component),
                "--fixture", str(fixture),
                "--lockfile", str(lockfile),
                "--runtime-log", str(log),
                "--rustc-version", "rustc test-version",
                "--output", str(output),
            ]
            mock_records = lambda relative: {"path": relative, "bytes": 1, "sha256": "a" * 64}
            with (
                patch.object(collector, "ROOT", root.resolve()),
                patch.dict(os.environ, {
                    "GITHUB_ACTIONS": "true",
                    "GITHUB_WORKFLOW": "Component Model runtime qualification",
                    "GITHUB_RUN_ID": "38126517666",
                    "GITHUB_RUN_ATTEMPT": "1",
                    "GITHUB_WORKFLOW_REF": "Luminous-Dynamics/sovereign-boot/.github/workflows/component-runtime-qualification.yml@refs/pull/1/merge",
                    "RUNNER_OS": "Linux",
                }, clear=True),
                patch.object(sys, "argv", argv),
                patch.object(
                    collector,
                    "git_output",
                    side_effect=[expected_commit, "?? tools/component-host-smoke/Cargo.lock", "tree-test"],
                ),
                patch.object(collector, "file_record", side_effect=mock_records),
                contextlib.redirect_stdout(io.StringIO()),
            ):
                self.assertEqual(0, collector.main())
            evidence = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual("pass", evidence["result"])
            self.assertEqual(expected_commit, evidence["source"]["commit"])
            self.assertEqual(expected_commit, evidence["source"]["expected_commit"])
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

    def test_wrong_checkout_commit_cannot_produce_pass_receipt(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            log = root / "runtime.log"
            output = root / "qualification.json"
            log.write_text(" ".join(collector.REQUIRED_MARKERS), encoding="utf-8")
            argv = [
                str(SCRIPT),
                "--expected-commit", "a" * 40,
                "--component", str(root / "component.wasm"),
                "--fixture", str(root / "first-germination.scene.json"),
                "--lockfile", str(root / "Cargo.lock"),
                "--runtime-log", str(log),
                "--rustc-version", "rustc test-version",
                "--output", str(output),
            ]
            with (
                patch.object(sys, "argv", argv),
                patch.object(collector, "git_output", return_value="b" * 40),
                contextlib.redirect_stderr(io.StringIO()),
            ):
                self.assertEqual(2, collector.main())
            self.assertFalse(output.exists())

    def test_artifact_path_cannot_escape_repository(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root = base / "repo"
            outside = base / "outside.wasm"
            root.mkdir()
            outside.write_bytes(b"outside")
            (root / "runtime.log").write_text(" ".join(collector.REQUIRED_MARKERS), encoding="utf-8")
            argv = [
                str(SCRIPT),
                "--expected-commit", "a" * 40,
                "--component", str(outside),
                "--fixture", str(root / "tests/fixtures/first-germination.scene.json"),
                "--lockfile", str(root / "tools/component-host-smoke/Cargo.lock"),
                "--runtime-log", str(root / "runtime.log"),
                "--rustc-version", "rustc test-version",
                "--output", str(root / "qualification.json"),
            ]
            with (
                patch.object(collector, "ROOT", root.resolve()),
                patch.dict(os.environ, {
                    "GITHUB_ACTIONS": "true",
                    "GITHUB_WORKFLOW": "Component Model runtime qualification",
                    "GITHUB_RUN_ID": "38126517666",
                    "GITHUB_RUN_ATTEMPT": "1",
                    "GITHUB_WORKFLOW_REF": "Luminous-Dynamics/sovereign-boot/.github/workflows/component-runtime-qualification.yml@refs/pull/1/merge",
                    "RUNNER_OS": "Linux",
                }, clear=True),
                patch.object(sys, "argv", argv),
                patch.object(
                    collector,
                    "git_output",
                    side_effect=["a" * 40, "?? tools/component-host-smoke/Cargo.lock"],
                ),
                contextlib.redirect_stderr(io.StringIO()),
            ):
                self.assertEqual(2, collector.main())
            self.assertFalse((root / "qualification.json").exists())



if __name__ == "__main__":
    unittest.main(verbosity=2)
