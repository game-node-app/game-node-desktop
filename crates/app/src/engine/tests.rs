use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use gamenode_core::callback::Provider;
use gamenode_core::credential::StorageMethod;
use gamenode_core::game_api::{
    ApiError, Collection, CoverError, ExternalMatch, GameApi, GameSummary, LibraryEntry,
};
use gamenode_core::library::cover::CoverUrl;
use gamenode_core::library::{EntryStatus, LibraryMachine};
use gamenode_core::ports::{
    AuthApi, BrowserError, BrowserOpener, CredentialError, CredentialStore, SettingsStore,
    StorageError,
};
use gamenode_core::provider::{Platform, memory::FixedRoots};
use gamenode_core::{
    AuthView, Input, Notice, SessionCredential, SessionError, Settings, SignInFailure, Snapshot,
    User, WindowGeometry,
};
use tokio::sync::watch;
use url::Url;

use super::{Engine, Ports};

mod drm_free;
mod library;
mod playtime;
mod tools;
use library::{FakeLibrary, FakeUri, SlowFiles};

fn ana() -> User {
    User {
        id: "u1".into(),
        name: "Ana".into(),
        email: Some("ana@example.com".into()),
        image: None,
    }
}

fn cred(v: &str) -> SessionCredential {
    SessionCredential::new(v).unwrap()
}

#[derive(Default)]
struct FakeApi {
    playtimes: Mutex<Vec<gamenode_core::playtime::Submission>>,
    playtime_delay: Mutex<Duration>,
    delay: Mutex<Duration>,
    begin: Mutex<Option<SignInFailure>>,
    session: Mutex<Option<Result<User, SessionError>>>,
    sign_out_ok: Mutex<bool>,
    started: Mutex<Vec<(Provider, String, String)>>,
    verified: Mutex<Vec<String>>,
    signed_out: Mutex<Vec<String>>,
    /// What GameNode answers to a lookup (`None`: it knows no game).
    lookup_result: Mutex<Option<Result<Vec<ExternalMatch>, ApiError>>>,
    lookups: Mutex<Vec<(u32, Vec<String>)>>,
    covers_fetched: Mutex<Vec<String>>,
    artworks_fetched: Mutex<Vec<u64>>,
    artwork_result: Mutex<Option<Result<Option<Vec<u8>>, ApiError>>>,
    search_result: Mutex<Option<Result<gamenode_core::game_api::SearchPage, ApiError>>>,
    entry: Mutex<Option<Result<Option<LibraryEntry>, ApiError>>>,
    created: Mutex<Vec<u64>>,
    updated: Mutex<Vec<u64>>,
}

impl FakeApi {
    fn new() -> Arc<Self> {
        let api = Arc::new(Self::default());
        *api.session.lock().unwrap() = Some(Ok(ana()));
        *api.sign_out_ok.lock().unwrap() = true;
        api
    }
    async fn pause(&self) {
        let delay = *self.delay.lock().unwrap();
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
    }
    fn success_url(&self) -> String {
        self.started
            .lock()
            .unwrap()
            .last()
            .expect("a sign-in was started")
            .1
            .clone()
    }
}

#[async_trait]
impl AuthApi for FakeApi {
    async fn begin_sign_in(
        &self,
        provider: Provider,
        success: &str,
        failure: &str,
    ) -> Result<Url, SignInFailure> {
        self.pause().await;
        self.started
            .lock()
            .unwrap()
            .push((provider, success.into(), failure.into()));
        match *self.begin.lock().unwrap() {
            Some(reason) => Err(reason),
            None => Ok(Url::parse(
                "https://server.test/v1/auth/capacitor-authorization-proxy?authorizationURL=x",
            )
            .unwrap()),
        }
    }
    async fn get_session(&self, credential: &SessionCredential) -> Result<User, SessionError> {
        self.pause().await;
        self.verified
            .lock()
            .unwrap()
            .push(credential.expose().into());
        self.session.lock().unwrap().clone().unwrap()
    }
    async fn sign_out(&self, credential: &SessionCredential) -> Result<(), ()> {
        self.pause().await;
        self.signed_out
            .lock()
            .unwrap()
            .push(credential.expose().into());
        if *self.sign_out_ok.lock().unwrap() {
            Ok(())
        } else {
            Err(())
        }
    }
}

