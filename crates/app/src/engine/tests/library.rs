//! Engine tests for the game library: scanning, lookups, covers, Play and the account rules.
//!
//! Every adapter is a double with a switchable delay, so the tests can show that slow scans,
//! requests and storage never hold up the caller.

use std::collections::BTreeMap;
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
    /// Installation ids by entry key, assigned like the SQLite store: once, never changed.
    pub(super) installations: Mutex<BTreeMap<String, String>>,
    /// While set, installation id requests fail.
    pub(super) installations_fail: AtomicBool,
    /// Match choices saved, by account.
    pub(super) chosen: Mutex<Vec<(String, ChosenMatch)>>,
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
    fn save_scan(
        &self,
        provider: &str,
        records: &[Record],
        _targets: &BTreeMap<String, gamenode_core::library::TargetSpec>,
        _now: i64,
    ) -> Result<(), StorageError> {
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
    fn save_chosen_match(&self, account: &str, choice: &ChosenMatch) -> Result<(), StorageError> {
        self.chosen
            .lock()
            .unwrap()
            .push((account.to_owned(), choice.clone()));
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
    fn installation_ids(
        &self,
        entry_keys: &[String],
    ) -> Result<Vec<(String, String)>, StorageError> {
        self.pause();
        if self.installations_fail.load(Ordering::SeqCst) {
            return Err(StorageError::Failed("installation ids unavailable".into()));
        }
        let mut ids = self.installations.lock().unwrap();
        Ok(entry_keys
            .iter()
            .map(|key| {
                let next = format!("{:032x}", ids.len() as u128 + 0xa5a5_0000);
                let id = ids.entry(key.clone()).or_insert(next).clone();
                (key.clone(), id)
            })
            .collect())
    }
    fn save_display_name(
        &self,
        entry_key: &str,
        expected_revision: u64,
        display_name: Option<&str>,
    ) -> Result<gamenode_core::library::DisplayWrite, StorageError> {
        use gamenode_core::library::{DisplayOverride, DisplayWrite};
        self.pause();
        let mut cache = self.cache.lock().unwrap();
        let current = cache
            .display_overrides
            .iter()
            .find(|over| over.entry_key == entry_key)
            .cloned();
        let revision = current.as_ref().map_or(0, |over| over.revision);
        if revision != expected_revision {
            return Ok(DisplayWrite::Conflict { current: revision });
        }
        if current
            .as_ref()
            .and_then(|over| over.display_name.as_deref())
            == display_name
        {
            return Ok(DisplayWrite::Unchanged { revision });
        }
        cache
            .display_overrides
            .retain(|over| over.entry_key != entry_key);
        cache.display_overrides.push(DisplayOverride {
            entry_key: entry_key.to_owned(),
            display_name: display_name.map(str::to_owned),
            revision: revision + 1,
        });
        Ok(DisplayWrite::Applied {
            revision: revision + 1,
        })
    }
    fn save_artwork(
        &self,
        entry_key: &str,
        expected_revision: u64,
        file: Option<&str>,
    ) -> Result<gamenode_core::library::ArtworkWrite, StorageError> {
        use gamenode_core::library::{ArtworkOverride, ArtworkWrite};
        self.pause();
        let mut cache = self.cache.lock().unwrap();
        let current = cache
            .artwork_overrides
            .iter()
            .find(|over| over.entry_key == entry_key)
            .cloned();
        let revision = current.as_ref().map_or(0, |over| over.revision);
        if revision != expected_revision {
            return Ok(ArtworkWrite::Conflict { current: revision });
        }
        if current.as_ref().and_then(|over| over.file.as_deref()) == file {
            return Ok(ArtworkWrite::Unchanged { revision });
        }
        cache
            .artwork_overrides
            .retain(|over| over.entry_key != entry_key);
        cache.artwork_overrides.push(ArtworkOverride {
            entry_key: entry_key.to_owned(),
            file: file.map(str::to_owned),
            revision: revision + 1,
        });
        Ok(ArtworkWrite::Applied {
            revision: revision + 1,
            replaced: current.and_then(|over| over.file),
        })
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
        target: None,
        owned: false,
        shared: false,
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
            target: None,
            owned: true,
            shared: false,
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

#[tokio::test(flavor = "multi_thread")]
async fn plugin_profiles_follow_the_cached_then_verified_account_and_end_at_sign_out() {
    use crate::plugin_host::profile::{PluginProfiles, profile_id_for_account};
    let api = FakeApi::new();
    *api.delay.lock().unwrap() = Duration::from_millis(300);
    let mut h = harness(api, FakeCreds::with(Some("stored")));
    seed_cache(&h, Some(account("u2", "Bo")));
    let profiles = std::sync::Arc::new(PluginProfiles::default());
    h.engine.set_plugin_profiles(profiles.clone());
    assert!(profiles.current().is_none(), "no profile before a session");
    h.engine.start();

    // An unverified restore runs on the cached account.
    h.until("cached library while verifying", |s| {
        s.auth == AuthView::Verifying && view(s).is_some()
    })
    .await;
    let cached = profiles.current().expect("cached account profile");
    assert_eq!(cached.profile_id(), profile_id_for_account("u2"));
    assert!(!cached.verified());

    // The session belongs to a different account: the cached profile is fenced.
    h.until("verified", |s| matches!(s.auth, AuthView::SignedIn { .. }))
        .await;
    let verified = profiles.current().expect("verified account profile");
    assert_eq!(verified.profile_id(), profile_id_for_account("u1"));
    assert!(verified.verified());
    assert!(verified.epoch() > cached.epoch());
    assert!(!profiles.is_current(cached.epoch()));

    h.engine.dispatch(Input::SignOut);
    h.until("signed out", |s| s.auth == AuthView::SignedOut)
        .await;
    assert!(profiles.current().is_none());
    assert!(!profiles.is_current(verified.epoch()));
}

#[tokio::test(flavor = "multi_thread")]
async fn verifying_the_cached_account_keeps_its_plugin_profile() {
    use crate::plugin_host::profile::PluginProfiles;
    let api = FakeApi::new();
    *api.delay.lock().unwrap() = Duration::from_millis(300);
    let mut h = harness(api, FakeCreds::with(Some("stored")));
    seed_cache(&h, Some(account("u1", "Ana")));
    let profiles = std::sync::Arc::new(PluginProfiles::default());
    h.engine.set_plugin_profiles(profiles.clone());
    h.engine.start();

    h.until("cached library while verifying", |s| {
        s.auth == AuthView::Verifying && view(s).is_some()
    })
    .await;
    let cached = profiles.current().expect("cached account profile");
    h.until("verified", |s| matches!(s.auth, AuthView::SignedIn { .. }))
        .await;
    let verified = profiles.current().expect("verified account profile");
    assert!(verified.verified());
    assert_eq!(verified.epoch(), cached.epoch());
    assert_eq!(verified.profile_id(), cached.profile_id());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rejected_restore_issues_no_plugin_profile() {
    use crate::plugin_host::profile::PluginProfiles;
    let api = FakeApi::new();
    *api.session.lock().unwrap() = Some(Err(SessionError::Rejected));
    let mut h = harness(api, FakeCreds::with(Some("stored")));
    seed_cache(&h, Some(account("u1", "Ana")));
    let profiles = std::sync::Arc::new(PluginProfiles::default());
    h.engine.set_plugin_profiles(profiles.clone());
    h.engine.start();

    h.until("signed out", |s| s.auth == AuthView::SignedOut)
        .await;
    assert!(profiles.current().is_none());
}

/// What a whole-library plugin would list right now: `(installation id, game id, name)`.
fn feed_entries(
    feed: &crate::plugin_host::library::LibraryFeed,
) -> Vec<(String, Option<u64>, String)> {
    use crate::plugin_host::library::Scope;
    let scope = Scope {
        own_provider: "steam".to_owned(),
        read_all: true,
        provider_filter: None,
    };
    feed.page(&scope, None, None, 200)
        .1
        .into_iter()
        .map(|entry| (entry.installation_id, entry.game_id, entry.name))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn plugins_see_entries_with_a_durable_id_and_a_game_id_that_follows_the_match() {
    use crate::plugin_host::library::LibraryFeed;
    let api = FakeApi::new();
    *api.lookup_result.lock().unwrap() = Some(Ok(vec![portal_2_match()]));
    let mut h = harness(api, FakeCreds::with(None));
    let feed = LibraryFeed::new();
    h.engine.set_library_feed(feed.clone());
    h.signed_out().await;
    assert!(feed_entries(&feed).is_empty());
    h.sign_in("token").await;

    // The scan lists the entry, then the lookup matches it to game 7.
    eventually("the entry is matched in the plugin projection", || {
        feed_entries(&feed)
            .first()
            .is_some_and(|(_, game, _)| *game == Some(7))
    })
    .await;
    let entries = feed_entries(&feed);
    assert_eq!(entries.len(), 1);
    let (id, game, name) = entries[0].clone();
    assert_eq!((game, name.as_str()), (Some(7), "Portal 2"));
    // The id is the one the library store holds for the entry, and it is not derived from it.
    assert_eq!(
        h.store.installations.lock().unwrap().get("steam:620"),
        Some(&id)
    );
    assert!(!id.contains("620") && !id.contains("steam"));

    // A rescan and a forced re-sync keep the installation id.
    h.engine
        .library_input(gamenode_core::library::Input::ScanRequested);
    h.until("rescanned", |s| view(s).is_some_and(|v| !v.scanning))
        .await;
    assert_eq!(feed_entries(&feed)[0].0, id);

    // Signing out removes the entries from the plugin projection.
    h.engine.dispatch(Input::SignOut);
    h.until("signed out", |s| s.auth == AuthView::SignedOut)
        .await;
    eventually("the projection is emptied", || {
        feed_entries(&feed).is_empty()
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_entry_stays_hidden_from_plugins_while_its_installation_id_cannot_be_stored() {
    use crate::plugin_host::library::LibraryFeed;
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.store.installations_fail.store(true, Ordering::SeqCst);
    let feed = LibraryFeed::new();
    h.engine.set_library_feed(feed.clone());
    h.signed_out().await;
    h.sign_in("token").await;
    h.until("listed in the library", |s| {
        view(s).is_some_and(|v| v.rows.len() == 1 && !v.scanning)
    })
    .await;
    assert!(feed_entries(&feed).is_empty(), "no id, no projection");
    assert!(h.store.installations.lock().unwrap().is_empty());
}

fn open_guard() -> crate::plugin_host::library::CommitGuard {
    Box::new(|| Ok(()))
}

fn denying_guard() -> crate::plugin_host::library::CommitGuard {
    Box::new(|| Err(tonic::Status::permission_denied("revoked")))
}

/// A signed-in harness whose Portal 2 entry is matched to game 7 and projected to plugins, with
/// the plugin write side connected.
async fn plugin_library() -> (
    Harness,
    std::sync::Arc<crate::plugin_host::library::LibraryFeed>,
    std::sync::Arc<dyn crate::plugin_host::library::LibraryHost>,
    String,
) {
    use crate::plugin_host::library::LibraryFeed;
    let api = FakeApi::new();
    *api.lookup_result.lock().unwrap() = Some(Ok(vec![portal_2_match()]));
    let mut h = harness(api, FakeCreds::with(None));
    let feed = LibraryFeed::new();
    h.engine.set_library_feed(feed.clone());
    h.signed_out().await;
    h.sign_in("token").await;
    eventually("the entry is matched in the plugin projection", || {
        feed_entries(&feed)
            .first()
            .is_some_and(|(_, game, _)| *game == Some(7))
    })
    .await;
    let id = feed_entries(&feed)[0].0.clone();
    let host = feed.host().expect("the engine connected its host");
    (h, feed, host, id)
}

fn projected(
    feed: &crate::plugin_host::library::LibraryFeed,
    id: &str,
) -> crate::plugin_host::library::Projected {
    use crate::plugin_host::library::Scope;
    let scope = Scope {
        own_provider: "steam".to_owned(),
        read_all: false,
        provider_filter: None,
    };
    feed.get(&scope, id).1.expect("the entry is projected")
}

fn row_title(snapshot: &Snapshot) -> Option<&str> {
    view(snapshot)
        .and_then(|v| v.rows.first())
        .map(|row| row.title.as_str())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_display_name_is_stored_shown_and_guarded_by_its_revision() {
    use gamenode_core::library::DisplayWrite;
    let (mut h, feed, host, id) = plugin_library().await;
    h.until("the matched title", |s| row_title(s) == Some("Portal 2"))
        .await;
    assert_eq!(projected(&feed, &id).metadata_revision, 0);

    let written = host
        .set_display_name(
            "steam:620".into(),
            0,
            Some("Portal Two".into()),
            open_guard(),
        )
        .await
        .unwrap();
    assert_eq!(written, DisplayWrite::Applied { revision: 1 });
    // The write reached the database double, the library shows it, plugins read it back, and the
    // detected facts are unchanged.
    let stored = h.store.cache.lock().unwrap().display_overrides.clone();
    assert_eq!(
        (
            stored[0].entry_key.as_str(),
            stored[0].display_name.as_deref(),
            stored[0].revision
        ),
        ("steam:620", Some("Portal Two"), 1)
    );
    h.until("the override is the title", |s| {
        row_title(s) == Some("Portal Two")
    })
    .await;
    let entry = projected(&feed, &id);
    assert_eq!(entry.display_name.as_deref(), Some("Portal Two"));
    assert_eq!(entry.metadata_revision, 1);
    assert_eq!(entry.name, "Portal 2");
    assert_eq!(entry.installation_id, id);

    // An obsolete revision overwrites nothing.
    let written = host
        .set_display_name("steam:620".into(), 0, Some("Stale".into()), open_guard())
        .await
        .unwrap();
    assert_eq!(written, DisplayWrite::Conflict { current: 1 });
    assert_eq!(
        projected(&feed, &id).display_name.as_deref(),
        Some("Portal Two")
    );
    assert_eq!(h.store.cache.lock().unwrap().display_overrides.len(), 1);

    // The same value again changes nothing.
    let written = host
        .set_display_name(
            "steam:620".into(),
            1,
            Some("Portal Two".into()),
            open_guard(),
        )
        .await
        .unwrap();
    assert_eq!(written, DisplayWrite::Unchanged { revision: 1 });
    assert_eq!(projected(&feed, &id).metadata_revision, 1);

    // Clearing restores the matched title; the revision keeps counting.
    let written = host
        .set_display_name("steam:620".into(), 1, None, open_guard())
        .await
        .unwrap();
    assert_eq!(written, DisplayWrite::Applied { revision: 2 });
    h.until("the title is restored", |s| {
        row_title(s) == Some("Portal 2")
    })
    .await;
    let entry = projected(&feed, &id);
    assert_eq!((entry.display_name, entry.metadata_revision), (None, 2));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_revoked_guard_stops_a_display_name_before_it_is_stored() {
    let (mut h, feed, host, id) = plugin_library().await;
    h.until("the matched title", |s| row_title(s) == Some("Portal 2"))
        .await;
    let before = projected(&feed, &id);

    let error = host
        .set_display_name("steam:620".into(), 0, Some("Nope".into()), denying_guard())
        .await
        .unwrap_err();
    assert_eq!(error.code(), tonic::Code::PermissionDenied);
    assert!(h.store.cache.lock().unwrap().display_overrides.is_empty());
    assert_eq!(projected(&feed, &id), before);
    assert_eq!(row_title(&h.engine.snapshot()), Some("Portal 2"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stored_display_name_and_its_revision_survive_a_restart() {
    use crate::plugin_host::library::LibraryFeed;
    use gamenode_core::library::{DisplayOverride, DisplayWrite};
    let api = FakeApi::new();
    *api.delay.lock().unwrap() = Duration::from_millis(100);
    let mut h = harness(api, FakeCreds::with(Some("stored")));
    seed_cache(&h, Some(account("u1", "Ana")));
    h.store
        .cache
        .lock()
        .unwrap()
        .display_overrides
        .push(DisplayOverride {
            entry_key: "steam:620".to_owned(),
            display_name: Some("Portal Two".to_owned()),
            revision: 3,
        });
    let feed = LibraryFeed::new();
    h.engine.set_library_feed(feed.clone());
    h.engine.start();

    h.until("the cached override is the title", |s| {
        row_title(s) == Some("Portal Two")
    })
    .await;
    eventually("the projection carries the stored revision", || {
        feed_entries(&feed).len() == 1
    })
    .await;
    let id = feed_entries(&feed)[0].0.clone();
    let entry = projected(&feed, &id);
    assert_eq!(entry.display_name.as_deref(), Some("Portal Two"));
    assert_eq!(entry.metadata_revision, 3);

    // The next write continues from the stored revision.
    let host = feed.host().unwrap();
    assert_eq!(
        host.set_display_name("steam:620".into(), 2, Some("Old".into()), open_guard())
            .await
            .unwrap(),
        DisplayWrite::Conflict { current: 3 }
    );
    assert_eq!(
        host.set_display_name("steam:620".into(), 3, Some("Again".into()), open_guard())
            .await
            .unwrap(),
        DisplayWrite::Applied { revision: 4 }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_favorites_only_games_its_own_entries_are_matched_to() {
    use crate::plugin_host::library::FavoriteWrite;
    let (mut h, _feed, host, _id) = plugin_library().await;
    let ok = || Ok(());
    let is_favorite =
        |s: &Snapshot| view(s).is_some_and(|v| v.rows.first().is_some_and(|r| r.is_favorite));
    assert_eq!(host.favorites("steam"), [(7, false)]);
    assert!(
        host.favorites("gog").is_empty(),
        "another provider's view is empty"
    );

    assert_eq!(
        host.set_favorite("steam", 7, true, &ok).unwrap(),
        FavoriteWrite::Applied
    );
    h.until("the star appears", is_favorite).await;
    assert_eq!(host.favorites("steam"), [(7, true)]);
    // Setting the same state again is idempotent.
    assert_eq!(
        host.set_favorite("steam", 7, true, &ok).unwrap(),
        FavoriteWrite::AlreadyApplied
    );
    // A provider with no entry matched to the game, and a game nobody matches, are the same
    // answer; neither changes anything.
    assert_eq!(
        host.set_favorite("gog", 7, false, &ok).unwrap(),
        FavoriteWrite::NoMatchedEntry
    );
    assert_eq!(
        host.set_favorite("steam", 8, true, &ok).unwrap(),
        FavoriteWrite::NoMatchedEntry
    );
    assert_eq!(host.favorites("steam"), [(7, true)]);

    // A revoked guard changes nothing.
    let denied = |_: ()| tonic::Status::permission_denied("revoked");
    let error = host
        .set_favorite("steam", 7, false, &|| Err(denied(())))
        .unwrap_err();
    assert_eq!(error.code(), tonic::Code::PermissionDenied);
    assert_eq!(host.favorites("steam"), [(7, true)]);
    assert!(is_favorite(&h.engine.snapshot()));

    assert_eq!(
        host.set_favorite("steam", 7, false, &ok).unwrap(),
        FavoriteWrite::Applied
    );
    h.until("the star is gone", |s| !is_favorite(s)).await;
    assert_eq!(host.favorites("steam"), [(7, false)]);
}

fn search_page(games: &[(u64, &str)]) -> gamenode_core::game_api::SearchPage {
    gamenode_core::game_api::SearchPage {
        games: games
            .iter()
            .map(|(id, name)| GameSummary {
                id: *id,
                name: (*name).to_owned(),
                release_year: Some(2013),
                cover_url: None,
            })
            .collect(),
        complete: Some(true),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_catalog_search_is_bounded_offers_its_games_and_opens_no_dialog() {
    let (h, _feed, host, _id) = plugin_library().await;
    *h.api.search_result.lock().unwrap() = Some(Ok(search_page(&[
        (40, "Defense of the Ancients"),
        (41, "Dota Underlords"),
        (42, "Dota 2"),
    ])));

    let found = host.search_catalog("dota".into(), 2).await.unwrap();
    assert_eq!(
        found
            .iter()
            .map(|g| (g.id, g.release_year))
            .collect::<Vec<_>>(),
        [(40, Some(2013)), (41, Some(2013))]
    );
    assert!(
        view(&h.engine.snapshot()).unwrap().match_dialog.is_none(),
        "a plugin search never touches the user's match dialog"
    );
    // Only the games that were returned can be assigned.
    let rev = h.engine.lock().library.plan_plugin_match("steam:620", 42);
    assert_eq!(rev, gamenode_core::library::PluginMatch::UnknownGame);
    let rev = h.engine.lock().library.plan_plugin_match("steam:620", 41);
    assert_eq!(rev, gamenode_core::library::PluginMatch::Assign);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unavailable_catalog_is_reported_without_changing_the_library() {
    let (h, _feed, host, _id) = plugin_library().await;
    *h.api.search_result.lock().unwrap() = Some(Err(ApiError::Failed));
    let error = host.search_catalog("dota".into(), 20).await.unwrap_err();
    assert_eq!(error.code(), tonic::Code::Unavailable);
    assert_eq!(
        h.engine.lock().library.plan_plugin_match("steam:620", 40),
        gamenode_core::library::PluginMatch::UnknownGame
    );

    // Without a verified session the catalog is not asked at all.
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    let feed = crate::plugin_host::library::LibraryFeed::new();
    h.engine.set_library_feed(feed.clone());
    h.signed_out().await;
    let host = feed.host().unwrap();
    let error = host.search_catalog("dota".into(), 20).await.unwrap_err();
    assert_eq!(error.code(), tonic::Code::Unavailable);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_assigns_its_own_entry_through_the_library_policy() {
    use crate::plugin_host::library::MatchWrite;
    use gamenode_core::library::{ChoiceSource, MatchSource, MatchStatus};
    let (mut h, feed, host, id) = plugin_library().await;
    let ok = || Ok(());
    *h.api.search_result.lock().unwrap() = Some(Ok(search_page(&[(40, "Dota"), (41, "Dota Two")])));
    host.search_catalog("dota".into(), 20).await.unwrap();
    let before = projected(&feed, &id);
    assert_eq!(before.game_id, Some(7));

    // The entry's revision guards the write; a game nobody offered is refused first.
    assert_eq!(
        host.assign_match("steam:620", 40, before.revision + 5, &ok)
            .unwrap(),
        MatchWrite::Conflict {
            current: before.revision
        }
    );
    assert_eq!(
        host.assign_match("steam:620", 99, before.revision, &ok)
            .unwrap(),
        MatchWrite::UnknownGame
    );
    assert!(h.store.chosen.lock().unwrap().is_empty());

    let MatchWrite::Applied { revision } = host
        .assign_match("steam:620", 40, before.revision, &ok)
        .unwrap()
    else {
        panic!("assigned");
    };
    assert!(revision > before.revision);
    let after = projected(&feed, &id);
    assert_eq!((after.game_id, after.revision), (Some(40), revision));
    h.until("the library shows the plugin's choice", |s| {
        view(s).is_some_and(|v| {
            v.rows.first().is_some_and(|row| {
                row.status
                    == MatchStatus::Matched {
                        game_id: 40,
                        source: MatchSource::Chosen,
                    }
            })
        })
    })
    .await;
    // The choice and the game it names are stored, marked as the plugin's. The storage worker
    // writes them after the library view updates, so wait for the write.
    eventually("the choice is stored", || {
        !h.store.chosen.lock().unwrap().is_empty()
    })
    .await;
    let chosen = h.store.chosen.lock().unwrap().clone();
    assert_eq!(chosen.len(), 1);
    assert_eq!(chosen[0].1.entry_key, "steam:620");
    assert_eq!(chosen[0].1.game_id, Some(40));
    assert_eq!(chosen[0].1.source, ChoiceSource::Plugin);
    eventually("the game record is stored", || {
        h.store
            .games
            .lock()
            .unwrap()
            .iter()
            .any(|g| g.game_id == 40)
    })
    .await;

    // The same assignment again changes nothing, whatever revision is named.
    assert_eq!(
        host.assign_match("steam:620", 40, 0, &ok).unwrap(),
        MatchWrite::AlreadyApplied { revision }
    );
    assert_eq!(h.store.chosen.lock().unwrap().len(), 1);
    // The plugin may replace its own choice.
    assert!(matches!(
        host.assign_match("steam:620", 41, revision, &ok).unwrap(),
        MatchWrite::Applied { .. }
    ));
    assert_eq!(projected(&feed, &id).game_id, Some(41));

    // A revoked guard changes nothing.
    let error = host
        .assign_match("steam:620", 40, projected(&feed, &id).revision, &|| {
            Err(tonic::Status::permission_denied("revoked"))
        })
        .unwrap_err();
    assert_eq!(error.code(), tonic::Code::PermissionDenied);
    assert_eq!(projected(&feed, &id).game_id, Some(41));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_never_replaces_what_the_user_chose_in_the_match_dialog() {
    use crate::plugin_host::library::MatchWrite;
    use gamenode_core::library::{ChoiceSource, Input, SelectMode};
    let (mut h, feed, host, id) = plugin_library().await;
    let ok = || Ok(());
    *h.api.search_result.lock().unwrap() = Some(Ok(search_page(&[(40, "Dota"), (41, "Two")])));
    h.engine.library_input(Input::Select {
        key: "steam:620".into(),
        mode: SelectMode::Replace,
    });
    h.engine.library_input(Input::OpenMatch);
    h.until("the dialog has results", |s| {
        view(s)
            .and_then(|v| v.match_dialog.as_ref())
            .is_some_and(|d| !d.searching && !d.results.is_empty())
    })
    .await;
    h.engine.library_input(Input::PickMatch {
        game_id: 40,
        now: 1,
    });
    eventually("the pick is stored as the user's", || {
        h.store
            .chosen
            .lock()
            .unwrap()
            .iter()
            .any(|(_, c)| c.game_id == Some(40) && c.source == ChoiceSource::User)
    })
    .await;
    host.search_catalog("dota".into(), 20).await.unwrap();
    let revision = projected(&feed, &id).revision;
    assert_eq!(
        host.assign_match("steam:620", 41, revision, &ok).unwrap(),
        MatchWrite::UserChoice
    );
    assert_eq!(projected(&feed, &id).game_id, Some(40));
    // The user's own game is simply already applied.
    assert_eq!(
        host.assign_match("steam:620", 40, revision, &ok).unwrap(),
        MatchWrite::AlreadyApplied { revision }
    );
}

fn row_cover(snapshot: &Snapshot) -> Option<&str> {
    view(snapshot)
        .and_then(|v| v.rows.first())
        .and_then(|row| row.cover_file.as_deref())
}

/// A prepared cover of one flat colour and a size, as the service would hand over.
fn cover_of(shade: u8, width: u32, height: u32) -> gamenode_platform::plugin_artwork::Prepared {
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        width,
        height,
        image::Rgb([shade, 60, 90]),
    ))
    .write_with_encoder(image::codecs::png::PngEncoder::new(&mut bytes))
    .unwrap();
    gamenode_platform::plugin_artwork::prepare(&bytes, "image/png").unwrap()
}

fn files_in(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_cover_is_stored_shown_read_back_replaced_and_cleared() {
    use gamenode_core::library::ArtworkWrite;
    use gamenode_platform::plugin_artwork::file_name;
    let (mut h, feed, host, id) = plugin_library().await;
    h.until("the game's own cover", |s| row_cover(s) == Some("7.jpg"))
        .await;
    let dir = h.covers.path().to_owned();
    assert_eq!(projected(&feed, &id).artwork_revision, 0);
    let own = host
        .read_artwork("steam:620".into(), 1 << 20)
        .await
        .unwrap();
    assert_eq!(
        (own.bytes.as_slice(), own.media_type, own.revision),
        (b"jpeg".as_slice(), "image/jpeg", 0)
    );

    // Set: the image lands under a name this entry owns, the library shows it, plugins see its
    // revision, and the game's own cover is untouched.
    let (first, second) = (cover_of(10, 300, 450), cover_of(200, 300, 450));
    let first_name = file_name("steam:620", &first);
    let before = projected(&feed, &id).revision;
    let written = host
        .set_artwork("steam:620".into(), 0, Some(first.clone()), open_guard())
        .await
        .unwrap();
    assert_eq!(
        written,
        ArtworkWrite::Applied {
            revision: 1,
            replaced: None
        }
    );
    assert_eq!(files_in(&dir), ["7.jpg".to_owned(), first_name.clone()]);
    assert_eq!(std::fs::read(dir.join("7.jpg")).unwrap(), b"jpeg");
    assert_eq!(std::fs::read(dir.join(&first_name)).unwrap(), first.bytes);
    h.until("the override is the cover", |s| {
        row_cover(s) == Some(first_name.as_str())
    })
    .await;
    let entry = projected(&feed, &id);
    assert_eq!(entry.artwork_revision, 1);
    assert!(entry.revision > before, "the entry changed");
    let read = host
        .read_artwork("steam:620".into(), 1 << 20)
        .await
        .unwrap();
    assert_eq!(
        (read.bytes.as_slice(), read.media_type, read.revision),
        (first.bytes.as_slice(), "image/jpeg", 1)
    );

    // Replace: the old file is deleted once the row points at the new one.
    let second_name = file_name("steam:620", &second);
    let written = host
        .set_artwork("steam:620".into(), 1, Some(second.clone()), open_guard())
        .await
        .unwrap();
    assert_eq!(
        written,
        ArtworkWrite::Applied {
            revision: 2,
            replaced: Some(first_name.clone())
        }
    );
    assert_eq!(files_in(&dir), ["7.jpg".to_owned(), second_name.clone()]);

    // A stale revision overwrites nothing and leaves no file behind.
    let third = cover_of(90, 300, 450);
    let written = host
        .set_artwork("steam:620".into(), 1, Some(third), open_guard())
        .await
        .unwrap();
    assert_eq!(written, ArtworkWrite::Conflict { current: 2 });
    assert_eq!(files_in(&dir), ["7.jpg".to_owned(), second_name.clone()]);

    // The same picture again changes nothing and keeps its file.
    let written = host
        .set_artwork("steam:620".into(), 2, Some(second), open_guard())
        .await
        .unwrap();
    assert_eq!(written, ArtworkWrite::Unchanged { revision: 2 });
    assert_eq!(files_in(&dir), ["7.jpg".to_owned(), second_name.clone()]);

    // Clear: the game's own cover is back, the file is gone and the revision keeps counting.
    let written = host
        .set_artwork("steam:620".into(), 2, None, open_guard())
        .await
        .unwrap();
    assert_eq!(
        written,
        ArtworkWrite::Applied {
            revision: 3,
            replaced: Some(second_name)
        }
    );
    assert_eq!(files_in(&dir), ["7.jpg".to_owned()]);
    h.until("the game's cover is back", |s| {
        row_cover(s) == Some("7.jpg")
    })
    .await;
    assert_eq!(projected(&feed, &id).artwork_revision, 3);
    let back = host
        .read_artwork("steam:620".into(), 1 << 20)
        .await
        .unwrap();
    assert_eq!(
        (back.bytes.as_slice(), back.revision),
        (b"jpeg".as_slice(), 3)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_revoked_guard_stops_a_cover_before_any_file_is_written() {
    let (mut h, feed, host, id) = plugin_library().await;
    h.until("the game's own cover", |s| row_cover(s) == Some("7.jpg"))
        .await;
    let before = projected(&feed, &id);
    let error = host
        .set_artwork(
            "steam:620".into(),
            0,
            Some(cover_of(10, 300, 450)),
            denying_guard(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), tonic::Code::PermissionDenied);
    assert!(h.store.cache.lock().unwrap().artwork_overrides.is_empty());
    assert_eq!(files_in(h.covers.path()), ["7.jpg".to_owned()]);
    assert_eq!(projected(&feed, &id), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cover_read_is_bounded_and_never_downloads_or_leaves_the_covers_folder() {
    let (mut h, _feed, host, _id) = plugin_library().await;
    h.until("the game's own cover", |s| row_cover(s) == Some("7.jpg"))
        .await;
    let fetched = h.api.covers_fetched.lock().unwrap().len();
    // 4 bytes is exactly the cover; one less is over the caller's budget.
    assert!(host.read_artwork("steam:620".into(), 4).await.is_ok());
    let error = host.read_artwork("steam:620".into(), 3).await.unwrap_err();
    assert_eq!(error.code(), tonic::Code::ResourceExhausted);
    // An entry the library does not know has no cover to read.
    let error = host
        .read_artwork("steam:nope".into(), 1 << 20)
        .await
        .unwrap_err();
    assert_eq!(error.code(), tonic::Code::NotFound);
    // A cover the file system no longer has is not found, and nothing is fetched to fill it.
    std::fs::remove_file(h.covers.path().join("7.jpg")).unwrap();
    let error = host
        .read_artwork("steam:620".into(), 1 << 20)
        .await
        .unwrap_err();
    assert_eq!(error.code(), tonic::Code::NotFound);
    assert_eq!(h.api.covers_fetched.lock().unwrap().len(), fetched);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stored_cover_and_its_revision_survive_a_restart() {
    use crate::plugin_host::library::LibraryFeed;
    use gamenode_core::library::{ArtworkOverride, ArtworkWrite};
    let api = FakeApi::new();
    *api.delay.lock().unwrap() = Duration::from_millis(100);
    let mut h = harness(api, FakeCreds::with(Some("stored")));
    seed_cache(&h, Some(account("u1", "Ana")));
    h.store
        .cache
        .lock()
        .unwrap()
        .artwork_overrides
        .push(ArtworkOverride {
            entry_key: "steam:620".to_owned(),
            file: Some("plugin-aa-bb.jpg".to_owned()),
            revision: 3,
        });
    let feed = LibraryFeed::new();
    h.engine.set_library_feed(feed.clone());
    h.engine.start();

    h.until("the cached override is the cover", |s| {
        row_cover(s) == Some("plugin-aa-bb.jpg")
    })
    .await;
    eventually("the projection carries the stored revision", || {
        feed_entries(&feed).len() == 1
    })
    .await;
    let id = feed_entries(&feed)[0].0.clone();
    assert_eq!(projected(&feed, &id).artwork_revision, 3);
    let host = feed.host().unwrap();
    assert_eq!(
        host.set_artwork("steam:620".into(), 2, None, open_guard())
            .await
            .unwrap(),
        ArtworkWrite::Conflict { current: 3 }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_starts_and_installs_through_the_users_own_path() {
    use tonic::Code;
    let (mut h, _feed, host, _id) = plugin_library().await;

    // What a launch would start is the same number until the target changes.
    let first = host.launch_fingerprint("steam:620".into()).await.unwrap();
    assert_eq!(
        first,
        host.launch_fingerprint("steam:620".into()).await.unwrap()
    );
    assert_eq!(
        host.launch_fingerprint("steam:1".into())
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );

    // The same links the user's Play and Install open, and the launch status is cleared the same
    // way once the operating system accepted them.
    host.open_entry("steam:620".into(), false).await.unwrap();
    host.open_entry("steam:620".into(), true).await.unwrap();
    assert_eq!(
        *h.uri.opened.lock().unwrap(),
        ["steam://rungameid/620", "steam://nav/games/details/620"]
    );
    h.until("cleared once the OS accepted the link", |s| {
        s.launch.is_none()
    })
    .await;

    // An entry that is not in the library is not found.
    assert_eq!(
        host.open_entry("steam:1".into(), false)
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );

    // A refusal by the operating system is the user's own error too, and the plugin is told.
    *h.uri.fail.lock().unwrap() = Some(UriError::NoHandler);
    assert_eq!(
        host.open_entry("steam:620".into(), false)
            .await
            .unwrap_err()
            .code(),
        Code::FailedPrecondition
    );
    assert_eq!(
        h.engine.snapshot().launch.unwrap().state,
        LaunchState::Failed(LaunchError::NoHandler)
    );
}
