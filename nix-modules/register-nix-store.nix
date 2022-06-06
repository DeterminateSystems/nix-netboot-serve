# register-nix-store
#
# Register the store paths provided by nix-netboot-serve  into the Nix
# database.
#
# Makes it safe to call nix-collect-garbage.
{ pkgs, ... }: {
  boot.postBootCommands = ''
    PATH=${pkgs.nix}/bin /nix/.nix-netboot-serve-db/register
  '';
}
