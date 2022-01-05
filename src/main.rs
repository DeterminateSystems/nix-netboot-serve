use http::response::Builder;
use std::ffi::OsString;
use std::net::SocketAddr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use structopt::StructOpt;
use tokio::process::Command;
use warp::reject;
use warp::Filter;
use warp::Rejection;

#[macro_use]
extern crate log;

mod boot;
mod cpio;
mod cpio_cache;
mod files;
mod hydra;
mod nix;
mod nofiles;
mod options;
mod webservercontext;
use crate::boot::{serve_initrd, serve_ipxe, serve_kernel};
use crate::cpio_cache::CpioCache;
use crate::nix::realize_path;
use crate::nofiles::set_nofiles;
use crate::options::Opt;
use crate::webservercontext::{server_error, with_context, WebserverContext};

fn feature_disabled(msg: &str) -> Rejection {
    warn!("Feature disabled: {}", msg);
    reject::not_found()
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    let opt = Opt::from_args();

    set_nofiles(opt.open_files).expect("Failed to set ulimit for the number of open files");

    let check_dir_exists = |path: PathBuf| {
        if !path.is_dir() {
            error!("Directory does not exist: {:?}", path);
            panic!();
        }

        path
    };

    let webserver = WebserverContext {
        profile_dir: opt.profile_dir.map(check_dir_exists),
        configuration_dir: opt.config_dir.map(check_dir_exists),
        gc_root: check_dir_exists(opt.gc_root_dir),
        cpio_cache: CpioCache::new(check_dir_exists(opt.cpio_cache_dir))
            .expect("Cannot construct a CPIO Cache"),
    };

    let root = warp::path::end().map(|| "nix-netboot-serve");
    let profile = warp::path!("dispatch" / "profile" / String)
        .and(with_context(webserver.clone()))
        .and_then(serve_profile);
    let configuration = warp::path!("dispatch" / "configuration" / String)
        .and(with_context(webserver.clone()))
        .and_then(serve_configuration);
    let hydra = warp::path!("dispatch" / "hydra" / String / String / String / String)
        .and(with_context(webserver.clone()))
        .and_then(serve_hydra);
    let ipxe = warp::path!("boot" / String / "netboot.ipxe").and_then(serve_ipxe);
    let initrd = warp::path!("boot" / String / "initrd")
        .and(with_context(webserver.clone()))
        .and_then(serve_initrd);
    let kernel = warp::path!("boot" / String / "bzImage").and_then(serve_kernel);

    let routes = warp::get()
        .and(
            root.or(profile)
                .or(configuration)
                .or(hydra)
                .or(ipxe)
                .or(initrd.clone())
                .or(kernel),
        )
        .or(warp::head().and(initrd));

    warp::serve(routes)
        .run(
            opt.listen
                .parse::<SocketAddr>()
                .expect("Failed to parse the listen argument"),
        )
        .await;
}

async fn serve_configuration(
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

async fn serve_profile(
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

async fn serve_hydra(
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
            warn!("No out for job {:?}", &job_name);
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
        warn!("No out for job {:?}", &job_name);
        Err(reject::not_found())
    }
}

fn redirect_symlink_to_boot(symlink: &Path) -> Result<OsString, Rejection> {
    let path = symlink.read_link().map_err(|e| {
        warn!("Reading the link {:?} failed with: {:?}", symlink, e);
        reject::not_found()
    })?;

    trace!("Resolved symlink {:?} to {:?}", symlink, path);
    redirect_to_boot_store_path(&path)
}

fn redirect_to_boot_store_path(path: &Path) -> Result<OsString, Rejection> {
    if !path.exists() {
        warn!("Path does not exist: {:?}", &path);
        return Err(reject::not_found());
    }

    if let Some(std::path::Component::Normal(pathname)) = path.components().last() {
        let mut location = OsString::from("/boot/");
        location.push(pathname);
        location.push("/netboot.ipxe");

        return Ok(location);
    } else {
        error!(
            "Store path {:?} resolves to {:?} which has no path components?",
            &path, &path
        );

        return Err(reject::not_found());
    }
}
