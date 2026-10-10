#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
output_dir="$script_dir/www/pkg"

command -v cargo >/dev/null 2>&1 || {
  echo "ERROR: cargo is required." >&2
  exit 1
}
command -v wasm-bindgen >/dev/null 2>&1 || {
  echo "ERROR: install the matching CLI with: cargo install wasm-bindgen-cli --version 0.2.108 --locked" >&2
  exit 1
}

cd "$repo_root"
rustup target list --installed | grep -qx 'wasm32-unknown-unknown' || {
  echo "ERROR: install the target with: rustup target add wasm32-unknown-unknown" >&2
  exit 1
}

cargo build --locked --release -p sovereign-visual-wasm \
  --target wasm32-unknown-unknown --features web
mkdir -p "$output_dir"
wasm-bindgen --target web --out-dir "$output_dir" \
  "$repo_root/target/wasm32-unknown-unknown/release/sovereign_visual_wasm.wasm"
printf '{"type":"module"}\n' > "$output_dir/package.json"

echo "Browser demo package built at $output_dir"
