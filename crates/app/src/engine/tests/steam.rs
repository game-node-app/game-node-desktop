//! Engine tests for the Steam store account: startup restore, sign-in, catalog sync, sign-out,
//! superseded results, coalescing and the secret-free `storeAccounts` snapshot.
//!
//! Every adapter is a fake, so the tests show the engine's orchestration without touching the
//! network, a keyring or a webview.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use gamenode_core::credential::StorageMethod;
use gamenode_core::library::{
    CatalogError, LastAccount, LibraryCache, ProviderScanState, StoreAccount, StoredEntry,
};
use gamenode_core::ports::{CredentialError, CredentialStore, StorageError};
use gamenode_core::snapshot::{StoreAccountState, StoreAccountView};
use gamenode_core::steam::{SteamError, SteamFamily, SteamOwnedGame, SteamSession, SteamWebApi};
use gamenode_core::{SessionCredential, Snapshot};
use tokio::sync::{Semaphore, oneshot};

use super::{FakeCreds, Harness, harness_with_steam};
use crate::engine::SteamPorts;
use crate::engine::steam_account::{Clock, StoreAccountStore};
use crate::engine::steam_login::{
    SteamLoginError, SteamLoginOutcome, SteamLoginResult, SteamLoginWindow,
};

const NOW: i64 = 1_700_000_000;
const LOGIN_COOKIE: &str = "steamLoginSecure=secret-cookie; sessionid=abc";
const WEBAPI_TOKEN: &str = "webapi-token-secret";

fn cred(value: &str) -> SessionCredential {
    SessionCredential::new(value).expect("a test credential")
}

fn session(id: &str, name: &str, cookie: &str) -> SteamSession {
    SteamSession {
        steam_id: id.to_owned(),
        account_name: Some(name.to_owned()),
        token: cred(WEBAPI_TOKEN),
        cookie: cred(cookie),
    }
}

fn stored_row(id: &str, name: &str) -> StoreAccount {
    StoreAccount {
        provider: "steam".to_owned(),
        account_id: id.to_owned(),
        account_name: Some(name.to_owned()),
        last_verified_at: Some(NOW - 60),
    }
}

fn game(id: &str, name: &str) -> SteamOwnedGame {
    SteamOwnedGame {
        app_id: id.to_owned(),
        name: name.to_owned(),
    }
}

fn account(id: &str, name: &str) -> LastAccount {
    LastAccount {
        id: id.to_owned(),
        name: name.to_owned(),
    }
}

fn signed_in(cookie: &str) -> SteamLoginOutcome {
    SteamLoginOutcome {
        attempt: 1,
        result: SteamLoginResult::Verified {
            session: session("B", "cassi", cookie),
        },
    }
}

fn entry(key: &str, name: &str, installed: bool, owned: bool, family: bool) -> StoredEntry {
    StoredEntry {
        key: key.to_owned(),
        provider: "steam".to_owned(),
        store_id: key.rsplit(':').next().expect("a key").to_owned(),
        name: Some(name.to_owned()),
        install_dir: if installed {
            name.to_owned()
        } else {
            String::new()
        },
        fields: Default::default(),
        installed,
        owned,
        family_shared: family,
        first_seen: 1,
        last_seen: 1,
    }
}

fn seed(h: &Harness, entries: Vec<StoredEntry>, account: Option<LastAccount>) {
    *h.store.account.lock().unwrap() = account;
    *h.store.cache.lock().unwrap() = LibraryCache {
        entries,
        scans: vec![ProviderScanState {
            provider: "steam".to_owned(),
            last_success_at: Some(1),
            ..ProviderScanState::default()
        }],
        ..LibraryCache::default()
    };
}

