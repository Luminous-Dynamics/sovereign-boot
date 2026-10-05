{
  description = "Sovereign Boot Ecology — DRM/KMS framebuffer renderer, fail-open state machine, and Linux recovery executor for NixOS";

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
            doCheck = false;
            meta = with pkgs.lib; {
              description = "DRM/KMS bare-metal boot animation renderer";
              license = licenses.agpl3Plus;
              platforms = platforms.linux;
            };
          };

          default = quicken-fb;
          spore-boot-tools = quicken-fb;
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
