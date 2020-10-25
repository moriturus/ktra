#![cfg(any(feature = "simple-auth", feature = "secure-auth"))]

use crate::db_manager::DbManager;
use crate::models::User;
use crate::utils::*;
use futures::TryFutureExt;
use std::sync::Arc;
use tokio::sync::Mutex;
use warp::{Filter, Rejection, Reply};

#[cfg(all(feature = "secure-auth", not(feature = "simple-auth")))]
use crate::error::Error;
#[cfg(all(feature = "secure-auth", not(feature = "simple-auth")))]
use crate::models::{ChangePassword, Credential};

#[cfg(all(feature = "simple-auth", not(feature = "secure-auth")))]
#[tracing::instrument(skip(db_manager))]
pub fn apis(
    db_manager: Arc<Mutex<DbManager>>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    new_user(db_manager)
}

#[cfg(all(feature = "secure-auth", not(feature = "simple-auth")))]
#[tracing::instrument(skip(db_manager))]
pub fn apis(
    db_manager: Arc<Mutex<DbManager>>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    new_user(db_manager.clone())
        .or(login(db_manager.clone()))
        .or(change_password(db_manager))
}

#[cfg(all(feature = "simple-auth", not(feature = "secure-auth")))]
#[tracing::instrument(skip(db_manager))]
fn new_user(
    db_manager: Arc<Mutex<DbManager>>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::post()
        .and(with_db_manager(db_manager))
        .and(warp::path!("ktra" / "api" / "v1" / "new_user" / String))
        .and_then(handle_new_user)
}

#[cfg(all(feature = "secure-auth", not(feature = "simple-auth")))]
#[tracing::instrument(skip(db_manager))]
fn new_user(
    db_manager: Arc<Mutex<DbManager>>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::post()
        .and(with_db_manager(db_manager))
        .and(warp::path!("ktra" / "api" / "v1" / "new_user" / String))
        .and(warp::body::json::<Credential>())
        .and_then(handle_new_user)
}

#[cfg(all(feature = "simple-auth", not(feature = "secure-auth")))]
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

    let new_token = random_alphanumeric_string(32)
        .map_err(warp::reject::custom)
        .await?;
    db_manager
        .set_token(user_id, new_token.clone())
        .map_err(warp::reject::custom)
        .await?;

    Ok(warp::reply::json(&serde_json::json!({
        "token": new_token
    })))
}

#[cfg(all(feature = "secure-auth", not(feature = "simple-auth")))]
#[tracing::instrument(skip(db_manager, name, credential))]
async fn handle_new_user(
    db_manager: Arc<Mutex<DbManager>>,
    name: String,
    credential: Credential,
) -> Result<impl Reply, Rejection> {
    let db_manager = db_manager.lock().await;

    let user_id = db_manager
        .last_user_id()
        .map_ok(|user_id| user_id.map(|u| u + 1).unwrap_or(0))
        .map_err(warp::reject::custom)
        .await?;
    let login_id = format!("ktra-secure-auth:{}", name);
    let user = User::new(user_id, login_id, Some(name));

    db_manager
        .add_new_user(user, credential.password)
        .map_err(warp::reject::custom)
        .await?;

    let new_token = random_alphanumeric_string(32)
        .map_err(warp::reject::custom)
        .await?;
    db_manager
        .set_token(user_id, new_token.clone())
        .map_err(warp::reject::custom)
        .await?;

    Ok(warp::reply::json(&serde_json::json!({
        "token": new_token
    })))
}

#[cfg(all(feature = "secure-auth", not(feature = "simple-auth")))]
#[tracing::instrument(skip(db_manager))]
fn login(
    db_manager: Arc<Mutex<DbManager>>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::post()
        .and(with_db_manager(db_manager))
        .and(warp::path!("ktra" / "api" / "v1" / "login" / String))
        .and(warp::body::json::<Credential>())
        .and_then(handle_login)
}

#[cfg(all(feature = "secure-auth", not(feature = "simple-auth")))]
#[tracing::instrument(skip(db_manager, name, credential))]
async fn handle_login(
    db_manager: Arc<Mutex<DbManager>>,
    name: String,
    credential: Credential,
) -> Result<impl Reply, Rejection> {
    let db_manager = db_manager.lock().await;

    let user = db_manager
        .user_by_username(name)
        .map_err(warp::reject::custom)
        .await?;

    if db_manager
        .verify_password(user.id, credential.password)
        .map_err(warp::reject::custom)
        .await?
    {
        let new_token = random_alphanumeric_string(32)
            .map_err(warp::reject::custom)
            .await?;
        db_manager
            .set_token(user.id, new_token.clone())
            .map_err(warp::reject::custom)
            .await?;

        Ok(warp::reply::json(&serde_json::json!({
            "token": new_token
        })))
    } else {
        Err(Error::InvalidPassword).map_err(warp::reject::custom)
    }
}

#[cfg(all(feature = "secure-auth", not(feature = "simple-auth")))]
#[tracing::instrument(skip(db_manager))]
fn change_password(
    db_manager: Arc<Mutex<DbManager>>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::post()
        .and(with_db_manager(db_manager))
        .and(warp::path!(
            "ktra" / "api" / "v1" / "change_password" / String
        ))
        .and(warp::body::json::<ChangePassword>())
        .and_then(handle_change_password)
}

#[cfg(all(feature = "secure-auth", not(feature = "simple-auth")))]
#[tracing::instrument(skip(db_manager, name, passwords))]
async fn handle_change_password(
    db_manager: Arc<Mutex<DbManager>>,
    name: String,
    passwords: ChangePassword,
) -> Result<impl Reply, Rejection> {
    let db_manager = db_manager.lock().await;

    let user = db_manager
        .user_by_username(name)
        .map_err(warp::reject::custom)
        .await?;

    if db_manager
        .change_password(user.id, &passwords.old_password, &passwords.new_password)
        .map_ok(always_true)
        .map_err(warp::reject::custom)
        .await?
    {
        let new_token = random_alphanumeric_string(32)
            .map_err(warp::reject::custom)
            .await?;
        db_manager
            .set_token(user.id, new_token.clone())
            .map_err(warp::reject::custom)
            .await?;

        Ok(warp::reply::json(&serde_json::json!({
            "token": new_token
        })))
    } else {
        Err(Error::InvalidPassword).map_err(warp::reject::custom)
    }
}
