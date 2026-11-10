//! Steam store session and catalog client.
//!
//! Steam has no public "sign in with a cookie" endpoint: the app holds the store session cookie
//! and this module validates it against the authenticated store page, reads the short-lived web API
//! token embedded there, and uses that token for the owned and Steam Family catalogs.
//!
//! Both the store page layout and the family endpoints are unofficial and can change. Every
//! response is parsed narrowly, every failure becomes a [`SteamError`] category, and no request
//! URL (it carries the token in its query), cookie, header or body is ever logged.

use std::time::Duration;

use crate::steam::{
    SteamAchievementDefinition, SteamAchievementState, SteamError, SteamFamily, SteamOwnedGame,
    SteamSession, SteamWebApi,
};
use async_trait::async_trait;
use gamenode_plugin_protocol::models::SessionCredential;
use percent_encoding::percent_decode_str;
use reqwest::header::{
    ACCEPT, ACCEPT_LANGUAGE, CONTENT_ENCODING, CONTENT_TYPE, COOKIE, HeaderMap, HeaderValue,
    LOCATION, REFERER, RETRY_AFTER, SET_COOKIE,
};
use reqwest::redirect::Policy;
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use serde::Deserialize;
use url::Url;

#[derive(Debug, thiserror::Error)]
#[error("Steam HTTP client initialization failed")]
pub struct ClientError;

/// The store front.
const STORE_BASE: &str = "https://store.steampowered.com/";
/// The login host that renews the short-lived store access cookie from the long-lived
/// `steamRefresh_steam` cookie.
const LOGIN_BASE: &str = "https://login.steampowered.com/";
/// The Web API host. It is never logged with its query string.
const API_BASE: &str = "https://api.steampowered.com/";

const STORE_CONFIG_PATH: &str = "/pointssummary/ajaxgetasyncconfig";
const STORE_EXPLORE_PATH: &str = "/explore/";
/// The store home page. Tried when the recommendation queue no longer carries the config.
const STORE_HOME_PATH: &str = "/";
const FINALIZE_LOGIN_PATH: &str = "/jwt/finalizelogin";
/// The redirect a browser lands on after login. The store token transfer follows it.
const FINALIZE_LOGIN_REDIRECT: &str = "https://store.steampowered.com/login/?redir=&redir_ssl=1";
const OWNED_GAMES_PATH: &str = "/IPlayerService/GetOwnedGames/v1/";
const FAMILY_GROUP_PATH: &str = "/IFamilyGroupsService/GetFamilyGroupForUser/v1/";
const SHARED_LIBRARY_PATH: &str = "/IFamilyGroupsService/GetSharedLibraryApps/v1/";
const ACHIEVEMENT_SCHEMA_PATH: &str = "/ISteamUserStats/GetSchemaForGame/v2/";
const PLAYER_ACHIEVEMENTS_PATH: &str = "/ISteamUserStats/GetPlayerAchievements/v1/";

/// How many times one store request may follow Steam's cookie-setting redirect back to itself.
const MAX_COOKIE_BOUNCES: u32 = 3;
const MAX_REDIRECTS: usize = 10;
const MAX_RETRIES: u32 = 3;
const RETRY_BASE: Duration = Duration::from_millis(100);
const MAX_RETRY_AFTER: Duration = Duration::from_secs(5);

/// The store page is small; 4 MiB is several times the largest page Steam serves.
const STORE_BODY_LIMIT: usize = 4 * 1024 * 1024;
/// A 10,000-game family answer still fits comfortably below this.
const API_BODY_LIMIT: usize = 16 * 1024 * 1024;

/// A realistic desktop Chrome identity; Steam serves a degraded page to unknown clients.
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// The headers a Chrome navigation to the store sends. Steam's frontend buckets some responses by
/// request shape, so the store page requests mirror Playnite's WebView instead of a bare HTTP GET.
const STORE_ACCEPT: &str =
    "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8";
const STORE_ACCEPT_JSON: &str = "application/json, text/plain, */*";
const STORE_ACCEPT_LANGUAGE: &str = "en-US,en;q=0.9";
const STORE_REFERER: &str = "https://store.steampowered.com/";

/// The Steam store and Web API client.
#[derive(Clone)]
pub struct SteamClient {
    http: Client,
    store: Url,
    api: Url,
    login: Url,
}

impl SteamClient {
    /// The production client: Steam's hosts, 20-second requests, system proxy and no cookie store.
    pub fn new() -> Result<Self, ClientError> {
        Self::with_timeout(Duration::from_secs(20))
    }

    /// The production hosts with a custom request timeout.
    pub fn with_timeout(timeout: Duration) -> Result<Self, ClientError> {
        let store = Url::parse(STORE_BASE).map_err(|_| ClientError)?;
        let api = Url::parse(API_BASE).map_err(|_| ClientError)?;
        let login = Url::parse(LOGIN_BASE).map_err(|_| ClientError)?;
        Self::build(store, api, login, timeout)
    }

