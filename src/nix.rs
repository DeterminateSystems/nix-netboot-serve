use std::io;
use std::path::Path;

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
