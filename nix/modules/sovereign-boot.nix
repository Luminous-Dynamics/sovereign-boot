# Sovereign Boot renderer host integration.
#
# Safety rule: Sovereign Boot is decorative. It may observe and render during
# boot, but it must never become a boot dependency or authority boundary.
#
# The state/recovery/LKG helpers from the original monorepo extraction are not
# wired here until their standalone binaries and evidence gates are present.
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.luminous.services.sovereignBoot;
  runtimeDir = "/run/sovereign-boot";

  compositorHandoff = pkgs.writeShellScript "sovereign-boot-compositor-handoff" ''
    set -u
    \${pkgs.coreutils}/bin/mkdir -p \${lib.escapeShellArg runtimeDir}
    \${pkgs.coreutils}/bin/touch \${lib.escapeShellArg (runtimeDir + "/compositor-handoff")}
    \${pkgs.systemd}/bin/systemctl stop sovereign-boot-animation.service >/dev/null 2>&1 || true
    exit 0
  '';

  rendererMayStart = pkgs.writeShellScript "sovereign-boot-renderer-may-start" ''
    # A live nixos-rebuild test/switch must never steal DRM from an active
    # desktop session. systemd treats exit 1 from ExecCondition as a clean skip.
    if \${pkgs.systemd}/bin/systemctl is-active --quiet display-manager.service; then
      exit 1
    fi
    exit 0
  '';


  canaryRequestDir = "/var/lib/sovereign-boot";
  canaryRequest = "${canaryRequestDir}/physical-canary.request";
  canaryResult = "${canaryRequestDir}/physical-canary.result";
  canaryProbeReceipt = "${canaryRequestDir}/physical-canary.probe";
  canaryOutput = "${canaryRequestDir}/physical-canary.output";

  physicalCanaryRunner = pkgs.writeShellScript "sovereign-boot-physical-canary-runner" ''
    set -euo pipefail

    request=${lib.escapeShellArg canaryRequest}
    inflight="$request.inflight"
    result=${lib.escapeShellArg canaryResult}
    canaryProbeReceipt=${lib.escapeShellArg canaryProbeReceipt}
    canaryOutput=${lib.escapeShellArg canaryOutput}
    artifact="${cfg.package}/bin/quicken-fb"

    write_atomic() {
      local target="$1"
      local tmp
      tmp="$(mktemp "$target.tmp.XXXXXX")"
      chmod 0600 "$tmp"
      cat >"$tmp"
      mv -f "$tmp" "$target"
    }

    if [[ ! -f "$request" ]]; then
      echo "sovereign-boot: no physical canary request; skipping"
      exit 0
    fi

    # Consume the request before renderer execution so an interrupted boot
    # cannot silently schedule the same destructive modeset on every reboot.
    mv -f "$request" "$inflight"
    request_active=1
    cleanup_request() {
      if ((request_active)); then
        rm -f "$inflight"
        request_active=0
      fi
    }
    trap cleanup_request EXIT

    device="$(${pkgs.gnused}/bin/sed -n 's/^device=//p' "$inflight")"
    seconds="$(${pkgs.gnused}/bin/sed -n 's/^seconds=//p' "$inflight")"
    requested_sha="$(${pkgs.gnused}/bin/sed -n 's/^artifact_sha256=//p' "$inflight")"
    preboot_probe_sha="$(${pkgs.gnused}/bin/sed -n 's/^preboot_probe_sha256=//p' "$inflight")"
    armed_at_unix_s="$(${pkgs.gnused}/bin/sed -n 's/^armed_at_unix_s=//p' "$inflight")"
    actual_sha="$(${pkgs.coreutils}/bin/sha256sum "$artifact" | ${pkgs.coreutils}/bin/cut -d' ' -f1)"

    if [[ "$device" != "${cfg.drmDevice}" ]]; then
      echo "sovereign-boot: request device does not match configured DRM device: $device" >&2
      printf 'status=FAIL_DEVICE_MISMATCH\nboot_id=%s\ndevice=%s\narmed_at_unix_s=%s\ndevice=%s\n' "$(< /proc/sys/kernel/random/boot_id)" "$armed_at_unix_s" "$device" | write_atomic "$result"
      request_active=0
      rm -f "$inflight"
      exit 2
    fi

    if [[ ! "$requested_sha" =~ ^[0-9a-f]{64}$ ]] || [[ "$requested_sha" != "$actual_sha" ]]; then
      echo "sovereign-boot: renderer artifact digest mismatch" >&2
      printf 'status=FAIL_ARTIFACT_MISMATCH\nboot_id=%s\narmed_at_unix_s=%s\nexpected_sha=%s\nactual_sha=%s\n' "$(< /proc/sys/kernel/random/boot_id)" "$armed_at_unix_s" "$requested_sha" "$actual_sha" | write_atomic "$result"
      request_active=0
      rm -f "$inflight"
      exit 3
    fi

    if [[ ! "$armed_at_unix_s" =~ ^[0-9]+$ ]]; then
      echo "sovereign-boot: invalid canary arm timestamp" >&2
      printf 'status=FAIL_INVALID_REQUEST\nboot_id=%s\n' "$(< /proc/sys/kernel/random/boot_id)" | write_atomic "$result"
      request_active=0
      rm -f "$inflight"
      exit 2
    fi

    if [[ "$device" != /dev/dri/card[0-9]* ]] || [[ ! -e "$device" ]]; then
      echo "sovereign-boot: invalid physical canary device request: $device" >&2
      printf 'status=FAIL_INVALID_REQUEST\nboot_id=%s\n' "$(< /proc/sys/kernel/random/boot_id)" | write_atomic "$result"
      request_active=0
      rm -f "$inflight"
      exit 2
    fi

    if [[ ! "$seconds" =~ ^[0-9]+$ ]] || (( seconds < 1 || seconds > 30 )); then
      echo "sovereign-boot: invalid physical canary duration: $seconds" >&2
      printf 'status=FAIL_INVALID_REQUEST\nboot_id=%s\n' "$(< /proc/sys/kernel/random/boot_id)" | write_atomic "$result"
      request_active=0
      rm -f "$inflight"
      exit 2
    fi

    if ${pkgs.systemd}/bin/systemctl is-active --quiet display-manager.service; then
      echo "sovereign-boot: display manager active; refusing canary" >&2
      printf 'status=FAIL_DISPLAY_MANAGER\nboot_id=%s\n' "$(< /proc/sys/kernel/random/boot_id)" | write_atomic "$result"
      request_active=0
      rm -f "$inflight"
      exit 4
    fi

    probe_output="$("$artifact" --probe --device "$device" 2>&1)" || {
      echo "sovereign-boot: boot-time non-mutating DRM probe failed" >&2
      printf 'status=FAIL_PROBE\nboot_id=%s\ndevice=%s\n' "$(< /proc/sys/kernel/random/boot_id)" "$device" | write_atomic "$result"
      request_active=0
      rm -f "$inflight"
      exit 5
    }
    case "$probe_output" in
      drm-ok\ *) ;;
      *)
        echo "sovereign-boot: boot-time DRM probe returned no valid receipt" >&2
        printf "%s\n" "$probe_output" | write_atomic "$canaryProbeReceipt"
        probe_sha="$(sha256sum "$canaryProbeReceipt" | cut -d' ' -f1)"
        printf 'status=FAIL_PROBE_RECEIPT\nboot_id=%s\ndevice=%s\npreboot_probe_sha256=%s\nboot_probe_sha256=%s\n' "$(< /proc/sys/kernel/random/boot_id)" "$device" "$preboot_probe_sha" "$probe_sha" | write_atomic "$result"
        request_active=0
        rm -f "$inflight"
        exit 6
        ;;
    esac
    printf "%s\n" "$probe_output" | write_atomic "$canaryProbeReceipt"
    probe_sha="$(sha256sum "$canaryProbeReceipt" | cut -d' ' -f1)"
    echo "Sovereign Boot: boot-time probe: $probe_output"

    active_vt="$(${pkgs.coreutils}/bin/cat /sys/class/tty/tty0/active 2>/dev/null || true)"
    if [[ "$active_vt" != "tty1" ]]; then
      echo "sovereign-boot: refusing physical canary because tty1 is not active (active=$active_vt)" >&2
      printf 'status=FAIL_WRONG_VT\nboot_id=%s\nactive_vt=%s\n' "$(< /proc/sys/kernel/random/boot_id)" "$active_vt" | write_atomic "$result"
      request_active=0
      rm -f "$inflight"
      exit 3
    fi

    echo "Sovereign Boot: boot-scoped physical canary armed for this boot."
    echo "Sovereign Boot: device=$device seconds=$seconds"
    echo "Sovereign Boot: display-manager.service is intentionally not started yet."

    set +e
    canary_output="$("$artifact" --genesis-phrase "${lib.escapeShellArg cfg.genesisPhrase}" \
      --device "$device" --canary-seconds "$seconds" 2>&1)"
    rc=$?
    set -e
    printf "%s\n" "$canary_output" | write_atomic "$canaryOutput"
    printf "%s\n" "$canary_output"

    output_sha="$(sha256sum "$canaryOutput" | cut -d' ' -f1)"
    restore_receipt="$(${pkgs.gnugrep}/bin/grep -m1 "^drm-restore-ok " <<<"$canary_output" || true)"
    if (( rc == 0 )) && [[ -n "$restore_receipt" ]]; then
      status="PASS"
    elif (( rc == 0 )); then
      status="FAIL_RESTORE_RECEIPT"
      rc=7
    else
      status="FAIL_RENDERER"
    fi

    restore_sha=""
    if [[ -n "$restore_receipt" ]]; then
      restore_sha="$(printf '%s\n' "$restore_receipt" | sha256sum | cut -d' ' -f1)"
    fi
    printf 'status=%s\nboot_id=%s\narmed_at_unix_s=%s\ndevice=%s\nseconds=%s\nexit_code=%s\nartifact_sha256=%s\npreboot_probe_sha256=%s\nboot_probe_sha256=%s\nrenderer_output_sha256=%s\nrestore_receipt_sha256=%s\n' \
      "$status" "$(< /proc/sys/kernel/random/boot_id)" "$armed_at_unix_s" "$device" "$seconds" "$rc" "$actual_sha" "$preboot_probe_sha" "$probe_sha" "$output_sha" "$restore_sha" | write_atomic "$result"
    request_active=0
    rm -f "$inflight"

    if (( rc == 0 )); then
      echo "Sovereign Boot: boot-scoped physical canary PASS"
    else
      echo "Sovereign Boot: boot-scoped physical canary FAIL (exit=$rc)" >&2
    fi

    exit "$rc"
  '';

  progressArg =
    lib.optionalString (cfg.progressPipe != null)
      " --progress-pipe \${lib.escapeShellArg cfg.progressPipe}";
