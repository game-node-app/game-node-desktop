//! Engine tests for the game library: scanning, lookups, covers, Play and the account rules.
//!
//! Every adapter is a double with a switchable delay, so the tests can show that slow scans,
//! requests and storage never hold up the caller.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use gamenode_core::game_api::{ApiError, ExternalMatch, GameSummary};
use gamenode_core::library::{
    AutoMatch, CatalogUpdate, ChosenMatch, FavoriteRecord, GameRecord, LastAccount, LaunchError,
    LaunchKind, LaunchState, LibraryCache, LibraryLayout, LibraryStore, LibraryView, MatchStatus,
    ProviderScanState, StoreAccount, StoredEntry,
};
use gamenode_core::ports::{HostFiles, StorageError, UriError, UriOpener};
use gamenode_core::provider::Record;
use gamenode_core::provider::memory::MemoryFiles;
use gamenode_core::{AuthView, Input, Notice, SessionError, Snapshot};
use url::Url;

use super::{FakeApi, FakeCreds, Harness, ana, harness, harness_over};

const STEAM: &str = "/home/me/.steam/steam";
const LIBRARY_FOLDERS: &str =
    "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"/home/me/.steam/steam\"\n\t}\n}\n";
const COVER: &str = "//images.igdb.com/igdb/image/upload/t_thumb/co1abc.jpg";

/// Holds an adapter call until the test lets it through, so a test can prove that the caller
/// does not wait for it without depending on how long anything takes.
#[derive(Default)]
pub(super) struct Gate(AtomicBool);