    fn build(store: Url, api: Url, login: Url, timeout: Duration) -> Result<Self, ClientError> {
        let http = Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(10))
            .timeout(timeout)
            .redirect(redirect_policy())
            .build()
            .map_err(|_| ClientError)?;
        Ok(Self {
            http,
            store,
            api,
            login,
        })
    }

    #[cfg(test)]
    fn with_bases(store: Url, api: Url, timeout: Duration) -> Result<Self, ClientError> {
        Self::build(store.clone(), api, store, timeout)
    }

    async fn get_with_retry<F>(
        &self,
        endpoint: &'static str,
        build: F,
    ) -> Result<Response, SteamError>
    where
        F: Fn() -> RequestBuilder,
    {
        let mut attempt = 0;
        loop {
            let response = match build().send().await {
                Ok(response) => response,
                Err(error) => {
                    // Only the failure kind: reqwest's own text can carry the request URL.
                    tracing::warn!(
                        endpoint,
                        timeout = error.is_timeout(),
                        connect = error.is_connect(),
                        "Steam request could not be sent"
                    );
                    return Err(SteamError::Unreachable);
                }
            };
            let status = response.status();
            if is_transient(status) && attempt < MAX_RETRIES {
                let delay = retry_delay(response.headers(), attempt);
                tracing::debug!(
                    endpoint,
                    status = status.as_u16(),
                    retry = attempt + 1,
                    "retrying a transient Steam response"
                );
                tokio::time::sleep(delay).await;
                attempt += 1;
                continue;
            }
            return Ok(response);
        }
    }

    async fn get_ok<F>(&self, endpoint: &'static str, build: F) -> Result<Response, SteamError>
    where
        F: Fn() -> RequestBuilder,
    {
        let response = match self.get_with_retry(endpoint, build).await {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(endpoint, error = %error, "Steam request failed");
                return Err(error);
            }
        };
        let status = response.status();
        // A redirect to the sign-in page is expired whether the policy followed it or not: the
        // follow can land on a sign-in page whose own status says nothing about the session.
        if is_login_url(response.url()) {
            return Err(SteamError::Expired);
        }
        if status.as_u16() < 400 {
            if status.is_redirection()
                && redirect_location(response.url(), response.headers())
                    .as_ref()
                    .is_some_and(is_login_url)
            {
                return Err(SteamError::Expired);
            }
            if status.is_redirection() {
                // A same-URL redirect is Steam's cookie bootstrap and is resolved by `get_store`.
                let target = redirect_location(response.url(), response.headers());
                let self_redirect = target
                    .as_ref()
                    .is_some_and(|target| *target == *response.url());
                if self_redirect {
                    tracing::debug!(endpoint, "Steam cookie bootstrap redirect");
                } else {
                    // Never the query: it can carry a token. A redirect that was not followed is
                    // a host outside Steam or a loop, and the target tells which.
                    tracing::warn!(
                        endpoint,
                        status = status.as_u16(),
                        from = %format!("{}{}", response.url().host_str().unwrap_or("no host"), response.url().path()),
                        target = %redirect_target(response.url(), response.headers()),
                        "Steam answered with a redirect that was not followed"
                    );
                }
            }
            tracing::debug!(endpoint, status = status.as_u16(), "Steam response");
            return Ok(response);
        }
        let error = match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => SteamError::Expired,
            StatusCode::TOO_MANY_REQUESTS => SteamError::RateLimited,
            StatusCode::NOT_FOUND => SteamError::Changed,
            status if status.is_server_error() => SteamError::Server,
            _ => SteamError::Server,
        };
        tracing::warn!(
            endpoint,
            status = status.as_u16(),
            error = %error,
            "Steam request failed"
        );
        Err(error)
    }

    /// One authenticated store request. Steam answers a request that lacks its frontend cookies
    /// (`steamCountry`, `browserid`, `Steam_Language`, ...) with a 302 to the very same URL plus
    /// `Set-Cookie`; resending with those cookies returns the real page. The redirect policy stops
    /// same-URL redirects as loops, so the bounce is resolved here: merge the cookies Steam set and
    /// resend the same URL, up to [`MAX_COOKIE_BOUNCES`] times.
    ///
    /// `stored_cookie` is the minimal persistable header (`steamLoginSecure` and `sessionid` only);
    /// `request_cookie` is the header actually sent and may carry the transient frontend cookies.
    async fn get_store(
        &self,
        endpoint: &'static str,
        accept: &'static str,
        referer: Option<&'static str>,
        stored_cookie: &str,
        request_cookie: &str,
    ) -> Result<StoreResponse, SteamError> {
        let mut stored_cookie = stored_cookie.to_owned();
        let mut request_cookie = request_cookie.to_owned();
        let mut bounces = 0;
        loop {
            let header =
                HeaderValue::from_str(&request_cookie).map_err(|_| SteamError::InvalidCookies)?;
            let url = endpoint_url(&self.store, endpoint);
            let response = self
                .get_ok(endpoint, || {
                    let mut request = self
                        .http
                        .get(url.clone())
                        .header(COOKIE, header.clone())
                        .header(ACCEPT, accept)
                        .header(ACCEPT_LANGUAGE, STORE_ACCEPT_LANGUAGE);
                    if let Some(referer) = referer {
                        request = request.header(REFERER, referer);
                    }
                    request
                })
                .await?;
            stored_cookie = rotated_cookie_header(&stored_cookie, response.headers());
            let status = response.status();
            if status.is_redirection() {
                let target = redirect_location(response.url(), response.headers());
                if target.as_ref().is_some_and(is_login_url) {
                    return Err(SteamError::Expired);
                }
                let self_redirect = target
                    .as_ref()
                    .is_some_and(|target| *target == *response.url());
                if self_redirect
                    && bounces < MAX_COOKIE_BOUNCES
                    && let Some(merged) = merge_set_cookies(&request_cookie, response.headers())
                {
                    tracing::debug!(
                        endpoint,
                        bounce = bounces + 1,
                        "Steam set cookies and redirected to itself"
                    );
                    request_cookie = merged;
                    bounces += 1;
                    continue;
                }
            }
            let content_type = header_text(response.headers(), CONTENT_TYPE);
            let content_encoding = header_text(response.headers(), CONTENT_ENCODING);
            let body = match read_capped(response, STORE_BODY_LIMIT).await {
                Ok(body) => body,
                Err(SteamError::Changed) => {
                    log_unparsed_store_page(
                        endpoint,
                        status,
                        &content_type,
                        &content_encoding,
                        None,
                    );
                    return Err(SteamError::Changed);
                }
                Err(error) => return Err(error),
            };
            return Ok(StoreResponse {
                status,
                content_type,
                content_encoding,
                body,
                stored_cookie,
                request_cookie,
            });
        }
    }

    async fn store_session_fallback(
        &self,
        stored_cookie: &str,
        request_cookie: &str,
    ) -> Result<SteamSession, SteamError> {
        let response = self
            .get_store(
                STORE_CONFIG_PATH,
                STORE_ACCEPT_JSON,
                Some(STORE_REFERER),
                stored_cookie,
                request_cookie,
            )
            .await?;
        let parsed: FallbackResponse = match serde_json::from_slice(&response.body) {
            Ok(parsed) => parsed,
            Err(_) => {
                tracing::warn!(
                    endpoint = STORE_CONFIG_PATH,
                    status = response.status.as_u16(),
                    content_encoding = response.content_encoding,
                    bytes = response.body.len(),
                    "Steam token fallback was not JSON"
                );
                return Err(SteamError::Changed);
            }
        };
        if parsed.success != 1 {
            return Err(SteamError::Expired);
        }
        let Some(data) = parsed.data else {
            return Err(SteamError::Expired);
        };
        let token = data
            .webapi_token
            .filter(|token| !token.is_empty())
            .ok_or(SteamError::Expired)?;
        // The fallback has no identity fields: the SteamID is encoded in the session cookie.
        let steam_id = steam_id_from_cookie(&response.stored_cookie).ok_or(SteamError::Changed)?;
        make_session(
            PageSession {
                steam_id,
                account_name: None,
                token,
            },
            response.stored_cookie,
        )
    }

    /// Exchanges the long-lived `steamRefresh_steam` cookie for a fresh store access cookie.
    ///
    /// A store login keeps the browser signed in by renewing the roughly one-day
    /// `steamLoginSecure` access cookie from the roughly two-hundred-day refresh cookie.
    /// GameNode does not have a browser cookie jar, so it performs the same transfer
    /// (`/jwt/finalizelogin` followed by the store's `/login/settoken`) itself. `Ok(None)`
    /// means the credential predates this flow or the user refused "remember me".
    async fn refresh_cookie_header(
        &self,
        cookie: &SessionCredential,
    ) -> Result<Option<SessionCredential>, SteamError> {
        let pairs = cookie_pairs(cookie.expose());
        let Some((_, refresh_value)) = pairs
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("steamRefresh_steam"))
        else {
            return Ok(None);
        };
        if refresh_value.is_empty() {
            return Ok(None);
        }
        // The cookie is `<steamid>%7C%7C<refresh token>`; the login host expects only the
        // token as its nonce.
        let refresh_token = refresh_token_from_cookie(refresh_value);
        let session_id = pairs
            .iter()
            .find(|(name, value)| name.eq_ignore_ascii_case("sessionid") && !value.is_empty())
            .map(|(_, value)| value.clone())
            .unwrap_or_else(random_session_id);
        let url = endpoint_url(&self.login, FINALIZE_LOGIN_PATH);
        let form = [
            ("nonce", refresh_token),
            ("sessionid", session_id.clone()),
            ("redir", FINALIZE_LOGIN_REDIRECT.to_owned()),
        ];
        let refresh_cookie = format!("steamRefresh_steam={refresh_value}; sessionid={session_id}");
        let response = self
            .get_with_retry("steam.login.finalize", || {
                self.http
                    .post(url.clone())
                    .header(ACCEPT, STORE_ACCEPT_JSON)
                    .header(COOKIE, refresh_cookie.clone())
                    .form(&form)
            })
            .await?;
        let status = response.status();
        if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
            tracing::warn!("Steam rejected the saved login renewal token");
            return Err(SteamError::Expired);
        }
        if status != StatusCode::OK {
            tracing::warn!(status = status.as_u16(), "Steam login renewal failed");
            return Err(request_error(status));
        }
        // The renewal may rotate the refresh cookie too; keep every pair Steam updated.
        let renewed = rotated_cookie_header(cookie.expose(), response.headers());
        let body = read_capped(response, API_BODY_LIMIT).await?;
        let finalize: FinalizeLogin =
            serde_json::from_slice(&body).map_err(|_| SteamError::Changed)?;
        if finalize.error.is_some_and(|error| !error.is_null()) {
            tracing::warn!("Steam rejected the saved login renewal token");
            return Err(SteamError::Expired);
        }
        let steam_id = finalize
            .steam_id
            .as_ref()
            .and_then(numeric_id)
            .or_else(|| steam_id_from_cookie(&renewed))
            .or_else(|| steam_id_from_named_cookie(&renewed, "steamRefresh_steam"))
            .ok_or(SteamError::Changed)?;
        // The store has its own session token now, so use the store transfer rather than
        // reusing a community token that the store would reject.
        let transfer = finalize
            .transfer_info
            .iter()
            .find(|transfer| {
                transfer
                    .url
                    .as_deref()
                    .is_some_and(|url| self.is_store_transfer(url))
            })
            .ok_or(SteamError::Changed)?;
        let transfer_url = Url::parse(transfer.url.as_deref().ok_or(SteamError::Changed)?)
            .map_err(|_| SteamError::Changed)?;
        let host = transfer_url.host_str().unwrap_or_default();
        if transfer_url.scheme() != "https" && !is_loopback_host(host) {
            return Err(SteamError::Changed);
        }
        if !is_steam_host(host) && !is_loopback_host(host) {
            return Err(SteamError::Changed);
        }
        let mut fields: Vec<(&str, String)> = vec![("steamID", steam_id)];
        let params = transfer.params.as_ref();
        for (name, value) in [
            ("nonce", params.and_then(|params| params.nonce.as_ref())),
            ("auth", params.and_then(|params| params.auth.as_ref())),
        ] {
            if let Some(value) = value.filter(|value| !value.is_empty()) {
                fields.push((name, value.clone()));
            }
        }
        let session_cookie = format!("sessionid={session_id}");
        let response = self
            .get_with_retry("steam.login.settoken", || {
                self.http
                    .post(transfer_url.clone())
                    .header(ACCEPT, STORE_ACCEPT_JSON)
                    .header(COOKIE, session_cookie.clone())
                    .form(&fields)
            })
            .await?;
        let status = response.status();
        if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
            return Err(SteamError::Expired);
        }
        if !status.is_success() {
            tracing::warn!(
                status = status.as_u16(),
                "Steam store token transfer failed"
            );
            return Err(request_error(status));
        }
        let renewed = rotated_cookie_header(&renewed, response.headers());
        if cookie_pairs(&renewed)
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("steamLoginSecure"))
            .is_none_or(|(_, value)| value.is_empty())
        {
            return Err(SteamError::Expired);
        }
        SessionCredential::new(renewed)
            .map(Some)
            .ok_or(SteamError::InvalidCookies)
    }

    fn is_store_transfer(&self, value: &str) -> bool {
        let Ok(url) = Url::parse(value) else {
            return false;
        };
        let host = url.host_str().unwrap_or_default();
        let scheme_ok = url.scheme() == "https" || is_loopback_host(host);
        scheme_ok
            && (is_loopback_host(host)
                || host.eq_ignore_ascii_case(self.store.host_str().unwrap_or_default()))
            && url.path() == "/login/settoken"
    }

    async fn read_store_session(
        &self,
        cookie: &SessionCredential,
    ) -> Result<SteamSession, SteamError> {
        let mut stored_cookie = cookie.expose().to_owned();
        let mut request_cookie = stored_cookie.clone();
        // Playnite reads the config from the store page Playnite navigates to. The recommendation
        // queue is that page; the home page carries the same contract if the queue changes shape.
        for path in [STORE_EXPLORE_PATH, STORE_HOME_PATH] {
            let page = match self
                .get_store(path, STORE_ACCEPT, None, &stored_cookie, &request_cookie)
                .await
            {
                Ok(page) => page,
                // A page shape we do not know: the other store page may still serve the config.
                Err(SteamError::Changed) => continue,
                Err(error) => return Err(error),
            };
            stored_cookie = page.stored_cookie;
            request_cookie = page.request_cookie;
            let text = match String::from_utf8(page.body) {
                Ok(text) => text,
                Err(error) => {
                    let body = error.as_bytes();
                    log_unparsed_store_page(
                        path,
                        page.status,
                        &page.content_type,
                        &page.content_encoding,
                        Some(body),
                    );
                    continue;
                }
            };
            match parse_store_page(&text) {
                Ok(page) => return make_session(page, stored_cookie),
                Err(SteamError::Expired) => return Err(SteamError::Expired),
                Err(_) => {
                    log_unparsed_store_page(
                        path,
                        page.status,
                        &page.content_type,
                        &page.content_encoding,
                        Some(text.as_bytes()),
                    );
                }
            }
        }
        self.store_session_fallback(&stored_cookie, &request_cookie)
            .await
    }
}

