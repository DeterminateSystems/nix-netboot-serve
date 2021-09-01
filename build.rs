use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn main() -> Result<(), FindErr> {
    let cpio = find_cmd("cpio")?;
    let find = find_cmd("find")?;
    let bash = find_cmd("bash")?;
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

    check("{cpio}")?;
    check("{find}")?;
    check("{bash}")?;
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
cd /
{find} ".$1" -print0 \
        | {sort} -z \
        | {cpio} -o -H newc -R +0:+1 --reproducible --null \
        | {zstd} --compress --stdout -19 > "$2"
"#);
    cmd.arg("--");
    cmd.arg(src_path);
    cmd.arg(dest_path);

    cmd
}}
        "##,
            cpio = cpio.to_str().expect("cpio path is not utf8 clean"),
            find = find.to_str().expect("find path is not utf8 clean"),
            bash = bash.to_str().expect("bash path is not utf8 clean"),
            sort = sort.to_str().expect("sort path is not utf8 clean"),
            zstd = zstd.to_str().expect("zstd path is not utf8 clean"),
        ),
    )
    .unwrap();

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
