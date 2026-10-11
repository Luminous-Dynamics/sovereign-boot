#!/usr/bin/env python3
"""Tests for malformed and path-escaping upstream snapshot manifests."""
from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPT = Path(__file__).with_name("verify_vendor_snapshot.py")
SPEC = importlib.util.spec_from_file_location("vendor_snapshot_verifier", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
verifier = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(verifier)


def source_entry(path: str, payload: bytes, upstream_path: str | None = None) -> dict[str, str]:
    return {
        "localPath": path,
        "upstreamPath": upstream_path or path,
        "gitBlobSha1": verifier.git_blob_sha1(payload),
    }


class VendorSnapshotVerifierTests(unittest.TestCase):
    def run_verifier(
        self,
        root: Path,
        entries: list[dict[str, str]],
        derived: list[dict[str, str]] | None = None,
    ) -> tuple[int, str, str]:
        manifest_path = root / "contracts" / "UPSTREAM_SNAPSHOT.json"
        manifest_path.parent.mkdir(parents=True, exist_ok=True)
        manifest_path.write_text(
            json.dumps(
                {
                    "schemaVersion": 1,
                    "upstream": {
                        "repository": "example/upstream",
                        "commit": "a" * 40,
                        "commitUrl": "https://example.invalid/commit/" + "a" * 40,
                    },
                    "identityAlgorithm": "git-blob-sha1",
                    "files": entries,
                    "derivedFiles": derived or [],
                }
            ),
            encoding="utf-8",
        )
        stdout, stderr = io.StringIO(), io.StringIO()
        with (
            patch.object(verifier, "ROOT", root.resolve()),
            patch.object(verifier, "MANIFEST_PATH", manifest_path),
            contextlib.redirect_stdout(stdout),
            contextlib.redirect_stderr(stderr),
        ):
            result = verifier.main()
        return result, stdout.getvalue(), stderr.getvalue()

    def test_source_and_derived_blob_identities_pass(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, overlay = b"pinned source\n", b"local integration overlay\n"
            (root / "source.py").write_bytes(source)
            (root / "overlay.py").write_bytes(overlay)
            result, stdout, stderr = self.run_verifier(
                root,
                [source_entry("source.py", source)],
                [{
                    "path": "overlay.py",
                    "purpose": "test overlay",
                    "gitBlobSha1": verifier.git_blob_sha1(overlay),
                }],
            )
            self.assertEqual(0, result, stderr)
            self.assertIn("PASS source.py", stdout)
            self.assertIn("PASS derived overlay.py", stdout)

    def test_invalid_git_blob_id_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            payload = b"file"
            (root / "source.py").write_bytes(payload)
            entry = source_entry("source.py", payload)
            entry["gitBlobSha1"] = "not-a-git-blob-id"
            result, _, stderr = self.run_verifier(root, [entry])
            self.assertEqual(2, result)
            self.assertIn("invalid Git blob ID", stderr)

    def test_duplicate_local_path_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            payload = b"file"
            (root / "source.py").write_bytes(payload)
            entry = source_entry("source.py", payload)
            result, _, stderr = self.run_verifier(root, [entry, entry.copy()])
            self.assertEqual(2, result)
            self.assertIn("duplicate localPath", stderr)

    def test_parent_traversal_is_rejected_before_read(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            result, _, stderr = self.run_verifier(
                root,
                [source_entry("../outside.txt", b"outside")],
            )
            self.assertEqual(2, result)
            self.assertIn("unsafe localPath", stderr)

    def test_symlink_escape_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root, outside = base / "repo", base / "outside"
            root.mkdir()
            outside.mkdir()
            payload = b"outside"
            (outside / "source.py").write_bytes(payload)
            (root / "escape.py").symlink_to(outside / "source.py")
            result, _, stderr = self.run_verifier(
                root,
                [source_entry("escape.py", payload)],
            )
            self.assertEqual(2, result)
            self.assertIn("resolves outside repository root", stderr)

    def test_malformed_derived_blob_id_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            payload = b"source"
            (root / "source.py").write_bytes(payload)
            result, _, stderr = self.run_verifier(
                root,
                [source_entry("source.py", payload)],
                [{"path": "overlay.py", "purpose": "test", "gitBlobSha1": "bad"}],
            )
            self.assertEqual(2, result)
            self.assertIn("invalid Git blob ID for derived file", stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
