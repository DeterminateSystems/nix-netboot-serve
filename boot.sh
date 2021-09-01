

#url=http://127.0.0.1:3030/boot/0m60ngchp6ki34jpwmpbdx3fby6ya0sf-nixos-system-nginx-21.11pre307912.fe01052444c/netboot.ipxe
#url=http://127.0.0.1:3030/dispatch/profile/amazon-image
url=http://127.0.0.1:3030/dispatch/configuration/m1.small

qemu-kvm \
  -enable-kvm \
  -m 16G \
  -cpu max \
  -serial mon:stdio \
  -net user,bootfile="$url" \
  -net nic \
  -msg timestamp=on \
  -nographic
