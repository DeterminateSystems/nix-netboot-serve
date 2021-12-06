use futures::stream::{FuturesOrdered, FuturesUnordered, TryStreamExt};
use futures::StreamExt;
use http::response::Builder;
use lazy_static::lazy_static;
use std::convert::TryInto;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::io;
use std::net::SocketAddr;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use structopt::StructOpt;
use tokio::fs;
use tokio::fs::File;
use tokio::io::BufReader;
use tokio::process::Command;
use tokio_stream::Stream;
use tokio_util::io::ReaderStream;
use warp::hyper::{body::Bytes, Body};
use warp::reject;
use warp::Filter;
use warp::Rejection;

#[macro_use]
extern crate log;

mod cpio_cache;
use crate::cpio_cache::CpioCache;

mod options;
use crate::options::Opt;

mod hydra;

#[derive(Clone)]
struct WebserverContext {
    profile_dir: Option<PathBuf>,
    configuration_dir: Option<PathBuf>,
    gc_root: PathBuf,
    cpio_cache: CpioCache,
}

fn with_context(
    context: WebserverContext,
) -> impl Filter<Extract = (WebserverContext,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || context.clone())
}

fn server_error() -> Rejection {
    reject::not_found()
}

fn feature_disabled(msg: &str) -> Rejection {
    warn!("Feature disabled: {}", msg);
    reject::not_found()
}

fn set_nofiles(limit: u64) -> io::Result<()> {
    let (soft, hard) = rlimit::Resource::NOFILE.get()?;

    if soft > limit {
        info!("Not increasing NOFILES ulimit: current soft ({}) is already higher than the specified ({})", soft, limit);
        return Ok(());
    }

    let mut setto = limit;

    if limit > hard {
        info!(
            "Requested NOFILES ({}) larger than the hard limit ({}), capping at {}.",
            limit, hard, hard
        );
        setto = hard;
    }

    if setto == soft {
        info!(
            "Requested NOFILES ({}) is the same as the current soft limit.",
            setto
        );
        return Ok(());
    }

    rlimit::Resource::NOFILE.set(limit, hard)?;

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

fn make_load_cpio(paths: &Vec<PathBuf>) -> Result<Vec<u8>, LoadCpioError> {
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
enum LoadCpioError {
    Io(std::io::Error),
    NoBasename(PathBuf),
}

lazy_static! {
    static ref LEADER_CPIO_BYTES: Vec<u8> =
        make_leader_cpio().expect("Failed to generate the leader CPIO.");
    static ref LEADER_CPIO_LEN: u64 = LEADER_CPIO_BYTES
        .len()
        .try_into()
        .expect("Failed to convert usize leader length to u64");
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

    // ulimit -Sn 500000

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

    let routes = warp::get().and(
        root.or(profile)
            .or(configuration)
            .or(hydra)
            .or(ipxe)
            .or(initrd)
            .or(kernel),
    );

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

    let build = Command::new("nix-build")
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
        &context,
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

async fn serve_ipxe(name: String) -> Result<impl warp::Reply, Rejection> {
    let params = Path::new("/nix/store").join(&name).join("kernel-params");
    let init = Path::new("/nix/store").join(&name).join("init");
    info!("Sending netboot.ipxe: {:?}", &name);

    let response = format!(
        "#!ipxe
echo Booting NixOS closure {name}. Note: initrd may stay pre-0% for a minute or two.


kernel bzImage  rdinit={init} console=ttyS0 {params}
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

async fn serve_initrd(
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

async fn serve_kernel(name: String) -> Result<impl warp::Reply, Rejection> {
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

async fn open_file_stream(path: &Path) -> std::io::Result<impl Stream<Item = io::Result<Bytes>>> {
    let file = File::open(path).await?;

    Ok(ReaderStream::new(BufReader::new(file)))
}

async fn realize_path(name: String, path: &str, context: &WebserverContext) -> io::Result<bool> {
    // changes between two boots. I'm definitely going to regret this.
    let symlink = context.gc_root.join(&name);

    let realize = Command::new("nix-store")
        .arg("--realise")
        .arg(path)
        .arg("--add-root")
        .arg(&symlink)
        .status()
        .await?;

    return Ok(realize.success());
}

async fn get_closure_paths(path: &Path) -> io::Result<Vec<PathBuf>> {
    let output = Command::new("nix-store")
        .arg("--query")
        .arg("--requisites")
        .arg(path)
        .output()
        .await?;

    let lines = output
        .stdout
        .split(|&ch| ch == b'\n')
        .filter_map(|line| {
            if line.is_empty() {
                None
            } else {
                let line = Vec::from(line);
                let line = OsString::from_vec(line);
                Some(PathBuf::from(line))
            }
        })
        .collect();

    Ok(lines)
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

fn basename(path: &Path) -> Option<&OsStr> {
    if let Some(std::path::Component::Normal(pathname)) = path.components().last() {
        Some(pathname)
    } else {
        None
    }
}
