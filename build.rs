use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn main() -> Result<(), FindErr> {
    let basename = find_cmd("basename")?;
    let bash = find_cmd("bash")?;
    let cpio = find_cmd("cpio")?;
    let find = find_cmd("find")?;
    let mkdir = find_cmd("mkdir")?;
    let mktemp = find_cmd("mktemp")?;
    let nix_store = find_cmd("nix-store")?;
    let nix_build = find_cmd("nix-build")?;
    let rm = find_cmd("rm")?;
    let sort = find_cmd("sort")?;
    let zstd = find_cmd("zstd")?;

    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("make_cpio.rs");

    fs::write(
        &dest_path,
        format!(
            r##"

pub fn verify_exists() -> Result<(), String> {{
    let check = |s| {{
        let p = Path::new(s);

        if p.exists() == false {{
            warn!("Missing executable: {{:?}}", p);
            Err(format!("Missing executable: {{:?}}", p))
        }} else {{
            Ok(())
        }}
    }};

    check("{basename}")?;
    check("{bash}")?;
    check("{cpio}")?;
    check("{find}")?;
    check("{mkdir}")?;
    check("{mktemp}")?;
    check("{nix_store}")?;
    check("{rm}")?;
    check("{sort}")?;
    check("{zstd}")?;
    Ok(())
}}

pub fn cpio_command(src_path: &Path, dest_path: &Path) -> Command {{
    let mut cmd = Command::new("{bash}");
    cmd.arg("-c");
    cmd.arg(r#"
set -eu
set -o pipefail

source=$1
dest=$2
export PATH="/dev/null"

scratch=$({mktemp} -d -t tmp.XXXXXXXXXX)
function finish {{
  {rm} -rf "$scratch"
}}
trap finish EXIT

{{
    {{
        # Make the cpio for the store path
        cd /
        {find} ".$source" -print0 \
            | {sort} -z \
            | {cpio} -o -H newc -R +0:+1 --reproducible --null
    }}

    {{
        # Make a new file at /nix/.nix-netboot-serve-db/registration/<<basename store-path>> so we can register it later
        cd "$scratch"
        regdir="./nix/.nix-netboot-serve-db/registration/"
        regpath="$regdir/$({basename} "$source")"
        {mkdir} -p "$regdir"
        {nix_store} --dump-db "$source" > "$regpath"

        {find} "$regpath" -print0 \
            | {sort} -z \
            | {cpio} -o -H newc -R +0:+1 --reproducible --null
    }}
}} | {zstd} --compress --stdout -10 > "$2"
"#);
    cmd.arg("--");
    cmd.arg(src_path);
    cmd.arg(dest_path);

    cmd
}}
        "##,
            basename = basename.to_str().expect("basename path is not utf8 clean"),
            bash = bash.to_str().expect("bash path is not utf8 clean"),
            cpio = cpio.to_str().expect("cpio path is not utf8 clean"),
            find = find.to_str().expect("find path is not utf8 clean"),
            mkdir = mkdir.to_str().expect("mkdir path is not utf8 clean"),
            mktemp = mktemp.to_str().expect("mktemp path is not utf8 clean"),
            nix_store = nix_store.to_str().expect("nix_store path is not utf8 clean"),
            rm = rm.to_str().expect("basename path is not utf8 clean"),
            sort = sort.to_str().expect("sort path is not utf8 clean"),
            zstd = zstd.to_str().expect("zstd path is not utf8 clean"),
        ),
    )
    .unwrap();

    println!("cargo:rustc-env=NIX_STORE_BIN={}", nix_store.to_str().expect("nix_store path is not utf8 clean"));
    println!("cargo:rustc-env=NIX_BUILD_BIN={}", nix_build.to_str().expect("nix_build path is not utf8 clean"));
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
