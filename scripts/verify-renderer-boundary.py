#!/usr/bin/env python3
"""Fail closed if the standalone renderer boundary regresses."""

from __future__ import annotations

import pathlib
import re
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]
errors: list[str] = []


def load_toml(path: pathlib.Path) -> dict:
    try:
        with path.open("rb") as fh:
            return tomllib.load(fh)
    except (OSError, tomllib.TOMLDecodeError) as exc:
        errors.append(f"{path.relative_to(ROOT)}: cannot parse TOML: {exc}")
        return {}


root_manifest = load_toml(ROOT / "Cargo.toml")
renderer_manifest = load_toml(ROOT / "crates/quicken-fb/Cargo.toml")

members = root_manifest.get("workspace", {}).get("members", [])
excludes = root_manifest.get("workspace", {}).get("exclude", [])
if "crates/quicken-fb" not in members:
    errors.append("Cargo workspace must include crates/quicken-fb")
if "crates/spore-kernel" not in excludes:
    errors.append("unfinished crates/spore-kernel must remain excluded from standalone qualification")

package = renderer_manifest.get("package", {})
if package.get("license") != "AGPL-3.0-or-later":
    errors.append("renderer package must declare AGPL-3.0-or-later")

deps = renderer_manifest.get("dependencies", {})
for name in ("blake3", "rand"):
    dep = deps.get(name)
    if not isinstance(dep, str) and not (
        isinstance(dep, dict) and dep.get("workspace") is True
    ):
        errors.append(f"renderer dependency {name} must remain explicit/workspace-declared")

module = ROOT / "nix/modules/sovereign-boot.nix"
module_text = module.read_text(encoding="utf-8")
for binary in ("quicken-fb", "spore-boot-state", "spore-recovery-linux"):
    if f"/bin/{binary}" not in module_text:
        errors.append(f"host module no longer references required binary: {binary}")

flake = (ROOT / "flake.nix").read_text(encoding="utf-8")
if "cargoLock.lockFile = ./Cargo.lock;" not in flake:
    errors.append("renderer flake package must consume the root Cargo.lock")

for nix_file in ROOT.rglob("*.nix"):
    if any(part in {".git", "result", "target"} for part in nix_file.parts):
        continue
    text = nix_file.read_text(encoding="utf-8")
    if re.search(r"(^|[\s=(])\.\./", text):
        errors.append(f"{nix_file.relative_to(ROOT)}: contains parent-relative path reference")

if errors:
    print("sovereign-boot renderer boundary: FAIL")
    for error in errors:
        print(f" - {error}")
    sys.exit(1)

print("sovereign-boot renderer boundary: PASS")
