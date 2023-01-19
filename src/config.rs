use serde::Deserialize;
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
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

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            remote_url: Default::default(),
            local_path: Self::local_path_default(),
            branch: Self::branch_default(),
            https_username: Default::default(),
            https_password: Default::default(),
            ssh_username: Default::default(),
            ssh_pubkey_path: Default::default(),
            ssh_privkey_path: Default::default(),
            ssh_key_passphrase: Default::default(),
            name: Self::name_default(),
            email: Default::default(),
        }
    }
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
    pub issuer_url: String,
    pub redirect_url: String,
    pub client_id: String,
    pub client_secret: String,
    #[serde(default)]
    pub additional_scopes: Vec<String>,
    pub gitlab_authorized_groups: Option<Vec<String>>,
    pub gitlab_authorized_users: Option<Vec<String>>,
}
