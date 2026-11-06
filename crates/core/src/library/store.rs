//! What the library keeps on disk, as plain data plus the port the platform implements.
//!
//! Entries are never deleted. A game that a completed scan no longer finds is kept with
//! `installed = false`, so its match and cover are still there if it comes back.

use std::collections::BTreeMap;

use crate::ports::StorageError;
use crate::provider::Record;

use super::layout::LibraryLayout;

/// The identity of a library entry: `<provider>:<store id>`.
pub fn entry_key(provider: &str, store_id: &str) -> String {
    format!("{provider}:{store_id}")
}

/// A game a provider has detected on this PC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredEntry {
    pub key: String,
    pub provider: String,
    pub store_id: String,
    pub name: Option<String>,
    pub install_dir: String,
    pub fields: BTreeMap<String, String>,
    pub installed: bool,
    pub first_seen: i64,
    pub last_seen: i64,
}

impl StoredEntry {
    /// The scan record this entry came from, for building launch and install URIs.
    pub fn record(&self) -> Record {
        Record {
            id: self.store_id.clone(),
            name: self.name.clone(),
            install_dir: self.install_dir.clone(),
            fields: self.fields.clone(),
        }
    }
}

/// The last scan outcome of one provider.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProviderScanState {
    pub provider: String,
    /// Unix seconds of the last successful scan.
    pub last_success_at: Option<i64>,
    /// Set when the latest scan failed; cleared by the next success.
    pub last_error: Option<String>,
}

/// The GameNode games a store id maps to, as last reported by the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoMatch {
    pub category: u32,
    pub uid: String,
    /// Empty when the server knows no game for this id.
    pub game_ids: Vec<u64>,
    pub checked_at: i64,
}

/// What the user picked for an entry: a game, or `None` for "no GameNode game".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChosenMatch {
    pub entry_key: String,
    pub game_id: Option<u64>,
    pub chosen_at: i64,
}

/// GameNode game metadata kept for display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameRecord {
    pub game_id: u64,
    pub name: String,
    pub release_year: Option<i32>,
    pub cover_url: Option<String>,
    /// File name under the per-user `covers/` folder.
    pub cover_file: Option<String>,
    /// Unix seconds of the last failed cover download; the next scan retries.
    pub cover_failed_at: Option<i64>,
}

/// Everything the library needs at startup, read in one go.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LibraryCache {
    pub entries: Vec<StoredEntry>,
    pub scans: Vec<ProviderScanState>,
    pub auto_matches: Vec<AutoMatch>,
    /// The chosen matches of the account that was asked for; empty when there is none.
    pub chosen_matches: Vec<ChosenMatch>,
    pub games: Vec<GameRecord>,
    pub excluded: Vec<String>,
}

/// The last signed-in account, kept so the cached library can open before verification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LastAccount {
    pub id: String,
    pub name: String,
}

/// Library persistence. All calls may block; run them on the storage worker.
///
/// `now` is unix seconds, passed in so this crate never reads the clock.
pub trait LibraryStore: Send + Sync {
    fn load_cache(&self, account_id: Option<&str>) -> Result<LibraryCache, StorageError>;

    /// Records a completed scan of `provider` in one transaction: its entries in `records` are
    /// inserted or refreshed and installed, entries it no longer finds are marked uninstalled, and
    /// the provider's state shows the success. Other providers are untouched.
    fn save_scan(&self, provider: &str, records: &[Record], now: i64) -> Result<(), StorageError>;

    /// Records that a scan of `provider` failed. Its entries are left as they were.
    fn save_scan_failure(&self, provider: &str, error: &str) -> Result<(), StorageError>;

    /// Stores the server's answer for each id (insert or replace).
    fn save_auto_matches(&self, matches: &[AutoMatch]) -> Result<(), StorageError>;

    /// Stores the account's choice for an entry (insert or replace).
    fn save_chosen_match(&self, account_id: &str, choice: &ChosenMatch)
    -> Result<(), StorageError>;

    /// Forgets the account's choice for an entry, so the automatic match (if any) applies again.
    fn delete_chosen_match(&self, account_id: &str, entry_key: &str) -> Result<(), StorageError>;

    /// Inserts or updates game name, year and cover URL. The `cover_file` and `cover_failed_at`
    /// of the input are ignored (use [`save_cover_ready`](Self::save_cover_ready) and
    /// [`save_cover_failed`](Self::save_cover_failed)); a changed cover URL clears the stored
    /// ones, since they belonged to the old image.
    fn save_games(&self, games: &[GameRecord]) -> Result<(), StorageError>;

    fn save_cover_ready(&self, game_id: u64, file: &str) -> Result<(), StorageError>;

    fn save_cover_failed(&self, game_id: u64, at: i64) -> Result<(), StorageError>;

    fn load_layout(&self) -> Result<LibraryLayout, StorageError>;
    fn save_layout(&self, layout: &LibraryLayout) -> Result<(), StorageError>;

    fn save_excluded(&self, excluded: &[String]) -> Result<(), StorageError> {
        let _ = excluded;
        Ok(())
    }

    fn load_last_account(&self) -> Result<Option<LastAccount>, StorageError>;
    fn save_last_account(&self, account: &LastAccount) -> Result<(), StorageError>;
    /// Removes the last-account record. The cache and chosen matches stay for a later sign-in.
    fn clear_last_account(&self) -> Result<(), StorageError>;
}
