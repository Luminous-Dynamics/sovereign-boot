from __future__ import annotations
import copy
import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
from validate_scene_pack import load_json_bytes, validate_manifest  # noqa: E402

SCHEMA_PATH = ROOT / "contracts/ambient-scene-pack-v1.schema.json"
EXAMPLE_PATH = ROOT / "tests/fixtures/first-germination.scene.json"
NEGATIVE_PATH = ROOT / "contracts/tests/scene-pack-v1-negative-fixtures.json"


def set_pointer(document, pointer, value):
    parts = [p.replace("~1", "/").replace("~0", "~") for p in pointer.strip("/").split("/")]
    target = document
    for part in parts[:-1]:
        target = target[int(part)] if isinstance(target, list) else target[part]
    final = parts[-1]
    if isinstance(target, list):
        target[int(final)] = copy.deepcopy(value)
    else:
        target[final] = copy.deepcopy(value)


class ScenePackValidationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
        cls.example = json.loads(EXAMPLE_PATH.read_text(encoding="utf-8"))
        cls.negative = json.loads(NEGATIVE_PATH.read_text(encoding="utf-8"))

    def test_example_and_draft_2020_12_schema_pass(self):
        issues = validate_manifest(self.schema, self.example, package_root=EXAMPLE_PATH.parent)
        self.assertEqual([], issues, "\n".join(map(str, issues)))

    def test_committed_negative_fixtures_fail_closed(self):
        for fixture in self.negative:
            with self.subTest(fixture=fixture["id"]):
                candidate = copy.deepcopy(self.example)
                set_pointer(candidate, fixture["set"]["path"], fixture["set"]["value"])
                issues = validate_manifest(self.schema, candidate, package_root=EXAMPLE_PATH.parent)
                self.assertTrue(issues, f"negative fixture unexpectedly passed: {fixture['id']}")
                self.assertIn(fixture["expectedCode"], {issue.code for issue in issues}, str(issues))

    def test_unresolved_local_schema_reference_is_rejected(self):
        candidate_schema = copy.deepcopy(self.schema)
        candidate_schema["$defs"]["color"] = {"$ref": "#/$defs/not-present"}
        issues = validate_manifest(candidate_schema, self.example, package_root=EXAMPLE_PATH.parent)
        self.assertIn("schema.unresolved_ref", {issue.code for issue in issues})

    def test_local_json_schema_anchor_reference_is_accepted(self):
        candidate_schema = copy.deepcopy(self.schema)
        candidate_schema["$defs"]["color"]["$anchor"] = "color-profile"
        candidate_schema["$defs"]["colorAlias"] = {"$ref": "#color-profile"}
        issues = validate_manifest(candidate_schema, self.example, package_root=EXAMPLE_PATH.parent)
        self.assertEqual([], issues, "\n".join(map(str, issues)))

    def test_local_dynamic_anchor_reference_is_accepted(self):
        candidate_schema = copy.deepcopy(self.schema)
        candidate_schema["$defs"]["color"]["$dynamicAnchor"] = "color-profile"
        candidate_schema["$defs"]["colorAlias"] = {"$dynamicRef": "#color-profile"}
        issues = validate_manifest(candidate_schema, self.example, package_root=EXAMPLE_PATH.parent)
        self.assertEqual([], issues, "\n".join(map(str, issues)))

    def test_unresolved_local_anchor_reference_is_rejected(self):
        candidate_schema = copy.deepcopy(self.schema)
        candidate_schema["$defs"]["color"] = {"$ref": "#anchor-that-does-not-exist"}
        issues = validate_manifest(candidate_schema, self.example, package_root=EXAMPLE_PATH.parent)
        self.assertIn("schema.unresolved_ref", {issue.code for issue in issues})

    def test_invalid_json_pointer_escape_is_rejected(self):
        candidate_schema = copy.deepcopy(self.schema)
        candidate_schema["$defs"]["color"] = {"$ref": "#/$defs/bad~2token"}
        issues = validate_manifest(candidate_schema, self.example, package_root=EXAMPLE_PATH.parent)
        self.assertIn("schema.unresolved_ref", {issue.code for issue in issues})

    def test_legacy_recursive_reference_keywords_are_rejected(self):
        for keyword, value in (("$recursiveRef", "#/$defs/color"), ("$recursiveAnchor", True)):
            with self.subTest(keyword=keyword):
                candidate_schema = copy.deepcopy(self.schema)
                candidate_schema["$defs"]["legacy"] = {keyword: value}
                issues = validate_manifest(candidate_schema, self.example, package_root=EXAMPLE_PATH.parent)
                self.assertIn("schema.unsupported_ref_keyword", {issue.code for issue in issues})

    def test_invalid_uri_format_is_rejected(self):
        candidate = copy.deepcopy(self.example)
        candidate["license"]["sourceUrl"] = "not a uri"
        issues = validate_manifest(self.schema, candidate, package_root=EXAMPLE_PATH.parent)
        self.assertIn("schema.invalid", {issue.code for issue in issues})

    def test_invalid_percent_escape_in_uri_is_rejected(self):
        candidate = copy.deepcopy(self.example)
        candidate["license"]["sourceUrl"] = "https://example.invalid/%zz"
        issues = validate_manifest(self.schema, candidate, package_root=EXAMPLE_PATH.parent)
        self.assertIn("schema.invalid", {issue.code for issue in issues})

    def test_non_ascii_uri_is_rejected(self):
        candidate = copy.deepcopy(self.example)
        candidate["license"]["sourceUrl"] = "https://example.invalid/café"
        issues = validate_manifest(self.schema, candidate, package_root=EXAMPLE_PATH.parent)
        self.assertIn("schema.invalid", {issue.code for issue in issues})

    def test_percent_encoded_uri_is_accepted(self):
        candidate = copy.deepcopy(self.example)
        candidate["license"]["sourceUrl"] = "https://example.invalid/caf%C3%A9"
        issues = validate_manifest(self.schema, candidate, package_root=EXAMPLE_PATH.parent)
        self.assertEqual([], issues, "\n".join(map(str, issues)))

    def test_non_numeric_uri_port_is_rejected(self):
        candidate = copy.deepcopy(self.example)
        candidate["license"]["sourceUrl"] = "https://example.invalid:service/path"
        issues = validate_manifest(self.schema, candidate, package_root=EXAMPLE_PATH.parent)
        self.assertIn("schema.invalid", {issue.code for issue in issues})

    def test_brackets_outside_uri_authority_are_rejected(self):
        candidate = copy.deepcopy(self.example)
        candidate["license"]["sourceUrl"] = "https://example.invalid/path[fragment]"
        issues = validate_manifest(self.schema, candidate, package_root=EXAMPLE_PATH.parent)
        self.assertIn("schema.invalid", {issue.code for issue in issues})

    def test_bracketed_ipv6_uri_is_accepted(self):
        candidate = copy.deepcopy(self.example)
        candidate["license"]["sourceUrl"] = "https://[2001:db8::1]:443/path"
        issues = validate_manifest(self.schema, candidate, package_root=EXAMPLE_PATH.parent)
        self.assertEqual([], issues, "\n".join(map(str, issues)))

    def test_invalid_schema_meta_schema_is_rejected(self):
        candidate_schema = copy.deepcopy(self.schema)
        candidate_schema["properties"]["schemaVersion"]["type"] = "not-a-json-schema-type"
        issues = validate_manifest(candidate_schema, self.example, package_root=EXAMPLE_PATH.parent)
        self.assertIn("schema.meta_schema", {issue.code for issue in issues})

    def test_external_schema_references_are_rejected(self):
        candidate_schema = copy.deepcopy(self.schema)
        candidate_schema["$defs"]["color"] = {"$ref": "https://example.invalid/untrusted-schema"}
        issues = validate_manifest(candidate_schema, self.example, package_root=EXAMPLE_PATH.parent)
        self.assertIn("schema.external_ref", {issue.code for issue in issues})

    def test_asset_bearing_manifest_requires_explicit_package_root_for_library_call(self):
        candidate = copy.deepcopy(self.example)
        candidate["assets"] = [{
            "assetId": "scene-one", "path": "scene.svg", "sha256": "0" * 64,
            "mediaType": "image/svg+xml", "license": {"spdxId": "MIT"},
        }]
        issues = validate_manifest(self.schema, candidate, package_root=None)
        self.assertIn("asset.root_required", {issue.code for issue in issues})

    def test_duplicate_json_keys_rejected(self):
        with self.assertRaisesRegex(ValueError, "duplicate JSON object key"):
            load_json_bytes(b'{"schemaVersion":1,"schemaVersion":1}')

    def test_invalid_utf8_rejected(self):
        with self.assertRaisesRegex(ValueError, "invalid start byte|invalid UTF-8"):
            load_json_bytes(b'{"title":"\xff"}')

    def test_nan_rejected_as_non_json(self):
        with self.assertRaisesRegex(ValueError, "non-JSON numeric constant"):
            load_json_bytes(b'{"value":NaN}')

    def test_asset_hash_mismatch_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "scene.svg").write_bytes(b"<svg/>")
            candidate = copy.deepcopy(self.example)
            candidate["assets"] = [{
                "assetId": "scene-one", "path": "scene.svg", "sha256": "0" * 64,
                "mediaType": "image/svg+xml", "license": {"spdxId": "MIT"},
            }]
            issues = validate_manifest(self.schema, candidate, package_root=root)
            self.assertIn("asset.hash_mismatch", {issue.code for issue in issues})

    def test_symlink_escape_rejected_even_when_hash_matches(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root, outside = base / "package", base / "outside"
            root.mkdir()
            outside.mkdir()
            payload = b"<svg/>"
            (outside / "scene.svg").write_bytes(payload)
            (root / "escape").symlink_to(outside, target_is_directory=True)
            candidate = copy.deepcopy(self.example)
            candidate["assets"] = [{
                "assetId": "scene-one", "path": "escape/scene.svg",
                "sha256": hashlib.sha256(payload).hexdigest(), "mediaType": "image/svg+xml",
                "license": {"spdxId": "MIT"},
            }]
            issues = validate_manifest(self.schema, candidate, package_root=root)
            self.assertIn("asset.symlink_escape", {issue.code for issue in issues})

    def test_required_capability_must_be_explicitly_supported(self):
        candidate = copy.deepcopy(self.example)
        candidate["capabilities"]["required"] = ["gpu"]
        rejected = validate_manifest(self.schema, candidate, package_root=EXAMPLE_PATH.parent)
        accepted = validate_manifest(self.schema, candidate, package_root=EXAMPLE_PATH.parent, supported_capabilities={"gpu"})
        self.assertIn("capability.unsupported", {issue.code for issue in rejected})
        self.assertNotIn("capability.unsupported", {issue.code for issue in accepted})


if __name__ == "__main__":
    unittest.main(verbosity=2)
