//! What the library keeps on disk, as plain data plus the port the platform implements.
//!
//! Entries are never deleted. A game that a completed scan no longer finds is kept with
//! `installed = false`, so its match and cover are still there if it comes back.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use crate::game_api::{AccountEntry, Collection};
use crate::ports::StorageError;
use crate::provider::Record;

use super::layout::LibraryLayout;
use super::organization::SavedView;

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
    /// The launch target the provider detected for this entry, if any (task 7.7). It is stored
    /// apart from `fields` so a Struct/opaque field can never become an executable target.
    #[serde(default)]
    pub target: Option<super::target::DetectedTarget>,
    /// Found by the connected store account's owned catalog.
    #[serde(default)]
    pub owned: bool,
    /// Shared with the account through the provider's declared shared access (owned wins when
    /// both are true).
    #[serde(default)]
    pub shared: bool,
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

    /// Whether the library shows this entry: at least one of installed, owned or shared.
    pub fn is_visible(&self) -> bool {
        self.installed || self.owned || self.shared
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
    /// Unix seconds of the last successful shared-catalog sync.
    #[serde(default)]
    pub last_shared_at: Option<i64>,
    /// Why the latest shared-catalog sync failed; cleared by the next success.
    #[serde(default)]
    pub shared_error: Option<CatalogError>,
    /// Where the last successful scan found the store, when it reported a location.
    #[serde(default)]
    pub install_root: Option<String>,
    /// How many library folders the last successful scan reported, when it reported them.
    #[serde(default)]
    pub library_folders: Option<u32>,
    /// Whether the last successful local scan found an installed store. Unlike catalog facts this
    /// ignores ownership, search, facets and hidden entries. `None` until a scan succeeds.
    #[serde(default)]
    pub last_detected: Option<bool>,
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
    SharedUnavailable,
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
            Self::SharedUnavailable => "sharedUnavailable",
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
            // Rows written before the shared rename still say "familyUnavailable".
            "sharedUnavailable" | "familyUnavailable" => Some(Self::SharedUnavailable),
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

/// One game from a provider's owned or shared catalog.
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
    pub shared: Option<Result<Vec<CatalogRecord>, CatalogError>>,
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
    /// Who made the choice. Stored choices from before plugins existed read as the user's.
    #[serde(default)]
    pub source: ChoiceSource,
}

/// Who made a [`ChosenMatch`]. A plugin may replace its own choices but never the user's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChoiceSource {
    /// Picked in the match dialog (including "no GameNode game").
    #[default]
    User,
    /// Assigned by a plugin through the plugin API for its own entry.
    Plugin,
}

impl ChoiceSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Plugin => "plugin",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "user" => Some(Self::User),
            "plugin" => Some(Self::Plugin),
            _ => None,
        }
    }
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
    /// Platform abbreviations a search answer reported. Persisted so a stored game keeps the
    /// platforms it already received; lookups and old rows start empty.
    #[serde(default)]
    pub platforms: Vec<String>,
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

/// A presentation override a provider's plugin wrote for one of its own entries. It is kept apart
/// from the detected facts: clearing it (`display_name = None`) restores the detected name. The
/// revision counts writes per entry, starts at 1 and only ever grows, so a reset keeps its row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayOverride {
    pub entry_key: String,
    pub display_name: Option<String>,
    pub revision: u64,
}

/// How a guarded display-name write ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayWrite {
    /// The value changed; this is the entry's new revision.
    Applied { revision: u64 },
    /// The entry already had this value; its revision is unchanged.
    Unchanged { revision: u64 },
    /// The expected revision was obsolete; nothing was written.
    Conflict { current: u64 },
}

/// The cover a provider's plugin set for one of its own entries. `file` names an image in the
/// covers folder that only this entry owns; the game's own cover is never touched. Clearing it
/// (`file = None`) restores the game's cover. The revision counts writes per entry, starts at 1
/// and only ever grows, so a reset keeps its row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtworkOverride {
    pub entry_key: String,
    pub file: Option<String>,
    pub revision: u64,
}

/// How a guarded artwork write ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArtworkWrite {
    /// The value changed; this is the entry's new revision, and `replaced` the file it no longer
    /// uses (which the caller deletes).
    Applied {
        revision: u64,
        replaced: Option<String>,
    },
    /// The entry already had this value; its revision is unchanged.
    Unchanged { revision: u64 },
    /// The expected revision was obsolete; nothing was written.
    Conflict { current: u64 },
}

