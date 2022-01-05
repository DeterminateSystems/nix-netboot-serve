use http::response::Builder;
use std::os::unix::ffi::OsStrExt;

use tokio::process::Command;
use warp::reject;
use warp::Rejection;

use crate::dispatch::redirect_symlink_to_boot;
use crate::webservercontext::{feature_disabled, server_error, WebserverContext};

pub async fn serve_configuration(
    name: String,
    context: WebserverContext,
) -> Result<impl warp::Reply, Rejection> {
    let config = context
        .configuration_dir
        .as_ref()
        .ok_or_else(|| feature_disabled("Configuration booting is not configured on this server."))?
        .join(&name)
        .join("default.nix");

    if !config.is_file() {
        println!(
            "Configuration {} resolves to {:?} which is not a file",
            name, config
        );
        return Err(reject::not_found());
    }

    // TODO: not thread safe sorta, but kinda is, unless the config
    // changes between two boots. I'm definitely going to regret this.
    let symlink = context.gc_root.join(&name);

    let build = Command::new(env!("NIX_BUILD_BIN"))
        .arg(&config)
        .arg("--out-link")
        .arg(&symlink)
        .status()
        .await
        .map_err(|e| {
            warn!(
                "Executing nix-build on {:?} failed at some fundamental level: {:?}",
                config, e
            );
            server_error()
        })?;

    if !build.success() {
        return Ok(Builder::new().status(200).body(format!(
            "#!ipxe

echo Failed to render the configuration.
echo Will retry in 5s, press enter to retry immediately.

menu Failed to render the configuration. Will retry in 5s, or press enter to retry immediately.
item gonow Retry now
choose --default gonow --timeout 5000 shouldwedoit

chain /dispatch/configuration/{}",
            name
        )));
    }

    Ok(Builder::new()
        .status(302)
        .header("Location", redirect_symlink_to_boot(&symlink)?.as_bytes())
        .body(String::new()))
}
