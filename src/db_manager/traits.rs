use crate::config::DbConfig;
use crate::error::Error;
use crate::models::{Metadata, Query, Search, User};
use async_trait::async_trait;
use semver::Version;

#[async_trait]
pub trait DbManager: Send + Sync + Sized {
    async fn new(confg: &DbConfig) -> Result<Self, Error>;
    async fn get_login_prefix(&self) -> Result<&str, Error>;

    async fn can_edit_owners(&self, user_id: u32, name: &str) -> Result<bool, Error>;
    async fn owners(&self, name: &str) -> Result<Vec<User>, Error>;
    async fn add_owners(&self, name: &str, logins: &[String]) -> Result<(), Error>;
    async fn remove_owners(&self, name: &str, logins: &[String]) -> Result<(), Error>;

    async fn last_user_id(&self) -> Result<Option<u32>, Error>;
    async fn user_id_for_token(&self, token: &str) -> Result<u32, Error>;
    async fn token_by_login(&self, login: &str) -> Result<Option<String>, Error>;
    async fn token_by_username(&self, name: &str) -> Result<Option<String>, Error>;
    async fn set_token(&self, user_id: u32, token: &str) -> Result<(), Error>;
    async fn user_by_username(&self, name: &str) -> Result<User, Error>;
    async fn user_by_login(&self, login: &str) -> Result<User, Error>;
    async fn add_new_user(&self, user: User, password: &str) -> Result<(), Error>;
    async fn verify_password(&self, user_id: u32, password: &str) -> Result<bool, Error>;
    async fn change_password(
        &self,
        user_id: u32,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), Error>;

    async fn can_add_metadata(
        &self,
        user_id: u32,
        name: &str,
        version: Version,
    ) -> Result<bool, Error>;
    async fn add_new_metadata(&self, owner_id: u32, metadata: Metadata) -> Result<(), Error>;

    async fn can_edit_package(
        &self,
        user_id: u32,
        name: &str,
        version: Version,
    ) -> Result<bool, Error>;
    async fn yank(&self, name: &str, version: Version) -> Result<(), Error>;
    async fn unyank(&self, name: &str, version: Version) -> Result<(), Error>;

    async fn search(&self, query: &Query) -> Result<Search, Error>;

    /// Store a nonce associated to a CsrfToken. A single entry is allowed per CsrfToken
    #[cfg(feature = "openid")]
    async fn store_nonce_by_csrf(
        &self,
        state: openidconnect::CsrfToken,
        nonce: openidconnect::Nonce,
    ) -> Result<(), Error>;

    /// Find the nonce associated to a CsrfToken, and remove the association in database.
    #[cfg(feature = "openid")]
    async fn get_nonce_by_csrf(
        &self,
        state: openidconnect::CsrfToken,
    ) -> Result<openidconnect::Nonce, Error>;
}
