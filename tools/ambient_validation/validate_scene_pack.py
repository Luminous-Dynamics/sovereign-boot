#!/usr/bin/env python3
"""Validate Scene Pack v1 schema, cross-field invariants, and packaged assets."""
from __future__ import annotations
import argparse, hashlib, json, re, sys
from dataclasses import dataclass
from hmac import compare_digest
from pathlib import Path, PurePosixPath
from typing import Any, Iterable
from jsonschema import Draft202012Validator, FormatChecker
from jsonschema.exceptions import SchemaError

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_SCHEMA = ROOT / "contracts/ambient-scene-pack-v1.schema.json"
DRAFT = "https://json-schema.org/draft/2020-12/schema"
CAPABILITIES = {"gpu", "dmabuf", "vulkan", "egl", "wayland-layer-shell"}

# jsonschema's optional URI backend varies by installation. Keep the Scene Pack
# v1 URI-format gate deterministic even when optional extras are absent.
STRICT_FORMAT_CHECKER = FormatChecker()


@STRICT_FORMAT_CHECKER.checks("uri")
def _strict_absolute_uri(value: Any) -> bool:
    if not isinstance(value, str):
        return True
    if (
        not value
        or not value.isascii()
        or re.search(r"[\x00-\x20\x7f<>\"{}|\\^]", value)
        or chr(96) in value
        or re.search(r"%(?![0-9A-Fa-f]{2})", value)
    ):
        return False
        return False
    from urllib.parse import urlsplit

    try:
        parsed = urlsplit(value)
    except ValueError:
        return False
    return bool(re.fullmatch(r"[A-Za-z][A-Za-z0-9+.-]*", parsed.scheme))


class DuplicateKeyError(ValueError):
    pass


