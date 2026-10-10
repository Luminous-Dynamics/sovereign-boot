#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

grep -q '"crates/quicken-fb"' Cargo.toml
grep -q '"crates/visual-core"' Cargo.toml
grep -q '"crates/visual-wasm"' Cargo.toml
grep -q '"crates/visual-wasi"' Cargo.toml
grep -q '"crates/visual-component"' Cargo.toml
test -s crates/visual-component/Cargo.toml
test -s crates/visual-component/src/lib.rs
test -s crates/visual-component/wit/visual.wit
grep -q 'wit-bindgen = "=0.57.1"' crates/visual-component/Cargo.toml
grep -q 'world visual-component' crates/visual-component/wit/visual.wit
grep -q 'export scene' crates/visual-component/wit/visual.wit
grep -q 'GuestVisualScene' crates/visual-component/src/lib.rs
test -s crates/visual-core/src/lib.rs
test -s crates/visual-core/src/mycelium.rs
test -s crates/visual-pack/Cargo.toml
test -s crates/visual-pack/src/lib.rs
test -s tests/fixtures/first-germination.scene.json
grep -q 'parse_scene_pack_v1' crates/visual-pack/src/lib.rs
grep -q 'duplicate JSON object key' crates/visual-pack/src/lib.rs
grep -q 'resource_max_branches' crates/visual-pack/src/lib.rs
grep -q 'AssetHashProvider' crates/visual-pack/src/lib.rs
grep -q 'crates/visual-pack' Cargo.toml
grep -q 'name = "sovereign-visual-pack"' Cargo.lock
test -s crates/visual-core/examples/rgba-fixture.rs
grep -q 'portable-smoke-fixture' crates/visual-core/examples/rgba-fixture.rs
test -s crates/visual-wasm/src/lib.rs
test -s tests/visual-wasm-smoke.mjs
test -s crates/visual-wasm/www/index.html
test -s crates/visual-wasm/www/app.js
test -s crates/visual-wasm/build-demo.sh
grep -q 'prefers-reduced-motion' crates/visual-wasm/www/app.js
grep -q 'visibilitychange' crates/visual-wasm/www/app.js
grep -q 'wasm-bindgen --target web' crates/visual-wasm/build-demo.sh
bash -n crates/visual-wasm/build-demo.sh
grep -q 'render_rgba' tests/visual-wasm-smoke.mjs
test -s crates/visual-wasi/src/main.rs
grep -q 'pub use sovereign_visual_core::{color, mycelium};' crates/quicken-fb/src/lib.rs
grep -q 'render_rgba' crates/visual-wasm/src/lib.rs
grep -q 'binary PPM' crates/visual-wasi/src/main.rs
! grep -q '"crates/spore-kernel"' Cargo.toml

! grep -q 'workspace = true' crates/quicken-fb/Cargo.toml
test -s Cargo.lock
test -s rust-toolchain.toml
test -s flake.lock
grep -q '"a7868a727837f3c09cee2ce0ca671c76b1589fed"' flake.lock
grep -q '"11707dc2f618dd54ca8739b309ec4fc024de578b"' flake.lock
grep -q '"da67096a3b9bf56a91d16901293e51ba5b49a27e"' flake.lock

grep -q -- '--genesis-phrase' crates/quicken-fb/src/main.rs
grep -q -- '--device' crates/quicken-fb/src/main.rs
grep -q -- '--probe' crates/quicken-fb/src/main.rs

! grep -q -- '--receipt' nix/modules/sovereign-boot.nix
! grep -Eq 'spore-recovery-linux|spore-boot-state|spore-boot-tools.nix' nix/modules/sovereign-boot.nix flake.nix
! grep -q '\.\./\.\./\.\./' flake.nix nix/modules/sovereign-boot.nix

