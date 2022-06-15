use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::{convert::TryInto, fs::File};

use cpio::{newc, write_cpio};
use lazy_static::lazy_static;
use walkdir::WalkDir;

use crate::files::basename;

#[cfg(unix)]
fn make_archive_from_dir(path: PathBuf, out_path: &OsStr) -> std::io::Result<()> {
    let dir = WalkDir::new(path)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            let meta = entry.metadata();
            meta.map(|x| x.is_file()).unwrap_or(false)
        })
        .filter_map(|entry| {
            let entry_name = entry.path().to_str()?;
            let meta = entry.metadata().ok()?;
            let built = newc::Builder::new(entry_name)
                .dev_major(meta.dev().try_into().ok()?)
                .rdev_major(meta.rdev().try_into().ok()?)
                .gid(meta.gid())
                .uid(meta.uid())
                .ino(meta.ino().try_into().ok()?)
                .nlink(meta.nlink().try_into().ok()?)
                .mode(meta.mode())
                .mtime(meta.mtime().try_into().ok()?);
            let readable_file = File::open(entry.path()).ok()?;
            Some((built, readable_file))
        });
    let out_archive = File::create(out_path)?;
    write_cpio(dir, out_archive)?;
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
        process::{Command},
    };
    use tempfile::NamedTempFile;

    use super::make_archive_from_dir;

    #[test]
    fn test_single_file_archive() -> Result<(), Box<dyn Error>> {
        let mut file = NamedTempFile::new()?;
        let archive = NamedTempFile::new()?;
        write!(file, "Hello cpio!")?;
        make_archive_from_dir(file.path().to_path_buf(), archive.path().as_os_str())?;
        let mut command = Command::new("sh");
        command.args([
            "-c",
            format!("cpio -iv < {:}", archive.path().display()).as_str(),
        ]);
        command.current_dir("/tmp");
        remove_file(file.path())?;
        dbg!(&command);
        let out = command.output()?;
        assert_eq!(out.status.success(), true);
        dbg!(String::from_utf8(out.stdout)?);
        dbg!(String::from_utf8(out.stderr)?);
        let read_text = read_to_string(file.path())?;
        dbg!(&read_text);
        assert_eq!(read_text, "Hello cpio!");
        remove_file(archive.path())?;
        Ok(())
    }

    #[test]
    fn test_multiple_file_archive() -> Result<(), Box<dyn Error>> {
        let base_dir = tempfile::Builder::new().prefix("cpio-test").tempdir()?;
        let file1_path = base_dir.path().join("test1");
        let file2_path = base_dir.path().join("test2");
        let mut file = File::create(&file1_path)?;
        let mut file2 = File::create(&file2_path)?;
        let archive = NamedTempFile::new()?;
        write!(file, "Hello cpio!")?;
        write!(file2, "Hello cpio2!")?;
        make_archive_from_dir(
            file1_path.parent().ok_or("No Parent?")?.to_path_buf(),
            archive.path().as_os_str(),
        )?;
        let mut command = Command::new("sh");
        command.args([
            "-c",
            format!("cpio -iv < {:}", archive.path().display()).as_str(),
        ]);
        command.current_dir("/tmp");
        remove_file(&file1_path)?;
        remove_file(&file2_path)?;
        dbg!(&command);
        let out = command.output()?;
        assert_eq!(out.status.success(), true);
        dbg!(String::from_utf8(out.stdout)?);
        dbg!(String::from_utf8(out.stderr)?);
        let read_text = read_to_string(&file1_path)?;
        let read_text2 = read_to_string(&file2_path)?;
        dbg!(&read_text);
        dbg!(&read_text2);
        assert_eq!(read_text, "Hello cpio!");
        assert_eq!(read_text2, "Hello cpio2!");
        remove_file(archive.path())?;
        remove_file(&file1_path)?;
        remove_file(&file2_path)?;
        Ok(())
    }

    #[test]
    fn test_multiple_nested_file_archive() {}
}
