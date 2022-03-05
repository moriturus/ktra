use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use url::Url;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetadataDependency {
    pub name: String,
    pub version_req: VersionReq,
    pub features: Vec<String>,
    pub optional: bool,
    pub default_features: bool,
    pub target: Option<String>,
    pub kind: Option<String>,
    pub registry: Option<Url>,
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
    pub name: String,
    pub req: VersionReq,
    pub features: Vec<String>,
    pub optional: bool,
    pub default_features: bool,
    pub target: Option<String>,
    pub kind: Option<String>,
    pub registry: Option<Url>,
    pub package: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metadata {
    pub name: String,
    pub vers: Version,
    pub deps: Vec<MetadataDependency>,
    pub features: HashMap<String, Vec<String>>,
    pub authors: Vec<String>,
    pub description: Option<String>,
    pub documentation: Option<String>,
    pub homepage: Option<url::Url>,
    pub readme: Option<String>,
    pub readme_file: Option<String>,
    pub keywords: Vec<String>,
    pub categories: Vec<String>,
    pub license: Option<String>,
    pub license_file: Option<String>,
    pub repository: Option<url::Url>,
    pub badges: HashMap<String, HashMap<String, String>>,
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
