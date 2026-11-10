//! Epic Games Store HTTP client.
//!
//! Epic publishes no API for launchers. The OAuth, library, catalog and playtime endpoints here
//! mirror what the official launcher and open-source launchers use, so they are unofficial and can
//! change. Every endpoint stays isolated in this module, every response is parsed narrowly, every
//! failure becomes an [`EpicError`] category, and no token, authorization code, account id, query
//! string or body is ever logged. Logs name only a static endpoint label, a status code and a body
//! size.
//!
//! The launcher client id and secret below are the widely published constants used by open-source
//! Epic launchers. They identify the public launcher client, are not user secrets, and are shared
//! by every install; user credentials are never derived from them.

use std::{collections::BTreeMap, fmt, time::Duration};

use gamenode_plugin_protocol::models::SessionCredential;
use reqwest::{
    Client, RequestBuilder, Response, StatusCode, header::CONTENT_TYPE, redirect::Policy,
};
use serde::Deserialize;
use url::{Url, form_urlencoded};

/// The public Epic Games Launcher OAuth client. Not a user secret.
pub const LAUNCHER_CLIENT_ID: &str = "34a02cf8f4414e29b15921876da36f9a";
/// The public Epic Games Launcher OAuth client secret. Not a user secret.
pub const LAUNCHER_CLIENT_SECRET: &str = "daafbccc737745039dffe53d94fc76cf";

const OAUTH_BASE: &str = "https://account-public-service-prod03.ol.epicgames.com";
const LIBRARY_BASE: &str = "https://library-service.live.use1a.on.epicgames.com";
const CATALOG_BASE: &str = "https://catalog-public-service-prod06.ol.epicgames.com";

const TOKEN_PATH: &str = "/account/api/oauth/token";
const SESSION_REVOKE_PATH: &str = "/account/api/oauth/sessions/kill/";
const LIBRARY_ITEMS_PATH: &str = "/library/api/public/items";
const CATALOG_NAMESPACE_PATH: &str = "/catalog/api/shared/namespace/";
const PLAYTIME_PATH: &str = "/library/api/public/playtime/account/";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Revoking a session is best effort; a short timeout keeps sign-out responsive.
const REVOKE_TIMEOUT: Duration = Duration::from_secs(3);

const SMALL_BODY_LIMIT: usize = 64 * 1024;
const LIBRARY_BODY_LIMIT: usize = 16 * 1024 * 1024;
const CATALOG_BODY_LIMIT: usize = 16 * 1024 * 1024;
const PLAYTIME_BODY_LIMIT: usize = 16 * 1024 * 1024;
/// A generous upper bound that still stops a cursor loop from running forever.
const MAX_LIBRARY_PAGES: usize = 1000;

/// Why an Epic request failed. Every variant is a category: no token, authorization code, account
/// id, URL or response body is carried in the value, its `Debug`, or its `Display`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EpicError {
    Configuration,
    Unreachable,
    RateLimited,
    Server,
    Unauthorized,
    Rejected,
    InvalidGrant,
    Changed,
}

impl fmt::Display for EpicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Configuration => "the Epic client configuration is invalid",
            Self::Unreachable => "Epic could not be reached",
            Self::RateLimited => "Epic is rate limiting requests",
            Self::Server => "Epic returned a server error",
            Self::Unauthorized => "the Epic credential is no longer valid",
            Self::Rejected => "Epic rejected the request",
            Self::InvalidGrant => "Epic rejected the authorization grant",
            Self::Changed => "Epic changed the response this client expects",
        })
    }
}

impl std::error::Error for EpicError {}

/// A token response. The tokens stay in [`SessionCredential`] so they cannot be logged by accident.
pub struct EpicToken {
    pub access_token: SessionCredential,
    pub refresh_token: SessionCredential,
    /// The account id used by the playtime endpoint. It is an identifier and is never logged.
    pub account_id: String,
    pub display_name: String,
}

impl fmt::Debug for EpicToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EpicToken(<redacted>)")
    }
}