/// The account's last complete GameNode library read: one entry per game it knows, the account's
/// collections and when the read finished. Games that exist only in the account library are still
/// not library rows; the cache feeds status columns, the status facet, collection scopes and the
/// detail pane.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AccountLibrary {
    /// The account's library entries, keyed by GameNode game id.
    pub entries: BTreeMap<u64, AccountEntry>,
    pub collections: Vec<Collection>,
    /// Unix seconds of the completed read.
    pub read_at: i64,
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
    /// Presentation overrides written by providers' plugins.
    #[serde(default)]
    pub display_overrides: Vec<DisplayOverride>,
    /// Covers written by providers' plugins.
    #[serde(default)]
    pub artwork_overrides: Vec<ArtworkOverride>,
    /// The account's saved views, in sidebar order. Populated by the organization work.
    #[serde(default)]
    pub saved_views: Vec<SavedView>,
    /// Library keys hidden for the account. Populated by the organization work.
    #[serde(default)]
    pub hidden: Vec<String>,
    /// The last complete GameNode library read for the account, if any.
    #[serde(default)]
    pub account_library: Option<AccountLibrary>,
}

/// The last signed-in account, kept so the cached library can open before verification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastAccount {
    pub id: String,
    pub name: String,
}

/// What happened to an approval or repair of a detected target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetApproval {
    /// The target was stored at this revision with the new decision.
    Applied { revision: u64 },
    /// The target changed since the caller read it; nothing was written.
    Conflict { current: u64 },
    /// The entry has no detected target any more.
    Missing,
}

/// Library persistence. All calls may block; run them on the storage worker.
///
/// `now` is unix seconds, passed in so this crate never reads the clock.
pub trait LibraryStore: Send + Sync {
    fn load_cache(&self, account_id: Option<&str>) -> Result<LibraryCache, StorageError>;

    /// Records a completed scan of `provider` in one transaction: its entries in `records` are
    /// inserted or refreshed and installed, entries it no longer finds are marked uninstalled, and
    /// the provider's state shows the success. `targets` carries the detected launch target of
    /// each record id, if any; a target no longer reported stays stored but unavailable. Other
    /// providers are untouched.
    fn save_scan(
        &self,
        provider: &str,
        records: &[Record],
        targets: &std::collections::BTreeMap<String, super::target::TargetSpec>,
        now: i64,
    ) -> Result<(), StorageError>;

    /// Records that a scan of `provider` failed. Its entries are left as they were.
    fn save_scan_failure(&self, provider: &str, error: &str) -> Result<(), StorageError>;

    /// Stores the user's approval (or repair) of `entry_key`'s detected target if its revision is
    /// still `expected_revision`. A changed target is a conflict and writes nothing.
    fn save_target_review(
        &self,
        entry_key: &str,
        expected_revision: u64,
        replacement: &super::target::DetectedTarget,
    ) -> Result<TargetApproval, StorageError> {
        let _ = (entry_key, expected_revision, replacement);
        Ok(TargetApproval::Missing)
    }

    /// One plugin-owned recipe, scoped by a host-derived owner key.
    fn custom_launch_recipe(
        &self,
        owner_key: &str,
        recipe_id: &str,
    ) -> Result<Option<super::custom_launch::CustomLaunchRecipe>, StorageError> {
        let _ = (owner_key, recipe_id);
        Err(StorageError::Failed(
            "custom launch recipes are unavailable in this store".into(),
        ))
    }

    /// Persists a new host-issued recipe or reports the already-stored revision.
    fn save_custom_launch_recipe(
        &self,
        owner_key: &str,
        recipe: &super::custom_launch::CustomLaunchRecipe,
    ) -> Result<super::custom_launch::CustomLaunchMutation, StorageError> {
        let _ = (owner_key, recipe);
        Err(StorageError::Failed(
            "custom launch recipes are unavailable in this store".into(),
        ))
    }

    fn remove_custom_launch_recipe(
        &self,
        owner_key: &str,
        recipe_id: &str,
        expected_revision: u64,
    ) -> Result<super::custom_launch::CustomLaunchMutation, StorageError> {
        let _ = (owner_key, recipe_id, expected_revision);
        Err(StorageError::Failed(
            "custom launch recipes are unavailable in this store".into(),
        ))
    }

    /// Lists recipes owned by one authenticated plugin for an installation.
    fn custom_launch_recipes(
        &self,
        owner_key: &str,
        game_id: u64,
    ) -> Result<Vec<super::custom_launch::CustomLaunchRecipe>, StorageError> {
        let _ = (owner_key, game_id);
        Ok(Vec::new())
    }

    /// The user's active custom launch choice. No row means the detected target is active.
    fn launch_recipe_selection(
        &self,
        entry_key: &str,
    ) -> Result<Option<super::custom_launch::LaunchRecipeSelection>, StorageError> {
        let _ = entry_key;
        Ok(None)
    }

