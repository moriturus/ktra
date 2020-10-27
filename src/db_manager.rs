use crate::config::DbConfig;
use crate::error::Error;
use crate::models::{Entry, Metadata, Query, Search, User};
use crate::utils::random_alphanumeric_string;
use crate::utils::sink;
use argon2::{self, hash_encoded, verify_encoded, ThreadMode, Variant};
use futures::TryFutureExt;
use semver::Version;
use serde::de::DeserializeOwned;
use serde::ser::Serialize;
use sled::{self, Db};
use std::collections::HashMap;

type TokenMap = HashMap<u32, String>;

const WINDOWS_NG_FILENAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9",
];

const SCHEMA_VERSION_KEY: &str = "__SCHEMA_VERSION__";
const SCHEMA_VERSION: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 2];
const USERS_KEY: &str = "__USERS__";
const PASSWORDS_KEY: &str = "__PASSWORDS__";

pub struct DbManager {
    tree: Db,
}

impl DbManager {
    #[tracing::instrument(skip(config))]
    pub async fn new(config: &DbConfig) -> Result<DbManager, Error> {
        let path = config.db_dir_path.clone();
        tracing::info!("create and/or open database: {:?}", config.db_dir_path);

        let tree = tokio::task::spawn_blocking(|| sled::open(path).map_err(Error::Db))
            .map_err(Error::Join)
            .await??;

        if tree.contains_key(SCHEMA_VERSION_KEY).map_err(Error::Db)? {
            tree.insert(SCHEMA_VERSION_KEY, &SCHEMA_VERSION)
                .map(sink)
                .map_err(Error::Db)?;
        }

        let db_manager = DbManager { tree };
        Ok(db_manager)
    }

    #[tracing::instrument(skip(self, user_id, name))]
    pub async fn can_edit_owners(
        &self,
        user_id: u32,
        name: impl Into<String>,
    ) -> Result<bool, Error> {
        let name = name.into();
        check_crate_name(&name)?;

        let entry = self.entry(&name).await?;

        if entry.is_empty() {
            Err(Error::CrateNotFoundInDb(name))
        } else if !entry.owner_ids().contains(&user_id) {
            Err(Error::InvalidUser(user_id))
        } else {
            Ok(true)
        }
    }

    #[tracing::instrument(skip(self, name))]
    pub async fn owners(&self, name: impl Into<String>) -> Result<Vec<User>, Error> {
        let users: Vec<User> = self.deserialize(USERS_KEY)?.unwrap_or_default();
        let entry = self.entry(name).await?;
        let owners = users
            .into_iter()
            .filter(|u| entry.owner_ids().contains(&u.id))
            .collect();
        Ok(owners)
    }

    #[tracing::instrument(skip(self, name, logins, editor))]
    async fn edit_owners<N, L, E>(&self, name: N, logins: L, editor: E) -> Result<(), Error>
    where
        N: Into<String>,
        L: IntoIterator<Item = String>,
        E: FnOnce(&[u32], &mut Entry),
    {
        let mut users: Vec<User> = self.deserialize(USERS_KEY)?.unwrap_or_default();
        users.sort_by_key(|u| u.login.clone());

        let (ids, errors): (Vec<_>, Vec<_>) = logins
            .into_iter()
            .map(|l| {
                users
                    .binary_search_by_key(&l, |u| u.login.clone())
                    .map(|i| users[i].id)
                    .map_err(|_| l.clone())
            })
            .partition(Result::is_ok);

        if errors.is_empty() {
            let name = name.into();
            let mut entry: Entry = self.entry(&name).await?;

            let ids: Vec<_> = ids.into_iter().map(Result::unwrap).collect();
            editor(&ids, &mut entry);

            self.insert_entry(&name, entry).await
        } else {
            Err(Error::InvalidLoginNames(
                errors.into_iter().map(Result::unwrap_err).collect(),
            ))
        }
    }

    #[tracing::instrument(skip(self, name, logins))]
    pub async fn add_owners(
        &self,
        name: impl Into<String>,
        logins: impl IntoIterator<Item = String>,
    ) -> Result<(), Error> {
        self.edit_owners(name, logins, |ids, entry| {
            entry.owner_ids_mut().extend(ids);
            entry.owner_ids_mut().sort();
            entry.owner_ids_mut().dedup();
        })
        .await
    }

