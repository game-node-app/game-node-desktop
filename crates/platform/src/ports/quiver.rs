//! The Quiver Catalog API client (design D2, D3).
//!
//! GameNode reads the public, unauthenticated Quiver catalog itself instead of using Quiver's
//! launcher library. This client performs **only** GET requests, pages the `/apps` listing at
//! `limit=100`, and deserializes responses into the GameNode catalog model. It runs on the blocking
//! `reqwest` stack, so every call must run on a worker thread, never the UI or main thread.
//!
//! Failure rules (design D2): an entry missing `catalogId`, `slug` or `name` is skipped and
//! counted; more than 20 % skipped, an empty listing, or a listing that ends before its last page
//! fails the fetch, so the caller keeps the previous snapshot. Unknown fields are ignored and
//! every field the client reads is optional.
//!
//! `Retry-After` on HTTP 429 is honoured; without it, requests back off exponentially. The sleeper
//! is injectable so tests do not wait.

use std::collections::BTreeSet;
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use url::Url;

use gamenode_core::ports::{
    CatalogArt, CatalogDeveloper, CatalogEntry, CatalogGame, CatalogProject, CheckingRelease,
    LauncherRecipe, PortRelease, VerifiedRelease, WithdrawnRelease,
};

use super::catalog_worker::CatalogSource;

/// The public read-only catalog API.
pub const QUIVER_API_BASE_URL: &str = "https://api.quiverlauncher.com/api/v1";

/// The upstream caps a listing page at 100 entries.
const PAGE_LIMIT: &str = "100";

/// Bounds a broken upstream cannot make the worker exceed.
const MAX_LISTING_PAGES: usize = 200;
const MAX_RELEASE_PAGES: usize = 50;
const MAX_BODY_BYTES: u64 = 8 * 1024 * 1024;

/// A rate-limited request is tried this many times in total.
const MAX_ATTEMPTS: u32 = 4;
/// `Retry-After` beyond this is treated as "come back later" instead of stalling a worker.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);
const BACKOFF_BASE: Duration = Duration::from_millis(500);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// The User-Agent every request carries: the application version and the project page.
pub fn user_agent() -> String {
    format!(
        "GameNode-Desktop/{} (+https://github.com/game-node-app/game-node-desktop)",
        env!("CARGO_PKG_VERSION")
    )
}

/// Waits between a rate-limited response and the retry.
///
/// Real clients sleep; tests inject a recorder so retry logic is exercised without waiting.
pub trait Sleeper: Send + Sync {
    fn sleep(&self, duration: Duration);
}

/// Sleeps the current thread.
#[derive(Clone, Copy, Debug, Default)]
pub struct ThreadSleeper;

impl Sleeper for ThreadSleeper {
    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// Why a request could not produce a response at all. The caller maps it to "unreachable".
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the catalog request could not be sent")]
pub struct QuiverTransportError;

/// One raw response, reduced to what the client needs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    /// The parsed `Retry-After` header, when the response carried one.
    pub retry_after: Option<Duration>,
    pub body: Vec<u8>,
}

/// The minimal HTTP surface the client uses.
///
/// The trait deliberately exposes only a GET: the host never writes to the upstream (design D2).
/// Implementations run on a blocking worker thread.
pub trait QuiverHttp: Send + Sync {
    fn get(&self, url: &str) -> Result<HttpResponse, QuiverTransportError>;
}

/// Blocking `reqwest` transport.
pub struct ReqwestQuiverHttp {
    client: reqwest::blocking::Client,
}

impl ReqwestQuiverHttp {
    pub fn new() -> Result<Self, QuiverError> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(user_agent())
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| QuiverError::Unreachable)?;
        Ok(Self { client })
    }
}