/// One record from the owned library. Fields can be absent; the catalog sync filters records that
/// are not application-addressable.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EpicLibraryItem {
    pub app_name: Option<String>,
    pub namespace: Option<String>,
    pub catalog_item_id: Option<String>,
    pub product_id: Option<String>,
    /// Epic's playtime ledger is keyed by this artifact id, not by the application name.
    pub artifact_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EpicLibraryPage {
    pub records: Vec<EpicLibraryItem>,
    pub next_cursor: Option<String>,
}

/// One catalog title resolved for a library record.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EpicCatalogItem {
    pub id: String,
    pub title: String,
    pub is_game: bool,
    pub has_main_game: bool,
}

/// One total from Epic's own playtime ledger.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpicPlaytime {
    pub artifact_id: String,
    pub seconds: i64,
}

/// The Epic Games Store HTTP client.
#[derive(Clone)]
pub struct EpicClient {
    http: Client,
    oauth: Url,
    library: Url,
    catalog: Url,
}

impl EpicClient {
    /// The production client: Epic's hosts, 20-second requests, rustls and no redirect following
    /// (a redirect could carry an authorization header to another host).
    pub fn new() -> Result<Self, EpicError> {
        Self::build(
            parse_base(OAUTH_BASE)?,
            parse_base(LIBRARY_BASE)?,
            parse_base(CATALOG_BASE)?,
            REQUEST_TIMEOUT,
        )
    }

    #[cfg(test)]
    fn with_bases(
        oauth: Url,
        library: Url,
        catalog: Url,
        timeout: Duration,
    ) -> Result<Self, EpicError> {
        Self::build(oauth, library, catalog, timeout)
    }

    fn build(oauth: Url, library: Url, catalog: Url, timeout: Duration) -> Result<Self, EpicError> {
        let http = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(timeout)
            .redirect(Policy::none())
            .build()
            .map_err(|_| EpicError::Configuration)?;
        Ok(Self {
            http,
            oauth,
            library,
            catalog,
        })
    }

    /// Exchanges the one-time authorization code from the approved page probe.
    pub async fn exchange_code(&self, code: &SessionCredential) -> Result<EpicToken, EpicError> {
        self.token_request(&[
            ("grant_type", "authorization_code"),
            ("code", code.expose()),
            ("token_type", "eg1"),
        ])
        .await
    }

