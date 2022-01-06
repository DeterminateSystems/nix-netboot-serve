#[allow(unused_imports)]
use futures::stream::{FuturesOrdered, FuturesUnordered, TryStreamExt};
use futures::StreamExt;
use http::response::Builder;
use std::convert::TryInto;
use std::path::Path;
use tokio::fs;
use warp::hyper::Body;
use warp::reject;
use warp::Rejection;

use crate::cpio::{make_load_cpio, LEADER_CPIO_BYTES, LEADER_CPIO_LEN};
use crate::files::open_file_stream;
use crate::nix::get_closure_paths;
use crate::webservercontext::{server_error, WebserverContext};

pub async fn serve_ipxe(name: String) -> Result<impl warp::Reply, Rejection> {
    let params = Path::new("/nix/store").join(&name).join("kernel-params");
    let init = Path::new("/nix/store").join(&name).join("init");
    info!("Sending netboot.ipxe: {:?}", &name);

    let response = format!(
        "#!ipxe
echo Booting NixOS closure {name}. Note: initrd may stay pre-0% for a minute or two.


kernel bzImage rdinit={init} {params}
initrd initrd
boot
",
        name = &name,
        init = init.display(),
        params = fs::read_to_string(&params).await.map_err(|e| {
            warn!(
                "Failed to load parameters from the generation at {:?}: {:?}",
                params, e
            );
            server_error()
        })?
    );

    Ok(Builder::new().status(200).body(response))
}

pub async fn serve_initrd(
    name: String,
    context: WebserverContext,
) -> Result<impl warp::Reply, Rejection> {
    let store_path = Path::new("/nix/store").join(name);
    info!("Sending closure: {:?}", &store_path);

    let closure_paths = get_closure_paths(&store_path).await.map_err(|e| {
        warn!("Error calculating closure for {:?}: {:?}", store_path, e);
        server_error()
    })?;

    let mut cpio_makers = closure_paths
        .to_owned()
        .into_iter()
        .map(|path| async { context.cpio_cache.dump_cpio(path).await })
        .collect::<FuturesUnordered<_>>();

    let mut size: u64 = 0;
    let mut readers: Vec<_> = vec![];

    while let Some(result) = cpio_makers.next().await {
        let cpio = result.map_err(|e| {
            error!("Failure generating a CPIO: {:?}", e);
            server_error()
        })?;
        size += cpio.size();
        readers.push(cpio);
    }

    readers.sort_unstable_by(|left, right| {
        left.path()
            .partial_cmp(right.path())
            .expect("Sorting &Path should have no chance for NaNs, thus no unwrap")
    });

    let mut streams = FuturesOrdered::new();
    for cpio in readers.into_iter() {
        streams.push(async {
            trace!("Handing over the reader for {:?}", cpio.path());
            Ok::<_, std::io::Error>(cpio.reader_stream().await.map_err(|e| {
                error!("Failed to get a reader stream: {:?}", e);
                e
            })?)
        });
    }

    let size = size
        .checked_add(*LEADER_CPIO_LEN)
        .expect("Failed to sum the leader length with the total initrd size");
    let leader_stream = futures::stream::once(async {
        Ok::<_, std::io::Error>(warp::hyper::body::Bytes::from_static(&LEADER_CPIO_BYTES))
    });

    let store_loader = make_load_cpio(&closure_paths).map_err(|e| {
        error!("Failed to generate a load CPIO: {:?}", e);
        server_error()
    })?;
    let size = size
        .checked_add(
            store_loader
                .len()
                .try_into()
                .expect("Failed to convert a usize to u64"),
        )
        .expect("Failed to sum the loader length with the total initrd size");

    let body_stream = leader_stream
        .chain(futures::stream::once(async move {
            Ok::<_, std::io::Error>(warp::hyper::body::Bytes::from(store_loader))
        }))
        .chain(streams.try_flatten());

    Ok(Builder::new()
        .header("Content-Length", size)
        .status(200)
        .body(Body::wrap_stream(body_stream)))
}

pub async fn serve_kernel(name: String) -> Result<impl warp::Reply, Rejection> {
    let kernel = Path::new("/nix/store").join(name).join("kernel");
    info!("Sending kernel: {:?}", kernel);

    let read_stream = open_file_stream(&kernel).await.map_err(|e| {
        warn!("Failed to serve kernel {:?}: {:?}", kernel, e);
        reject::not_found()
    })?;

    Ok(Builder::new()
        .status(200)
        .body(Body::wrap_stream(read_stream)))
}
