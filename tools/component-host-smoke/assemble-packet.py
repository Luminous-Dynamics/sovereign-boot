#!/usr/bin/env python3
"""Assemble a fail-closed, reviewable packet for Component Model qualification."""
from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import sys
import tempfile
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
SCHEMA = "luminous-sovereign-visual-component-runtime-packet-status-v1"
REQUIRED_STEPS = ("toolchain", "lockfile", "receipt_tests", "format", "build", "runtime", "receipt")
STEP_ENV = {
    "toolchain": "TOOLCHAIN_OUTCOME",
    "lockfile": "LOCKFILE_OUTCOME",
    "receipt_tests": "RECEIPT_TESTS_OUTCOME",
    "format": "FORMAT_OUTCOME",
    "build": "BUILD_OUTCOME",
    "runtime": "RUNTIME_OUTCOME",
    "receipt": "RECEIPT_OUTCOME",
}
ALLOWED_OUTCOMES = {"success", "failure", "cancelled", "skipped"}


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def file_hash(path: Path) -> str:
    return sha256_file(path) if path.is_file() else ""


def file_size(path: Path) -> int:
    return path.stat().st_size if path.is_file() else -1


def copy_if_present(source: Path, destination: Path) -> None:
    if source.is_file():
        shutil.copyfile(source, destination)
    else:
        destination.unlink(missing_ok=True)


def validate_receipt_document(
    receipt: Any,
    *,
    expected_commit: str,
    run_id: str,
    run_attempt: str,
    workflow_ref: str,
    runner_os: str,
    hashes: dict[str, str],
    sizes: dict[str, int],
) -> tuple[bool, str]:
    """Validate the pass receipt against the actual packet inputs and run context."""
    if not isinstance(receipt, dict):
        return False, "receipt root is not a JSON object"
    if receipt.get("schema") != "luminous-sovereign-visual-component-runtime-evidence-v1":
        return False, "unexpected receipt schema"
    if receipt.get("result") != "pass":
        return False, "receipt result is not pass"
    if re.fullmatch(r"[0-9a-f]{40}", expected_commit) is None:
        return False, "expected source commit is not a full lowercase Git SHA"

    source = receipt.get("source")
    if not isinstance(source, dict):
        return False, "receipt source section is missing"
    if source.get("commit") != expected_commit or source.get("expected_commit") != expected_commit:
        return False, "receipt source commit does not match the packet commit"
    source_files = source.get("files")
    if not isinstance(source_files, list):
        return False, "receipt source file inventory is missing"
    source_paths = {
        record.get("path")
        for record in source_files
        if isinstance(record, dict) and isinstance(record.get("path"), str)
    }
    required_source_paths = {
        ".gitignore",
        ".github/workflows/component-runtime-qualification.yml",
        "tools/component-host-smoke/assemble-packet.py",
        "tools/component-host-smoke/collect-evidence.py",
        "tools/component-host-smoke/test_collect_evidence.py",
        "tools/component-host-smoke/src/main.rs",
    }
    missing_paths = sorted(required_source_paths - source_paths)
    if missing_paths:
        return False, "receipt source inventory omits: " + ", ".join(missing_paths)

    execution = receipt.get("execution")
    if not isinstance(execution, dict):
        return False, "receipt execution section is missing"
    for field, expected in (
        ("run_id", run_id),
        ("run_attempt", run_attempt),
        ("workflow_ref", workflow_ref),
        ("runner_os", runner_os),
    ):
        if execution.get(field) != expected:
            return False, f"receipt execution {field} does not match the packet context"
    if execution.get("runtime_log_sha256") != hashes.get("runtime_log"):
        return False, "runtime log digest does not match the receipt"

    artifacts = receipt.get("artifacts")
    if not isinstance(artifacts, dict):
        return False, "receipt artifact section is missing"
    for receipt_name, hash_name in (
        ("component", "component"),
        ("host_lockfile", "host_lockfile"),
        ("scene_pack_fixture", "scene_pack_fixture"),
    ):
        record = artifacts.get(receipt_name)
        if not isinstance(record, dict):
            return False, f"receipt {receipt_name} record is missing"
        if not hashes.get(hash_name) or record.get("sha256") != hashes[hash_name]:
            return False, f"receipt {receipt_name} digest does not match the packet"
        if record.get("bytes") != sizes.get(hash_name):
            return False, f"receipt {receipt_name} size does not match the packet"
    return True, ""


def qualification_result(
    outcomes: dict[str, str], *, receipt_present: bool, receipt_valid: bool
) -> str:
    """Pass only if every required stage succeeded and the receipt validates."""
    if outcomes.get("receipt") == "success" and (not receipt_present or not receipt_valid):
        return "failure"
    if any(value == "failure" for value in outcomes.values()):
        return "failure"
    if any(value == "cancelled" for value in outcomes.values()):
        return "cancelled"
    if (
        all(outcomes.get(step) == "success" for step in REQUIRED_STEPS)
        and receipt_present
        and receipt_valid
    ):
        return "pass"
    return "not_issued"


