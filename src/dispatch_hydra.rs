use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use http::response::Builder;
use warp::reject;
use warp::Rejection;

use crate::dispatch::redirect_to_boot_store_path;
use crate::hydra;
use crate::nix::realize_path;
use crate::webservercontext::{server_error, WebserverContext};

pub async fn serve_hydra(
    server: String,
    project: String,
    jobset: String,
    job_name: String,
    context: WebserverContext,
) -> Result<impl warp::Reply, Rejection> {
    let job = hydra::get_latest_job(&server, &project, &jobset, &job_name)
        .await
        .map_err(|e| {
            warn!(
                "Getting the latest job from {} {}:{}:{} failed: {:?}",
                server, project, jobset, job_name, e
            );
            server_error()
        })?;

    let output = &job
        .buildoutputs
        .get("out")
        .ok_or_else(|| {
            warn!("No out for job {:?}. Got: {:?}", &job_name, job);
            reject::not_found()
        })?
        .path;

    let realize = realize_path(
        format!("{}-{}-{}-{}", &server, &project, &jobset, &job_name),
        &output,
        &context.gc_root,
    )
    .await
    .map_err(|e| {
        warn!(
            "Getting the latest job from {} {}:{}:{} failed: {:?}",
            server, project, jobset, job_name, e
        );
        server_error()
    })?;

    if realize {
        Ok(Builder::new()
            .status(302)
            .header(
                "Location",
                redirect_to_boot_store_path(Path::new(&output))?.as_bytes(),
            )
            .body(String::new()))
    } else {
        warn!("Failed to realize output {} for {:?}", &output, &job_name);
        Err(reject::not_found())
    }
}
