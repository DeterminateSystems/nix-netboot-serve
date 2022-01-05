use std::ffi::OsString;
use std::io;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use tokio::process::Command;

pub async fn realize_path(name: String, path: &str, gc_root: &Path) -> io::Result<bool> {
    // FIXME: Two interleaving requests could make this gc root go away, letting the closure be
    // GC'd during the serve.
    let symlink = gc_root.join(&name);

    let realize = Command::new(env!("NIX_STORE_BIN"))
        .arg("--realise")
        .arg(path)
        .arg("--add-root")
        .arg(&symlink)
        .arg("--indirect")
        .status()
        .await?;

    return Ok(realize.success());
}

pub async fn get_closure_paths(path: &Path) -> io::Result<Vec<PathBuf>> {
    let output = Command::new(env!("NIX_STORE_BIN"))
        .arg("--query")
        .arg("--requisites")
        .arg(path)
        .output()
        .await?;

    let lines = output
        .stdout
        .split(|&ch| ch == b'\n')
        .filter_map(|line| {
            if line.is_empty() {
                None
            } else {
                let line = Vec::from(line);
                let line = OsString::from_vec(line);
                Some(PathBuf::from(line))
            }
        })
        .collect();

    Ok(lines)
}