    /// Exchanges the stored refresh token. Epic rotates it on every successful exchange.
    pub async fn refresh(&self, refresh_token: &SessionCredential) -> Result<EpicToken, EpicError> {
        self.token_request(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.expose()),
            ("token_type", "eg1"),
        ])
        .await
    }

    async fn token_request(&self, fields: &[(&str, &str)]) -> Result<EpicToken, EpicError> {
        let url = self
            .oauth
            .join(TOKEN_PATH)
            .map_err(|_| EpicError::Configuration)?;
        let request = self
            .http
            .post(url)
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .basic_auth(LAUNCHER_CLIENT_ID, Some(LAUNCHER_CLIENT_SECRET))
            .body(form_body(fields));
        let (status, body) = self
            .fetch("oauth_token", request, SMALL_BODY_LIMIT, true)
            .await?;
        parse_token("oauth_token", status, &body)
    }

    /// Best-effort session revoke. The caller ignores failures; the local secret is already gone.
    pub async fn revoke(&self, access_token: &SessionCredential) -> Result<(), EpicError> {
        let mut url = self
            .oauth
            .join(SESSION_REVOKE_PATH)
            .map_err(|_| EpicError::Configuration)?;
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| EpicError::Configuration)?;
            segments.pop_if_empty().push(access_token.expose());
        }
        let request = self
            .http
            .delete(url)
            .bearer_auth(access_token.expose())
            .timeout(REVOKE_TIMEOUT);
        self.fetch("oauth_revoke", request, SMALL_BODY_LIMIT, false)
            .await?;
        Ok(())
    }

    /// One page of the owned library. `includeMetadata=true` is required for the records to carry
    /// their namespace and catalog item id.
    pub async fn library_page(
        &self,
        access_token: &SessionCredential,
        cursor: Option<&str>,
    ) -> Result<EpicLibraryPage, EpicError> {
        let mut url = self
            .library
            .join(LIBRARY_ITEMS_PATH)
            .map_err(|_| EpicError::Configuration)?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("includeMetadata", "true");
            if let Some(cursor) = cursor.filter(|cursor| !cursor.is_empty()) {
                query.append_pair("cursor", cursor);
            }
        }
        let request = self.http.get(url).bearer_auth(access_token.expose());
        let (status, body) = self
            .fetch("library_items", request, LIBRARY_BODY_LIMIT, false)
            .await?;
        parse_library_page(status, &body)
    }

    /// Follows the library cursor until Epic reports no next page.
    pub async fn library_items(
        &self,
        access_token: &SessionCredential,
    ) -> Result<Vec<EpicLibraryItem>, EpicError> {
        let mut cursor = None;
        let mut seen = std::collections::BTreeSet::new();
        let mut items = Vec::new();
        for _ in 0..MAX_LIBRARY_PAGES {
            let page = self.library_page(access_token, cursor.as_deref()).await?;
            items.extend(page.records);
            match page.next_cursor {
                Some(next) if seen.insert(next.clone()) => cursor = Some(next),
                Some(_) => {
                    tracing::warn!(endpoint = "library_items", "Epic repeated a library cursor");
                    return Err(EpicError::Changed);
                }
                None => return Ok(items),
            }
        }
        tracing::warn!(
            endpoint = "library_items",
            pages = MAX_LIBRARY_PAGES,
            "Epic library exceeded the page bound"
        );
        Err(EpicError::Changed)
    }

    /// Resolves catalog titles for one namespace. `ids` are sent comma-separated, matching the
    /// launcher's bulk-items shape.
    pub async fn catalog_items(
        &self,
        access_token: &SessionCredential,
        namespace: &str,
        ids: &[String],
    ) -> Result<BTreeMap<String, EpicCatalogItem>, EpicError> {
        if ids.is_empty() {
            return Ok(BTreeMap::new());
        }
        if namespace.trim().is_empty() {
            return Err(EpicError::Configuration);
        }
        let mut url = self
            .catalog
            .join(CATALOG_NAMESPACE_PATH)
            .map_err(|_| EpicError::Configuration)?;
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| EpicError::Configuration)?;
            segments
                .pop_if_empty()
                .push(namespace)
                .push("bulk")
                .push("items");
        }
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("id", &ids.join(","));
            query.append_pair("includeMainGameDetails", "true");
            query.append_pair("country", "US");
            query.append_pair("locale", "en-US");
        }
        let request = self.http.get(url).bearer_auth(access_token.expose());
        let (status, body) = self
            .fetch("catalog_bulk", request, CATALOG_BODY_LIMIT, false)
            .await?;
        parse_catalog_items(status, &body)
    }

    /// Epic's cumulative playtime ledger for the account, in seconds.
    pub async fn playtime(
        &self,
        access_token: &SessionCredential,
        account_id: &str,
    ) -> Result<Vec<EpicPlaytime>, EpicError> {
        if account_id.is_empty() {
            return Err(EpicError::Configuration);
        }
        let mut url = self
            .library
            .join(PLAYTIME_PATH)
            .map_err(|_| EpicError::Configuration)?;
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| EpicError::Configuration)?;
            segments.pop_if_empty().push(account_id).push("all");
        }
        let request = self.http.get(url).bearer_auth(access_token.expose());
        let (status, body) = self
            .fetch("playtime", request, PLAYTIME_BODY_LIMIT, false)
            .await?;
        parse_playtime(status, &body)
    }

    async fn fetch(
        &self,
        endpoint: &'static str,
        request: RequestBuilder,
        limit: usize,
        oauth: bool,
    ) -> Result<(StatusCode, Vec<u8>), EpicError> {
        let response = request.send().await.map_err(|error| {
            // Only the failure kind: reqwest's own text can carry the request URL.
            tracing::warn!(
                endpoint,
                timeout = error.is_timeout(),
                connect = error.is_connect(),
                "Epic request could not be sent"
            );
            EpicError::Unreachable
        })?;
        let status = response.status();
        if status.is_redirection() {
            tracing::warn!(
                endpoint,
                status = status.as_u16(),
                "Epic answered with a redirect that was not followed"
            );
            return Err(EpicError::Changed);
        }
        let body = read_capped(endpoint, response, limit).await?;
        if status.is_success() {
            tracing::debug!(
                endpoint,
                status = status.as_u16(),
                bytes = body.len(),
                "Epic response"
            );
            return Ok((status, body));
        }
        Err(classify(endpoint, status, &body, oauth))
    }
}

