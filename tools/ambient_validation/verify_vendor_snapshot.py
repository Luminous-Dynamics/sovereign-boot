#!/usr/bin/env python3
"""Verify vendored Scene Pack files still match their pinned upstream Git blobs."""
from __future__ import annotations

import hashlib
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MANIFEST_PATH = ROOT / "contracts/UPSTREAM_SNAPSHOT.json"


def git_blob_sha1(data: bytes) -> str:
    # Git object identity is SHA-1("blob " + size + NUL + content), not SHA-1(content).
    header = b"blob " + str(len(data)).encode("ascii") + b"\0"
    return hashlib.sha1(header + data).hexdigest()


def main() -> int:
    try:
        manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
        if manifest.get("schemaVersion") != 1:
            raise ValueError("unsupported source snapshot manifest version")
        if manifest.get("identityAlgorithm") != "git-blob-sha1":
            raise ValueError("unsupported source identity algorithm")
        files = manifest.get("files")
        if not isinstance(files, list) or not files:
            raise ValueError("files must be a non-empty array")

        derived_files = manifest.get("derivedFiles", [])
        if not isinstance(derived_files, list):
            raise ValueError("derivedFiles must be an array when present")

        failed = False
        seen_paths: set[str] = set()
        for item in files:
            if not isinstance(item, dict):
                raise ValueError("each source entry must be an object")
            local_path = item.get("localPath")
            expected = item.get("gitBlobSha1")
            upstream_path = item.get("upstreamPath")
            if not all(isinstance(value, str) and value for value in (local_path, expected, upstream_path)):
                raise ValueError("each source entry requires localPath, upstreamPath and gitBlobSha1")
            if re.fullmatch(r"[0-9a-f]{40}", expected) is None:
                raise ValueError(f"invalid Git blob ID for {local_path}: expected 40 lowercase hex characters")
            if local_path in seen_paths:
                raise ValueError(f"duplicate localPath in source snapshot manifest: {local_path}")
            seen_paths.add(local_path)
            relative = Path(local_path)
            if relative.is_absolute() or ".." in relative.parts:
                raise ValueError(f"unsafe localPath in snapshot manifest: {local_path}")
            path = (ROOT / relative).resolve()
            if not path.is_relative_to(ROOT):
                raise ValueError(f"localPath resolves outside repository root: {local_path}")
            try:
                actual = git_blob_sha1(path.read_bytes())
            except OSError as exc:
                print(f"FAIL {local_path}: cannot read file: {exc}", file=sys.stderr)
                failed = True
                continue
            if actual != expected:
                print(
                    f"FAIL {local_path}: blob={actual}; expected={expected} "
                    f"(upstream {manifest['upstream']['repository']}:{upstream_path})",
                    file=sys.stderr,
                )
                failed = True
            else:
                print(f"PASS {local_path} {actual}")

        for item in derived_files:
            if not isinstance(item, dict):
                raise ValueError("each derived entry must be an object")
            local_path = item.get("path")
            expected = item.get("gitBlobSha1")
            purpose = item.get("purpose")
            if not all(isinstance(value, str) and value for value in (local_path, expected, purpose)):
                raise ValueError("each derived entry requires path, purpose and gitBlobSha1")
            if re.fullmatch(r"[0-9a-f]{40}", expected) is None:
                raise ValueError(f"invalid Git blob ID for derived file {local_path}")
            if local_path in seen_paths:
                raise ValueError(f"duplicate local path across source/derived entries: {local_path}")
            seen_paths.add(local_path)
            relative = Path(local_path)
            if relative.is_absolute() or ".." in relative.parts:
                raise ValueError(f"unsafe derived path in snapshot manifest: {local_path}")
            path = (ROOT / relative).resolve()
            if not path.is_relative_to(ROOT):
                raise ValueError(f"derived path resolves outside repository root: {local_path}")
            try:
                actual = git_blob_sha1(path.read_bytes())
            except OSError as exc:
                print(f"FAIL derived {local_path}: cannot read file: {exc}", file=sys.stderr)
                failed = True
                continue
            if actual != expected:
                print(
                    f"FAIL derived {local_path}: blob={actual}; expected={expected} ({purpose})",
                    file=sys.stderr,
                )
                failed = True
            else:
                print(f"PASS derived {local_path} {actual} ({purpose})")

        if failed:
            return 1
        print(
            f"PASS: {len(files)} upstream snapshot files and {len(derived_files)} derived overlays "
            f"match their pinned Git blob IDs (upstream commit {manifest['upstream']['commit']})"
        )
        return 0
    except (OSError, UnicodeError, json.JSONDecodeError, AttributeError, TypeError, ValueError, KeyError) as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
