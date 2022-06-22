#![cfg(feature = "sparse-index")]

use std::convert::Infallible;
use std::path::PathBuf;

use warp::path::Tail;
use warp::{reject, Filter, Rejection, Reply};

use crate::config::SparseIndexConfig;
use crate::get::into_boxed_filters;

#[tracing::instrument(skip(sparse_index_config, local_index_path))]
pub fn apis(
    sparse_index_config: SparseIndexConfig,
    local_index_path: PathBuf,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    into_boxed_filters(
        sparse_index_config
            .path
            .split('/')
            .map(ToString::to_string)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>(),
    )
    .and(warp::path::tail())
    .and(with_local_index_path(local_index_path))
    .and_then(read_crate_index)
}

#[tracing::instrument(skip(path))]
fn with_local_index_path(
    path: PathBuf,
) -> impl Filter<Extract = (PathBuf,), Error = Infallible> + Clone {
    warp::any().map(move || path.clone())
}

#[tracing::instrument(skip(tail, local_index_path))]
async fn read_crate_index(tail: Tail, local_index_path: PathBuf) -> Result<String, Rejection> {
    if tail.as_str().starts_with(".") {
        Err(reject::not_found())
    } else {
        std::fs::read_to_string(local_index_path.join(tail.as_str()))
            .map_err(|_| reject::not_found())
    }
}
