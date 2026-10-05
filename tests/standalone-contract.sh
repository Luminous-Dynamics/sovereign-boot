#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

grep -q '"crates/quicken-fb"' Cargo.toml
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

echo "sovereign-boot standalone contract: PASS"

# Nix must package from the repository root so the root Cargo.lock is present,
# while compiling/testing only the quicken-fb workspace member.
grep -q 'src = ./\.;' flake.nix
grep -q 'buildAndTestSubdir = "crates/quicken-fb";' flake.nix
grep -q 'cargoLock.lockFile = ./Cargo.lock;' flake.nix

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
grep -q 'SOVEREIGN_BOOT_ARTIFACT' flake.nix
grep -q -- '--isolate' scripts/launch-physical-canary.sh
grep -q 'EUID -ne 0' scripts/launch-physical-canary.sh
! grep -q '^[[:space:]]*sudo[[:space:]]*grep -q 'systemctl isolate graphical.target' scripts/launch-physical-canary.sh
grep -q 'trap cleanup EXIT' scripts/launch-physical-canary.sh
bash -n scripts/launch-physical-canary.sh

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
grep -q 'sovereign-boot-boundary' flake.nix
grep -q 'runNixOSTest' flake.nix
grep -q 'nix flake check --no-update-lock-file --no-write-lock-file' .github/workflows/ci.yml
grep -q 'nix build .#quicken-fb --no-update-lock-file --no-write-lock-file' .github/workflows/ci.yml

# The service must be isolated from the desktop handoff and cannot become a
# required target dependency.
grep -q 'Conflicts = \[ "display-manager.service" \];' nix/modules/sovereign-boot.nix
grep -q 'ProtectSystem = "strict";' nix/modules/sovereign-boot.nix

echo "sovereign-boot restoration/buffer contract: PASS"
 flake.nix
grep -q 'systemctl isolate multi-user.target' scripts/launch-physical-canary.sh
grep -q 'systemctl isolate graphical.target' scripts/launch-physical-canary.sh
grep -q 'trap cleanup EXIT' scripts/launch-physical-canary.sh
bash -n scripts/launch-physical-canary.sh

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
grep -q 'sovereign-boot-boundary' flake.nix
grep -q 'runNixOSTest' flake.nix
grep -q 'nix flake check --no-update-lock-file --no-write-lock-file' .github/workflows/ci.yml
grep -q 'nix build .#quicken-fb --no-update-lock-file --no-write-lock-file' .github/workflows/ci.yml

# The service must be isolated from the desktop handoff and cannot become a
# required target dependency.
grep -q 'Conflicts = \[ "display-manager.service" \];' nix/modules/sovereign-boot.nix
grep -q 'ProtectSystem = "strict";' nix/modules/sovereign-boot.nix

echo "sovereign-boot restoration/buffer contract: PASS"
