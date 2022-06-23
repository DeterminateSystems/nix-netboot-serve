use std::path::PathBuf;
use std::process::{Command, Stdio};

fn main() -> Result<(), FindErr> {
    let nix_store = find_cmd("nix-store")?;
    let nix_build = find_cmd("nix-build")?;

    println!(
        "cargo:rustc-env=NIX_BUILD_BIN={}",
        nix_build
            .to_str()
            .expect("nix_build path is not utf8 clean")
    );
    println!(
        "cargo:rustc-env=NIX_STORE_BIN={}",
        nix_store
            .to_str()
            .expect("nix_store path is not utf8 clean")
    );
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PATH");

    Ok(())
}

fn find_cmd(cmd: &str) -> Result<PathBuf, FindErr> {
    eprintln!("Trying to find {:?}...", cmd);
    let output = Command::new("which")
        .arg(&cmd)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| FindErr::Io(e))?;

    if !output.status.success() {
        return Err(FindErr::Missing(cmd.to_string()));
    }

    let stdout = String::from_utf8(output.stdout).map_err(|e| FindErr::NotUtf8(e))?;

    let path = PathBuf::from(stdout.trim());
    eprintln!("\tCommand `{}` is supposedly at {:?}...", cmd, path);

    if !path.exists() {
        eprintln!("\tBut it does not appear to exist at {:?}...", path);
        return Err(FindErr::Missing(cmd.to_string()));
    }

    if path.is_file() {
        eprintln!("\t{:?} is a file, returning that location.", path);
        return Ok(path);
    }

    eprintln!("\tTrying to resolve {:?} as a symlink.", path);
    let path = path.read_link().map_err(|e| FindErr::Io(e))?;

    if !path.exists() {
        eprintln!("\tBut it does not appear to exist at {:?}...", path);
        return Err(FindErr::Missing(cmd.to_string()));
    }

    Ok(path)
}

#[derive(Debug)]
enum FindErr {
    Io(std::io::Error),
    NotUtf8(std::string::FromUtf8Error),
    Missing(String),
}
