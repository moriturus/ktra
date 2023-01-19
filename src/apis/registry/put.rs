use crate::db_manager::DbManager;
use crate::error::Error;
use crate::index_manager::IndexManager;
use crate::models::{Metadata, Owners};
use crate::utils::{
    authorization_header, empty_json_message, ok_json_message, ok_with_msg_json_message,
    with_db_manager, with_dl_dir_path, with_index_manager,
};
use bytes::Bytes;
use futures::TryFutureExt;
use semver::Version;
use sha2::{Digest, Sha256};
use std::convert::TryInto;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use warp::{Filter, Rejection, Reply};

#[tracing::instrument(skip(db_manager, index_manager, dl_dir_path))]
pub fn apis(
    db_manager: Arc<RwLock<impl DbManager>>,
    index_manager: Arc<IndexManager>,
    dl_dir_path: Arc<PathBuf>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    new(db_manager.clone(), index_manager.clone(), dl_dir_path)
        .or(unyank(db_manager.clone(), index_manager))
        .or(owners(db_manager))
}

#[tracing::instrument(skip(db_manager, index_manager, dl_dir_path))]
fn new(
    db_manager: Arc<RwLock<impl DbManager>>,
    index_manager: Arc<IndexManager>,
    dl_dir_path: Arc<PathBuf>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::put()
        .and(with_db_manager(db_manager))
        .and(with_index_manager(index_manager))
        .and(authorization_header())
        .and(with_dl_dir_path(dl_dir_path))
        .and(warp::path!("api" / "v1" / "crates" / "new"))
        .and(warp::body::bytes())
        .and_then(handle_new)
}

#[tracing::instrument(skip(db_manager, index_manager, token, dl_dir_path, body))]
async fn handle_new(
    db_manager: Arc<RwLock<impl DbManager>>,
    index_manager: Arc<IndexManager>,
    token: String,
    dl_dir_path: Arc<PathBuf>,
    body: Bytes,
) -> Result<impl Reply, Rejection> {
    let db_manager = db_manager.write().await;

    let user_id = db_manager
        .user_id_for_token(&token)
        .map_err(warp::reject::custom)
        .await?;

    tracing::debug!("user_id: {}", user_id);

    // body length must be greater than or equals to 4 bytes.
    let (metadata_length, remainder) = len(body, 4).map_err(warp::reject::custom)?;
    tracing::debug!("metadata length: {}", metadata_length);

    // the remainder's length must be greater than or equals to `metadata_length` bytes.
    let (metadata_string, remainder) = map(remainder, metadata_length, |bytes| {
        String::from_utf8(bytes[..].to_vec()).map_err(Error::InvalidUtf8Bytes)
    })
    .map_err(warp::reject::custom)?;
    let metadata: Metadata = serde_json::from_str(&metadata_string)
        .map_err(Error::InvalidJson)
        .map_err(warp::reject::custom)?;

    // check if not exist in the database
    let name = metadata.name.clone();
    let name_cloned = name.clone();
    let version = metadata.vers.clone();
    db_manager
        .can_add_metadata(user_id, &name, version.clone())
        .and_then(|addable| async move {
            if addable {
                Ok(())
            } else {
                Err(Error::OverlappedCrateName(name_cloned))
            }
        })
        .map_err(warp::reject::custom)
        .await?;

    // the remainder's length must be greater than or equals to 4 bytes.
    let (crate_length, remainder) = len(remainder, 4).map_err(warp::reject::custom)?;
    tracing::debug!("crate length: {}", crate_length);

    // the remainder's length must be `crate_length` exactly.
    let (crate_data, remainder) =
        map(remainder, crate_length, Result::Ok).map_err(warp::reject::custom)?;

    if remainder.is_empty() {
        let checksum = checksum(&crate_data);

        let package = metadata.to_package(checksum);
        index_manager
            .add_package(package)
            .map_err(warp::reject::custom)
            .await?;

        let mut crates_dir_path = dl_dir_path.to_path_buf();
        crates_dir_path.push(&metadata.name);
        crates_dir_path.push(metadata.vers.to_string());
        let crates_dir_path = crates_dir_path;

        save_crate_file(crates_dir_path, &crate_data)
            .map_err(warp::reject::custom)
            .await?;
        db_manager
            .add_new_metadata(user_id, metadata)
            .map_ok(empty_json_message)
            .map_err(warp::reject::custom)
            .await
    } else {
        Err(Error::InvalidBodyLength(remainder.len())).map_err(warp::reject::custom)
    }
}

