#!/usr/bin/env python3
"""Assemble a fail-closed, reviewable packet for Component Model qualification."""
from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path, PurePosixPath
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
REQUIRED_SOURCE_PATHS = frozenset({
    ".gitignore",
    ".github/workflows/component-runtime-qualification.yml",
    "contracts/tests/scene-pack-v1-negative-fixtures.json",
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "crates/visual-core/Cargo.toml",
    "crates/visual-core/src/color.rs",
    "crates/visual-core/src/contract.rs",
    "crates/visual-core/src/lib.rs",
    "crates/visual-core/src/mycelium.rs",
    "crates/visual-core/src/settings.rs",
    "crates/visual-pack/Cargo.toml",
    "crates/visual-pack/src/lib.rs",
    "crates/visual-component/Cargo.toml",
    "crates/visual-component/src/lib.rs",
    "crates/visual-component/wit/visual.wit",
    "tests/fixtures/first-germination.scene.json",
    "tools/component-host-smoke/Cargo.toml",
    "tools/component-host-smoke/src/main.rs",
    "tools/component-host-smoke/assemble-packet.py",
    "tools/component-host-smoke/collect-evidence.py",
    "tools/component-host-smoke/test_collect_evidence.py",
})
REQUIRED_RUNTIME_MARKERS = (
    "component_runtime=wasmtime-49.0.2",
    "configured_replay=byte-identical",
    "branch_budget=pass",
    "invalid_config_atomic=pass",
    "tick_batch_bound=pass",
    "wasi_imports=none",
    "resources=dropped",
)
REQUIRED_SCOPE = {
    "component_instantiated": True,
    "host_imports_registered": False,
    "configured_replay_byte_identical": True,
    "hosted_ci_only": True,
    "browser_presentation": False,
    "physical_boot": False,
    "whole_process_memory_limit": False,
}


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


def validate_source_inventory(source_files: Any, source_root: Path) -> tuple[bool, str]:
    """Recompute every committed source record and require the complete expected set."""
    if not isinstance(source_files, list):
        return False, "receipt source file inventory is missing"
    root = source_root.resolve()
    records: dict[str, dict[str, Any]] = {}
    for record in source_files:
        if not isinstance(record, dict):
            return False, "receipt source inventory contains a non-object record"
        relative = record.get("path")
        if not isinstance(relative, str) or not relative or "\\" in relative:
            return False, "receipt source inventory contains an invalid path"
        pure = PurePosixPath(relative)
        if pure.is_absolute() or any(part in ("", ".", "..") for part in pure.parts):
            return False, f"receipt source inventory contains an unsafe path: {relative}"
        if relative in records:
            return False, f"receipt source inventory duplicates path: {relative}"
        recorded_bytes = record.get("bytes")
        recorded_hash = record.get("sha256")
        if type(recorded_bytes) is not int or recorded_bytes < 0:
            return False, f"receipt source inventory has invalid byte count: {relative}"
        if not isinstance(recorded_hash, str) or re.fullmatch(r"[0-9a-f]{64}", recorded_hash) is None:
            return False, f"receipt source inventory has invalid SHA-256: {relative}"
        records[relative] = record

    seen_paths = set(records)
    if seen_paths != REQUIRED_SOURCE_PATHS:
        missing = sorted(REQUIRED_SOURCE_PATHS - seen_paths)
        extra = sorted(seen_paths - REQUIRED_SOURCE_PATHS)
        detail = []
        if missing:
            detail.append("missing: " + ", ".join(missing))
        if extra:
            detail.append("unexpected: " + ", ".join(extra))
        return False, "receipt source inventory set mismatch (" + "; ".join(detail) + ")"

    for relative, record in records.items():
        candidate = (root / Path(*PurePosixPath(relative).parts)).resolve()
        if not candidate.is_relative_to(root):
            return False, f"receipt source path escapes checkout: {relative}"
        if not candidate.is_file():
            return False, f"receipt source file is missing: {relative}"
        if candidate.stat().st_size != record["bytes"]:
            return False, f"receipt source byte count mismatch: {relative}"
        if sha256_file(candidate) != record["sha256"]:
            return False, f"receipt source SHA-256 mismatch: {relative}"
        if candidate.suffix == ".rs":
            try:
                source_text = candidate.read_text(encoding="utf-8")
            except (OSError, UnicodeError) as exc:
                return False, f"cannot inspect Rust module declarations in {relative}: {exc}"
            module_names = re.findall(
                r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;",
                source_text,
            )
            parent = PurePosixPath(relative).parent
            for module_name in module_names:
                candidates = {
                    (parent / f"{module_name}.rs").as_posix(),
                    (parent / module_name / "mod.rs").as_posix(),
                }
                if not candidates.intersection(records):
                    return False, (
                        f"Rust module declared by {relative} is not source-bound: "
                        f"{module_name}"
                    )

            include_call_count = len(
                re.findall(r"\binclude(?:_str|_bytes)?!\s*\(", source_text)
            )
            include_matches = list(
                re.finditer(
                    r'\binclude(?:_str|_bytes)?!\s*\(\s*"([^"]+)"\s*\)',
                    source_text,
                    flags=re.DOTALL,
                )
            )
            if include_call_count != len(include_matches):
                return False, (
                    f"Rust file {relative} contains compile-time include syntax "
                    "that is not a directly auditable string path"
                )
            for include_match in include_matches:
                include_path = include_match.group(1)
                if "\\" in include_path:
                    return False, f"Rust include path uses a non-portable separator: {relative}"
                include_pure = PurePosixPath(include_path)
                if include_pure.is_absolute() or any(part in ("", ".", "..") for part in include_pure.parts):
                    # Parent traversals are valid relative Rust paths; resolve them
                    # against the source file below and enforce repository containment.
                    if include_pure.is_absolute():
                        return False, f"Rust include path is absolute: {relative}"
                included = (candidate.parent / Path(*include_pure.parts)).resolve()
                if not included.is_relative_to(root):
                    return False, f"Rust include path escapes checkout: {relative} -> {include_path}"
                included_relative = included.relative_to(root).as_posix()
                if included_relative not in records:
                    return False, (
                        f"Rust compile-time include from {relative} is not source-bound: "
                        f"{included_relative}"
                    )
    return True, ""


