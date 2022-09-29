{ pkgs, lib, config, options, ... }:
let
  cfg = config.services.nix-netboot-serve;
  opts = options.services.nix-netboot-serve;
in
{
  options = {
    services.nix-netboot-serve = {
      enable = lib.mkEnableOption "nix-netboot-serve";

      profile_dir = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
      };

      configuration_dir = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
      };

      gc_root_dir = lib.mkOption {
        type = lib.types.path;
        default = "/var/cache/nix-netboot-serve/gc-roots";
      };

      cpio_cache_dir = lib.mkOption {
        type = lib.types.path;
        default = "/var/cache/nix-netboot-serve/cpios";
      };

      cpio_cache_max_bytes = lib.mkOption {
        type = lib.types.int;
        description = "Maximum amount of space the CPIO cache may take up, in bytes.";
        default = 5 * 1024 * 1024 * 1024;
      };

      listen = lib.mkOption {
        type = lib.types.str;
        default = "0.0.0.0:3030";
      };

      debug = lib.mkOption {
        type = lib.types.bool;
        default = false;
      };

      open_files = lib.mkOption {
        type = lib.types.nullOr lib.types.ints.u32;
        default = null;
      };
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.tmpfiles.rules =
      (lib.optional (cfg.gc_root_dir == opts.gc_root_dir.default) "d ${cfg.gc_root_dir} 0700 nix-netboot-serve nix-netboot-serve -")
      ++ (lib.optional (cfg.cpio_cache_dir == opts.cpio_cache_dir.default) "d ${cfg.cpio_cache_dir} 0700 nix-netboot-serve nix-netboot-serve -");

    users.users.nix-netboot-serve = {
      group = "nix-netboot-serve";
      isSystemUser = true;
    };
    users.groups.nix-netboot-serve = { };

    systemd.services.nix-netboot-serve = {
      wantedBy = [ "multi-user.target" ];
      description = "A netboot image generator for NixOS closures.";
      environment.RUST_LOG = if cfg.debug then "debug" else "info";
      serviceConfig = {
        User = "nix-netboot-serve";
        Group = "nix-netboot-serve";
        ExecStart = "${pkgs.nix-netboot-serve}/bin/nix-netboot-serve " +
          (lib.escapeShellArgs (
            [ "--gc-root-dir" cfg.gc_root_dir ]
              ++ [ "--cpio-cache-dir" cfg.cpio_cache_dir ]
              ++ [ "--max-cpio-cache-bytes" cfg.cpio_cache_max_bytes ]
              ++ [ "--listen" cfg.listen ]
              ++ (lib.optionals (cfg.profile_dir != null) [ "--profile-dir" cfg.profile_dir ])
              ++ (lib.optionals (cfg.configuration_dir != null) [ "--config-dir" cfg.configuration_dir ])
              ++ (lib.optionals (cfg.open_files != null) [ "--open-files" cfg.open_files ])
          ));
      };
    };
  };
}
