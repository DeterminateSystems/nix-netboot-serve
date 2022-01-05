use std::os::unix::ffi::OsStrExt;

use http::response::Builder;
use warp::Rejection;

use crate::dispatch::redirect_symlink_to_boot;
use crate::webservercontext::{feature_disabled, WebserverContext};

pub async fn serve_profile(
    name: String,
    context: WebserverContext,
) -> Result<impl warp::Reply, Rejection> {
    let symlink = context
        .profile_dir
        .as_ref()
        .ok_or_else(|| feature_disabled("Profile booting is not configured on this server."))?
        .join(&name);

    Ok(Builder::new()
        .status(302)
        .header("Location", redirect_symlink_to_boot(&symlink)?.as_bytes())
        .body(String::new()))
}
