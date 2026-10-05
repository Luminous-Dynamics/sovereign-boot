{
  description = "Sovereign Boot Ecology — fail-open DRM/KMS boot animation for NixOS";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in
      {
        packages = rec {
          quicken-fb = pkgs.rustPlatform.buildRustPackage {
            pname = "quicken-fb";
            version = "0.1.0";
            src = ./crates/quicken-fb;
            cargoLock.lockFile = ./Cargo.lock;
            doCheck = true;
            meta = with pkgs.lib; {
              description = "DRM/KMS bare-metal boot animation renderer";
              license = licenses.agpl3Plus;
              platforms = platforms.linux;
            };
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
      }
    );
}