/// A store response before parsing, with the rotated persistable cookie and the full request-local
/// cookie header, plus just enough response metadata for a secret-free diagnostic when the page does
/// not match the parsed contract.
struct StoreResponse {
    status: StatusCode,
    content_type: String,
    content_encoding: String,
    body: Vec<u8>,
    stored_cookie: String,
    request_cookie: String,
}

impl SteamClient {
    async fn achievement_body(
        &self,
        endpoint: &'static str,
        url: Url,
    ) -> Result<Option<Vec<u8>>, SteamError> {
        let response = self
            .get_with_retry(endpoint, || self.http.get(url.clone()))
            .await?;
        let status = response.status();
        let content_type = header_text(response.headers(), CONTENT_TYPE);
        let body = read_capped(response, API_BODY_LIMIT).await?;
        match status {
            StatusCode::OK => Ok(Some(body)),
            StatusCode::BAD_REQUEST => {
                // A game without stats (and an unknown app id) answers 400 JSON. Anything else
                // means the endpoint changed shape.
                let value: serde_json::Value =
                    serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
                let no_stats = value
                    .pointer("/playerstats/error")
                    .and_then(|error| error.as_str())
                    .is_some_and(|error| error.to_ascii_lowercase().contains("no stats"))
                    || value.as_object().is_some_and(|object| object.is_empty());
                if no_stats {
                    tracing::debug!(endpoint, "Steam reports no stats for this app");
                    Ok(None)
                } else {
                    tracing::warn!(
                        endpoint,
                        status = status.as_u16(),
                        "unexpected achievement response"
                    );
                    Err(SteamError::Changed)
                }
            }
            StatusCode::UNAUTHORIZED => Err(SteamError::Expired),
            StatusCode::FORBIDDEN => {
                // A JSON 403 is a privacy refusal; the invalid-token answer is HTML.
                let json = content_type.to_ascii_lowercase().contains("json")
                    || body
                        .iter()
                        .copied()
                        .find(|byte| !byte.is_ascii_whitespace())
                        == Some(b'{');
                if json {
                    tracing::warn!(endpoint, "Steam does not disclose these achievements");
                    Err(SteamError::Privacy)
                } else {
                    Err(SteamError::Expired)
                }
            }
            StatusCode::TOO_MANY_REQUESTS => Err(SteamError::RateLimited),
            status if status.is_server_error() => Err(SteamError::Server),
            _ => Err(SteamError::Server),
        }
    }
}

