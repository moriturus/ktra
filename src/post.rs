use crate::db_manager::DbManager;
use crate::models::User;
use crate::utils::*;
use futures::TryFutureExt;
use std::sync::Arc;
use tokio::sync::Mutex;
use warp::{Filter, Rejection, Reply};

#[cfg(feature = "simple-auth")]
use rand::distributions::Alphanumeric;
#[cfg(feature = "simple-auth")]
use rand::prelude::*;

#[tracing::instrument(skip(db_manager))]
pub fn apis(
    db_manager: Arc<Mutex<DbManager>>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    #[cfg(feature = "simple-auth")]
    let db_manager_for_new_user = db_manager;
    #[cfg(feature = "simple-auth")]
    let routes = new_user(db_manager_for_new_user);

    routes
}

#[cfg(feature = "simple-auth")]
#[tracing::instrument(skip(db_manager))]
fn new_user(
    db_manager: Arc<Mutex<DbManager>>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::post()
        .and(with_db_manager(db_manager))
        .and(warp::path!("ktra" / "api" / "v1" / "new_user" / String))
        .and_then(handle_new_user)
}

#[cfg(feature = "simple-auth")]
#[tracing::instrument(skip(db_manager, name))]
async fn handle_new_user(
    db_manager: Arc<Mutex<DbManager>>,
    name: String,
) -> Result<impl Reply, Rejection> {
    let db_manager = db_manager.lock().await;

    let user_id = db_manager
        .last_user_id()
        .map_ok(|user_id| user_id.map(|u| u + 1).unwrap_or(0))
        .map_err(warp::reject::custom)
        .await?;
    let login_id = format!("ktra-simple-auth:{}", name);
    let user = User::new(user_id, login_id, Some(name));

    db_manager
        .add_new_user(user)
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
        "token": new_token
    })))
}