def _object_pairs(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise DuplicateKeyError(f"duplicate JSON object key: {key!r}")
        result[key] = value
    return result


def _reject_constant(value: str) -> None:
    raise ValueError(f"non-JSON numeric constant is forbidden: {value}")


def load_json_bytes(raw: bytes, *, label: str = "JSON document") -> Any:
    """Read strict UTF-8 JSON; reject duplicate keys and NaN/Infinity."""
    try:
        return json.loads(raw.decode("utf-8", errors="strict"),
                          object_pairs_hook=_object_pairs, parse_constant=_reject_constant)
    except (UnicodeDecodeError, json.JSONDecodeError, DuplicateKeyError, ValueError) as exc:
        raise ValueError(f"{label}: {exc}") from exc


def load_json_file(path: Path) -> Any:
    return load_json_bytes(path.read_bytes(), label=str(path))


def _walk(value: Any) -> Iterable[Any]:
    yield value
    if isinstance(value, dict):
        for child in value.values():
            yield from _walk(child)
    elif isinstance(value, list):
        for child in value:
            yield from _walk(child)


@dataclass(frozen=True)
class Issue:
    code: str
    message: str

    def __str__(self) -> str:
        return f"[{self.code}] {self.message}"


def _resolve_local_reference(document: Any, ref: str) -> tuple[bool, str]:
    """Resolve a document-local JSON Schema URI reference without fetching resources."""
    from urllib.parse import unquote, urlsplit

    parsed = urlsplit(ref)
    if parsed.scheme or parsed.netloc or parsed.path or parsed.query:
        return False, "reference is not a fragment-only URI"

    # URI fragment percent-decoding precedes JSON Pointer/anchor interpretation.
    fragment = unquote(parsed.fragment)
    if not fragment:
        return True, ""
    if fragment.startswith("/"):
        pointer = fragment[1:]
        current = document
        for raw_token in pointer.split("/"):
            # RFC 6901 escaping permits only ~0 and ~1.
            i = 0
            while i < len(raw_token):
                if raw_token[i] == "~":
                    if i + 1 >= len(raw_token) or raw_token[i + 1] not in "01":
                        return False, f"invalid JSON Pointer escape in {raw_token!r}"
                    i += 2
                else:
                    i += 1
            token = raw_token.replace("~1", "/").replace("~0", "~")
            if isinstance(current, dict) and token in current:
                current = current[token]
            elif isinstance(current, list) and token.isdigit():
                index = int(token)
                if index >= len(current) or str(index) != token:
                    return False, f"array index {token!r} does not exist"
                current = current[index]
            else:
                return False, f"pointer token {token!r} does not exist"
        return True, ""

    # Draft 2020-12 permits plain-name fragments that target $anchor or
    # $dynamicAnchor; treating every non-pointer fragment as an external ref
    # would reject valid schemas even though no resource retrieval is needed.
    for node in _walk(document):
        if isinstance(node, dict) and (
            node.get("$anchor") == fragment or node.get("$dynamicAnchor") == fragment
        ):
            return True, ""
    return False, f"anchor {fragment!r} does not exist"


def check_schema(schema: Any) -> list[Issue]:
    if not isinstance(schema, dict):
        return [Issue("schema.invalid_document", "schema root must be an object")]
    if schema.get("$schema") != DRAFT:
        return [Issue("schema.wrong_dialect", f"$schema must be {DRAFT}")]

    # Diagnose unsupported legacy syntax before Draft 2020-12's meta-schema
    # rejects it, so the error identifies the actual compatibility boundary.
    for node in _walk(schema):
        if not isinstance(node, dict):
            continue
        for legacy_keyword in ("$recursiveRef", "$recursiveAnchor"):
            if legacy_keyword in node:
                return [Issue(
                    "schema.unsupported_ref_keyword",
                    f"{legacy_keyword} is a legacy reference keyword unsupported by the Draft 2020-12 profile",
                )]

    try:
        Draft202012Validator.check_schema(schema)
    except SchemaError as exc:
        return [Issue("schema.meta_schema", f"invalid Draft 2020-12 schema: {exc.message}")]

    # References are document-local only; never fetch remote schema resources.
    for node in _walk(schema):
        if not isinstance(node, dict):
            continue
        for keyword in ("$ref", "$dynamicRef"):
            if keyword not in node:
                continue
            ref = node[keyword]
            if not isinstance(ref, str):
                return [Issue("schema.invalid_ref", f"{keyword} must be a string")]
            ok, detail = _resolve_local_reference(schema, ref)
            if not ok:
                from urllib.parse import urlsplit
                parsed = urlsplit(ref)
                local_only = not (parsed.scheme or parsed.netloc or parsed.path or parsed.query)
                code = "schema.unresolved_ref" if local_only else "schema.external_ref"
                return [Issue(code, f"{keyword} {ref!r}: {detail}")]
    return []

def _resolve_asset(package_root: Path, asset_path: str) -> tuple[Path | None, Issue | None]:
    """Resolve a package-relative asset without traversal or symlink escape."""
    if not asset_path or "\\" in asset_path or "\x00" in asset_path:
        return None, Issue("asset.path_escape", f"invalid relative asset path {asset_path!r}")
    raw_parts = asset_path.split("/")
    if asset_path.startswith("/") or any(part in ("", ".", "..") for part in raw_parts):
        return None, Issue("asset.path_escape", f"unsafe relative asset path {asset_path!r}")

    root = package_root.resolve()
    relative = PurePosixPath(asset_path)
    candidate = root.joinpath(*relative.parts)
    traversed_symlink = False
    current = root
    for part in relative.parts:
        current = current / part
        try:
            traversed_symlink = traversed_symlink or current.is_symlink()
        except OSError:
            pass

    try:
        resolved = candidate.resolve(strict=False)
    except (OSError, RuntimeError) as exc:
        return None, Issue("asset.resolve_error", f"cannot resolve asset {asset_path!r}: {exc}")
    if not resolved.is_relative_to(root):
        code = "asset.symlink_escape" if traversed_symlink else "asset.path_escape"
        return None, Issue(code, f"asset path {asset_path!r} resolves outside the package root")
    return resolved, None


def validate_semantics(manifest: Any, *, package_root: Path | None = None,
                       supported_capabilities: set[str] | None = None) -> list[Issue]:
    """Enforce v1 cross-field, duplicate-ID, capability, and asset rules."""
    if not isinstance(manifest, dict):
        return [Issue("semantic.root_type", "manifest root must be an object")]
    issues: list[Issue] = []
    assets = manifest.get("assets", [])
    ids = [item.get("assetId") for item in assets if isinstance(item, dict)]
    if len(ids) != len(set(ids)):
        issues.append(Issue("asset.duplicate_id", "assetId values must be unique"))
    inputs = manifest.get("inputs", [])
    ids = [item.get("id") for item in inputs if isinstance(item, dict)]
    if len(ids) != len(set(ids)):
        issues.append(Issue("input.duplicate_id", "input id values must be unique"))

    budget = manifest.get("resourceBudget", {})
    presentations = manifest.get("presentations", {})
    max_fps = budget.get("maxFps")
    if isinstance(presentations, dict) and isinstance(max_fps, int):
        for name, item in presentations.items():
            if isinstance(item, dict) and isinstance(item.get("maxFps"), int) and item["maxFps"] > max_fps:
                issues.append(Issue("budget.fps", f"presentations.{name}.maxFps exceeds resourceBudget.maxFps"))
    branch_limit = manifest.get("simulation", {}).get("parameters", {}).get("branchLimit")
    max_branches = budget.get("maxBranches")
    if isinstance(branch_limit, int) and isinstance(max_branches, int) and branch_limit > max_branches:
        issues.append(Issue("budget.branches", "branchLimit exceeds resourceBudget.maxBranches"))

    fallback = presentations.get("staticFallback") if isinstance(presentations, dict) else None
    if isinstance(fallback, dict) and (fallback.get("motion") != "none" or fallback.get("maxFps") != 0
                                       or fallback.get("composition") != "gradient-only"):
        issues.append(Issue("fallback.invalid", "staticFallback must use motion=none, maxFps=0, composition=gradient-only"))
    if isinstance(presentations, dict):
        for name, item in presentations.items():
            if not isinstance(item, dict):
                continue
            for region in item.get("safeRegions", []):
                if not isinstance(region, dict):
                    continue
                x, y, width, height = (region.get(k) for k in ("x", "y", "width", "height"))
                if isinstance(x, (int, float)) and isinstance(width, (int, float)) and x + width > 1:
                    issues.append(Issue("region.out_of_bounds", f"{name}.{region.get('id', '?')}: x + width exceeds 1"))
                if isinstance(y, (int, float)) and isinstance(height, (int, float)) and y + height > 1:
                    issues.append(Issue("region.out_of_bounds", f"{name}.{region.get('id', '?')}: y + height exceeds 1"))

    for capability in manifest.get("capabilities", {}).get("required", []):
        if capability not in (supported_capabilities or set()):
            issues.append(Issue("capability.unsupported", f"required capability {capability!r} is not declared supported"))

    if assets:
        if package_root is None:
            issues.append(Issue("asset.root_required", "assets require an explicit package root"))
        else:
            for asset in assets:
                if not isinstance(asset, dict) or not isinstance(asset.get("path"), str):
                    continue
                resolved, error = _resolve_asset(package_root, asset["path"])
                if error:
                    issues.append(error)
                    continue
                assert resolved is not None
                try:
                    digest = hashlib.sha256(resolved.read_bytes()).hexdigest()
                except OSError as exc:
                    issues.append(Issue("asset.read_error", f"cannot read {asset['path']!r}: {exc}"))
                    continue
                expected = asset.get("sha256", "")
                if not isinstance(expected, str) or not compare_digest(digest, expected):
                    issues.append(Issue("asset.hash_mismatch", f"SHA-256 mismatch for {asset['path']!r}"))
    return issues


def validate_manifest(schema: Any, manifest: Any, *, package_root: Path | None = None,
                      supported_capabilities: set[str] | None = None) -> list[Issue]:
    schema_issues = check_schema(schema)
    if schema_issues:
        return schema_issues
    validator = Draft202012Validator(schema, format_checker=STRICT_FORMAT_CHECKER)
    errors = sorted(validator.iter_errors(manifest),
                    key=lambda e: (tuple(map(str, e.absolute_path)), e.message))
    if errors:
        return [Issue("schema.invalid", f"at /{'/'.join(map(str, e.absolute_path))}: {e.message}") for e in errors]
    return validate_semantics(manifest, package_root=package_root,
                              supported_capabilities=supported_capabilities)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path, help="Scene Pack JSON manifest")
    parser.add_argument("--schema", type=Path, default=DEFAULT_SCHEMA)
    parser.add_argument("--asset-root", type=Path, help="unpacked package root for relative asset paths")
    parser.add_argument("--supports-capability", action="append", default=[], choices=sorted(CAPABILITIES))
    args = parser.parse_args(argv)
    try:
        schema, manifest = load_json_file(args.schema), load_json_file(args.manifest)
    except (OSError, ValueError) as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2
    root = args.asset_root if args.asset_root is not None else args.manifest.parent
    issues = validate_manifest(schema, manifest, package_root=root,
                               supported_capabilities=set(args.supports_capability))
    if issues:
        for issue in issues:
            print(f"ERROR: {issue}", file=sys.stderr)
        return 1
    print(f"PASS: {args.manifest} validates against JSON Schema Draft 2020-12 and Scene Pack v1 semantic checks")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
