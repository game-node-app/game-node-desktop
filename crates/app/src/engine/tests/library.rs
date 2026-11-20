//! Engine tests for the game library: scanning, lookups, covers, Play and the account rules.
//!
//! Every adapter is a double with a switchable delay, so the tests can show that slow scans,
//! requests and storage never hold up the caller.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use gamenode_core::game_api::{AccountEntry, ApiError, Collection, ExternalMatch, GameSummary};
use gamenode_core::library::{
    AccountLibrary, AutoMatch, CatalogUpdate, ChosenMatch, EntryStatus, FavoriteRecord, GameRecord,
    LastAccount, LaunchError, LaunchKind, LaunchState, LibraryCache, LibraryFilters, LibraryLayout,
    LibraryStore, LibraryView, MatchStatus, ProviderScanState, SavedView, Sort, SortDirection,
    SortKey, StoreAccount, StoredEntry,
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
        Self::steam_with_games(&[("620", "Portal 2")])
    }

    /// Steam with several installed games, each with a manifest and a common folder.
    pub(super) fn steam_with_games(games: &[(&str, &str)]) -> Self {
        let mut files = MemoryFiles::new().with_file(
            &format!("{STEAM}/steamapps/libraryfolders.vdf"),
            LIBRARY_FOLDERS,
        );
        for (appid, name) in games {
            files = files
                .with_file(
                    &format!("{STEAM}/steamapps/appmanifest_{appid}.acf"),
                    &format!(
                        "\"AppState\"\n{{\n\t\"appid\"\t\t\"{appid}\"\n\t\"name\"\t\t\"{name}\"\n\t\"StateFlags\"\t\t\"4\"\n\t\"installdir\"\t\t\"{name}\"\n}}\n"
                    ),
                )
                .with_dir(&format!("{STEAM}/steamapps/common/{name}"));
        }
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
    pub(super) installations: Mutex<BTreeMap<String, String>>,
    pub(super) installations_fail: AtomicBool,
    pub(super) chosen: Mutex<Vec<(String, ChosenMatch)>>,
    pub(super) launch_selection: Mutex<Option<gamenode_core::library::LaunchRecipeSelection>>,
}

impl FakeLibrary {
    fn pause(&self) {
        self.gate.pass();
    }
}

impl LibraryStore for FakeLibrary {
    fn launch_recipe_selection(
        &self,
        entry_key: &str,
    ) -> Result<Option<gamenode_core::library::LaunchRecipeSelection>, StorageError> {
        Ok(self
            .launch_selection
            .lock()
            .unwrap()
            .clone()
            .filter(|selection| selection.entry_key == entry_key))
    }

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
    fn save_view(&self, _account: &str, view: &SavedView) -> Result<i64, StorageError> {
        self.pause();
        let mut cache = self.cache.lock().unwrap();
        if view.id != 0
            && let Some(existing) = cache
                .saved_views
                .iter_mut()
                .find(|saved| saved.id == view.id)
        {
            existing.name.clone_from(&view.name);
            existing.payload.clone_from(&view.payload);
            existing.position = view.position;
            return Ok(view.id);
        }
        let id = cache
            .saved_views
            .iter()
            .map(|saved| saved.id)
            .max()
            .unwrap_or(0)
            + 1;
        let position = if view.position > 0 {
            view.position
        } else {
            cache
                .saved_views
                .iter()
                .map(|saved| saved.position)
                .max()
                .unwrap_or(-1)
                + 1
        };
        cache.saved_views.push(SavedView {
            id,
            name: view.name.clone(),
            payload: view.payload.clone(),
            position,
            created_at: view.created_at,
        });
        Ok(id)
    }
    fn delete_view(&self, _account: &str, id: i64) -> Result<(), StorageError> {
        self.pause();
        self.cache
            .lock()
            .unwrap()
            .saved_views
            .retain(|saved| saved.id != id);
        Ok(())
    }
    fn set_hidden(
        &self,
        _account: &str,
        keys: &[String],
        hidden: bool,
        _now: i64,
    ) -> Result<(), StorageError> {
        self.pause();
        let mut cache = self.cache.lock().unwrap();
        for key in keys {
            if hidden {
                if !cache.hidden.contains(key) {
                    cache.hidden.push(key.clone());
                }
            } else {
                cache.hidden.retain(|saved| saved != key);
            }
        }
        Ok(())
    }
    fn replace_account_library(
        &self,
        _account: &str,
        entries: &[AccountEntry],
        collections: &[Collection],
        read_at: i64,
    ) -> Result<(), StorageError> {
        self.pause();
        self.cache.lock().unwrap().account_library = Some(AccountLibrary {
            entries: entries
                .iter()
                .map(|entry| (entry.game_id, entry.clone()))
                .collect(),
            collections: collections.to_vec(),
            read_at,
        });
        Ok(())
    }
    fn save_account_entry(
        &self,
        _account: &str,
        game_id: u64,
        entry: Option<&AccountEntry>,
    ) -> Result<(), StorageError> {
        self.pause();
        let mut cache = self.cache.lock().unwrap();
        if let Some(library) = &mut cache.account_library {
            match entry {
                Some(entry) => {
                    library.entries.insert(game_id, entry.clone());
                }
                None => {
                    library.entries.remove(&game_id);
                }
            }
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
        target: None,
        installed: true,
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
            platforms: Vec::new(),
        },
    }
}

