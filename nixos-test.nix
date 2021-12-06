{ pkgs, nixosModules }:
import "${pkgs.path}/nixos/tests/make-test-python.nix" ({ pkgs, ... }: {
  name = "nix-netboot-serve";
  meta.maintainers = pkgs.lib.teams.determinatesystems.members;

  nodes.machine = { pkgs, ... }: {
    imports = [ nixosModules.nix-netboot-serve ];
    environment.systemPackages = [
      pkgs.hello
    ];

    services.nix-netboot-serve.enable = true;
  };

  testScript = ''
    machine.start()

    machine.wait_for_unit("default.target")

    # Fetch the closure of the current system
    system_store_path_name = machine.succeed("readlink /run/current-system").strip().split("/")[3]

    machine.wait_for_open_port(3030)
    machine.succeed(f"curl --fail http://127.0.0.1:3030/boot/{system_store_path_name}/netboot.ipxe")
    machine.succeed(f"curl --fail http://127.0.0.1:3030/boot/{system_store_path_name}/bzImage > /dev/null")

    # Note: We don't generate the initrd for the system store path because
    # it kept running out of open files. This is despite setting NOFILES to ~500,000, the hard limit.
    # machine.succeed(f"curl --fail http://127.0.0.1:3030/boot/{system_store_path_name}/initrd > /dev/null")

    hello_store_path_name = machine.succeed("readlink $(which hello)").strip().split("/")[3]
    machine.succeed(f"curl --fail http://127.0.0.1:3030/boot/{hello_store_path_name}/initrd > /dev/null")
    print(machine.succeed(f"curl --fail --head http://127.0.0.1:3030/boot/{hello_store_path_name}/initrd"))
  '';
})
