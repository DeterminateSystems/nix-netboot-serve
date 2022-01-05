use std::path::PathBuf;

use warp::Filter;

use crate::cpio_cache::CpioCache;

#[derive(Clone)]
pub struct WebserverContext {
    pub profile_dir: Option<PathBuf>,
    pub configuration_dir: Option<PathBuf>,
    pub gc_root: PathBuf,
    pub cpio_cache: CpioCache,
}

pub fn with_context(
    context: WebserverContext,
) -> impl Filter<Extract = (WebserverContext,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || context.clone())
}
