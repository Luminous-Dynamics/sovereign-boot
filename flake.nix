{
  description = "Sovereign Boot Ecology — optional DRM/KMS renderer with standalone fail-open lifecycle tools";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    let
      nixosModule = { config, pkgs, lib, utils, ... }@args:
        import ./nix/modules/sovereign-boot.nix (args // {
          inputs = (args.inputs or { }) // {
            sovereignBootTools = self.packages.${pkgs.system}.spore-boot-tools;
          };
        });
    in
    {
      nixosModules = {
        default = nixosModule;
        sovereignBoot = nixosModule;
        sporeBoot = nixosModule;
      };
    } // flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in
      {
        packages = rec {
          quicken-fb = pkgs.rustPlatform.buildRustPackage {
            pname = "quicken-fb";
            version = "0.3.4";
            src = ./crates/quicken-fb;
            cargoLock.lockFile = ./crates/quicken-fb/Cargo.lock;
            # Renderer correctness is tested without requiring a live DRM device.
            doCheck = true;
            cargoTestFlags = [ "--all-targets" ];
            meta = with pkgs.lib; {
              description = "Optional DRM/KMS procedural boot renderer";
              license = licenses.agpl3Plus;
              platforms = platforms.linux;
            };
          };

          boot-lifecycle = pkgs.rustPlatform.buildRustPackage {
            pname = "sovereign-boot-lifecycle";
            version = "0.1.0";
            src = ./crates/boot-lifecycle;
            cargoLock.lockFile = ./crates/boot-lifecycle/Cargo.lock;
            doCheck = true;
            meta = with pkgs.lib; {
              description = "Standalone fail-open boot-state receipts and Last Known Good GC roots";
              license = licenses.agpl3Plus;
              platforms = platforms.linux;
            };
          };

          # The module expects these binaries under one package by default;
          # consumers may still override renderer and statePackage independently.
          spore-boot-tools = pkgs.buildEnv {
            name = "spore-boot-tools";
            paths = [ quicken-fb boot-lifecycle ];
          };

          default = quicken-fb;
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            cargo
            rustc
            pkg-config
            nil
            nixfmt-rfc-style
          ];
        };
      });
}
