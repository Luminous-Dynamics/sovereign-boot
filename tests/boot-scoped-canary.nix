{ pkgs, ... }:
let
  fakeRenderer = pkgs.writeShellScriptBin "quicken-fb" ''
    set -euo pipefail
    [[ "$(< /sys/class/tty/tty0/active)" == "tty1" ]]
    [[ "$(readlink /proc/self/fd/0)" == "/dev/tty1" ]]
    is_probe=0
    is_canary=0
    while (($#)); do
      case "$1" in
        --probe) is_probe=1 ;;
        --canary-seconds) is_canary=1; shift ;;
      esac
      shift
    done
    if ((is_probe)); then
      echo "drm-ok device=/dev/dri/card99 connector=VM-1 crtc=fake selection=current mode=1024x768 refresh=60Hz"
      exit 0
    fi
    if ((is_canary)) && [[ -e /run/sovereign-boot-test/fail ]]; then
      echo "fake quicken-fb: intentional qualification failure" >&2
      exit 7
    fi
    echo "fake quicken-fb: tty=tty1 active_vt=tty1"
    echo "drm-restore-ok crtc=fake connectors=1 framebuffer=fake"
    exit 0
  '';

  seedRequest = pkgs.writeShellScript "seed-physical-canary-request" ''
    set -euo pipefail
    mkdir -p /dev/dri /run/sovereign-boot-test /var/lib/sovereign-boot
    : > /dev/dri/card99
    sha256="$(sha256sum \${fakeRenderer}/bin/quicken-fb | cut -d' ' -f1)"
    printf 'device=/dev/dri/card99\nseconds=1\nartifact_sha256=%s\n' "$sha256" > /var/lib/sovereign-boot/physical-canary.request
  '';

  fakeDisplayManager = pkgs.writeShellScript "fake-display-manager" ''
    set -euo pipefail
    test -f /var/lib/sovereign-boot/physical-canary.result
    echo "fake-display-manager: started after canary" > /run/sovereign-boot-test/display-manager.started
  '';
