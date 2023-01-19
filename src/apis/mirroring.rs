#![cfg(feature = "crates-io-mirroring")]

use std::path::PathBuf;
use std::sync::Arc;

use futures::TryFutureExt;
use reqwest::Client;
use semver::Version;
use tokio::fs::OpenOptions;
use tokio::io::AsyncReadExt;
use tokio::io::{AsyncWriteExt, BufWriter};
use url::Url;
use warp::http::Response;
use warp::hyper::body::Bytes;
use warp::{Filter, Rejection, Reply};

use crate::error::Error;
use crate::utils::{file_exists_and_not_empty, with_cache_dir_path, with_http_client};

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

#[tracing::instrument(skip(cache_dir_path))]
pub fn download_crates_io(
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

#[tracing::instrument(skip(crate_file_data))]
async fn handle_download_crates_io(crate_file_data: Bytes) -> Result<impl Reply, Rejection> {
    let response = Response::builder()
        .header("Content-Type", "application/x-tar")
        .body(crate_file_data)
        .map_err(Error::HttpResponseBuilding)?;

    Ok(response)
}
