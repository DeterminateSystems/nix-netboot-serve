{
  description = "nix-netboot-serve";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    cpiotools.url = "github:DeterminateSystems/cpiotools";
  };

  outputs =
    { self
    , nixpkgs
    , cpiotools
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
          cpiotools.packages.${system}.package
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

      defaultPackage = forAllSystems
        ({ system, ... }: self.packages.${system}.package);

      nixosModules = {
        nix-netboot-serve = {
          imports = [ ./nix-modules/nix-netboot-serve-service.nix ];
          nixpkgs.overlays = [
            (final: prev: {
              nix-netboot-serve = self.defaultPackage."${final.stdenv.hostPlatform.system}";
            })
          ];
        };
        no-filesystem = ./nix-modules/no-filesystem.nix;
        register-nix-store = ./nix-modules/register-nix-store.nix;
        swap-to-disk = ./nix-modules/swap-to-disk.nix;
        tmpfs-root = ./nix-modules/tmpfs-root.nix;
      };
    };
}
