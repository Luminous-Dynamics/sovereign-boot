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

  physicalCanaryRunner = pkgs.writeShellScript "sovereign-boot-physical-canary-runner" ''
    set -euo pipefail

    request=${lib.escapeShellArg canaryRequest}
    result=${lib.escapeShellArg canaryResult}
    artifact="${cfg.package}/bin/quicken-fb"

    if [[ ! -f "$request" ]]; then
      echo "sovereign-boot: no physical canary request; skipping"
      exit 0
    fi

    device="$(${pkgs.gnused}/bin/sed -n 's/^device=//p' "$request")"
    seconds="$(${pkgs.gnused}/bin/sed -n 's/^seconds=//p' "$request")"

    if [[ "$device" != /dev/dri/card[0-9]* ]] || [[ ! -e "$device" ]]; then
      echo "sovereign-boot: invalid physical canary device request: $device" >&2
      printf 'status=FAIL_INVALID_REQUEST\nboot_id=%s\n' "$(< /proc/sys/kernel/random/boot_id)" >"$result"
      rm -f "$request"
      exit 2
    fi

    if [[ ! "$seconds" =~ ^[0-9]+$ ]] || (( seconds < 1 || seconds > 30 )); then
      echo "sovereign-boot: invalid physical canary duration: $seconds" >&2
      printf 'status=FAIL_INVALID_REQUEST\nboot_id=%s\n' "$(< /proc/sys/kernel/random/boot_id)" >"$result"
      rm -f "$request"
      exit 2
    fi

    active_vt="$(${pkgs.coreutils}/bin/cat /sys/class/tty/tty0/active 2>/dev/null || true)"
    if [[ "$active_vt" != "tty1" ]]; then
      echo "sovereign-boot: refusing physical canary because tty1 is not active (active=$active_vt)" >&2
      printf 'status=FAIL_WRONG_VT\nboot_id=%s\nactive_vt=%s\n' "$(< /proc/sys/kernel/random/boot_id)" "$active_vt" >"$result"
      rm -f "$request"
      exit 3
    fi

    echo "Sovereign Boot: boot-scoped physical canary armed for this boot."
    echo "Sovereign Boot: device=$device seconds=$seconds"
    echo "Sovereign Boot: display-manager.service is intentionally not started yet."

    set +e
    "$artifact" --genesis-phrase "${lib.escapeShellArg cfg.genesisPhrase}" \
      --device "$device" --canary-seconds "$seconds"
    rc=$?
    set -e

    if (( rc == 0 )); then
      status="PASS"
    else
      status="FAIL_RENDERER"
    fi

    printf 'status=%s\nboot_id=%s\ndevice=%s\nseconds=%s\nexit_code=%s\n' \
      "$status" "$(< /proc/sys/kernel/random/boot_id)" "$device" "$seconds" "$rc" >"$result"
    rm -f "$request"

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
        assertion = lib.hasPrefix "/dev/dri/card" cfg.drmDevice;
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
        DeviceAllow = "${cfg.drmDevice} rw";
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