#[async_trait]
impl SteamWebApi for SteamClient {
    async fn session(&self, cookie: &SessionCredential) -> Result<SteamSession, SteamError> {
        self.read_store_session(cookie).await
    }

    async fn renew_session(
        &self,
        cookie: &SessionCredential,
    ) -> Result<Option<SessionCredential>, SteamError> {
        self.refresh_cookie_header(cookie).await
    }

    async fn owned_games(&self, session: &SteamSession) -> Result<Vec<SteamOwnedGame>, SteamError> {
        let url = api_url(
            &self.api,
            OWNED_GAMES_PATH,
            &[
                ("access_token", session.token.expose()),
                ("steamid", &session.steam_id),
                ("format", "json"),
                ("language", "english"),
                ("include_appinfo", "true"),
                ("include_played_free_games", "true"),
                ("include_free_sub", "false"),
                ("skip_unvetted_apps", "false"),
            ],
        );
        let response = self
            .get_ok(OWNED_GAMES_PATH, || self.http.get(url.clone()))
            .await?;
        let body = read_capped(response, API_BODY_LIMIT).await?;
        let parsed: OwnedResponse =
            serde_json::from_slice(&body).map_err(|_| SteamError::Changed)?;
        let Some(response) = parsed.response else {
            return Err(SteamError::Privacy);
        };
        // A missing `games` array means Steam did not disclose the library. An empty array with
        // `game_count: 0` is a successful empty catalog.
        let Some(games) = response.games else {
            return Err(SteamError::Privacy);
        };
        let games = collect_games(games, false);
        tracing::info!(games = games.len(), "Steam owned catalog fetched");
        Ok(games)
    }

