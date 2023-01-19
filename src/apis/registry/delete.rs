use crate::db_manager::DbManager;
use crate::error::Error;
use crate::index_manager::IndexManager;
use crate::models::Owners;
use crate::utils::{
    authorization_header, ok_json_message, ok_with_msg_json_message, with_db_manager,
    with_index_manager,
};
use futures::TryFutureExt;
use semver::Version;
use std::sync::Arc;
use tokio::sync::RwLock;
use warp::{Filter, Rejection, Reply};

#[tracing::instrument(skip(db_manager, index_manager))]
pub fn apis(
    db_manager: Arc<RwLock<impl DbManager>>,
    index_manager: Arc<IndexManager>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    yank(db_manager.clone(), index_manager).or(owners(db_manager))
}

#[tracing::instrument(skip(db_manager, index_manager))]
fn yank(
    db_manager: Arc<RwLock<impl DbManager>>,
    index_manager: Arc<IndexManager>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::delete()
        .and(with_db_manager(db_manager))
        .and(with_index_manager(index_manager))
        .and(authorization_header())
        .and(warp::path!(
            "api" / "v1" / "crates" / String / Version / "yank"
        ))
        .and_then(handle_yank)
}

#[tracing::instrument(skip(db_manager, index_manager, token, crate_name, version))]
async fn handle_yank(
    db_manager: Arc<RwLock<impl DbManager>>,
    index_manager: Arc<IndexManager>,
    token: String,
    crate_name: String,
    version: Version,
) -> Result<impl Reply, Rejection> {
    let db_manager = db_manager.write().await;

    let user_id = db_manager
        .user_id_for_token(&token)
        .map_err(warp::reject::custom)
        .await?;

    let crate_name_cloned = crate_name.clone();
    db_manager
        .can_edit_package(user_id, &crate_name, version.clone())
        .and_then(|editable| async move {
            if editable {
                Ok(())
            } else {
                Err(Error::OverlappedCrateName(crate_name_cloned))
            }
        })
        .map_err(warp::reject::custom)
        .await?;

    index_manager
        .yank(&crate_name, version.clone())
        .map_err(warp::reject::custom)
        .await?;

    db_manager
        .yank(&crate_name, version)
        .map_ok(ok_json_message)
        .map_err(warp::reject::custom)
        .await
}

#[tracing::instrument(skip(db_manager))]
fn owners(
    db_manager: Arc<RwLock<impl DbManager>>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::delete()
        .and(with_db_manager(db_manager))
        .and(authorization_header())
        .and(warp::path!("api" / "v1" / "crates" / String / "owners"))
        .and(warp::body::json::<Owners>())
        .and_then(handle_owners)
}

#[tracing::instrument(skip(db_manager, token, name, owners))]
async fn handle_owners(
    db_manager: Arc<RwLock<impl DbManager>>,
    token: String,
    name: String,
    owners: Owners,
) -> Result<impl Reply, Rejection> {
    if owners.logins.is_empty() {
        return Err(warp::reject::custom(Error::LoginsNotDefined));
    }

    let db_manager = db_manager.write().await;

    let user_id = db_manager
        .user_id_for_token(&token)
        .map_err(warp::reject::custom)
        .await?;
    db_manager
        .can_edit_owners(user_id, &name)
        .map_err(warp::reject::custom)
        .await?;

    db_manager
        .remove_owners(&name, &owners.logins)
        // the specification says the `msg` field is not required but `cargo` command demands it.
        .map_ok(|_| ok_with_msg_json_message(""))
        .map_err(warp::reject::custom)
        .await
}