fn finished_entry() -> AccountEntry {
    AccountEntry {
        id: "entry-7".to_owned(),
        game_id: 7,
        status: Some(EntryStatus::Finished),
        collection_ids: vec!["backlog".to_owned()],
    }
}

fn backlog() -> Collection {
    Collection {
        id: "backlog".to_owned(),
        name: "Backlog curto".to_owned(),
        default_status: None,
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
async fn detected_stores_report_the_scan_location_and_unreported_ones_keep_it_empty() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("token").await;
    let snapshot = h
        .until("the scan reports its location", |s| {
            view(s).is_some_and(|view| {
                !view.scanning
                    && view
                        .providers
                        .iter()
                        .any(|provider| provider.id == "steam" && provider.install_root.is_some())
            })
        })
        .await;
    let providers = &view(&snapshot).unwrap().providers;
    let steam = providers
        .iter()
        .find(|provider| provider.id == "steam")
        .unwrap();
    assert_eq!(steam.install_root.as_deref(), Some(STEAM));
    assert_eq!(steam.library_folders, Some(1));
    assert_eq!(steam.installed_count, 1);
    let packagekit = providers
        .iter()
        .find(|provider| provider.id == "packagekit")
        .unwrap();
    assert_eq!(
        (
            packagekit.install_root.as_deref(),
            packagekit.library_folders
        ),
        (None, None),
        "a provider that reports no location keeps only its installed count"
    );
    assert_eq!(packagekit.installed_count, 0);
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
async fn selected_recipe_never_falls_back_to_the_detected_provider_link() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("token").await;
    h.until("listed", |s| view(s).is_some_and(|v| v.rows.len() == 1))
        .await;
    *h.store.launch_selection.lock().unwrap() =
        Some(gamenode_core::library::LaunchRecipeSelection {
            entry_key: "steam:620".into(),
            owner_key: "plugin-owner".into(),
            plugin_id: "org.example.launcher".into(),
            publisher_fingerprint: "ab".repeat(32),
            recipe_id: "selected-recipe".into(),
            recipe_revision: 1,
            base_target_revision: 1,
            approved_plan_fingerprint: String::new(),
        });

    h.engine
        .open_entry("steam:620".to_owned(), LaunchKind::Play);
    let snapshot = h
        .until("selection needs attention", |snapshot| {
            snapshot.launch.as_ref().is_some_and(|launch| {
                launch.state == LaunchState::Failed(LaunchError::RecipeNeedsAttention)
            })
        })
        .await;
    assert!(h.uri.opened.lock().unwrap().is_empty());
    assert_eq!(
        snapshot.launch.unwrap().state,
        LaunchState::Failed(LaunchError::RecipeNeedsAttention)
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
        // Reading the account library is a GET; only a create is a write.
        when.method(POST).path_includes("collections-entries");
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
async fn verification_syncs_the_account_library_and_persists_the_complete_read() {
    let api = FakeApi::new();
    *api.lookup_result.lock().unwrap() = Some(Ok(vec![portal_2_match()]));
    *api.account_entries.lock().unwrap() = Some(Ok(vec![finished_entry()]));
    *api.collections_result.lock().unwrap() = Some(Ok(vec![backlog()]));
    let mut h = harness(api, FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("token").await;

    let snapshot = h
        .until("the status from the account library", |s| {
            view(s).is_some_and(|v| {
                v.rows.iter().any(|row| {
                    row.gamenode
                        .as_ref()
                        .is_some_and(|entry| entry.status == Some(EntryStatus::Finished))
                })
            })
        })
        .await;
    let library = view(&snapshot).unwrap();
    assert!(library.filter_counts.gamenode_status.available);
    assert_eq!(library.filter_counts.gamenode_status.finished, 1);
    assert!(*h.api.account_library_reads.lock().unwrap() >= 1);
    assert!(*h.api.collection_reads.lock().unwrap() >= 1);

    let store = h.store.clone();
    eventually("the complete read is persisted", || {
        store
            .cache
            .lock()
            .unwrap()
            .account_library
            .as_ref()
            .is_some_and(|library| {
                library
                    .entries
                    .get(&7)
                    .is_some_and(|entry| entry.status == Some(EntryStatus::Finished))
            })
    })
    .await;
    let cache = h
        .store
        .cache
        .lock()
        .unwrap()
        .account_library
        .clone()
        .unwrap();
    assert_eq!(cache.collections, [backlog()]);
    assert!(cache.read_at > 0);

    // A rescan reads the whole library again.
    let reads = *h.api.account_library_reads.lock().unwrap();
    h.engine.rescan();
    eventually("the rescan reads the account library again", || {
        *h.api.account_library_reads.lock().unwrap() > reads
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restart_shows_the_cached_account_library_before_any_request() {
    let api = FakeApi::new();
    *api.session.lock().unwrap() = Some(Err(SessionError::Unreachable));
    let mut h = harness(api, FakeCreds::with(Some("stored")));
    *h.store.account.lock().unwrap() = Some(account("u1", "Ana"));
    let mut cache = LibraryCache {
        entries: vec![cached_portal_2()],
        scans: vec![ProviderScanState {
            provider: "steam".to_owned(),
            last_success_at: Some(1),
            last_error: None,
            ..ProviderScanState::default()
        }],
        auto_matches: vec![AutoMatch {
            category: 1,
            uid: "620".to_owned(),
            game_ids: vec![7],
            checked_at: i64::MAX,
        }],
        games: vec![GameRecord {
            game_id: 7,
            name: "Portal 2".to_owned(),
            release_year: Some(2011),
            cover_url: None,
            cover_file: None,
            cover_failed_at: None,
            platforms: Vec::new(),
        }],
        ..LibraryCache::default()
    };
    cache.account_library = Some(AccountLibrary {
        entries: BTreeMap::from([(7, finished_entry())]),
        collections: vec![backlog()],
        read_at: 50,
    });
    *h.store.cache.lock().unwrap() = cache;
    h.engine.start();

    let snapshot = h
        .until("the cached status without a request", |s| {
            s.auth == AuthView::RestoreFailed
                && view(s).is_some_and(|v| {
                    v.rows.iter().any(|row| {
                        row.gamenode
                            .as_ref()
                            .is_some_and(|entry| entry.status == Some(EntryStatus::Finished))
                    })
                })
        })
        .await;
    assert_eq!(*h.api.account_library_reads.lock().unwrap(), 0);
    assert_eq!(*h.api.collection_reads.lock().unwrap(), 0);
    let library = view(&snapshot).unwrap();
    assert!(library.filter_counts.gamenode_status.available);
    assert_eq!(library.filter_counts.gamenode_status.finished, 1);

    // The detail pane shows the cached membership even though no read can run offline.
    h.engine
        .library_input(gamenode_core::library::Input::Select {
            key: "steam:620".to_owned(),
            mode: gamenode_core::library::SelectMode::Replace,
        });
    let snapshot = h.engine.snapshot();
    assert_eq!(
        view(&snapshot).unwrap().detail.as_ref().unwrap().membership,
        gamenode_core::library::Membership::InLibrary(Some(EntryStatus::Finished))
    );
    assert_eq!(*h.api.account_library_reads.lock().unwrap(), 0);
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
            target: None,
            installed: false,
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
            platforms: Vec::new(),
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
async fn a_saved_view_and_a_hidden_entry_survive_a_restart_on_the_same_store() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("good").await;

    // Wait until the startup scan produced rows, so the view has a real count and hiding has a
    // real key.
    let snapshot = h
        .until("the library rows", |s| {
            view(s).is_some_and(|v| !v.rows.is_empty())
        })
        .await;
    let key = snapshot.library.as_ref().unwrap().rows[0].key.clone();

    h.engine
        .library_input(gamenode_core::library::Input::SetSort(Sort {
            key: SortKey::Playtime,
            direction: SortDirection::Descending,
        }));
    h.engine
        .library_input(gamenode_core::library::Input::SaveView {
            name: "  Playtime  ".to_owned(),
        });
    let snapshot = h
        .until("the saved view", |s| {
            s.library.as_ref().is_some_and(|v| {
                v.saved_views.len() == 1
                    && v.saved_views[0].name == "Playtime"
                    && v.last_saved_view.is_some()
            })
        })
        .await;
    let saved = snapshot.library.as_ref().unwrap().saved_views[0].clone();
    assert!(saved.id > 0, "the database id is not guessed");

    h.engine
        .library_input(gamenode_core::library::Input::SetHidden {
            keys: vec![key.clone()],
            hidden: true,
        });
    h.until("the hidden row", |s| {
        s.library.as_ref().is_some_and(|v| {
            v.rows.iter().all(|row| row.key != key)
                && v.filter_counts.show_hidden == 1
                && v.last_hidden.is_some()
        })
    })
    .await;

    // A fresh engine over the same store, with the same signed-in account, opens the view and
    // the hidden key from disk.
    let mut restarted = harness(FakeApi::new(), FakeCreds::with(Some("good")));
    {
        let shared = Arc::get_mut(&mut restarted.engine.shared).unwrap();
        shared.ports.library = h.store.clone();
    }
    restarted.engine.start();
    let snapshot = restarted
        .until("the organization cache", |s| {
            matches!(s.auth, AuthView::SignedIn { .. })
                && s.library.as_ref().is_some_and(|v| {
                    v.saved_views.len() == 1
                        && v.saved_views[0].id == saved.id
                        && v.rows.iter().all(|row| row.key != key)
                        && v.filter_counts.show_hidden == 1
                })
        })
        .await;
    assert_eq!(
        snapshot.library.as_ref().unwrap().saved_views[0].name,
        "Playtime"
    );

    // Revealing it with Show hidden proves hiding is not deletion.
    let mut show_hidden = LibraryFilters::default();
    show_hidden.show_hidden = true;
    restarted
        .engine
        .library_input(gamenode_core::library::Input::SetFilters(show_hidden));
    restarted
        .until("the revealed row", |s| {
            view(s).is_some_and(|v| v.rows.iter().any(|row| row.key == key))
        })
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn adding_a_game_reads_back_its_id_and_undo_deletes_exactly_that_entry() {
    use gamenode_core::library::{Input as LibInput, SelectMode, UndoState};

    let api = FakeApi::new();
    *api.lookup_result.lock().unwrap() = Some(Ok(vec![portal_2_match()]));
    let mut h = harness(api, FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("token").await;
    h.until("matched", |s| {
        view(s).is_some_and(|v| {
            v.rows
                .first()
                .is_some_and(|row| matches!(row.status, MatchStatus::Matched { .. }))
        })
    })
    .await;
    h.engine.library_input(LibInput::Select {
        key: "steam:620".into(),
        mode: SelectMode::Replace,
    });
    h.until("the standing is known", |s| {
        view(s).is_some_and(|v| v.actions.add_to_gamenode.is_available())
    })
    .await;
    h.engine.library_input(LibInput::OpenAdd);
    h.until("the dialog can confirm", |s| {
        view(s).is_some_and(|v| {
            v.add_dialog
                .as_ref()
                .is_some_and(|dialog| dialog.can_confirm)
        })
    })
    .await;
    h.engine.library_input(LibInput::ConfirmAdd);
    let snapshot = h
        .until("the add summary", |s| {
            view(s).is_some_and(|v| {
                v.add_summary
                    .as_ref()
                    .is_some_and(|summary| summary.undoable && summary.added == 1)
            })
        })
        .await;
    let run = view(&snapshot)
        .unwrap()
        .add_summary
        .as_ref()
        .unwrap()
        .run_id;
    assert_eq!(*h.api.created.lock().unwrap(), [7]);
    assert_eq!(
        view(&snapshot).unwrap().rows[0]
            .gamenode
            .as_ref()
            .map(|entry| entry.entry_id.as_str()),
        Some("entry-7"),
        "the id read back from the create"
    );

    // The notification's data survives closing the dialog.
    h.engine.library_input(LibInput::CloseAdd);
    let snapshot = h.engine.snapshot();
    let library = view(&snapshot).unwrap();
    assert!(library.add_dialog.is_none());
    assert_eq!(library.add_summary.as_ref().unwrap().run_id, run);

    h.engine.library_input(LibInput::UndoAdd { run });
    let snapshot = h
        .until("the undo result", |s| {
            view(s).is_some_and(|v| {
                v.undo
                    .as_ref()
                    .is_some_and(|undo| undo.state == UndoState::Done)
            })
        })
        .await;
    assert_eq!(
        *h.api.deleted.lock().unwrap(),
        ["entry-7".to_owned()],
        "only the id this add created is deleted"
    );
    let library = view(&snapshot).unwrap();
    assert!(
        library
            .add_summary
            .as_ref()
            .is_some_and(|summary| !summary.undoable)
    );
    assert!(
        library.rows[0].gamenode.is_none(),
        "the deleted entry leaves the status column"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn signing_out_after_hiding_a_batch_reports_the_remaining_games_once() {
    use gamenode_core::library::{Input as LibInput, SelectMode};

    let api = FakeApi::new();
    *api.lookup_result.lock().unwrap() = Some(Ok(vec![
        steam_match("500", 8, "Left 4 Dead"),
        steam_match("620", 7, "Portal 2"),
        steam_match("504230", 10, "Celeste"),
    ]));
    let mut h = harness(api, FakeCreds::with(None));
    let files = SlowFiles::steam_with_games(&[
        ("500", "Left 4 Dead"),
        ("620", "Portal 2"),
        ("504230", "Celeste"),
    ]);
    *h.files.inner.lock().unwrap() = files.inner.into_inner().unwrap();
    h.signed_out().await;
    h.sign_in("token").await;
    h.until("three installed games", |s| {
        view(s).is_some_and(|v| {
            v.rows.len() == 3
                && v.rows
                    .iter()
                    .all(|row| matches!(row.status, MatchStatus::Matched { .. }))
        })
    })
    .await;

    h.engine.library_input(LibInput::Select {
        key: "steam:500".into(),
        mode: SelectMode::Replace,
    });
    h.engine.library_input(LibInput::Select {
        key: "steam:620".into(),
        mode: SelectMode::Toggle,
    });
    h.engine.library_input(LibInput::Select {
        key: "steam:504230".into(),
        mode: SelectMode::Toggle,
    });
    h.until("the add is available", |s| {
        view(s).is_some_and(|v| v.actions.add_to_gamenode.is_available())
    })
    .await;
    h.engine.library_input(LibInput::OpenAdd);
    h.until("ready to confirm", |s| {
        view(s).is_some_and(|v| {
            v.add_dialog
                .as_ref()
                .is_some_and(|dialog| dialog.can_confirm)
        })
    })
    .await;

    // Hold every GameNode write so the batch is still running when the account goes away.
    *h.api.delay.lock().unwrap() = Duration::from_millis(250);
    h.engine.library_input(LibInput::ConfirmAdd);
    h.engine.library_input(LibInput::HideAdd);
    h.engine.dispatch(Input::SignOut);

    let snapshot = h
        .until("signed out", |s| s.auth == AuthView::SignedOut)
        .await;
    assert!(snapshot.library.is_none());
    let notice = snapshot
        .add_notice
        .as_ref()
        .expect("the hidden batch's outcome survives sign-out");
    assert!(notice.batch);
    assert_eq!(notice.account_id.as_deref(), Some("u1"));
    assert_eq!(
        notice.summary.added
            + notice.summary.already_present
            + notice.summary.skipped
            + notice.summary.failed,
        3,
        "every selected game is counted: {:?}",
        notice.summary
    );
    let run = notice.summary.run_id;
    let created_at_sign_out = h.api.created.lock().unwrap().len();

    // The answers to any request that was already out are ignored: nothing else is created and
    // the notice's counts do not change.
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(h.api.created.lock().unwrap().len(), created_at_sign_out);
    assert!(!h.engine.lock().library.add_run_active());
    let after = h.engine.snapshot().add_notice.expect("the notice is kept");
    assert_eq!(after.summary.run_id, run);
    assert_eq!(after.summary, notice.summary);
}

/// One Steam match for the multi-game library fixture.
fn steam_match(uid: &str, id: u64, name: &str) -> ExternalMatch {
    ExternalMatch {
        uid: uid.to_owned(),
        game: GameSummary {
            id,
            name: name.to_owned(),
            release_year: None,
            cover_url: None,
            platforms: Vec::new(),
        },
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn switching_accounts_after_hiding_a_batch_stops_creates_and_keeps_first_account_notice() {
    use gamenode_core::library::{Input as LibInput, SelectMode};

    let api = FakeApi::new();
    *api.lookup_result.lock().unwrap() = Some(Ok(vec![
        steam_match("500", 8, "Left 4 Dead"),
        steam_match("620", 7, "Portal 2"),
        steam_match("504230", 10, "Celeste"),
    ]));
    let mut h = harness(api, FakeCreds::with(None));
    let files = SlowFiles::steam_with_games(&[
        ("500", "Left 4 Dead"),
        ("620", "Portal 2"),
        ("504230", "Celeste"),
    ]);
    *h.files.inner.lock().unwrap() = files.inner.into_inner().unwrap();
    h.signed_out().await;
    h.sign_in("token-u1").await;
    h.until("three installed games", |s| {
        view(s).is_some_and(|v| {
            v.rows.len() == 3
                && v.rows
                    .iter()
                    .all(|row| matches!(row.status, MatchStatus::Matched { .. }))
        })
    })
    .await;

    h.engine.library_input(LibInput::Select {
        key: "steam:500".into(),
        mode: SelectMode::Replace,
    });
    h.engine.library_input(LibInput::Select {
        key: "steam:620".into(),
        mode: SelectMode::Toggle,
    });
    h.engine.library_input(LibInput::Select {
        key: "steam:504230".into(),
        mode: SelectMode::Toggle,
    });
    h.until("the add is available", |s| {
        view(s).is_some_and(|v| v.actions.add_to_gamenode.is_available())
    })
    .await;
    h.engine.library_input(LibInput::OpenAdd);
    h.until("ready to confirm", |s| {
        view(s).is_some_and(|v| {
            v.add_dialog
                .as_ref()
                .is_some_and(|dialog| dialog.can_confirm)
        })
    })
    .await;

    *h.api.delay.lock().unwrap() = Duration::from_millis(250);
    h.engine.library_input(LibInput::ConfirmAdd);
    h.engine.library_input(LibInput::HideAdd);
    h.engine.dispatch(Input::SignOut);
    let signed_out = h
        .until("signed out", |s| s.auth == AuthView::SignedOut)
        .await;
    let notice = signed_out
        .add_notice
        .clone()
        .expect("u1's hidden batch notice survives sign-out");
    let created_before_switch = h.api.created.lock().unwrap().len();

    *h.api.delay.lock().unwrap() = Duration::ZERO;
    *h.api.session.lock().unwrap() = Some(Ok(gamenode_core::User {
        id: "u2".into(),
        name: "Bob".into(),
        email: Some("bob@example.com".into()),
        image: None,
    }));
    h.sign_in("token-u2").await;
    let switched = h
        .until("u2 library ready", |s| {
            matches!(&s.auth, AuthView::SignedIn { user, .. } if user.id == "u2")
                && view(s).is_some()
        })
        .await;
    assert!(view(&switched).unwrap().add_summary.is_none());

    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(h.api.created.lock().unwrap().len(), created_before_switch);
    let after = h.engine.snapshot();
    assert!(view(&after).unwrap().add_summary.is_none());
    let kept = after
        .add_notice
        .expect("u1's notice stays attributed to u1");
    assert_eq!(kept.account_id.as_deref(), Some("u1"));
    assert_eq!(kept.summary.run_id, notice.summary.run_id);
    assert_eq!(kept.summary, notice.summary);
}

#[tokio::test(flavor = "multi_thread")]
async fn hidden_batch_with_all_failed_or_skipped_items_still_populates_add_notice() {
    use gamenode_core::library::{Input as LibInput, SelectMode};

    let api = FakeApi::new();
    // Only 620 is matched; 500 stays unmatched and is skipped by the add run.
    *api.lookup_result.lock().unwrap() = Some(Ok(vec![steam_match("620", 7, "Portal 2")]));
    let mut h = harness(api, FakeCreds::with(None));
    let files = SlowFiles::steam_with_games(&[("500", "Left 4 Dead"), ("620", "Portal 2")]);
    *h.files.inner.lock().unwrap() = files.inner.into_inner().unwrap();
    h.signed_out().await;
    h.sign_in("token").await;
    h.until("both games scanned and matched", |s| {
        view(s).is_some_and(|v| {
            v.rows.len() == 2
                && v.rows.iter().any(|row| {
                    row.key == "steam:620" && matches!(row.status, MatchStatus::Matched { .. })
                })
        })
    })
    .await;

    h.engine.library_input(LibInput::Select {
        key: "steam:500".into(),
        mode: SelectMode::Replace,
    });
    h.engine.library_input(LibInput::Select {
        key: "steam:620".into(),
        mode: SelectMode::Toggle,
    });
    h.until("the add is available", |s| {
        view(s).is_some_and(|v| v.actions.add_to_gamenode.is_available())
    })
    .await;
    h.engine.library_input(LibInput::OpenAdd);
    h.until("ready to confirm", |s| {
        view(s).is_some_and(|v| {
            v.add_dialog
                .as_ref()
                .is_some_and(|dialog| dialog.can_confirm)
        })
    })
    .await;

    *h.api.entry.lock().unwrap() = Some(Err(ApiError::Failed));
    h.engine.library_input(LibInput::ConfirmAdd);
    h.engine.library_input(LibInput::HideAdd);

    let snapshot = h
        .until("hidden batch finished with zero added", |s| {
            s.add_notice
                .as_ref()
                .is_some_and(|notice| notice.summary.skipped == 1 && notice.summary.failed == 1)
        })
        .await;
    let notice = snapshot.add_notice.as_ref().unwrap();
    assert!(notice.batch);
    assert_eq!(notice.account_id.as_deref(), Some("u1"));
    assert_eq!(notice.summary.added, 0);
    assert_eq!(notice.summary.already_present, 0);
    assert_eq!(notice.summary.skipped, 1);
    assert_eq!(notice.summary.failed, 1);
    assert!(notice.summary.run_id > 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_a_single_game_add_stays_quiet_without_notice() {
    use gamenode_core::library::{Input as LibInput, SelectMode};

    let api = FakeApi::new();
    *api.lookup_result.lock().unwrap() = Some(Ok(vec![portal_2_match()]));
    let mut h = harness(api, FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("token").await;
    h.until("matched", |s| {
        view(s).is_some_and(|v| {
            v.rows
                .first()
                .is_some_and(|row| matches!(row.status, MatchStatus::Matched { .. }))
        })
    })
    .await;

    h.engine.library_input(LibInput::Select {
        key: "steam:620".into(),
        mode: SelectMode::Replace,
    });
    h.until("the add is available", |s| {
        view(s).is_some_and(|v| v.actions.add_to_gamenode.is_available())
    })
    .await;
    h.engine.library_input(LibInput::OpenAdd);
    h.until("ready to confirm", |s| {
        view(s).is_some_and(|v| {
            v.add_dialog
                .as_ref()
                .is_some_and(|dialog| dialog.can_confirm)
        })
    })
    .await;

    *h.api.delay.lock().unwrap() = Duration::from_millis(200);
    h.engine.library_input(LibInput::ConfirmAdd);
    h.engine.library_input(LibInput::CloseAdd);

    tokio::time::sleep(Duration::from_millis(350)).await;
    let snapshot = h.engine.snapshot();
    assert!(!h.engine.lock().library.add_run_active());
    assert!(snapshot.add_notice.is_none());
    assert!(view(&snapshot).unwrap().add_summary.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn batch_status_and_collection_changes_reach_the_api_and_refresh_the_rows() {
    use gamenode_core::library::Input as LibInput;

    let api = FakeApi::new();
    *api.lookup_result.lock().unwrap() = Some(Ok(vec![portal_2_match()]));
    *api.account_entries.lock().unwrap() = Some(Ok(vec![finished_entry()]));
    *api.collections_result.lock().unwrap() = Some(Ok(vec![
        backlog(),
        Collection {
            id: "coop".to_owned(),
            name: "Coop no sofá".to_owned(),
            default_status: None,
        },
    ]));
    let mut h = harness(api, FakeCreds::with(None));
    h.signed_out().await;
    h.sign_in("token").await;
    h.until("the cached status", |s| {
        view(s).is_some_and(|v| {
            v.rows.first().is_some_and(|row| {
                row.gamenode
                    .as_ref()
                    .is_some_and(|entry| entry.status == Some(EntryStatus::Finished))
            })
        })
    })
    .await;

    h.engine.library_input(LibInput::SetStatus {
        keys: vec!["steam:620".into()],
        status: EntryStatus::Dropped,
    });
    let snapshot = h
        .until("the status run", |s| {
            view(s).is_some_and(|v| {
                v.status_run
                    .as_ref()
                    .is_some_and(|run| run.changed == 1 && run.status == EntryStatus::Dropped)
            })
        })
        .await;
    assert_eq!(*h.api.updated.lock().unwrap(), [7]);
    let library = view(&snapshot).unwrap();
    assert_eq!(
        library.rows[0]
            .gamenode
            .as_ref()
            .and_then(|entry| entry.status),
        Some(EntryStatus::Dropped)
    );
    assert_eq!(
        library.rows[0]
            .gamenode
            .as_ref()
            .map(|entry| entry.collection_ids.clone()),
        Some(vec!["backlog".to_owned()]),
        "the status change preserved the collection"
    );

    h.engine.library_input(LibInput::AddToCollection {
        keys: vec!["steam:620".into()],
        collection_id: "coop".to_owned(),
    });
    let snapshot = h
        .until("the collection run", |s| {
            view(s).is_some_and(|v| {
                v.collection_run
                    .as_ref()
                    .is_some_and(|run| run.changed == 1 && run.collection_name == "Coop no sofá")
            })
        })
        .await;
    assert_eq!(*h.api.updated.lock().unwrap(), [7, 7]);
    let library = view(&snapshot).unwrap();
    let entry = library.rows[0].gamenode.as_ref().unwrap();
    assert_eq!(
        entry.status,
        Some(EntryStatus::Dropped),
        "the collection change preserved the status"
    );
    assert!(entry.collection_ids.iter().any(|id| id == "backlog"));
    assert!(entry.collection_ids.iter().any(|id| id == "coop"));
}

// Tests retained from the plugin branch during main integration.

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

fn open_guard() -> crate::plugin_host::library::CommitGuard {
    Box::new(|| Ok(()))
}

fn denying_guard() -> crate::plugin_host::library::CommitGuard {
    Box::new(|| Err(tonic::Status::permission_denied("revoked")))
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

fn search_page(games: &[(u64, &str)]) -> gamenode_core::game_api::SearchPage {
    gamenode_core::game_api::SearchPage {
        games: games
            .iter()
            .map(|(id, name)| GameSummary {
                id: *id,
                name: (*name).to_owned(),
                release_year: Some(2013),
                cover_url: None,
                platforms: vec![],
            })
            .collect(),
        complete: Some(true),
    }
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

// Plugin-specific engine regressions retained across the library redesign.

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

#[tokio::test(flavor = "multi_thread")]
async fn extension_readiness_resolves_only_current_account_library_choices() {
    use crate::plugin_host::{library::LibraryFeed, profile::PluginProfiles};
    let api = FakeApi::new();
    *api.lookup_result.lock().unwrap() = Some(Ok(vec![portal_2_match()]));
    let mut h = harness(api, FakeCreds::with(None));
    let profiles = Arc::new(PluginProfiles::default());
    let feed = LibraryFeed::new();
    h.engine.set_plugin_profiles(profiles.clone());
    h.engine.set_library_feed(feed.clone());
    h.signed_out().await;
    assert!(h.engine.extension_readiness_choices().is_empty());
    h.sign_in("token").await;
    eventually("matched entry is selectable for readiness", || {
        h.engine.extension_readiness_choices().len() == 1
    })
    .await;

    let choices = serde_json::to_value(h.engine.extension_readiness_choices()).unwrap();
    let choice = &choices[0];
    assert_eq!(choice["name"], "Portal 2");
    let selection = choice["selection"].clone();
    let resolve = |selection| {
        h.engine
            .resolve_extension_readiness(serde_json::from_value(selection).unwrap())
    };
    let target = resolve(selection.clone()).unwrap();
    assert_eq!(target.context.game_id, "7");
    assert_eq!(
        target.context.profile_id,
        profiles.require().unwrap().profile_id()
    );
    assert!(target.is_current());

    let mut foreign = selection.clone();
    foreign["installationId"] = "foreign-installation".into();
    assert!(resolve(foreign).is_err());

    let (mut source, _) = feed.projected_entry("steam:620").unwrap();
    source.detected_target = Some((99, true, true));
    feed.sync(vec![source]);
    assert!(!target.is_current());
    assert!(resolve(selection).is_err());

    let choices = serde_json::to_value(h.engine.extension_readiness_choices()).unwrap();
    let selection = choices[0]["selection"].clone();
    profiles.update(Some("another-account"), true);
    profiles.update(Some("u1"), true);
    assert!(resolve(selection).is_err());
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

#[tokio::test(flavor = "multi_thread")]
async fn creating_a_collection_from_the_dialog_relists_and_selects_it() {
    use gamenode_core::library::{CollectionsView, Input as LibInput, SelectMode};
    let api = FakeApi::new();
    *api.lookup_result.lock().unwrap() = Some(Ok(vec![portal_2_match()]));
    *api.collections_result.lock().unwrap() = Some(Ok(vec![Collection {
        id: "c1".to_owned(),
        name: "Backlog".to_owned(),
        default_status: None,
    }]));
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
    h.until("collections listed", |s| {
        view(s).is_some_and(|v| {
            v.add_dialog.as_ref().is_some_and(|d| {
                d.can_confirm
                    && matches!(&d.collections, CollectionsView::Loaded(list) if list.len() == 1)
            })
        })
    })
    .await;

    h.engine.library_input(LibInput::StartNewCollection);
    h.engine
        .library_input(LibInput::SetNewCollectionName("Coop".to_owned()));
    h.engine.library_input(LibInput::CreateCollection);
    let snapshot = h
        .until("the new collection is selected", |s| {
            view(s).is_some_and(|v| {
                v.add_dialog.as_ref().is_some_and(|d| {
                    d.new_collection.is_none()
                        && matches!(&d.collections, CollectionsView::Loaded(list)
                            if list.iter().any(|c| c.id == "created-1" && c.name == "Coop" && c.picked))
                })
            })
        })
        .await;
    assert!(
        view(&snapshot)
            .unwrap()
            .add_dialog
            .as_ref()
            .unwrap()
            .items
            .is_empty(),
        "creating a collection adds no game"
    );
    assert_eq!(*h.api.collections_created.lock().unwrap(), ["Coop"]);
    assert!(h.api.created.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_shared_games_preference_updates_the_library_without_a_sync() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.signed_out().await;
    let shared_only = StoredEntry {
        key: "steam:900".to_owned(),
        provider: "steam".to_owned(),
        store_id: "900".to_owned(),
        name: Some("Family Game".to_owned()),
        target: None,
        install_dir: "Family Game".to_owned(),
        fields: Default::default(),
        installed: false,
        owned: false,
        shared: true,
        first_seen: 1,
        last_seen: 1,
    };
    *h.store.cache.lock().unwrap() = LibraryCache {
        entries: vec![cached_portal_2(), shared_only],
        ..LibraryCache::default()
    };
    h.sign_in("token").await;
    h.until("the shared entry is a row", |s| {
        view(s).is_some_and(|library| {
            library.rows.iter().any(|row| row.key == "steam:900") && !library.scanning
        })
    })
    .await;

    h.engine
        .set_app_preference(gamenode_core::settings::AppPreferencePatch::SharedExcluded(
            std::collections::BTreeSet::from(["steam".to_owned()]),
        ))
        .unwrap();
    let snapshot = h.engine.snapshot();
    let library = view(&snapshot).unwrap();
    assert!(
        library.rows.iter().all(|row| row.key != "steam:900"),
        "the shared-only entry leaves the rows immediately"
    );
    assert!(
        library.rows.iter().any(|row| row.key == "steam:620"),
        "owned and installed entries stay"
    );
    let steam = library
        .providers
        .iter()
        .find(|provider| provider.id == "steam")
        .unwrap();
    assert_eq!(
        steam.shared_count, 1,
        "the store card keeps the catalog count"
    );
    assert_eq!(steam.count, 1);

    h.engine
        .set_app_preference(gamenode_core::settings::AppPreferencePatch::SharedExcluded(
            std::collections::BTreeSet::new(),
        ))
        .unwrap();
    let snapshot = h.engine.snapshot();
    assert!(
        view(&snapshot)
            .unwrap()
            .rows
            .iter()
            .any(|row| row.key == "steam:900"),
        "turning the preference back on restores the row without a sync"
    );
}
