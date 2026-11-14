//! GameNode HTTP client.
//!
//! Speaks only endpoints that already exist on the server. The Better Auth ones are described in
//! `docs/auth-contract.md`: `POST /v1/auth/sign-in/social`, the browser-facing
//! `GET /v1/auth/capacitor-authorization-proxy`, `GET /v1/auth/get-session` and
//! `POST /v1/auth/sign-out`. The game endpoints the library calls are in [`game`]. Store
//! integrations such as Steam's session and catalog live in their plugins, not here.

use std::time::Duration;

use async_trait::async_trait;
use gamenode_core::callback::{NATIVE_ORIGIN, Provider};
use gamenode_core::ports::AuthApi;
use gamenode_core::{SessionCredential, SessionError, SignInFailure, User};
use percent_encoding::percent_decode_str;
use reqwest::{Client, StatusCode, header};
use serde::Deserialize;
use url::Url;

/// Server used when `GAMENODE_SERVER_URL` is not set (the production GameNode server).
pub const DEFAULT_SERVER_URL: &str = "https://server.gamenode.app";
/// Environment variable that overrides the server, for development and tests.
pub const SERVER_URL_ENV: &str = "GAMENODE_SERVER_URL";

const AUTH_BASE: &str = "/v1/auth";
const NATIVE_ORIGIN_HEADER: &str = "capacitor-origin";
const OAUTH_STATE_COOKIE: &str = "better-auth.oauth_state";
const SECURE_PREFIX: &str = "__Secure-";

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("the GameNode server URL is invalid")]
    InvalidServerUrl,
    #[error("the HTTP client could not be created")]
    Http,
}

/// Resolves the server base URL from the environment or the default.
pub fn server_url_from_env() -> Result<Url, ClientError> {
    let raw = std::env::var(SERVER_URL_ENV).unwrap_or_else(|_| DEFAULT_SERVER_URL.to_owned());
    Url::parse(&raw)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https") && url.host().is_some())
        .ok_or(ClientError::InvalidServerUrl)
}

mod game;
mod playtime;

#[derive(Clone)]
pub struct GameNodeClient {
    http: Client,
    base: Url,
}

impl GameNodeClient {
    pub fn new(base: Url) -> Result<Self, ClientError> {
        Self::with_timeout(base, Duration::from_secs(20))
    }

    pub fn with_timeout(base: Url, timeout: Duration) -> Result<Self, ClientError> {
        let http = Client::builder()
            .user_agent(concat!("GameNode-Desktop/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .timeout(timeout)
            // Redirects are never followed: the sign-in response must come straight from the server.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| ClientError::Http)?;
        Ok(Self { http, base })
    }

    fn endpoint(&self, path: &str) -> Url {
        let mut url = self.base.clone();
        let prefix = url.path().trim_end_matches('/').to_owned();
        url.set_path(&format!("{prefix}{AUTH_BASE}{path}"));
        url.set_query(None);
        url
    }

    /// The URL the system browser must open: the server's authorization proxy, which sets the
    /// browser's OAuth-state cookie (our HTTP request cannot) and then redirects to the provider.
    pub fn proxy_url(&self, authorization_url: &Url, oauth_state: Option<&str>) -> Url {
        let mut url = self.endpoint("/capacitor-authorization-proxy");
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("authorizationURL", authorization_url.as_str());
            if let Some(state) = oauth_state {
                query.append_pair("oauthState", state);
            }
        }
        url
    }
}

#[derive(Deserialize)]
struct SignInResponse {
    url: Option<String>,
}

#[derive(Deserialize)]
struct SessionResponse {
    user: Option<ApiUser>,
}

#[derive(Deserialize)]
struct ApiUser {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    image: Option<String>,
}

/// The decoded value of the OAuth-state cookie in the sign-in response, if the server sent one.
fn oauth_state_cookie(headers: &header::HeaderMap) -> Option<String> {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .find_map(|value| {
            let pair = value.to_str().ok()?.split(';').next()?;
            let (name, value) = pair.split_once('=')?;
            let name = name.trim();
            (name.strip_prefix(SECURE_PREFIX).unwrap_or(name) == OAUTH_STATE_COOKIE)
                .then(|| {
                    percent_decode_str(value.trim())
                        .decode_utf8()
                        .ok()
                        .map(|v| v.into_owned())
                })
                .flatten()
                .filter(|v| !v.is_empty())
        })
}

#[async_trait]
impl AuthApi for GameNodeClient {
    async fn begin_sign_in(
        &self,
        provider: Provider,
        success_url: &str,
        failure_url: &str,
    ) -> Result<Url, SignInFailure> {
        let response = self
            .http
            .post(self.endpoint("/sign-in/social"))
            .header(NATIVE_ORIGIN_HEADER, NATIVE_ORIGIN)
            .json(&serde_json::json!({
                "provider": provider.id(),
                "callbackURL": success_url,
                "errorCallbackURL": failure_url,
            }))
            .send()
            .await
            .map_err(|_| SignInFailure::ServerUnavailable)?;
        if response.status() != StatusCode::OK {
            tracing::warn!(
                status = response.status().as_u16(),
                "sign-in start was refused"
            );
            return Err(SignInFailure::ServerUnavailable);
        }
        let oauth_state = oauth_state_cookie(response.headers());
        let body: SignInResponse = response
            .json()
            .await
            .map_err(|_| SignInFailure::ServerUnavailable)?;
        let authorization = body
            .url
            .and_then(|raw| Url::parse(&raw).ok())
            .filter(|url| matches!(url.scheme(), "http" | "https") && url.host().is_some())
            .ok_or(SignInFailure::ServerUnavailable)?;
        Ok(self.proxy_url(&authorization, oauth_state.as_deref()))
    }

    async fn get_session(&self, credential: &SessionCredential) -> Result<User, SessionError> {
        let response = self
            .http
            .get(self.endpoint("/get-session"))
            .header(NATIVE_ORIGIN_HEADER, NATIVE_ORIGIN)
            .bearer_auth(credential.expose())
            .send()
            .await
            .map_err(|_| SessionError::Unreachable)?;
        match response.status() {
            StatusCode::OK => {}
            // Only an explicit refusal means the credential is bad. Anything else (outage,
            // misconfiguration, throttling) must not destroy a possibly valid session.
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => return Err(SessionError::Rejected),
            _ => return Err(SessionError::Unreachable),
        }
        // Better Auth answers 200 with a JSON `null` body when there is no session.
        let body: Option<SessionResponse> = response
            .json()
            .await
            .map_err(|_| SessionError::Unreachable)?;
        let user = body.and_then(|b| b.user).ok_or(SessionError::Rejected)?;
        Ok(User {
            name: user.name.unwrap_or_default(),
            id: user.id,
            email: user.email,
            image: user.image,
        })
    }

    async fn sign_out(&self, credential: &SessionCredential) -> Result<(), ()> {
        let response = self
            .http
            .post(self.endpoint("/sign-out"))
            .header(NATIVE_ORIGIN_HEADER, NATIVE_ORIGIN)
            .bearer_auth(credential.expose())
            .json(&serde_json::json!({}))
            .send()
            .await
            .map_err(|_| ())?;
        response.status().is_success().then_some(()).ok_or(())
    }
}

#[cfg(test)]
mod tests;
