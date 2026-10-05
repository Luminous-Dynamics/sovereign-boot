#!/usr/bin/env bash
set -euo pipefail

request="/var/lib/sovereign-boot/physical-canary.request"
device=""
seconds=5

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
    -h|--help)
      echo "Usage: arm-physical-canary.sh --device /dev/dri/cardN [--seconds N]"
      echo
      echo "Probe the selected DRM device, arm a one-shot boot-time canary, and reboot."
      echo "The current graphical session is never isolated or stopped."
      exit 0
      ;;
    *)
      echo "ERROR: unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

if [[ $EUID -ne 0 ]]; then
  echo "ERROR: arming the boot-time canary requires root privileges." >&2
  exit 1
fi

[[ "$seconds" =~ ^[0-9]+$ ]] && ((seconds >= 1 && seconds <= 30)) || {
  echo "ERROR: --seconds must be an integer from 1 to 30" >&2
  exit 2
}

[[ "$device" =~ ^/dev/dri/card[0-9]+$ ]] || {
  echo "ERROR: --device must be an explicit /dev/dri/cardN path" >&2
  exit 2
}

systemctl is-enabled --quiet sovereign-boot-physical-canary.service || {
  echo "ERROR: sovereign-boot-physical-canary.service is not enabled in the running NixOS configuration." >&2
  echo "Enable the Sovereign Boot NixOS module before arming a physical boot canary." >&2
  exit 1
}

systemctl is-active --quiet graphical.target || {
  echo "ERROR: graphical.target is not active; refusing to arm from an unexpected boot state." >&2
  exit 1
}

systemctl is-active --quiet display-manager.service || {
  echo "ERROR: display-manager.service is not active; refusing to arm outside a live graphical session." >&2
  exit 1
}

[[ -e "$device" ]] || {
  echo "ERROR: DRM device does not exist: $device" >&2
  exit 1
}

artifact="\${SOVEREIGN_BOOT_ARTIFACT:?SOVEREIGN_BOOT_ARTIFACT is required}"

echo "=== Sovereign Boot boot-scoped physical canary ==="
echo "Probing $device before arming; this probe is non-mutating."

probe_output="$("$artifact" --probe --device "$device" 2>&1)" || {
  echo "REFUSING: non-mutating DRM probe failed:" >&2
  echo "$probe_output" >&2
  exit 1
}
printf "%s\n" "$probe_output"

install -d -m 0755 /var/lib/sovereign-boot
tmp="$(mktemp /var/lib/sovereign-boot/physical-canary.request.XXXXXX)"
trap 'rm -f "$tmp"' EXIT
chmod 0600 "$tmp"
artifact_sha256="$(sha256sum "$artifact" | cut -d' ' -f1)"
printf 'device=%s\nseconds=%s\nartifact_sha256=%s\n' "$device" "$seconds" "$artifact_sha256" >"$tmp"
mv -f "$tmp" "$request"
sync

echo
echo "ARMED: physical canary will run on the next boot before the display manager."
echo "The current KDE session is left untouched until reboot."
echo "Rebooting..."
exec systemctl reboot
