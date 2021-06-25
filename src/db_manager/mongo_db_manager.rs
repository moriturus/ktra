#![cfg(feature = "db-mongo")]

use crate::config::DbConfig;
use crate::error::Error;
use crate::models::{Entry, Metadata, Query, Search, User};
use argon2::{self, hash_encoded, verify_encoded};
use async_trait::async_trait;
use bson::{doc, from_document, to_document, Document};
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::TryFutureExt;
use mongodb::{
    options::{ClientOptions, UpdateOptions},
    Client,
};
use semver::Version;
use serde::ser::Serialize;
use serde::{Deserialize as DeserializeTrait, Serialize as SerializeTrait};
use url::Url;

use crate::db_manager::utils::{argon2_config_and_salt, check_crate_name, normalized_crate_name};
use crate::db_manager::DbManager;

const SCHEMA_VERSION_KEY: &str = "__SCHEMA_VERSION__";
const SCHEMA_VERSION: i64 = 1;
const ENTRIES_KEY: &str = "__ENTRIES__";
const USERS_KEY: &str = "__USERS__";
const PASSWORDS_KEY: &str = "__PASSWORDS__";
const TOKENS_KEY: &str = "__TOKENS__";
const OAUTH_NONCES_KEY: &str = "__OAUTH_NONCES__";

#[derive(Clone, SerializeTrait, DeserializeTrait)]
struct TokenMap {
    id: u32,
    token: String,
}

#[derive(Clone, SerializeTrait, DeserializeTrait)]
struct PasswordMap {
    id: u32,
    password: String,
}

#[derive(Debug, Clone, SerializeTrait, DeserializeTrait)]
struct EntryMap {
    name: String,
    entry: Entry,
}

pub struct MongoDbManager {
    client: Client,
    database_name: String,
    login_prefix: String,
}

#[async_trait]
impl DbManager for MongoDbManager {
    #[tracing::instrument(skip(config))]
    async fn new(config: &DbConfig) -> Result<MongoDbManager, Error> {
        tracing::info!("connect to MongoDB server: {}", config.mongodb_url);

        let url = Url::parse(&config.mongodb_url).map_err(Error::UrlParsing)?;
        let database_name = url
            .path_segments()
            .and_then(|s| s.last())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| "ktra".to_owned());

