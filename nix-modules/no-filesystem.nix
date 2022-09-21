{
  fileSystems."/" = {
    device = "/dev/bogus";
    fsType = "ext4";
  };
  boot.loader.grub.enable = false;
}
