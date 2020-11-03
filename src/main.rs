#![type_length_limit = "2000000"]

mod config;
mod db_manager;
mod delete;
mod error;
mod get;
mod index_manager;
mod models;
mod post;
mod put;
mod utils;

use crate::config::Config;
use crate::index_manager::IndexManager;
use clap::{clap_app, crate_authors, crate_version, ArgMatches};
use db_manager::DbManager;
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use warp::{Filter, Rejection, Reply};

#[cfg(all(feature = "db-redis", not(feature = "db-sled")))]
use db_manager::RedisDbManager;
#[cfg(all(feature = "db-sled", not(feature = "db-redis")))]
use db_manager::SledDbManager;

#[tracing::instrument(skip(db_manager, index_manager, dl_dir_path, dl_path))]
fn apis(
    db_manager: Arc<Mutex<impl DbManager>>,
    index_manager: Arc<Mutex<IndexManager>>,
    dl_dir_path: Arc<PathBuf>,
    dl_path: Vec<String>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    get::apis(db_manager.clone(), dl_dir_path.clone(), dl_path)
        .or(delete::apis(db_manager.clone(), index_manager.clone()))
        .or(post::apis(db_manager.clone()))
        .or(put::apis(db_manager, index_manager, dl_dir_path))
}

#[tracing::instrument(skip(rejection))]
async fn handle_rejection(rejection: Rejection) -> Result<impl Reply, Infallible> {
    if let Some(application_error) = rejection.find::<crate::error::Error>() {
        let (json, status_code) = application_error.to_reply();
        Ok(warp::reply::with_status(json, status_code))
    } else {
        Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "errors": [
                    { "detail": "resource or api is not defined" }
                ]
            })),
            warp::http::StatusCode::NOT_FOUND,
        ))
    }
}

#[tracing::instrument(skip(config))]
async fn run_server(config: Config) -> anyhow::Result<()> {
    tracing::info!(
        "crates directory: {:?}",
        config.crate_files_config.dl_dir_path
    );

    tokio::fs::create_dir_all(&config.crate_files_config.dl_dir_path).await?;
    let dl_dir_path = config.crate_files_config.dl_dir_path.clone();
    let dl_path = config.crate_files_config.dl_path.clone();
    let server_config = config.server_config.clone();

    #[cfg(all(feature = "db-sled", not(feature = "db-redis")))]
    let db_manager = SledDbManager::new(&config.db_config).await?;
    #[cfg(all(feature = "db-redis", not(feature = "db-sled")))]
    let db_manager = RedisDbManager::new(&config.db_config).await?;
    let index_manager = IndexManager::new(config.index_config).await?;
    index_manager.pull().await?;

    let routes = apis(
        Arc::new(Mutex::new(db_manager)),
        Arc::new(Mutex::new(index_manager)),
        Arc::new(dl_dir_path),
        dl_path,
    )
    .with(warp::trace::request())
    .recover(handle_rejection);

    warp::serve(routes)
        .run(server_config.to_socket_addr())
        .await;
    Ok(())
}

#[tracing::instrument(skip(path))]
async fn config(path: impl AsRef<Path>) -> anyhow::Result<Config> {
    let path = path.as_ref();
    if path.exists() {
        Config::open(path).await
    } else {
        Ok(Config::default())
    }
}

