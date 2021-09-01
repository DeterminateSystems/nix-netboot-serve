use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tempfile::NamedTempFile;
use tokio::fs::File;
use tokio::io::BufReader;
use tokio::process::Command;
use tokio_util::io::ReaderStream;

include!(concat!(env!("OUT_DIR"), "/make_cpio.rs"));

#[derive(Clone)]
pub struct CpioCache {
    cache_dir: PathBuf,
    cache: Arc<RwLock<HashMap<PathBuf, Cpio>>>,
}

impl CpioCache {
    pub fn new(cache_dir: PathBuf) -> Result<Self, String> {
        verify_exists()?;

        Ok(Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            cache_dir,
        })
    }

    pub async fn dump_cpio(&self, path: PathBuf) -> Result<Cpio, CpioError> {
        if let Some(cpio) = self.get_cached(&path) {
            info!("Found CPIO in the memory cache {:?}", path);
            return Ok(cpio);
        } else if let Ok(cpio) = self.get_directory_cached(&path).await {
            info!("Found CPIO in the directory cache {:?}", path);
            return Ok(cpio);
        } else {
            info!("Making a new CPIO for {:?}", path);
            self.make_cpio(&path).await
        }
    }

    fn get_cached(&self, path: &Path) -> Option<Cpio> {
        self.cache
            .read()
            .expect("Failed to get a read lock on the cpio cache")
            .get(path)
            .map(|entry| entry.clone())
    }

    async fn get_directory_cached(&self, path: &Path) -> Result<Cpio, CpioError> {
        let cached_location = self.cache_path(&path)?;
        let cpio = Cpio::new(cached_location.clone())
            .await
            .map_err(|e| CpioError::Io {
                src: path.clone().to_path_buf(),
                dest: cached_location,
                e: e,
            })?;

        self.cache
            .write()
            .expect("Failed to get a write lock on the cpio cache")
            .insert(path.clone().to_path_buf(), cpio.clone());

        Ok(cpio)
    }

    async fn make_cpio(&self, path: &Path) -> Result<Cpio, CpioError> {
        let final_dest = self.cache_path(&path)?;
        let temp_dest = NamedTempFile::new_in(&self.cache_dir).map_err(|e| CpioError::Io {
            src: path.clone().to_path_buf(),
            dest: final_dest.clone(),
            e: e,
        })?;

        trace!(
            "Constructing CPIO for {:?} at {:?}, to be moved to {:?}",
            &path,
            &temp_dest,
            &final_dest
        );
        let status = cpio_command(&path, &temp_dest.path())
            .status()
            .await
            .map_err(|e| CpioError::Io {
                src: path.clone().to_path_buf(),
                dest: final_dest.clone(),
                e: e,
            })?;

        if !status.success() {
            warn!("Failed to generate a CPIO for {:?}: {:?}", path, status);
            return Err(CpioError::Failed);
        }

        temp_dest.persist(&final_dest).map_err(|e| CpioError::Io {
            src: path.clone().to_path_buf(),
            dest: final_dest.clone(),
            e: e.error,
        })?;

        self.get_directory_cached(&path).await
    }

    fn cache_path(&self, src: &Path) -> Result<PathBuf, CpioError> {
        if let Some(std::path::Component::Normal(pathname)) = src.components().last() {
            let mut cache_name = OsString::from(pathname);
            cache_name.push(".cpio.zstd");

            Ok(self.cache_dir.join(cache_name))
        } else {
            Err(CpioError::Uncachable(format!(
                "Cannot calculate a cache path for: {:?}",
                src
            )))
        }
    }
}

pub struct Cpio {
    size: u64,
    file: Option<File>,
    path: PathBuf,
}

impl Clone for Cpio {
    fn clone(&self) -> Self {
        Cpio {
            size: self.size,
            file: None,
            path: self.path.clone(),
        }
    }
}

impl Cpio {
    pub async fn new(path: PathBuf) -> std::io::Result<Self> {
        let file = File::open(&path).await?;
        let metadata = file.metadata().await?;

        Ok(Self {
            size: metadata.len(),
            file: Some(file),
            path,
        })
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn reader_stream(
        mut self,
    ) -> std::io::Result<ReaderStream<tokio::io::BufReader<tokio::fs::File>>> {
        Ok(ReaderStream::new(BufReader::new(self.handle().await?)))
    }

    async fn handle(&mut self) -> std::io::Result<tokio::fs::File> {
        match self.file.take() {
            Some(handle) => Ok(handle),
            None => Ok(File::open(&self.path).await?),
        }
    }
}

#[derive(Debug)]
pub enum CpioError {
    Io {
        src: PathBuf,
        dest: PathBuf,
        e: std::io::Error,
    },
    Uncachable(String),
    Failed,
}