    async fn family_games(&self, session: &SteamSession) -> Result<SteamFamily, SteamError> {
        let group_url = api_url(
            &self.api,
            FAMILY_GROUP_PATH,
            &[
                ("access_token", session.token.expose()),
                ("steamid", &session.steam_id),
            ],
        );
        let response = self
            .get_ok(FAMILY_GROUP_PATH, || self.http.get(group_url.clone()))
            .await?;
        let body = read_capped(response, API_BODY_LIMIT).await?;
        let parsed: FamilyGroupResponse =
            serde_json::from_slice(&body).map_err(|_| SteamError::Changed)?;
        let Some(group) = parsed.response else {
            return Err(SteamError::Changed);
        };
        if group.is_not_member_of_any_group {
            return Ok(SteamFamily::default());
        }
        let Some(group_id) = group.family_groupid.and_then(IdValue::into_numeric_string) else {
            // No group id is indistinguishable from no family for this catalog.
            return Ok(SteamFamily::default());
        };

        let shared_url = api_url(
            &self.api,
            SHARED_LIBRARY_PATH,
            &[
                ("access_token", session.token.expose()),
                ("steamid", &session.steam_id),
                ("family_groupid", &group_id),
                ("language", "english"),
                ("include_own", "false"),
                ("include_excluded", "false"),
                ("include_free", "false"),
                ("include_non_games", "false"),
                ("max_apps", "10000"),
            ],
        );
        let response = self
            .get_ok(SHARED_LIBRARY_PATH, || self.http.get(shared_url.clone()))
            .await?;
        let body = read_capped(response, API_BODY_LIMIT).await?;
        let parsed: SharedLibraryResponse =
            serde_json::from_slice(&body).map_err(|_| SteamError::Changed)?;
        let Some(shared) = parsed.response else {
            return Err(SteamError::Changed);
        };
        let games = collect_games(shared.apps.unwrap_or_default(), true);
        tracing::info!(games = games.len(), "Steam family catalog fetched");
        Ok(SteamFamily {
            group_id: Some(group_id),
            games,
        })
    }

    async fn achievement_definitions(
        &self,
        session: &SteamSession,
        app_id: &str,
        language: &str,
    ) -> Result<Option<Vec<SteamAchievementDefinition>>, SteamError> {
        let url = api_url(
            &self.api,
            ACHIEVEMENT_SCHEMA_PATH,
            &[
                ("access_token", session.token.expose()),
                ("key", ""),
                ("appid", app_id),
                ("l", language),
                ("format", "json"),
            ],
        );
        let Some(body) = self.achievement_body(ACHIEVEMENT_SCHEMA_PATH, url).await? else {
            return Ok(None);
        };
        let parsed: SchemaResponse =
            serde_json::from_slice(&body).map_err(|_| SteamError::Changed)?;
        let Some(game) = parsed.game else {
            return Err(SteamError::Changed);
        };
        let Some(stats) = game.available_game_stats else {
            return Ok(None);
        };
        let Some(achievements) = stats.achievements else {
            return Ok(None);
        };
        let definitions = achievements
            .into_iter()
            .filter_map(|achievement| {
                let api_name = achievement.name?.trim().to_owned();
                if api_name.is_empty() {
                    return None;
                }
                Some(SteamAchievementDefinition {
                    api_name,
                    display_name: crate::steam::clean_achievement_text(
                        &achievement.display_name.unwrap_or_default(),
                    ),
                    description: crate::steam::clean_achievement_text(
                        &achievement.description.unwrap_or_default(),
                    ),
                    hidden: achievement.hidden.unwrap_or(0) != 0,
                    icon: achievement.icon.unwrap_or_default(),
                    icon_gray: achievement.icongray.unwrap_or_default(),
                })
            })
            .collect::<Vec<_>>();
        Ok(Some(definitions))
    }

    async fn achievement_states(
        &self,
        session: &SteamSession,
        app_id: &str,
        language: &str,
    ) -> Result<Option<Vec<SteamAchievementState>>, SteamError> {
        let url = api_url(
            &self.api,
            PLAYER_ACHIEVEMENTS_PATH,
            &[
                ("access_token", session.token.expose()),
                ("key", ""),
                ("steamid", &session.steam_id),
                ("appid", app_id),
                ("l", language),
                ("format", "json"),
            ],
        );
        let Some(body) = self.achievement_body(PLAYER_ACHIEVEMENTS_PATH, url).await? else {
            return Ok(None);
        };
        let parsed: PlayerAchievementsResponse =
            serde_json::from_slice(&body).map_err(|_| SteamError::Changed)?;
        let Some(stats) = parsed.playerstats else {
            return Err(SteamError::Changed);
        };
        if !stats.success {
            return Err(SteamError::Privacy);
        }
        let Some(achievements) = stats.achievements else {
            return Ok(None);
        };
        let states = achievements
            .into_iter()
            .filter_map(|achievement| {
                let api_name = achievement.apiname?.trim().to_owned();
                if api_name.is_empty() {
                    return None;
                }
                let achieved = achievement.achieved.unwrap_or(0) != 0;
                let unlock_time = achievement.unlocktime.filter(|time| *time > 0 && achieved);
                Some(SteamAchievementState {
                    api_name,
                    achieved,
                    unlock_time,
                    display_name: crate::steam::clean_achievement_text(
                        &achievement.name.unwrap_or_default(),
                    ),
                    description: crate::steam::clean_achievement_text(
                        &achievement.description.unwrap_or_default(),
                    ),
                })
            })
            .collect::<Vec<_>>();
        Ok(Some(states))
    }
}