#[tracing::instrument(skip(db_manager, index_manager))]
fn unyank(
    db_manager: Arc<RwLock<impl DbManager>>,
    index_manager: Arc<IndexManager>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::put()
        .and(with_db_manager(db_manager))
        .and(with_index_manager(index_manager))
        .and(authorization_header())
        .and(warp::path!(
            "api" / "v1" / "crates" / String / Version / "unyank"
        ))
        .and_then(handle_unyank)
}

#[tracing::instrument(skip(db_manager, index_manager, token, crate_name, version))]
async fn handle_unyank(
    db_manager: Arc<RwLock<impl DbManager>>,
    index_manager: Arc<IndexManager>,
    token: String,
    crate_name: String,
    version: Version,
) -> Result<impl warp::Reply, warp::Rejection> {
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
        .unyank(&crate_name, version.clone())
        .map_err(warp::reject::custom)
        .await?;

    db_manager
        .unyank(&crate_name, version)
        .map_ok(ok_json_message)
        .map_err(warp::reject::custom)
        .await
}

#[tracing::instrument(skip(db_manager))]
fn owners(
    db_manager: Arc<RwLock<impl DbManager>>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::put()
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

    let logins_cloned = owners.logins.clone();
    db_manager
        .add_owners(&name, &owners.logins)
        .map_ok(|_| {
            let msg = match logins_cloned.len() {
                1 => format!(
                    "user {} has been added to the owners list of crate {}",
                    logins_cloned[0], name
                ),
                _ => format!(
                    "users {:?} have been added to the owners list of crate {}",
                    logins_cloned, name
                ),
            };
            ok_with_msg_json_message(msg)
        })
        .map_err(warp::reject::custom)
        .await
}

#[tracing::instrument(skip(bytes, required_length))]
fn len(mut bytes: Bytes, required_length: usize) -> Result<(usize, Bytes), Error> {
    if bytes.len() < required_length {
        Err(Error::InvalidBodyLength(bytes.len()))
    } else {
        Ok((
            u32::from_le_bytes(
                bytes.split_to(required_length)[..]
                    .try_into()
                    .expect("should be 4 bytes"),
            ) as usize,
            bytes,
        ))
    }
}

#[tracing::instrument(skip(bytes, required_length, f))]
fn map<F, T>(mut bytes: Bytes, required_length: usize, f: F) -> Result<(T, Bytes), Error>
where
    F: FnOnce(Bytes) -> Result<T, Error>,
{
    if bytes.len() < required_length {
        Err(Error::InvalidBodyLength(bytes.len()))
    } else {
        f(bytes.split_to(required_length)).map(|v| (v, bytes))
    }
}

#[tracing::instrument(skip(data))]
fn checksum(data: &[u8]) -> String {
    let mut hasher = Sha256::default();
    hasher.update(data);
    let checksum = hasher.finalize();
    format!("{:x}", checksum)
}

#[tracing::instrument(skip(crates_dir_path, crate_data))]
async fn save_crate_file(
    crates_dir_path: impl AsRef<Path>,
    crate_data: &[u8],
) -> Result<(), Error> {
    let crates_dir_path = crates_dir_path.as_ref().to_path_buf();
    tokio::fs::create_dir_all(&crates_dir_path)
        .map_err(Error::Io)
        .await?;

    let mut crate_binary_path = crates_dir_path;
    crate_binary_path.push("download");

    tokio::fs::write(crate_binary_path, &crate_data)
        .map_err(Error::Io)
        .await
}