    /// Persists a user-selected recipe only while its game target and recipe revisions match.
    fn save_launch_recipe_selection(
        &self,
        selection: &super::custom_launch::LaunchRecipeSelection,
        expected_target_revision: u64,
    ) -> Result<super::custom_launch::LaunchRecipeSelectionWrite, StorageError> {
        let _ = (selection, expected_target_revision);
        Ok(super::custom_launch::LaunchRecipeSelectionWrite::RecipeMissing)
    }

    /// Explicitly returns the entry to its detected default, using target revision CAS.
    fn clear_launch_recipe_selection(
        &self,
        entry_key: &str,
        expected_target_revision: u64,
    ) -> Result<super::custom_launch::LaunchRecipeSelectionWrite, StorageError> {
        let _ = (entry_key, expected_target_revision);
        Ok(super::custom_launch::LaunchRecipeSelectionWrite::RecipeMissing)
    }

    /// Records remembered user approval for the currently selected, exact plan.
    fn approve_launch_recipe_plan(
        &self,
        entry_key: &str,
        recipe_id: &str,
        recipe_revision: u64,
        expected_target_revision: u64,
        fingerprint: &str,
    ) -> Result<super::custom_launch::LaunchRecipeSelectionWrite, StorageError> {
        let _ = (
            entry_key,
            recipe_id,
            recipe_revision,
            expected_target_revision,
            fingerprint,
        );
        Ok(super::custom_launch::LaunchRecipeSelectionWrite::RecipeMissing)
    }

    /// Lists this plugin owner's approved runtime resources.
    fn runtime_resources(
        &self,
        owner_key: &str,
    ) -> Result<Vec<super::runtime::RuntimeResource>, StorageError> {
        let _ = owner_key;
        Err(StorageError::Failed(
            "runtime resources are unavailable in this store".into(),
        ))
    }

    /// Lists every owner's runtimes for an authorized common-category read.
    fn shared_runtime_resources(
        &self,
    ) -> Result<Vec<(String, super::runtime::RuntimeResource)>, StorageError> {
        Err(StorageError::Failed(
            "runtime resources are unavailable in this store".into(),
        ))
    }

    /// Combined catalog revision across every owner, for a shared read subscription.
    fn shared_runtime_catalog_revision(&self) -> Result<u64, StorageError> {
        Err(StorageError::Failed(
            "runtime resources are unavailable in this store".into(),
        ))
    }

    /// Safe availability rows for the named installations. `owner_key` limits the projection to
    /// the caller's private bindings; `None` includes every owner for an authorized category read.
    fn runtime_availability(
        &self,
        owner_key: Option<&str>,
        installation_ids: &[String],
    ) -> Result<Vec<super::runtime::RuntimeAvailabilityEntry>, StorageError> {
        let _ = (owner_key, installation_ids);
        Err(StorageError::Failed(
            "runtime availability is unavailable in this store".into(),
        ))
    }

    /// Looks up one runtime without exposing another plugin owner's private record.
    fn runtime_resource(
        &self,
        owner_key: &str,
        runtime_id: &str,
    ) -> Result<Option<super::runtime::RuntimeResource>, StorageError> {
        let _ = (owner_key, runtime_id);
        Err(StorageError::Failed(
            "runtime resources are unavailable in this store".into(),
        ))
    }

    /// Creates a new runtime at revision 1 or updates it only at current revision + 1.
    fn save_runtime_resource(
        &self,
        owner_key: &str,
        runtime: &super::runtime::RuntimeResource,
    ) -> Result<super::runtime::RuntimeMutation, StorageError> {
        let _ = (owner_key, runtime);
        Err(StorageError::Failed(
            "runtime resources are unavailable in this store".into(),
        ))
    }

    /// Removes only the runtime record. Historical content bindings deliberately remain so they
    /// can be shown as unavailable and migrated; games, saves and play history are independent.
    fn remove_runtime_resource(
        &self,
        owner_key: &str,
        runtime_id: &str,
        expected_revision: u64,
    ) -> Result<super::runtime::RuntimeMutation, StorageError> {
        let _ = (owner_key, runtime_id, expected_revision);
        Err(StorageError::Failed(
            "runtime resources are unavailable in this store".into(),
        ))
    }

    /// Lists runtime bindings for a stable GameNode game and installation identity.
    fn runtime_content_bindings(
        &self,
        owner_key: &str,
        game_id: u64,
        installation_id: &str,
    ) -> Result<Vec<super::runtime::RuntimeContentBinding>, StorageError> {
        let _ = (owner_key, game_id, installation_id);
        Err(StorageError::Failed(
            "runtime bindings are unavailable in this store".into(),
        ))
    }

