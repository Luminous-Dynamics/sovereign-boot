import init, { VisualScene } from "./pkg/sovereign_visual_wasm.js";

const canvas = document.querySelector("#scene");
const context = canvas.getContext("2d", { alpha: false });
const status = document.querySelector("#status");
const pauseButton = document.querySelector("#pause");
const regenerateButton = document.querySelector("#regenerate");
const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");

const TARGET_FPS = 30;
const FIXED_STEP_HZ = 30;
const FIXED_STEP_SECONDS = 1 / FIXED_STEP_HZ;
const MAX_HOST_DELTA_SECONDS = 0.25;
const PALETTE_RGB = Uint8Array.from([
  10, 16, 14,       // canvas
  26, 46, 34,       // substrate
  126, 200, 160,    // filament
  232, 197, 71,     // node
  90, 107, 94,      // lichen
  118, 217, 193,    // glow
]);

let scene = null;
let imageData = null;
let generation = 0;
let paused = false;
let frameHandle = 0;
let lastFrameTime = 0;
let tickAccumulator = 0;

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
  const rgba = scene.render_rgba();
  imageData.data.set(rgba);
  context.putImageData(imageData, 0, 0);
}

function updateControls() {
  if (reducedMotion.matches) {
    pauseButton.disabled = true;
    pauseButton.setAttribute("aria-pressed", "true");
    pauseButton.textContent = "Motion disabled by system preference";
  } else {
    pauseButton.disabled = !scene;
    pauseButton.setAttribute("aria-pressed", String(paused));
    pauseButton.textContent = paused ? "Resume animation" : "Pause animation";
  }
  regenerateButton.disabled = !scene;
}

function advanceStaticFrame() {
  // One bounded canonical batch: no animation loop is scheduled in reduced
  // motion mode. The host still presents a frame through the normal canvas.
  scene.advance_ticks(40, 0.55);
  renderFrame();
}

function animate(now) {
  frameHandle = 0;
  if (!scene || paused || document.hidden || reducedMotion.matches) return;

  if (lastFrameTime !== 0 && now - lastFrameTime < 1000 / TARGET_FPS) {
    frameHandle = requestAnimationFrame(animate);
    return;
  }

  const delta = lastFrameTime === 0
    ? FIXED_STEP_SECONDS
    : Math.min((now - lastFrameTime) / 1000, MAX_HOST_DELTA_SECONDS);
  lastFrameTime = now;
  tickAccumulator = Math.min(
    tickAccumulator + delta,
    MAX_HOST_DELTA_SECONDS,
  );

  const ticks = Math.floor(tickAccumulator / FIXED_STEP_SECONDS);
  if (ticks > 0) {
    scene.advance_ticks(ticks, 0.55);
    tickAccumulator -= ticks * FIXED_STEP_SECONDS;
  }
  renderFrame();
  frameHandle = requestAnimationFrame(animate);
}

function startLoop() {
  stopLoop();
  if (!scene || paused || document.hidden || reducedMotion.matches) return;
  // Render one deterministic fixed step promptly, then follow requestAnimationFrame.
  tickAccumulator = FIXED_STEP_SECONDS;
  frameHandle = requestAnimationFrame(animate);
}

function makeScene() {
  stopLoop();
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
  const seed = (20261010 + generation) >>> 0;
  scene = VisualScene.createConfigured(
    width,
    height,
    seed,
    2048,             // branch limit
    10,               // max depth
    0.28,             // growth rate
    FIXED_STEP_HZ,
    7.5,              // pulse period in seconds
    0.12,             // deterministic drift amplitude
    128,              // conservative renderer memory budget, MiB
    PALETTE_RGB,
  );

  if (reducedMotion.matches) {
    advanceStaticFrame();
    status.textContent = "Reduced motion is enabled; showing one configured static frame.";
  } else if (paused) {
    renderFrame();
    status.textContent = "Animation paused.";
  } else {
    renderFrame();
    status.textContent = "Configured scene running locally in WebAssembly at a bounded 30 FPS.";
    startLoop();
  }

  updateControls();
}

pauseButton.addEventListener("click", () => {
  if (!scene || reducedMotion.matches) return;
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

regenerateButton.addEventListener("click", () => {
  generation += 1;
  makeScene();
  status.textContent = `Created deterministic scene ${generation + 1}.`;
});

document.addEventListener("visibilitychange", () => {
  if (document.hidden) {
    stopLoop();
  } else if (!reducedMotion.matches && !paused) {
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
  makeScene();
} catch (error) {
  console.error("Sovereign Visual Core failed to initialize:", error);
  status.textContent =
    "WebAssembly failed to load. Build the WASM demo package and serve this directory over HTTP.";
  pauseButton.disabled = true;
  regenerateButton.disabled = true;
}
