use std::ffi::OsString;
use std::io::{Cursor, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::{convert::TryInto, fs::File};

use cpio::{newc, write_cpio};
use lazy_static::lazy_static;
use tokio::process::Command;
use walkdir::WalkDir;

use crate::files::basename;

pub trait ReadSeek: std::io::Read + std::io::Seek {}
impl<T: std::io::Read + std::io::Seek> ReadSeek for T {}

#[cfg(unix)]
pub fn make_archive_from_dir<W>(root: &Path, path: &Path, out: W) -> std::io::Result<()>
where
    W: Write,
{
    path.strip_prefix(&root).unwrap_or_else(|_| {
        panic!("Path {:?} is not inside root ({:?})", path, &root);
    });

    let dir = WalkDir::new(path)
        .sort_by(|a, b| a.file_name().cmp(b.file_name()))
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let entry_name = entry
                .path()
                .strip_prefix(&root)
                .unwrap_or_else(|_| {
                    panic!("Path {:?} is not inside root ({:?})", entry.path(), &root)
                })
                .to_str()?;
            let meta = entry.metadata().ok()?;
            let built = newc::Builder::new(entry_name)
                .dev_major(0)
                .rdev_major(0)
                .gid(1)
                .uid(0)
                .ino(meta.ino().try_into().ok()?)
                .nlink(meta.nlink().try_into().ok()?)
                .mode(meta.mode())
                .mtime(1);

            let readable_file: Box<dyn ReadSeek> = if meta.is_file() {
                Box::new(File::open(entry.path()).ok()?)
            } else if entry.path().is_symlink() {
                Box::new(Cursor::new(
                    entry
                        .path()
                        .read_link()
                        .expect("TOCTTOU: is_symlink said this is a symlink")
                        .into_os_string()
                        .into_vec(),
                ))
            } else {
                Box::new(std::io::empty())
            };

            Some((built, readable_file))
        });
    write_cpio(dir, out)?;
    Ok(())
}

fn make_leader_cpio() -> std::io::Result<Vec<u8>> {
    let mut leader_cpio = std::io::Cursor::new(vec![]);
    cpio::write_cpio(
        vec![
            // mode for a directory: 0o40000 + 0o00xxx for its permission bits
            // nlink for directories == the number of things in it plus 2 (., ..)
            (
                cpio::newc::Builder::new(".").mode(0o40755).nlink(3),
                std::io::empty(),
            ),
            (
                cpio::newc::Builder::new("nix").mode(0o40755).nlink(3),
                std::io::empty(),
            ),
            (
                cpio::newc::Builder::new("nix/store")
                    .mode(0o40775)
                    .nlink(2)
                    .uid(0)
                    .gid(30000),
                std::io::empty(),
            ),
            (
                cpio::newc::Builder::new("nix/.nix-netboot-serve-db")
                    .mode(0o40755)
                    .nlink(3),
                std::io::empty(),
            ),
            (
                cpio::newc::Builder::new("nix/.nix-netboot-serve-db/registration")
                    .mode(0o40755)
                    .nlink(2),
                std::io::empty(),
            ),
        ]
        .into_iter(),
        &mut leader_cpio,
    )?;

    Ok(leader_cpio.into_inner())
}

pub async fn make_registration<W>(path: &Path, dest: &mut W) -> Result<(), MakeRegistrationError>
where
    W: Write,
{
    let out = Command::new(env!("NIX_STORE_BIN"))
        .arg("--dump-db")
        .arg(&path)
        .output()
        .await
        .map_err(MakeRegistrationError::Exec)?;
    if !out.status.success() {
        return Err(MakeRegistrationError::DumpDb(out.stderr));
    }

    let filename = path
        .file_name()
        .ok_or(MakeRegistrationError::NoFilename)?
        .to_str()
        .ok_or(MakeRegistrationError::FilenameInvalidUtf8)?;

    cpio::write_cpio(
        vec![(
            cpio::newc::Builder::new(&format!(
                "nix/.nix-netboot-serve-db/registration/{}",
                filename
            ))
            .mode(0o0100500)
            .nlink(1),
            std::io::Cursor::new(out.stdout),
        )]
        .into_iter(),
        dest,
    )
    .map_err(MakeRegistrationError::Io)?;

    Ok(())
}

#[derive(Debug)]
pub enum MakeRegistrationError {
    Exec(std::io::Error),
    DumpDb(Vec<u8>),
    Io(std::io::Error),
    NoFilename,
    FilenameInvalidUtf8,
}

pub fn make_load_cpio(paths: &Vec<PathBuf>) -> Result<Vec<u8>, LoadCpioError> {
    let script = paths
        .iter()
        .map(|p| {
            let mut line =
                OsString::from("nix-store --load-db < /nix/.nix-netboot-serve-db/registration/");
            line.push(basename(p).ok_or_else(|| LoadCpioError::NoBasename(p.to_path_buf()))?);
            Ok(line)
        })
        .collect::<Result<Vec<OsString>, LoadCpioError>>()?
        .into_iter()
        .fold(OsString::from("#!/bin/sh"), |mut acc, line| {
            acc.push("\n");
            acc.push(line);
            acc
        });
    let mut loader = std::io::Cursor::new(vec![]);
    cpio::write_cpio(
        vec![(
            cpio::newc::Builder::new("nix/.nix-netboot-serve-db/register")
                .mode(0o0100500)
                .nlink(1),
            std::io::Cursor::new(script.as_bytes()),
        )]
        .into_iter(),
        &mut loader,
    )
    .map_err(LoadCpioError::Io)?;

    Ok(loader.into_inner())
}

#[derive(Debug)]
pub enum LoadCpioError {
    Io(std::io::Error),
    NoBasename(PathBuf),
}

lazy_static! {
    pub static ref LEADER_CPIO_BYTES: Vec<u8> =
        make_leader_cpio().expect("Failed to generate the leader CPIO.");
    pub static ref LEADER_CPIO_LEN: u64 = LEADER_CPIO_BYTES
        .len()
        .try_into()
        .expect("Failed to convert usize leader length to u64");
}

#[cfg(test)]
mod tests {
    use std::{
        error::Error,
        fs::{read_to_string, remove_file, File},
        io::Write,
        process::Command,
    };
    use tempfile::NamedTempFile;

    use super::make_archive_from_dir;

    #[test]
    fn test_single_file_archive() -> Result<(), Box<dyn Error>> {
        let mut file = NamedTempFile::new()?;
        let archive = NamedTempFile::new()?;
        write!(file, "Hello cpio!")?;
        make_archive_from_dir(
            file.path().parent().unwrap(),
            file.path(),
            File::create(archive.path())?,
        )?;
        let mut command = Command::new("sh");
        command.args(["-c", "cpio -iv < \"$1\"", "--"]);
        command.arg(archive.path());
        command.current_dir(
            archive
                .path()
                .parent()
                .expect("Don't make / your tmp please"),
        );
        remove_file(file.path())?;
        let out = command.output()?;
        assert!(out.status.success());
        let read_text = read_to_string(file.path())?;
        assert_eq!(read_text, "Hello cpio!");
        remove_file(archive.path())?;
        Ok(())
    }
}
