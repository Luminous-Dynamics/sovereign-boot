{ pkgs, ... }:
{
  name = "sovereign-boot-boundary";

  nodes.machine = { ... }: {
    imports = [ ../nix/modules/sovereign-boot.nix ];

    luminous.services.sovereignBoot = {
      enable = true;
      package = pkgs.writeShellScriptBin "quicken-fb" ''
        echo "fake quicken-fb"
        exit 0
      '';
      drmDevice = "/dev/dri/card99";
      genesisPhrase = "VM qualification";
    };

    # No graphical stack and therefore no /dev/dri connector device. This
    # exercises the decorative no-DRM skip path rather than hardware rendering.
    services.displayManager.enable = false;
    virtualisation.memorySize = 1024;
  };

  testScript = ''
    machine.start()
    machine.wait_for_unit("multi-user.target")

    # The service is enabled through Wants=, not Requires=, so its failure or
    # absence cannot make multi-user.target unavailable.
    requires = machine.succeed(
        "systemctl show multi-user.target -p Requires --value"
    )
    assert "sovereign-boot-animation.service" not in requires

    enabled = machine.succeed(
        "systemctl is-enabled sovereign-boot-animation.service"
    ).strip()
    assert enabled == "enabled", enabled

    # With no /dev/dri in the VM, the service must cleanly remain inactive
    # because its ConditionPathExists gate prevents renderer startup.
    active = machine.succeed(
        "systemctl show sovereign-boot-animation.service -p ActiveState --value"
    ).strip()
    assert active in ("inactive", "dead"), active

    unit = machine.succeed("systemctl cat sovereign-boot-animation.service")
    assert "ConditionPathExists=/dev/dri" in unit
    assert "CapabilityBoundingSet=" in unit
    assert "ExecCondition=" in unit

    machine.succeed("test ! -e /dev/dri/card99")
  '';
}
