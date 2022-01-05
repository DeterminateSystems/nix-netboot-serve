use std::ffi::OsStr;
use std::io;
use std::path::Path;
use tokio::fs::File;
use tokio::io::BufReader;
use tokio_stream::Stream;
use tokio_util::io::ReaderStream;
use warp::hyper::body::Bytes;

pub async fn open_file_stream(
    path: &Path,
) -> std::io::Result<impl Stream<Item = io::Result<Bytes>>> {
    let file = File::open(path).await?;

    Ok(ReaderStream::new(BufReader::new(file)))
}

pub fn basename(path: &Path) -> Option<&OsStr> {
    if let Some(std::path::Component::Normal(pathname)) = path.components().last() {
        Some(pathname)
    } else {
        None
    }
}