#[async_trait]
impl GameApi for FakeApi {
    async fn lookup_external(
        &self,
        _credential: &SessionCredential,
        category: u32,
        uids: &[String],
    ) -> Result<Vec<ExternalMatch>, ApiError> {
        self.pause().await;
        self.lookups.lock().unwrap().push((category, uids.to_vec()));
        self.lookup_result
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(Ok(vec![]))
    }
    async fn search_games(
        &self,
        _credential: &SessionCredential,
        _query: &str,
    ) -> Result<Vec<GameSummary>, ApiError> {
        self.pause().await;
        if let Some(result) = self.search_result.lock().unwrap().clone() {
            return result.map(|p| p.games);
        }
        Ok(vec![])
    }
    async fn search_page(
        &self,
        credential: &SessionCredential,
        query: &str,
    ) -> Result<gamenode_core::game_api::SearchPage, ApiError> {
        self.pause().await;
        if let Some(result) = self.search_result.lock().unwrap().clone() {
            return result;
        }
        Ok(gamenode_core::game_api::SearchPage {
            games: self.search_games(credential, query).await?,
            complete: None,
        })
    }
    async fn fetch_cover(&self, cover: &CoverUrl) -> Result<Vec<u8>, CoverError> {
        self.pause().await;
        self.covers_fetched
            .lock()
            .unwrap()
            .push(cover.as_url().to_string());
        Ok(b"jpeg".to_vec())
    }
    async fn fetch_artwork(
        &self,
        _credential: &SessionCredential,
        game_id: u64,
    ) -> Result<Option<Vec<u8>>, ApiError> {
        self.artworks_fetched.lock().unwrap().push(game_id);
        self.artwork_result
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(Ok(None))
    }
    async fn list_collections(
        &self,
        _credential: &SessionCredential,
        _user_id: &str,
    ) -> Result<Vec<Collection>, ApiError> {
        self.pause().await;
        Ok(vec![])
    }
    async fn library_entry(
        &self,
        _credential: &SessionCredential,
        _game_id: u64,
    ) -> Result<Option<LibraryEntry>, ApiError> {
        self.pause().await;
        self.entry.lock().unwrap().clone().unwrap_or(Ok(None))
    }
    async fn update_entry(
        &self,
        _credential: &SessionCredential,
        game_id: u64,
        _status: EntryStatus,
        _collection_ids: &[String],
        _existing: &LibraryEntry,
    ) -> Result<(), ApiError> {
        self.updated.lock().unwrap().push(game_id);
        Ok(())
    }
    async fn create_entry(
        &self,
        _credential: &SessionCredential,
        game_id: u64,
        _status: EntryStatus,
        _collection_ids: &[String],
    ) -> Result<(), ApiError> {
        self.pause().await;
        self.created.lock().unwrap().push(game_id);
        Ok(())
    }
}

#[derive(Default)]
struct FakeCreds {
    value: Mutex<Option<SessionCredential>>,
    fail_load: Mutex<bool>,
    fail_save: Mutex<bool>,
    fail_delete: Mutex<bool>,
    delay: Mutex<Duration>,
    log: Mutex<Vec<&'static str>>,
}

impl FakeCreds {
    fn with(value: Option<&str>) -> Arc<Self> {
        let c = Arc::new(Self::default());
        *c.value.lock().unwrap() = value.map(cred);
        c
    }
    fn pause(&self) {
        let delay = *self.delay.lock().unwrap();
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }
    }
    fn stored(&self) -> Option<String> {
        self.value
            .lock()
            .unwrap()
            .as_ref()
            .map(|c| c.expose().to_owned())
    }
}

