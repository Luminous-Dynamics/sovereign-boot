#!/usr/bin/env python3
"""Validate the first-party Waywallen renderer-only package contract.

This is packaging/metadata validation only. It does not claim that a renderer
binary exists, that Waywallen accepts the plugin at runtime, or that Vulkan
buffer/fence negotiation works.
"""

from __future__ import annotations

import pathlib
import sys
import tomllib


ROOT = pathlib.Path(__file__).resolve().parents[1]
PLUGIN_ROOT = ROOT / "integrations/waywallen/org.luminous-dynamics.ouroboros"
MANIFEST = PLUGIN_ROOT / "plugin.toml"
FILES_LIST = PLUGIN_ROOT / "files.txt"
EXPECTED_BINARY = "luminous-ouroboros-renderer"
EXPECTED_RENDERER = "luminous-ouroboros"
EXPECTED_PLUGIN_ID = "org.luminous-dynamics.ouroboros"
EXPECTED_SPAWN_VERSION = 10


def fail(message: str) -> None:
    raise SystemExit(f"waywallen-plugin-contract: FAIL: {message}")


def main() -> int:
    try:
        manifest = tomllib.loads(MANIFEST.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        fail(f"cannot parse plugin manifest: {error}")

    plugin = manifest.get("plugin")
    renderers = manifest.get("renderers")
    if not isinstance(plugin, dict) or not isinstance(renderers, dict):
        fail("manifest must contain [plugin] and [renderers.*] tables")

    if plugin.get("id") != EXPECTED_PLUGIN_ID:
        fail(f"unexpected plugin id: {plugin.get('id')!r}")
    if "entry" in plugin or "entry_version" in plugin:
        fail("renderer-only plugin must not declare a daemon-loaded Lua entry")

    if set(renderers) != {EXPECTED_RENDERER}:
        fail(f"unexpected renderer component set: {sorted(renderers)}")
    renderer = renderers[EXPECTED_RENDERER]
    if renderer.get("spawn_version") != EXPECTED_SPAWN_VERSION:
        fail("renderer spawn_version must match the reviewed protocol-v3 target (10)")
    if renderer.get("activity") != "continuous":
        fail("Ouroboros must declare continuous frame activity")
    if renderer.get("types") != ["ouroboros"]:
        fail("renderer must register the canonical ouroboros wallpaper type")

    binary = renderer.get("bin")
    if not isinstance(binary, str) or pathlib.PurePosixPath(binary).is_absolute():
        fail("renderer binary path must be a relative package path")
    if pathlib.PurePosixPath(binary).name != EXPECTED_BINARY:
        fail("renderer binary basename does not match the package contract")

    settings = renderer.get("settings")
    if not isinstance(settings, dict):
        fail("renderer settings table is required")
    expected_settings = {"fps", "quality", "motion"}
    if set(settings) != expected_settings:
        fail(f"settings must be exactly {sorted(expected_settings)}")
    if settings["fps"].get("type") != "u32":
        fail("fps setting must be u32")
    if (settings["fps"].get("min"), settings["fps"].get("max")) != (1, 120):
        fail("fps must remain bounded to 1..=120")
    if settings["quality"].get("type") != "string":
        fail("quality setting must be string")
    if settings["quality"].get("choices") != ["economy", "balanced", "cinematic"]:
        fail("quality choices must remain explicit and closed")
    if settings["motion"].get("type") != "string":
        fail("motion setting must be string")
    if settings["motion"].get("choices") != ["reduced", "low_power", "balanced", "cinematic"]:
        fail("motion choices must map to the four bounded scene motion policies")
    if "resolution" in settings:
        fail("Auto resolution must not be advertised before render-target capability support")

    try:
        file_entries = [
            line.strip()
            for line in FILES_LIST.read_text(encoding="utf-8").splitlines()
            if line.strip()
        ]
    except OSError as error:
        fail(f"cannot read required files.txt: {error}")

    if len(file_entries) != len(set(file_entries)):
        fail("files.txt contains duplicate paths")
    if "plugin.toml" not in file_entries:
        fail("files.txt must include plugin.toml")
    if binary not in file_entries:
        fail("files.txt must include the renderer executable")

    # Waywallen installs plugins under $out/share/waywallen/plugins/<plugin-id>.
    # The upstream glTF plugin uses the same four-level relative path to $out/bin.
    stage_prefix = pathlib.Path("/tmp/ouroboros-plugin-stage")
    installed_plugin = stage_prefix / "share/waywallen/plugins" / EXPECTED_PLUGIN_ID
    binary_target = (installed_plugin / binary).resolve()
    expected_target = (stage_prefix / "bin" / EXPECTED_BINARY).resolve()
    if binary_target != expected_target:
        fail(f"renderer path resolves outside the expected package bin directory: {binary_target}")

    for entry in file_entries:
        candidate = (installed_plugin / entry).resolve()
        try:
            candidate.relative_to(stage_prefix.resolve())
        except ValueError:
            fail(f"files.txt path escapes package prefix: {entry!r}")

    readme = (PLUGIN_ROOT / "README.md").read_text(encoding="utf-8")
    if "e2ed0b0adc6be6477c7bcc5015c76f4a1bb2b437" not in readme:
        fail("README must pin the reviewed upstream Waywallen source revision")
    if "protocol v3" not in readme or "spawn version: 10" not in readme:
        fail("README must identify protocol v3 and spawn version 10")
    if "SYNC_FD" not in readme or "release syncobj" not in readme:
        fail("README must document both acquire and release synchronization")
    if "Auto sizing" not in readme:
        fail("README must retain the deferred Auto render-target qualification note")

    print(
        "waywallen-plugin-contract: PASS "
        f"plugin={EXPECTED_PLUGIN_ID} renderer={EXPECTED_RENDERER} "
        f"spawn_version={EXPECTED_SPAWN_VERSION} files={len(file_entries)}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
