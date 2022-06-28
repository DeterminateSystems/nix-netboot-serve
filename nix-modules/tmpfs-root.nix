# tmpfs-root
#
# Patch the kernel to make / in the early boot environment a tmpfs
# instead of a rootfs.
#
# Without this patch, Nix and other container engines cannot use
# pivot_root.
{ pkgs, ... }: {
  boot.kernelParams = [ "nonroot_initramfs" ];
  boot.kernelPatches = [
    {
      name = "nonroot_initramfs";

      # Linux upstream unpacks the initramfs in to the rootfs directly. This breaks
      # pivot_root which breaks Nix's process of setting up the build sandbox. This
      # Nix uses pivot_root even when the sandbox is disabled.
      #
      # This patch has been upstreamed by Ignat Korchagin <ignat@cloudflare.com> before,
      # then updated by me and upstreamed again here:
      #
      # https://lore.kernel.org/all/20210914170933.1922584-2-graham@determinate.systems/T/#m433939dc30c753176404792628b9bcd64d05ed7b
      #
      # It is available on my Linux fork on GitHub:
      # https://github.com/grahamc/linux/tree/v5.15-rc1-nonroot-initramfs
      #
      # If this patch stops applying it should be fairly easy to rebase that
      # branch on future revisions of the kernel. If it stops being easy to
      # rebase, we can stop building our own kernel and take a slower approach
      # instead, proposed on the LKML: as the very first step in our init:
      #
      # 1. create a tmpfs at /root
      # 2. copy everything in / to /root
      # 3. switch_root to /root
      #
      # This takes extra time as it will need to copy everything, and it may use
      # double the memory. Unsure. Hopefully this patch is merged or applies
      # easily forever.
      patch = pkgs.fetchpatch {
        name = "nonroot_initramfs";
        url = "https://github.com/grahamc/linux/commit/65d2e9daeb2c849ad5c73f587604fec24c5cce43.patch";
        sha256 = "sha256-ERzjkick0Kzq4Zxgp6f7a+oiT3IbL05k+c9P+MdMi+h=";
      };
    }
  ];
}