#[tracing::instrument]
fn matches() -> ArgMatches<'static> {
    clap_app!(ktra =>
        (version: crate_version!())
        (author: crate_authors!())
        (about: "Your Little Cargo Registry.")
        (@arg CONFIG: -c --config +takes_value "Sets a config file")
        (@arg DL_DIR_PATH: --("dl-dir-path") +takes_value "Sets the crate files directory")
        (@arg DL_PATH: --("dl-path") +takes_value ... "Sets a crate files download path")
        (@arg DB_DIR_PATH: --("db-dir-path") +takes_value "Sets a database directory (needs `db-sled` feature)")
        (@arg REDIS_URL: --("redis-url") + takes_value "Sets a Redis URL (needs `db-redis` feature)")
        (@arg REMOTE_URL: --("remote-url") +takes_value "Sets a URL for the remote index git repository")
        (@arg LOCAL_PATH: --("local-path") +takes_value "Sets a path for local index git repository")
        (@arg BRANCH: --branch +takes_value "Sets a branch name of the index git repository")
        (@arg HTTPS_USERNAME: --("https-username") +takes_value "Sets a username to use for authentication if the remote index git repository uses HTTPS protocol")
        (@arg HTTPS_PASSWORD: --("https-password") +takes_value "Sets a password to use for authentication if the remote index git repository uses HTTPS protocol")
        (@arg SSH_USERNAME: --("ssh-username") +takes_value "Sets a username to use for authentication if the remote index git repository uses SSH protocol")
        (@arg SSH_PUBKEY_PATH: --("ssh-pubkey-path") +takes_value "Sets a public key path to use for authentication if the remote index git repository uses SSH protocol")
        (@arg SSH_PRIVKEY_PATH: --("ssh-privkey-path") +takes_value "Sets a private key path to use for authentication if the remote index git repository uses SSH protocol")
        (@arg SSH_KEY_PASSPHRASE: --("ssh-key-passphrase") +takes_value "Sets a private key's passphrase to use for authentication if the remote index git repository uses SSH protocol")
        (@arg GIT_NAME: --("git-name") +takes_value "Sets an author and committer name")
        (@arg GIT_EMAIL: --("git-email") +takes_value "Sets an author and committer email address")
        (@arg ADDRESS: --("address") +takes_value "Sets an address HTTP server runs on")
        (@arg PORT: --("port") +takes_value "Sets a port number HTTP server listens")
    )
    .get_matches()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let matches = matches();

    let config_file_path = matches.value_of("CONFIG").unwrap_or("ktra.toml");
    let mut config = config(config_file_path).await?;

    if let Some(dl_dir_path) = matches.value_of("DL_DIR_PATH").map(PathBuf::from) {
        config.crate_files_config.dl_dir_path = dl_dir_path;
    }

    if let Some(dl_path) = matches
        .values_of("DL_PATH")
        .map(|vs| vs.map(ToOwned::to_owned).collect())
    {
        config.crate_files_config.dl_path = dl_path;
    }

    #[cfg(feature = "db-sled")]
    if let Some(db_dir_path) = matches.value_of("DB_DIR_PATH").map(PathBuf::from) {
        config.db_config.db_dir_path = db_dir_path;
    }

    #[cfg(feature = "db-redis")]
    if let Some(redis_url) = matches.value_of("REDIS_URL").map(ToOwned::to_owned) {
        config.db_config.redis_url = redis_url;
    }

    if let Some(remote_url) = matches.value_of("REMOTE_URL").map(ToOwned::to_owned) {
        config.index_config.remote_url = remote_url;
    }

    if let Some(local_path) = matches.value_of("LOCAL_PATH").map(PathBuf::from) {
        config.index_config.local_path = local_path;
    }

    if let Some(branch) = matches.value_of("BRANCH").map(ToOwned::to_owned) {
        config.index_config.branch = branch;
    }

    if let Some(https_username) = matches.value_of("HTTPS_USERNAME").map(ToOwned::to_owned) {
        config.index_config.https_username = Some(https_username);
    }

    if let Some(https_password) = matches.value_of("HTTPS_PASSWORD").map(ToOwned::to_owned) {
        config.index_config.https_password = Some(https_password);
    }

    if let Some(ssh_username) = matches.value_of("SSH_USERNAME").map(ToOwned::to_owned) {
        config.index_config.ssh_username = Some(ssh_username);
    }

    if let Some(ssh_pubkey_path) = matches.value_of("SSH_PUBKEY_PATH").map(PathBuf::from) {
        config.index_config.ssh_pubkey_path = Some(ssh_pubkey_path);
    }

    if let Some(ssh_privkey_path) = matches.value_of("SSH_PRIVKEY_PATH").map(PathBuf::from) {
        config.index_config.ssh_privkey_path = Some(ssh_privkey_path);
    }

    if let Some(ssh_key_passphrase) = matches
        .value_of("SSH_KEY_PASSPHRASE")
        .map(ToOwned::to_owned)
    {
        config.index_config.ssh_key_passphrase = Some(ssh_key_passphrase);
    }

    if let Some(name) = matches.value_of("GIT_NAME").map(ToOwned::to_owned) {
        config.index_config.name = name;
    }

    if let Some(email) = matches.value_of("GIT_EMAIL").map(ToOwned::to_owned) {
        config.index_config.email = Some(email);
    }

    if let Some(address) = matches
        .value_of("ADDRESS")
        .map(|s| s.split('.').map(|i| i.parse().unwrap()).collect::<Vec<_>>())
    {
        let address: [u8; 4] = [address[0], address[1], address[2], address[3]];
        config.server_config.address = address;
    }

    if let Some(port) = matches.value_of("PORT").map(|s| s.parse().unwrap()) {
        config.server_config.port = port;
    }

    run_server(config).await
}
