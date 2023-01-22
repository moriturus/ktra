use crate::db_manager::DbManager;
use crate::models::{Query, User};
use crate::utils::*;
use futures::TryFutureExt;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use warp::{filters::BoxedFilter, Filter, Rejection, Reply};

#[tracing::instrument(skip(db_manager, dl_dir_path, path))]
pub fn apis(
    db_manager: Arc<RwLock<impl DbManager>>,
    dl_dir_path: Arc<PathBuf>,
    path: Vec<String>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let routes = download(dl_dir_path, path)
        .or(owners(db_manager.clone()))
        .or(search(db_manager));

    routes
}

#[tracing::instrument(skip(path))]
pub(crate) fn into_boxed_filters(path: Vec<String>) -> BoxedFilter<()> {
    let (h, t) = path.split_at(1);
    t.iter().fold(warp::path(h[0].clone()).boxed(), |accm, s| {
        accm.and(warp::path(s.clone())).boxed()
    })
}

#[tracing::instrument(skip(path, dl_dir_path))]
fn download(
    dl_dir_path: Arc<PathBuf>,
    path: Vec<String>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    into_boxed_filters(path).and(warp::fs::dir(dl_dir_path.to_path_buf()))
}

#[tracing::instrument(skip(db_manager))]
fn owners(
    db_manager: Arc<RwLock<impl DbManager>>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::get()
        .and(with_db_manager(db_manager))
        .and(authorization_header())
        .and(warp::path!("api" / "v1" / "crates" / String / "owners"))
        .and_then(handle_owners)
}

#[tracing::instrument(skip(db_manager, _token, name))]
async fn handle_owners(
    db_manager: Arc<RwLock<impl DbManager>>,
    // `token` is not a used argument.
    // the specification demands that the authorization is required but listing owners api does not update the database.
    _token: String,
    name: String,
) -> Result<impl Reply, Rejection> {
    let db_manager = db_manager.read().await;
    let owners = db_manager
        .owners(&name)
        .map_err(warp::reject::custom)
        .await?;
    Ok(owners_json(owners))
}

#[tracing::instrument(skip(db_manager))]
fn search(
    db_manager: Arc<RwLock<impl DbManager>>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::get()
        .and(with_db_manager(db_manager))
        .and(warp::path!("api" / "v1" / "crates"))
        .and(warp::query::<Query>())
        .and_then(handle_search)
}

#[tracing::instrument(skip(db_manager, query))]
async fn handle_search(
    db_manager: Arc<RwLock<impl DbManager>>,
    query: Query,
) -> Result<impl Reply, Rejection> {
    let db_manager = db_manager.read().await;
    db_manager
        .search(&query)
        .map_ok(|s| warp::reply::json(&s))
        .map_err(warp::reject::custom)
        .await
}

#[tracing::instrument(skip(owners))]
fn owners_json(owners: Vec<User>) -> impl Reply {
    warp::reply::json(&serde_json::json!({ "users": owners }))
}