fn redirect_policy() -> Policy {
    Policy::custom(|attempt| {
        let url = attempt.url();
        let host = url.host_str().unwrap_or_default();
        let loopback = is_loopback_host(host);
        let scheme_ok = url.scheme() == "https" || (url.scheme() == "http" && loopback);
        if attempt.previous().len() >= MAX_REDIRECTS
            || attempt.previous().iter().any(|previous| previous == url)
            || !scheme_ok
            || !(is_steam_host(host) || loopback)
        {
            // Not following leaves the redirect response to the caller instead of sending the
            // session cookie to an unrelated host.
            attempt.stop()
        } else {
            attempt.follow()
        }
    })
}

fn is_steam_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("steampowered.com")
        || host.ends_with(".steampowered.com")
        || host.eq_ignore_ascii_case("steamcommunity.com")
        || host.ends_with(".steamcommunity.com")
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]")
}

fn is_transient(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

/// The absolute URL a redirect response points to, if it has a usable Location.
fn redirect_location(base: &Url, headers: &HeaderMap) -> Option<Url> {
    headers
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| base.join(value).ok())
}

/// The host and path a redirect points to, without its query. Diagnostics only.
fn redirect_target(base: &Url, headers: &HeaderMap) -> String {
    match redirect_location(base, headers) {
        Some(url) => format!("{}{}", url.host_str().unwrap_or("no host"), url.path()),
        None => "no usable location".to_owned(),
    }
}

fn retry_delay(headers: &HeaderMap, attempt: u32) -> Duration {
    let retry_after = headers
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|seconds| Duration::from_secs(seconds).min(MAX_RETRY_AFTER));
    retry_after.unwrap_or_else(|| {
        let factor = 1u32 << attempt.min(4);
        (RETRY_BASE * factor).min(MAX_RETRY_AFTER)
    })
}

fn endpoint_url(base: &Url, path: &str) -> Url {
    let mut url = base.clone();
    url.set_path(path);
    url.set_query(None);
    url
}

fn api_url(base: &Url, path: &str, params: &[(&str, &str)]) -> Url {
    let mut url = endpoint_url(base, path);
    {
        let mut query = url.query_pairs_mut();
        for (name, value) in params {
            query.append_pair(name, value);
        }
    }
    url
}

async fn read_capped(mut response: Response, limit: usize) -> Result<Vec<u8>, SteamError> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(SteamError::Changed);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        tracing::warn!(
            timeout = error.is_timeout(),
            "Steam response body was interrupted"
        );
        SteamError::Unreachable
    })? {
        if body.len() + chunk.len() > limit {
            return Err(SteamError::Changed);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// One response header as text, or `none` when absent. Used only for secret-free diagnostics.
fn header_text(headers: &HeaderMap, name: reqwest::header::HeaderName) -> String {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("none")
        .to_owned()
}

/// Describes a store page that did not match the parsed contract without ever logging the body,
/// the cookie or the token: only the response shape and which markers the page carried.
fn log_unparsed_store_page(
    endpoint: &'static str,
    status: StatusCode,
    content_type: &str,
    content_encoding: &str,
    body: Option<&[u8]>,
) {
    let text = body.map(String::from_utf8_lossy);
    let has = |needle: &str| text.as_deref().is_some_and(|text| text.contains(needle));
    tracing::warn!(
        endpoint,
        status = status.as_u16(),
        content_type,
        content_encoding,
        bytes = body.map_or(0, <[u8]>::len),
        has_application_config = has("application_config"),
        has_userinfo = has("data-userinfo"),
        has_store_config = has("data-store_user_config"),
        "Steam store page did not match the parsed contract"
    );
}

fn is_login_url(url: &Url) -> bool {
    let path = url.path();
    path == "/login" || path.starts_with("/login/")
}

#[derive(Debug)]
struct PageSession {
    steam_id: String,
    account_name: Option<String>,
    token: String,
}

fn make_session(page: PageSession, cookie: String) -> Result<SteamSession, SteamError> {
    let token = SessionCredential::new(page.token).ok_or(SteamError::Changed)?;
    let cookie = SessionCredential::new(cookie).ok_or(SteamError::InvalidCookies)?;
    tracing::debug!(
        has_account_name = page.account_name.is_some(),
        "Steam store session validated"
    );
    Ok(SteamSession {
        steam_id: page.steam_id,
        account_name: page.account_name,
        token,
        cookie,
    })
}

fn parse_store_page(body: &str) -> Result<PageSession, SteamError> {
    let tag = application_config(body).ok_or(SteamError::Changed)?;
    let userinfo = attribute(tag, "data-userinfo").ok_or(SteamError::Changed)?;
    let userinfo: UserInfo =
        serde_json::from_str(&html_entity_decode(userinfo)).map_err(|_| SteamError::Changed)?;
    match userinfo.logged_in {
        Some(true) => {}
        Some(false) => return Err(SteamError::Expired),
        None => return Err(SteamError::Changed),
    }
    let steam_id = userinfo
        .steamid
        .and_then(IdValue::into_numeric_string)
        .ok_or(SteamError::Changed)?;
    let account_name = userinfo
        .account_name
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty());

    let config = attribute(tag, "data-store_user_config").ok_or(SteamError::Changed)?;
    let config: StoreConfig =
        serde_json::from_str(&html_entity_decode(config)).map_err(|_| SteamError::Changed)?;
    let token = config
        .webapi_token
        .filter(|token| !token.is_empty())
        .ok_or(SteamError::Changed)?;
    Ok(PageSession {
        steam_id,
        account_name,
        token,
    })
}

/// The `<div id="application_config" ...>` start tag.
fn application_config(body: &str) -> Option<&str> {
    for needle in ["id=\"application_config\"", "id='application_config'"] {
        if let Some(index) = body.find(needle) {
            let tag_start = body[..index].rfind('<')?;
            let tag_end = body[index..].find('>').map(|end| index + end)?;
            return Some(&body[tag_start..=tag_end]);
        }
    }
    None
}

/// One quoted attribute of a start tag, without HTML-entity decoding.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut search_from = 0;
    while let Some(relative) = tag[search_from..].find(name) {
        let index = search_from + relative;
        let starts_the_name = index == 0
            || tag[..index]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        if starts_the_name {
            let after = tag[index + name.len()..].trim_start();
            if let Some(after_eq) = after.strip_prefix('=') {
                let after_eq = after_eq.trim_start();
                if let Some(quote) = after_eq
                    .chars()
                    .next()
                    .filter(|quote| *quote == '"' || *quote == '\'')
                {
                    let value = &after_eq[quote.len_utf8()..];
                    if let Some(end) = value.find(quote) {
                        return Some(&value[..end]);
                    }
                }
            }
        }
        search_from = index + name.len();
    }
    None
}