impl QuiverHttp for ReqwestQuiverHttp {
    fn get(&self, url: &str) -> Result<HttpResponse, QuiverTransportError> {
        let response = self
            .client
            .get(url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .map_err(|_| QuiverTransportError)?;
        let status = response.status().as_u16();
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_retry_after);
        if response
            .content_length()
            .is_some_and(|length| length > MAX_BODY_BYTES)
        {
            return Err(QuiverTransportError);
        }
        let mut body = Vec::new();
        response
            .take(MAX_BODY_BYTES + 1)
            .read_to_end(&mut body)
            .map_err(|_| QuiverTransportError)?;
        if body.len() as u64 > MAX_BODY_BYTES {
            return Err(QuiverTransportError);
        }
        Ok(HttpResponse {
            status,
            retry_after,
            body,
        })
    }
}

fn parse_retry_after(value: &str) -> Option<Duration> {
    value.trim().parse::<u64>().ok().map(Duration::from_secs)
}

/// Why the upstream could not produce a usable document.
///
/// Every variant fails the fetch, so the caller keeps the previous snapshot. Messages are safe to
/// show and name no URL or response content.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum QuiverError {
    #[error("the port catalog could not be reached")]
    Unreachable,
    #[error("the port catalog is rate limiting requests")]
    RateLimited,
    #[error("the port catalog does not list that app")]
    NotFound,
    #[error("the port catalog returned a response this version cannot read")]
    InvalidResponse,
    #[error("the port catalog listing ended before its last page")]
    Truncated,
    #[error("the port catalog listed no entries")]
    EmptyListing,
    #[error("the port catalog skipped {skipped} of {total} entries, more than the accepted 20%")]
    TooManySkipped { skipped: usize, total: usize },
}

/// The Quiver Catalog API client.
pub struct QuiverClient {
    base_url: String,
    http: Arc<dyn QuiverHttp>,
    sleeper: Arc<dyn Sleeper>,
}

impl QuiverClient {
    /// The production client against the public API.
    pub fn new() -> Result<Self, QuiverError> {
        Ok(Self::with_parts(
            QUIVER_API_BASE_URL,
            Arc::new(ReqwestQuiverHttp::new()?),
            Arc::new(ThreadSleeper),
        ))
    }

    /// A client with a custom transport and the production base URL.
    pub fn with_http(http: Arc<dyn QuiverHttp>) -> Self {
        Self::with_parts(QUIVER_API_BASE_URL, http, Arc::new(ThreadSleeper))
    }