    #[tracing::instrument(skip(self, name, logins))]
    pub async fn remove_owners(
        &self,
        name: impl Into<String>,
        logins: impl IntoIterator<Item = String>,
    ) -> Result<(), Error> {
        self.edit_owners(name, logins, |ids, entry| {
            entry.owner_ids_mut().retain(|i| !ids.contains(i));
        })
        .await
    }

    #[tracing::instrument(skip(self))]
    pub async fn last_user_id(&self) -> Result<Option<u32>, Error> {
        let last_user_id = self
            .deserialize("tokens")?
            .or_else(|| Some(Default::default()))
            .map(|map: TokenMap| {
                if map.is_empty() {
                    None
                } else {
                    map.keys().max().cloned()
                }
            })
            .flatten();
        Ok(last_user_id)
    }

    #[tracing::instrument(skip(self, token))]
    pub async fn user_id_for_token(&self, token: impl Into<String>) -> Result<u32, Error> {
        let token = token.into();
        self.deserialize("tokens")?
            .and_then(|map: TokenMap| {
                map.iter()
                    .find_map(|(k, v)| if v == &token { Some(*k) } else { None })
            })
            .ok_or_else(|| Error::InvalidToken(token))
    }

    #[tracing::instrument(skip(self, user_id, token))]
    pub async fn set_token(&self, user_id: u32, token: impl Into<String>) -> Result<(), Error> {
        let token = token.into();
        let mut tokens: TokenMap = self.deserialize("tokens")?.unwrap_or_default();
        tokens.insert(user_id, token);

        self.insert("tokens", tokens).await
    }

    #[tracing::instrument(skip(self, name))]
    pub async fn user_by_username(&self, name: impl Into<String>) -> Result<User, Error> {
        let name = name.into();
        let login = format!("ktra-secure-auth:{}", name);
        let mut users: Vec<User> = self.deserialize(USERS_KEY)?.unwrap_or_default();

        users.sort_by_key(|u| u.login.clone());
        let index = users
            .binary_search_by_key(&login, |u| u.login.clone())
            .map_err(|_| Error::InvalidUsername(name))?;
        Ok(users.remove(index))
    }

    #[tracing::instrument(skip(self, user, password))]
    pub async fn add_new_user(&self, user: User, password: impl Into<String>) -> Result<(), Error> {
        let mut users: Vec<User> = self.deserialize(USERS_KEY)?.unwrap_or_default();
        let mut passwords: HashMap<u32, String> =
            self.deserialize(PASSWORDS_KEY)?.unwrap_or_default();

        let password = password.into();
        let user_id = user.id;

        if users.iter().any(|u| u.login == user.login) {
            return Err(Error::UserExists(user.login));
        } else {
            users.push(user);
        }

        let (config, salt) = argon2_config_and_salt().await?;
        let encoded_password =
            hash_encoded(password.as_bytes(), salt.as_bytes(), &config).map_err(Error::Argon2)?;
        passwords.insert(user_id, encoded_password);
        self.insert(PASSWORDS_KEY, passwords).await?;

        users.sort_by_key(|u| u.id);
        self.insert(USERS_KEY, users).await
    }

    #[tracing::instrument(skip(self, user_id, password))]
    pub async fn verify_password(
        &self,
        user_id: u32,
        password: impl Into<String>,
    ) -> Result<bool, Error> {
        let password = password.into();
        let passwords: HashMap<u32, String> = self.deserialize(PASSWORDS_KEY)?.unwrap_or_default();

        if let Some(result) = passwords
            .get(&user_id)
            .map(|e| verify_encoded(e, password.as_bytes()))
        {
            result.map_err(Error::Argon2)
        } else {
            Err(Error::InvalidUser(user_id))
        }
    }

    #[tracing::instrument(skip(self, user_id, old_password, new_password))]
    pub async fn change_password(
        &self,
        user_id: u32,
        old_password: impl Into<String>,
        new_password: impl Into<String>,
    ) -> Result<(), Error> {
        let old_password = old_password.into();
        let new_password = new_password.into();

        if old_password == new_password {
            return Err(Error::SamePasswords);
        }

        let mut passwords: HashMap<u32, String> =
            self.deserialize(PASSWORDS_KEY)?.unwrap_or_default();

        if let Some(encoded_old_password) = passwords.get(&user_id) {
            if verify_encoded(encoded_old_password, old_password.as_bytes())
                .map_err(Error::Argon2)?
            {
                let (config, salt) = argon2_config_and_salt().await?;
                let encoded_new_password =
                    hash_encoded(new_password.as_bytes(), salt.as_bytes(), &config)
                        .map_err(Error::Argon2)?;
                passwords.insert(user_id, encoded_new_password);
                self.insert(PASSWORDS_KEY, passwords).await
            } else {
                Err(Error::InvalidPassword)
            }
        } else {
            Err(Error::InvalidUser(user_id))
        }
    }

