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
      default = "/dev/dri/card0";
      description = "DRM/KMS card device used for the boot canary.";
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
      "d \${runtimeDir} 0755 root root -"
    ];

    systemd.services.sovereign-boot-animation = {
      description = "Sovereign Boot procedural DRM/KMS animation";
      wantedBy = [ "multi-user.target" ];
      before = [ "display-manager.service" ];
      unitConfig = {
        ConditionPathExists = "/dev/dri";
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