async fn eventually(what: &str, mut condition: impl FnMut() -> bool) {
    let waited = tokio::time::timeout(Duration::from_secs(5), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await;
    assert!(waited.is_ok(), "timed out waiting for: {what}");
}

fn card(snapshot: &Snapshot) -> &StoreAccountView {
    snapshot
        .store_accounts
        .first()
        .expect("the Steam card is always in the snapshot")
}

fn state(snapshot: &Snapshot) -> StoreAccountState {
    snapshot
        .store_accounts
        .first()
        .map_or(StoreAccountState::NotConnected, |card| card.state)
}

fn no_owned_rows(snapshot: &Snapshot) -> bool {
    snapshot
        .library
        .as_ref()
        .is_none_or(|view| view.rows.iter().all(|row| !row.owned && !row.family_shared))
}

fn assert_no_secret(snapshot: &Snapshot) {
    let json = serde_json::to_string(snapshot).expect("a snapshot serializes");
    assert!(
        json.contains("\"storeAccounts\""),
        "the card field is present"
    );
    for secret in [
        "steamLoginSecure",
        "sessionid",
        WEBAPI_TOKEN,
        "secret-cookie",
    ] {
        assert!(
            !json.contains(secret),
            "a snapshot must never carry {secret}: {json}"
        );
    }
}

struct FakeCredentialStore {
    value: Mutex<Option<SessionCredential>>,
    fail_load: AtomicBool,
    fail_save: AtomicBool,
    fail_delete: AtomicBool,
    saved: Mutex<Vec<String>>,
    deletes: AtomicUsize,
}

impl FakeCredentialStore {
    fn new(value: Option<&str>) -> Self {
        Self {
            value: Mutex::new(value.map(cred)),
            fail_load: AtomicBool::new(false),
            fail_save: AtomicBool::new(false),
            fail_delete: AtomicBool::new(false),
            saved: Mutex::new(Vec::new()),
            deletes: AtomicUsize::new(0),
        }
    }

    fn stored(&self) -> Option<String> {
        self.value
            .lock()
            .unwrap()
            .as_ref()
            .map(|credential| credential.expose().to_owned())
    }
}

impl CredentialStore for FakeCredentialStore {
    fn method(&self) -> StorageMethod {
        StorageMethod::SecretService
    }

    fn load(&self) -> Result<Option<SessionCredential>, CredentialError> {
        if self.fail_load.load(Ordering::SeqCst) {
            Err(CredentialError::Locked)
        } else {
            Ok(self.value.lock().unwrap().clone())
        }
    }

    fn save(&self, credential: &SessionCredential) -> Result<(), CredentialError> {
        if self.fail_save.load(Ordering::SeqCst) {
            return Err(CredentialError::Failed);
        }
        self.saved
            .lock()
            .unwrap()
            .push(credential.expose().to_owned());
        *self.value.lock().unwrap() = Some(credential.clone());
        Ok(())
    }

    fn delete(&self) -> Result<(), CredentialError> {
        if self.fail_delete.load(Ordering::SeqCst) {
            return Err(CredentialError::Failed);
        }
        self.deletes.fetch_add(1, Ordering::SeqCst);
        *self.value.lock().unwrap() = None;
        Ok(())
    }
}

#[derive(Default)]
struct FakeAccounts {
    account: Mutex<Option<StoreAccount>>,
    fail_save: AtomicBool,
    clears: AtomicUsize,
}

impl StoreAccountStore for FakeAccounts {
    fn load(&self) -> Result<Option<StoreAccount>, StorageError> {
        Ok(self.account.lock().unwrap().clone())
    }

    fn save(&self, account: &StoreAccount) -> Result<(), StorageError> {
        if self.fail_save.load(Ordering::SeqCst) {
            return Err(StorageError::Failed("test save failure".to_owned()));
        }
        *self.account.lock().unwrap() = Some(account.clone());
        Ok(())
    }

    fn clear(&self) -> Result<(), StorageError> {
        self.clears.fetch_add(1, Ordering::SeqCst);
        *self.account.lock().unwrap() = None;
        Ok(())
    }
}

struct FakeClock(AtomicI64);

impl Clock for FakeClock {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct FakeSteamClient {
    session: Mutex<Option<Result<SteamSession, SteamError>>>,
    owned: Mutex<Option<Result<Vec<SteamOwnedGame>, SteamError>>>,
    family: Mutex<Option<Result<SteamFamily, SteamError>>>,
    session_calls: AtomicUsize,
    owned_calls: AtomicUsize,
    family_calls: AtomicUsize,
    started: Semaphore,
    release: Mutex<Option<std::sync::Arc<Semaphore>>>,
}

impl FakeSteamClient {
    fn new() -> Self {
        Self {
            session: Mutex::new(None),
            owned: Mutex::new(None),
            family: Mutex::new(None),
            session_calls: AtomicUsize::new(0),
            owned_calls: AtomicUsize::new(0),
            family_calls: AtomicUsize::new(0),
            started: Semaphore::new(0),
            release: Mutex::new(None),
        }
    }

    fn set_session(&self, value: Result<SteamSession, SteamError>) {
        *self.session.lock().unwrap() = Some(value);
    }

    fn set_owned(&self, value: Result<Vec<SteamOwnedGame>, SteamError>) {
        *self.owned.lock().unwrap() = Some(value);
    }

    fn set_family(&self, value: Result<SteamFamily, SteamError>) {
        *self.family.lock().unwrap() = Some(value);
    }

    fn hold_until(&self, release: std::sync::Arc<Semaphore>) {
        *self.release.lock().unwrap() = Some(release);
    }

    async fn hold(&self) {
        let release = { self.release.lock().unwrap().clone() };
        if let Some(release) = release {
            self.started.add_permits(1);
            let _ = release.acquire().await;
        }
    }

    async fn wait_started(&self) {
        self.started
            .acquire()
            .await
            .expect("the fake is alive")
            .forget();
    }
}

#[async_trait]
impl SteamWebApi for FakeSteamClient {
    async fn session(&self, _cookie: &SessionCredential) -> Result<SteamSession, SteamError> {
        self.session_calls.fetch_add(1, Ordering::SeqCst);
        self.hold().await;
        self.session
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(Err(SteamError::Unreachable))
    }

    async fn owned_games(
        &self,
        _session: &SteamSession,
    ) -> Result<Vec<SteamOwnedGame>, SteamError> {
        self.owned_calls.fetch_add(1, Ordering::SeqCst);
        self.hold().await;
        self.owned.lock().unwrap().clone().unwrap_or(Ok(Vec::new()))
    }

    async fn family_games(&self, _session: &SteamSession) -> Result<SteamFamily, SteamError> {
        self.family_calls.fetch_add(1, Ordering::SeqCst);
        self.hold().await;
        self.family
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(Ok(SteamFamily::default()))
    }
}

struct FakeLogin {
    next: Mutex<VecDeque<SteamLoginOutcome>>,
    pending: Mutex<Option<oneshot::Receiver<SteamLoginOutcome>>>,
    cancelled: std::sync::Arc<Semaphore>,
    run_calls: AtomicUsize,
    focus_calls: AtomicUsize,
    cancel_calls: AtomicUsize,
    cleanup_calls: AtomicUsize,
}

impl Default for FakeLogin {
    fn default() -> Self {
        Self {
            next: Mutex::new(VecDeque::new()),
            pending: Mutex::new(None),
            cancelled: std::sync::Arc::new(Semaphore::new(0)),
            run_calls: AtomicUsize::new(0),
            focus_calls: AtomicUsize::new(0),
            cancel_calls: AtomicUsize::new(0),
            cleanup_calls: AtomicUsize::new(0),
        }
    }
}

impl FakeLogin {
    fn failed(&self, error: SteamLoginError) -> SteamLoginOutcome {
        SteamLoginOutcome {
            attempt: 1,
            result: SteamLoginResult::Failed(error),
        }
    }

    fn respond(&self, outcome: SteamLoginOutcome) {
        self.next.lock().unwrap().push_back(outcome);
    }

    fn set_pending(&self, receiver: oneshot::Receiver<SteamLoginOutcome>) {
        *self.pending.lock().unwrap() = Some(receiver);
    }
}

#[async_trait]
impl SteamLoginWindow for FakeLogin {
    async fn run(&self) -> SteamLoginOutcome {
        self.run_calls.fetch_add(1, Ordering::SeqCst);
        let pending = { self.pending.lock().unwrap().take() };
        if let Some(mut receiver) = pending {
            let outcome = tokio::select! {
                result = &mut receiver => {
                    result.unwrap_or_else(|_| self.failed(SteamLoginError::OpenFailed))
                }
                _ = self.cancelled.acquire() => SteamLoginOutcome {
                    attempt: 1,
                    result: SteamLoginResult::Cancelled,
                },
            };
            outcome
        } else {
            self.next
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| self.failed(SteamLoginError::OpenFailed))
        }
    }

    fn focus(&self) -> Result<(), SteamLoginError> {
        self.focus_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn cancel(&self) -> Result<(), SteamLoginError> {
        self.cancel_calls.fetch_add(1, Ordering::SeqCst);
        self.cancelled.add_permits(1);
        Ok(())
    }

    fn cleanup_profile(&self) {
        self.cleanup_calls.fetch_add(1, Ordering::SeqCst);
    }
}

struct SteamFixture {
    credentials: std::sync::Arc<FakeCredentialStore>,
    accounts: std::sync::Arc<FakeAccounts>,
    client: std::sync::Arc<FakeSteamClient>,
    login: std::sync::Arc<FakeLogin>,
    clock: std::sync::Arc<FakeClock>,
}

impl SteamFixture {
    fn new() -> Self {
        Self {
            credentials: std::sync::Arc::new(FakeCredentialStore::new(None)),
            accounts: std::sync::Arc::new(FakeAccounts::default()),
            client: std::sync::Arc::new(FakeSteamClient::new()),
            login: std::sync::Arc::new(FakeLogin::default()),
            clock: std::sync::Arc::new(FakeClock(AtomicI64::new(NOW))),
        }
    }

    fn with_stored(self, cookie: &str, account: Option<StoreAccount>) -> Self {
        *self.credentials.value.lock().unwrap() = Some(cred(cookie));
        *self.accounts.account.lock().unwrap() = account;
        self
    }

    fn ports(&self) -> SteamPorts {
        SteamPorts {
            credentials: self.credentials.clone(),
            accounts: self.accounts.clone(),
            client: self.client.clone(),
            login: self.login.clone(),
            clock: self.clock.clone(),
        }
    }
}

/// A harness whose GameNode session is restored, so the library is open.
fn steam_harness(fixture: &SteamFixture) -> Harness {
    harness_with_steam(
        super::FakeApi::new(),
        FakeCreds::with(Some("stored-session")),
        None,
        Some(fixture.ports()),
    )
}

/// A harness with no GameNode session, so the Steam card must still work and no catalog may sync.
fn steam_harness_signed_out(fixture: &SteamFixture) -> Harness {
    harness_with_steam(
        super::FakeApi::new(),
        FakeCreds::with(None),
        None,
        Some(fixture.ports()),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn startup_restores_a_stored_session_and_applies_the_catalog() {
    let fixture = SteamFixture::new().with_stored(LOGIN_COOKIE, Some(stored_row("A", "old-name")));
    fixture
        .client
        .set_session(Ok(session("A", "cassi", LOGIN_COOKIE)));
    fixture
        .client
        .set_owned(Ok(vec![game("620", "Portal 2"), game("400", "Portals")]));
    fixture.client.set_family(Ok(SteamFamily {
        group_id: Some("family".to_owned()),
        games: vec![game("730", "Counter-Strike 2")],
    }));
    let mut h = steam_harness(&fixture);
    seed(
        &h,
        vec![entry("steam:620", "Portal 2", true, false, false)],
        Some(account("u1", "Ana")),
    );

    h.engine.start();
    let snapshot = h
        .until("connected with the catalog", |s| {
            s.store_accounts.first().is_some_and(|card| {
                card.state == StoreAccountState::Connected
                    && card.owned_count == 2
                    && card.family_count == 1
            })
        })
        .await;

    let view = snapshot.library.as_ref().expect("the library is open");
    assert!(
        view.rows
            .iter()
            .any(|row| row.key == "steam:400" && row.owned)
    );
    assert!(
        view.rows
            .iter()
            .any(|row| row.key == "steam:730" && row.family_shared)
    );
    let card = card(&snapshot);
    assert_eq!(card.account_id.as_deref(), Some("A"));
    assert_eq!(card.account_name.as_deref(), Some("cassi"));
    assert_eq!(card.installed_count, 1);
    assert_eq!(card.last_checked_at, Some(NOW));
    assert_eq!(card.storage, Some(StorageMethod::SecretService));
    assert!(!card.persist_failed);
    assert!(card.can_check && card.can_sign_out);
    assert!(!card.can_sign_in);
    assert!(!card.can_cancel_sign_in && !card.can_focus_sign_in);
    // The unrotated cookie is not rewritten; the catalog is persisted for the next start.
    assert!(fixture.credentials.saved.lock().unwrap().is_empty());
    let store = h.store.clone();
    eventually("the catalog is persisted", || {
        !store.catalogs.lock().unwrap().is_empty()
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn signing_in_saves_the_cookie_and_imports_the_library() {
    let cookie = "steamLoginSecure=login-cookie";
    let fixture = SteamFixture::new();
    fixture
        .client
        .set_session(Ok(session("B", "cassi", cookie)));
    fixture.client.set_owned(Ok(vec![game("400", "Portals")]));
    let (sender, receiver) = oneshot::channel();
    fixture.login.set_pending(receiver);
    let mut h = steam_harness(&fixture);
    seed(&h, vec![], Some(account("u1", "Ana")));
    h.engine.start();
    h.until("the library is open and Steam is not connected", |s| {
        s.library.is_some() && state(s) == StoreAccountState::NotConnected
    })
    .await;

    let engine = h.engine.clone();
    let attempt = tokio::spawn(async move { engine.steam_sign_in().await });
    h.until("signing in", |s| state(s) == StoreAccountState::SigningIn)
        .await;
    sender.send(signed_in(cookie)).expect("the attempt waits");
    attempt.await.unwrap();

    let snapshot = h
        .until("connected with the imported count", |s| {
            s.store_accounts.first().is_some_and(|card| {
                card.state == StoreAccountState::Connected && card.owned_count == 1
            })
        })
        .await;
    assert_eq!(fixture.credentials.stored().as_deref(), Some(cookie));
    assert_eq!(
        fixture
            .accounts
            .account
            .lock()
            .unwrap()
            .as_ref()
            .map(|a| a.account_id.clone()),
        Some("B".to_owned())
    );
    assert!(
        snapshot
            .library
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .any(|row| row.key == "steam:400" && row.owned)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_sign_in_saves_nothing_and_restores_the_previous_state() {
    let fixture = SteamFixture::new();
    let (_sender, receiver) = oneshot::channel();
    fixture.login.set_pending(receiver);
    let mut h = steam_harness(&fixture);
    seed(&h, vec![], Some(account("u1", "Ana")));
    h.engine.start();
    h.until("not connected", |s| {
        state(s) == StoreAccountState::NotConnected
    })
    .await;

    let engine = h.engine.clone();
    let attempt = tokio::spawn(async move { engine.steam_sign_in().await });
    h.until("signing in", |s| state(s) == StoreAccountState::SigningIn)
        .await;
    h.engine.steam_cancel_sign_in().await;

    let snapshot = h
        .until("back to not connected", |s| {
            state(s) == StoreAccountState::NotConnected
        })
        .await;
    attempt.await.unwrap();
    assert_eq!(fixture.login.cancel_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.credentials.stored(), None);
    assert!(fixture.accounts.account.lock().unwrap().is_none());
    assert!(fixture.credentials.saved.lock().unwrap().is_empty());
    assert!(no_owned_rows(&snapshot));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_expired_session_removes_the_credential_and_keeps_the_cached_library() {
    let fixture = SteamFixture::new().with_stored(LOGIN_COOKIE, Some(stored_row("A", "cassi")));
    fixture.client.set_session(Err(SteamError::Expired));
    let mut h = steam_harness(&fixture);
    seed(
        &h,
        vec![
            entry("steam:400", "Portals", false, true, false),
            entry("steam:620", "Portal 2", true, false, false),
        ],
        Some(account("u1", "Ana")),
    );

    h.engine.start();
    let snapshot = h
        .until("expired with the cached library", |s| {
            state(s) == StoreAccountState::Expired
                && s.library.as_ref().is_some_and(|view| {
                    view.rows
                        .iter()
                        .any(|row| row.key == "steam:400" && row.owned)
                })
        })
        .await;
    assert_eq!(fixture.credentials.stored(), None);
    let card = card(&snapshot);
    assert!(card.can_sign_in && card.can_sign_out && !card.can_check);
    let view = snapshot.library.as_ref().unwrap();
    assert!(
        view.rows
            .iter()
            .any(|row| row.key == "steam:400" && row.owned),
        "the cached owned library stays visible"
    );
    assert!(
        view.rows
            .iter()
            .any(|row| row.key == "steam:620" && row.installed)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unreachable_steam_keeps_the_credential_and_the_cached_library() {
    let fixture = SteamFixture::new().with_stored(LOGIN_COOKIE, Some(stored_row("A", "cassi")));
    fixture.client.set_session(Err(SteamError::Unreachable));
    let mut h = steam_harness(&fixture);
    seed(
        &h,
        vec![entry("steam:400", "Portals", false, true, false)],
        Some(account("u1", "Ana")),
    );

    h.engine.start();
    let snapshot = h
        .until("unreachable with the cached library", |s| {
            state(s) == StoreAccountState::Unreachable
                && s.library.as_ref().is_some_and(|view| {
                    view.rows
                        .iter()
                        .any(|row| row.key == "steam:400" && row.owned)
                })
        })
        .await;
    assert_eq!(fixture.credentials.stored().as_deref(), Some(LOGIN_COOKIE));
    assert!(
        snapshot
            .library
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .any(|row| row.key == "steam:400" && row.owned)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_changed_steam_response_keeps_the_credential() {
    // "Steam changed its response" is not an authentication failure: the saved cookie must survive
    // so the next check can use it once Steam serves the expected page again.
    let fixture = SteamFixture::new().with_stored(LOGIN_COOKIE, Some(stored_row("A", "cassi")));
    fixture.client.set_session(Err(SteamError::Changed));
    let mut h = steam_harness(&fixture);
    seed(
        &h,
        vec![entry("steam:400", "Portals", false, true, false)],
        Some(account("u1", "Ana")),
    );

    h.engine.start();
    let snapshot = h
        .until("a non-credential failure keeps the session", |s| {
            state(s) == StoreAccountState::Unreachable
                && s.library.as_ref().is_some_and(|view| {
                    view.rows
                        .iter()
                        .any(|row| row.key == "steam:400" && row.owned)
                })
        })
        .await;
    assert_eq!(fixture.credentials.stored().as_deref(), Some(LOGIN_COOKIE));
    assert!(card(&snapshot).can_check);
    assert!(card(&snapshot).can_sign_in);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_family_failure_keeps_the_owned_catalog() {
    let fixture = SteamFixture::new().with_stored(LOGIN_COOKIE, Some(stored_row("A", "cassi")));
    fixture
        .client
        .set_session(Ok(session("A", "cassi", LOGIN_COOKIE)));
    fixture.client.set_owned(Ok(vec![game("400", "Portals")]));
    fixture.client.set_family(Err(SteamError::Server));
    let mut h = steam_harness(&fixture);
    seed(&h, vec![], Some(account("u1", "Ana")));

    h.engine.start();
    let snapshot = h
        .until(
            "the owned catalog imports despite the family failure",
            |s| {
                s.store_accounts.first().is_some_and(|card| {
                    card.state == StoreAccountState::Connected
                        && card.owned_count == 1
                        && card.family_error == Some(CatalogError::FamilyUnavailable)
                })
            },
        )
        .await;
    assert_eq!(fixture.credentials.stored().as_deref(), Some(LOGIN_COOKIE));
    assert!(
        snapshot
            .library
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .any(|row| row.key == "steam:400" && row.owned)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_credential_save_leaves_the_account_connected_for_this_run() {
    let cookie = "steamLoginSecure=login-cookie";
    let fixture = SteamFixture::new();
    fixture
        .client
        .set_session(Ok(session("B", "cassi", cookie)));
    fixture.client.set_owned(Ok(vec![game("400", "Portals")]));
    fixture.credentials.fail_save.store(true, Ordering::SeqCst);
    fixture.login.respond(signed_in(cookie));
    let mut h = steam_harness(&fixture);
    seed(&h, vec![], Some(account("u1", "Ana")));
    h.engine.start();
    h.until("not connected", |s| {
        state(s) == StoreAccountState::NotConnected
    })
    .await;
    // Let the seeded library finish opening before starting the sign-in, so this test asserts
    // the failed-save behavior rather than racing the startup cache load.
    h.until("library open", |s| {
        s.library
            .as_ref()
            .is_some_and(|view| view.rows.iter().any(|row| row.key == "steam:620"))
    })
    .await;

    h.engine.steam_sign_in().await;
    let snapshot = match tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let snapshot = h.snapshots.borrow_and_update().clone();
            if snapshot.store_accounts.first().is_some_and(|card| {
                card.state == StoreAccountState::Connected
                    && card.persist_failed
                    && card.owned_count == 1
            }) {
                return snapshot;
            }
            if h.snapshots.changed().await.is_err() {
                panic!("the engine stopped publishing");
            }
        }
    })
    .await
    {
        Ok(snapshot) => snapshot,
        Err(_) => panic!(
            "timed out; session_calls={} owned_calls={} family_calls={}",
            fixture.client.session_calls.load(Ordering::SeqCst),
            fixture.client.owned_calls.load(Ordering::SeqCst),
            fixture.client.family_calls.load(Ordering::SeqCst)
        ),
    };
    assert_eq!(fixture.credentials.stored(), None);
    assert_eq!(card(&snapshot).owned_count, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_different_account_clears_the_old_catalog_before_importing_the_new_one() {
    let cookie = "steamLoginSecure=new-cookie";
    let fixture = SteamFixture::new().with_stored(cookie, Some(stored_row("A", "old")));
    fixture.client.set_session(Ok(session("B", "new", cookie)));
    fixture
        .client
        .set_owned(Ok(vec![game("730", "Counter-Strike 2")]));
    let mut h = steam_harness(&fixture);
    seed(
        &h,
        vec![
            entry("steam:400", "Portals", false, true, false),
            entry("steam:620", "Portal 2", true, false, false),
        ],
        Some(account("u1", "Ana")),
    );

    h.engine.start();
    let snapshot = h
        .until("the new account's catalog", |s| {
            s.library.as_ref().is_some_and(|view| {
                view.rows
                    .iter()
                    .any(|row| row.key == "steam:730" && row.owned)
                    && !view.rows.iter().any(|row| row.key == "steam:400")
            })
        })
        .await;
    let view = snapshot.library.as_ref().unwrap();
    assert!(
        !view.rows.iter().any(|row| row.key == "steam:400"),
        "the previous account's owned game is gone"
    );
    assert!(
        view.rows
            .iter()
            .any(|row| row.key == "steam:730" && row.owned)
    );
    assert!(
        view.rows
            .iter()
            .any(|row| row.key == "steam:620" && row.installed),
        "installed games are kept"
    );
    let store = h.store.clone();
    eventually("the clear is persisted", || {
        !store.cleared.lock().unwrap().is_empty()
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn identity_change_before_library_activation_survives_the_old_cache_load() {
    let cookie = "steamLoginSecure=new-cookie";
    let fixture = SteamFixture::new().with_stored(cookie, Some(stored_row("A", "old")));
    fixture.client.set_session(Ok(session("B", "new", cookie)));
    fixture
        .client
        .set_owned(Ok(vec![game("730", "Counter-Strike 2")]));
    let mut h = steam_harness(&fixture);
    seed(
        &h,
        vec![entry("steam:400", "Portals", false, true, false)],
        Some(account("u1", "Ana")),
    );
    // Force the startup check to commit before the library starts loading.
    // The cache still contains the previous account's ownership facts.
    h.engine.provider_check("steam").await;
    h.engine.start();
    h.until(
        "new ownership after a pre-activation identity change",
        |s| {
            s.library.as_ref().is_some_and(|view| {
                view.rows.iter().any(|r| r.key == "steam:730" && r.owned)
                    && !view.rows.iter().any(|r| r.key == "steam:400")
            })
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_changed_account_whose_catalog_fails_does_not_keep_the_old_owned_games() {
    let cookie = "steamLoginSecure=new-cookie";
    let fixture = SteamFixture::new().with_stored(cookie, Some(stored_row("A", "old")));
    fixture.client.set_session(Ok(session("B", "new", cookie)));
    fixture.client.set_owned(Err(SteamError::Privacy));
    let mut h = steam_harness(&fixture);
    seed(
        &h,
        vec![
            entry("steam:400", "Portals", false, true, false),
            entry("steam:620", "Portal 2", true, false, false),
        ],
        Some(account("u1", "Ana")),
    );

    h.engine.start();
    let snapshot = h
        .until("the failed new catalog", |s| {
            s.store_accounts.first().is_some_and(|card| {
                card.state == StoreAccountState::Connected
                    && card.catalog_error == Some(gamenode_core::library::CatalogError::Privacy)
            }) && s
                .library
                .as_ref()
                .is_some_and(|view| !view.rows.iter().any(|row| row.key == "steam:400"))
        })
        .await;
    assert!(
        no_owned_rows(&snapshot),
        "the previous account's games never reappear as owned"
    );
    assert!(
        snapshot
            .library
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .any(|row| row.key == "steam:620" && row.installed)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn signing_out_clears_the_catalog_and_keeps_installed_games() {
    let fixture = SteamFixture::new().with_stored(LOGIN_COOKIE, Some(stored_row("A", "cassi")));
    fixture
        .client
        .set_session(Ok(session("A", "cassi", LOGIN_COOKIE)));
    fixture
        .client
        .set_owned(Ok(vec![game("400", "Portals"), game("620", "Portal 2")]));
    let mut h = steam_harness(&fixture);
    seed(
        &h,
        vec![entry("steam:620", "Portal 2", true, false, false)],
        Some(account("u1", "Ana")),
    );
    h.engine.start();
    h.until("connected with the catalog", |s| {
        s.store_accounts
            .first()
            .is_some_and(|card| card.state == StoreAccountState::Connected && card.owned_count == 2)
    })
    .await;

    h.engine.steam_sign_out().await;
    let snapshot = h
        .until("signed out", |s| {
            state(s) == StoreAccountState::NotConnected
        })
        .await;
    assert_eq!(fixture.credentials.stored(), None);
    assert_eq!(fixture.credentials.deletes.load(Ordering::SeqCst), 1);
    assert!(fixture.accounts.account.lock().unwrap().is_none());
    let view = snapshot.library.as_ref().unwrap();
    assert!(!view.rows.iter().any(|row| row.key == "steam:400"));
    assert!(
        view.rows
            .iter()
            .any(|row| row.key == "steam:620" && row.installed)
    );
    let store = h.store.clone();
    eventually("the clear is persisted", || {
        store.cleared.lock().unwrap().iter().any(|p| p == "steam")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_catalog_result_superseded_by_sign_out_is_ignored() {
    let fixture = SteamFixture::new().with_stored(LOGIN_COOKIE, Some(stored_row("A", "cassi")));
    fixture
        .client
        .set_session(Ok(session("A", "cassi", LOGIN_COOKIE)));
    fixture.client.set_owned(Ok(vec![game("400", "Portals")]));
    let mut h = steam_harness(&fixture);
    seed(
        &h,
        vec![entry("steam:620", "Portal 2", true, false, false)],
        Some(account("u1", "Ana")),
    );
    h.engine.start();
    h.until("connected with the catalog", |s| {
        s.store_accounts
            .first()
            .is_some_and(|card| card.state == StoreAccountState::Connected && card.owned_count == 1)
    })
    .await;

    let gate = std::sync::Arc::new(Semaphore::new(0));
    fixture.client.hold_until(gate.clone());
    h.engine.steam_check().await;
    fixture.client.wait_started().await;
    h.engine.steam_sign_out().await;
    gate.add_permits(10);

    let snapshot = h
        .until("signed out", |s| {
            state(s) == StoreAccountState::NotConnected
        })
        .await;
    assert!(no_owned_rows(&snapshot));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        no_owned_rows(&h.engine.snapshot()),
        "a late result from the superseded attempt must not restore the catalog"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn repeated_catalog_requests_coalesce_into_one_more_sync() {
    let fixture = SteamFixture::new().with_stored(LOGIN_COOKIE, Some(stored_row("A", "cassi")));
    fixture
        .client
        .set_session(Ok(session("A", "cassi", LOGIN_COOKIE)));
    fixture.client.set_owned(Ok(vec![game("400", "Portals")]));
    let mut h = steam_harness(&fixture);
    seed(&h, vec![], Some(account("u1", "Ana")));
    h.engine.start();
    h.until("connected with the catalog", |s| {
        s.store_accounts
            .first()
            .is_some_and(|card| card.state == StoreAccountState::Connected && card.owned_count == 1)
    })
    .await;
    let baseline = fixture.client.owned_calls.load(Ordering::SeqCst);

    let gate = std::sync::Arc::new(Semaphore::new(0));
    fixture.client.hold_until(gate.clone());
    // Start one sync and hold it, then ask five times while it runs.
    h.engine
        .library_input(gamenode_core::library::Input::RequestCatalog);
    fixture.client.wait_started().await;
    for _ in 0..5 {
        h.engine
            .library_input(gamenode_core::library::Input::RequestCatalog);
    }
    gate.add_permits(10);

    eventually("the coalesced sync finishes", || {
        fixture.client.owned_calls.load(Ordering::SeqCst) > baseline + 1
    })
    .await;
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(
        fixture.client.owned_calls.load(Ordering::SeqCst),
        baseline + 2,
        "the running sync plus exactly one more for the five requests made meanwhile"
    );
    assert_eq!(
        h.engine.snapshot().store_accounts[0].owned_count,
        1,
        "the catalog stays applied"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_card_works_without_a_gamenode_session_and_never_syncs_a_catalog() {
    let fixture = SteamFixture::new().with_stored(LOGIN_COOKIE, Some(stored_row("A", "cassi")));
    fixture
        .client
        .set_session(Ok(session("A", "cassi", LOGIN_COOKIE)));
    let mut h = steam_harness_signed_out(&fixture);
    h.engine.start();
    let snapshot = h
        .until("connected without a library", |s| {
            state(s) == StoreAccountState::Connected && s.library.is_none()
        })
        .await;

    let card = card(&snapshot);
    assert_eq!(card.account_name.as_deref(), Some("cassi"));
    assert_eq!(card.owned_count, 0);
    assert_eq!(card.installed_count, 0);
    assert_eq!(
        fixture.client.owned_calls.load(Ordering::SeqCst),
        0,
        "no library means no catalog request"
    );
    assert_no_secret(&snapshot);
}

#[tokio::test(flavor = "multi_thread")]
async fn every_account_status_reaches_the_snapshot_without_a_secret() {
    let mut gates: Vec<std::sync::Arc<Semaphore>> = Vec::new();

    // Not connected: no credential.
    {
        let fixture = SteamFixture::new();
        let mut h = steam_harness_signed_out(&fixture);
        h.engine.start();
        let snapshot = h
            .until("not connected", |s| {
                state(s) == StoreAccountState::NotConnected
            })
            .await;
        assert_no_secret(&snapshot);
    }

    // Checking: the stored session is being validated.
    {
        let fixture = SteamFixture::new().with_stored(LOGIN_COOKIE, None);
        fixture
            .client
            .set_session(Ok(session("A", "cassi", LOGIN_COOKIE)));
        let gate = std::sync::Arc::new(Semaphore::new(0));
        fixture.client.hold_until(gate.clone());
        gates.push(gate);
        let mut h = steam_harness_signed_out(&fixture);
        h.engine.start();
        let snapshot = h
            .until("checking", |s| state(s) == StoreAccountState::Checking)
            .await;
        assert_no_secret(&snapshot);
    }

    // Signing in: the login window is open.
    {
        let fixture = SteamFixture::new();
        let (_sender, receiver) = oneshot::channel();
        fixture.login.set_pending(receiver);
        let mut h = steam_harness_signed_out(&fixture);
        h.engine.start();
        h.until("not connected", |s| {
            state(s) == StoreAccountState::NotConnected
        })
        .await;
        let engine = h.engine.clone();
        let attempt = tokio::spawn(async move { engine.steam_sign_in().await });
        let snapshot = h
            .until("signing in", |s| state(s) == StoreAccountState::SigningIn)
            .await;
        assert_no_secret(&snapshot);
        h.engine.steam_cancel_sign_in().await;
        let _ = attempt.await;
    }

    // Connected: the card renders without a GameNode session.
    {
        let fixture = SteamFixture::new().with_stored(LOGIN_COOKIE, None);
        fixture
            .client
            .set_session(Ok(session("A", "cassi", LOGIN_COOKIE)));
        let mut h = steam_harness_signed_out(&fixture);
        h.engine.start();
        let snapshot = h
            .until("connected", |s| state(s) == StoreAccountState::Connected)
            .await;
        assert_no_secret(&snapshot);
    }

    // Expired.
    {
        let fixture = SteamFixture::new().with_stored(LOGIN_COOKIE, None);
        fixture.client.set_session(Err(SteamError::Expired));
        let mut h = steam_harness_signed_out(&fixture);
        h.engine.start();
        let snapshot = h
            .until("expired", |s| state(s) == StoreAccountState::Expired)
            .await;
        assert_no_secret(&snapshot);
    }

    // Unreachable.
    {
        let fixture = SteamFixture::new().with_stored(LOGIN_COOKIE, None);
        fixture.client.set_session(Err(SteamError::Unreachable));
        let mut h = steam_harness_signed_out(&fixture);
        h.engine.start();
        let snapshot = h
            .until("unreachable", |s| {
                state(s) == StoreAccountState::Unreachable
            })
            .await;
        assert_no_secret(&snapshot);
    }

    // Credential error: the keyring could not be read.
    {
        let fixture = SteamFixture::new().with_stored(LOGIN_COOKIE, None);
        fixture.credentials.fail_load.store(true, Ordering::SeqCst);
        let mut h = steam_harness_signed_out(&fixture);
        h.engine.start();
        let snapshot = h
            .until("credential error", |s| {
                state(s) == StoreAccountState::CredentialError
            })
            .await;
        let card = card(&snapshot);
        assert!(card.can_sign_in && card.can_check && card.can_remove_credential);
        assert_no_secret(&snapshot);
    }

    for gate in gates {
        gate.add_permits(100);
    }
}
