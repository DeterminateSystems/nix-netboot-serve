use std::ffi::OsString;
use std::path::Path;

use warp::reject;
use warp::Rejection;

pub fn redirect_symlink_to_boot(symlink: &Path) -> Result<OsString, Rejection> {
    let path = symlink.read_link().map_err(|e| {
        warn!("Reading the link {:?} failed with: {:?}", symlink, e);
        reject::not_found()
    })?;

    trace!("Resolved symlink {:?} to {:?}", symlink, path);
    redirect_to_boot_store_path(&path)
}

pub fn redirect_to_boot_store_path(path: &Path) -> Result<OsString, Rejection> {
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
