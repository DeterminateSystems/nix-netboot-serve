# nix-netboot-serve

Dynamically generate netboot images for arbitrary NixOS system closures,
profiles, or configurations with 30s iteration times.

## Usage

Make sure you run it with a very high number of open files:

```
ulimit -Sn 50000
```

Then create working directories for it:

```
mkdir ./gc-roots ./profiles ./configurations ./cpio-cache
```

Then start up the server:

```
RUST_LOG=info cargo run -- --gc-root-dir ./gc-roots --config-dir ./configurations --profile-dir ./profiles/ --cpio-cache-dir ./cpio-cache/
```

See `./boot.sh` for an example of booting with QEMU.

## Booting an absolute closure

### How To

To boot from a specific closure like
`/nix/store/0m60ngchp6ki34jpwmpbdx3fby6ya0sf-nixos-system-nginx-21.11pre307912.fe01052444c`,
use `/boot/0m60ngchp6ki34jpwmpbdx3fby6ya0sf-nixos-system-nginx-21.11pre307912.fe01052444c/netboot.ipxe`
as your chain url.

## Behavior

As long as that closure exists on the host, that closure will always
be booted, unchanged.

## Booting a profile

### How To

In the `profiles` directory, create symlinks to top level system paths.
For example:

```console
$ ls -la profiles/
example-host -> /nix/store/4y829p7lljdvwnmsk6pnig3mlh6ygklj-nixos-system-example-host-21.11pre130979.gfedcba
```

then use `/dispatch/profile/example-host` to boot it.

### Behavior

The symlink will be resolved every time a machine boots.

## Booting a configuration

### How To
In the `configurations` directory, create a directory for each system,
and create a `default.nix` inside. For example:

```console
$ tree configurations/
configurations/
└── m1.small
    └── default.nix
```

In the `default.nix`, create an expression with your NixOS configuration
ready to be built:

```nix
(import <nixpkgs/nixos> {
    configuration = { pkgs, ... }: {
        networking.hostName = "m1small";
        environment.systemPackages = [ pkgs.hello ];
        fileSystems."/" = {
            device = "/dev/bogus";
            fsType = "ext4";
        };
        boot.loader.grub.devices = [ "/dev/bogus" ];
    };
}).system
```

Then use `/dispatch/configuration/m1.small` to boot it.

### Behavior

The configuration will be `nix-build` once per boot, and create a symlink
in the `--gc-root-dir` directory with the same name as the configuration.

If the build fails, the ipxe client will be told to retry in 5s.

Note: there is currently a buggy race condition. In the following circumstance:

1. machine A turns on
1. machine B turns on
1. machine A hits the build URL and a long build starts
1. you change the configuration to have a very short build
1. machine B hits the build URL and the short build starts
1. machine B's configuration finishes building
1. machine B boots the short build configuration
1. machine A's configuration finishes building
1. machine A boots the **short configuration** instead of the long configuration

## Notes on NixOS Configuration

Booting a machine from this server will completely ignore any of the
defined `fileSystems`, everything will run out of RAM.

This system assumes a _normal_ NixOS system booting off a regular disk:
trying to use this to netboot a USB installer _will not work_.

If you don't have an existing configuration to start with, you could
start with this:

```nix
{
    fileSystems."/" = {
        device = "/dev/bogus";
        fsType = "ext4";
    };
    boot.loader.grub.devices = [ "/dev/bogus" ];
}
```

## Theory of Operation

Linux's boot process starts with two things:

1. the kernel
1. an initrd, or an initial ram disk

The ramdisk has all the files needed to mount any disks and start any
software needed for the machine. Typically the ramdisk is constructed
of a [CPIO](https://en.wikipedia.org/wiki/Cpio), a very simple file
archive.

Linux supports a special case of its initrd being comprised of
_multiple_ CPIOs. By simply concatenating two CPIOs together,
Linux's boot process will see the merged contents of both CPIOs.

Furthermore, individual CPIOs can be compressed independently,
merged together with concatenation, and Linux will decompress
and read each CPIO independently.

A NixOS system is comprised of hundreds of independent, immutable
`/nix/store` paths.

Merging these together, we can dynamically create a single, compressed
CPIO per Nix store path and cache it for later.

When a new boot request comes in, the software fetches the list of
Nix store paths for the requested NixOS system. Then, every path
has a CPIO built for it. Once each store path has a CPIO, the results
are streamed back to the iPXE client. By caching the resulting CPIO,
iterative development on a system configuration can result in just
3-5 new CPIOs per change.

## Improvements over NixOS's NetBoot Support

NixOS's NetBoot image creation support works well, however iterating
on a single closure involves recreating the CPIO and recompressing for
every store path every time. This can add several minutes to cycle
time.