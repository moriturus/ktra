#![cfg(feature = "openid")]

use crate::config::OpenIdConfig;
use crate::db_manager::DbManager;
use crate::error::Error;
use crate::models::{Claims, CodeQuery, User};
use crate::utils::*;
use futures::TryFutureExt;
use openidconnect::core::{
    CoreClient, CoreGenderClaim, CoreIdTokenClaims, CoreIdTokenVerifier, CoreProviderMetadata,
    CoreResponseType,
};
use openidconnect::{
    AdditionalClaims, AuthenticationFlow, AuthorizationCode, ClientId, ClientSecret, CsrfToken,
    IssuerUrl, Nonce, OAuth2TokenResponse, RedirectUrl, Scope, UserInfoClaims,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use warp::{Filter, Rejection, Reply};

impl AdditionalClaims for Claims {}

#[tracing::instrument(skip(db_manager, openid_config))]
pub fn apis(
    db_manager: Arc<RwLock<impl DbManager>>,
    openid_config: Arc<OpenIdConfig>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    authenticate(db_manager.clone(), openid_config.clone())
        .or(me(db_manager.clone(), openid_config.clone()))
        .or(handle_replace_token(
            db_manager.clone(),
            openid_config.clone(),
        ))
        .or(replace_token(db_manager, openid_config))
}

#[tracing::instrument(skip(db_manager, openid_config))]
fn authenticate(
    db_manager: Arc<RwLock<impl DbManager>>,
    openid_config: Arc<OpenIdConfig>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::get()
        .and(with_db_manager(db_manager))
        .and(with_openid_config(openid_config))
        .and(warp::path!("ktra" / "api" / "v1" / "openid" / "me"))
        .and(warp::query::<CodeQuery>())
        .and_then(validate)
}

#[tracing::instrument(skip(db_manager, openid_config))]
fn handle_replace_token(
    db_manager: Arc<RwLock<impl DbManager>>,
    openid_config: Arc<OpenIdConfig>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::get()
        .and(with_db_manager(db_manager))
        .and(with_openid_config(openid_config))
        .and(warp::path!("ktra" / "api" / "v1" / "openid" / "replace"))
        .and(warp::query::<CodeQuery>())
        .and_then(validate_and_replace)
}

#[tracing::instrument(skip(db_manager, openid_config))]
fn me(
    db_manager: Arc<RwLock<impl DbManager>>,
    openid_config: Arc<OpenIdConfig>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::get()
        .and(with_db_manager(db_manager))
        .and(with_openid_config(openid_config))
        .and(warp::path!("me"))
        .and_then(initiate_openid)
}

#[tracing::instrument(skip(db_manager, openid_config))]
fn replace_token(
    db_manager: Arc<RwLock<impl DbManager>>,
    openid_config: Arc<OpenIdConfig>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::get()
        .and(with_db_manager(db_manager))
        .and(with_openid_config(openid_config))
        .and(warp::path!("replace_token"))
        .and_then(replace_openid)
}

#[tracing::instrument(skip(db_manager, openid_config))]
async fn initiate_openid(
    db_manager: Arc<RwLock<impl DbManager>>,
    openid_config: Arc<OpenIdConfig>,
) -> Result<warp::reply::Response, Rejection> {
    start_openid_with_redirect(db_manager, openid_config, "ktra/api/v1/openid/me").await
}

#[tracing::instrument(skip(db_manager, openid_config))]
async fn replace_openid(
    db_manager: Arc<RwLock<impl DbManager>>,
    openid_config: Arc<OpenIdConfig>,
) -> Result<warp::reply::Response, Rejection> {
    start_openid_with_redirect(db_manager, openid_config, "ktra/api/v1/openid/replace").await
}

#[tracing::instrument(skip(db_manager, openid_config))]
async fn start_openid_with_redirect(
    db_manager: Arc<RwLock<impl DbManager>>,
    openid_config: Arc<OpenIdConfig>,
    redirect_path: &str,
) -> Result<warp::reply::Response, Rejection> {
    let db_manager = db_manager.write().await;

    let client = get_openid_client(openid_config.clone(), redirect_path).await?;

    let mut url_builder = client.authorize_url(
        AuthenticationFlow::<CoreResponseType>::AuthorizationCode,
        CsrfToken::new_random,
        Nonce::new_random,
    );
    for scope in openid_config.additional_scopes.iter().cloned() {
        url_builder = url_builder.add_scope(Scope::new(scope));
    }
    let (authorize_url, csrf_state, nonce) = url_builder.url();

    // Store the nonce for comparison later in the redirect endpoint
    db_manager.store_nonce_by_csrf(csrf_state, nonce).await?;

    Ok(warp::redirect::temporary(
        authorize_url
            .as_str()
            .parse::<openidconnect::http::Uri>()
            .unwrap(),
    )
    .into_response())
}

#[tracing::instrument(skip(db_manager, openid_config, query))]
async fn validate(
    db_manager: Arc<RwLock<impl DbManager>>,
    openid_config: Arc<OpenIdConfig>,
    query: CodeQuery,
) -> Result<warp::reply::Response, Rejection> {
    finish_openid_with_redirect(
        db_manager,
        openid_config,
        query,
        "ktra/api/v1/openid/me",
        false,
    )
    .await
}

#[tracing::instrument(skip(db_manager, openid_config, query))]
async fn validate_and_replace(
    db_manager: Arc<RwLock<impl DbManager>>,
    openid_config: Arc<OpenIdConfig>,
    query: CodeQuery,
) -> Result<warp::reply::Response, Rejection> {
    finish_openid_with_redirect(
        db_manager,
        openid_config,
        query,
        "ktra/api/v1/openid/replace",
        true,
    )
    .await
}

#[tracing::instrument(skip(db_manager, openid_config, query))]
async fn finish_openid_with_redirect(
    db_manager: Arc<RwLock<impl DbManager>>,
    openid_config: Arc<OpenIdConfig>,
    query: CodeQuery,
    redirect_path: &str,
    revoke_old_token: bool,
) -> Result<warp::reply::Response, Rejection> {
    let client = get_openid_client(openid_config.clone(), redirect_path).await?;

    let code = AuthorizationCode::new(query.code);
    let state = CsrfToken::new(query.state.unwrap());
    let nonce = db_manager.write().await.get_nonce_by_csrf(state).await?;
    let token_response = client
        .exchange_code(code)
        .request_async(openidconnect::reqwest::async_http_client)
        .await
        .map_err(|_| {
            warp::reject::custom(Error::OpenId(
                "Failed to contact token endpoint".to_string(),
            ))
        })?;

    let id_token_verifier: CoreIdTokenVerifier = client.id_token_verifier();
    let id_token_claims: &CoreIdTokenClaims = token_response
        .extra_fields()
        .id_token()
        .ok_or_else(|| {
            warp::reject::custom(Error::OpenId(
                "Server did not return an ID token".to_string(),
            ))
        })?
        .claims(&id_token_verifier, &nonce)
        .map_err(|_| {
            warp::reject::custom(Error::OpenId("Failed to verify ID token".to_string()))
        })?;

    let userinfo_claims: UserInfoClaims<Claims, CoreGenderClaim> = client
        .user_info(token_response.access_token().to_owned(), None)
        .map_err(|_| warp::reject::custom(Error::OpenId("No user info endpoint".to_string())))?
        .request_async(openidconnect::reqwest::async_http_client)
        .await
        .map_err(|_| {
            warp::reject::custom(Error::OpenId("Failed requesting user info".to_string()))
        })?;

    if !check_user_authorization(openid_config, id_token_claims, &userinfo_claims) {
        Err(warp::reject::custom(Error::OpenId(
            "Unauthorized user for publishing/owning rights".to_string(),
        )))
    } else {
        handle_authorized_user(
            db_manager,
            id_token_claims,
            &userinfo_claims,
            revoke_old_token,
        )
        .await
    }
}

#[tracing::instrument(skip(openid_config))]
async fn get_openid_client(
    openid_config: Arc<OpenIdConfig>,
    redirect_path: &str,
) -> Result<CoreClient, Rejection> {
    let issuer = IssuerUrl::new(openid_config.issuer_url.clone())
        .map_err(|_| warp::reject::custom(Error::OpenId("Invalid issuer URL".to_string())))?;
    let redirect_url = format!("{}/{}", openid_config.redirect_url, redirect_path);
    let provider_metadata =
        CoreProviderMetadata::discover_async(issuer, openidconnect::reqwest::async_http_client)
            .map_err(|_| {
                warp::reject::custom(Error::OpenId(
                    "Failed to discover OpenID Provider".to_string(),
                ))
            })
            .await?;

    Ok(CoreClient::from_provider_metadata(
        provider_metadata,
        ClientId::new(openid_config.client_id.to_string()),
        Some(ClientSecret::new(openid_config.client_secret.to_string())),
    )
    .set_redirect_uri(
        RedirectUrl::new(redirect_url)
            .map_err(|_| warp::reject::custom(Error::OpenId("Invalid redirect URL".to_string())))?,
    ))
}

#[tracing::instrument(skip(openid_config, _id_token, userinfo))]
fn check_user_authorization<GC: openidconnect::GenderClaim>(
    openid_config: Arc<OpenIdConfig>,
    _id_token: &CoreIdTokenClaims,
    userinfo: &UserInfoClaims<Claims, GC>,
) -> bool {
    if openid_config
        .gitlab_authorized_users
        .as_ref()
        .map(Vec::is_empty)
        .unwrap_or(true)
        && openid_config
            .gitlab_authorized_groups
            .as_ref()
            .map(Vec::is_empty)
            .unwrap_or(true)
    {
        tracing::info!("no openid config authorization restrictions, authorizing.");
        return true;
    }
    if let Some(ref auth_groups) = openid_config.gitlab_authorized_groups {
        if let Some(ref groups) = userinfo.additional_claims().groups {
            for group in groups {
                if auth_groups.contains(group) {
                    tracing::info!("matched authorized group {}, authorizing.", group);
                    return true;
                }
            }
        }
    }
    if let Some(ref auth_users) = openid_config.gitlab_authorized_users {
        if auth_users.contains(
            &userinfo
                .nickname()
                .map(|nick| nick.get(None).unwrap().as_str().to_string())
                .unwrap_or_default(),
        ) {
            tracing::info!("matched authorized nickname, authorizing.");
            return true;
        }
    }
    return false;
}

#[tracing::instrument(skip(db_manager, userinfo))]
async fn handle_authorized_user<GC: openidconnect::GenderClaim>(
    db_manager: Arc<RwLock<impl DbManager>>,
    id_token: &CoreIdTokenClaims,
    userinfo: &UserInfoClaims<Claims, GC>,
    revoke_old_token: bool,
) -> Result<warp::reply::Response, Rejection> {
    let issuer = id_token.issuer().url().host_str().ok_or_else(|| {
        warp::reject::custom(Error::OpenId("Invalid scheme for issuer URL".to_string()))
    })?;
    let name = userinfo
        .nickname()
        .ok_or_else(|| {
            warp::reject::custom(Error::OpenId(
                "No nickname available for registration in Ktra".to_string(),
            ))
        })?
        .get(None)
        .ok_or_else(|| {
            warp::reject::custom(Error::OpenId(
                "No nickname available for default locale registration in Ktra".to_string(),
            ))
        })?
        .as_str();

    let user = get_or_create_user(db_manager.clone(), issuer, name).await?;
    let existing_token = db_manager.read().await.token_by_login(&user.login).await?;

    if revoke_old_token || existing_token.is_none() {
        let new_token = random_alphanumeric_string(32)
            .map_err(warp::reject::custom)
            .await?;
        db_manager
            .write()
            .await
            .set_token(user.id, &new_token)
            .map_err(warp::reject::custom)
            .await?;

        Ok(warp::reply::json(&serde_json::json!({
            "username": user.login,
            "new_token": new_token,
            "revoked_token": existing_token
        }))
        .into_response())
    } else {
        Ok(warp::reply::json(&serde_json::json!({
            "username": user.login,
            "existing_token": existing_token.expect("existing_token is Some(_) in this branch.")
        }))
        .into_response())
    }
}

async fn get_or_create_user(
    db_manager: Arc<RwLock<impl DbManager>>,
    issuer: &str,
    name: &str,
) -> Result<User, Rejection> {
    let db_manager = db_manager.write().await;

    let login_id = format!("{}:{}", issuer, name);

    if let Ok(user) = db_manager.user_by_login(&login_id).await {
        return Ok(user);
    }

    let user_id = db_manager
        .last_user_id()
        .map_ok(|user_id| user_id.map(|u| u + 1).unwrap_or(0))
        .map_err(warp::reject::custom)
        .await?;
    let user = User::new(user_id, login_id, Some(name));

    db_manager
        .add_new_user(
            user.clone(),
            "passphrases are unsupported with openid feature",
        )
        .map_err(warp::reject::custom)
        .await?;
    Ok(user)
}