    #[tracing::instrument(skip(self, name))]
    pub async fn entry(&self, name: impl Into<String>) -> Result<Entry, Error> {
        let name = normalized_crate_name(&name.into());
        self.deserialize(&name).map(Option::unwrap_or_default)
    }

    #[tracing::instrument(skip(self, user_id, name, version))]
    pub async fn can_add_metadata(
        &self,
        user_id: u32,
        name: impl Into<String>,
        version: Version,
    ) -> Result<bool, Error> {
        let name = name.into();
        check_crate_name(&name)?;

        let entry = self.entry(&name).await?;

        if entry.is_empty() {
            return Ok(true);
        } else if !entry.owner_ids().contains(&user_id) {
            return Err(Error::InvalidUser(user_id));
        } else if entry.versions().contains_key(&version) {
            return Err(Error::VersionExists(name, version));
        }

        tracing::debug!("user: {}, name: {}, version: {}", user_id, name, version);

        tracing::debug!("entry: {:?}", entry);
        let latest_version = entry.latest_version();
        tracing::debug!("latest: {:?}", latest_version);
        let can_add_metadata = entry
            .latest_version()
            .and_then(|v| entry.versions().get(v))
            .map(|p| name == p.name)
            .expect("latest version must exists");
        Ok(can_add_metadata)
    }

    #[tracing::instrument(skip(self, owner_id, metadata))]
    pub async fn add_new_metadata(&self, owner_id: u32, metadata: Metadata) -> Result<(), Error> {
        let name = metadata.name.clone();
        let version = metadata.vers.clone();
        let mut entry = self.entry(&name).await?;

        // check if it is the first publishing
        if entry.is_empty() {
            entry.owner_ids_mut().push(owner_id);
        }
        // check if the user is allowed to publish
        if !entry.owner_ids().contains(&owner_id) {
            return Err(Error::InvalidUser(owner_id));
        }

        entry.versions_mut().insert(version, metadata);
        self.insert_entry(&name, entry).await
    }

    #[tracing::instrument(skip(self, user_id, name, version))]
    pub async fn can_edit_package(
        &self,
        user_id: u32,
        name: impl Into<String>,
        version: Version,
    ) -> Result<bool, Error> {
        let name = name.into();
        check_crate_name(&name)?;

        let entry = self.entry(&name).await?;

        if entry.is_empty() {
            return Err(Error::CrateNotFoundInDb(name));
        } else if !entry.owner_ids().contains(&user_id) {
            return Err(Error::InvalidUser(user_id));
        } else if !entry.versions().contains_key(&version) {
            return Err(Error::VersionNotFoundInDb(version));
        }

        let can_edit_package = entry
            .versions()
            .get(&version)
            .map(|p| name == p.name)
            .expect("specified version must exists");
        Ok(can_edit_package)
    }

    #[tracing::instrument(skip(self, name, version, yanked, no_changed_error_closure))]
    async fn change_yanked<F>(
        &self,
        name: impl Into<String>,
        version: Version,
        yanked: bool,
        no_changed_error_closure: F,
    ) -> Result<(), Error>
    where
        F: FnOnce(String, Version) -> Error,
    {
        let name = name.into();
        let name_cloned = name.clone();
        let entry = self
            .entry(&name)
            .and_then(|mut entry| async move {
                let package = entry
                    .package_mut(&version)
                    .ok_or_else(|| Error::VersionNotFoundInDb(version.clone()))?;

                if package.yanked == yanked {
                    Err(no_changed_error_closure(name_cloned, version))
                } else {
                    package.yanked = yanked;
                    Ok(entry)
                }
            })
            .await?;

        self.insert_entry(&name, entry).await
    }

    #[tracing::instrument(skip(self, name, version))]
    pub async fn yank(&self, name: impl Into<String>, version: Version) -> Result<(), Error> {
        self.change_yanked(name, version, true, Error::AlreadyYanked)
            .await
    }

