let
  source = builtins.getFlake (toString ./..);
  nixpkgs = builtins.getFlake "github:NixOS/nixpkgs/5b2c2d84341b2afb5647081c1386a80d7a8d8605";
  system = "x86_64-linux";
  evaluated = nixpkgs.lib.nixosSystem {
    inherit system;
    modules = [
      source.nixosModules.sovereignBoot
      ({ ... }: {
        luminous.services.sporeBoot.enable = true;
        boot.loader.systemd-boot.enable = true;
        fileSystems."/".device = "/dev/disk/by-label/sovereign-boot-smoke";
        system.stateVersion = "25.11";
      })
    ];
  };
in {
  prepareExecStart = evaluated.config.systemd.services.spore-boot-state-prepare.serviceConfig.ExecStart;
  animationExecStart = evaluated.config.systemd.services.spore-boot-animation.serviceConfig.ExecStart;
  lkgTimerUnit = evaluated.config.systemd.timers.spore-boot-lkg-promote.timerConfig.Unit;
  moduleDisabledByDefault = source.nixosModules.sovereignBoot != null;
}