in
{
  options.luminous.services.sovereignBoot = {
    enable = lib.mkEnableOption "fail-open Sovereign Boot DRM/KMS renderer";

    package = lib.mkOption {
      type = lib.types.package;
      description = ''
        The sovereign-boot package containing bin/quicken-fb. Bind this to the
        exact flake input revision used by the host configuration.
      '';
    };

    genesisPhrase = lib.mkOption {
      type = lib.types.str;
      default = "Sovereign Boot";
      description = "Deterministic visual seed for the boot animation.";
    };

    drmDevice = lib.mkOption {
      type = lib.types.str;
      description = ''
        Explicit DRM/KMS card device used for the boot canary. No implicit
        card0 default is provided because multi-GPU systems may expose the
        connected display on another DRM card.
      '';
    };

    progressPipe = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Optional named pipe receiving installer progress events.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = builtins.match "^/dev/dri/card[0-9]+$" cfg.drmDevice != null;
        message = "Sovereign Boot drmDevice must be an explicit /dev/dri/cardN path";
      }
    ];


    systemd.tmpfiles.rules = [
      "d ${runtimeDir} 0755 root root -",
      "d ${canaryRequestDir} 0755 root root -"
    ];

    systemd.services.sovereign-boot-physical-canary = {
      description = "Sovereign Boot one-shot physical DRM/KMS canary";
      wantedBy = [ "multi-user.target" ];
      before = [
        "display-manager.service"
        "getty@tty1.service"
        "sovereign-boot-animation.service"
      ];
      unitConfig = {
        ConditionPathExists = canaryRequest;
        Conflicts = [
          "display-manager.service"
          "getty@tty1.service"
          "sovereign-boot-animation.service"
        ];
      };
      serviceConfig = {
        Type = "oneshot";
        ExecStart = physicalCanaryRunner;
        StandardInput = "tty";
        StandardOutput = "journal";
        StandardError = "journal";
        TTYPath = "/dev/tty1";
        TTYReset = true;
        TTYVHangup = true;
        TTYVTDisallocate = false;
        User = "root";
        SupplementaryGroups = [ "video" "render" ];
        TimeoutStartSec = "35s";
        NoNewPrivileges = true;
        ProtectHome = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictNamespaces = true;
        LockPersonality = true;
        PrivateTmp = true;
        ProtectSystem = "strict";
        RestrictSUIDSGID = true;
        RestrictRealtime = true;
        CapabilityBoundingSet = "";
        UMask = "0077";
        DevicePolicy = "strict";
        DeviceAllow = [
          "${cfg.drmDevice} rw"
          "/dev/tty1 rw"
          "/dev/null rw"
          "/dev/zero r"
          "/dev/full r"
          "/dev/random r"
          "/dev/urandom r"
        ];
        ReadWritePaths = [ canaryRequestDir ];
      };
    };


    systemd.services.sovereign-boot-animation = {
      description = "Sovereign Boot procedural DRM/KMS animation";
      wantedBy = [ "multi-user.target" ];
      before = [ "display-manager.service" ];
      unitConfig = {
        ConditionPathExists = "/dev/dri";
        Conflicts = [ "display-manager.service" ];
      };
      serviceConfig = {
        Type = "simple";
        ExecCondition = rendererMayStart;
        ExecStart =
          "\${cfg.package}/bin/quicken-fb"
          + " --genesis-phrase \${lib.escapeShellArg cfg.genesisPhrase}"
          + " --device \${lib.escapeShellArg cfg.drmDevice}"
          + progressArg;
        User = "root";
        SupplementaryGroups = [ "video" "render" ];
        KillSignal = "SIGTERM";
        TimeoutStartSec = "3s";
        TimeoutStopSec = "750ms";
        Restart = "no";
        NoNewPrivileges = true;
        ProtectHome = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictNamespaces = true;
        LockPersonality = true;
        PrivateTmp = true;
        ProtectSystem = "strict";
        RestrictSUIDSGID = true;
        RestrictRealtime = true;
        CapabilityBoundingSet = "";
        DeviceAllow = "\${cfg.drmDevice} rw";
        ReadWritePaths = [ runtimeDir ];
      };
    };

    # The display manager is the explicit DRM owner handoff point. Existing
    # ExecStartPre hooks are preserved because mkBefore composes the list.
    systemd.services.display-manager.serviceConfig.ExecStartPre =
      lib.mkBefore [ compositorHandoff ];
  };
}
