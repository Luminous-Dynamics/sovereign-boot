#!/usr/bin/env node
// Runtime smoke test for the generated wasm-bindgen browser package.
// This runs under Node's WebAssembly host; it does not claim real-browser or
// desktop/mobile wallpaper integration.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const packageDir = resolve(process.argv[2] ?? "");
if (!process.argv[2]) {
  throw new Error("Usage: node tests/visual-wasm-smoke.mjs <generated-package-dir>");
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

assert.deepEqual(
  renderFixture("portable-smoke-fixture"),
  renderFixture("portable-smoke-fixture"),
  "same seed and steps should yield identical RGBA output",
);

assert.throws(() => new bindings.VisualScene(0, 24, "invalid"), /dimensions/i);
const scene = new bindings.VisualScene(16, 16, "bounds");
assert.throws(() => scene.advance(1, 0.5), /dt_seconds/i);
assert.throws(() => scene.advance(0.1, 1.5), /activity/i);
scene.free();

console.log("browser WASM runtime smoke: PASS");