impl CredentialStore for FakeCreds {
    fn method(&self) -> StorageMethod {
        StorageMethod::SecretService
    }
    fn load(&self) -> Result<Option<SessionCredential>, CredentialError> {
        self.pause();
        self.log.lock().unwrap().push("load");
        if *self.fail_load.lock().unwrap() {
            return Err(CredentialError::Locked);
        }
        Ok(self.value.lock().unwrap().clone())
    }
    fn save(&self, credential: &SessionCredential) -> Result<(), CredentialError> {
        self.pause();
        self.log.lock().unwrap().push("save");
        if *self.fail_save.lock().unwrap() {
            return Err(CredentialError::Failed);
        }
        *self.value.lock().unwrap() = Some(credential.clone());
        Ok(())
    }
    fn delete(&self) -> Result<(), CredentialError> {
        self.pause();
        self.log.lock().unwrap().push("delete");
        if *self.fail_delete.lock().unwrap() {
            return Err(CredentialError::Failed);
        }
        self.value.lock().unwrap().take();
        Ok(())
    }
}

#[derive(Default)]
struct FakeBrowser {
    opened: Mutex<Vec<Url>>,
    fail: Mutex<bool>,
}

impl BrowserOpener for FakeBrowser {
    fn open(&self, url: &Url) -> Result<(), BrowserError> {
        if *self.fail.lock().unwrap() {
            return Err(BrowserError);
        }
        self.opened.lock().unwrap().push(url.clone());
        Ok(())
    }
}

#[derive(Default)]
struct FakeSettings {
    saved: Mutex<Vec<Settings>>,
}

impl SettingsStore for FakeSettings {
    fn load(&self) -> Result<Settings, StorageError> {
        Ok(Settings::default())
    }
    fn save(&self, settings: &Settings) -> Result<(), StorageError> {
        self.saved.lock().unwrap().push(*settings);
        Ok(())
    }
}

struct Harness {
    processes: Arc<gamenode_core::tracker::scripted::ScriptedProcesses>,
    engine: Engine,
    snapshots: watch::Receiver<Snapshot>,
    api: Arc<FakeApi>,
    creds: Arc<FakeCreds>,
    browser: Arc<FakeBrowser>,
    settings: Arc<FakeSettings>,
    store: Arc<FakeLibrary>,
    files: Arc<SlowFiles>,
    uri: Arc<FakeUri>,
    covers: tempfile::TempDir,
}

fn harness(api: Arc<FakeApi>, creds: Arc<FakeCreds>) -> Harness {
    harness_over(api, creds, None)
}

#[tokio::test(flavor = "multi_thread")]
async fn idle_auth_ticks_do_not_publish_rebuilt_snapshots() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    let _ = h.snapshots.borrow_and_update().clone();

    h.engine.dispatch(Input::Tick);

    assert!(!h.snapshots.has_changed().unwrap());
}