fn parse_base(base: &str) -> Result<Url, EpicError> {
    Url::parse(base).map_err(|_| EpicError::Configuration)
}

fn form_body(fields: &[(&str, &str)]) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (name, value) in fields {
        serializer.append_pair(name, value);
    }
    serializer.finish()
}

async fn read_capped(
    endpoint: &'static str,
    mut response: Response,
    limit: usize,
) -> Result<Vec<u8>, EpicError> {
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        tracing::warn!(
            endpoint,
            status = status.as_u16(),
            bytes = response.content_length().unwrap_or(0),
            "Epic response was larger than the accepted bound"
        );
        return Err(EpicError::Changed);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        tracing::warn!(
            endpoint,
            timeout = error.is_timeout(),
            "Epic response body was interrupted"
        );
        EpicError::Unreachable
    })? {
        if body.len() + chunk.len() > limit {
            tracing::warn!(
                endpoint,
                status = status.as_u16(),
                bytes = body.len() + chunk.len(),
                "Epic response was larger than the accepted bound"
            );
            return Err(EpicError::Changed);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn classify(endpoint: &'static str, status: StatusCode, body: &[u8], oauth: bool) -> EpicError {
    let error = if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        EpicError::Unauthorized
    } else if status == StatusCode::TOO_MANY_REQUESTS {
        EpicError::RateLimited
    } else if status.is_server_error() {
        EpicError::Server
    } else if oauth && status == StatusCode::BAD_REQUEST {
        if oauth_error_is_invalid_grant(body) {
            EpicError::InvalidGrant
        } else {
            EpicError::Rejected
        }
    } else {
        EpicError::Changed
    };
    tracing::warn!(
        endpoint,
        status = status.as_u16(),
        bytes = body.len(),
        error = %error,
        "Epic request failed"
    );
    error
}

#[derive(Deserialize)]
struct OAuthErrorBody {
    #[serde(default, rename = "errorCode")]
    error_code: Option<String>,
    #[serde(default, rename = "error")]
    error: Option<String>,
}

fn oauth_error_is_invalid_grant(body: &[u8]) -> bool {
    serde_json::from_slice::<OAuthErrorBody>(body).is_ok_and(|body| {
        [body.error_code, body.error]
            .into_iter()
            .flatten()
            .any(|code| code.eq_ignore_ascii_case("invalid_grant"))
    })
}

#[derive(Deserialize)]
struct TokenBody {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default, rename = "displayName")]
    display_name: Option<String>,
}

fn parse_token(
    endpoint: &'static str,
    status: StatusCode,
    body: &[u8],
) -> Result<EpicToken, EpicError> {
    let parsed: TokenBody = match serde_json::from_slice(body) {
        Ok(parsed) => parsed,
        Err(_) => {
            log_unparsed(endpoint, status, body.len());
            return Err(EpicError::Changed);
        }
    };
    let (Some(access_token), Some(refresh_token), Some(account_id)) = (
        required(parsed.access_token).and_then(SessionCredential::new),
        required(parsed.refresh_token).and_then(SessionCredential::new),
        required(parsed.account_id),
    ) else {
        log_unparsed(endpoint, status, body.len());
        return Err(EpicError::Changed);
    };
    Ok(EpicToken {
        access_token,
        refresh_token,
        account_id,
        display_name: text(parsed.display_name).unwrap_or_default(),
    })
}

#[derive(Deserialize)]
struct LibraryPageBody {
    #[serde(default, rename = "responseMetadata")]
    response_metadata: Option<LibraryMetadata>,
    #[serde(default)]
    records: Option<Vec<LibraryRecord>>,
}