    /// Durable watch cursor for this owner's runtime and content catalog.
    fn runtime_catalog_revision(&self, owner_key: &str) -> Result<u64, StorageError> {
        let _ = owner_key;
        Err(StorageError::Failed(
            "runtime resources are unavailable in this store".into(),
        ))
    }

    /// Upserts one binding by owner/game/installation/content/runtime identity, revision-checked.
    /// If the stable content locator changes, update all sibling runtime bindings for that content
    /// atomically and advance their revisions too.
    fn save_runtime_content_binding(
        &self,
        owner_key: &str,
        binding: &super::runtime::RuntimeContentBinding,
    ) -> Result<super::runtime::RuntimeMutation, StorageError> {
        let _ = (owner_key, binding);
        Err(StorageError::Failed(
            "runtime bindings are unavailable in this store".into(),
        ))
    }

    fn remove_runtime_content_binding(
        &self,
        owner_key: &str,
        binding: &super::runtime::RuntimeContentBinding,
    ) -> Result<super::runtime::RuntimeMutation, StorageError> {
        let _ = (owner_key, binding);
        Err(StorageError::Failed(
            "runtime bindings are unavailable in this store".into(),
        ))
    }

    /// Applies an owned or shared catalog result for `provider`, replacing only the fact sets the
    /// update covers and leaving installed facts, matches and covers untouched.
    fn save_catalog(&self, provider: &str, update: &CatalogUpdate) -> Result<(), StorageError>;

    /// Clears the owned and shared facts and catalog sync state of `provider` without deleting
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

    /// Inserts or updates one saved view of the account and returns its database id. A view with
    /// `id == 0` is inserted; a positive `position` places it there, otherwise it goes after the
    /// account's last view. An existing id is renamed or updated in place and keeps its creation
    /// time. Names are unique per account.
    fn save_view(&self, account_id: &str, view: &SavedView) -> Result<i64, StorageError>;

    /// Deletes one saved view of the account. A missing id is not an error.
    fn delete_view(&self, account_id: &str, id: i64) -> Result<(), StorageError>;

    /// Hides or reveals `keys` for the account in one transaction. `now` is unix seconds and is
    /// recorded when an entry is hidden.
    fn set_hidden(
        &self,
        account_id: &str,
        keys: &[String],
        hidden: bool,
        now: i64,
    ) -> Result<(), StorageError>;

    /// Replaces the account's complete GameNode library read in one transaction: entries,
    /// collections and the completion time. A failed insertion leaves the previous cache exactly
    /// as it was.
    fn replace_account_library(
        &self,
        account_id: &str,
        entries: &[AccountEntry],
        collections: &[Collection],
        read_at: i64,
    ) -> Result<(), StorageError>;

    /// Upserts (`Some`) or removes (`None`) one game of an existing complete read. It never
    /// creates or clears the completion marker: a game learned before a complete read cannot make
    /// the cache look complete.
    fn save_account_entry(
        &self,
        account_id: &str,
        game_id: u64,
        entry: Option<&AccountEntry>,
    ) -> Result<(), StorageError>;

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

    /// The opaque installation ids of the entries in `entry_keys`, as `(entry key, id)` pairs.
    /// A key without an id is given a new random one in the same transaction, so every returned
    /// id is durable and never changes for its key. Keys are not checked against the entries.
    fn installation_ids(
        &self,
        entry_keys: &[String],
    ) -> Result<Vec<(String, String)>, StorageError>;

    /// Sets (`Some`) or clears (`None`) the display-name override of `entry_key` if its current
    /// revision is `expected_revision` (0 when it was never written), in one transaction.
    fn save_display_name(
        &self,
        entry_key: &str,
        expected_revision: u64,
        display_name: Option<&str>,
    ) -> Result<DisplayWrite, StorageError>;

    /// Sets (`Some`) or clears (`None`) the artwork override of `entry_key` if its current
    /// revision is `expected_revision` (0 when it was never written), in one transaction.
    fn save_artwork(
        &self,
        entry_key: &str,
        expected_revision: u64,
        file: Option<&str>,
    ) -> Result<ArtworkWrite, StorageError>;

    fn load_last_account(&self) -> Result<Option<LastAccount>, StorageError>;
    fn save_last_account(&self, account: &LastAccount) -> Result<(), StorageError>;
    /// Removes the last-account record. The cache and chosen matches stay for a later sign-in.
    fn clear_last_account(&self) -> Result<(), StorageError>;
}