/// Like [`harness`], but the GameNode auth and game requests go through `remote` when given.
fn harness_over(
    api: Arc<FakeApi>,
    creds: Arc<FakeCreds>,
    remote: Option<Arc<gamenode_api::GameNodeClient>>,
) -> Harness {
    let browser = Arc::new(FakeBrowser::default());
    let settings = Arc::new(FakeSettings::default());
    let store = Arc::new(FakeLibrary::default());
    let files = Arc::new(SlowFiles::steam_with_portal_2());
    let uri = Arc::new(FakeUri::default());
    let covers = tempfile::tempdir().unwrap();
    let playtime_db = gamenode_platform::db::SqliteSettingsStore::new(
        gamenode_platform::db::open(std::path::Path::new(":memory:")).unwrap(),
    );
    let processes = Arc::new(gamenode_core::tracker::scripted::ScriptedProcesses::default());
    let ports = Ports {
        backups: None,
        local_games: None,
        installations: None,
        tools: None,
        tool_catalog: None,
        tool_installer: None,
        processes: processes.clone(),
        playtime: Arc::new(playtime_db.playtime_store()),
        playtime_api: api.clone(),
        settings: settings.clone(),
        credentials: creds.clone(),
        api: match &remote {
            Some(remote) => remote.clone(),
            None => api.clone(),
        },
        browser: browser.clone(),
        library: store.clone(),
        games: match &remote {
            Some(remote) => remote.clone(),
            None => api.clone(),
        },
        files: files.clone(),
        roots: Arc::new(FixedRoots::with_home("/home/me")),
        uri: uri.clone(),
        covers_dir: covers.path().to_owned(),
        platform: Platform::Linux,
    };
    let mut n = 0;
    let nonce: gamenode_core::auth::NonceSource = Box::new(move || {
        n += 1;
        format!("nonce{n}")
    });
    let (tx, rx) = watch::channel(Snapshot {
        local_games: Default::default(),
        playtime_error: false,
        tracking: Default::default(),
        sync: Default::default(),
        playtime_source: Default::default(),
        auth: AuthView::Starting,
        notice: None,
        storage: StorageMethod::SecretService,
        settings: Settings::default(),
        library: None,
        launch: None,
        tools: Default::default(),
    });
    let providers = crate::providers::load_bundled(Platform::Linux).providers;
    let engine = Engine::new(
        ports,
        tokio::runtime::Handle::current(),
        Settings::default(),
        LibraryMachine::new(providers, Default::default()),
        nonce,
        move |s| {
            let _ = tx.send(s);
        },
    );
    Harness {
        processes,
        engine,
        snapshots: rx,
        api,
        creds,
        browser,
        settings,
        store,
        files,
        uri,
        covers,
    }
}

