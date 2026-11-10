//! What the library keeps on disk, as plain data plus the port the platform implements.
//!
//! Entries are never deleted. A game that a completed scan no longer finds is kept with
//! `installed = false`, so its match and cover are still there if it comes back.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use crate::ports::StorageError;
use crate::provider::Record;

use super::layout::LibraryLayout;

use serde::{Deserialize, Serialize};

/// The identity of a library entry: `<provider>:<store id>`.
pub fn entry_key(provider: &str, store_id: &str) -> String {
    format!("{provider}:{store_id}")
}

/// A game a provider has detected on this PC.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredEntry {
    pub key: String,
    pub provider: String,
    pub store_id: String,
    pub name: Option<String>,
    pub install_dir: String,
    pub fields: BTreeMap<String, String>,
    pub installed: bool,
    /// Found by the connected store account's owned catalog.
    #[serde(default)]
    pub owned: bool,
    /// Shared with the account through Steam Family (owned wins when both are true).
    #[serde(default)]
    pub family_shared: bool,
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

    /// Whether the library shows this entry: at least one of installed, owned or family-shared.
    pub fn is_visible(&self) -> bool {
        self.installed || self.owned || self.family_shared
    }
}

/// The last scan outcome of one provider.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderScanState {
    pub provider: String,
    /// Unix seconds of the last successful scan.
    pub last_success_at: Option<i64>,
    /// Set when the latest scan failed; cleared by the next success.
    pub last_error: Option<String>,
    /// Unix seconds of the last successful owned-catalog sync.
    #[serde(default)]
    pub last_catalog_at: Option<i64>,
    /// Why the latest owned-catalog sync failed; cleared by the next success.
    #[serde(default)]
    pub catalog_error: Option<CatalogError>,
    /// Unix seconds of the last successful family-catalog sync.
    #[serde(default)]
    pub last_family_at: Option<i64>,
    /// Why the latest family-catalog sync failed; cleared by the next success.
    #[serde(default)]
    pub family_error: Option<CatalogError>,
}

/// The categories a catalog sync failure is stored and shown under.
///
/// The strings are stable: the database stores them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CatalogError {
    Unreachable,
    RateLimited,
    Server,
    Expired,
    Privacy,
    Changed,
    #[serde(alias = "invalidCookies")]
    InvalidCredential,
    FamilyUnavailable,
}

impl CatalogError {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unreachable => "unreachable",
            Self::RateLimited => "rateLimited",
            Self::Server => "server",
            Self::Expired => "expired",
            Self::Privacy => "privacy",
            Self::Changed => "changed",
            Self::InvalidCredential => "invalidCredential",
            Self::FamilyUnavailable => "familyUnavailable",
        }
    }

    /// The category stored under `value`, if it is one this build knows.
    pub fn from_category(value: &str) -> Option<Self> {
        match value {
            "unreachable" => Some(Self::Unreachable),
            "rateLimited" => Some(Self::RateLimited),
            "server" => Some(Self::Server),
            "expired" => Some(Self::Expired),
            "privacy" => Some(Self::Privacy),
            "changed" => Some(Self::Changed),
            // Rows written before the provider-neutral rename still say "invalidCookies".
            "invalidCredential" | "invalidCookies" => Some(Self::InvalidCredential),
            "familyUnavailable" => Some(Self::FamilyUnavailable),
            _ => None,
        }
    }
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for CatalogError {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::from_category(value).ok_or(())
    }
}

/// One game from a provider's owned or family-shared catalog.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRecord {
    pub id: String,
    pub name: String,
    /// Named facts the catalog reports, such as the matching uid or action-template values.
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
}

/// A catalog sync result: `None` means that fact set was not attempted, `Ok` replaces it and
/// `Err` keeps it and records the failure category.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogUpdate {
    pub owned: Option<Result<Vec<CatalogRecord>, CatalogError>>,
    pub family: Option<Result<Vec<CatalogRecord>, CatalogError>>,
    pub now: i64,
}

/// The non-secret identity of a connected store account.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreAccount {
    pub provider: String,
    pub account_id: String,
    pub account_name: Option<String>,
    pub last_verified_at: Option<i64>,
}

/// The GameNode games a store id maps to, as last reported by the server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoMatch {
    pub category: u32,
    pub uid: String,
    /// Empty when the server knows no game for this id.
    pub game_ids: Vec<u64>,
    pub checked_at: i64,
}

/// What the user picked for an entry: a game, or `None` for "no GameNode game".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChosenMatch {
    pub entry_key: String,
    pub game_id: Option<u64>,
    pub chosen_at: i64,
}

/// GameNode game metadata kept for display.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

/// The account's favorite state for one GameNode game.
///
/// `pending` marks a local change GameNode has not confirmed yet; a false, confirmed row is
/// deleted instead of stored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FavoriteRecord {
    pub account_id: String,
    pub game_id: u64,
    pub is_favorite: bool,
    pub pending: bool,
    /// Unix seconds of the local change.
    pub updated_at: i64,
}

/// Everything the library needs at startup, read in one go.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryCache {
    pub entries: Vec<StoredEntry>,
    pub scans: Vec<ProviderScanState>,
    pub auto_matches: Vec<AutoMatch>,
    /// The chosen matches of the account that was asked for; empty when there is none.
    pub chosen_matches: Vec<ChosenMatch>,
    /// The favorites of the account that was asked for; empty when there is none.
    #[serde(default)]
    pub favorites: Vec<FavoriteRecord>,
    pub games: Vec<GameRecord>,
    pub excluded: Vec<String>,
}

/// The last signed-in account, kept so the cached library can open before verification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

    /// Applies an owned or family catalog result for `provider`, replacing only the fact sets the
    /// update covers and leaving installed facts, matches and covers untouched.
    fn save_catalog(&self, provider: &str, update: &CatalogUpdate) -> Result<(), StorageError>;

    /// Clears the owned and family facts and catalog sync state of `provider` without deleting
    /// entries, matches or covers.
    fn clear_catalog(&self, provider: &str) -> Result<(), StorageError>;

    /// The non-secret identity of every connected store account.
    fn load_store_accounts(&self) -> Result<Vec<StoreAccount>, StorageError>;

    /// Inserts or replaces one connected store account.
    fn save_store_account(&self, account: &StoreAccount) -> Result<(), StorageError>;

    /// Forgets one connected store account.
    fn clear_store_account(&self, provider: &str) -> Result<(), StorageError>;

    /// Stores the server's answer for each id (insert or replace).
    fn save_auto_matches(&self, matches: &[AutoMatch]) -> Result<(), StorageError>;

    /// Stores the account's choice for an entry (insert or replace).
    fn save_chosen_match(&self, account_id: &str, choice: &ChosenMatch)
    -> Result<(), StorageError>;

    /// Forgets the account's choice for an entry, so the automatic match (if any) applies again.
    fn delete_chosen_match(&self, account_id: &str, entry_key: &str) -> Result<(), StorageError>;

    /// Inserts or replaces one game's favorite state for the account. A false, confirmed row is
    /// deleted: absence means "not favorited and nothing to send".
    fn save_favorite(&self, favorite: &FavoriteRecord) -> Result<(), StorageError>;

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
