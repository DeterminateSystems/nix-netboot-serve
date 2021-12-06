let pkgs = import <nixpkgs> {}; in
pkgs.mkShell {
    buildInputs = [
        pkgs.cargo 
        pkgs.rustfmt
        pkgs.vim # xxd
        pkgs.qemu
        pkgs.file
        pkgs.entr
        pkgs.binwalk
        pkgs.openssl
        pkgs.pkgconfig
    ];
}
