use std::path::PathBuf;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "nix-netboot-serve", about = "Serve up some netboots")]
pub struct Opt {
    /// Path to a directory of Nix profiles for booting
    #[structopt(long, parse(from_os_str))]
    pub profile_dir: Option<PathBuf>,

    /// Path to a Nix directory of directories of NixOS configurations
    #[structopt(long, parse(from_os_str))]
    pub config_dir: Option<PathBuf>,

    /// Path to directory to put GC roots
    #[structopt(long, parse(from_os_str))]
    pub gc_root_dir: PathBuf,

    /// Path to directory to put cached cpio files
    #[structopt(long, parse(from_os_str))]
    pub cpio_cache_dir: PathBuf,

    /// IP:port to listen on
    #[structopt(long)]
    pub listen: String,

    /// Number of open files to set the ulimit to
    #[structopt(long, default_value = "50000")]
    pub open_files: u64,

    /// Size in bytes of how much space the CPIO cache is allowed to consume
    #[structopt(long, default_value = "5368709120")] // 5 GiB in bytes
    pub max_cpio_cache_bytes: u64,
}