impl Gate {
    pub(super) fn close(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub(super) fn open(&self) {
        self.0.store(false, Ordering::SeqCst);
    }
    fn pass(&self) {
        while self.0.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// A file system whose reads wait at a gate, so a scan can be held in progress.
pub(super) struct SlowFiles {
    pub(super) inner: Mutex<MemoryFiles>,
    pub(super) gate: Gate,
    pub(super) library_reads: AtomicUsize,
}

impl SlowFiles {
    /// Steam with one installed game: Portal 2 (`620`).
    pub(super) fn steam_with_portal_2() -> Self {
        let files = MemoryFiles::new()
            .with_file(
                &format!("{STEAM}/steamapps/libraryfolders.vdf"),
                LIBRARY_FOLDERS,
            )
            .with_file(
                &format!("{STEAM}/steamapps/appmanifest_620.acf"),
                "\"AppState\"\n{\n\t\"appid\"\t\t\"620\"\n\t\"name\"\t\t\"Portal 2\"\n\t\"StateFlags\"\t\t\"4\"\n\t\"installdir\"\t\t\"Portal 2\"\n}\n",
            )
            .with_dir(&format!("{STEAM}/steamapps/common/Portal 2"));
        Self {
            inner: Mutex::new(files),
            gate: Gate::default(),
            library_reads: AtomicUsize::new(0),
        }
    }

    fn pause(&self) {
        self.gate.pass();
    }
}

impl HostFiles for SlowFiles {
    fn read_to_string(&self, path: &str) -> std::io::Result<String> {
        self.pause();
        if path.ends_with("libraryfolders.vdf") {
            self.library_reads.fetch_add(1, Ordering::SeqCst);
        }
        self.inner.lock().unwrap().read_to_string(path)
    }
    fn read_dir(&self, path: &str) -> std::io::Result<Vec<String>> {
        self.pause();
        self.inner.lock().unwrap().read_dir(path)
    }
    fn is_dir(&self, path: &str) -> bool {
        self.inner.lock().unwrap().is_dir(path)
    }
    fn canonicalize(&self, path: &str) -> std::io::Result<String> {
        self.inner.lock().unwrap().canonicalize(path)
    }
}

#[derive(Default)]
pub(super) struct FakeUri {
    pub(super) opened: Mutex<Vec<String>>,
    pub(super) fail: Mutex<Option<UriError>>,
    pub(super) gate: Gate,
}

impl UriOpener for FakeUri {
    fn open(&self, uri: &Url) -> Result<(), UriError> {
        self.gate.pass();
        if let Some(error) = *self.fail.lock().unwrap() {
            return Err(error);
        }
        self.opened.lock().unwrap().push(uri.to_string());
        Ok(())
    }
}

/// An in-memory library database that remembers every write.
#[derive(Default)]
pub(super) struct FakeLibrary {
    pub(super) cache: Mutex<LibraryCache>,
    pub(super) gate: Gate,
    pub(super) loaded_for: Mutex<Vec<Option<String>>>,
    pub(super) scans: Mutex<Vec<(String, Vec<Record>)>>,
    pub(super) matches: Mutex<Vec<AutoMatch>>,
    pub(super) games: Mutex<Vec<GameRecord>>,
    pub(super) covers_ready: Mutex<Vec<(u64, String)>>,
    pub(super) account: Mutex<Option<LastAccount>>,
    pub(super) excluded: Mutex<Vec<String>>,
    pub(super) catalogs: Mutex<Vec<(String, CatalogUpdate)>>,
    pub(super) cleared: Mutex<Vec<String>>,
    pub(super) favorites: Mutex<Vec<FavoriteRecord>>,
    pub(super) store_accounts: Mutex<Vec<StoreAccount>>,
}

impl FakeLibrary {
    fn pause(&self) {
        self.gate.pass();
    }
}

impl LibraryStore for FakeLibrary {
    fn load_cache(&self, account_id: Option<&str>) -> Result<LibraryCache, StorageError> {
        self.pause();
        self.loaded_for
            .lock()
            .unwrap()
            .push(account_id.map(str::to_owned));
        Ok(self.cache.lock().unwrap().clone())
    }
    fn save_scan(&self, provider: &str, records: &[Record], _now: i64) -> Result<(), StorageError> {
        self.pause();
        self.scans
            .lock()
            .unwrap()
            .push((provider.to_owned(), records.to_vec()));
        Ok(())
    }
    fn save_scan_failure(&self, _provider: &str, _error: &str) -> Result<(), StorageError> {
        Ok(())
    }
    fn save_catalog(&self, provider: &str, update: &CatalogUpdate) -> Result<(), StorageError> {
        self.pause();
        self.catalogs
            .lock()
            .unwrap()
            .push((provider.to_owned(), update.clone()));
        Ok(())
    }
    fn clear_catalog(&self, provider: &str) -> Result<(), StorageError> {
        self.pause();
        self.cleared.lock().unwrap().push(provider.to_owned());
        Ok(())
    }
    fn load_store_accounts(&self) -> Result<Vec<StoreAccount>, StorageError> {
        Ok(self.store_accounts.lock().unwrap().clone())
    }
    fn save_store_account(&self, account: &StoreAccount) -> Result<(), StorageError> {
        let mut accounts = self.store_accounts.lock().unwrap();
        accounts.retain(|existing| existing.provider != account.provider);
        accounts.push(account.clone());
        Ok(())
    }
    fn clear_store_account(&self, provider: &str) -> Result<(), StorageError> {
        self.store_accounts
            .lock()
            .unwrap()
            .retain(|account| account.provider != provider);
        Ok(())
    }
    fn save_auto_matches(&self, matches: &[AutoMatch]) -> Result<(), StorageError> {
        self.pause();
        self.matches.lock().unwrap().extend_from_slice(matches);
        Ok(())
    }
    fn save_chosen_match(&self, _account: &str, _choice: &ChosenMatch) -> Result<(), StorageError> {
        Ok(())
    }
    fn delete_chosen_match(&self, _account: &str, _entry_key: &str) -> Result<(), StorageError> {
        Ok(())
    }
    fn save_games(&self, games: &[GameRecord]) -> Result<(), StorageError> {
        self.games.lock().unwrap().extend_from_slice(games);
        Ok(())
    }
    fn save_cover_ready(&self, game_id: u64, file: &str) -> Result<(), StorageError> {
        self.covers_ready
            .lock()
            .unwrap()
            .push((game_id, file.to_owned()));
        Ok(())
    }
    fn save_cover_failed(&self, _game_id: u64, _at: i64) -> Result<(), StorageError> {
        Ok(())
    }
    fn save_favorite(&self, favorite: &FavoriteRecord) -> Result<(), StorageError> {
        self.pause();
        let mut saved = self.favorites.lock().unwrap();
        saved.retain(|record| {
            !(record.account_id == favorite.account_id && record.game_id == favorite.game_id)
        });
        if favorite.is_favorite || favorite.pending {
            saved.push(favorite.clone());
        }
        Ok(())
    }
    fn load_layout(&self) -> Result<LibraryLayout, StorageError> {
        Ok(LibraryLayout::default())
    }
    fn save_layout(&self, _layout: &LibraryLayout) -> Result<(), StorageError> {
        Ok(())
    }
    fn save_excluded(&self, excluded: &[String]) -> Result<(), StorageError> {
        self.pause();
        *self.excluded.lock().unwrap() = excluded.to_vec();
        self.cache.lock().unwrap().excluded = excluded.to_vec();
        Ok(())
    }
    fn load_last_account(&self) -> Result<Option<LastAccount>, StorageError> {
        Ok(self.account.lock().unwrap().clone())
    }
    fn save_last_account(&self, account: &LastAccount) -> Result<(), StorageError> {
        self.pause();
        *self.account.lock().unwrap() = Some(account.clone());
        Ok(())
    }
    fn clear_last_account(&self) -> Result<(), StorageError> {
        self.pause();
        self.account.lock().unwrap().take();
        Ok(())
    }
}

/// Waits for something that is not part of the snapshot (a write that reached an adapter).
async fn eventually(what: &str, mut condition: impl FnMut() -> bool) {
    let waited = tokio::time::timeout(Duration::from_secs(5), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await;
    assert!(waited.is_ok(), "timed out waiting for: {what}");
}

fn view(snapshot: &Snapshot) -> Option<&LibraryView> {
    snapshot.library.as_ref()
}

fn account(id: &str, name: &str) -> LastAccount {
    LastAccount {
        id: id.to_owned(),
        name: name.to_owned(),
    }
}

fn cached_portal_2() -> StoredEntry {
    StoredEntry {
        key: "steam:620".to_owned(),
        provider: "steam".to_owned(),
        store_id: "620".to_owned(),
        name: Some("Portal 2".to_owned()),
        install_dir: "Portal 2".to_owned(),
        fields: Default::default(),
        installed: true,
        owned: false,
        family_shared: false,
        first_seen: 1,
        last_seen: 1,
    }
}

fn seed_cache(h: &Harness, account: Option<LastAccount>) {
    *h.store.account.lock().unwrap() = account;
    *h.store.cache.lock().unwrap() = LibraryCache {
        entries: vec![cached_portal_2()],
        scans: vec![ProviderScanState {
            provider: "steam".to_owned(),
            last_success_at: Some(1),
            last_error: None,
            ..ProviderScanState::default()
        }],
        ..LibraryCache::default()
    };
}

fn portal_2_match() -> ExternalMatch {
    ExternalMatch {
        uid: "620".to_owned(),
        game: GameSummary {
            id: 7,
            name: "Portal 2".to_owned(),
            release_year: Some(2011),
            cover_url: Some(COVER.to_owned()),
        },
    }
}

fn lookup_count(h: &Harness) -> usize {
    h.api.lookups.lock().unwrap().len()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_running_scan_never_blocks_the_caller_and_repeated_requests_coalesce() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    // Every file read now waits, so the first scan cannot finish until the gate opens.
    h.files.gate.close();
    h.sign_in("token").await;
    h.until("the first scan runs", |s| {
        view(s).is_some_and(|v| v.scanning)
    })
    .await;

    // These all return although the scan is stuck: nothing here waits for it.
    for _ in 0..5 {
        h.engine.rescan();
        let _ = h.engine.snapshot();
    }
    assert!(h.engine.snapshot().library.unwrap().scanning);

    h.files.gate.open();
    let store = h.store.clone();
    eventually("the follow-up scan is saved", || {
        store.scans.lock().unwrap().len() == 2
    })
    .await;
    h.until("the scan is over", |s| view(s).is_some_and(|v| !v.scanning))
        .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        h.store.scans.lock().unwrap().len(),
        2,
        "one scan, then one more for all the requests made meanwhile"
    );
    assert_eq!(h.files.library_reads.load(Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn stuck_storage_does_not_block_the_caller() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("token").await;
    h.until("listed", |s| view(s).is_some_and(|v| v.rows.len() == 1))
        .await;

    h.store.gate.close();
    h.engine.rescan();
    for _ in 0..10 {
        h.engine
            .library_input(gamenode_core::library::Input::SetSearch("p".into()));
        let _ = h.engine.snapshot();
    }
    assert_eq!(h.engine.snapshot().library.unwrap().search, "p");

    // Once storage recovers, the queued work completes in order.
    h.store.gate.open();
    let store = h.store.clone();
    eventually("the queued scan is saved", || {
        store.scans.lock().unwrap().len() >= 2
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_scan_is_saved_and_the_signed_in_account_is_remembered() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("token").await;
    let store = h.store.clone();
    let snapshot = h
        .until("the scan is listed", |s| {
            view(s).is_some_and(|v| v.rows.len() == 1 && !v.scanning)
        })
        .await;
    assert_eq!(view(&snapshot).unwrap().rows[0].key, "steam:620");
    eventually("saved", || !store.scans.lock().unwrap().is_empty()).await;
    let scans = h.store.scans.lock().unwrap().clone();
    assert_eq!(scans[0].0, "steam");
    assert_eq!(scans[0].1[0].id, "620");
    assert_eq!(*h.store.account.lock().unwrap(), Some(account("u1", "Ana")));
}

#[tokio::test(flavor = "multi_thread")]
async fn verified_lookups_match_games_and_download_covers() {
    let api = FakeApi::new();
    *api.lookup_result.lock().unwrap() = Some(Ok(vec![portal_2_match()]));
    let mut h = harness(api, FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("token").await;

    let snapshot = h
        .until("matched with a cover", |s| {
            view(s).is_some_and(|v| {
                v.rows.first().is_some_and(|row| {
                    matches!(row.status, MatchStatus::Matched { game_id: 7, .. })
                        && row.cover_file.as_deref() == Some("7.jpg")
                })
            })
        })
        .await;
    assert_eq!(view(&snapshot).unwrap().rows[0].title, "Portal 2");
    assert_eq!(
        *h.api.lookups.lock().unwrap(),
        [(1, vec!["620".to_owned()])]
    );
    assert!(h.covers.path().join("7.jpg").exists());
    eventually("cover persisted", || {
        !h.store.covers_ready.lock().unwrap().is_empty()
    })
    .await;
    assert_eq!(
        *h.store.covers_ready.lock().unwrap(),
        [(7, "7.jpg".to_owned())]
    );
    assert_eq!(h.store.matches.lock().unwrap()[0].game_ids, [7]);
    assert_eq!(h.store.games.lock().unwrap()[0].name, "Portal 2");
}

#[tokio::test(flavor = "multi_thread")]
async fn play_and_install_open_the_providers_links_without_blocking() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("token").await;
    h.until("listed", |s| view(s).is_some_and(|v| v.rows.len() == 1))
        .await;

    // The link is held at the operating system: Play returns and shows "opening".
    h.uri.gate.close();
    h.engine
        .open_entry("steam:620".to_owned(), LaunchKind::Play);
    // Repeated activation while the OS opener is still working must not enqueue a second
    // launch for the same action.
    h.engine
        .open_entry("steam:620".to_owned(), LaunchKind::Play);
    assert_eq!(
        h.engine.snapshot().launch.unwrap().state,
        LaunchState::Opening
    );
    h.uri.gate.open();
    h.until("cleared once the OS accepted the link", |s| {
        s.launch.is_none()
    })
    .await;

    h.engine
        .open_entry("steam:620".to_owned(), LaunchKind::Install);
    h.until("done", |s| s.launch.is_none()).await;
    assert_eq!(
        *h.uri.opened.lock().unwrap(),
        ["steam://rungameid/620", "steam://nav/games/details/620"]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_store_client_is_reported_until_dismissed() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("token").await;
    h.until("listed", |s| view(s).is_some_and(|v| v.rows.len() == 1))
        .await;
    *h.uri.fail.lock().unwrap() = Some(UriError::NoHandler);

    h.engine
        .open_entry("steam:620".to_owned(), LaunchKind::Play);
    let snapshot = h
        .until("failure shown", |s| {
            s.launch
                .as_ref()
                .is_some_and(|l| matches!(l.state, LaunchState::Failed(_)))
        })
        .await;
    assert_eq!(
        snapshot.launch.unwrap().state,
        LaunchState::Failed(LaunchError::NoHandler)
    );
    h.engine.dismiss_launch();
    h.until("dismissed", |s| s.launch.is_none()).await;

    // An entry that is not in the library is ignored.
    h.engine.open_entry("steam:1".to_owned(), LaunchKind::Play);
    assert!(h.engine.snapshot().launch.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn restart_signed_in_shows_the_cached_library_before_the_session_is_verified() {
    let api = FakeApi::new();
    *api.delay.lock().unwrap() = Duration::from_millis(300);
    let mut h = harness(api, FakeCreds::with(Some("stored")));
    seed_cache(&h, Some(account("u1", "Ana")));
    h.engine.start();

    let snapshot = h
        .until("cached library while verifying", |s| {
            s.auth == AuthView::Verifying && view(s).is_some_and(|v| v.rows.len() == 1)
        })
        .await;
    assert_eq!(
        view(&snapshot).unwrap().access,
        gamenode_core::library::Access::Unverified
    );
    // Nothing is sent to GameNode, and local actions already work.
    assert_eq!(lookup_count(&h), 0);
    assert!(
        view(&snapshot).unwrap().actions.play.is_available()
            || view(&snapshot).unwrap().selected.is_empty()
    );
    h.engine
        .open_entry("steam:620".to_owned(), LaunchKind::Play);
    h.until("played", |s| s.launch.is_none()).await;
    assert_eq!(*h.uri.opened.lock().unwrap(), ["steam://rungameid/620"]);

    h.until("verified", |s| {
        matches!(s.auth, AuthView::SignedIn { .. })
            && view(s).is_some_and(|v| v.access == gamenode_core::library::Access::Verified)
    })
    .await;
    // The same account was verified: the cache stays and lookups begin.
    assert_eq!(*h.store.loaded_for.lock().unwrap(), [Some("u1".to_owned())]);
    let api = h.api.clone();
    eventually("lookups begin", || !api.lookups.lock().unwrap().is_empty()).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rejected_restore_hides_the_cached_library_and_forgets_the_account() {
    let api = FakeApi::new();
    *api.session.lock().unwrap() = Some(Err(SessionError::Rejected));
    let mut h = harness(api, FakeCreds::with(Some("stored")));
    seed_cache(&h, Some(account("u1", "Ana")));
    h.engine.start();

    h.until("signed out", |s| s.auth == AuthView::SignedOut)
        .await;
    let snapshot = h.engine.snapshot();
    assert!(snapshot.library.is_none());
    let store = h.store.clone();
    eventually("account forgotten", || {
        store.account.lock().unwrap().is_none()
    })
    .await;
    assert_eq!(lookup_count(&h), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_network_failure_keeps_the_cache_with_network_actions_disabled_until_retry() {
    let api = FakeApi::new();
    *api.session.lock().unwrap() = Some(Err(SessionError::Unreachable));
    let mut h = harness(api, FakeCreds::with(Some("stored")));
    seed_cache(&h, Some(account("u1", "Ana")));
    h.engine.start();

    let snapshot = h
        .until("restore failed with the cache shown", |s| {
            s.auth == AuthView::RestoreFailed && view(s).is_some_and(|v| v.rows.len() == 1)
        })
        .await;
    let library = view(&snapshot).unwrap();
    assert_eq!(library.access, gamenode_core::library::Access::Unverified);
    assert!(!library.actions.add_to_gamenode.is_available());
    assert_eq!(lookup_count(&h), 0);
    h.engine
        .open_entry("steam:620".to_owned(), LaunchKind::Play);
    h.until("local play works", |s| s.launch.is_none()).await;
    assert_eq!(h.uri.opened.lock().unwrap().len(), 1);

    *h.api.session.lock().unwrap() = Some(Ok(ana()));
    h.engine.dispatch(Input::RetryRestore);
    let snapshot = h
        .until("verified after retry", |s| {
            matches!(s.auth, AuthView::SignedIn { .. })
                && view(s).is_some_and(|v| v.access == gamenode_core::library::Access::Verified)
        })
        .await;
    assert_eq!(view(&snapshot).unwrap().rows.len(), 1);
    let api = h.api.clone();
    eventually("lookups begin", || !api.lookups.lock().unwrap().is_empty()).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn verifying_a_different_account_replaces_the_library() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(Some("stored")));
    seed_cache(&h, Some(account("u2", "Bo")));
    h.engine.start();

    let store = h.store.clone();
    h.until("the new account is remembered", |s| {
        matches!(s.auth, AuthView::SignedIn { .. })
            && store
                .account
                .lock()
                .unwrap()
                .as_ref()
                .map(|a| a.id.as_str())
                == Some("u1")
    })
    .await;
    // First the previous account's cache, then the verified account's.
    let store = h.store.clone();
    eventually("both caches were read", || {
        store.loaded_for.lock().unwrap().len() == 2
    })
    .await;
    assert_eq!(
        *h.store.loaded_for.lock().unwrap(),
        [Some("u2".to_owned()), Some("u1".to_owned())]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn signing_out_removes_the_library_and_the_account_record() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("token").await;
    h.until("listed", |s| view(s).is_some_and(|v| v.rows.len() == 1))
        .await;
    let store = h.store.clone();
    eventually("account stored", || store.account.lock().unwrap().is_some()).await;

    h.engine.dispatch(Input::SignOut);
    h.until("signed out", |s| s.auth == AuthView::SignedOut)
        .await;
    assert!(h.engine.snapshot().library.is_none());
    let store = h.store.clone();
    eventually("account forgotten", || {
        store.account.lock().unwrap().is_none()
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_401_from_a_library_request_signs_the_user_out() {
    let api = FakeApi::new();
    *api.lookup_result.lock().unwrap() = Some(Err(ApiError::Rejected));
    let mut h = harness(api, FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("token").await;

    let snapshot = h
        .until("signed out by the rejection", |s| {
            s.auth == AuthView::SignedOut && s.notice == Some(Notice::SessionRejected)
        })
        .await;
    assert!(snapshot.library.is_none());
    let creds = h.creds.clone();
    eventually("the credential is deleted", || creds.stored().is_none()).await;
}

/// Scanning, matching, launching and signing in never create or change a GameNode entry: the
/// server below fails any request to the create endpoint, and the test proves none arrived.
#[tokio::test(flavor = "multi_thread")]
async fn scanning_matching_launching_and_signing_in_never_create_entries() {
    use httpmock::prelude::*;
    use serde_json::json;

    let server = MockServer::start_async().await;
    server.mock(|when, then| {
        when.method(GET).path("/v1/auth/get-session");
        then.status(200).json_body(json!({
            "session": { "id": "s1", "token": "ignored" },
            "user": { "id": "u1", "name": "Ana", "email": null, "image": null, "role": "user" }
        }));
    });
    let lookup = server.mock(|when, then| {
        when.method(GET).path("/v1/game/external");
        then.status(200).json_body(json!({
            "data": [{
                "id": 70, "uid": "620", "category": 1, "name": "Portal 2", "year": 2011,
                "gameId": 7,
                "game": { "id": 7, "name": "Portal 2", "firstReleaseDate": "2011-04-18T00:00:00.000Z" }
            }],
            "pagination": { "totalItems": 1, "totalPages": 1, "hasNextPage": false }
        }));
    });
    let creates = server.mock(|when, then| {
        when.any_request().path_includes("collections-entries");
        then.status(500);
    });
    let remote = std::sync::Arc::new(
        gamenode_api::GameNodeClient::with_timeout(
            Url::parse(&server.base_url()).unwrap(),
            Duration::from_secs(2),
        )
        .unwrap(),
    );

    // The stored credential is restored and verified, which starts the scan and the lookup.
    let mut h = harness_over(FakeApi::new(), FakeCreds::with(Some("token")), Some(remote));
    h.engine.start();
    h.until("matched", |s| {
        view(s).is_some_and(|v| {
            v.rows.len() == 1 && matches!(v.rows[0].status, MatchStatus::Matched { game_id: 7, .. })
        })
    })
    .await;
    lookup.assert_calls_async(1).await;

    h.engine.rescan();
    h.engine
        .open_entry("steam:620".to_owned(), LaunchKind::Play);
    h.until("launched", |s| s.launch.is_none()).await;
    let store = h.store.clone();
    eventually("the rescan is saved", || {
        store.scans.lock().unwrap().len() >= 2
    })
    .await;

    creates.assert_calls_async(0).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn editing_a_store_game_uses_update_without_importing_the_account_library() {
    use gamenode_core::game_api::LibraryEntry;
    use gamenode_core::library::{EntryStatus, Input as LibInput, SelectMode};
    let api = FakeApi::new();
    *api.lookup_result.lock().unwrap() = Some(Ok(vec![portal_2_match()]));
    *api.entry.lock().unwrap() = Some(Ok(Some(LibraryEntry {
        status: Some(EntryStatus::Finished),
        platform_ids: vec![6],
        ..Default::default()
    })));
    let mut h = harness(api, FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("token").await;
    h.until("matched", |s| {
        view(s).is_some_and(|v| {
            v.rows
                .first()
                .is_some_and(|r| matches!(r.status, MatchStatus::Matched { .. }))
        })
    })
    .await;
    h.engine.library_input(LibInput::Select {
        key: "steam:620".into(),
        mode: SelectMode::Replace,
    });
    h.until("membership", |s| {
        view(s).is_some_and(|v| v.actions.add_to_gamenode.is_available())
    })
    .await;
    h.engine.library_input(LibInput::OpenAdd);
    h.until("ready to edit", |s| {
        view(s).is_some_and(|v| {
            v.add_dialog
                .as_ref()
                .is_some_and(|d| d.editing_existing && d.can_confirm)
        })
    })
    .await;
    h.engine
        .library_input(LibInput::SetAddStatus(EntryStatus::Planned));
    h.engine.library_input(LibInput::ConfirmAdd);
    let snapshot = h
        .until("saved", |s| {
            view(s).is_some_and(|v| v.add_dialog.as_ref().is_some_and(|d| d.summary.is_some()))
        })
        .await;
    assert_eq!(*h.api.updated.lock().unwrap(), [7]);
    assert!(h.api.created.lock().unwrap().is_empty());
    assert_eq!(view(&snapshot).unwrap().rows.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_a_store_entry_temporarily_or_permanently_and_resetting_ignores() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("token").await;
    h.until("listed", |s| {
        view(s).is_some_and(|v| v.rows.len() == 1 && !v.scanning)
    })
    .await;

    // Temporary removal removes the entry immediately; rescan brings it back.
    h.engine.remove_entry("steam:620".to_owned(), true);
    let snapshot = h.engine.snapshot();
    assert!(view(&snapshot).unwrap().rows.is_empty());
    assert_eq!(view(&snapshot).unwrap().total, 0);

    h.engine.rescan();
    h.until("restored after rescan", |s| {
        view(s).is_some_and(|v| v.rows.len() == 1 && !v.scanning)
    })
    .await;

    // Permanent removal persists the exclusion and keeps it out across rescans.
    h.engine.remove_entry("steam:620".to_owned(), false);
    let snapshot = h.engine.snapshot();
    assert!(view(&snapshot).unwrap().rows.is_empty());
    assert_eq!(view(&snapshot).unwrap().total, 0);
    let store = h.store.clone();
    eventually("excluded persisted", || {
        *store.excluded.lock().unwrap() == ["steam:620"]
    })
    .await;

    let scans_before = h.store.scans.lock().unwrap().len();
    h.engine.rescan();
    let store = h.store.clone();
    eventually("rescan finished", || {
        store.scans.lock().unwrap().len() > scans_before
    })
    .await;
    h.until("still excluded after rescan", |s| {
        view(s).is_some_and(|v| v.rows.is_empty() && v.total == 0 && !v.scanning)
    })
    .await;

    // Resetting ignored discoveries restores the excluded store entry.
    h.engine.reset_local_ignores();
    let store = h.store.clone();
    eventually("excluded cleared", || {
        store.excluded.lock().unwrap().is_empty()
    })
    .await;
    h.until("restored after reset_local_ignores", |s| {
        view(s).is_some_and(|v| v.rows.len() == 1 && v.total == 1 && !v.scanning)
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn activating_an_entry_launches_playable_games_and_opens_details_when_unavailable() {
    use gamenode_core::library::{Direction, Input as LibInput};

    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("token").await;
    h.until("listed", |s| {
        view(s).is_some_and(|v| v.rows.len() == 1 && !v.scanning)
    })
    .await;

    // Close the detail pane first; activating a playable entry by key (double-click) selects and
    // launches it without forcing the detail pane open.
    h.engine.library_input(LibInput::SetDetailOpen(false));
    assert!(!view(&h.engine.snapshot()).unwrap().layout.detail_open);

    crate::commands::activate_entry_by_key(&h.engine, "steam:620");
    h.until("played via double-click key", |s| s.launch.is_none())
        .await;
    assert_eq!(*h.uri.opened.lock().unwrap(), ["steam://rungameid/620"]);
    assert_eq!(view(&h.engine.snapshot()).unwrap().selected, ["steam:620"]);

    // Navigating with the keyboard sets focus; activating with an empty key (Enter) launches the
    // focused entry.
    h.uri.opened.lock().unwrap().clear();
    h.engine.library_input(LibInput::Move {
        direction: Direction::Home,
        extend: false,
    });
    crate::commands::activate_entry_by_key(&h.engine, "");
    h.until("played via keyboard focus", |s| s.launch.is_none())
        .await;
    assert_eq!(*h.uri.opened.lock().unwrap(), ["steam://rungameid/620"]);

    // When Play is unavailable (e.g. an unknown key leaves nothing selected), activating falls back
    // to opening the detail pane.
    h.engine.library_input(LibInput::SetDetailOpen(false));
    crate::commands::activate_entry_by_key(&h.engine, "steam:missing");
    assert!(view(&h.engine.snapshot()).unwrap().layout.detail_open);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cover_burst_publishes_far_fewer_snapshots_and_keeps_the_last_cover() {
    const COVERS: u64 = 64;
    let api = FakeApi::new();
    // Keep the first downloads in flight until after the counting starts; the same delay also
    // slows the session check, which only makes the setup wait for the library to be ready.
    *api.delay.lock().unwrap() = Duration::from_millis(300);
    let mut h = harness(api.clone(), FakeCreds::with(Some("stored")));
    *h.store.account.lock().unwrap() = Some(account("u1", "Ana"));
    let mut cache = LibraryCache::default();
    for i in 0..COVERS {
        let store_id = (100 + i).to_string();
        let game_id = 1100 + i;
        cache.entries.push(StoredEntry {
            key: format!("steam:{store_id}"),
            provider: "steam".to_owned(),
            store_id: store_id.clone(),
            name: Some(format!("Game {i}")),
            install_dir: format!("Game {i}"),
            fields: Default::default(),
            installed: false,
            owned: true,
            family_shared: false,
            first_seen: 1,
            last_seen: 1,
        });
        cache.auto_matches.push(AutoMatch {
            category: 1,
            uid: store_id,
            game_ids: vec![game_id],
            // Fresh enough that the scan does not ask the server about these ids.
            checked_at: i64::MAX,
        });
        cache.games.push(GameRecord {
            game_id,
            name: format!("Game {i}"),
            release_year: None,
            cover_url: Some(format!(
                "//images.igdb.com/igdb/image/upload/t_thumb/co{game_id}.jpg"
            )),
            cover_file: None,
            cover_failed_at: None,
        });
    }
    *h.store.cache.lock().unwrap() = cache;
    h.engine.start();
    h.until("the cached library and its covers are queued", |s| {
        matches!(s.auth, AuthView::SignedIn { .. })
            && view(s).is_some_and(|v| {
                v.rows.iter().filter(|row| row.owned).count() as u64 == COVERS && !v.scanning
            })
    })
    .await;

    // Start counting after the library is ready; every remaining publish is cover-driven.
    h.publishes.store(0, Ordering::SeqCst);
    *api.delay.lock().unwrap() = Duration::ZERO;
    h.until("every cover completed", |s| {
        view(s).is_some_and(|v| {
            v.rows.iter().filter(|row| row.cover_file.is_some()).count() as u64 == COVERS
        })
    })
    .await;
    // The trailing flush publishes the final state after the burst.
    tokio::time::sleep(Duration::from_millis(400)).await;

    let published = h.publishes.load(Ordering::SeqCst);
    assert!(
        published < COVERS as usize / 4,
        "{published} snapshots published for {COVERS} cover completions"
    );

    let snapshot = h.engine.snapshot();
    let rows = &view(&snapshot).unwrap().rows;
    let mut covered = 0;
    for row in rows.iter() {
        let Some(game_id) = row.status.game_id() else {
            continue;
        };
        if (1100..1100 + COVERS).contains(&game_id) {
            assert_eq!(
                row.cover_file.as_deref(),
                Some(format!("{game_id}.jpg").as_str())
            );
            covered += 1;
        }
    }
    assert_eq!(covered, COVERS as usize, "the final snapshot lost a cover");
}
