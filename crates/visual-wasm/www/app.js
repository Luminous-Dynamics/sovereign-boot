import init, { VisualScene } from "./pkg/sovereign_visual_wasm.js";

const canvas = document.querySelector("#scene");
const context = canvas.getContext("2d", { alpha: false });
const status = document.querySelector("#status");
const pauseButton = document.querySelector("#pause");
const regenerateButton = document.querySelector("#regenerate");
const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");

let scene = null;
let imageData = null;
let generation = 0;
let paused = false;
let frameHandle = 0;
let lastFrameTime = 0;

function stopLoop() {
  if (frameHandle !== 0) {
    cancelAnimationFrame(frameHandle);
    frameHandle = 0;
  }
  lastFrameTime = 0;
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

function animate(now) {
  frameHandle = 0;
  if (!scene || paused || document.hidden || reducedMotion.matches) return;

  if (lastFrameTime !== 0 && now - lastFrameTime < 1000 / 30) {
    frameHandle = requestAnimationFrame(animate);
    return;
  }

  const delta = lastFrameTime === 0
    ? 1 / 30
    : Math.min((now - lastFrameTime) / 1000, 0.25);
  lastFrameTime = now;
  scene.advance(delta, 0.55);
  renderFrame();
  frameHandle = requestAnimationFrame(animate);
}

function startLoop() {
  stopLoop();
  if (!scene || paused || document.hidden || reducedMotion.matches) return;
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
  scene = new VisualScene(width, height, `sovereign-visual-demo-${generation}`);

  if (reducedMotion.matches) {
    // Move to a fully visible scene, then present a single static frame.
    for (let i = 0; i < 40; i += 1) {
      scene.advance(1 / 30, 0.55);
    }
    renderFrame();
    status.textContent = "Reduced motion is enabled; showing one static frame.";
  } else if (paused) {
    renderFrame();
    status.textContent = "Animation paused.";
  } else {
    renderFrame();
    status.textContent = "Rendering locally in WebAssembly at a bounded 30 FPS.";
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
