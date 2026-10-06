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
    if ((is_canary)) && [[ -e /run/sovereign-boot-test/hang ]]; then
      sleep 30
    fi
    if ((is_canary)) && [[ -e /run/sovereign-boot-test/no-restore ]]; then
      echo "fake quicken-fb: successful exit without restoration receipt"
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
    probe='drm-ok device=/dev/dri/card99 connector=VM-1 crtc=fake selection=current mode=1024x768 refresh=60Hz'
    printf '%s\n' "$probe" > /var/lib/sovereign-boot/physical-canary.preboot-probe
    chmod 0600 /var/lib/sovereign-boot/physical-canary.preboot-probe
    probe_sha="$(sha256sum /var/lib/sovereign-boot/physical-canary.preboot-probe | cut -d' ' -f1)"
    now="$(date +%s)"
    printf 'request_id=%s\ndevice=/dev/dri/card99\nseconds=1\nartifact_sha256=%s\npreboot_probe_sha256=%s\narmed_at_unix_s=%s\nexpires_at_unix_s=%s\n' 11111111111111111111111111111111 "$sha256" "$probe_sha" "$now" "$((now + 900))" > /var/lib/sovereign-boot/physical-canary.request
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

    systemd.services.sovereign-boot-physical-canary.serviceConfig.TimeoutStartSec = pkgs.lib.mkForce "3s";

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
    assert "request_id=11111111111111111111111111111111" in result, result
    assert "armed_at_unix_s=" in result, result
    assert "expires_at_unix_s=" in result, result
    assert "preboot_probe_sha256=" in result, result
    assert "boot_probe_sha256=" in result, result
    assert "restore_receipt_sha256=" in result, result
    machine.succeed("test -f /run/sovereign-boot-test/display-manager.started")
    machine.succeed("test ! -e /var/lib/sovereign-boot/physical-canary.request")
    machine.succeed("test ! -e /var/lib/sovereign-boot/physical-canary.request.inflight")
    machine.succeed("test -s /var/lib/sovereign-boot/requests/11111111111111111111111111111111.request")
    machine.succeed("test -s /var/lib/sovereign-boot/requests/11111111111111111111111111111111.result")
    machine.succeed("test -s /var/lib/sovereign-boot/requests/11111111111111111111111111111111.probe")
    machine.succeed("test -s /var/lib/sovereign-boot/requests/11111111111111111111111111111111.output")
    machine.succeed("test -s /var/lib/sovereign-boot/requests/11111111111111111111111111111111.preboot-probe")
    permissions = machine.succeed("stat -c %a /var/lib/sovereign-boot/physical-canary.result").strip()
    assert permissions == "600", permissions, result

    unit = machine.succeed(
        "systemctl cat sovereign-boot-physical-canary.service"
    )
    assert "ConditionPathExists=/var/lib/sovereign-boot/physical-canary.request" in unit
    assert "StandardInput=tty-fail" in unit
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

    # Starting the canary while the display manager is active must be a
    # non-destructive refusal, not a stop-job against the desktop.
    live_sha = machine.succeed(
        "sha256sum ${fakeRenderer}/bin/quicken-fb | cut -d' ' -f1"
    ).strip()
    machine.succeed(
        "printf 'request_id=88888888888888888888888888888888\\ndevice=/dev/dri/card99\\nseconds=1\\nartifact_sha256=%s\\npreboot_probe_sha256=8888888888888888888888888888888888888888888888888888888888888888\\narmed_at_unix_s=%s\\nexpires_at_unix_s=%s\\n' " + live_sha + " \"$(date +%s)\" \"$(($(date +%s) + 900))\" > /var/lib/sovereign-boot/physical-canary.request"
    )
    machine.succeed("systemctl reset-failed sovereign-boot-physical-canary.service")
    machine.fail("systemctl start sovereign-boot-physical-canary.service")
    result = machine.succeed(
        "cat /var/lib/sovereign-boot/physical-canary.result"
    )
    assert "status=FAIL_DISPLAY_MANAGER" in result, result
    machine.succeed("test -s /var/lib/sovereign-boot/requests/88888888888888888888888888888888.request")
    assert machine.succeed("systemctl is-active display-manager.service").strip() == "active"
    machine.succeed("test ! -e /var/lib/sovereign-boot/physical-canary.request.inflight")

    # Leave the display-manager stopped while exercising subsequent
    # boot-scoped requests; the real service would only run before it starts.
    machine.succeed("systemctl stop display-manager.service")

    # A stale/mismatched artifact request must fail closed before the renderer
    # executes, and the request must still be consumed.
    machine.succeed(
        "printf 'request_id=%s\ndevice=/dev/dri/card99\nseconds=1\nartifact_sha256=%064d\npreboot_probe_sha256=%s\narmed_at_unix_s=%s\nexpires_at_unix_s=%s\n' 22222222222222222222222222222222 0 2222222222222222222222222222222222222222222222222222222222222222 "$(date +%s)" "$(($(date +%s) + 900))" > /var/lib/sovereign-boot/physical-canary.request"
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
    machine.succeed("test -s /var/lib/sovereign-boot/requests/22222222222222222222222222222222.request")
    machine.succeed("test -s /var/lib/sovereign-boot/requests/22222222222222222222222222222222.result")
    assert machine.succeed(
        "systemctl is-active multi-user.target"
    ).strip() == "active"

    # A wrong requested DRM card must fail closed even when an alternate card path exists.
    machine.succeed("touch /dev/dri/card98")
    machine.succeed(
        "printf 'request_id=%s\ndevice=/dev/dri/card98\nseconds=1\nartifact_sha256=0000000000000000000000000000000000000000000000000000000000000000\npreboot_probe_sha256=%s\narmed_at_unix_s=%s\nexpires_at_unix_s=%s\n' 33333333333333333333333333333333 3333333333333333333333333333333333333333333333333333333333333333 "$(date +%s)" "$(($(date +%s) + 900))" > /var/lib/sovereign-boot/physical-canary.request"
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
    machine.succeed("test -s /var/lib/sovereign-boot/requests/33333333333333333333333333333333.request")

    # Exit code zero is insufficient without the explicit restoration receipt.
    machine.succeed("touch /run/sovereign-boot-test/no-restore")
    sha = machine.succeed(
        "sha256sum ${fakeRenderer}/bin/quicken-fb | cut -d' ' -f1"
    ).strip()
    machine.succeed(
        "printf 'request_id=44444444444444444444444444444444\\ndevice=/dev/dri/card99\\nseconds=1\\nartifact_sha256=%s\\npreboot_probe_sha256=32d2589e89bde7b3a00a939e390d74b997b7909307c3ed7ea4b2a07b63b82708\\narmed_at_unix_s=%s\\nexpires_at_unix_s=%s\\n' " + sha + " \"$(date +%s)\" \"$(($(date +%s) + 900))\" > /var/lib/sovereign-boot/physical-canary.request"
    )
    machine.succeed("systemctl reset-failed sovereign-boot-physical-canary.service")
    machine.fail("systemctl start sovereign-boot-physical-canary.service")
    result = machine.succeed(
        "cat /var/lib/sovereign-boot/physical-canary.result"
    )
    assert "status=FAIL_RESTORE_RECEIPT" in result, result
    assert "exit_code=7" in result, result
    assert "restore_receipt_sha256=" in result, result
    assert "renderer_output_sha256=" in result, result
    machine.succeed("test ! -e /var/lib/sovereign-boot/physical-canary.request.inflight")
    machine.succeed("test -s /var/lib/sovereign-boot/requests/44444444444444444444444444444444.request")
    machine.succeed("test -s /var/lib/sovereign-boot/requests/44444444444444444444444444444444.result")
    machine.succeed("test -s /var/lib/sovereign-boot/requests/44444444444444444444444444444444.output")
    machine.succeed("test -s /var/lib/sovereign-boot/requests/44444444444444444444444444444444.preboot-probe")
    machine.succeed("rm -f /run/sovereign-boot-test/no-restore")

    # An expired request must never execute the renderer.
    stale_sha = machine.succeed(
        "sha256sum ${fakeRenderer}/bin/quicken-fb | cut -d' ' -f1"
    ).strip()
    machine.succeed(
        "printf 'request_id=55555555555555555555555555555555\\ndevice=/dev/dri/card99\\nseconds=1\\nartifact_sha256=%s\\npreboot_probe_sha256=deadbeef\\narmed_at_unix_s=1\\nexpires_at_unix_s=2\\n' " + stale_sha + " > /var/lib/sovereign-boot/physical-canary.request"
    )
    machine.succeed("systemctl reset-failed sovereign-boot-physical-canary.service")
    machine.fail("systemctl start sovereign-boot-physical-canary.service")
    result = machine.succeed(
        "cat /var/lib/sovereign-boot/physical-canary.result"
    )
    assert "status=FAIL_EXPIRED_REQUEST" in result, result
    machine.succeed(
        "test ! -e /var/lib/sovereign-boot/physical-canary.request.inflight"
    )
    machine.succeed("test -s /var/lib/sovereign-boot/requests/55555555555555555555555555555555.request")

    # A renderer failure must also remain fail-open for the boot target.
    machine.succeed("touch /run/sovereign-boot-test/fail")
    sha = machine.succeed(
        "sha256sum \${fakeRenderer}/bin/quicken-fb | cut -d' ' -f1"
    ).strip()
    machine.succeed(
        "printf 'request_id=66666666666666666666666666666666\ndevice=/dev/dri/card99\nseconds=1\nartifact_sha256=%s\npreboot_probe_sha256=6666666666666666666666666666666666666666666666666666666666666666\narmed_at_unix_s=%s\nexpires_at_unix_s=%s\n' " + sha + " \"$(date +%s)\" \"$(($(date +%s) + 900))\" > /var/lib/sovereign-boot/physical-canary.request"
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

    # A hard renderer timeout must generate a durable service-level failure.
    machine.succeed("touch /run/sovereign-boot-test/hang")
    sha = machine.succeed(
        "sha256sum ${fakeRenderer}/bin/quicken-fb | cut -d' ' -f1"
    ).strip()
    machine.succeed(
        "printf 'request_id=77777777777777777777777777777777\\ndevice=/dev/dri/card99\\nseconds=1\\nartifact_sha256=%s\\npreboot_probe_sha256=32d2589e89bde7b3a00a939e390d74b997b7909307c3ed7ea4b2a07b63b82708\\narmed_at_unix_s=%s\\nexpires_at_unix_s=%s\\n' " + sha + " \"$(date +%s)\" \"$(($(date +%s) + 900))\" > /var/lib/sovereign-boot/physical-canary.request"
    )
    machine.succeed("systemctl reset-failed sovereign-boot-physical-canary.service")
    machine.fail("systemctl start sovereign-boot-physical-canary.service")
    result = machine.succeed(
        "cat /var/lib/sovereign-boot/physical-canary.result"
    )
    assert "status=FAIL_SERVICE_TIMEOUT" in result, result
    assert "request_id=77777777777777777777777777777777" in result, result
    assert "service_result=timeout" in result, result
    machine.succeed(
        "test ! -e /var/lib/sovereign-boot/physical-canary.request.inflight"
    )
    machine.succeed("test -s /var/lib/sovereign-boot/requests/77777777777777777777777777777777.request")
    machine.succeed("test -s /var/lib/sovereign-boot/requests/77777777777777777777777777777777.result")
    assert machine.succeed(
        "systemctl is-active multi-user.target"
    ).strip() == "active"
    machine.succeed("rm -f /run/sovereign-boot-test/hang")

    # The fake display manager participates in the same boot transaction and
    # can only report success after the canary result exists.
    machine.succeed("systemctl start graphical.target")
    machine.wait_for_unit("display-manager.service")
    machine.succeed(
        "test -f /run/sovereign-boot-test/display-manager.started"
    )
  '';
}
