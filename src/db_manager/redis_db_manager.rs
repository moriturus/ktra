#![cfg(feature = "db-redis")]

use crate::config::DbConfig;
use crate::error::Error;
use crate::models::{Entry, Metadata, Query, Search, User};
use argon2::{self, hash_encoded, verify_encoded};
use async_trait::async_trait;
use futures::TryFutureExt;
use redis::{AsyncCommands, Client};
use semver::Version;
use serde::de::DeserializeOwned;
use serde::ser::Serialize;
use std::collections::HashMap;

use crate::db_manager::utils::{argon2_config_and_salt, check_crate_name, normalized_crate_name};
use crate::db_manager::DbManager;

type TokenMap = HashMap<u32, String>;

const SCHEMA_VERSION_KEY: &str = "ktra:__SCHEMA_VERSION__";
const SCHEMA_VERSION: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 1];
const ENTRIES_KEY: &str = "ktra:__ENTRIES__";
const USERS_KEY: &str = "ktra:__USERS__";
const PASSWORDS_KEY: &str = "ktra:__PASSWORDS__";
const TOKENS_KEY: &str = "ktra:__TOKENS__";
const OAUTH_NONCES_KEY: &str = "ktra:__OAUTH_NONCES__";

pub struct RedisDbManager {
    client: Client,
    login_prefix: String,
}

#[async_trait]
impl DbManager for RedisDbManager {
    #[tracing::instrument(skip(config))]
    async fn new(config: &DbConfig) -> Result<RedisDbManager, Error> {
        tracing::info!("connect to redis server: {}", config.redis_url);

        let initialization = async {
            let client = Client::open(&*config.redis_url)?;
            let mut connection = client.get_async_connection().await?;

            if !connection.exists(SCHEMA_VERSION_KEY).await? {
                connection.set(SCHEMA_VERSION_KEY, &SCHEMA_VERSION).await?;
            }

            let db_manager = RedisDbManager {
                client,
                login_prefix: config.login_prefix.clone(),
            };
            Ok(db_manager)
        };

        initialization.map_err(Error::Db).await
    }

    async fn get_login_prefix(&self) -> Result<&str, Error> {
        Ok(&self.login_prefix)
    }

    #[tracing::instrument(skip(self, user_id, name))]
    async fn can_edit_owners(&self, user_id: u32, name: &str) -> Result<bool, Error> {
        check_crate_name(&name)?;

        let entry = self.entry(&name).await?;

        if entry.is_empty() {
            Err(Error::CrateNotFoundInDb(name.to_owned()))
        } else if !entry.owner_ids().contains(&user_id) {
            Err(Error::InvalidUser(user_id))
        } else {
            Ok(true)
        }
    }

    #[tracing::instrument(skip(self, name))]
    async fn owners(&self, name: &str) -> Result<Vec<User>, Error> {
        let users: Vec<User> = self.deserialize(USERS_KEY).await?.unwrap_or_default();
        let entry = self.entry(name).await?;
        let owners = users
            .into_iter()
            .filter(|u| entry.owner_ids().contains(&u.id))
            .collect();
        Ok(owners)
    }

    #[tracing::instrument(skip(self, name, logins))]
    async fn add_owners(&self, name: &str, logins: &[String]) -> Result<(), Error> {
        self.edit_owners(name, logins.iter(), |ids, entry| {
            entry.owner_ids_mut().extend(ids);
            entry.owner_ids_mut().sort_unstable();
            entry.owner_ids_mut().dedup();
        })
        .await
    }

    #[tracing::instrument(skip(self, name, logins))]
    async fn remove_owners(&self, name: &str, logins: &[String]) -> Result<(), Error> {
        self.edit_owners(name, logins.iter(), |ids, entry| {
            entry.owner_ids_mut().retain(|i| !ids.contains(i));
        })
        .await
    }

