#!/usr/bin/env node
// Runtime smoke test for the generated wasm-bindgen browser package.
// This runs under Node's WebAssembly host; it does not claim real-browser or
// desktop/mobile wallpaper integration.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const packageDir = resolve(process.argv[2] ?? "");
if (!process.argv[2] || !process.argv[3] || !process.argv[4]) {
  throw new Error(
    "Usage: node tests/visual-wasm-smoke.mjs <generated-package-dir> <native-legacy-rgba-reference> <native-configured-rgba-reference>",
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

// Use the byte-pinned upstream Scene Pack as the shared cross-target input.
// The test covers both direct typed-API compatibility and the production strict
// parser path; it runs under Node and does not claim real-browser presentation.
const sourceScenePackBytes = await readFile(
  resolve("tests/fixtures/first-germination.scene.json"),
);
const scenePackBytes = await readFile(join(packageDir, "first-germination.scene.json"));
assert.deepEqual(
  scenePackBytes,
  sourceScenePackBytes,
  "browser package must contain the byte-pinned source manifest",
);
const scenePack = JSON.parse(scenePackBytes.toString("utf8"));
assert.equal(scenePack.schemaVersion, 1);
assert.equal(scenePack.simulation.engine, "mycelial-network-v1");
const simulation = scenePack.simulation;
const parameters = simulation.parameters;
const resourceBudget = scenePack.resourceBudget;
const staticFallback = scenePack.presentations.staticFallback;
const rgb = (hex) => {
  assert.match(hex, /^#[0-9A-Fa-f]{6}$/);
  return [0, 2, 4].map((offset) => Number.parseInt(hex.slice(offset + 1, offset + 3), 16));
};
const scenePackPalette = Uint8Array.from([
  ...rgb(scenePack.palette.canvas),
  ...rgb(scenePack.palette.substrate),
  ...rgb(scenePack.palette.filament),
  ...rgb(scenePack.palette.node),
  ...rgb(scenePack.palette.lichen),
  ...rgb(scenePack.palette.glow),
]);
assert.ok(parameters.branchLimit <= resourceBudget.maxBranches);
assert.ok(simulation.fixedStepHz >= 1 && simulation.fixedStepHz <= 120);

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

assert.equal(
  typeof bindings.VisualScene.createFromScenePack,
  "function",
  "browser adapter exposes the validated Scene Pack constructor",
);

function renderPackBootFixture() {
  const scene = bindings.VisualScene.createFromScenePack(
    32,
    24,
    scenePackBytes,
    "boot",
  );
  assert.equal(scene.sceneId(), "luminous.first-germination");
  assert.equal(scene.sceneVersion(), "0.1.0");
  assert.equal(scene.sceneTitle(), "First Germination");
  assert.equal(scene.fixedStepHz(), simulation.fixedStepHz);
  assert.equal(scene.presentationMaxFps(), scenePack.presentations.boot.maxFps);
  assert.equal(scene.isStaticFallback(), false);

  for (let batch = 0; batch < 4; batch += 1) {
    scene.advance_ticks(simulation.fixedStepHz, 0.7);
  }
  const rgba = Buffer.from(scene.renderPresentedRgba());
  assert.equal(rgba.length, 32 * 24 * 4);
  scene.free();
  return rgba;
}

assert.deepEqual(
  renderPackBootFixture(),
  renderConfiguredFixture(),
  "the browser Scene Pack constructor must map to the same core engine/settings",
);

const packStaticScene = bindings.VisualScene.createFromScenePack(
  16,
  16,
  scenePackBytes,
  "staticFallback",
);
assert.equal(packStaticScene.isStaticFallback(), true);
assert.equal(packStaticScene.presentationMaxFps(), 0);
const packStaticFrame = Buffer.from(packStaticScene.renderPresentedRgba());
assert.deepEqual(packStaticFrame.subarray(0, 4), Buffer.from([6, 9, 8, 255]));
assert.deepEqual(
  packStaticFrame.subarray(packStaticFrame.length - 4),
  Buffer.from([15, 27, 20, 255]),
);
packStaticScene.free();

assert.throws(
  () => bindings.VisualScene.createFromScenePack(32, 24, scenePackBytes, "desktop"),
  /safeRegions/i,
  "the browser adapter must reject presentation fields it has not implemented",
);

const unsupportedCapabilityPack = structuredClone(scenePack);
unsupportedCapabilityPack.capabilities.required = ["gpu"];
assert.throws(
  () => bindings.VisualScene.createFromScenePack(
    32,
    24,
    new TextEncoder().encode(JSON.stringify(unsupportedCapabilityPack)),
    "boot",
  ),
  /cannot satisfy required Scene Pack capabilities/i,
  "the browser adapter must fail closed when a valid pack requires unsupported host capability",
);


function renderConfiguredFixture() {
  const scene = bindings.VisualScene.createConfigured(
    32, 24, simulation.seed,
    parameters.branchLimit,
    resourceBudget.maxBranches,
    parameters.maxDepth,
    parameters.growthRate,
    simulation.fixedStepHz,
    parameters.pulsePeriodSeconds,
    parameters.driftAmplitude,
    resourceBudget.maxMemoryMiB,
    scenePackPalette,
  );
  assert.equal(scene.contract_version(), 1);
  assert.equal(scene.width(), 32);
  assert.equal(scene.height(), 24);
  const beforeFallback = Buffer.from(scene.render_rgba());
  const fullBrightnessGradient = Buffer.from(scene.render_static_fallback(1.0));
  assert.equal(fullBrightnessGradient.length, 32 * 24 * 4);
  assert.deepEqual(
    fullBrightnessGradient.subarray(0, 4),
    Buffer.from([10, 16, 14, 255]),
    "static gradient starts at the configured canvas color",
  );
  assert.deepEqual(
    fullBrightnessGradient.subarray(fullBrightnessGradient.length - 4),
    Buffer.from([26, 46, 34, 255]),
    "static gradient ends at the configured substrate color",
  );
  const manifestFallback = Buffer.from(
    scene.render_static_fallback(staticFallback.brightness),
  );
  assert.deepEqual(
    manifestFallback.subarray(0, 4),
    Buffer.from([6, 9, 8, 255]),
    "fixture static-fallback brightness is applied at the canvas endpoint",
  );
  assert.deepEqual(
    manifestFallback.subarray(manifestFallback.length - 4),
    Buffer.from([15, 27, 20, 255]),
    "fixture static-fallback brightness is applied at the substrate endpoint",
  );
  assert.deepEqual(
    Buffer.from(scene.render_rgba()),
    beforeFallback,
    "rendering the static fallback must not mutate the scene",
  );
  assert.throws(
    () => scene.render_static_fallback(1.1),
    /brightness/i,
  );
  assert.deepEqual(
    Buffer.from(scene.render_rgba().slice(0, 4)),
    Buffer.from([10, 16, 14, 255]),
    "configured canvas color is applied before stepping",
  );
  assert.throws(
    () => scene.advance_ticks(simulation.fixedStepHz + 1, 0.7),
    /tick batch must/i,
    "simulation catch-up cannot exceed the core batch ceiling",
  );
  for (let i = 0; i < simulation.fixedStepHz * 4; i += 1) {
    scene.advance_ticks(1, 0.7);
  }
  const frame = Buffer.from(scene.render_rgba());
  assert.equal(frame.length, 32 * 24 * 4);
  assert.ok(scene.branch_count() <= resourceBudget.maxBranches);
  scene.free();
  return frame;
}

assert.deepEqual(
  renderConfiguredFixture(),
  renderConfiguredFixture(),
  "numeric seed and typed settings must replay to identical RGBA bytes",
);

const variableModeScene = new bindings.VisualScene(16, 16, "variable-mode");
variableModeScene.advance(1 / 30, 0.7);
assert.throws(
  () => variableModeScene.advance_ticks(1, 0.7),
  /cannot mix/i,
  "variable-delta stepping must not switch to fixed ticks",
);
variableModeScene.free();

const fixedModeScene = bindings.VisualScene.createConfigured(
  16, 16, simulation.seed, parameters.branchLimit, resourceBudget.maxBranches, parameters.maxDepth,
  parameters.growthRate, simulation.fixedStepHz, parameters.pulsePeriodSeconds,
  parameters.driftAmplitude, resourceBudget.maxMemoryMiB, scenePackPalette,
);
fixedModeScene.advance_ticks(1, 0.7);
assert.throws(
  () => fixedModeScene.advance(1 / 30, 0.7),
  /cannot mix/i,
  "fixed-tick stepping must not switch to variable deltas",
);
fixedModeScene.free();
assert.throws(
  () => bindings.VisualScene.createConfigured(
    32, 24, simulation.seed, 0, resourceBudget.maxBranches, parameters.maxDepth, parameters.growthRate,
    simulation.fixedStepHz, parameters.pulsePeriodSeconds, parameters.driftAmplitude,
    resourceBudget.maxMemoryMiB, scenePackPalette,
  ),
  /branch_limit/i,
);
assert.throws(
  () => bindings.VisualScene.createConfigured(
    32, 24, simulation.seed, parameters.branchLimit, resourceBudget.maxBranches - 1,
    parameters.maxDepth, parameters.growthRate, simulation.fixedStepHz, parameters.pulsePeriodSeconds,
    parameters.driftAmplitude, resourceBudget.maxMemoryMiB, scenePackPalette,
  ),
  /branch_limit exceeds resource_budget\.max_branches/i,
  "scene-specific request must fit the independent host branch budget",
);
assert.throws(
  () => bindings.VisualScene.createConfigured(
    32, 24, simulation.seed, parameters.branchLimit, resourceBudget.maxBranches, parameters.maxDepth,
    parameters.growthRate, simulation.fixedStepHz, parameters.pulsePeriodSeconds,
    parameters.driftAmplitude, resourceBudget.maxMemoryMiB, new Uint8Array(17),
  ),
  /18 bytes/i,
);

const nativeFrame = await readFile(resolve(process.argv[3]));
assert.equal(nativeFrame.length, 32 * 24 * 4);
assert.deepEqual(
  wasmFrame,
  nativeFrame,
  "native Rust and browser WASM must render identical RGBA bytes for the legacy phrase-seed fixture",
);

const nativeConfiguredFrame = await readFile(resolve(process.argv[4]));
assert.equal(nativeConfiguredFrame.length, 32 * 24 * 4);
assert.deepEqual(
  renderConfiguredFixture(),
  nativeConfiguredFrame,
  "native Rust and browser WASM must render identical RGBA bytes for the configured numeric-seed Scene Pack settings",
);
assert.deepEqual(
  renderPackBootFixture(),
  nativeConfiguredFrame,
  "the browser's production Scene Pack parser path must match the native configured reference frame",
);

assert.throws(() => new bindings.VisualScene(0, 24, "invalid"), /dimensions/i);
const scene = new bindings.VisualScene(16, 16, "bounds");
assert.throws(() => scene.advance(1, 0.5), /dt_seconds/i);
assert.throws(() => scene.advance(0.1, 1.5), /activity/i);
scene.free();

console.log("native/browser WASM RGBA parity (legacy + configured Scene Pack): PASS");
