#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

grep -q '"crates/quicken-fb"' Cargo.toml
! grep -q '"crates/spore-kernel"' Cargo.toml

! grep -q 'workspace = true' crates/quicken-fb/Cargo.toml
test -s Cargo.lock
test -s rust-toolchain.toml

grep -q -- '--genesis-phrase' crates/quicken-fb/src/main.rs
grep -q -- '--device' crates/quicken-fb/src/main.rs
grep -q -- '--probe' crates/quicken-fb/src/main.rs

! grep -q -- '--receipt' nix/modules/sovereign-boot.nix
! grep -Eq 'spore-recovery-linux|spore-boot-state|spore-boot-tools.nix' nix/modules/sovereign-boot.nix flake.nix
! grep -q '\.\./\.\./\.\./' flake.nix nix/modules/sovereign-boot.nix

echo "sovereign-boot standalone contract: PASS"