# Nix must package from the repository root so the root Cargo.lock is present,
# while compiling/testing only the quicken-fb workspace member.
grep -q 'src = ./\.;' flake.nix
grep -q 'buildAndTestSubdir = "crates/quicken-fb";' flake.nix
grep -q 'cargoLock.lockFile = ./Cargo.lock;' flake.nix
grep -q 'Install WebAssembly targets' .github/workflows/ci.yml
grep -q 'wasm32-unknown-unknown --features web --release --locked' .github/workflows/ci.yml
grep -q 'cargo +1.96.0 install wasm-bindgen-cli --version 0.2.108 --locked' .github/workflows/ci.yml
grep -q 'wasm-bindgen --target web' .github/workflows/ci.yml
grep -q 'node tests/visual-wasm-smoke.mjs' .github/workflows/ci.yml
grep -q 'cargo +1.96.0 run --locked -p sovereign-visual-core --example rgba-fixture' .github/workflows/ci.yml
grep -q 'native.rgba' .github/workflows/ci.yml
grep -q 'native Rust and browser WASM must render identical RGBA bytes' tests/visual-wasm-smoke.mjs
grep -q 'wasm32-wasip2 --release --locked' .github/workflows/ci.yml
grep -q 'cargo +1.96.0 build -p sovereign-visual-component --target wasm32-wasip2 --release --locked' .github/workflows/ci.yml

# Probe must expose selected connector identity and CRTC in its receipt.
grep -q 'connector_interface' crates/quicken-fb/src/framebuffer.rs
grep -q 'connector_interface_id' crates/quicken-fb/src/framebuffer.rs
grep -q 'original_connectors' crates/quicken-fb/src/framebuffer.rs
grep -q 'pub fn restore' crates/quicken-fb/src/framebuffer.rs
grep -q 'DrmRestoreReceipt' crates/quicken-fb/src/framebuffer.rs
grep -q 'SourceBufferTooSmall' crates/quicken-fb/src/framebuffer.rs
grep -q 'probe.connector_interface' crates/quicken-fb/src/main.rs
grep -q 'probe.selection_source' crates/quicken-fb/src/main.rs
grep -q 'free-compatible' crates/quicken-fb/src/framebuffer.rs

# Multi-GPU hosts require an explicit DRM card; probe errors must include connector diagnostics.
! grep -q 'default = "/dev/dri/card0"' nix/modules/sovereign-boot.nix
grep -q 'NoConnectedDisplay' crates/quicken-fb/src/framebuffer.rs
grep -q 'connector_diagnostics' crates/quicken-fb/src/framebuffer.rs

# The executable must require explicit multi-GPU selection and expose a bounded canary.
grep -q -- '--canary-seconds' crates/quicken-fb/src/main.rs
grep -q -- '--device is required' crates/quicken-fb/src/main.rs
grep -q '1..=30' crates/quicken-fb/src/main.rs

# Operator launcher must allocate a real VT and preserve the renderer\'s ownership guards.
test -x scripts/launch-physical-canary.sh
grep -q 'openvt --switch --wait' scripts/launch-physical-canary.sh
grep -q 'display-manager.service' scripts/launch-physical-canary.sh
grep -q 'drm-restore-ok' scripts/launch-physical-canary.sh