def main() -> int:
    env = os.environ
    runner_temp_value = env.get("RUNNER_TEMP", "").strip()
    if not runner_temp_value:
        print("ERROR: RUNNER_TEMP is required to assemble qualification evidence", file=sys.stderr)
        return 2
    runner_temp = Path(runner_temp_value)
    evidence_dir = runner_temp / "component-evidence"
    evidence_dir.mkdir(parents=True, exist_ok=True)

    expected_commit = env.get("EXPECTED_COMMIT", "").strip()
    run_id = env.get("GITHUB_RUN_ID", "").strip()
    run_attempt = env.get("GITHUB_RUN_ATTEMPT", "").strip()
    runner_os = env.get("RUNNER_OS", "").strip()
    workflow_ref = env.get("GITHUB_WORKFLOW_REF", "").strip()

    source_paths = {
        "host_lockfile": ROOT / "tools/component-host-smoke/Cargo.lock",
        "component": ROOT / "target/wasm32-wasip2/release/sovereign_visual_component.wasm",
        "runtime_log": runner_temp / "component-runtime.log",
        "runtime_stderr": runner_temp / "component-runtime-stderr.log",
    }
    packet_paths = {
        "host_lockfile": evidence_dir / "component-host-Cargo.lock",
        "component": evidence_dir / "sovereign_visual_component.wasm",
        "runtime_log": evidence_dir / "runtime.log",
        "runtime_stderr": evidence_dir / "runtime.stderr.log",
    }
    for key, source in source_paths.items():
        copy_if_present(source, packet_paths[key])

    fixture_path = ROOT / "tests/fixtures/first-germination.scene.json"
    receipt_path = evidence_dir / "qualification.json"
    collector_stderr_path = evidence_dir / "collector-stderr.log"
    files_for_hash = {
        **packet_paths,
        "scene_pack_fixture": fixture_path,
        "qualification_receipt": receipt_path,
        "collector_stderr": collector_stderr_path,
    }
    hashes = {key: file_hash(path) for key, path in files_for_hash.items()}
    sizes = {key: file_size(path) for key, path in files_for_hash.items()}
    receipt_present = receipt_path.is_file() and receipt_path.stat().st_size > 0

    receipt: Any = None
    receipt_valid = False
    validation_error = "qualification receipt is missing"
    if receipt_present:
        try:
            with receipt_path.open("r", encoding="utf-8") as stream:
                receipt = json.load(stream)
        except (OSError, UnicodeError, json.JSONDecodeError) as exc:
            validation_error = f"qualification receipt is unreadable JSON: {exc}"
        else:
            valid, validation_error = validate_receipt_document(
                receipt,
                expected_commit=expected_commit,
                run_id=run_id,
                run_attempt=run_attempt,
                workflow_ref=workflow_ref,
                runner_os=runner_os,
                hashes=hashes,
                sizes=sizes,
            )
            receipt_valid = valid
    else:
        receipt_valid = False

    outcomes = {
        step: env.get(env_name, "skipped").strip() or "skipped"
        for step, env_name in STEP_ENV.items()
    }
    for step, outcome in tuple(outcomes.items()):
        if outcome not in ALLOWED_OUTCOMES:
            outcomes[step] = "skipped"

    result = qualification_result(
        outcomes, receipt_present=receipt_present, receipt_valid=receipt_valid
    )
    status = {
        "schema": SCHEMA,
        "source_commit": expected_commit,
        "run_id": run_id,
        "run_attempt": run_attempt,
        "workflow_ref": workflow_ref,
        "runner_os": runner_os,
        "qualification_result": result,
        "receipt_present": receipt_present,
        "receipt_validation": "valid" if receipt_valid else ("invalid" if receipt_present else "missing"),
        "receipt_validation_error": "" if receipt_valid else validation_error,
        "steps": outcomes,
        "sha256": hashes,
        "bytes": sizes,
    }

    output = evidence_dir / "packet-status.json"
    temporary_name = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", dir=evidence_dir, prefix=".packet-status-", delete=False
        ) as stream:
            temporary_name = stream.name
            json.dump(status, stream, indent=2, sort_keys=True)
            stream.write("\n")
        Path(temporary_name).replace(output)
    except OSError as exc:
        if temporary_name:
            Path(temporary_name).unlink(missing_ok=True)
        print(f"ERROR: cannot write packet status: {exc}", file=sys.stderr)
        return 2

    print(f"qualification_result={result} receipt_validation={status['receipt_validation']}")
    if result in {"failure", "not_issued"}:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
