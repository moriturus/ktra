use crate::db_manager::DbManager;
use crate::models::{Query, User};
use crate::utils::*;
use futures::TryFutureExt;
use std::sync::Arc;
use tokio::sync::Mutex;
use warp::{filters::BoxedFilter, Filter, Rejection, Reply};

#[cfg(feature = "simple-auth")]
use rand::distributions::Alphanumeric;
#[cfg(feature = "simple-auth")]
use rand::prelude::*;

#[tracing::instrument(skip(db_manager))]
pub fn apis(
    db_manager: Arc<Mutex<DbManager>>,
    path: Vec<String>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    download(path)
        .or(owners(db_manager.clone()))
        .or(search(db_manager.clone()))
        .or(me(db_manager))
}

#[tracing::instrument(skip(path))]
fn into_boxed_filters(path: Vec<String>) -> BoxedFilter<()> {
    let (h, t) = path.split_at(1);
    t.iter().fold(warp::path(h[0].clone()).boxed(), |accm, s| {
        accm.and(warp::path(s.clone())).boxed()
    })
}

#[tracing::instrument(skip(path))]
fn download(path: Vec<String>) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    into_boxed_filters(path).and(warp::fs::dir("crates"))
}

#[tracing::instrument(skip(db_manager))]
fn owners(
    db_manager: Arc<Mutex<DbManager>>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::get()
        .and(with_db_manager(db_manager))
        .and(authorization_header())
        .and(warp::path!("api" / "v1" / "crates" / String / "owners"))
        .and_then(handle_owners)
}

#[tracing::instrument(skip(db_manager, _token, name))]
async fn handle_owners(
    db_manager: Arc<Mutex<DbManager>>,
    // `token` is not a used argument.
    // the specification demands that the authorization is required but listing owners api does not update the database.
    _token: String,
    name: String,
) -> Result<impl Reply, Rejection> {
    let db_manager = db_manager.lock().await;
    let owners = db_manager
        .owners(&name)
        .map_err(warp::reject::custom)
        .await?;
    Ok(owners_json(owners))
}

#[tracing::instrument(skip(db_manager))]
fn search(
    db_manager: Arc<Mutex<DbManager>>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::get()
        .and(with_db_manager(db_manager))
        .and(warp::path!("api" / "v1" / "crates"))
        .and(warp::query::<Query>())
        .and_then(handle_search)
}

#[tracing::instrument(skip(db_manager, query))]
async fn handle_search(
    db_manager: Arc<Mutex<DbManager>>,
    query: Query,
) -> Result<impl Reply, Rejection> {
    let db_manager = db_manager.lock().await;
    db_manager
        .search(&query)
        .map_ok(|s| warp::reply::json(&s))
        .map_err(warp::reject::custom)
        .await
}

#[tracing::instrument(skip(db_manager))]
fn me(
    db_manager: Arc<Mutex<DbManager>>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::get()
        .and(with_db_manager(db_manager))
        .and(authorization_header())
        .and(warp::path!("me"))
        .and_then(handle_me)
}

#[cfg(feature = "simple-auth")]
#[tracing::instrument(skip(db_manager, token))]
async fn handle_me(
    db_manager: Arc<Mutex<DbManager>>,
    token: String,
) -> Result<impl Reply, Rejection> {
    let db_manager = db_manager.lock().await;

    let user_id = db_manager
        .user_id_for_token(&token)
        .map_err(warp::reject::custom)
        .await?;
    let new_token: String = tokio::task::block_in_place(|| {
        rand::thread_rng()
            .sample_iter(Alphanumeric)
            .take(32)
            .collect()
    });

    db_manager
        .set_token(user_id, new_token.clone())
        .map_err(warp::reject::custom)
        .await?;

    Ok(warp::reply::json(&serde_json::json!({
        "new_token": new_token
    })))
}

#[tracing::instrument(skip(owners))]
fn owners_json(owners: Vec<User>) -> impl Reply {
    warp::reply::json(&serde_json::json!({ "users": owners }))
}
