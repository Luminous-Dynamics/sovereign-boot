import init, { VisualScene } from "./pkg/sovereign_visual_wasm.js";

const canvas = document.querySelector("#scene");
const context = canvas.getContext("2d", { alpha: false });
const status = document.querySelector("#status");
const pauseButton = document.querySelector("#pause");
const restartButton = document.querySelector("#restart");
const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");

const MAX_HOST_DELTA_SECONDS = 0.25;

let scenePackBytes = null;
let scene = null;
let imageData = null;
let paused = false;
let frameHandle = 0;
let lastFrameTime = 0;
let tickAccumulator = 0;
let fixedStepHz = 30;
let fixedStepSeconds = 1 / fixedStepHz;
let targetFps = 30;
let loadedPresentation = "boot";

function stopLoop() {
  if (frameHandle !== 0) {
    cancelAnimationFrame(frameHandle);
    frameHandle = 0;
  }
  // Never carry hidden/suspended time into simulation catch-up.
  lastFrameTime = 0;
  tickAccumulator = 0;
}

function renderFrame() {
  if (!scene || !context || !imageData) return;
  const rgba = scene.renderPresentedRgba();
  imageData.data.set(rgba);
  context.putImageData(imageData, 0, 0);
}

function updateControls() {
  const isStatic = !scene || scene.isStaticFallback() || reducedMotion.matches;
  pauseButton.disabled = isStatic;
  pauseButton.setAttribute("aria-pressed", String(paused));
  pauseButton.textContent = isStatic
    ? "Motion disabled by presentation"
    : paused ? "Resume animation" : "Pause animation";
  restartButton.disabled = !scene;
}

function animate(now) {
  frameHandle = 0;
  if (!scene || paused || document.hidden || scene.isStaticFallback() || reducedMotion.matches) {
    return;
  }

  const frameInterval = 1000 / Math.max(1, targetFps);
  if (lastFrameTime !== 0 && now - lastFrameTime < frameInterval) {
    frameHandle = requestAnimationFrame(animate);
    return;
  }

  const delta = lastFrameTime === 0
    ? fixedStepSeconds
    : Math.min((now - lastFrameTime) / 1000, MAX_HOST_DELTA_SECONDS);
  lastFrameTime = now;
  tickAccumulator = Math.min(
    tickAccumulator + delta,
    MAX_HOST_DELTA_SECONDS,
  );

  const ticks = Math.floor(tickAccumulator / fixedStepSeconds);
  if (ticks > 0) {
    // The core also enforces at most one simulated second per call. This
    // accumulator is tighter: it never catches up across a hidden/suspended gap.
    scene.advance_ticks(ticks, 0.55);
    tickAccumulator -= ticks * fixedStepSeconds;
  }
  renderFrame();
  frameHandle = requestAnimationFrame(animate);
}

function startLoop() {
  stopLoop();
  if (!scene || paused || document.hidden || scene.isStaticFallback() || reducedMotion.matches) return;
  frameHandle = requestAnimationFrame(animate);
}

function makeScene() {
  stopLoop();
  if (!scenePackBytes) return;

  if (scene) {
    scene.free();
    scene = null;
  }

  const cssWidth = Math.max(1, canvas.clientWidth);
  const scale = Math.min(window.devicePixelRatio || 1, 1.5);
  const width = Math.max(320, Math.min(960, Math.round(cssWidth * scale)));
  const height = Math.max(180, Math.round(width * 9 / 16));

  canvas.width = width;
  canvas.height = height;
  imageData = context.createImageData(width, height);

  // The browser host supplies manifest bytes. The WASM parser has no fetch,
  // filesystem, network or display capabilities. Reduced-motion selects the
  // manifest's required gradient-only fallback; the normal demo selects boot.
  loadedPresentation = reducedMotion.matches ? "staticFallback" : "boot";
  scene = VisualScene.createFromScenePack(
    width,
    height,
    scenePackBytes,
    loadedPresentation,
  );
  fixedStepHz = scene.fixedStepHz();
  fixedStepSeconds = 1 / fixedStepHz;
  targetFps = scene.presentationMaxFps();

  renderFrame();
  if (scene.isStaticFallback()) {
    status.textContent = "Showing the manifest's deterministic static fallback; animation is disabled.";
  } else if (paused) {
    status.textContent = "Animation paused. Restart reinitializes the same pinned Scene Pack seed.";
  } else {
    status.textContent =
      `First Germination · Scene Pack ${scene.sceneVersion()} · ${targetFps} FPS cap · local WebAssembly rendering.`;
    startLoop();
  }
  updateControls();
}

pauseButton.addEventListener("click", () => {
  if (!scene || scene.isStaticFallback() || reducedMotion.matches) return;
  paused = !paused;
  if (paused) {
    stopLoop();
    renderFrame();
    status.textContent = "Animation paused.";
  } else if (!document.hidden) {
    status.textContent = "Animation resumed.";
    startLoop();
  }
  updateControls();
});

restartButton.addEventListener("click", () => {
  makeScene();
  status.textContent = `Reinitialized ${scene?.sceneTitle() ?? "Scene Pack"} from its declared seed.`;
});

document.addEventListener("visibilitychange", () => {
  if (document.hidden) {
    stopLoop();
  } else if (!reducedMotion.matches && !paused && scene && !scene.isStaticFallback()) {
    startLoop();
  }
});

reducedMotion.addEventListener("change", () => {
  makeScene();
});

if ("ResizeObserver" in window) {
  const resizeObserver = new ResizeObserver(() => {
    if (scene) makeScene();
  });
  resizeObserver.observe(canvas);
} else {
  window.addEventListener("resize", () => {
    if (scene) makeScene();
  });
}

try {
  await init();

  // This is an explicit host fetch of a same-origin static asset. The guest
  // sees only the bounded bytes; it cannot fetch arbitrary URLs or open paths.
  const response = await fetch("./pkg/first-germination.scene.json", { cache: "no-cache" });
  if (!response.ok) {
    throw new Error(`Scene Pack request failed with HTTP ${response.status}`);
  }
  const manifest = await response.arrayBuffer();
  if (manifest.byteLength === 0 || manifest.byteLength > 1_048_576) {
    throw new Error("Scene Pack byte length is outside the accepted 1 MiB bound");
  }
  scenePackBytes = new Uint8Array(manifest);
  makeScene();
} catch (error) {
  console.error("Sovereign Visual Core failed to initialize:", error);
  stopLoop();
  if (scene) {
    scene.free();
    scene = null;
  }
  status.textContent =
    "WebAssembly or the pinned Scene Pack could not be loaded. Rebuild the demo and serve the generated directory over HTTP.";
  pauseButton.disabled = true;
  restartButton.disabled = true;
}
