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
grep -q 'probe.connector_interface' crates/quicken-fb/src/main.rs

# Multi-GPU hosts require an explicit DRM card; probe errors must include connector diagnostics.
! grep -q 'default = "/dev/dri/card0"' nix/modules/sovereign-boot.nix
grep -q 'NoConnectedDisplay' crates/quicken-fb/src/framebuffer.rs
grep -q 'connector_diagnostics' crates/quicken-fb/src/framebuffer.rs