        let initialization = async {
            let options = ClientOptions::parse(url.as_str()).await?;
            let client = Client::with_options(options)?;
            let db = client.database(&database_name);
            let collection = db.collection(SCHEMA_VERSION_KEY);

            if collection.estimated_document_count(None).await? == 0 {
                collection
                    .insert_one(doc! { "version": SCHEMA_VERSION }, None)
                    .await?;
            }

            let db_manager = MongoDbManager {
                client,
                database_name,
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
        let normalized_crate_name = normalized_crate_name(name);
        let collection = self
            .client
            .database(&self.database_name)
            .collection(ENTRIES_KEY);
        let cursor = collection
            .aggregate(
                vec![
                    doc! {
                        "$match": {
                            "name": normalized_crate_name
                        }
                    },
                    doc! {
                        "$lookup": {
                            "from": USERS_KEY,
                            "localField": "owner_ids",
                            "foreignField": "id",
                            "as": "users"
                        }
                    },
                    doc! {
                        "$unwind": "$users"
                    },
                    doc! {
                        "$project": {
                            "_id": false,
                            "versions": false,
                            "owner_ids": false,
                            "id": "$users.id",
                            "login": "$users.login",
                            "name": "$users.name"
                        }
                    },
                ],
                None,
            )
            .map_err(Error::Db)
            .await?;
        let results: Vec<Result<User, Error>> = cursor
            .map_err(Error::Db)
            .map(|d| d.and_then(|d| from_document::<User>(d).map_err(Error::BsonDeserialization)))
            .collect()
            .await;
        let (owners, errors): (Vec<_>, Vec<_>) = results.into_iter().partition(Result::is_ok);

        if errors.is_empty() {
            let owners = owners.into_iter().map(Result::unwrap).collect();
            Ok(owners)
        } else {
            Err(Error::multiple(errors))
        }
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
        let collection = self
            .client
            .database(&self.database_name)
            .collection(TOKENS_KEY);
        let mut cursor = collection
            .aggregate(
                vec![doc! {
                    "$group": {
                        "_id": null,
                        "last": {
                            "$max": "$id"
                        }
                    }
                }],
                None,
            )
            .map_err(Error::Db)
            .await?;
        let last_user_id = cursor
            .next()
            .await
            .transpose()
            .map_err(Error::Db)?
            .and_then(|d| d.get("last").cloned())
            .and_then(|b| b.as_i64())
            .map(|i| i as u32);
        Ok(last_user_id)
    }

    #[tracing::instrument(skip(self, token))]
    async fn user_id_for_token(&self, token: &str) -> Result<u32, Error> {
        let collection = self
            .client
            .database(&self.database_name)
            .collection(TOKENS_KEY);
        collection
            .find_one(doc! { "token": token }, None)
            .map_err(Error::Db)
            .await?
            .and_then(|d| d.get("id").cloned())
            .and_then(|b| b.as_i64())
            .map(|i| i as u32)
            .ok_or_else(|| Error::InvalidToken(token.to_owned()))
    }

    #[tracing::instrument(skip(self, login))]
    async fn token_by_login(&self, login: &str) -> Result<Option<String>, Error> {
        match self.user_by_login(login).await {
            Ok(user) => {
                let collection = self
                    .client
                    .database(&self.database_name)
                    .collection(TOKENS_KEY);
                Ok(collection
                    .find_one(doc! { "id": user.id }, None)
                    .map_err(Error::Db)
                    .await?
                    .and_then(|d| d.get("token").cloned())
                    .and_then(|b| b.as_str().map(ToString::to_string)))
            }
            Err(_) => Ok(None),
        }
    }

    #[tracing::instrument(skip(self, name))]
    async fn token_by_username(&self, name: &str) -> Result<Option<String>, Error> {
        match self.user_by_username(name).await {
            Ok(user) => {
                let collection = self
                    .client
                    .database(&self.database_name)
                    .collection(TOKENS_KEY);
                Ok(collection
                    .find_one(doc! { "id": user.id }, None)
                    .map_err(Error::Db)
                    .await?
                    .and_then(|d| d.get("token").cloned())
                    .and_then(|b| b.as_str().map(ToString::to_string)))
            }
            Err(_) => Ok(None),
        }
    }

    #[tracing::instrument(skip(self, user_id, token))]
    async fn set_token(&self, user_id: u32, token: &str) -> Result<(), Error> {
        let token = token.to_owned();
        let token_map = TokenMap { id: user_id, token };
        self.update_or_insert_one(TOKENS_KEY, doc! { "id": user_id }, token_map)
            .await
    }

    #[tracing::instrument(skip(self, name))]
    async fn user_by_username(&self, name: &str) -> Result<User, Error> {
        let name = name.to_owned();
        let login = format!("{}{}", self.login_prefix, name);
        self.user_by_login(&login)
            .await
            .map_err(|_| Error::InvalidUsername(name.to_string()))
    }

    #[tracing::instrument(skip(self, login))]
    async fn user_by_login(&self, login: &str) -> Result<User, Error> {
        let login = login.to_owned();
        let collection = self
            .client
            .database(&self.database_name)
            .collection(USERS_KEY);

        collection
            .find_one(doc! { "login": login.clone() }, None)
            .map_err(Error::Db)
            .await?
            .map(from_document::<User>)
            .transpose()
            .map_err(Error::BsonDeserialization)?
            .ok_or_else(|| Error::InvalidLogin(login))
    }

    #[tracing::instrument(skip(self, user, password))]
    async fn add_new_user(&self, user: User, password: &str) -> Result<(), Error> {
        let user_id = user.id;
        let users_collection = self
            .client
            .database(&self.database_name)
            .collection(USERS_KEY);
        let user_query_document = doc! {"login": user.login.clone() };

        if users_collection
            .find_one(user_query_document.clone(), None)
            .map_err(Error::Db)
            .await?
            .is_some()
        {
            return Err(Error::UserExists(user.login));
        } else {
            self.update_or_insert_one(USERS_KEY, user_query_document, user)
                .await?;
        }

        let (config, salt) = argon2_config_and_salt().await?;
        let encoded_password =
            hash_encoded(password.as_bytes(), salt.as_bytes(), &config).map_err(Error::Argon2)?;
        let password_map = PasswordMap {
            id: user_id,
            password: encoded_password,
        };
        self.update_or_insert_one(PASSWORDS_KEY, doc! { "id": user_id }, password_map)
            .await
    }

    #[tracing::instrument(skip(self, user_id, password))]
    async fn verify_password(&self, user_id: u32, password: &str) -> Result<bool, Error> {
        let collection = self
            .client
            .database(&self.database_name)
            .collection(PASSWORDS_KEY);
        let encoded_password = collection
            .find_one(doc! { "id": user_id }, None)
            .map_err(Error::Db)
            .await?
            .map(from_document::<PasswordMap>)
            .transpose()
            .map_err(Error::BsonDeserialization)?
            .map(|p| p.password);

        if let Some(result) = encoded_password.map(|e| verify_encoded(&e, password.as_bytes())) {
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

        let collection = self
            .client
            .database(&self.database_name)
            .collection(PASSWORDS_KEY);
        let encoded_old_password = collection
            .find_one(doc! { "id": user_id }, None)
            .map_err(Error::Db)
            .await?
            .map(from_document::<PasswordMap>)
            .transpose()
            .map_err(Error::BsonDeserialization)?
            .map(|p| p.password);

        if let Some(encoded_old_password) = encoded_old_password {
            if verify_encoded(&encoded_old_password, old_password.as_bytes())
                .map_err(Error::Argon2)?
            {
                let (config, salt) = argon2_config_and_salt().await?;
                let encoded_new_password =
                    hash_encoded(new_password.as_bytes(), salt.as_bytes(), &config)
                        .map_err(Error::Argon2)?;
                let password_map = PasswordMap {
                    id: user_id,
                    password: encoded_new_password,
                };
                self.update_or_insert_one(PASSWORDS_KEY, doc! { "id": user_id }, password_map)
                    .await
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
        let query_string = normalized_crate_name(&query.string);
        let collection = self
            .client
            .database(&self.database_name)
            .collection(ENTRIES_KEY);
        let cursor = collection
            .find(
                Some(doc! {
                    "name": {
                        "$regex": query_string,
                        "$options": "i"
                    }
                }),
                None,
            )
            .map_err(Error::Db)
            .await?;
        let (entries, errors): (Vec<_>, Vec<_>) = cursor
            .map_err(Error::Db)
            .and_then(|document| async {
                from_document::<EntryMap>(document).map_err(Error::BsonDeserialization)
            })
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .partition(Result::is_ok);

        if errors.is_empty() {
            let filtered: Vec<_> = entries
                .into_iter()
                .map(Result::unwrap)
                .filter_map(|entry_map| {
                    let (_, latest_version) = entry_map
                        .entry
                        .versions()
                        .iter()
                        .filter(|(_, metadata)| !metadata.yanked)
                        .max_by_key(|(key, _)| *key)?;
                    Some(latest_version.to_searched())
                })
                .collect();

            let count = filtered.len();
            let filtered: Vec<_> = filtered.into_iter().take(query.limit).collect();

            Ok(Search::new(filtered, count))
        } else {
            Err(Error::multiple(errors))
        }
    }

    #[cfg(feature = "openid")]
    async fn store_nonce_by_csrf(
        &self,
        state: openidconnect::CsrfToken,
        nonce: openidconnect::Nonce,
    ) -> Result<(), Error> {
        let collection = self
            .client
            .database(&self.database_name)
            .collection(OAUTH_NONCES_KEY);
        let nonces_query_document = doc! {"state": state.secret().to_string() };

        self.update_or_insert_one(OAUTH_NONCES_KEY, nonces_query_document, nonce)
            .await
    }

    #[cfg(feature = "openid")]
    async fn get_nonce_by_csrf(
        &self,
        state: openidconnect::CsrfToken,
    ) -> Result<openidconnect::Nonce, Error> {
        let collection = self
            .client
            .database(&self.database_name)
            .collection(OAUTH_NONCES_KEY);

        collection
            .find_one(doc! { "state": state.secret().to_string() }, None)
            .map_err(Error::Db)
            .await?
            .map(from_document::<openidconnect::Nonce>)
            .transpose()
            .map_err(Error::BsonDeserialization)?
            .ok_or_else(|| Error::InvalidCsrfToken(state.secret().to_string()))
    }
}

impl MongoDbManager {
    #[tracing::instrument(skip(self, name, logins, editor))]
    async fn edit_owners<N, L, S, E>(&self, name: N, logins: L, editor: E) -> Result<(), Error>
    where
        N: Into<String>,
        L: Iterator<Item = S>,
        S: Into<String>,
        E: FnOnce(&[u32], &mut Entry),
    {
        let logins: Vec<_> = logins.map(Into::into).collect();
        let collection = self
            .client
            .database(&self.database_name)
            .collection(USERS_KEY);
        let cursor = collection
            .find(
                doc! {
                    "login": {
                        "$in": logins.clone()
                    }
                },
                None,
            )
            .map_err(Error::Db)
            .await?;
        let (ids, errors): (Vec<_>, Vec<_>) = cursor
            .map_err(Error::Db)
            .and_then(|d| async { from_document::<User>(d).map_err(Error::BsonDeserialization) })
            .map_ok(|u| u.id)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .partition(Result::is_ok);

        if ids.is_empty() {
            return Err(Error::InvalidLoginNames(logins));
        }

        if errors.is_empty() {
            let name = name.into();
            let mut entry: Entry = self.entry(&name).await?;

            let ids: Vec<_> = ids.into_iter().map(Result::unwrap).collect();
            editor(&ids, &mut entry);

            self.insert_entry(&name, entry).await
        } else {
            Err(Error::multiple(errors))
        }
    }

    #[tracing::instrument(skip(self, name))]
    async fn entry(&self, name: &str) -> Result<Entry, Error> {
        let normalized_crate_name = normalized_crate_name(name);
        let collection = self
            .client
            .database(&self.database_name)
            .collection(ENTRIES_KEY);
        let entry = collection
            .find_one(doc! { "name": normalized_crate_name }, None)
            .map_err(Error::Db)
            .await?
            .and_then(|d| d.get("entry").and_then(|b| b.as_document()).cloned())
            .map(from_document::<Entry>)
            .transpose()
            .map_err(Error::BsonDeserialization)?
            .unwrap_or_default();
        Ok(entry)
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

    #[tracing::instrument(skip(self, name, entry))]
    async fn insert_entry<'a>(&self, name: &str, entry: Entry) -> Result<(), Error> {
        let normalized_crate_name = normalized_crate_name(name);
        let document = to_document(&entry).map_err(Error::BsonSerialization)?;
        let document = doc! { "name": normalized_crate_name.clone(), "entry": document };

        let insertion = async {
            let db = self.client.database(&self.database_name);
            let collection = db.collection(ENTRIES_KEY);
            let options = UpdateOptions::builder().upsert(true).build();
            collection
                .update_one(
                    doc! { "name": normalized_crate_name },
                    document,
                    Some(options),
                )
                .map_ok(drop)
                .await
        };

        insertion.map_err(Error::Db).await
    }

    #[tracing::instrument(skip(self, collection_name, query, value))]
    async fn update_or_insert_one(
        &self,
        collection_name: &str,
        query: Document,
        value: impl Serialize,
    ) -> Result<(), Error> {
        let document = to_document(&value).map_err(Error::BsonSerialization)?;

        let insertion = async {
            let db = self.client.database(&self.database_name);
            let collection = db.collection(collection_name);
            let options = UpdateOptions::builder().upsert(true).build();
            collection
                .update_one(query, document, Some(options))
                .map_ok(drop)
                .await
        };

        insertion.map_err(Error::Db).await
    }
}
