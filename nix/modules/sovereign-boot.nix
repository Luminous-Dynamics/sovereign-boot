# Spore Boot Ecology host integration.
#
# Safety rule: Spore may observe boot; Spore must never be required for boot.
# This module is deliberately disabled by default until the VM gates are green.
{
  config,
  pkgs,
  lib,
  inputs ? null,
  ...
}:

let
  cfg = config.luminous.services.sporeBoot;
  types = lib.types;

  # Prefer explicit package overrides (useful for tests and canaries). On the
  # real host, build the two boot binaries hermetically from the pinned Symthaea
  # flake source instead of falling back to /usr/local or another mutable path.
  inputSporeTools =
    if inputs != null && inputs ? symthaea then
      import (inputs.symthaea.outPath + "/nix/packages/spore-boot-tools.nix") {
        inherit pkgs;
        src = inputs.symthaea.outPath;
      }
    else
      null;
  renderer = if cfg.package != null then cfg.package else inputSporeTools;
  stateTool = if cfg.statePackage != null then cfg.statePackage else inputSporeTools;

  missingPackage = pkgs.runCommand "spore-boot-missing-package" { } ''
    mkdir -p $out/bin
  '';
  # Keep interpolation evaluable so the explicit assertions below produce the
  # useful error if somebody enables the module without declarative packages.
  rendererPkg = if renderer == null then missingPackage else renderer;
  statePkg = if stateTool == null then missingPackage else stateTool;
  stateDir = cfg.stateDirectory;
  runtimeDir = cfg.runtimeDirectory;
  rootsDir = "/nix/var/nix/gcroots/spore-boot";

  # Semantic rollback state must describe the generation that actually booted,
  # not merely a userspace generation activated later with nixos-rebuild
  # test/switch. /run/booted-system is created by stage 2 and remains stable
  # until reboot; /run/current-system may legitimately change during activation.
  bootedGeneration = pkgs.writeShellScript "spore-booted-generation" ''
    set -eu
    generation="$(${pkgs.coreutils}/bin/readlink -f /run/booted-system 2>/dev/null || true)"
    case "$generation" in
      /nix/store/*) printf '%s\n' "$generation" ;;
      *) exit 1 ;;
    esac
  '';

  # Replace a GC-root symlink with rename semantics instead of unlink/create.
  # A crash can therefore leave the old root or the new root, but not an
  # intentionally missing interval. `sync -f` makes the filesystem containing
  # the root durable before the caller proceeds. This is intentionally heavier
  # than a best-effort metadata flush: these writes happen only at lifecycle
  # boundaries and protect recovery provenance.
  atomicGcRoot = pkgs.writeShellScript "spore-atomic-gc-root" ''
    set -eu
    link="$1"
    target="$2"
    dir="$(${pkgs.coreutils}/bin/dirname "$link")"
    base="$(${pkgs.coreutils}/bin/basename "$link")"
    tmp="$dir/.''${base}.tmp.$$"
    cleanup() { ${pkgs.coreutils}/bin/rm -f "$tmp"; }
    trap cleanup EXIT INT TERM
    ${pkgs.coreutils}/bin/mkdir -p "$dir"
    ${pkgs.coreutils}/bin/ln -s "$target" "$tmp"
    ${pkgs.coreutils}/bin/mv -Tf "$tmp" "$link"
    ${pkgs.coreutils}/bin/sync -f "$dir"
    trap - EXIT INT TERM
  '';

  # Power loss can strand scratch symlinks created immediately before an
  # atomic rename. They are never semantic recovery roots, so remove only our
  # narrowly named scratch links before rotating Current/Previous.
  cleanupStaleRootScratch = pkgs.writeShellScript "spore-clean-stale-root-scratch" ''
    set -eu
    root=${lib.escapeShellArg rootsDir}
    ${pkgs.coreutils}/bin/mkdir -p "$root"
    ${pkgs.findutils}/bin/find "$root" -maxdepth 1 -type l \
      \( -name '.current.tmp.*' -o -name '.previous.tmp.*' -o -name '.last-known-good.pending.*' \) \
      -delete
  '';

  hardwareFingerprint = pkgs.writeShellScript "spore-hardware-fingerprint" ''
    set -eu
    {
      cat /sys/class/dmi/id/product_name 2>/dev/null || true
      printf 'cpus=%s\n' "$(${pkgs.coreutils}/bin/nproc 2>/dev/null || echo unknown)"
      for dev in /sys/bus/pci/devices/*; do
        [ -r "$dev/vendor" ] || continue
        [ -r "$dev/device" ] || continue
        printf '%s:%s\n' "$(cat "$dev/vendor")" "$(cat "$dev/device")"
      done | ${pkgs.coreutils}/bin/sort
    } | ${pkgs.coreutils}/bin/sha256sum | ${pkgs.coreutils}/bin/cut -d' ' -f1
  '';

  updateCurrentPreviousRoots = pkgs.writeShellScript "spore-update-current-previous-roots" ''
    set -eu
    root=${lib.escapeShellArg rootsDir}
    current="$(${bootedGeneration})"
    ${pkgs.coreutils}/bin/mkdir -p "$root"

    if [ -L "$root/current" ]; then
      old="$(${pkgs.coreutils}/bin/readlink -f "$root/current" 2>/dev/null || true)"
      # "Previous" means previous distinct *booted* build, not merely the
      # previous reboot or a live nixos-rebuild test/switch activation.
      if [ -n "$old" ] && [ "$old" != "$current" ]; then
        ${atomicGcRoot} "$root/previous" "$old"
      fi
    fi
    ${atomicGcRoot} "$root/current" "$current"
  '';

  prepareState = pkgs.writeShellScript "spore-prepare-boot-state" ''
    set -eu
    receipt=${lib.escapeShellArg (runtimeDir + "/boot-state.json")}
    lineage=${lib.escapeShellArg (runtimeDir + "/lineage.json")}

    ${cleanupStaleRootScratch}

    # A failed/restarted preparation must never leave an older or partial
    # receipt eligible for ConditionPathExists. The renderer is decorative: no
    # valid fresh receipt means it is skipped rather than fed ambiguous state.
    ${pkgs.coreutils}/bin/rm -f \
      "$receipt" \
      "$lineage" \
      ${lib.escapeShellArg (runtimeDir + "/compositor-handoff")} \
      ${lib.escapeShellArg (runtimeDir + "/bless-skipped-live-activation")} \
      ${lib.escapeShellArg (runtimeDir + "/lkg-promoted-generation")} \
      ${lib.escapeShellArg (runtimeDir + "/lkg-decision-v1.json")} \
      ${lib.escapeShellArg (runtimeDir + "/state-blessed")}

    boot_id="$(${pkgs.coreutils}/bin/cat /proc/sys/kernel/random/boot_id 2>/dev/null || echo "unknown")"
    generation="$(${bootedGeneration})"
    hardware="$(${hardwareFingerprint})"
    rc=0
    ${statePkg}/bin/spore-recovery-linux prepare \
      --roots-dir ${lib.escapeShellArg rootsDir} \
      --state-dir ${lib.escapeShellArg stateDir} \
      --runtime-dir ${lib.escapeShellArg runtimeDir} \
      --generation "$generation" \
      --boot-id "$boot_id" \
      --hardware-fingerprint "$hardware" \
      --storage-state unknown || rc=$?

    if [ "$rc" -ne 0 ] || [ ! -s "$receipt" ] || [ ! -s "$lineage" ]; then
      ${pkgs.coreutils}/bin/rm -f "$receipt" "$lineage"
      if [ "$rc" -ne 0 ]; then
        exit "$rc"
      fi
      exit 20
    fi
  '';

  # Last Known Good is promoted asynchronously and idempotently. Each invocation
  # samples systemd's current monotonic state exactly once: if graphical/display
  # health has not converged yet, the service exits successfully and the timer
  # retries later. The decorative renderer is never part of this predicate.
  promoteLkg = pkgs.writeShellScript "spore-promote-last-known-good" ''
    set -eu
    decision_path=${lib.escapeShellArg (runtimeDir + "/lkg-decision-v1.json")}
    receipt=${lib.escapeShellArg (runtimeDir + "/boot-state.json")}
    lineage=${lib.escapeShellArg (runtimeDir + "/lineage.json")}

    stop_timer() {
      ${pkgs.systemd}/bin/systemctl stop spore-boot-lkg-promote.timer >/dev/null 2>&1 || true
    }

    write_decision() {
      local decision="$1"
      local reason_code="$2"
      boot_id="$(${pkgs.coreutils}/bin/cat /proc/sys/kernel/random/boot_id 2>/dev/null || echo "unknown")"
      booted="$(${bootedGeneration} 2>/dev/null || true)"
      current="$(${pkgs.coreutils}/bin/readlink -f /run/current-system 2>/dev/null || true)"
      local json
      json="$(${pkgs.jq}/bin/jq -n \
        --arg schema "lkg-decision-v1" \
        --arg boot_id "$boot_id" \
        --arg booted "$booted" \
        --arg current "$current" \
        --arg decision "$decision" \
        --arg reason_code "$reason_code" \
        '{
          schema: $schema,
          boot_id: $boot_id,
          booted_generation: $booted,
          current_userspace_generation: $current,
          graphical_state: "inactive",
          display_manager_state: "inactive",
          display_manager_main_pid: 0,
          display_manager_active_enter_monotonic_usec: 0,
          required_stability_usec: 0,
          observed_stability_usec: 0,
          decision: $decision,
          reason_code: $reason_code,
          state_bless_result: 0,
          resulting_lkg_target: ""
        }')"
      ${pkgs.coreutils}/bin/mkdir -p ${lib.escapeShellArg runtimeDir}
      printf '%s\n' "$json" > "$decision_path" || true
    }

    if ${pkgs.systemd}/bin/systemctl is-failed --quiet spore-boot-state-prepare.service 2>/dev/null; then
      write_decision "skipped" "state-preparation-failed"
      stop_timer
      exit 0
    fi

    if [ ! -s "$receipt" ] || [ ! -s "$lineage" ]; then
      write_decision "skipped" "missing-runtime-state"
      stop_timer
      exit 0
    fi

    boot_id="$(${pkgs.coreutils}/bin/cat /proc/sys/kernel/random/boot_id 2>/dev/null || echo "unknown")"
    booted="$(${bootedGeneration} 2>/dev/null || true)"
    current="$(${pkgs.coreutils}/bin/readlink -f /run/current-system 2>/dev/null || true)"
    graphical_state="$(${pkgs.systemd}/bin/systemctl is-active graphical.target 2>/dev/null || true)"
    [ -z "$graphical_state" ] && graphical_state="inactive"
    display_state="$(${pkgs.systemd}/bin/systemctl is-active display-manager.service 2>/dev/null || true)"
    [ -z "$display_state" ] && display_state="inactive"
    display_pid="$(${pkgs.systemd}/bin/systemctl show -p MainPID --value display-manager.service 2>/dev/null || echo 0)"
    active_enter_usec="$(${pkgs.systemd}/bin/systemctl show -p ActiveEnterTimestampMonotonic --value display-manager.service 2>/dev/null || echo 0)"

    now_monotonic_usec() {
      ${pkgs.python3Minimal}/bin/python3 -c 'import time; print(int(time.clock_gettime(time.CLOCK_MONOTONIC) * 1000000))' 2>/dev/null || echo 0
    }
    now_usec="$(now_monotonic_usec)"
    observed_usec=$(( now_usec - active_enter_usec ))
    required_usec=$(( ${toString cfg.healthStabilitySeconds} * 1000000 ))

    timer_started="$(${pkgs.systemd}/bin/systemctl show -p ActiveEnterTimestampMonotonic --value spore-boot-lkg-promote.timer 2>/dev/null || echo 0)"
    expired_flag=""
    if [ -n "$timer_started" ] && [ "$timer_started" -ne 0 ]; then
      max_usec=$(( ${toString cfg.healthQualificationMaxSeconds} * 1000000 ))
      if [ "$(( now_usec - timer_started ))" -ge "$max_usec" ]; then
        expired_flag="--qualification-deadline-expired"
      fi
    fi

    rc=0
    ${statePkg}/bin/spore-recovery-linux qualify \
      --roots-dir ${lib.escapeShellArg rootsDir} \
      --state-dir ${lib.escapeShellArg stateDir} \
      --runtime-dir ${lib.escapeShellArg runtimeDir} \
      --boot-id "$boot_id" \
      --booted-generation "$booted" \
      --current-generation "$current" \
      --graphical-state "$graphical_state" \
      --display-state "$display_state" \
      --display-pid "$display_pid" \
      --active-enter-usec "$active_enter_usec" \
      --observed-usec "$observed_usec" \
      --required-stability-usec "$required_usec" \
      $expired_flag \
      --report-path "$decision_path" || rc=$?

    if [ "$rc" -eq 75 ]; then
      # Not ready yet, try again on next timer tick
      exit 0
    fi

    if [ "$rc" -eq 0 ] && [ -s "$decision_path" ]; then
      disp="$(${pkgs.jq}/bin/jq -r '.disposition // .decision // empty' "$decision_path" 2>/dev/null || true)"
      reason="$(${pkgs.jq}/bin/jq -r '.terminal_reason // .reason_code // empty' "$decision_path" 2>/dev/null || true)"
      if [ "$disp" = "promoted" ] || [ "$disp" = "already-promoted" ] || [ "$disp" = "already-known-good" ]; then
        ${pkgs.coreutils}/bin/mkdir -p ${lib.escapeShellArg runtimeDir}
        ${pkgs.coreutils}/bin/touch ${lib.escapeShellArg (runtimeDir + "/lkg-promoted-generation")}
        if [ "$reason" != "already-promoted" ] && [ "$reason" != "already-known-good" ]; then
          ${pkgs.coreutils}/bin/touch ${lib.escapeShellArg (runtimeDir + "/state-blessed")}
        fi
      fi
    fi

    # Terminal, Promoted, AlreadyKnownGood, or Failed -> stop timer
    stop_timer
    if [ "$rc" -ne 0 ]; then
      exit "$rc"
    fi
  '';

  markLifecycle =
    kind:
    pkgs.writeShellScript "spore-mark-${kind}" ''
      set -eu
      generation="$(${bootedGeneration} 2>/dev/null || true)"
      uptime="$(${pkgs.gawk}/bin/awk '{ print int($1) }' /proc/uptime 2>/dev/null || echo 0)"
      hardware="$(${hardwareFingerprint} 2>/dev/null || true)"
      args=(
        shutdown
        --state-dir ${lib.escapeShellArg stateDir}
        --runtime-dir ${lib.escapeShellArg runtimeDir}
        --kind ${lib.escapeShellArg kind}
        --uptime-secs "$uptime"
      )
      [ -n "$generation" ] && args+=(--generation "$generation")
      [ -n "$hardware" ] && args+=(--hardware-fingerprint "$hardware")
      exec ${statePkg}/bin/spore-boot-state "''${args[@]}"
    '';

  # This runs immediately before the real display manager starts. The marker is
  # the graceful path: quicken-fb sees it and leaves DRM. `systemctl stop` is the
  # enforcement path; TimeoutStopSec on the renderer bounds even a broken or
  # maliciously stubborn process. The command is fail-open for the desktop.
  compositorHandoff = pkgs.writeShellScript "spore-compositor-handoff" ''
    set -u
    ${pkgs.coreutils}/bin/mkdir -p ${lib.escapeShellArg runtimeDir}
    ${pkgs.coreutils}/bin/touch ${lib.escapeShellArg (runtimeDir + "/compositor-handoff")}
    ${pkgs.systemd}/bin/systemctl stop spore-boot-animation.service >/dev/null 2>&1 || true
    exit 0
  '';

  # Avoid DRM acquisition during a live `nixos-rebuild test`/`switch` where the
  # display manager is already running. Exit 1 from ExecCondition means systemd
  # cleanly skips the decorative unit rather than treating it as a failure.
  rendererMayStart = pkgs.writeShellScript "spore-renderer-may-start" ''
    if ${pkgs.systemd}/bin/systemctl is-active --quiet display-manager.service; then
      exit 1
    fi
    exit 0
  '';

in
{
  options.luminous.services.sporeBoot = {
    enable = lib.mkEnableOption "state-aware Spore Boot Ecology";

    package = lib.mkOption {
      type = types.nullOr types.package;
      default = null;
      description = "Optional override package containing bin/quicken-fb; otherwise the pinned Symthaea input is built.";
    };

    statePackage = lib.mkOption {
      type = types.nullOr types.package;
      default = null;
      description = "Optional override package containing bin/spore-boot-state; otherwise the pinned Symthaea input is built.";
    };

    stateDirectory = lib.mkOption {
      type = types.str;
      default = "/var/lib/spore-boot";
      description = "Persistent bounded Spore boot lineage/state directory.";
    };

    runtimeDirectory = lib.mkOption {
      type = types.str;
      default = "/run/spore-boot";
      description = "Ephemeral directory containing the current boot receipt.";
    };

    drmDevice = lib.mkOption {
      type = types.str;
      default = "auto";
      description = "DRM/KMS device path or auto discovery.";
    };

    healthStabilitySeconds = lib.mkOption {
      type = types.ints.between 0 120;
      default = 10;
      description = ''
        Seconds the display-manager process must remain continuously active after
        graphical.target before the actually booted generation may become Last
        Known Good. This promotion is asynchronous and never delays the desktop.
      '';
    };

    healthQualificationMaxSeconds = lib.mkOption {
      type = types.ints.between 1 86400;
      default = 120;
      description = ''
        Maximum monotonic retry window for asynchronous Last Known Good
        qualification during one boot. Once this window expires, Spore stops
        retrying and leaves the previous LKG untouched.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = renderer != null;
        message = "Spore Boot requires luminous.services.sporeBoot.package or the Symthaea flake input";
      }
      {
        assertion = stateTool != null;
        message = "Spore Boot requires luminous.services.sporeBoot.statePackage or the Symthaea flake input";
      }
      {
        assertion = cfg.healthQualificationMaxSeconds >= cfg.healthStabilitySeconds;
        message = "Spore Boot healthQualificationMaxSeconds must be >= healthStabilitySeconds";
      }
    ];

    # Keep broad generation history in systemd-boot while the three semantic
    # roots below protect exact store closures from GC.
    boot.loader.systemd-boot.configurationLimit = lib.mkDefault 15;

    systemd.tmpfiles.rules = [
      "d ${runtimeDir} 0755 root root -"
      "d ${stateDir} 0700 root root -"
      "d ${rootsDir} 0755 root root -"
    ];

    # Capture previous-state facts and rotate Current/Previous semantic roots.
    # Failure is non-fatal to boot and merely causes the animation to skip.
    systemd.services.spore-boot-state-prepare = {
      description = "Prepare factual Spore boot-state receipt";
      wantedBy = [ "multi-user.target" ];
      after = [
        "local-fs.target"
        "systemd-tmpfiles-setup.service"
      ];
      before = [ "spore-boot-animation.service" ];
      serviceConfig = {
        Type = "oneshot";
        ExecStart = prepareState;
        # cfg.stateDirectory/cfg.runtimeDirectory are created by tmpfiles above.
        # Do not also create hard-coded /var/lib/spore-boot or /run/spore-boot
        # here, otherwise custom paths silently diverge from the service sandbox.
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectHome = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
      };
    };

    # Decorative renderer. It only attempts DRM before the display manager is
    # already active (important for `nixos-rebuild test/switch` on a live
    # desktop). At normal boot it runs until the display manager's explicit
    # ExecStartPre handoff below or its own hard deadline.
    systemd.services.spore-boot-animation = {
      description = "Spore state-aware procedural boot animation";
      wantedBy = [ "multi-user.target" ];
      wants = [ "spore-boot-state-prepare.service" ];
      after = [ "spore-boot-state-prepare.service" ];
      before = [ "display-manager.service" ];
      unitConfig = {
        ConditionPathExists = runtimeDir + "/boot-state.json";
      };
      serviceConfig = {
        Type = "simple";
        ExecCondition = rendererMayStart;
        ExecStart = "${rendererPkg}/bin/quicken-fb --receipt ${runtimeDir}/boot-state.json --lineage ${runtimeDir}/lineage.json --handoff-path ${runtimeDir}/compositor-handoff --device ${lib.escapeShellArg cfg.drmDevice}";
        User = "root";
        SupplementaryGroups = [
          "video"
          "render"
        ];
        KillSignal = "SIGTERM";
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
      };
    };

    # Make DRM ownership transfer explicit instead of relying on a mutual
    # Conflicts transaction. Existing display-manager ExecStartPre entries are
    # preserved and this hook is prepended.
    systemd.services.display-manager.serviceConfig.ExecStartPre = lib.mkBefore [ compositorHandoff ];

    # LKG promotion is deliberately *not* ordered before graphical.target. A
    # broken health/promote helper cannot delay graphical availability, and a
    # display manager that only survives startup momentarily is not considered
    # stable enough to bless the generation.
    systemd.services.spore-boot-lkg-promote = {
      description = "Promote stable booted generation to Spore Last Known Good";
      serviceConfig = {
        Type = "oneshot";
        ExecStart = promoteLkg;
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectHome = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
      };
    };

    systemd.timers.spore-boot-lkg-promote = {
      description = "Schedule non-blocking Spore Last Known Good qualification";
      wantedBy = [ "multi-user.target" ];
      wants = [ "spore-boot-state-prepare.service" ];
      after = [ "spore-boot-state-prepare.service" ];
      timerConfig = {
        OnActiveSec = "1s";
        OnUnitInactiveSec = "2s";
        AccuracySec = "1s";
        Unit = "spore-boot-lkg-promote.service";
      };
    };

    # Separate shutdown targets preserve the difference between reboot and
    # poweroff without guessing in the state model.
    systemd.services.spore-boot-mark-reboot = {
      description = "Record clean reboot for next Spore boot";
      wantedBy = [ "reboot.target" ];
      before = [ "reboot.target" ];
      unitConfig.DefaultDependencies = false;
      serviceConfig = {
        Type = "oneshot";
        ExecStart = markLifecycle "reboot";
      };
    };

    systemd.services.spore-boot-mark-poweroff = {
      description = "Record clean poweroff for next Spore boot";
      wantedBy = [
        "poweroff.target"
        "halt.target"
      ];
      before = [
        "poweroff.target"
        "halt.target"
      ];
      unitConfig.DefaultDependencies = false;
      serviceConfig = {
        Type = "oneshot";
        ExecStart = markLifecycle "poweroff";
      };
    };

    # Sleep/hibernate state is factual too. A post-resume hook removes the
    # marker so a later unexpected loss is not misreported as a clean resume.
    environment.etc."systemd/system-sleep/spore-boot-state" = {
      mode = "0755";
      text = ''
        #!${pkgs.runtimeShell}
        set -eu
        case "$1:$2" in
          pre:suspend|pre:suspend-then-hibernate)
            ${markLifecycle "suspend"} || true
            ;;
          pre:hibernate|pre:hybrid-sleep)
            ${markLifecycle "hibernate"} || true
            ;;
          post:*)
            ${pkgs.coreutils}/bin/rm -f ${lib.escapeShellArg (stateDir + "/clean-shutdown.json")}
            ;;
        esac
      '';
    };
  };
}