fn html_entity_decode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut index = 0;
    while let Some(relative) = input[index..].find('&') {
        let start = index + relative;
        out.push_str(&input[index..start]);
        if let Some(end) = input[start..]
            .find(';')
            .map(|end| start + end)
            .filter(|end| *end > start + 1)
            && let Some(character) = decode_entity(&input[start + 1..end])
        {
            out.push(character);
            index = end + 1;
            continue;
        }
        out.push('&');
        index = start + 1;
    }
    out.push_str(&input[index..]);
    out
}

fn decode_entity(entity: &str) -> Option<char> {
    match entity {
        "quot" => Some('"'),
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{a0}'),
        _ => {
            if let Some(hex) = entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
            {
                u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
            } else {
                entity
                    .strip_prefix('#')
                    .and_then(|decimal| decimal.parse::<u32>().ok())
                    .and_then(char::from_u32)
            }
        }
    }
}

#[derive(Deserialize)]
struct UserInfo {
    #[serde(default)]
    logged_in: Option<bool>,
    #[serde(default)]
    steamid: Option<IdValue>,
    #[serde(default)]
    account_name: Option<String>,
}

#[derive(Deserialize)]
struct StoreConfig {
    #[serde(default)]
    webapi_token: Option<String>,
}

#[derive(Deserialize)]
struct FallbackResponse {
    #[serde(default)]
    success: i32,
    #[serde(default)]
    data: Option<FallbackData>,
}

#[derive(Deserialize)]
struct FallbackData {
    #[serde(default)]
    webapi_token: Option<String>,
}

/// The login host's answer to a saved refresh cookie.
///
/// `steamID` is a JSON string in Steam's answer, but older captures used a number; both are
/// accepted. `transfer_info` names the per-domain endpoints that mint the new session cookies.
#[derive(Deserialize)]
struct FinalizeLogin {
    #[serde(default, rename = "steamID")]
    steam_id: Option<serde_json::Value>,
    #[serde(default)]
    transfer_info: Vec<LoginTransfer>,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct LoginTransfer {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    params: Option<LoginTransferParams>,
}

#[derive(Default, Deserialize)]
struct LoginTransferParams {
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    auth: Option<String>,
}

#[derive(Deserialize)]
struct OwnedResponse {
    #[serde(default)]
    response: Option<OwnedBody>,
}

#[derive(Deserialize)]
struct OwnedBody {
    #[serde(default)]
    games: Option<Vec<ApiGame>>,
}

#[derive(Deserialize)]
struct FamilyGroupResponse {
    #[serde(default)]
    response: Option<FamilyGroupBody>,
}

#[derive(Deserialize)]
struct FamilyGroupBody {
    #[serde(default)]
    is_not_member_of_any_group: bool,
    #[serde(default)]
    family_groupid: Option<IdValue>,
}

#[derive(Deserialize)]
struct SharedLibraryResponse {
    #[serde(default)]
    response: Option<SharedLibraryBody>,
}

#[derive(Deserialize)]
struct SharedLibraryBody {
    #[serde(default)]
    apps: Option<Vec<ApiGame>>,
}

#[derive(Deserialize)]
struct SchemaResponse {
    #[serde(default)]
    game: Option<SchemaGame>,
}

#[derive(Deserialize)]
struct SchemaGame {
    #[serde(default, rename = "availableGameStats")]
    available_game_stats: Option<SchemaStats>,
}

#[derive(Deserialize)]
struct SchemaStats {
    #[serde(default)]
    achievements: Option<Vec<SchemaAchievement>>,
}

#[derive(Deserialize)]
struct SchemaAchievement {
    #[serde(default)]
    name: Option<String>,
    #[serde(default, rename = "displayName")]
    display_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    hidden: Option<i64>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    icongray: Option<String>,
}

#[derive(Deserialize)]
struct PlayerAchievementsResponse {
    #[serde(default)]
    playerstats: Option<PlayerAchievements>,
}

#[derive(Deserialize)]
struct PlayerAchievements {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    achievements: Option<Vec<PlayerAchievement>>,
}

#[derive(Deserialize)]
struct PlayerAchievement {
    #[serde(default)]
    apiname: Option<String>,
    #[serde(default)]
    achieved: Option<i64>,
    #[serde(default)]
    unlocktime: Option<i64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Deserialize)]
struct ApiGame {
    #[serde(default)]
    appid: Option<IdValue>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    exclude_reason: Option<i64>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum IdValue {
    Number(u64),
    Text(String),
}

impl IdValue {
    fn into_numeric_string(self) -> Option<String> {
        match self {
            Self::Number(id) => Some(id.to_string()),
            Self::Text(text) => {
                let text = text.trim();
                if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
                    return None;
                }
                text.parse::<u64>().ok().map(|id| id.to_string())
            }
        }
    }
}