#[derive(Deserialize)]
struct LibraryMetadata {
    #[serde(default, rename = "nextCursor")]
    next_cursor: Option<String>,
}

#[derive(Deserialize)]
struct LibraryRecord {
    #[serde(default, rename = "appName")]
    app_name: Option<String>,
    #[serde(default)]
    namespace: Option<String>,
    #[serde(default, rename = "catalogItemId")]
    catalog_item_id: Option<String>,
    #[serde(default, rename = "productId")]
    product_id: Option<String>,
    /// The ledger key for this item, present on real launcher records.
    #[serde(default, rename = "artifactId")]
    artifact_id: Option<String>,
}

fn parse_library_page(status: StatusCode, body: &[u8]) -> Result<EpicLibraryPage, EpicError> {
    let parsed: LibraryPageBody = match serde_json::from_slice(body) {
        Ok(parsed) => parsed,
        Err(_) => {
            log_unparsed("library_items", status, body.len());
            return Err(EpicError::Changed);
        }
    };
    let Some(records) = parsed.records else {
        log_unparsed("library_items", status, body.len());
        return Err(EpicError::Changed);
    };
    Ok(EpicLibraryPage {
        records: records
            .into_iter()
            .map(|record| EpicLibraryItem {
                app_name: text(record.app_name),
                namespace: text(record.namespace),
                catalog_item_id: text(record.catalog_item_id),
                product_id: text(record.product_id),
                artifact_id: text(record.artifact_id),
            })
            .collect(),
        next_cursor: parsed
            .response_metadata
            .and_then(|metadata| text(metadata.next_cursor)),
    })
}

#[derive(Deserialize, Default)]
struct CatalogCategory {
    #[serde(default)]
    path: Option<String>,
}

#[derive(Deserialize, Default)]
struct CatalogRecord {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    categories: Vec<CatalogCategory>,
    #[serde(default, rename = "mainGameItem")]
    main_game_item: Option<serde_json::Value>,
}

fn parse_catalog_items(
    status: StatusCode,
    body: &[u8],
) -> Result<BTreeMap<String, EpicCatalogItem>, EpicError> {
    let parsed: BTreeMap<String, CatalogRecord> = match serde_json::from_slice(body) {
        Ok(parsed) => parsed,
        Err(_) => {
            log_unparsed("catalog_bulk", status, body.len());
            return Err(EpicError::Changed);
        }
    };
    let mut items = BTreeMap::new();
    for (key, record) in parsed {
        let id = text(record.id).unwrap_or(key);
        if id.is_empty() {
            continue;
        }
        let is_game = record
            .categories
            .iter()
            .any(|category| category.path.as_deref().is_some_and(|path| path == "games"));
        items.insert(
            id.clone(),
            EpicCatalogItem {
                id,
                title: text(record.title).unwrap_or_default(),
                is_game,
                has_main_game: record.main_game_item.is_some(),
            },
        );
    }
    Ok(items)
}

#[derive(Deserialize)]
struct PlaytimeRecord {
    #[serde(default, rename = "artifactId")]
    artifact_id: Option<String>,
    #[serde(default, rename = "totalTime")]
    total_time: i64,
}

fn parse_playtime(status: StatusCode, body: &[u8]) -> Result<Vec<EpicPlaytime>, EpicError> {
    let parsed: Vec<PlaytimeRecord> = match serde_json::from_slice(body) {
        Ok(parsed) => parsed,
        Err(_) => {
            log_unparsed("playtime", status, body.len());
            return Err(EpicError::Changed);
        }
    };
    Ok(parsed
        .into_iter()
        .filter_map(|record| {
            Some(EpicPlaytime {
                artifact_id: text(record.artifact_id)?,
                seconds: record.total_time,
            })
        })
        .collect())
}

fn required(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

fn text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn log_unparsed(endpoint: &'static str, status: StatusCode, bytes: usize) {
    tracing::warn!(
        endpoint,
        status = status.as_u16(),
        bytes,
        "Epic response did not match the expected shape"
    );
}

#[cfg(test)]
mod tests;
