use crate::db_manager::DbManager;
#[cfg(feature = "crates-io-mirroring")]
use crate::error::Error;
use crate::models::{Query, User};
use crate::utils::*;
use futures::TryFutureExt;
#[cfg(feature = "crates-io-mirroring")]
use reqwest::Client;
#[cfg(feature = "crates-io-mirroring")]
use semver::Version;
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(feature = "crates-io-mirroring")]
use tokio::fs::OpenOptions;
#[cfg(feature = "crates-io-mirroring")]
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::{io::AsyncReadExt, sync::RwLock};
#[cfg(feature = "crates-io-mirroring")]
use url::Url;
#[cfg(feature = "crates-io-mirroring")]
use warp::http::Response;
#[cfg(feature = "crates-io-mirroring")]
use warp::hyper::body::Bytes;
use warp::{filters::BoxedFilter, Filter, Rejection, Reply};

#[cfg(not(feature = "crates-io-mirroring"))]
#[tracing::instrument(skip(db_manager, dl_dir_path, path))]
pub fn apis(
    db_manager: Arc<RwLock<impl DbManager>>,
    dl_dir_path: Arc<PathBuf>,
    path: Vec<String>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let routes = download(dl_dir_path, path)
        .or(owners(db_manager.clone()))
        .or(search(db_manager));

    // With openid enabled, the `/me` route is handled in src/openid.rs
    #[cfg(not(feature = "openid"))]
    let routes = routes.or(me());

    routes
}

#[cfg(feature = "crates-io-mirroring")]
#[tracing::instrument(skip(db_manager, dl_dir_path, http_client, cache_dir_path, path))]
pub fn apis(
    db_manager: Arc<RwLock<impl DbManager>>,
    dl_dir_path: Arc<PathBuf>,
    http_client: Client,
    cache_dir_path: Arc<PathBuf>,
    path: Vec<String>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let routes = download(dl_dir_path, path)
        .or(download_crates_io(http_client, cache_dir_path))
        .or(owners(db_manager.clone()))
        .or(search(db_manager));
    // With openid enabled, the `/me` route is handled in src/openid.rs
    #[cfg(not(feature = "openid"))]
    let routes = routes.or(me());
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

#[cfg(feature = "crates-io-mirroring")]
#[tracing::instrument(skip(http_client, cache_dir_path, crate_name, version))]
async fn cache_crate_file(
    http_client: Client,
    cache_dir_path: Arc<PathBuf>,
    crate_name: impl AsRef<str>,
    version: Version,
) -> Result<Bytes, Rejection> {
    let computation = async move {
        let mut cache_dir_path = cache_dir_path.as_ref().to_path_buf();
        let crate_components = format!("{}/{}/download", crate_name.as_ref(), version);
        cache_dir_path.push(&crate_components);
        let cache_file_path = cache_dir_path;

        if file_exists_and_not_empty(&cache_file_path).await {
            OpenOptions::new()
                .write(false)
                .create(false)
                .read(true)
                .open(cache_file_path)
                .and_then(|mut file| async move {
                    let mut buffer = Vec::new();
                    file.read_to_end(&mut buffer).await?;
                    Ok(Bytes::from(buffer))
                })
                .map_err(Error::Io)
                .await
        } else {
            let mut crate_dir_path = cache_file_path.clone();
            crate_dir_path.pop();
            let crate_dir_path = crate_dir_path;

            tokio::fs::create_dir_all(crate_dir_path)
                .map_err(Error::Io)
                .await?;

            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .read(true)
                .open(&cache_file_path)
                .map_err(Error::Io)
                .await?;
            let mut file = BufWriter::with_capacity(128 * 1024, file);

            let crates_io_base_url =
                Url::parse("https://crates.io/api/v1/crates/").map_err(Error::UrlParsing)?;
            let crate_file_url = crates_io_base_url
                .join(&crate_components)
                .map_err(Error::UrlParsing)?;
            let body = http_client
                .get(crate_file_url)
                .send()
                .and_then(|res| async move { res.error_for_status() })
                .and_then(|res| res.bytes())
                .map_err(Error::HttpRequest)
                .await?;

            if body.is_empty() {
                return Err(Error::InvalidHttpResponseLength);
            }

            file.write_all(&body).map_err(Error::Io).await?;
            file.flush().map_err(Error::Io).await?;

            Ok(body)
        }
    };

    computation.map_err(warp::reject::custom).await
}

#[cfg(feature = "crates-io-mirroring")]
#[tracing::instrument(skip(cache_dir_path))]
fn download_crates_io(
    http_client: Client,
    cache_dir_path: Arc<PathBuf>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::get()
        .and(with_http_client(http_client))
        .and(with_cache_dir_path(cache_dir_path))
        .and(warp::path!(
            "ktra" / "api" / "v1" / "mirror" / String / Version / "download"
        ))
        .and_then(cache_crate_file)
        .and_then(handle_download_crates_io)
}

#[cfg(feature = "crates-io-mirroring")]
#[tracing::instrument(skip(crate_file_data))]
async fn handle_download_crates_io(crate_file_data: Bytes) -> Result<impl Reply, Rejection> {
    let response = Response::builder()
        .header("Content-Type", "application/x-tar")
        .body(crate_file_data)
        .map_err(Error::HttpResponseBuilding)?;

    Ok(response)
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

#[tracing::instrument]
fn me() -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::get()
        .and(warp::path!("me"))
        .map(|| "$ curl -X POST -H 'Content-Type: application/json' -d '{\"password\":\"YOUR PASSWORD\"}' https://<YOURDOMAIN>/ktra/api/v1/login/<YOUR USERNAME>")
}

#[tracing::instrument(skip(owners))]
fn owners_json(owners: Vec<User>) -> impl Reply {
    warp::reply::json(&serde_json::json!({ "users": owners }))
}
