#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runner="$root/scripts/physical-canary.sh"

command -v openvt >/dev/null 2>&1 || {
  echo "ERROR: openvt is required (util-linux)" >&2
  exit 1
}

[[ -x "$runner" ]] || {
  echo "ERROR: physical-canary.sh is missing or not executable: $runner" >&2
  exit 1
}

exec sudo openvt --switch --wait -- "$runner" "$@"