    /// A client with every moving part injected; tests use this.
    pub fn with_parts(
        base_url: impl Into<String>,
        http: Arc<dyn QuiverHttp>,
        sleeper: Arc<dyn Sleeper>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            http,
            sleeper,
        }
    }

    /// The complete `/apps` listing, paged sequentially.
    ///
    /// Entries missing `catalogId`, `slug` or `name` are skipped. More than 20 % skipped, an empty
    /// listing or a page that promises a cursor but cannot deliver the next page fails the fetch.
    pub fn listing(&self) -> Result<Vec<CatalogEntry>, QuiverError> {
        let mut entries = Vec::new();
        let mut skipped = 0usize;
        let mut cursors = BTreeSet::new();
        let mut cursor: Option<String> = None;
        let mut pages = 0usize;
        loop {
            pages += 1;
            if pages > MAX_LISTING_PAGES {
                return Err(QuiverError::Truncated);
            }
            let url = self.apps_endpoint(&["apps"], cursor.as_deref())?;
            let page: AppPage = self.get_json(&url)?;
            for item in page.items {
                // Parse entries one by one so one malformed entry cannot fail the whole page.
                let Ok(app) = serde_json::from_value::<UpstreamApp>(item) else {
                    skipped += 1;
                    continue;
                };
                let entry = app.into_entry();
                if entry.is_complete() {
                    entries.push(entry);
                } else {
                    skipped += 1;
                }
            }
            match next_cursor(page.next_cursor, page.is_done)? {
                Some(next) => {
                    if !cursors.insert(next.clone()) {
                        return Err(QuiverError::Truncated);
                    }
                    cursor = Some(next);
                }
                None => break,
            }
        }

        let total = entries.len() + skipped;
        if total == 0 {
            return Err(QuiverError::EmptyListing);
        }
        // Exactly 20 % skipped is still accepted; more is a breaking upstream change.
        if skipped * 5 > total {
            return Err(QuiverError::TooManySkipped { skipped, total });
        }
        Ok(entries)
    }

    /// One app with its project, checking release and withdrawals filled in (design D3).
    pub fn detail(&self, slug: &str) -> Result<CatalogEntry, QuiverError> {
        let url = self.endpoint(&["apps", slug], &[])?;
        let page: AppDetailPage = self.get_json(&url)?;
        let mut entry = page.entry.into_entry();
        if !entry.is_complete() {
            return Err(QuiverError::InvalidResponse);
        }
        if let Some(project) = page
            .project
            .filter(|project| !project.name.trim().is_empty())
        {
            entry.project = Some(project);
        }
        if page.checking.is_some() {
            entry.checking = page.checking;
        }
        entry.withdrawn = page.withdrawn;
        Ok(entry)
    }

    /// Every published release, newest first and paged sequentially.
    pub fn releases(&self, slug: &str) -> Result<Vec<PortRelease>, QuiverError> {
        let mut releases = Vec::new();
        let mut cursors = BTreeSet::new();
        let mut cursor: Option<String> = None;
        let mut pages = 0usize;
        loop {
            pages += 1;
            if pages > MAX_RELEASE_PAGES {
                return Err(QuiverError::Truncated);
            }
            let url = self.apps_endpoint(&["apps", slug, "releases"], cursor.as_deref())?;
            let page: ReleasePage = self.get_json(&url)?;
            releases.extend(page.items);
            match next_cursor(page.next_cursor, page.is_done)? {
                Some(next) => {
                    if !cursors.insert(next.clone()) {
                        return Err(QuiverError::Truncated);
                    }
                    cursor = Some(next);
                }
                None => break,
            }
        }
        Ok(releases)
    }

    fn apps_endpoint(&self, segments: &[&str], cursor: Option<&str>) -> Result<Url, QuiverError> {
        let mut query: Vec<(&str, &str)> = vec![("limit", PAGE_LIMIT)];
        if let Some(cursor) = cursor {
            query.push(("cursor", cursor));
        }
        self.endpoint(segments, &query)
    }

    fn endpoint(&self, segments: &[&str], query: &[(&str, &str)]) -> Result<Url, QuiverError> {
        let mut url = Url::parse(self.base_url.trim_end_matches('/'))
            .map_err(|_| QuiverError::Unreachable)?;
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|_| QuiverError::Unreachable)?;
            path.pop_if_empty();
            for segment in segments {
                path.push(segment);
            }
        }
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query.iter().copied());
        }
        Ok(url)
    }

    fn get_json<T: DeserializeOwned>(&self, url: &Url) -> Result<T, QuiverError> {
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let response = self
                .http
                .get(url.as_str())
                .map_err(|_| QuiverError::Unreachable)?;
            if response.status == 429 {
                if attempt >= MAX_ATTEMPTS {
                    return Err(QuiverError::RateLimited);
                }
                let delay = match response.retry_after {
                    Some(delay) if delay <= MAX_RETRY_AFTER => delay,
                    // A long cool-down would stall the background worker; report it instead.
                    Some(_) => return Err(QuiverError::RateLimited),
                    None => backoff(attempt),
                };
                self.sleeper.sleep(delay);
                continue;
            }
            if !(200..300).contains(&response.status) {
                return Err(match response.status {
                    404 => QuiverError::NotFound,
                    500..=599 => QuiverError::Unreachable,
                    _ => QuiverError::InvalidResponse,
                });
            }
            return serde_json::from_slice(&response.body)
                .map_err(|_| QuiverError::InvalidResponse);
        }
    }
}

impl CatalogSource for QuiverClient {
    fn listing(&self) -> Result<Vec<CatalogEntry>, QuiverError> {
        QuiverClient::listing(self)
    }

