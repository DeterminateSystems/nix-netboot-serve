#[allow(unused_imports)]
use futures::stream::{FuturesOrdered, FuturesUnordered, TryStreamExt};
use http::response::Builder;
use std::path::Path;
use tokio::fs;
use warp::hyper::Body;
use warp::reject;
use warp::Rejection;

use crate::dispatch::NetbootIpxeTuning;
use crate::files::open_file_stream;
use crate::webservercontext::{server_error, WebserverContext};

pub async fn serve_ipxe(
    name: String,
    tuning: NetbootIpxeTuning,
) -> Result<impl warp::Reply, Rejection> {
    let params = Path::new("/nix/store").join(&name).join("kernel-params");
    let init = Path::new("/nix/store").join(&name).join("init");
    info!("Sending netboot.ipxe: {:?}", &name);

    let response = format!(
        "#!ipxe
echo Booting NixOS closure {name}. Note: initrd may stay pre-0% for a minute or two.


kernel bzImage rdinit={init} {pre_params} {params} {post_params}
initrd initrd
boot
",
        name = &name,
        init = init.display(),
        pre_params = tuning.cmdline_prefix_args.unwrap_or("".to_string()),
        post_params = tuning.cmdline_suffix_args.unwrap_or("".to_string()),
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

    let (size, stream) = nix_cpio_generator::stream::stream(&context.cpio_cache, &store_path)
        .await
        .map_err(|e| {
            warn!("Error streaming the CPIO for {:?}: {:?}", store_path, e);
            server_error()
        })?;

    Ok(Builder::new()
        .header("Content-Length", size)
        .status(200)
        .body(Body::wrap_stream(stream)))
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
