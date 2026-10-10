#!/usr/bin/env python3
"""Create a small, source-bound receipt for successful Component Model execution."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
REQUIRED_MARKERS = (
    "component_runtime=wasmtime-49.0.2",
    "configured_replay=byte-identical",
    "branch_budget=pass",
    "invalid_config_atomic=pass",
    "tick_batch_bound=pass",
    "wasi_imports=none",
    "resources=dropped",
)
def missing_required_markers(log_text: str) -> list[str]:
    """Require one explicit host receipt line containing every assertion marker."""
    receipt_lines = [
        line for line in log_text.splitlines()
        if line.startswith("component_runtime=")
    ]
    if len(receipt_lines) != 1:
        return ["exactly one component_runtime receipt line"]
    return [marker for marker in REQUIRED_MARKERS if marker not in receipt_lines[0]]


SOURCE_PATHS = (
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "crates/visual-core/src/settings.rs",
    "crates/visual-pack/src/lib.rs",
    "crates/visual-component/wit/visual.wit",
    "tools/component-host-smoke/Cargo.toml",
    "tools/component-host-smoke/src/main.rs",
)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def file_record(relative: str) -> dict[str, object]:
    path = (ROOT / relative).resolve()
    if not path.is_relative_to(ROOT) or not path.is_file():
        raise ValueError(f"required evidence input is missing or outside repository: {relative}")
    return {"path": relative, "bytes": path.stat().st_size, "sha256": sha256_file(path)}


def git_output(*args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout.strip()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--component", required=True, type=Path)
    parser.add_argument("--fixture", required=True, type=Path)
    parser.add_argument("--lockfile", required=True, type=Path)
    parser.add_argument("--runtime-log", required=True, type=Path)
    parser.add_argument("--rustc-version", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    try:
        # Do not issue a pass receipt just because the process exited 0:
        # verify the host's explicit assertions are present in the log.
        log_bytes = args.runtime_log.read_bytes()
        log_text = log_bytes.decode("utf-8", errors="strict")
        missing = missing_required_markers(log_text)
        if missing:
            raise ValueError("runtime receipt missing required success markers: " + ", ".join(missing))

        component = args.component.resolve()
        fixture = args.fixture.resolve()
        lockfile = args.lockfile.resolve()
        for path in (component, fixture, lockfile):
            if not path.is_file():
                raise ValueError(f"required evidence file does not exist: {path}")
        if component.stat().st_size == 0 or lockfile.stat().st_size == 0:
            raise ValueError("component and host lockfile must be non-empty")

        evidence = {
            "schema": "luminous-sovereign-visual-component-runtime-evidence-v1",
            "result": "pass",
            "source": {
                "commit": git_output("rev-parse", "HEAD"),
                "tree": git_output("rev-parse", "HEAD^{tree}"),
                "files": [file_record(path) for path in SOURCE_PATHS],
            },
            "execution": {
                "workflow": os.environ.get("GITHUB_WORKFLOW", ""),
                "run_id": os.environ.get("GITHUB_RUN_ID", ""),
                "run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT", ""),
                "workflow_ref": os.environ.get("GITHUB_WORKFLOW_REF", ""),
                "runner_os": os.environ.get("RUNNER_OS", ""),
                "rustc_version": args.rustc_version.strip(),
                "required_runtime_markers": list(REQUIRED_MARKERS),
                "runtime_log_sha256": hashlib.sha256(log_bytes).hexdigest(),
            },
            "artifacts": {
                "component": {
                    "path": str(args.component),
                    "bytes": component.stat().st_size,
                    "sha256": sha256_file(component),
                },
                "scene_pack_fixture": {
                    "path": str(args.fixture),
                    "bytes": fixture.stat().st_size,
                    "sha256": sha256_file(fixture),
                },
                "host_lockfile": {
                    "path": str(args.lockfile),
                    "bytes": lockfile.stat().st_size,
                    "sha256": sha256_file(lockfile),
                },
            },
            "scope": {
                "component_instantiated": True,
                "host_imports_registered": False,
                "configured_replay_byte_identical": True,
                "hosted_ci_only": True,
                "browser_presentation": False,
                "physical_boot": False,
                "whole_process_memory_limit": False,
            },
        }
        output = args.output.resolve()
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(f"PASS: wrote source-bound runtime evidence to {output}")
        return 0
    except (OSError, UnicodeError, subprocess.CalledProcessError, ValueError, TypeError) as exc:
        print(f"ERROR: cannot create runtime evidence: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
