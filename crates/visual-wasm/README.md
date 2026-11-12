# Sovereign Visual Core — browser demo

This small demo renders the shared Rust scene in a browser canvas through
`wasm32-unknown-unknown` and `wasm-bindgen`. The host page owns the canvas,
frame clock, reduced-motion preference, page visibility and display updates.

## Build and run

Install the target and a CLI version that matches the pinned Rust crate:

```text
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.108 --locked
bash crates/visual-wasm/build-demo.sh
nix run nixpkgs#miniserve -- --port 8080 crates/visual-wasm/www
```

Open `http://localhost:8080`. The generated `www/pkg/` directory is ignored by
Git; the committed files are the host page, JavaScript lifecycle adapter, and
build instructions.

## Behavior and limits

- The scene is locally rendered; no network account or server is needed by the
  visual core after the page assets load.
- Frame dimensions are bounded in the Rust API; the demo uses at most 960px wide.
- Rendering is capped at 30 FPS and stops when the page is hidden.
- The page respects `prefers-reduced-motion` by producing a single static frame.
- The host copies RGBA data to a 2D canvas; it does not use DRM/KMS or change the
  system wallpaper.
- This demonstrates browser runtime behavior, not compatibility with every browser,
  a desktop wallpaper extension, mobile live wallpaper, TV screensaver, boot
  splash, lock screen, or firmware UI.

The CI workflow tests the generated bindings and deterministic frame output under
Node's WebAssembly host. Real-browser tests and per-OS/device integrations remain
separate qualification gates.
