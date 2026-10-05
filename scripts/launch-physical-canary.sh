#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
artifact="${SOVEREIGN_BOOT_ARTIFACT:-$root/result/bin/quicken-fb}"
seconds=5
device=""
isolate=0

while (($#)); do
  case "$1" in
    --device)
      [[ $# -ge 2 ]] || { echo "ERROR: --device needs a path" >&2; exit 2; }
      device="$2"
      shift 2
      ;;
    --seconds)
      [[ $# -ge 2 ]] || { echo "ERROR: --seconds needs a value" >&2; exit 2; }
      seconds="$2"
      shift 2
      ;;
    --isolate)
      isolate=1
      shift
      ;;
    -h|--help)
      echo "Usage: launch-physical-canary.sh [--device /dev/dri/cardN] [--seconds N] [--isolate]"
      echo
      echo "--isolate: allocate a real VT first, isolate multi-user.target, run the"
      echo "           bounded canary, then attempt graphical.target recovery."
      exit 0
      ;;
    *)
      echo "ERROR: unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

if [[ $EUID -ne 0 ]]; then
  echo "ERROR: this physical canary must be launched with root privileges." >&2
  echo "Elevate the outer nix run command; do not elevate a package binary from /nix/store." >&2
  exit 1
fi

[[ "$seconds" =~ ^[0-9]+$ ]] && ((seconds >= 1 && seconds <= 30)) || {
  echo "ERROR: --seconds must be an integer from 1 to 30" >&2
  exit 2
}

[[ -x "$artifact" ]] || {
  echo "ERROR: renderer not found: $artifact" >&2
  exit 1
}

command -v openvt >/dev/null 2>&1 || {
  echo "ERROR: openvt is required (util-linux)" >&2
  exit 1
}

inner='set -euo pipefail
artifact="$1"
seconds="$2"
device="$3"
isolate="$4"
did_isolate=0

cleanup() {
  rc=$?
  if ((did_isolate)); then
    echo
    echo "Recovering graphical.target..."
    if ! systemctl isolate graphical.target; then
      echo "WARNING: graphical.target recovery failed; use the recovery shell." >&2
      rc=1
    elif ! systemctl is-active --quiet graphical.target; then
      echo "WARNING: graphical.target is not active after recovery." >&2
      rc=1
    elif ! systemctl is-active --quiet display-manager.service; then
      echo "WARNING: display-manager.service is not active after recovery." >&2
      rc=1
    else
      echo "Graphical recovery: PASS"
    fi
  fi
  exit "$rc"
}
trap cleanup EXIT

if [[ "$isolate" == "1" ]]; then
  if systemctl is-active --quiet graphical.target; then
    echo "Opt-in isolation requested; this VT was acquired before graphical.target isolation."
    did_isolate=1
    systemctl isolate multi-user.target
  else
    echo "REFUSING: --isolate requires graphical.target to be active" >&2
    exit 1
  fi
fi

if systemctl is-active --quiet display-manager.service; then
  echo "REFUSING: display-manager.service is active" >&2
  echo "Use --isolate for an explicit automated graphical handoff." >&2
  exit 1
fi

active_vt="$(cat /sys/class/tty/tty0/active 2>/dev/null || true)"
stdin_tty="$(readlink /proc/self/fd/0 || true)"
[[ "$stdin_tty" == "/dev/$active_vt" ]] || {
  echo "REFUSING: launcher stdin is not the active VT (stdin=$stdin_tty active=$active_vt)" >&2
  exit 1
}

if [[ -z "$device" ]]; then
  success_count=0
  success_device=""
  found_any=0
  for card in /dev/dri/card[0-9]*; do
    [[ -e "$card" ]] || continue
    found_any=1
    if output="$("$artifact" --probe --device "$card" 2>&1)"; then
      echo "  PASS  $card"
      echo "        $output"
      success_count=$((success_count + 1))
      success_device="$card"
    else
      echo "  skip  $card"
      echo "        $output" >&2
    fi
  done
  ((found_any)) || {
    echo "ERROR: no /dev/dri/cardN devices found" >&2
    exit 1
  }
  ((success_count == 1)) || {
    echo "REFUSING: auto-selection requires exactly one probe-successful DRM card; found $success_count." >&2
    echo "Use --device /dev/dri/cardN explicitly." >&2
    exit 1
  }
  device="$success_device"
fi

[[ "$device" == /dev/dri/card[0-9]* ]] || {
  echo "ERROR: device must be /dev/dri/cardN: $device" >&2
  exit 2
}
[[ -e "$device" ]] || {
  echo "ERROR: DRM device does not exist: $device" >&2
  exit 1
}

echo
echo "=== Sovereign Boot physical canary ==="
echo "Device:  $device"
echo "Seconds: $seconds"
echo "Binary:  $artifact"
echo

probe_output="$("$artifact" --probe --device "$device" 2>&1)" || {
  echo "REFUSING: final non-mutating probe failed:" >&2
  echo "$probe_output" >&2
  exit 1
}
printf "%s\n" "$probe_output"

echo
echo "Starting bounded renderer canary..."
set +e
canary_output="$("$artifact" --genesis-phrase "Sovereign Boot" --device "$device" --canary-seconds "$seconds" 2>&1)"
canary_rc=$?
set -e
printf "%s\n" "$canary_output"

((canary_rc == 0)) || {
  echo "CANARY RESULT: FAIL (renderer exit code $canary_rc)" >&2
  exit "$canary_rc"
}

grep -q "^drm-restore-ok" <<<"$canary_output" || {
  echo "CANARY RESULT: FAIL (no explicit drm-restore-ok receipt)" >&2
  exit 1
}

echo
echo "CANARY RESULT: PASS"
'

exec openvt --switch --wait -- bash -c "$inner" _ "$artifact" "$seconds" "$device" "$isolate"
