{
  description = "Sovereign Boot Ecology — fail-open DRM/KMS boot animation for NixOS";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    let
      nixosModule = import ./nix/modules/sovereign-boot.nix;
    in
    {
      nixosModules = {
        default = nixosModule;
        sovereignBoot = nixosModule;
      };
    } // flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in
      {
        packages = rec {
          quicken-fb = pkgs.rustPlatform.buildRustPackage {
            pname = "quicken-fb";
            version = "0.1.0";
            src = ./.;
            buildAndTestSubdir = "crates/quicken-fb";
            cargoLock.lockFile = ./Cargo.lock;
            doCheck = true;
            meta = with pkgs.lib; {
              description = "DRM/KMS bare-metal boot animation renderer";
              license = licenses.agpl3Plus;
              platforms = platforms.linux;
            };
          };

          physical-canary = pkgs.writeShellApplication {
            name = "sovereign-boot-physical-canary";
            runtimeInputs = with pkgs; [
              coreutils
              gnugrep
              systemd
              util-linux
            ];
            text = ''
              export SOVEREIGN_BOOT_ARTIFACT="${quicken-fb}/bin/quicken-fb"
              exec ${./scripts/launch-physical-canary.sh} "$@"
            '';
          };

          arm-physical-canary = pkgs.writeShellApplication {
            name = "sovereign-boot-arm-physical-canary";
            runtimeInputs = with pkgs; [
              coreutils
              gnugrep
              systemd
            ];
            text = ''
              export SOVEREIGN_BOOT_ARTIFACT="${quicken-fb}/bin/quicken-fb"
              exec ${./scripts/arm-physical-canary.sh} "$@"
            '';
          };

          default = quicken-fb;
        };

        checks = {
          sovereign-boot-boundary =
            pkgs.testers.runNixOSTest (import ./tests/boot-boundary.nix);
          sovereign-boot-boot-scoped-canary =
            pkgs.testers.runNixOSTest (import ./tests/boot-scoped-canary.nix);
        };

        formatter = pkgs.nixfmt;

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            cargo
            rustc
            pkg-config
            nil
            nixfmt
          ];
        };
      }
    );
}