fn collect_games(rows: Vec<ApiGame>, skip_excluded: bool) -> Vec<SteamOwnedGame> {
    let mut seen = std::collections::BTreeSet::new();
    let mut dropped = 0usize;
    let mut excluded = 0usize;
    let mut games = Vec::new();
    for row in rows {
        if skip_excluded && row.exclude_reason.unwrap_or(0) != 0 {
            excluded += 1;
            continue;
        }
        let Some(app_id) = row.appid.and_then(IdValue::into_numeric_string) else {
            dropped += 1;
            continue;
        };
        let Some(name) = row
            .name
            .as_deref()
            .map(clean_name)
            .filter(|name| !name.is_empty())
        else {
            dropped += 1;
            continue;
        };
        if !seen.insert(app_id.clone()) {
            continue;
        }
        games.push(SteamOwnedGame { app_id, name });
    }
    if dropped > 0 {
        tracing::warn!(
            dropped,
            "dropped Steam catalog entries without a usable app id or name"
        );
    }
    if excluded > 0 {
        tracing::warn!(excluded, "skipped excluded Steam Family apps");
    }
    games
}

/// Steam names often end in trademark signs; the library shows the plain name.
fn clean_name(name: &str) -> String {
    name.chars()
        .filter(|character| !matches!(character, '™' | '®' | '©'))
        .collect::<String>()
        .trim()
        .to_owned()
}

fn cookie_pairs(header: &str) -> Vec<(String, String)> {
    header
        .split(';')
        .filter_map(|pair| {
            let pair = pair.trim();
            let (name, value) = pair.split_once('=')?;
            let name = name.trim();
            (!name.is_empty()).then(|| (name.to_owned(), value.trim().to_owned()))
        })
        .collect()
}

/// Merges every `Set-Cookie` pair of a response into the request-local cookie header, including
/// the transient frontend cookies (`steamCountry`, `browserid`, `Steam_Language`) that Steam needs
/// before it serves a store page. Returns `None` when the header did not change. Emptied cookies are
/// removed.
fn merge_set_cookies(current: &str, headers: &HeaderMap) -> Option<String> {
    let mut pairs = cookie_pairs(current);
    let mut changed = false;
    for value in headers.get_all(SET_COOKIE).iter() {
        let Ok(value) = value.to_str() else {
            continue;
        };
        let Some((name, value)) = value
            .split(';')
            .next()
            .and_then(|pair| pair.split_once('='))
        else {
            continue;
        };
        let (name, value) = (name.trim(), value.trim());
        if name.is_empty() {
            continue;
        }
        let position = pairs
            .iter()
            .position(|(existing, _)| existing.eq_ignore_ascii_case(name));
        if value.is_empty() {
            if let Some(position) = position {
                pairs.remove(position);
                changed = true;
            }
            continue;
        }
        match position {
            Some(position) if pairs[position].1 == value => {}
            Some(position) => {
                pairs[position].1 = value.to_owned();
                changed = true;
            }
            None => {
                pairs.push((name.to_owned(), value.to_owned()));
                changed = true;
            }
        }
    }
    changed.then(|| {
        pairs
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ")
    })
}

/// Rebuilds the minimal cookie header with the rotated `steamLoginSecure`, `sessionid` and
/// `steamRefresh_steam` values Steam sent, preserving every other stored pair.
fn rotated_cookie_header(current: &str, headers: &HeaderMap) -> String {
    let mut updates = Vec::new();
    for value in headers.get_all(SET_COOKIE).iter() {
        let Ok(value) = value.to_str() else {
            continue;
        };
        let Some(pair) = value.split(';').next() else {
            continue;
        };
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        let name = name.trim();
        let value = value.trim();
        if value.is_empty()
            || !(name.eq_ignore_ascii_case("steamLoginSecure")
                || name.eq_ignore_ascii_case("sessionid")
                || name.eq_ignore_ascii_case("steamRefresh_steam"))
        {
            continue;
        }
        updates.push((name.to_owned(), value.to_owned()));
    }
    if updates.is_empty() {
        return current.trim().to_owned();
    }
    let mut pairs = cookie_pairs(current);
    for (name, value) in updates {
        if let Some(pair) = pairs
            .iter_mut()
            .find(|(existing, _)| existing.eq_ignore_ascii_case(&name))
        {
            pair.1 = value;
        } else {
            pairs.push((name, value));
        }
    }
    pairs
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// A numeric SteamID from the login answer's `steamID`, whether it was typed as a string or a
/// number.
fn numeric_id(value: &serde_json::Value) -> Option<String> {
    let id = match value {
        serde_json::Value::String(id) => id.trim().to_owned(),
        serde_json::Value::Number(number) => number.to_string(),
        _ => return None,
    };
    (!id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit())).then_some(id)
}

/// The refresh token from a `steamRefresh_steam` cookie value (`<steamid>%7C%7C<token>`). A
/// value without the separator is assumed to already be the token.
fn refresh_token_from_cookie(value: &str) -> String {
    let decoded = percent_decode_str(value)
        .decode_utf8()
        .map_or_else(|_| value.to_owned(), |decoded| decoded.into_owned());
    match decoded.split_once("||") {
        Some((_, token)) => token.to_owned(),
        None => decoded,
    }
}

/// An ID-shaped value from `login.steampowered.com`, which sets the refresh cookie as
/// `<steamid>%7C%7C<token>`.
fn steam_id_from_named_cookie(header: &str, name: &str) -> Option<String> {
    let (_, value) = cookie_pairs(header)
        .into_iter()
        .find(|(existing, _)| existing.eq_ignore_ascii_case(name))?;
    let decoded = percent_decode_str(&value).decode_utf8().ok()?;
    let id = decoded.split("||").next()?.trim();
    (!id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit())).then(|| id.to_owned())
}

fn steam_id_from_cookie(header: &str) -> Option<String> {
    steam_id_from_named_cookie(header, "steamLoginSecure")
}

/// The CSRF value for the transfer request. Steam treats it as opaque; only its presence matters.
fn random_session_id() -> String {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).expect("operating system randomness is unavailable");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn request_error(status: StatusCode) -> SteamError {
    match status {
        StatusCode::TOO_MANY_REQUESTS => SteamError::RateLimited,
        status if status.is_server_error() => SteamError::Server,
        _ => SteamError::Changed,
    }
}

#[cfg(test)]
mod tests;