    fn detail(&self, slug: &str) -> Result<CatalogEntry, QuiverError> {
        QuiverClient::detail(self, slug)
    }

    fn releases(&self, slug: &str) -> Result<Vec<PortRelease>, QuiverError> {
        QuiverClient::releases(self, slug)
    }
}

fn backoff(attempt: u32) -> Duration {
    let factor = 1u32 << (attempt.saturating_sub(1)).min(6);
    (BACKOFF_BASE * factor).min(MAX_BACKOFF)
}

/// The next page, from a page's cursor and done flag.
///
/// A page that says it is not done but carries no cursor is a truncated listing, never a silent
/// end: the caller has to fail the refresh instead of storing a partial catalog.
fn next_cursor(cursor: Option<String>, is_done: bool) -> Result<Option<String>, QuiverError> {
    if is_done {
        return Ok(None);
    }
    match cursor {
        Some(cursor) if !cursor.trim().is_empty() => Ok(Some(cursor)),
        _ => Err(QuiverError::Truncated),
    }
}

// ---------------------------------------------------------------------------------------------
// Upstream documents
//
// These mirror the public OpenAPI shapes closely enough to map into the catalog model. Every field
// is optional so an upstream addition or removal cannot make a whole page unreadable. Unknown
// fields are ignored by serde's default behavior.
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct AppPage {
    /// Raw items, parsed one by one so a malformed entry is skipped instead of failing the page.
    items: Vec<serde_json::Value>,
    next_cursor: Option<String>,
    is_done: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct AppDetailPage {
    entry: UpstreamApp,
    project: Option<CatalogProject>,
    checking: Option<CheckingRelease>,
    withdrawn: Vec<WithdrawnRelease>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ReleasePage {
    items: Vec<PortRelease>,
    next_cursor: Option<String>,
    is_done: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct UpstreamApp {
    catalog_id: String,
    slug: String,
    name: String,
    description: String,
    project_id: Option<String>,
    project_name: Option<String>,
    developer: Option<CatalogDeveloper>,
    games: Vec<CatalogGame>,
    consoles: Vec<String>,
    #[serde(rename = "supportedOS", alias = "supportedOs")]
    supported_os: Vec<String>,
    /// The app's own icon, which the model keeps beside the `libraryArt` sizes (design D9).
    artwork: Option<String>,
    library_art: CatalogArt,
    verified: Option<VerifiedRelease>,
    checking: Option<CheckingRelease>,
    withdrawn: Vec<WithdrawnRelease>,
    launcher: LauncherRecipe,
}

impl UpstreamApp {
    fn into_entry(self) -> CatalogEntry {
        let mut art = self.library_art;
        if let Some(artwork) = non_empty(self.artwork) {
            art.artwork = Some(artwork.to_owned());
        }
        let project = self
            .project_name
            .filter(|name| !name.trim().is_empty())
            .map(|name| CatalogProject {
                id: self.project_id.unwrap_or_default(),
                name,
                ..CatalogProject::default()
            });
        CatalogEntry {
            catalog_id: self.catalog_id,
            slug: self.slug,
            name: self.name,
            description: self.description,
            project,
            developer: self.developer,
            games: self.games,
            consoles: self.consoles,
            supported_os: self.supported_os,
            art,
            verified: self.verified,
            checking: self.checking,
            withdrawn: self.withdrawn,
            launcher: self.launcher,
            ..CatalogEntry::default()
        }
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Mutex;
    use std::time::Instant;

    use serde_json::json;

    use super::*;

    const LISTING_PAGE_1: &str = include_str!("../../tests/fixtures/quiver/listing-page-1.json");
    const LISTING_PAGE_2: &str = include_str!("../../tests/fixtures/quiver/listing-page-2.json");
    const APP_DETAIL: &str = include_str!("../../tests/fixtures/quiver/app-detail.json");
    const RELEASES: &str = include_str!("../../tests/fixtures/quiver/releases.json");

    #[derive(Default)]
    struct FakeHttp {
        responses: Mutex<VecDeque<Result<HttpResponse, QuiverTransportError>>>,
        requests: Mutex<Vec<String>>,
    }

    impl FakeHttp {
        fn new(responses: Vec<HttpResponse>) -> Arc<Self> {
            Arc::new(Self {
                responses: Mutex::new(responses.into_iter().map(Ok).collect()),
                requests: Mutex::new(Vec::new()),
            })
        }

        fn requests(&self) -> Vec<String> {
            lock(&self.requests).clone()
        }
    }

    impl QuiverHttp for FakeHttp {
        fn get(&self, url: &str) -> Result<HttpResponse, QuiverTransportError> {
            lock(&self.requests).push(url.to_owned());
            lock(&self.responses)
                .pop_front()
                .unwrap_or(Err(QuiverTransportError))
        }
    }

    #[derive(Default)]
    struct FakeSleeper {
        sleeps: Mutex<Vec<Duration>>,
    }

    impl FakeSleeper {
        fn sleeps(&self) -> Vec<Duration> {
            lock(&self.sleeps).clone()
        }
    }

    impl Sleeper for FakeSleeper {
        fn sleep(&self, duration: Duration) {
            lock(&self.sleeps).push(duration);
        }
    }

    fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        mutex
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn ok(body: &str) -> HttpResponse {
        HttpResponse {
            status: 200,
            retry_after: None,
            body: body.as_bytes().to_vec(),
        }
    }

    fn http_status(status: u16, retry_after: Option<Duration>) -> HttpResponse {
        HttpResponse {
            status,
            retry_after,
            body: Vec::new(),
        }
    }

    fn client(http: Arc<FakeHttp>, sleeper: Arc<FakeSleeper>) -> QuiverClient {
        QuiverClient::with_parts("https://api.test.invalid/api/v1", http, sleeper)
    }

    fn item(id: Option<&str>, slug: Option<&str>, name: Option<&str>) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        if let Some(id) = id {
            map.insert("catalogId".into(), id.into());
        }
        if let Some(slug) = slug {
            map.insert("slug".into(), slug.into());
        }
        if let Some(name) = name {
            map.insert("name".into(), name.into());
        }
        serde_json::Value::Object(map)
    }

    fn page(items: &[serde_json::Value], next_cursor: Option<&str>, is_done: bool) -> String {
        json!({
            "items": items,
            "nextCursor": next_cursor,
            "isDone": is_done,
        })
        .to_string()
    }

    #[test]
    fn listing_pages_sequentially_and_maps_the_catalog_model() {
        let http = FakeHttp::new(vec![ok(LISTING_PAGE_1), ok(LISTING_PAGE_2)]);
        let sleeper = Arc::new(FakeSleeper::default());
        let entries = client(Arc::clone(&http), Arc::clone(&sleeper))
            .listing()
            .expect("the recorded listing parses");

        assert_eq!(entries.len(), 5);
        let first = &entries[0];
        assert_eq!(first.catalog_id, "qcat_fictional-lantern");
        assert_eq!(first.slug, "fictional-game-pc-port");
        assert_eq!(first.name, "Fictional Game");
        assert!(first.supports_windows());
        assert!(first.has_verified_release());
        assert_eq!(
            first.project.as_ref().map(|project| project.name.as_str()),
            Some("Fictional Game PC Port")
        );
        assert_eq!(
            first.game_titles().collect::<Vec<_>>(),
            vec!["Fictional Game"]
        );
        // The icon and the library art are merged into one art record.
        assert_eq!(
            first.art.artwork.as_deref(),
            Some("https://example.com/art/fictional-icon.ico")
        );
        assert_eq!(
            first.art.capsule.as_deref(),
            Some("https://example.com/art/fictional-capsule.png")
        );
        assert_eq!(first.launcher.setup_files.len(), 1);
        assert_eq!(
            first.launcher.setup_files[0].contains,
            vec!["eboot.bin".to_string(), "sce_sys/param.sfo".to_string()]
        );
        assert_eq!(
            first.launcher.preferred_executables.windows,
            vec!["FictionalGame.exe".to_string()]
        );

        assert_eq!(entries[3].catalog_id, "qcat_fictional-prerelease");
        assert!(entries[3].is_prerelease_only());
        assert_eq!(entries[4].catalog_id, "qcat_fictional-tool");

        let requests = http.requests();
        assert_eq!(requests.len(), 2, "each page is fetched once, in order");
        assert!(requests[0].starts_with("https://api.test.invalid/api/v1/apps?"));
        assert!(requests[0].contains("limit=100"));
        assert!(!requests[0].contains("cursor="));
        assert!(requests[1].contains("cursor=fixture-cursor-page-2"));
        assert!(sleeper.sleeps().is_empty());
    }

    #[test]
    fn unknown_fields_are_ignored_everywhere() {
        let body = json!({
            "items": [{
                "catalogId": "qcat_x",
                "slug": "x",
                "name": "X",
                "future": {"a": [1, 2]},
                "libraryArt": {
                    "capsule": "https://example.com/c.png",
                    "futureArt": "ignored"
                },
                "launcher": {"futureRecipeField": true}
            }],
            "nextCursor": null,
            "isDone": true,
            "futurePageField": true
        })
        .to_string();
        let http = FakeHttp::new(vec![ok(&body)]);
        let entries = client(http, Arc::new(FakeSleeper::default()))
            .listing()
            .expect("unknown fields never fail a page");
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].art.capsule.as_deref(),
            Some("https://example.com/c.png")
        );
    }

    #[test]
    fn one_malformed_entry_is_skipped_without_failing_the_page() {
        let mut items: Vec<_> = (0..9)
            .map(|index| {
                let id = format!("qcat_{index}");
                item(Some(&id), Some("slug"), Some("Name"))
            })
            .collect();
        // A wrong-typed field makes this entry unreadable; it must not take the page with it.
        items.push(json!({
            "catalogId": "qcat_broken",
            "slug": "broken",
            "name": "Broken",
            "launcher": "not an object"
        }));
        let http = FakeHttp::new(vec![ok(&page(&items, None, true))]);
        let entries = client(http, Arc::new(FakeSleeper::default()))
            .listing()
            .expect("one bad entry in ten is tolerated");
        assert_eq!(entries.len(), 9);
    }

    #[test]
    fn entries_missing_required_fields_are_skipped_and_counted() {
        let mut items: Vec<_> = (0..8)
            .map(|index| {
                let id = format!("qcat_{index}");
                item(Some(&id), Some("slug"), Some("Name"))
            })
            .collect();
        // Two of ten incomplete entries is exactly 20 %, which is still accepted.
        items.push(item(None, Some("slug"), Some("Name")));
        items.push(item(Some("qcat_x"), None, Some("Name")));
        let http = FakeHttp::new(vec![ok(&page(&items, None, true))]);
        let entries = client(http, Arc::new(FakeSleeper::default()))
            .listing()
            .expect("20% skipped is tolerated");
        assert_eq!(entries.len(), 8);
    }

    #[test]
    fn more_than_twenty_percent_skipped_fails_the_fetch() {
        let items = [
            item(Some("qcat_1"), Some("slug"), Some("Name")),
            item(Some("qcat_2"), Some("slug"), Some("Name")),
            item(Some("qcat_3"), Some("slug"), Some("Name")),
            item(Some("qcat_4"), None, Some("Name")),
            item(Some("qcat_5"), Some("slug"), None),
        ];
        let http = FakeHttp::new(vec![ok(&page(&items, None, true))]);
        let error = client(http, Arc::new(FakeSleeper::default()))
            .listing()
            .expect_err("40% skipped is a breaking upstream change");
        assert_eq!(
            error,
            QuiverError::TooManySkipped {
                skipped: 2,
                total: 5
            }
        );
    }

    #[test]
    fn an_empty_listing_fails_the_fetch() {
        let http = FakeHttp::new(vec![ok(&page(&[], None, true))]);
        let error = client(http, Arc::new(FakeSleeper::default()))
            .listing()
            .expect_err("an empty listing is a failed refresh");
        assert_eq!(error, QuiverError::EmptyListing);
    }

    #[test]
    fn a_listing_that_loses_a_page_fails_the_fetch() {
        let items = [item(Some("qcat_1"), Some("slug"), Some("Name"))];
        let http = FakeHttp::new(vec![
            ok(&page(&items, Some("fixture-cursor"), false)),
            http_status(500, None),
        ]);
        let error = client(Arc::clone(&http), Arc::new(FakeSleeper::default()))
            .listing()
            .expect_err("a failed page is a truncated listing");
        assert_eq!(error, QuiverError::Unreachable);
        assert_eq!(http.requests().len(), 2);
    }

    #[test]
    fn a_page_that_promises_no_cursor_but_is_not_done_is_truncated() {
        let items = [item(Some("qcat_1"), Some("slug"), Some("Name"))];
        let http = FakeHttp::new(vec![ok(&page(&items, None, false))]);
        let error = client(Arc::clone(&http), Arc::new(FakeSleeper::default()))
            .listing()
            .expect_err("the listing cannot silently stop early");
        assert_eq!(error, QuiverError::Truncated);
        assert_eq!(http.requests().len(), 1);
    }

    #[test]
    fn a_repeated_cursor_is_truncated_instead_of_looping() {
        let items = [item(Some("qcat_1"), Some("slug"), Some("Name"))];
        let body = page(&items, Some("same-cursor"), false);
        let http = FakeHttp::new(vec![ok(&body), ok(&body)]);
        let error = client(http, Arc::new(FakeSleeper::default()))
            .listing()
            .expect_err("a repeated cursor is a broken page");
        assert_eq!(error, QuiverError::Truncated);
    }

    #[test]
    fn a_retry_after_header_is_honoured_before_the_retry() {
        let http = FakeHttp::new(vec![
            http_status(429, Some(Duration::from_secs(2))),
            ok(LISTING_PAGE_1),
            ok(LISTING_PAGE_2),
        ]);
        let sleeper = Arc::new(FakeSleeper::default());
        let entries = client(http, Arc::clone(&sleeper))
            .listing()
            .expect("the retry succeeds");
        assert_eq!(entries.len(), 5);
        assert_eq!(sleeper.sleeps(), vec![Duration::from_secs(2)]);
    }

    #[test]
    fn repeated_rate_limits_back_off_exponentially_and_then_fail() {
        let http = FakeHttp::new(vec![
            http_status(429, None),
            http_status(429, None),
            http_status(429, None),
            http_status(429, None),
        ]);
        let sleeper = Arc::new(FakeSleeper::default());
        let error = client(Arc::clone(&http), Arc::clone(&sleeper))
            .listing()
            .expect_err("the upstream keeps rate limiting");
        assert_eq!(error, QuiverError::RateLimited);
        assert_eq!(http.requests().len(), 4);
        assert_eq!(
            sleeper.sleeps(),
            vec![
                Duration::from_millis(500),
                Duration::from_secs(1),
                Duration::from_secs(2)
            ]
        );
    }

    #[test]
    fn a_long_rate_limit_cool_down_does_not_stall_the_worker() {
        let http = FakeHttp::new(vec![http_status(429, Some(Duration::from_secs(3_600)))]);
        let sleeper = Arc::new(FakeSleeper::default());
        let error = client(http, Arc::clone(&sleeper))
            .listing()
            .expect_err("an hour-long cool-down is reported instead of slept");
        assert_eq!(error, QuiverError::RateLimited);
        assert!(sleeper.sleeps().is_empty());
    }

    #[test]
    fn app_detail_fills_the_project_checking_and_withdrawn_data() {
        let http = FakeHttp::new(vec![ok(APP_DETAIL)]);
        let entry = client(Arc::clone(&http), Arc::new(FakeSleeper::default()))
            .detail("fictional-game-pc-port")
            .expect("the recorded detail parses");
        assert_eq!(entry.catalog_id, "qcat_fictional-lantern");
        assert!(entry.description.contains("detail description"));
        let project = entry.project.expect("the detail project wins");
        assert_eq!(
            project.repository.as_deref(),
            Some("example-org/fictional-game-pc-port")
        );
        assert_eq!(project.provider.as_deref(), Some("github"));
        assert_eq!(
            project.source_url().as_deref(),
            Some("https://example.com/fictional-game")
        );
        let checking = entry.checking.expect("checking release");
        assert!(checking.needs_review);
        assert_eq!(checking.reasons, vec!["A file changed upstream."]);
        assert_eq!(entry.withdrawn.len(), 1);
        assert_eq!(entry.withdrawn[0].version, "v0.8.0");
        assert_eq!(http.requests().len(), 1);
        assert!(http.requests()[0].ends_with("/apps/fictional-game-pc-port"));
    }

    #[test]
    fn releases_are_parsed_from_the_recorded_shape() {
        let http = FakeHttp::new(vec![ok(RELEASES)]);
        let releases = client(http, Arc::new(FakeSleeper::default()))
            .releases("fictional-game-pc-port")
            .expect("the recorded releases parse");
        assert_eq!(releases.len(), 2);
        let newest = &releases[0];
        assert_eq!(newest.release_id, "release_1");
        assert_eq!(newest.version, "v1.0.0");
        assert_eq!(newest.assets.len(), 2);
        assert_eq!(newest.assets[0].filename, "fictional-game-windows.zip");
        assert!(newest.assets[0].is_windows());
        assert_eq!(
            newest.assets[0].verified_sha256().as_deref(),
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
        );
        assert!(
            newest.assets[0]
                .scan
                .as_ref()
                .is_some_and(|scan| scan.verdict == gamenode_core::ports::ScanVerdict::Clean)
        );
        assert_eq!(
            newest.assets[1].unverified_sha256().as_deref(),
            Some("abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789")
        );
        assert!(releases[1].prerelease);
    }

    #[test]
    fn a_missing_app_is_reported_as_not_found() {
        let http = FakeHttp::new(vec![http_status(404, None)]);
        let error = client(http, Arc::new(FakeSleeper::default()))
            .detail("missing")
            .expect_err("404 is not a document");
        assert_eq!(error, QuiverError::NotFound);
    }

    fn accept_with_timeout(listener: &TcpListener) -> TcpStream {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match listener.accept() {
                Ok((stream, _)) => return stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "no request reached the fixture server"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("fixture server failed: {error}"),
            }
        }
    }

    #[test]
    fn only_get_requests_are_sent_with_the_documented_user_agent() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("fixture listener");
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for body in [LISTING_PAGE_1, LISTING_PAGE_2] {
                let mut stream = accept_with_timeout(&listener);
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut request = Vec::new();
                let mut chunk = [0u8; 1024];
                loop {
                    let read = stream.read(&mut chunk).expect("request headers");
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..read]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                requests.push(String::from_utf8_lossy(&request).to_string());
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
            requests
        });

        let http = Arc::new(ReqwestQuiverHttp::new().expect("HTTP client"));
        let client = QuiverClient::with_parts(
            format!("http://{address}"),
            http,
            Arc::new(FakeSleeper::default()),
        );
        let result = client.listing();
        let requests = server.join().expect("fixture server");
        let entries = result.expect("the local fixture server answers");
        assert_eq!(entries.len(), 5);
        assert_eq!(requests.len(), 2);
        for request in &requests {
            let request_line = request.lines().next().unwrap_or_default();
            assert!(
                request_line.starts_with("GET "),
                "the client sent a non-GET request: {request_line}"
            );
            let lowercase = request.to_ascii_lowercase();
            assert!(lowercase.contains("user-agent: gamenode-desktop/"));
            assert!(request.contains("(+https://github.com/game-node-app/game-node-desktop)"));
        }
    }
}
