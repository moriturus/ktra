use futures::TryFutureExt;
use serde::Deserialize;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, BufReader};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct IndexConfig {
    pub remote_url: String,
    #[serde(default = "IndexConfig::local_path_default")]
    pub local_path: PathBuf,
    #[serde(default = "IndexConfig::branch_default")]
    pub branch: String,
    pub https_username: Option<String>,
    pub https_password: Option<String>,
    pub ssh_username: Option<String>,
    pub ssh_pubkey_path: Option<PathBuf>,
    pub ssh_privkey_path: Option<PathBuf>,
    pub ssh_key_passphrase: Option<String>,
    #[serde(default = "IndexConfig::name_default")]
    pub name: String,
    pub email: Option<String>,
}

impl IndexConfig {
    fn local_path_default() -> PathBuf {
        PathBuf::from("index")
    }

    fn branch_default() -> String {
        "main".to_owned()
    }

    fn name_default() -> String {
        "ktra-driver".to_owned()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CrateFilesConfig {
    #[serde(default = "CrateFilesConfig::dl_dir_path_default")]
    pub dl_dir_path: PathBuf,
    #[cfg(feature = "crates-io-mirroring")]
    #[serde(default = "CrateFilesConfig::cache_dir_path_default")]
    pub cache_dir_path: PathBuf,
    #[serde(default = "CrateFilesConfig::dl_path_default")]
    pub dl_path: Vec<String>,
}

impl Default for CrateFilesConfig {
    fn default() -> CrateFilesConfig {
        CrateFilesConfig {
            dl_dir_path: CrateFilesConfig::dl_dir_path_default(),
            #[cfg(feature = "crates-io-mirroring")]
            cache_dir_path: CrateFilesConfig::cache_dir_path_default(),
            dl_path: CrateFilesConfig::dl_path_default(),
        }
    }
}

impl CrateFilesConfig {
    pub fn dl_dir_path_default() -> PathBuf {
        PathBuf::from("crates")
    }

    #[cfg(feature = "crates-io-mirroring")]
    pub fn cache_dir_path_default() -> PathBuf {
        PathBuf::from("crates_io_caches")
    }

    pub fn dl_path_default() -> Vec<String> {
        vec!["dl".to_owned()]
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DbConfig {
    #[serde(default = "DbConfig::login_prefix_default")]
    pub login_prefix: String,

    #[cfg(feature = "db-sled")]
    #[serde(default = "DbConfig::db_dir_path_default")]
    pub db_dir_path: PathBuf,

    #[cfg(feature = "db-redis")]
    #[serde(default = "DbConfig::redis_url_default")]
    pub redis_url: String,

    #[cfg(feature = "db-mongo")]
    #[serde(default = "DbConfig::mongodb_url_default")]
    pub mongodb_url: String,
}

impl Default for DbConfig {
    fn default() -> DbConfig {
        DbConfig {
            login_prefix: DbConfig::login_prefix_default(),
            #[cfg(feature = "db-sled")]
            db_dir_path: DbConfig::db_dir_path_default(),
            #[cfg(feature = "db-redis")]
            redis_url: DbConfig::redis_url_default(),
            #[cfg(feature = "db-mongo")]
            mongodb_url: DbConfig::mongodb_url_default(),
        }
    }
}

impl DbConfig {
    fn login_prefix_default() -> String {
        "ktra-secure-auth:".to_owned()
    }

    #[cfg(feature = "db-sled")]
    fn db_dir_path_default() -> PathBuf {
        PathBuf::from("db")
    }

    #[cfg(feature = "db-redis")]
    fn redis_url_default() -> String {
        "redis://localhost".to_owned()
    }

    #[cfg(feature = "db-mongo")]
    fn mongodb_url_default() -> String {
        "mongodb://localhost:27017".to_owned()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "ServerConfig::address_default")]
    pub address: [u8; 4],
    #[serde(default = "ServerConfig::port_default")]
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> ServerConfig {
        ServerConfig {
            address: ServerConfig::address_default(),
            port: ServerConfig::port_default(),
        }
    }
}

impl ServerConfig {
    pub fn to_socket_addr(&self) -> SocketAddr {
        (self.address, self.port).into()
    }

    fn address_default() -> [u8; 4] {
        [0, 0, 0, 0]
    }

    fn port_default() -> u16 {
        8000
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Default)]
pub struct OpenIdConfig {
    pub(crate) issuer_url: String,
    pub(crate) redirect_url: String,
    pub(crate) client_id: String,
    pub(crate) client_secret: String,
    #[serde(default)]
    pub(crate) additional_scopes: Vec<String>,
    pub(crate) gitlab_authorized_groups: Option<Vec<String>>,
    pub(crate) gitlab_authorized_users: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub crate_files_config: CrateFilesConfig,
    #[serde(default)]
    pub db_config: DbConfig,
    #[serde(default = "Config::index_config_default")]
    pub index_config: IndexConfig,
    #[serde(default)]
    pub server_config: ServerConfig,
    #[serde(default)]
    pub openid_config: OpenIdConfig,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            crate_files_config: Default::default(),
            db_config: Default::default(),
            index_config: Config::index_config_default(),
            server_config: Default::default(),
            openid_config: Default::default(),
        }
    }
}

impl Config {
    pub async fn open(path: impl AsRef<Path>) -> anyhow::Result<Config> {
        let mut file = OpenOptions::new()
            .read(true)
            .open(path)
            .map_ok(BufReader::new)
            .await?;
        let mut buf = String::new();
        file.read_to_string(&mut buf).await?;

        toml::from_str(&buf).map_err(Into::into)
    }

    fn index_config_default() -> IndexConfig {
        IndexConfig {
            local_path: IndexConfig::local_path_default(),
            branch: IndexConfig::branch_default(),
            name: IndexConfig::name_default(),
            ..Default::default()
        }
    }
}