def validate_runtime_log(log_text: str) -> tuple[bool, str]:
    """Independently verify assertion tokens in the archived runtime log."""
    receipt_lines = [
        line for line in log_text.splitlines()
        if line.startswith("component_runtime=")
    ]
    if len(receipt_lines) != 1:
        return False, "runtime log must contain exactly one component receipt line"
    tokens = set(receipt_lines[0].split())
    missing = [marker for marker in REQUIRED_RUNTIME_MARKERS if marker not in tokens]
    if missing:
        return False, "runtime log is missing required markers: " + ", ".join(missing)
    return True, ""


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
    source_root: Path,
    runtime_log_text: str,
    rustc_version_text: str,
    checkout_commit: str,
    checkout_tree: str,
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
    if checkout_commit != expected_commit:
        return False, "actual checkout commit does not match the packet commit"
    if not re.fullmatch(r"[0-9a-f]{40}", checkout_tree) or source.get("tree") != checkout_tree:
        return False, "receipt source tree does not match the checked-out Git tree"
    source_files = source.get("files")
    inventory_valid, inventory_error = validate_source_inventory(source_files, source_root)
    if not inventory_valid:
        return False, inventory_error

    execution = receipt.get("execution")
    if not isinstance(execution, dict):
        return False, "receipt execution section is missing"
    if re.fullmatch(r"[0-9]+", run_id) is None:
        return False, "packet run ID must be numeric"
    if re.fullmatch(r"[1-9][0-9]*", run_attempt) is None:
        return False, "packet run attempt must be a positive integer"
    if runner_os != "Linux":
        return False, "packet runner OS must be Linux"
    if re.search(
        r"/\.github/workflows/component-runtime-qualification\.yml@[^@\s]+$",
        workflow_ref,
    ) is None:
        return False, "packet workflow reference is not the runtime qualification workflow"
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
    if execution.get("workflow") != "Component Model runtime qualification":
        return False, "receipt workflow name does not match the qualification lane"
    if execution.get("rustc_version") != rustc_version_text.strip():
        return False, "receipt Rust compiler version does not match the retained compiler version record"
    if execution.get("required_runtime_markers") != list(REQUIRED_RUNTIME_MARKERS):
        return False, "receipt required runtime marker policy does not match the packet policy"
    runtime_valid, runtime_error = validate_runtime_log(runtime_log_text)
    if not runtime_valid:
        return False, runtime_error

    scope = receipt.get("scope")
    if not isinstance(scope, dict):
        return False, "receipt scope section is missing"
    for field, expected in REQUIRED_SCOPE.items():
        if scope.get(field) is not expected:
            return False, f"receipt scope {field} does not match the allowed claim boundary"

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

    fixture_path = ROOT / "tests/fixtures/first-germination.scene.json"
    source_paths = {
        "host_lockfile": ROOT / "tools/component-host-smoke/Cargo.lock",
        "component": ROOT / "target/wasm32-wasip2/release/sovereign_visual_component.wasm",
        "scene_pack_fixture": fixture_path,
        "rustc_version": runner_temp / "rustc-version.txt",
        "runtime_log": runner_temp / "component-runtime.log",
        "runtime_stderr": runner_temp / "component-runtime-stderr.log",
    }
    packet_paths = {
        "host_lockfile": evidence_dir / "component-host-Cargo.lock",
        "component": evidence_dir / "sovereign_visual_component.wasm",
        "scene_pack_fixture": evidence_dir / "first-germination.scene.json",
        "rustc_version": evidence_dir / "rustc-version.txt",
        "runtime_log": evidence_dir / "runtime.log",
        "runtime_stderr": evidence_dir / "runtime.stderr.log",
    }
    for key, source in source_paths.items():
        copy_if_present(source, packet_paths[key])

    receipt_path = evidence_dir / "qualification.json"
    collector_stderr_path = evidence_dir / "collector-stderr.log"
    files_for_hash = {
        **packet_paths,
        "qualification_receipt": receipt_path,
        "collector_stderr": collector_stderr_path,
    }
    hashes = {key: file_hash(path) for key, path in files_for_hash.items()}
    sizes = {key: file_size(path) for key, path in files_for_hash.items()}
    receipt_present = receipt_path.is_file() and receipt_path.stat().st_size > 0

    receipt: Any = None
    receipt_valid = False
    validation_error = "qualification receipt is missing"
    checkout_commit = ""
    checkout_tree = ""
    checkout_identity_error = ""
    try:
        checkout_commit = subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, check=True,
            capture_output=True, text=True,
        ).stdout.strip()
        checkout_tree = subprocess.run(
            ["git", "rev-parse", "HEAD^{tree}"], cwd=ROOT, check=True,
            capture_output=True, text=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError) as exc:
        checkout_identity_error = str(exc)

    if receipt_present:
        try:
            with receipt_path.open("r", encoding="utf-8") as stream:
                receipt = json.load(stream)
        except (OSError, UnicodeError, json.JSONDecodeError) as exc:
            validation_error = f"qualification receipt is unreadable JSON: {exc}"
        else:
            try:
                runtime_log_text = packet_paths["runtime_log"].read_text(encoding="utf-8")
                rustc_version_text = packet_paths["rustc_version"].read_text(encoding="utf-8")
                valid, validation_error = validate_receipt_document(
                    receipt,
                    expected_commit=expected_commit,
                    run_id=run_id,
                    run_attempt=run_attempt,
                    workflow_ref=workflow_ref,
                    runner_os=runner_os,
                    hashes=hashes,
                    sizes=sizes,
                    source_root=ROOT,
                    runtime_log_text=runtime_log_text,
                    rustc_version_text=rustc_version_text,
                    checkout_commit=checkout_commit,
                    checkout_tree=checkout_tree,
                )
                receipt_valid = valid
            except (OSError, UnicodeError, subprocess.CalledProcessError) as exc:
                validation_error = f"cannot independently verify checkout/runtime context: {exc}"
                receipt_valid = False
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
        "checkout_commit": checkout_commit,
        "checkout_tree": checkout_tree,
        "checkout_identity_error": checkout_identity_error,
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
