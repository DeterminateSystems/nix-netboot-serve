# swap-to-disk
#
# Harness all the block devices on the system as part of a striped,
# encrypted swap disk.
#
# WARNING: This will erase all of the disks in the system. It is not a
# feature if it happens to not erase a specific disk. It may change
# in the future to do so.
#
# Instead of writing out a filesystem to disks or other tricks like
# an overlayfs, we can stay in the rootfs (or tmpfs with `tmpfs-root`)
# and add all the disks in the system as swap.
#
# The root filesystem is limited to the size of the disks. In other
# words, even if the `/` memory-backed filesystem is 100% full, none
# of your system's RAM will be required for holding filesystem data.
{ pkgs, ... }:
{
  systemd.services.add-disks-to-swap = {
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "oneshot";
      Restart = "on-failure";
      RestartSec = "5s";
    };
    unitConfig = {
      X-ReloadIfChanged = false;
      X-RestartIfChanged = false;
      X-StopIfChanged = false;
      X-StopOnReconfiguration = false;
      X-StopOnRemoval = false;
    };
    script = ''
      set -eux
      ${pkgs.kmod}/bin/modprobe raid0

      echo 2 > /sys/module/raid0/parameters/default_layout

      ${pkgs.util-linux}/bin/lsblk -d -e 1,7,11,230 -o PATH -n | ${pkgs.findutils}/bin/xargs ${pkgs.mdadm}/bin/mdadm /dev/md/spill.decrypted --create --level=0 --force --raid-devices=$(${pkgs.util-linux}/bin/lsblk -d -e 1,7,11,230 -o PATH -n | ${pkgs.busybox}/bin/wc -l)
      ${pkgs.cryptsetup}/bin/cryptsetup -c aes-xts-plain64 -d /dev/random create spill.encrypted /dev/md/spill.decrypted

      ${pkgs.util-linux}/bin/mkswap /dev/mapper/spill.encrypted
      ${pkgs.util-linux}/bin/swapon /dev/mapper/spill.encrypted

      size=$(${pkgs.util-linux}/bin/lsblk --noheadings --bytes --output SIZE /dev/mapper/spill.encrypted)
      pagesize=$(${pkgs.glibc.bin}/bin/getconf PAGESIZE)
      inodes=$((size / pagesize))
      ${pkgs.util-linux}/bin/mount -o remount,size=$size,nr_inodes=$inodes /
    '';
  };
}