impl Harness {
    async fn until(
        &mut self,
        what: &str,
        mut predicate: impl FnMut(&Snapshot) -> bool,
    ) -> Snapshot {
        let result = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                {
                    let snapshot = self.snapshots.borrow_and_update().clone();
                    if predicate(&snapshot) {
                        return snapshot;
                    }
                }
                self.snapshots.changed().await.unwrap();
            }
        })
        .await;
        result.unwrap_or_else(|_| {
            panic!(
                "timed out waiting for: {what}; last = {:?}",
                self.snapshots.borrow()
            )
        })
    }

    async fn signed_out(&mut self) {
        self.engine.start();
        self.until("signed out", |s| s.auth == AuthView::SignedOut)
            .await;
    }

    /// The server's redirect: the app's success URL plus the `cookie` parameter.
    fn callback(&self, token: &str) -> String {
        let cookie = format!("better-auth.session_token={token}; Max-Age=604800; Path=/; HttpOnly");
        let encoded: String = url::form_urlencoded::byte_serialize(cookie.as_bytes()).collect();
        format!("{}&cookie={encoded}", self.api.success_url())
    }

    async fn sign_in(&mut self, token: &str) {
        self.engine.dispatch(Input::StartSignIn(Provider::Google));
        self.until("browser opened", |s| {
            matches!(
                s.auth,
                AuthView::Pending {
                    awaiting_browser: true,
                    ..
                }
            )
        })
        .await;
        let callback = self.callback(token);
        self.engine.dispatch(Input::CallbackReceived(callback));
        self.until("signed in", |s| matches!(s.auth, AuthView::SignedIn { .. }))
            .await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn fixture_browser_flow_signs_in_stores_and_shows_the_user() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;

    h.engine.dispatch(Input::StartSignIn(Provider::Discord));
    h.until("awaiting browser", |s| {
        matches!(
            s.auth,
            AuthView::Pending {
                provider: Provider::Discord,
                awaiting_browser: true
            }
        )
    })
    .await;
    // The API got the exact callbacks and the browser got the proxy URL.
    let started = h.api.started.lock().unwrap().clone();
    assert_eq!(started[0].0, Provider::Discord);
    assert_eq!(started[0].1, "app.gamenode://home?desktop_attempt=nonce1");
    assert_eq!(
        started[0].2,
        "app.gamenode://home/sign-in-failed?desktop_attempt=nonce1"
    );
    for _ in 0..100 {
        if !h.browser.opened.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(h.browser.opened.lock().unwrap().len(), 1);
    assert_eq!(
        h.browser.opened.lock().unwrap()[0].path(),
        "/v1/auth/capacitor-authorization-proxy"
    );

    let callback = h.callback("signed.token+/==");
    h.engine.dispatch(Input::CallbackReceived(callback));
    let snapshot = h
        .until("signed in", |s| matches!(s.auth, AuthView::SignedIn { .. }))
        .await;
    let AuthView::SignedIn { user } = snapshot.auth else {
        unreachable!()
    };
    assert_eq!(user.display_name(), "Ana");
    // The credential was verified with GameNode, then stored, exactly as received.
    assert_eq!(*h.api.verified.lock().unwrap(), ["signed.token+/=="]);
    for _ in 0..50 {
        if h.creds.stored().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(h.creds.stored().as_deref(), Some("signed.token+/=="));
    assert_eq!(snapshot.notice, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_replayed_and_late_callbacks_cannot_replace_the_session() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("good").await;

    // Replay of the consumed callback, an intruder's attempt id, and garbage.
    let replay = h.callback("evil");
    for raw in [
        replay,
        "app.gamenode://home?desktop_attempt=other&cookie=better-auth.session_token%3Devil"
            .to_string(),
        "x".to_string(),
    ] {
        h.engine.dispatch(Input::CallbackReceived(raw));
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    let snapshot = h.engine.snapshot();
    assert!(matches!(snapshot.auth, AuthView::SignedIn { .. }));
    assert_eq!(h.creds.stored().as_deref(), Some("good"));
    assert_eq!(
        *h.api.verified.lock().unwrap(),
        ["good"],
        "no callback reached verification"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rejected_session_from_a_callback_does_not_sign_in_or_store() {
    let api = FakeApi::new();
    *api.session.lock().unwrap() = Some(Err(SessionError::Rejected));
    let mut h = harness(api, FakeCreds::with(None));
    h.signed_out().await;
    h.engine.dispatch(Input::StartSignIn(Provider::Google));
    h.until("awaiting", |s| {
        matches!(
            s.auth,
            AuthView::Pending {
                awaiting_browser: true,
                ..
            }
        )
    })
    .await;
    let callback = h.callback("forged");
    h.engine.dispatch(Input::CallbackReceived(callback));
    let s = h
        .until("rejected", |s| s.notice == Some(Notice::SignInRejected))
        .await;
    assert_eq!(s.auth, AuthView::SignedOut);
    assert_eq!(h.creds.stored(), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn browser_launch_and_server_failures_leave_signed_out_and_allow_retry() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;

    *h.browser.fail.lock().unwrap() = true;
    h.engine.dispatch(Input::StartSignIn(Provider::Google));
    h.until("launch failed", |s| {
        s.notice == Some(Notice::BrowserLaunchFailed)
    })
    .await;
    assert_eq!(h.engine.snapshot().auth, AuthView::SignedOut);

    *h.browser.fail.lock().unwrap() = false;
    *h.api.begin.lock().unwrap() = Some(SignInFailure::ServerUnavailable);
    h.engine.dispatch(Input::StartSignIn(Provider::Google));
    h.until("server failed", |s| {
        s.notice == Some(Notice::SignInUnavailable)
    })
    .await;

    *h.api.begin.lock().unwrap() = None;
    h.engine.dispatch(Input::StartSignIn(Provider::Google));
    h.until("retry works", |s| {
        matches!(
            s.auth,
            AuthView::Pending {
                awaiting_browser: true,
                ..
            }
        )
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn restart_with_a_valid_stored_session_signs_in_without_the_browser() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(Some("stored")));
    h.engine.start();
    let s = h
        .until("restored", |s| matches!(s.auth, AuthView::SignedIn { .. }))
        .await;
    assert_eq!(s.notice, None);
    assert!(h.browser.opened.lock().unwrap().is_empty());
    assert!(h.api.started.lock().unwrap().is_empty());
    assert_eq!(*h.api.verified.lock().unwrap(), ["stored"]);
    assert_eq!(
        h.creds.stored().as_deref(),
        Some("stored"),
        "restoration does not rewrite the credential"
    );
    assert!(!h.creds.log.lock().unwrap().contains(&"save"));
}

#[tokio::test(flavor = "multi_thread")]
async fn restart_with_a_rejected_session_removes_the_credential() {
    let api = FakeApi::new();
    *api.session.lock().unwrap() = Some(Err(SessionError::Rejected));
    let mut h = harness(api, FakeCreds::with(Some("revoked")));
    h.engine.start();
    let s = h
        .until("rejected", |s| s.notice == Some(Notice::SessionRejected))
        .await;
    assert_eq!(s.auth, AuthView::SignedOut);
    for _ in 0..50 {
        if h.creds.stored().is_none() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(h.creds.stored(), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn restart_when_offline_keeps_the_credential_and_retry_recovers() {
    let api = FakeApi::new();
    *api.session.lock().unwrap() = Some(Err(SessionError::Unreachable));
    let mut h = harness(api, FakeCreds::with(Some("stored")));
    h.engine.start();
    let s = h
        .until("offline", |s| s.auth == AuthView::RestoreFailed)
        .await;
    assert_eq!(s.notice, Some(Notice::ConnectionProblem));
    assert_eq!(h.creds.stored().as_deref(), Some("stored"));
    assert!(
        !h.creds.log.lock().unwrap().contains(&"delete"),
        "a timeout never deletes the credential"
    );

    *h.api.session.lock().unwrap() = Some(Ok(ana()));
    h.engine.dispatch(Input::RetryRestore);
    h.until("recovered", |s| matches!(s.auth, AuthView::SignedIn { .. }))
        .await;
    assert_eq!(s_notice(&h), None);
}

fn s_notice(h: &Harness) -> Option<Notice> {
    h.engine.snapshot().notice
}

#[tokio::test(flavor = "multi_thread")]
async fn unreadable_credential_store_at_startup_is_reported() {
    let creds = FakeCreds::with(Some("stored"));
    *creds.fail_load.lock().unwrap() = true;
    let mut h = harness(FakeApi::new(), creds);
    h.engine.start();
    let s = h
        .until("unreadable", |s| {
            s.notice == Some(Notice::CredentialUnreadable)
        })
        .await;
    assert_eq!(s.auth, AuthView::SignedOut);
}

#[tokio::test(flavor = "multi_thread")]
async fn persist_failure_is_reported_while_the_session_stays_usable() {
    let creds = FakeCreds::with(None);
    *creds.fail_save.lock().unwrap() = true;
    let mut h = harness(FakeApi::new(), creds);
    h.signed_out().await;
    h.engine.dispatch(Input::StartSignIn(Provider::Google));
    h.until("awaiting", |s| {
        matches!(
            s.auth,
            AuthView::Pending {
                awaiting_browser: true,
                ..
            }
        )
    })
    .await;
    let callback = h.callback("tok");
    h.engine.dispatch(Input::CallbackReceived(callback));
    let s = h
        .until("persist failure", |s| {
            s.notice == Some(Notice::PersistFailed)
        })
        .await;
    assert!(matches!(s.auth, AuthView::SignedIn { .. }));
    assert_eq!(h.creds.stored(), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn sign_out_online_invalidates_the_server_session_and_is_not_restored() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("tok").await;

    h.engine.dispatch(Input::SignOut);
    let s = h
        .until("signed out", |s| s.auth == AuthView::SignedOut)
        .await;
    assert_eq!(s.notice, None);
    assert_eq!(*h.api.signed_out.lock().unwrap(), ["tok"]);
    assert_eq!(h.creds.stored(), None);

    // "Restart": a new engine over the same storage stays signed out.
    let mut again = harness(FakeApi::new(), h.creds.clone());
    again.engine.start();
    again
        .until("still signed out", |s| s.auth == AuthView::SignedOut)
        .await;
    assert!(again.api.verified.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn sign_out_offline_removes_local_authentication_and_says_so() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("tok").await;
    *h.api.sign_out_ok.lock().unwrap() = false;

    h.engine.dispatch(Input::SignOut);
    let s = h
        .until("not confirmed", |s| {
            s.notice == Some(Notice::SignOutNotConfirmed)
        })
        .await;
    assert_eq!(s.auth, AuthView::SignedOut);
    assert_eq!(h.creds.stored(), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_credential_deletion_on_sign_out_is_reported() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("tok").await;
    *h.creds.fail_delete.lock().unwrap() = true;

    h.engine.dispatch(Input::SignOut);
    let s = h
        .until("delete failed", |s| {
            s.notice == Some(Notice::CredentialDeleteFailed)
        })
        .await;
    assert_eq!(s.auth, AuthView::SignedOut);
}

#[tokio::test(flavor = "multi_thread")]
async fn sign_out_cancels_a_pending_attempt_so_a_late_callback_does_nothing() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    h.engine.dispatch(Input::StartSignIn(Provider::Google));
    h.until("awaiting", |s| {
        matches!(
            s.auth,
            AuthView::Pending {
                awaiting_browser: true,
                ..
            }
        )
    })
    .await;
    let late = h.callback("late");

    h.engine.dispatch(Input::SignOut);
    h.until("signed out", |s| s.auth == AuthView::SignedOut)
        .await;
    h.engine.dispatch(Input::CallbackReceived(late));
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(h.engine.snapshot().auth, AuthView::SignedOut);
    assert!(h.api.verified.lock().unwrap().is_empty());
    assert_eq!(h.creds.stored(), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn sign_out_during_verification_prevents_a_late_sign_in() {
    let api = FakeApi::new();
    let mut h = harness(api, FakeCreds::with(None));
    h.signed_out().await;
    h.engine.dispatch(Input::StartSignIn(Provider::Google));
    h.until("awaiting", |s| {
        matches!(
            s.auth,
            AuthView::Pending {
                awaiting_browser: true,
                ..
            }
        )
    })
    .await;

    *h.api.delay.lock().unwrap() = Duration::from_millis(300);
    let callback = h.callback("tok");
    h.engine.dispatch(Input::CallbackReceived(callback));
    h.until("verifying", |s| s.auth == AuthView::Verifying)
        .await;
    h.engine.dispatch(Input::SignOut);

    h.until("signed out", |s| s.auth == AuthView::SignedOut)
        .await;
    tokio::time::sleep(Duration::from_millis(600)).await; // the slow verification completes meanwhile
    assert_eq!(h.engine.snapshot().auth, AuthView::SignedOut);
    assert_eq!(
        h.creds.stored(),
        None,
        "nothing was stored for the abandoned sign-in"
    );
    assert!(!h.creds.log.lock().unwrap().contains(&"save"));
}

#[tokio::test(flavor = "multi_thread")]
async fn slow_network_and_keyring_never_block_the_caller() {
    let api = FakeApi::new();
    *api.delay.lock().unwrap() = Duration::from_millis(500);
    let creds = FakeCreds::with(Some("stored"));
    *creds.delay.lock().unwrap() = Duration::from_millis(500);
    let mut h = harness(api, creds);

    let started = Instant::now();
    h.engine.start();
    h.engine.update_settings(|s| {
        s.window = Some(WindowGeometry {
            x: Some(10),
            y: Some(20),
            width: 800,
            height: 600,
            maximized: false,
        });
    });
    let _ = h.engine.snapshot();
    h.engine.dispatch(Input::CancelSignIn);
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "dispatch must not wait for I/O: {:?}",
        started.elapsed()
    );

    // The settings change is visible immediately even though restoration is still running.
    assert_eq!(
        h.engine.snapshot().settings.window,
        Some(WindowGeometry {
            x: Some(10),
            y: Some(20),
            width: 800,
            height: 600,
            maximized: false,
        })
    );
    assert_eq!(h.engine.snapshot().auth, AuthView::Starting);
    h.until("restored eventually", |s| {
        matches!(s.auth, AuthView::SignedIn { .. })
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn settings_changes_are_persisted_in_order() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    let geom1 = WindowGeometry {
        x: Some(1),
        y: Some(1),
        width: 800,
        height: 600,
        maximized: false,
    };
    let geom2 = WindowGeometry {
        x: Some(2),
        y: Some(2),
        width: 1024,
        height: 768,
        maximized: true,
    };
    h.engine.update_settings(|s| s.window = Some(geom1));
    h.engine.update_settings(|s| s.window = Some(geom2));
    for _ in 0..50 {
        if h.settings.saved.lock().unwrap().len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let saved = h.settings.saved.lock().unwrap().clone();
    assert_eq!(
        saved.iter().map(|s| s.window).collect::<Vec<_>>(),
        [Some(geom1), Some(geom2)]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn cold_start_callback_with_no_pending_attempt_is_rejected() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    h.engine.dispatch(Input::CallbackReceived(
        "app.gamenode://home?desktop_attempt=nonce1&cookie=better-auth.session_token%3Dtok".into(),
    ));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(h.engine.snapshot().auth, AuthView::SignedOut);
    assert!(h.api.verified.lock().unwrap().is_empty());
    assert_eq!(h.creds.stored(), None);
}

#[async_trait]
impl gamenode_core::ports::PlaytimeApi for FakeApi {
    async fn submit(
        &self,
        _credential: &SessionCredential,
        total: &gamenode_core::playtime::Submission,
    ) -> Result<(), gamenode_core::playtime::SubmitError> {
        self.playtimes.lock().unwrap().push(total.clone());
        let delay = *self.playtime_delay.lock().unwrap();
        tokio::time::sleep(delay).await;
        Ok(())
    }
}

#[tokio::test]
async fn artwork_survives_reload_and_is_separate_from_covers() {
    let api = FakeApi::new();
    let dir = tempfile::tempdir().unwrap();
    let artwork_dir = dir.path().join("artworks");
    let bytes = b"\xff\xd8\xfflandscape".to_vec();
    *api.artwork_result.lock().unwrap() = Some(Ok(Some(bytes.clone())));
    std::fs::write(dir.path().join("7.jpg"), b"portrait").unwrap();
    assert_eq!(
        super::artwork::load(api.as_ref(), Some(&cred("test")), &artwork_dir, 7).await,
        Ok(Some(bytes.clone()))
    );
    // A fresh API adapter and no session: only the persisted file can provide this image.
    let fresh_api = FakeApi::new();
    assert_eq!(
        super::artwork::load(fresh_api.as_ref(), None, &artwork_dir, 7).await,
        Ok(Some(bytes))
    );
    assert!(fresh_api.artworks_fetched.lock().unwrap().is_empty());
    assert_eq!(
        std::fs::read(dir.path().join("7.jpg")).unwrap(),
        b"portrait"
    );
    assert_eq!(*api.artworks_fetched.lock().unwrap(), [7]);
}

#[tokio::test]
async fn corrupt_artwork_is_replaced_and_failures_can_be_retried() {
    let api = FakeApi::new();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("7.jpg"), b"broken").unwrap();
    *api.artwork_result.lock().unwrap() = Some(Err(ApiError::Failed));
    assert_eq!(
        super::artwork::load(api.as_ref(), Some(&cred("test")), dir.path(), 7).await,
        Err(ApiError::Failed)
    );
    let bytes = b"\xff\xd8\xfflandscape".to_vec();
    *api.artwork_result.lock().unwrap() = Some(Ok(Some(bytes.clone())));
    assert_eq!(
        super::artwork::load(api.as_ref(), Some(&cred("test")), dir.path(), 7).await,
        Ok(Some(bytes.clone()))
    );
    assert_eq!(std::fs::read(dir.path().join("7.jpg")).unwrap(), bytes);
}
