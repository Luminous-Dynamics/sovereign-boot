#!/usr/bin/env node
// Runtime smoke test for the generated wasm-bindgen browser package.
// This runs under Node's WebAssembly host; it does not claim real-browser or
// desktop/mobile wallpaper integration.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const packageDir = resolve(process.argv[2] ?? "");
if (!process.argv[2] || !process.argv[3]) {
  throw new Error(
    "Usage: node tests/visual-wasm-smoke.mjs <generated-package-dir> <native-rgba-reference>",
  );
}

const wasmPath = join(packageDir, "sovereign_visual_wasm_bg.wasm");
const jsPath = join(packageDir, "sovereign_visual_wasm.js");
const wasmBytes = await readFile(wasmPath);
const wasmModule = await WebAssembly.compile(wasmBytes);
const bindings = await import(pathToFileURL(jsPath).href);

assert.equal(typeof bindings.VisualScene, "function");
assert.equal(typeof bindings.initSync, "function");
bindings.initSync({ module: wasmModule });

function renderFixture(seed) {
  const scene = new bindings.VisualScene(32, 24, seed);
  assert.equal(scene.width(), 32);
  assert.equal(scene.height(), 24);
  assert.equal(scene.contract_version(), 1, "scene contract version is explicit");

  for (let i = 0; i < 20; i += 1) {
    scene.advance(1 / 30, 0.7);
  }

  const frame = scene.render_rgba();
  assert.ok(frame instanceof Uint8Array);
  assert.equal(frame.length, 32 * 24 * 4);
  for (let i = 3; i < frame.length; i += 4) {
    assert.equal(frame[i], 255, "every RGBA pixel must be opaque");
  }
  assert.ok(scene.branch_count() > 0);
  scene.free();
  return Buffer.from(frame);
}

const wasmFrame = renderFixture("portable-smoke-fixture");
assert.deepEqual(
  wasmFrame,
  renderFixture("portable-smoke-fixture"),
  "same seed and steps should yield identical RGBA output",
);

assert.equal(
  typeof bindings.VisualScene.createConfigured,
  "function",
  "browser adapter exposes the configured numeric-seed constructor",
);

const scenePackPalette = Uint8Array.from([
  10, 16, 14,       // canvas
  26, 46, 34,       // substrate
  126, 200, 160,    // filament
  232, 197, 71,     // node
  90, 107, 94,      // lichen
  118, 217, 193,    // glow
]);

function renderConfiguredFixture() {
  const scene = bindings.VisualScene.createConfigured(
    32, 24, 20261010,
    2048, 10, 0.28, 30, 7.5, 0.12, 128,
    scenePackPalette,
  );
  assert.equal(scene.contract_version(), 1);
  assert.equal(scene.width(), 32);
  assert.equal(scene.height(), 24);
  for (let i = 0; i < 120; i += 1) {
    scene.advance_ticks(1, 0.7);
  }
  const frame = Buffer.from(scene.render_rgba());
  assert.equal(frame.length, 32 * 24 * 4);
  assert.ok(scene.branch_count() <= 2048);
  scene.free();
  return frame;
}

assert.deepEqual(
  renderConfiguredFixture(),
  renderConfiguredFixture(),
  "numeric seed and typed settings must replay to identical RGBA bytes",
);
assert.throws(
  () => bindings.VisualScene.createConfigured(
    32, 24, 20261010, 0, 10, 0.28, 30, 7.5, 0.12, 128, scenePackPalette,
  ),
  /branch_limit/i,
);
assert.throws(
  () => bindings.VisualScene.createConfigured(
    32, 24, 20261010, 2048, 10, 0.28, 30, 7.5, 0.12, 128, new Uint8Array(17),
  ),
  /18 bytes/i,
);

const nativeFrame = await readFile(resolve(process.argv[3]));
assert.equal(nativeFrame.length, 32 * 24 * 4);
assert.deepEqual(
  wasmFrame,
  nativeFrame,
  "native Rust and browser WASM must render identical RGBA bytes for the same fixture",
);

assert.throws(() => new bindings.VisualScene(0, 24, "invalid"), /dimensions/i);
const scene = new bindings.VisualScene(16, 16, "bounds");
assert.throws(() => scene.advance(1, 0.5), /dt_seconds/i);
assert.throws(() => scene.advance(0.1, 1.5), /activity/i);
scene.free();

console.log("native/browser WASM RGBA parity: PASS");