    #[tracing::instrument(skip(self))]
    async fn last_user_id(&self) -> Result<Option<u32>, Error> {
        let last_user_id = self
            .deserialize(TOKENS_KEY)
            .await?
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
    async fn user_id_for_token(&self, token: &str) -> Result<u32, Error> {
        let token = token.into();
        self.deserialize(TOKENS_KEY)
            .await?
            .and_then(|map: TokenMap| {
                map.iter()
                    .find_map(|(k, v)| if v == &token { Some(*k) } else { None })
            })
            .ok_or_else(|| Error::InvalidToken(token))
    }

    #[tracing::instrument(skip(self, login))]
    async fn token_by_login(&self, login: &str) -> Result<Option<String>, Error> {
        match self.user_by_login(login).await {
            Ok(user) => Ok(self
                .deserialize(TOKENS_KEY)
                .await?
                .and_then(|map: TokenMap| {
                    map.iter().find_map(|(k, v)| {
                        if k == &user.id {
                            Some(v.to_string())
                        } else {
                            None
                        }
                    })
                })),
            Err(_) => Ok(None),
        }
    }

    #[tracing::instrument(skip(self, name))]
    async fn token_by_username(&self, name: &str) -> Result<Option<String>, Error> {
        match self.user_by_username(name).await {
            Ok(user) => Ok(self
                .deserialize(TOKENS_KEY)
                .await?
                .and_then(|map: TokenMap| {
                    map.iter().find_map(|(k, v)| {
                        if k == &user.id {
                            Some(v.to_string())
                        } else {
                            None
                        }
                    })
                })),
            Err(_) => Ok(None),
        }
    }

    #[tracing::instrument(skip(self, user_id, token))]
    async fn set_token(&self, user_id: u32, token: &str) -> Result<(), Error> {
        let token = token.into();
        let mut tokens: TokenMap = self.deserialize(TOKENS_KEY).await?.unwrap_or_default();
        tokens.insert(user_id, token);

        self.insert(TOKENS_KEY, tokens).await
    }

    #[tracing::instrument(skip(self, name))]
    async fn user_by_username(&self, name: &str) -> Result<User, Error> {
        let login = format!("{}{}", self.login_prefix, name);
        self.user_by_login(&login)
            .await
            .map_err(|_| Error::InvalidUsername(name.to_string()))
    }

    #[tracing::instrument(skip(self, login))]
    async fn user_by_login(&self, login: &str) -> Result<User, Error> {
        let login = login.to_owned();
        let mut users: Vec<User> = self.deserialize(USERS_KEY).await?.unwrap_or_default();

        users.sort_by_key(|u| u.login.clone());
        let index = users
            .binary_search_by_key(&login, |u| u.login.clone())
            .map_err(|_| Error::InvalidLogin(login))?;
        Ok(users.remove(index))
    }

    #[tracing::instrument(skip(self, user, password))]
    async fn add_new_user(&self, user: User, password: &str) -> Result<(), Error> {
        let mut users: Vec<User> = self.deserialize(USERS_KEY).await?.unwrap_or_default();
        let mut passwords: HashMap<u32, String> =
            self.deserialize(PASSWORDS_KEY).await?.unwrap_or_default();

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
    async fn verify_password(&self, user_id: u32, password: &str) -> Result<bool, Error> {
        let passwords: HashMap<u32, String> =
            self.deserialize(PASSWORDS_KEY).await?.unwrap_or_default();

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
    async fn change_password(
        &self,
        user_id: u32,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), Error> {
        if old_password == new_password {
            return Err(Error::SamePasswords);
        }

        let mut passwords: HashMap<u32, String> =
            self.deserialize(PASSWORDS_KEY).await?.unwrap_or_default();

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

    #[tracing::instrument(skip(self, user_id, name, version))]
    async fn can_add_metadata(
        &self,
        user_id: u32,
        name: &str,
        version: Version,
    ) -> Result<bool, Error> {
        check_crate_name(name)?;

        let entry = self.entry(name).await?;

        if entry.is_empty() {
            return Ok(true);
        } else if !entry.owner_ids().contains(&user_id) {
            return Err(Error::InvalidUser(user_id));
        } else if entry.versions().contains_key(&version) {
            return Err(Error::VersionExists(name.to_owned(), version));
        }

        let can_add_metadata = entry
            .latest_version()
            .and_then(|v| entry.versions().get(v))
            .map(|p| name == p.name)
            .expect("latest version must exists");
        Ok(can_add_metadata)
    }

    #[tracing::instrument(skip(self, owner_id, metadata))]
    async fn add_new_metadata(&self, owner_id: u32, metadata: Metadata) -> Result<(), Error> {
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
    async fn can_edit_package(
        &self,
        user_id: u32,
        name: &str,
        version: Version,
    ) -> Result<bool, Error> {
        check_crate_name(name)?;

        let entry = self.entry(name).await?;

        if entry.is_empty() {
            return Err(Error::CrateNotFoundInDb(name.to_owned()));
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

    #[tracing::instrument(skip(self, name, version))]
    async fn yank(&self, name: &str, version: Version) -> Result<(), Error> {
        self.change_yanked(name, version, true, Error::AlreadyYanked)
            .await
    }

    #[tracing::instrument(skip(self, name, version))]
    async fn unyank(&self, name: &str, version: Version) -> Result<(), Error> {
        self.change_yanked(name, version, false, Error::NotYetYanked)
            .await
    }

    #[tracing::instrument(skip(self, query))]
    async fn search(&self, query: &Query) -> Result<Search, Error> {
        let mut connection = self
            .client
            .get_async_connection()
            .map_err(Error::Db)
            .await?;
        let entries: HashMap<String, String> =
            connection.hgetall(ENTRIES_KEY).map_err(Error::Db).await?;
        let (entries, errors): (HashMap<_, _>, HashMap<_, _>) = entries
            .into_iter()
            .map(|(name, json_string)| {
                let entry = serde_json::from_str::<Entry>(&json_string).map_err(Error::InvalidJson);
                (name, entry)
            })
            .partition(|(_, result)| result.is_ok());

        if errors.is_empty() {
            let query_string = normalized_crate_name(&query.string);
            let filtered: Vec<_> = entries
                .into_iter()
                .map(|(name, result)| (name, result.expect("must be ok")))
                .filter_map(|(name, entry)| {
                    if name.contains(&query_string) {
                        let (_, latest_version) = entry
                            .versions()
                            .iter()
                            .filter(|(_, metadata)| !metadata.yanked)
                            .max_by_key(|(key, _)| *key)?;
                        Some(latest_version.to_searched())
                    } else {
                        None
                    }
                })
                .collect();

            let count = filtered.len();
            let filtered: Vec<_> = filtered.into_iter().take(query.limit).collect();

            Ok(Search::new(filtered, count))
        } else {
            let errors: Vec<_> = errors.into_iter().map(|(_, result)| result).collect();
            Err(Error::multiple(errors))
        }
    }

    #[cfg(feature = "openid")]
    async fn store_nonce_by_csrf(
        &self,
        state: openidconnect::CsrfToken,
        nonce: openidconnect::Nonce,
    ) -> Result<(), Error> {
        let mut nonces: HashMap<String, openidconnect::Nonce> = self
            .deserialize(OAUTH_NONCES_KEY)
            .await?
            .unwrap_or_default();
        // TODO: check if nonces already contains state.secret()
        nonces.insert(state.secret().to_string(), nonce);
        self.insert(OAUTH_NONCES_KEY, nonces).await
    }

    #[cfg(feature = "openid")]
    async fn get_nonce_by_csrf(
        &self,
        state: openidconnect::CsrfToken,
    ) -> Result<openidconnect::Nonce, Error> {
        let mut nonces: HashMap<String, openidconnect::Nonce> = self
            .deserialize(OAUTH_NONCES_KEY)
            .await?
            .unwrap_or_default();
        let ret = nonces
            .remove(state.secret())
            .ok_or_else(|| Error::InvalidCsrfToken(state.secret().to_string()))?;
        self.insert(OAUTH_NONCES_KEY, nonces).await?;
        Ok(ret)
    }
}

impl RedisDbManager {
    #[tracing::instrument(skip(self, name, logins, editor))]
    async fn edit_owners<N, L, S, E>(&self, name: N, logins: L, editor: E) -> Result<(), Error>
    where
        N: Into<String>,
        L: Iterator<Item = S>,
        S: Into<String>,
        E: FnOnce(&[u32], &mut Entry),
    {
        let mut users: Vec<User> = self.deserialize(USERS_KEY).await?.unwrap_or_default();
        users.sort_by_key(|u| u.login.clone());

        let (ids, errors): (Vec<_>, Vec<_>) = logins
            .map(Into::into)
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

    #[tracing::instrument(skip(self, name))]
    async fn entry(&self, name: &str) -> Result<Entry, Error> {
        let normalized_crate_name = normalized_crate_name(name);
        let mut connection = self
            .client
            .get_async_connection()
            .map_err(Error::Db)
            .await?;
        let entry: Option<String> = connection
            .hget(ENTRIES_KEY, &normalized_crate_name)
            .map_err(Error::Db)
            .await?;
        let entry: Option<Entry> = entry
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(Error::InvalidJson)?;

        Ok(entry.unwrap_or_default())
    }

    #[tracing::instrument(skip(self, name, version, yanked, no_changed_error_closure))]
    async fn change_yanked<F>(
        &self,
        name: &str,
        version: Version,
        yanked: bool,
        no_changed_error_closure: F,
    ) -> Result<(), Error>
    where
        F: FnOnce(String, Version) -> Error,
    {
        let entry = self
            .entry(name)
            .and_then(|mut entry| async move {
                let package = entry
                    .package_mut(&version)
                    .ok_or_else(|| Error::VersionNotFoundInDb(version.clone()))?;

                if package.yanked == yanked {
                    Err(no_changed_error_closure(name.to_owned(), version))
                } else {
                    package.yanked = yanked;
                    Ok(entry)
                }
            })
            .await?;

        self.insert_entry(name, entry).await
    }

    #[tracing::instrument(skip(self, key))]
    async fn deserialize<T>(&self, key: &str) -> Result<Option<T>, Error>
    where
        T: DeserializeOwned,
    {
        let mut connection = self
            .client
            .get_async_connection()
            .map_err(Error::Db)
            .await?;
        let string: Option<String> = connection.get(key).map_err(Error::Db).await?;
        string
            .map(|s| serde_json::from_str::<T>(&s))
            .transpose()
            .map_err(Error::InvalidJson)
    }

    #[tracing::instrument(skip(self, name, entry))]
    async fn insert_entry<'a>(&self, name: &str, entry: Entry) -> Result<(), Error> {
        let normalized_crate_name = normalized_crate_name(name);
        let json_string = serde_json::to_string(&entry).map_err(Error::Serialization)?;

        let insertion = async {
            let mut connection = self.client.get_async_connection().await?;
            connection
                .hset(ENTRIES_KEY, normalized_crate_name, json_string)
                .await?;
            Ok(())
        };

        insertion.map_err(Error::Db).await
    }

    #[tracing::instrument(skip(self, key, value))]
    async fn insert(&self, key: &str, value: impl Serialize) -> Result<(), Error> {
        let json_string = serde_json::to_string(&value).map_err(Error::Serialization)?;

        let insertion = async {
            let mut connection = self.client.get_async_connection().await?;
            connection.set(key, json_string).await?;
            Ok(())
        };

        insertion.map_err(Error::Db).await
    }
}