    #[tracing::instrument(skip(self, name, version))]
    pub async fn unyank(&self, name: impl Into<String>, version: Version) -> Result<(), Error> {
        self.change_yanked(name, version, false, Error::NotYetYanked)
            .await
    }

    #[tracing::instrument(skip(self, query))]
    pub async fn search(&self, query: &Query) -> Result<Search, Error> {
        let query_string = normalized_crate_name(&query.string);

        let (filtered, errors): (Vec<_>, Vec<_>) = self
            .tree
            .iter()
            .filter_map(|result| {
                match result {
                    Ok((key, value)) => {
                        // the keys in ktra db must be valid UTF-8 string so ignore any validation errors.
                        let key = std::str::from_utf8(&key).ok()?;

                        let condition = key != USERS_KEY
                            && key != SCHEMA_VERSION_KEY
                            && key != PASSWORDS_KEY
                            && key.contains(&query_string);

                        if condition {
                            match serde_json::from_slice::<Entry>(&value)
                                .map_err(Error::InvalidJson)
                            {
                                Ok(entry) => {
                                    let (_, latest_version) = entry
                                        .versions()
                                        .iter()
                                        .filter(|(_, metadata)| !metadata.yanked)
                                        .max_by_key(|(key, _)| *key)?;
                                    Some(Ok(latest_version.to_searched()))
                                }
                                Err(e) => Some(Err(e)),
                            }
                        } else {
                            None
                        }
                    }
                    Err(e) => Some(Err(Error::Db(e))),
                }
            })
            .partition(Result::is_ok);

        if errors.is_empty() {
            let count = filtered.len();
            let filtered = filtered
                .into_iter()
                .take(query.limit)
                .map(Result::unwrap)
                .collect::<Vec<_>>();

            Ok(Search::new(filtered, count))
        } else {
            Err(Error::multiple(errors))
        }
    }

    #[tracing::instrument(skip(self, key))]
    fn deserialize<T>(&self, key: impl AsRef<[u8]>) -> Result<Option<T>, Error>
    where
        T: DeserializeOwned,
    {
        self.tree
            .get(key)
            .map_err(Error::Db)?
            .map(|v| v.to_vec())
            .map(String::from_utf8)
            .transpose()
            .map_err(Error::InvalidUtf8Bytes)?
            .map(|s| serde_json::from_str::<T>(&s))
            .transpose()
            .map_err(Error::InvalidJson)
    }

    #[tracing::instrument(skip(self, name, entry))]
    async fn insert_entry<'a>(&self, name: &str, entry: Entry) -> Result<(), Error> {
        self.insert(normalized_crate_name(&name), entry).await
    }

    #[tracing::instrument(skip(self, key, value))]
    async fn insert(&self, key: impl AsRef<[u8]>, value: impl Serialize) -> Result<(), Error> {
        let json_string = serde_json::to_string(&value).map_err(Error::Serialization)?;
        self.tree
            .insert(key, json_string.as_str())
            .map(sink)
            .map_err(Error::Db)?;
        self.tree
            .flush_async()
            .map_ok(sink)
            .map_err(Error::Db)
            .await
    }
}

#[tracing::instrument(skip(name))]
fn normalized_crate_name(name: &str) -> String {
    name.to_ascii_lowercase()
        .replace('_', "=")
        .replace('-', "=")
}

#[tracing::instrument(skip(name))]
fn check_crate_name(name: &str) -> Result<(), Error> {
    let name = name.to_lowercase();
    let length = name.chars().count();
    let first_char_check = name
        .chars()
        .next()
        .map_or(false, |c| c.is_ascii_alphabetic());
    let chars_check = name
        .chars()
        .all(|c| (c.is_alphanumeric() || c == '-' || c == '_') && c.is_ascii());
    let filename_check = !WINDOWS_NG_FILENAMES.contains(&name.as_str());

    if length <= 65 && first_char_check && chars_check && filename_check {
        Ok(())
    } else {
        Err(Error::InvalidCrateName(name))
    }
}

#[tracing::instrument]
async fn argon2_config_and_salt<'a>() -> Result<(argon2::Config<'a>, String), Error> {
    let config = argon2::Config {
        variant: Variant::Argon2id,
        lanes: 4,
        thread_mode: ThreadMode::Parallel,
        ..Default::default()
    };
    let salt: String = random_alphanumeric_string(32).await?;
    Ok((config, salt))
}