in
{
  name = "sovereign-boot-boot-scoped-canary";

  nodes.machine = { ... }: {
    imports = [ ../nix/modules/sovereign-boot.nix ];

    luminous.services.sovereignBoot = {
      enable = true;
      package = fakeRenderer;
      drmDevice = "/dev/dri/card99";
      genesisPhrase = "VM qualification";
    };

    services.displayManager.enable = false;
    systemd.defaultUnit = "graphical.target";

    systemd.services.sovereign-boot-animation.wantedBy = pkgs.lib.mkForce [ ];

    systemd.services.seed-physical-canary-request = {
      description = "Seed a deterministic physical canary request";
      wantedBy = [ "multi-user.target" ];
      before = [ "sovereign-boot-physical-canary.service" ];
      serviceConfig = {
        Type = "oneshot";
        ExecStart = seedRequest;
        TimeoutStartSec = "5s";
      };
    };

    systemd.services.display-manager = {
      description = "Fake display manager for boot-boundary qualification";
      wantedBy = [ "graphical.target" ];
      serviceConfig = {
        Type = "oneshot";
        ExecStart = fakeDisplayManager;
        RemainAfterExit = true;
      };
    };

    virtualisation.memorySize = 1024;
  };

  testScript = ''
    machine.start()
    machine.wait_for_unit("graphical.target")
    machine.wait_for_unit("multi-user.target")

    machine.succeed(
        "test ! -e /var/lib/sovereign-boot/physical-canary.request"
    )

    result = machine.succeed(
        "cat /var/lib/sovereign-boot/physical-canary.result"
    )
    assert "status=PASS" in result, result
    assert "device=/dev/dri/card99" in result, result
    assert "seconds=1" in result, result
    assert "exit_code=0" in result, result
    assert "artifact_sha256=" in result, result
    assert "probe_receipt=drm-ok" in result, result
    assert "restore_receipt=drm-restore-ok" in result, result
    machine.succeed("test -f /run/sovereign-boot-test/display-manager.started")
    machine.succeed("test ! -e /var/lib/sovereign-boot/physical-canary.request")
    machine.succeed("test ! -e /var/lib/sovereign-boot/physical-canary.request.inflight")
    permissions = machine.succeed("stat -c %a /var/lib/sovereign-boot/physical-canary.result").strip()
    assert permissions == "600", permissions, result

    unit = machine.succeed(
        "systemctl cat sovereign-boot-physical-canary.service"
    )
    assert "ConditionPathExists=/var/lib/sovereign-boot/physical-canary.request" in unit
    assert "StandardInput=tty" in unit
    assert "TTYPath=/dev/tty1" in unit
    assert "Before=display-manager.service" in unit
    assert "Before=getty@tty1.service" in unit
    assert "Conflicts=display-manager.service" in unit
    assert "Conflicts=getty@tty1.service" in unit
    assert "CapabilityBoundingSet=" in unit
    assert "NoNewPrivileges=true" in unit
    assert "ReadWritePaths=/var/lib/sovereign-boot" in unit
    assert "DevicePolicy=strict" in unit
    assert "/dev/tty1 rw" in unit

    requires = machine.succeed(
        "systemctl show multi-user.target -p Requires --value"
    )
    assert "sovereign-boot-physical-canary.service" not in requires

    active = machine.succeed(
        "systemctl is-active multi-user.target"
    ).strip()
    assert active == "active", active

    # Leave the display-manager stopped while exercising subsequent
    # boot-scoped requests; the real service would only run before it starts.
    machine.succeed("systemctl stop display-manager.service")

    # A stale/mismatched artifact request must fail closed before the renderer
    # executes, and the request must still be consumed.
    machine.succeed(
        "printf 'device=/dev/dri/card99\nseconds=1\nartifact_sha256=%064d\n' 0 > /var/lib/sovereign-boot/physical-canary.request"
    )
    machine.succeed("systemctl reset-failed sovereign-boot-physical-canary.service")
    machine.fail("systemctl start sovereign-boot-physical-canary.service")
    result = machine.succeed(
        "cat /var/lib/sovereign-boot/physical-canary.result"
    )
    assert "status=FAIL_ARTIFACT_MISMATCH" in result, result
    machine.succeed(
        "test ! -e /var/lib/sovereign-boot/physical-canary.request"
    )
    machine.succeed(
        "test ! -e /var/lib/sovereign-boot/physical-canary.request.inflight"
    )
    assert machine.succeed(
        "systemctl is-active multi-user.target"
    ).strip() == "active"

    # A wrong requested DRM card must fail closed even when an alternate card path exists.
    machine.succeed("touch /dev/dri/card98")
    machine.succeed(
        "printf 'device=/dev/dri/card98\nseconds=1\nartifact_sha256=0000000000000000000000000000000000000000000000000000000000000000\n' > /var/lib/sovereign-boot/physical-canary.request"
    )
    machine.succeed("systemctl reset-failed sovereign-boot-physical-canary.service")
    machine.fail("systemctl start sovereign-boot-physical-canary.service")
    result = machine.succeed(
        "cat /var/lib/sovereign-boot/physical-canary.result"
    )
    assert "status=FAIL_DEVICE_MISMATCH" in result, result
    machine.succeed(
        "test ! -e /var/lib/sovereign-boot/physical-canary.request"
    )

    # A renderer failure must also remain fail-open for the boot target.
    machine.succeed("touch /run/sovereign-boot-test/fail")
    sha = machine.succeed(
        "sha256sum \${fakeRenderer}/bin/quicken-fb | cut -d' ' -f1"
    ).strip()
    machine.succeed(
        "printf 'device=/dev/dri/card99\nseconds=1\nartifact_sha256=%s\n' " + sha + " > /var/lib/sovereign-boot/physical-canary.request"
    )
    machine.succeed("systemctl reset-failed sovereign-boot-physical-canary.service")
    machine.fail("systemctl start sovereign-boot-physical-canary.service")
    result = machine.succeed(
        "cat /var/lib/sovereign-boot/physical-canary.result"
    )
    assert "status=FAIL_RENDERER" in result, result
    assert "exit_code=7" in result, result
    machine.succeed(
        "test ! -e /var/lib/sovereign-boot/physical-canary.request"
    )
    machine.succeed(
        "test ! -e /var/lib/sovereign-boot/physical-canary.request.inflight"
    )
    assert machine.succeed(
        "systemctl is-active multi-user.target"
    ).strip() == "active"

    # The fake display manager participates in the same boot transaction and
    # can only report success after the canary result exists.
    machine.succeed("systemctl start graphical.target")
    machine.wait_for_unit("display-manager.service")
    machine.succeed(
        "test -f /run/sovereign-boot-test/display-manager.started"
    )
  '';
}