grep -q 'openvt --switch --wait' README.md
grep -q 'physical-canary = pkgs.writeShellApplication' flake.nix
grep -q 'arm-physical-canary = pkgs.writeShellApplication' flake.nix
grep -q 'SOVEREIGN_BOOT_ARTIFACT' flake.nix
grep -q 'EUID -ne 0' scripts/launch-physical-canary.sh
test -x scripts/arm-physical-canary.sh
grep -q 'EUID -ne 0' scripts/arm-physical-canary.sh
! grep -q 'systemctl isolate' scripts/launch-physical-canary.sh
! grep -q -- '--isolate' scripts/launch-physical-canary.sh
grep -q 'openvt --switch --wait' scripts/launch-physical-canary.sh
grep -q 'systemctl reboot' scripts/arm-physical-canary.sh
grep -q 'physical-canary.request' scripts/arm-physical-canary.sh
grep -q 'artifact_sha256' scripts/arm-physical-canary.sh
grep -q 'artifact_sha256' nix/modules/sovereign-boot.nix
grep -q 'FAIL_ARTIFACT_MISMATCH' nix/modules/sovereign-boot.nix
grep -q 'FAIL_DEVICE_MISMATCH' nix/modules/sovereign-boot.nix
grep -q 'FAIL_RENDERER' nix/modules/sovereign-boot.nix
grep -q 'FAIL_PROBE' nix/modules/sovereign-boot.nix
grep -q 'FAIL_PROBE_RECEIPT' nix/modules/sovereign-boot.nix
grep -q 'FAIL_EXPIRED_REQUEST' nix/modules/sovereign-boot.nix
grep -q 'FAIL_SERVICE_TIMEOUT' nix/modules/sovereign-boot.nix
grep -q 'ExecStopPost = physicalCanaryPostStop;' nix/modules/sovereign-boot.nix
grep -q 'SERVICE_RESULT:-unknown' nix/modules/sovereign-boot.nix
grep -q 'expires_at_unix_s' scripts/arm-physical-canary.sh
grep -q 'restore_receipt=' nix/modules/sovereign-boot.nix
grep -q 'canary_output=' nix/modules/sovereign-boot.nix
grep -q 'FAIL_DISPLAY_MANAGER' nix/modules/sovereign-boot.nix
grep -q 'probe_output=' nix/modules/sovereign-boot.nix
grep -q 'DevicePolicy = "strict";' nix/modules/sovereign-boot.nix
grep -q '"/dev/tty1 rw"' nix/modules/sovereign-boot.nix
grep -q '/dev/urandom r' nix/modules/sovereign-boot.nix
grep -q '/dev/null rw' nix/modules/sovereign-boot.nix
grep -q 'request.inflight' nix/modules/sovereign-boot.nix
grep -q 'canaryArchiveDir' nix/modules/sovereign-boot.nix
grep -q 'canaryPrebootProbe' nix/modules/sovereign-boot.nix
grep -q 'preboot_probe_sha256' scripts/arm-physical-canary.sh
grep -q '\$archive_dir/\${request_id}.request' nix/modules/sovereign-boot.nix
grep -q '0700 root root' nix/modules/sovereign-boot.nix
bash -n scripts/launch-physical-canary.sh
bash -n scripts/arm-physical-canary.sh

# Bounded physical canary must have an explicit active-VT ownership guard.
test -s crates/quicken-fb/src/vt.rs
grep -q 'KDGETMODE' crates/quicken-fb/src/vt.rs
grep -q 'KD_GRAPHICS' crates/quicken-fb/src/vt.rs
grep -q 'tty0/active' crates/quicken-fb/src/vt.rs
grep -q 'SIGHUP' crates/quicken-fb/src/main.rs

grep -q 'display-manager.service' crates/quicken-fb/src/vt.rs
grep -q 'requires a real VT' crates/quicken-fb/src/vt.rs
grep -q 'KDSETMODE' crates/quicken-fb/src/vt.rs

grep -q 'tcgetpgrp' crates/quicken-fb/src/vt.rs
grep -q 'SIGQUIT' crates/quicken-fb/src/main.rs
grep -q 'SIGTSTP' crates/quicken-fb/src/main.rs

test -s tests/boot-boundary.nix
test -s tests/boot-scoped-canary.nix
grep -q 'sovereign-boot-boundary' flake.nix
grep -q 'sovereign-boot-boot-scoped-canary' flake.nix
grep -q 'runNixOSTest' flake.nix
grep -q 'nix flake check --no-update-lock-file --no-write-lock-file' .github/workflows/ci.yml
grep -q 'nix build .#quicken-fb --no-update-lock-file --no-write-lock-file' .github/workflows/ci.yml
grep -q 'nix build .#arm-physical-canary --no-update-lock-file --no-write-lock-file' .github/workflows/ci.yml

# The boot-scoped canary must order before the desktop without adding a
# destructive Conflicts= stop relationship.
grep -q 'StandardInput = "tty-fail";' nix/modules/sovereign-boot.nix
grep -Fq 'before = [' nix/modules/sovereign-boot.nix
! sed -n '/systemd.services.sovereign-boot-physical-canary = {/,/systemd.services.sovereign-boot-animation = {/p' nix/modules/sovereign-boot.nix | grep -q 'Conflicts'
grep -q 'ProtectSystem = "strict";' nix/modules/sovereign-boot.nix

echo "sovereign-boot renderer/WASM/WASI standalone contract: PASS"
