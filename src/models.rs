use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use url::Url;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetadataDependency {
    /// Name of the dependency.
    /// If the dependency is renamed from the original package name,
    /// this is the original name. The new package name is stored in
    /// the `explicit_name_in_toml` field.
    pub name: String,
    /// The semver requirement for this dependency.
    pub version_req: VersionReq,
    /// Array of features (as strings) enabled for this dependency.
    pub features: Vec<String>,
    /// Boolean of whether or not this is an optional dependency.
    pub optional: bool,
    /// Boolean of whether or not default features are enabled.
    pub default_features: bool,
    /// The target platform for the dependency.
    /// null if not a target dependency.
    /// Otherwise, a string such as "cfg(windows)".
    pub target: Option<String>,
    /// The dependency kind.
    /// "dev", "build", or "normal".
    pub kind: Option<String>,
    /// The URL of the index of the registry where this dependency is
    /// from as a string. If not specified or null, it is assumed the
    /// dependency is in the current registry.
    pub registry: Option<Url>,
    /// If the dependency is renamed, this is a string of the new
    /// package name. If not specified or null, this dependency is not
    /// renamed.
    pub explicit_name_in_toml: Option<String>,
}

impl From<MetadataDependency> for Dependency {
    #[tracing::instrument(skip(val))]
    fn from(val: MetadataDependency) -> Self {
        let (name, package) = if let Some(local_new_name) = val.explicit_name_in_toml {
            (local_new_name, val.name.into())
        } else {
            (val.name.clone(), None)
        };

        Self {
            name,
            req: val.version_req,
            features: val.features,
            optional: val.optional,
            default_features: val.default_features,
            target: val.target,
            kind: val.kind,
            registry: val.registry,
            package,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dependency {
    /// Name of the dependency.
    /// If the dependency is renamed from the original package name,
    /// this is the new name. The original package name is stored in
    /// the `package` field.
    pub name: String,
    /// The SemVer requirement for this dependency.
    /// This must be a valid version requirement defined at
    /// https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html.
    pub req: VersionReq,
    /// Array of features (as strings) enabled for this dependency.
    pub features: Vec<String>,
    /// Boolean of whether or not this is an optional dependency.
    pub optional: bool,
    /// Boolean of whether or not default features are enabled.
    pub default_features: bool,
    /// The target platform for the dependency.
    /// null if not a target dependency.
    /// Otherwise, a string such as "cfg(windows)".
    pub target: Option<String>,
    /// The dependency kind.
    /// "dev", "build", or "normal".
    /// Note: this is a required field, but a small number of entries
    /// exist in the crates.io index with either a missing or null
    /// `kind` field due to implementation bugs.
    pub kind: Option<String>,
    /// The URL of the index of the registry where this dependency is
    /// from as a string. If not specified or null, it is assumed the
    /// dependency is in the current registry.
    pub registry: Option<Url>,
    /// If the dependency is renamed, this is a string of the actual
    /// package name. If not specified or null, this dependency is not
    /// renamed.
    pub package: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metadata {
    /// The name of the package.
    pub name: String,
    /// The version of the package being published.
    pub vers: Version,
    /// Array of direct dependencies of the package.
    pub deps: Vec<MetadataDependency>,
    /// Set of features defined for the package.
    /// Each feature maps to an array of features or dependencies it enables.
    /// Cargo does not impose limitations on feature names, but crates.io
    /// requires alphanumeric ASCII, `_` or `-` characters.
    pub features: HashMap<String, Vec<String>>,
    /// List of strings of the authors.
    pub authors: Vec<String>,
    /// Description field from the manifest.
    /// Note: crates.io requires at least some content.
    pub description: Option<String>,
    /// String of the URL to the website for this package's documentation.
    pub documentation: Option<String>,
    /// String of the URL to the website for this package's home page.
    pub homepage: Option<url::Url>,
    /// String of the content of the README file.
    pub readme: Option<String>,
    /// String of a relative path to a README file in the crate.
    pub readme_file: Option<String>,
    /// Array of strings of keywords for the package.
    pub keywords: Vec<String>,
    /// Array of strings of categories for the package.
    pub categories: Vec<String>,
    /// String of the license for the package.
    /// Note: crates.io requires either `license` or `license_file` to be set.
    pub license: Option<String>,
    /// String of a relative path to a license file in the crate.
    pub license_file: Option<String>,
    /// String of the URL to the website for the source repository of this package.
    pub repository: Option<url::Url>,
    /// Optional object of "status" badges. Each value is an object of
    /// arbitrary string to string mappings.
    /// crates.io has special interpretation of the format of the badges.
    pub badges: HashMap<String, HashMap<String, String>>,
    /// The `links` string value from the package's manifest, or None if not
    /// specified.
    pub links: Option<String>,
    #[serde(default)]
    pub yanked: bool,
}

impl Metadata {
    #[tracing::instrument(skip(self, checksum))]
    pub fn to_package(&self, checksum: impl Into<String>) -> Package {
        Package {
            name: self.name.clone(),
            vers: self.vers.clone(),
            deps: self.deps.iter().map(Clone::clone).map(Into::into).collect(),
            cksum: checksum.into(),
            features: self.features.clone(),
            yanked: false,
            links: self.links.clone(),
        }
    }

    #[tracing::instrument(skip(self))]
    pub fn to_searched(&self) -> SearchedMetadata {
        SearchedMetadata {
            name: self.name.clone(),
            max_version: self.vers.clone(),
            description: self.description.as_ref().cloned().unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchedMetadata {
    pub name: String,
    pub max_version: Version,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Package {
    pub name: String,
    pub vers: Version,
    pub deps: Vec<Dependency>,
    pub cksum: String,
    pub features: HashMap<String, Vec<String>>,
    pub yanked: bool,
    pub links: Option<String>,
}

impl Package {
    #[tracing::instrument(skip(self))]
    pub fn to_json_string(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct User {
    pub id: u32,
    pub login: String,
    pub name: Option<String>,
}

impl User {
    #[tracing::instrument(skip(id, login, name))]
    pub fn new(id: u32, login: impl Into<String>, name: Option<impl Into<String>>) -> User {
        User {
            id,
            login: login.into(),
            name: name.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Entry {
    versions: HashMap<Version, Metadata>,
    owner_ids: Vec<u32>,
}

impl Entry {
    #[tracing::instrument(skip(self))]
    pub fn is_empty(&self) -> bool {
        self.versions.is_empty() && self.owner_ids.is_empty()
    }

    #[tracing::instrument(skip(self))]
    pub fn versions(&self) -> &HashMap<Version, Metadata> {
        &self.versions
    }

    #[tracing::instrument(skip(self, version))]
    pub fn package_mut(&mut self, version: &Version) -> Option<&mut Metadata> {
        self.versions.get_mut(version)
    }

    #[tracing::instrument(skip(self))]
    pub fn latest_version(&self) -> Option<&Version> {
        self.versions().keys().max()
    }

    #[tracing::instrument(skip(self))]
    pub fn versions_mut(&mut self) -> &mut HashMap<Version, Metadata> {
        &mut self.versions
    }

    #[tracing::instrument(skip(self))]
    pub fn owner_ids(&self) -> &[u32] {
        &self.owner_ids
    }

    #[tracing::instrument(skip(self))]
    pub fn owner_ids_mut(&mut self) -> &mut Vec<u32> {
        &mut self.owner_ids
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Owners {
    #[serde(rename = "users")]
    pub logins: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Query {
    #[serde(rename = "q")]
    pub string: String,
    #[serde(default = "query_limit_default", rename = "per_page")]
    pub limit: usize,
}

const fn query_limit_default() -> usize {
    10
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Count {
    total: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Search {
    crates: Vec<SearchedMetadata>,
    meta: Count,
}

impl Search {
    #[tracing::instrument(skip(crates, total))]
    pub fn new(crates: Vec<SearchedMetadata>, total: usize) -> Search {
        Search {
            crates,
            meta: Count { total },
        }
    }
}

#[derive(Clone, Deserialize)]
pub struct Credential {
    pub password: String,
}

#[derive(Clone, Deserialize)]
pub struct ChangePassword {
    pub old_password: String,
    pub new_password: String,
}

#[cfg(feature = "openid")]
#[derive(Debug, Clone, Deserialize)]
pub struct CodeQuery {
    pub code: String,
    pub state: Option<String>,
}

/// The additional claims OpenId providers may send
///
/// All fields here are options so that the extra claims are caught when presents
#[cfg(feature = "openid")]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Claims {
    pub(crate) sub: Option<String>,
    pub(crate) sub_legacy: Option<String>,
    // Gitlab claims return the groups a user is in.
    // This property is used when gitlab_authorized_groups is set in the configuration
    pub(crate) groups: Option<Vec<String>>,
}
