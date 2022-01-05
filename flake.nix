{
  description = "nix-netboot-serve";

  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";

  outputs =
    { self
    , nixpkgs
    , ...
    } @ inputs:
    let
      nameValuePair = name: value: { inherit name value; };
      genAttrs = names: f: builtins.listToAttrs (map (n: nameValuePair n (f n)) names);
      allSystems = [ "x86_64-linux" "aarch64-linux" "i686-linux" "x86_64-darwin" ];

      forAllSystems = f: genAttrs allSystems (system: f {
        inherit system;
        pkgs = import nixpkgs { inherit system; };
      });
    in
    {
      devShell = forAllSystems ({ system, pkgs, ... }: self.packages.${system}.package.overrideAttrs ({ nativeBuildInputs ? [ ], ... }: {
        nativeBuildInputs = nativeBuildInputs ++ (with pkgs; [
          binwalk
          codespell
          entr
          file
          nixpkgs-fmt
          rustfmt
          vim # xxd
        ]);
      }));

      packages = forAllSystems
        ({ system, pkgs, ... }: {
          package = pkgs.rustPlatform.buildRustPackage rec {
            pname = "nix-netboot-serve";
            version = "unreleased";

            nativeBuildInputs = with pkgs; [
              which
              coreutils
              cpio
              nix
              zstd
              pkgconfig
            ];

            buildInputs = with pkgs; [
              openssl
            ];

            src = self;

            cargoLock.lockFile = src + "/Cargo.lock";
          };

          nixos-test = import ./nixos-test.nix { inherit pkgs; inherit (self) nixosModules; } { inherit pkgs system; };
        });

      defaultPackage = forAllSystems ({ system, ... }: self.packages.${system}.package);

      nixosModules.nix-netboot-serve = {
        imports = [ ./nixos-module.nix ];
        nixpkgs.overlays = [
          (final: prev: {
            nix-netboot-serve = self.defaultPackage."${final.stdenv.hostPlatform.system}";
          })
        ];
      };
    };
}
