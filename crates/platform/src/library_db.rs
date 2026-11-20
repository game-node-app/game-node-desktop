//! SQLite repository for the library cache, the remembered layout and the last signed-in account.
//!
//! It shares the connection of [`SqliteSettingsStore`](crate::db::SqliteSettingsStore), so every
//! write is ordered with settings writes. Nothing here stores a credential or callback URL.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use gamenode_core::game_api::{AccountEntry, Collection};
use gamenode_core::library::{
    AccountLibrary, ArtworkOverride, ArtworkWrite, AutoMatch, CatalogError, CatalogRecord,
    CatalogUpdate, ChoiceSource, ChosenMatch, DisplayOverride, DisplayWrite, EntryStatus,
    FavoriteRecord, GameRecord, LastAccount, LaunchRecipeSelection, LaunchRecipeSelectionWrite,
    LibraryCache, LibraryLayout, LibraryStore, ProviderScanState, RuntimeContentBinding,
    RuntimeMutation, RuntimeResource, SavedView, SavedViewPayload, StoreAccount, StoredEntry,
    entry_key,
};
use gamenode_core::ports::StorageError;
use gamenode_core::provider::Record;
use rusqlite::{Connection, OptionalExtension, params};

pub(crate) const KEY_LAYOUT: &str = "library.layout";
pub(crate) const KEY_EXCLUDED: &str = "library.excluded";
pub(crate) const KEY_ACCOUNT_ID: &str = "account.last_id";
pub(crate) const KEY_ACCOUNT_NAME: &str = "account.last_name";

pub struct SqliteLibraryStore {
    conn: Arc<Mutex<Connection>>,
}

fn failed(error: impl std::fmt::Display) -> StorageError {
    StorageError::Failed(error.to_string())
}

fn db_id(game_id: u64) -> Result<i64, StorageError> {
    i64::try_from(game_id).map_err(failed)
}

fn game_id(raw: i64) -> Option<u64> {
    u64::try_from(raw).ok()
}

fn bump_runtime_catalog_revision(
    tx: &rusqlite::Transaction<'_>,
    owner_key: &str,
) -> Result<u64, StorageError> {
    tx.execute(
        "INSERT INTO plugin_runtime_catalog_state (owner_key, catalog_revision)
         VALUES (?1, 1)
         ON CONFLICT(owner_key) DO UPDATE SET
            catalog_revision = plugin_runtime_catalog_state.catalog_revision + 1",
        [owner_key],
    )
    .map_err(failed)?;
    tx.query_row(
        "SELECT catalog_revision FROM plugin_runtime_catalog_state WHERE owner_key = ?1",
        [owner_key],
        |row| row.get::<_, i64>(0),
    )
    .map_err(failed)
    .and_then(|revision| u64::try_from(revision).map_err(failed))
}

impl SqliteLibraryStore {
    pub(crate) fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        crate::db::lock(&self.conn)
    }
}

fn read_setting(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
        row.get(0)
    })
    .optional()
}

fn write_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map(drop)
}

/// Reads one catalog flag. Anything that is not a stored `0`/`1` is treated as false so a
/// damaged row cannot make a game look owned.
fn read_flag(value: Option<i64>, key: &str, column: &str) -> bool {
    match value {
        Some(0) | None => false,
        Some(1) => true,
        Some(other) => {
            tracing::warn!(
                key,
                column,
                value = other,
                "ignored an unreadable entry flag"
            );
            false
        }
    }
}

fn read_catalog_error(provider: &str, column: &str, value: Option<String>) -> Option<CatalogError> {
    let value = value?;
    match CatalogError::from_category(&value) {
        Some(error) => Some(error),
        None => {
            tracing::warn!(
                provider,
                column,
                "ignored an unknown catalog error category"
            );
            None
        }
    }
}

fn load_entries(conn: &Connection) -> rusqlite::Result<Vec<StoredEntry>> {
    let mut stmt = conn.prepare(
        "SELECT key, provider, store_id, name, install_dir, fields_json, installed, owned,
                family_shared, first_seen, last_seen, target_json
         FROM library_entries ORDER BY key",
    )?;
    let rows = stmt.query_map([], |row| {
        let key: String = row.get(0)?;
        let fields_json: String = row.get(5)?;
        let fields: BTreeMap<String, String> = serde_json::from_str(&fields_json)
            .inspect_err(|error| tracing::warn!(%error, "ignored unreadable entry fields"))
            .unwrap_or_default();
        Ok(StoredEntry {
            owned: read_flag(row.get(7)?, &key, "owned"),
            // The SQL column keeps its historical `family_shared` name; no migration is needed.
            shared: read_flag(row.get(8)?, &key, "family_shared"),
            key,
            provider: row.get(1)?,
            store_id: row.get(2)?,
            name: row.get(3)?,
            install_dir: row.get(4)?,
            fields,
            installed: row.get::<_, i64>(6)? != 0,
            target: read_target(row.get::<_, Option<String>>(11)?),
            first_seen: row.get(9)?,
            last_seen: row.get(10)?,
        })
    })?;
    rows.collect()
}

/// A stored detected target, or `None` when the entry has none. An unreadable value is dropped
/// with a warning: the entry stays usable and the next scan registers the target again.
fn read_target(value: Option<String>) -> Option<gamenode_core::library::DetectedTarget> {
    let value = value?;
    match serde_json::from_str(&value) {
        Ok(target) => Some(target),
        Err(error) => {
            tracing::warn!(%error, "ignored an unreadable detected target");
            None
        }
    }
}

/// Which catalog fact set a phase covers. The SQL columns keep their historical
/// `family_shared` / `family_error` / `last_family_at` names; the values are provider-neutral.
#[derive(Clone, Copy)]
enum CatalogFact {
    Owned,
    Shared,
}

/// Applies one successful catalog phase inside the caller's transaction. The rules mirror
/// `LibraryMachine::merge_catalog`: the phase's own fact set is replaced, installed facts,
/// `install_dir`, first-seen time, matches and covers are untouched, the installed name wins, and
/// an existing non-empty name is only replaced when it is empty. Catalog fields only fill keys the
/// entry does not have yet, so an installed scan's fields are never erased.
fn apply_catalog_records(
    tx: &rusqlite::Transaction<'_>,
    provider: &str,
    records: &[CatalogRecord],
    now: i64,
    fact: CatalogFact,
) -> Result<(), StorageError> {
    const UPSERT_OWNED: &str = "INSERT INTO library_entries
            (key, provider, store_id, name, install_dir, fields_json, installed, owned,
             family_shared, first_seen, last_seen)
         VALUES (?1, ?2, ?3, ?4, '', ?6, 0, 1, 0, ?5, ?5)
         ON CONFLICT(key) DO UPDATE SET
            name = CASE
                WHEN library_entries.installed THEN library_entries.name
                WHEN library_entries.name IS NOT NULL
                     AND trim(library_entries.name) <> '' THEN library_entries.name
                ELSE excluded.name
            END,
            fields_json = excluded.fields_json,
            owned = 1,
            last_seen = excluded.last_seen";
    // The column keeps its historical family name; the fact it stores is now called shared.
    const UPSERT_SHARED: &str = "INSERT INTO library_entries
            (key, provider, store_id, name, install_dir, fields_json, installed, owned,
             family_shared, first_seen, last_seen)
         VALUES (?1, ?2, ?3, ?4, '', ?6, 0, 0, 1, ?5, ?5)
         ON CONFLICT(key) DO UPDATE SET
            name = CASE
                WHEN library_entries.installed THEN library_entries.name
                WHEN library_entries.name IS NOT NULL
                     AND trim(library_entries.name) <> '' THEN library_entries.name
                ELSE excluded.name
            END,
            fields_json = excluded.fields_json,
            family_shared = CASE WHEN library_entries.owned THEN 0 ELSE 1 END,
            last_seen = excluded.last_seen";

    let mut seen = BTreeSet::new();
    let mut dropped = 0usize;
    let mut upsert = tx
        .prepare(match fact {
            CatalogFact::Owned => UPSERT_OWNED,
            CatalogFact::Shared => UPSERT_SHARED,
        })
        .map_err(failed)?;
    for record in records {
        let id = record.id.trim();
        let name = record.name.trim();
        if id.is_empty() || name.is_empty() || !seen.insert(id.to_owned()) {
            dropped += 1;
            continue;
        }
        let key = entry_key(provider, id);
        let fields = merge_catalog_fields(tx, &key, &record.fields)?;
        upsert
            .execute(params![key, provider, id, name, now, fields])
            .map_err(failed)?;
    }
    if dropped > 0 {
        tracing::info!(
            provider,
            dropped,
            "ignored unusable catalog records (empty id/name or duplicate)"
        );
    }
    Ok(())
}

/// Fills the entry's missing field keys with the catalog's, keeping installed discovery values.
fn merge_catalog_fields(
    tx: &rusqlite::Transaction<'_>,
    key: &str,
    incoming: &BTreeMap<String, String>,
) -> Result<String, StorageError> {
    let existing: Option<String> = tx
        .query_row(
            "SELECT fields_json FROM library_entries WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .optional()
        .map_err(failed)?;
    let mut fields: BTreeMap<String, String> = existing
        .and_then(|json| {
            serde_json::from_str(&json)
                .inspect_err(|error| tracing::warn!(%error, key, "ignored unreadable entry fields"))
                .ok()
        })
        .unwrap_or_default();
    for (name, value) in incoming {
        fields.entry(name.clone()).or_insert_with(|| value.clone());
    }
    serde_json::to_string(&fields).map_err(failed)
}

fn save_catalog_fact(
    tx: &rusqlite::Transaction<'_>,
    provider: &str,
    result: &Result<Vec<CatalogRecord>, CatalogError>,
    now: i64,
    fact: CatalogFact,
) -> Result<(), StorageError> {
    match result {
        Ok(records) => {
            let clear = match fact {
                CatalogFact::Owned => "UPDATE library_entries SET owned = 0 WHERE provider = ?1",
                CatalogFact::Shared => {
                    "UPDATE library_entries SET family_shared = 0 WHERE provider = ?1"
                }
            };
            tx.execute(clear, [provider]).map_err(failed)?;
            apply_catalog_records(tx, provider, records, now, fact)?;
            let success = match fact {
                CatalogFact::Owned => {
                    "INSERT INTO provider_scans (provider, last_catalog_at, catalog_error)
                     VALUES (?1, ?2, NULL)
                     ON CONFLICT(provider) DO UPDATE SET
                        last_catalog_at = excluded.last_catalog_at, catalog_error = NULL"
                }
                CatalogFact::Shared => {
                    "INSERT INTO provider_scans (provider, last_family_at, family_error)
                     VALUES (?1, ?2, NULL)
                     ON CONFLICT(provider) DO UPDATE SET
                        last_family_at = excluded.last_family_at, family_error = NULL"
                }
            };
            tx.execute(success, params![provider, now])
                .map(drop)
                .map_err(failed)
        }
        Err(error) => {
            let failure = match fact {
                CatalogFact::Owned => {
                    "INSERT INTO provider_scans (provider, catalog_error) VALUES (?1, ?2)
                     ON CONFLICT(provider) DO UPDATE SET catalog_error = excluded.catalog_error"
                }
                CatalogFact::Shared => {
                    "INSERT INTO provider_scans (provider, family_error) VALUES (?1, ?2)
                     ON CONFLICT(provider) DO UPDATE SET family_error = excluded.family_error"
                }
            };
            tx.execute(failure, params![provider, error.as_str()])
                .map(drop)
                .map_err(failed)
        }
    }
}

fn load_auto_matches(conn: &Connection) -> rusqlite::Result<Vec<AutoMatch>> {
    let mut stmt = conn.prepare(
        "SELECT category, uid, game_ids_json, checked_at FROM auto_matches ORDER BY category, uid",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, u32>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;
    let mut matches = Vec::new();
    for row in rows {
        let (category, uid, ids_json, checked_at) = row?;
        // An unreadable row is dropped, not read as "no game": the next lookup replaces it.
        let Ok(game_ids) = serde_json::from_str::<Vec<u64>>(&ids_json) else {
            tracing::warn!(category, "ignored an unreadable cached match");
            continue;
        };
        matches.push(AutoMatch {
            category,
            uid,
            game_ids,
            checked_at,
        });
    }
    Ok(matches)
}

/// Reads a stored status. A value this build does not know means "in the library, status
/// unknown" and is kept as `None` rather than dropping the entry.
fn read_status(value: Option<String>) -> Option<EntryStatus> {
    value.and_then(|name| EntryStatus::from_api_name(&name))
}

fn read_collection_ids(raw: &str, game: u64) -> Vec<String> {
    serde_json::from_str(raw)
        .inspect_err(
            |error| tracing::warn!(%error, game, "ignored unreadable account collection ids"),
        )
        .unwrap_or_default()
}

fn load_saved_views(conn: &Connection, account_id: &str) -> rusqlite::Result<Vec<SavedView>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, payload, position, created_at FROM saved_views
         WHERE account_id = ?1 ORDER BY position, id",
    )?;
    let rows = stmt.query_map([account_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
        ))
    })?;
    let mut views = Vec::new();
    for row in rows {
        let (id, name, payload_json, position, created_at) = row?;
        // A damaged payload cannot be applied; it is dropped so the sidebar has no broken item.
        let Ok(payload) = serde_json::from_str::<SavedViewPayload>(&payload_json) else {
            tracing::warn!(id, "ignored an unreadable saved view");
            continue;
        };
        views.push(SavedView {
            id,
            name,
            payload,
            position,
            created_at,
        });
    }
    Ok(views)
}

fn load_hidden(conn: &Connection, account_id: &str) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT entry_key FROM hidden_entries WHERE account_id = ?1 ORDER BY entry_key")?;
    stmt.query_map([account_id], |row| row.get(0))?.collect()
}

/// Reads the account's last complete read. Entries or collections written before a complete read
/// are not exposed: without the read marker there is no complete library to show.
fn load_account_library(
    conn: &Connection,
    account_id: &str,
) -> rusqlite::Result<Option<AccountLibrary>> {
    let read_at: Option<i64> = conn
        .query_row(
            "SELECT read_at FROM account_library_reads WHERE account_id = ?1",
            [account_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(read_at) = read_at else {
        return Ok(None);
    };

    let mut collections_stmt = conn.prepare(
        "SELECT collection_id, name, default_status FROM account_collections
         WHERE account_id = ?1 ORDER BY position, collection_id",
    )?;
    let collections = collections_stmt
        .query_map([account_id], |row| {
            Ok(Collection {
                id: row.get(0)?,
                name: row.get(1)?,
                default_status: read_status(row.get::<_, Option<String>>(2)?),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut entries_stmt = conn.prepare(
        "SELECT game_id, entry_id, status, collection_ids FROM account_library_entries
         WHERE account_id = ?1 ORDER BY game_id",
    )?;
    let rows = entries_stmt.query_map([account_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    let mut entries = BTreeMap::new();
    for row in rows {
        let (raw_game_id, id, status, collection_ids) = row?;
        let Some(game_id) = game_id(raw_game_id) else {
            continue;
        };
        entries.insert(
            game_id,
            AccountEntry {
                id,
                game_id,
                status: read_status(status),
                collection_ids: read_collection_ids(&collection_ids, game_id),
            },
        );
    }

    Ok(Some(AccountLibrary {
        entries,
        collections,
        read_at,
    }))
}

impl LibraryStore for SqliteLibraryStore {
    fn load_cache(&self, account_id: Option<&str>) -> Result<LibraryCache, StorageError> {
        let conn = self.conn();
        let entries = load_entries(&conn).map_err(failed)?;

        let scans = conn
            .prepare(
                "SELECT provider, last_success_at, last_error, last_catalog_at, catalog_error,
                        last_family_at, family_error
                 FROM provider_scans ORDER BY provider",
            )
            .map_err(failed)?
            .query_map([], |row| {
                let provider: String = row.get(0)?;
                Ok(ProviderScanState {
                    last_catalog_at: row.get(3)?,
                    catalog_error: read_catalog_error(&provider, "catalog_error", row.get(4)?),
                    // The columns keep their historical family names; the fields are shared.
                    last_shared_at: row.get(5)?,
                    shared_error: read_catalog_error(&provider, "family_error", row.get(6)?),
                    provider,
                    last_success_at: row.get(1)?,
                    last_error: row.get(2)?,
                    // The scan location is live data reported with the scan; it is not stored.
                    install_root: None,
                    library_folders: None,
                    last_detected: None,
                })
            })
            .map_err(failed)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(failed)?;

        let auto_matches = load_auto_matches(&conn).map_err(failed)?;

        let favorites = match account_id {
            None => Vec::new(),
            Some(account) => conn
                .prepare(
                    "SELECT account_id, game_id, is_favorite, pending, updated_at
                     FROM favorites WHERE account_id = ?1 ORDER BY game_id",
                )
                .map_err(failed)?
                .query_map([account], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, bool>(2)?,
                        row.get::<_, bool>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                })
                .map_err(failed)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(failed)?
                .into_iter()
                .filter_map(
                    |(account_id, raw_game_id, is_favorite, pending, updated_at)| {
                        Some(FavoriteRecord {
                            account_id,
                            game_id: game_id(raw_game_id)?,
                            is_favorite,
                            pending,
                            updated_at,
                        })
                    },
                )
                .collect(),
        };

        let chosen_matches = match account_id {
            None => Vec::new(),
            Some(account) => conn
                .prepare(
                    "SELECT entry_key, game_id, chosen_at, source FROM chosen_matches
                     WHERE account_id = ?1 ORDER BY entry_key",
                )
                .map_err(failed)?
                .query_map([account], |row| {
                    Ok(ChosenMatch {
                        entry_key: row.get(0)?,
                        game_id: row.get::<_, Option<i64>>(1)?.and_then(game_id),
                        chosen_at: row.get(2)?,
                        // The column is constrained to the known values.
                        source: ChoiceSource::parse(&row.get::<_, String>(3)?).unwrap_or_default(),
                    })
                })
                .map_err(failed)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(failed)?,
        };

        let games = conn
            .prepare(
                "SELECT game_id, name, release_year, cover_url, cover_file, cover_failed_at,
                        platforms
                 FROM games ORDER BY game_id",
            )
            .map_err(failed)?
            .query_map([], |row| {
                let platforms_json: String = row.get(6)?;
                let platforms: Vec<String> = serde_json::from_str(&platforms_json)
                    .inspect_err(
                        |error| tracing::warn!(%error, "ignored unreadable game platforms"),
                    )
                    .unwrap_or_default();
                Ok((
                    row.get::<_, i64>(0)?,
                    GameRecord {
                        game_id: 0,
                        name: row.get(1)?,
                        release_year: row.get(2)?,
                        cover_url: row.get(3)?,
                        cover_file: row.get(4)?,
                        cover_failed_at: row.get(5)?,
                        platforms,
                    },
                ))
            })
            .map_err(failed)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(failed)?
            .into_iter()
            .filter_map(|(id, game)| {
                Some(GameRecord {
                    game_id: game_id(id)?,
                    ..game
                })
            })
            .collect();

        let excluded = read_setting(&conn, KEY_EXCLUDED)
            .map_err(failed)?
            .and_then(|json| {
                serde_json::from_str(&json)
                    .inspect_err(
                        |error| tracing::warn!(%error, "ignored an unreadable excluded list"),
                    )
                    .ok()
            })
            .unwrap_or_default();

        let display_overrides = conn
            .prepare("SELECT entry_key, display_name, revision FROM library_display_overrides ORDER BY entry_key")
            .map_err(failed)?
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, i64>(2)?)))
            .map_err(failed)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(failed)?
            .into_iter()
            .filter_map(|(entry_key, display_name, revision)| Some(DisplayOverride { entry_key, display_name, revision: u64::try_from(revision).ok()? }))
            .collect();
        let artwork_overrides = conn
            .prepare("SELECT entry_key, file, revision FROM library_artwork_overrides ORDER BY entry_key")
            .map_err(failed)?
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, i64>(2)?)))
            .map_err(failed)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(failed)?
            .into_iter()
            .filter_map(|(entry_key, file, revision)| Some(ArtworkOverride { entry_key, file, revision: u64::try_from(revision).ok()? }))
            .collect();
        let (saved_views, hidden, account_library) = match account_id {
            None => (Vec::new(), Vec::new(), None),
            Some(account) => (
                load_saved_views(&conn, account).map_err(failed)?,
                load_hidden(&conn, account).map_err(failed)?,
                load_account_library(&conn, account).map_err(failed)?,
            ),
        };

        Ok(LibraryCache {
            entries,
            scans,
            auto_matches,
            chosen_matches,
            favorites,
            games,
            excluded,
            display_overrides,
            artwork_overrides,
            saved_views,
            hidden,
            account_library,
        })
    }

    fn save_scan(
        &self,
        provider: &str,
        records: &[Record],
        targets: &BTreeMap<String, gamenode_core::library::TargetSpec>,
        now: i64,
    ) -> Result<(), StorageError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        // Targets the scan does not report stay stored but unavailable, so a later scan can offer
        // repair and an unavailable target can never launch.
        let existing = {
            let mut stmt = tx
                .prepare(
                    "SELECT key, target_json FROM library_entries
                     WHERE provider = ?1 AND target_json IS NOT NULL",
                )
                .map_err(failed)?;
            let rows = stmt
                .query_map([provider], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(failed)?;
            rows.collect::<rusqlite::Result<Vec<_>>>().map_err(failed)?
        };
        let reported: BTreeSet<String> = records
            .iter()
            .filter(|record| targets.contains_key(&record.id))
            .map(|record| entry_key(provider, &record.id))
            .collect();
        tx.execute(
            "UPDATE library_entries SET installed = 0 WHERE provider = ?1",
            [provider],
        )
        .map_err(failed)?;
        {
            let mut upsert = tx
                .prepare(
                    "INSERT INTO library_entries
                        (key, provider, store_id, name, install_dir, fields_json, installed, first_seen, last_seen, target_json)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?7, ?8)
                     ON CONFLICT(key) DO UPDATE SET
                        name = excluded.name,
                        install_dir = excluded.install_dir,
                        fields_json = excluded.fields_json,
                        installed = 1,
                        last_seen = excluded.last_seen,
                        target_json = excluded.target_json",
                )
                .map_err(failed)?;
            let existing_targets: BTreeMap<&str, &str> = existing
                .iter()
                .map(|(key, json)| (key.as_str(), json.as_str()))
                .collect();
            for record in records {
                let fields = serde_json::to_string(&record.fields).map_err(failed)?;
                let key = entry_key(provider, &record.id);
                let prior = existing_targets
                    .get(key.as_str())
                    .and_then(|json| serde_json::from_str(json).ok());
                let target = match targets.get(&record.id) {
                    Some(spec) => Some(gamenode_core::library::DetectedTarget::reported(
                        prior.as_ref(),
                        spec.clone(),
                        now,
                    )),
                    None => prior
                        .as_ref()
                        .map(|target| gamenode_core::library::DetectedTarget::missing(target, now)),
                };
                let target_json = target
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()
                    .map_err(failed)?;
                upsert
                    .execute(params![
                        key,
                        provider,
                        record.id,
                        record.name,
                        record.install_dir,
                        fields,
                        now,
                        target_json
                    ])
                    .map_err(failed)?;
            }
        }
        {
            let mut clear = tx
                .prepare(
                    "UPDATE library_entries SET target_json = ?1
                     WHERE key = ?2 AND target_json IS NOT NULL",
                )
                .map_err(failed)?;
            for (key, json) in &existing {
                if reported.contains(key) {
                    continue;
                }
                let Ok(target) =
                    serde_json::from_str::<gamenode_core::library::DetectedTarget>(json)
                else {
                    continue;
                };
                let missing = gamenode_core::library::DetectedTarget::missing(&target, now);
                clear
                    .execute(params![
                        serde_json::to_string(&missing).map_err(failed)?,
                        key
                    ])
                    .map_err(failed)?;
            }
        }
        tx.execute(
            "INSERT INTO provider_scans (provider, last_success_at, last_error) VALUES (?1, ?2, NULL)
             ON CONFLICT(provider) DO UPDATE SET last_success_at = excluded.last_success_at, last_error = NULL",
            params![provider, now],
        )
        .map_err(failed)?;
        tx.commit().map_err(failed)
    }

    fn save_target_review(
        &self,
        entry_key: &str,
        expected_revision: u64,
        replacement: &gamenode_core::library::DetectedTarget,
    ) -> Result<gamenode_core::library::TargetApproval, StorageError> {
        use gamenode_core::library::TargetApproval;
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        let stored: Option<Option<String>> = tx
            .query_row(
                "SELECT target_json FROM library_entries WHERE key = ?1",
                [entry_key],
                |row| row.get(0),
            )
            .optional()
            .map_err(failed)?;
        let Some(Some(json)) = stored else {
            return Ok(TargetApproval::Missing);
        };
        let Ok(target) = serde_json::from_str::<gamenode_core::library::DetectedTarget>(&json)
        else {
            return Ok(TargetApproval::Missing);
        };
        if target.revision != expected_revision {
            return Ok(TargetApproval::Conflict {
                current: target.revision,
            });
        }
        if replacement.revision < target.revision
            || replacement.revision > target.revision.saturating_add(1)
            || (replacement.spec != target.spec && replacement.revision == target.revision)
            || !replacement.spec.validate_shape()
        {
            return Ok(TargetApproval::Conflict {
                current: target.revision,
            });
        }
        tx.execute(
            "UPDATE library_entries SET target_json = ?1 WHERE key = ?2",
            params![
                serde_json::to_string(replacement).map_err(failed)?,
                entry_key
            ],
        )
        .map_err(failed)?;
        tx.commit().map_err(failed)?;
        Ok(TargetApproval::Applied {
            revision: replacement.revision,
        })
    }

    fn custom_launch_recipe(
        &self,
        owner_key: &str,
        recipe_id: &str,
    ) -> Result<Option<gamenode_core::library::CustomLaunchRecipe>, StorageError> {
        let conn = self.conn();
        let json: Option<String> = conn
            .query_row(
                "SELECT recipe_json FROM plugin_custom_launch_recipes
                 WHERE owner_key = ?1 AND recipe_id = ?2",
                params![owner_key, recipe_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(failed)?;
        json.map(|json| serde_json::from_str(&json).map_err(failed))
            .transpose()
    }

    fn save_custom_launch_recipe(
        &self,
        owner_key: &str,
        recipe: &gamenode_core::library::CustomLaunchRecipe,
    ) -> Result<gamenode_core::library::CustomLaunchMutation, StorageError> {
        use gamenode_core::library::CustomLaunchMutation;
        if owner_key.is_empty() || !recipe.validate() {
            return Err(failed("invalid custom launch recipe"));
        }
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        let current: Option<u64> = tx
            .query_row(
                "SELECT revision FROM plugin_custom_launch_recipes
                 WHERE owner_key = ?1 AND recipe_id = ?2",
                params![owner_key, recipe.recipe_id],
                |row| row.get::<_, i64>(0).map(|value| value as u64),
            )
            .optional()
            .map_err(failed)?;
        let expected = match current {
            Some(revision) => revision
                .checked_add(1)
                .ok_or_else(|| failed("custom launch recipe revision is exhausted"))?,
            None => 1,
        };
        if recipe.revision != expected {
            return Ok(match current {
                Some(current) => CustomLaunchMutation::Conflict { current },
                None => CustomLaunchMutation::Conflict { current: 0 },
            });
        }
        tx.execute(
            "INSERT INTO plugin_custom_launch_recipes
                (owner_key, recipe_id, installation_id, revision, recipe_json)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(owner_key, recipe_id) DO UPDATE SET
                installation_id = excluded.installation_id,
                revision = excluded.revision,
                recipe_json = excluded.recipe_json",
            params![
                owner_key,
                recipe.recipe_id,
                recipe.installation_id,
                i64::try_from(recipe.revision).map_err(failed)?,
                serde_json::to_string(recipe).map_err(failed)?,
            ],
        )
        .map_err(failed)?;
        tx.commit().map_err(failed)?;
        Ok(CustomLaunchMutation::Applied {
            revision: recipe.revision,
        })
    }

    fn remove_custom_launch_recipe(
        &self,
        owner_key: &str,
        recipe_id: &str,
        expected_revision: u64,
    ) -> Result<gamenode_core::library::CustomLaunchMutation, StorageError> {
        use gamenode_core::library::CustomLaunchMutation;
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        let current: Option<u64> = tx
            .query_row(
                "SELECT revision FROM plugin_custom_launch_recipes
                 WHERE owner_key = ?1 AND recipe_id = ?2",
                params![owner_key, recipe_id],
                |row| row.get::<_, i64>(0).map(|value| value as u64),
            )
            .optional()
            .map_err(failed)?;
        let Some(current) = current else {
            return Ok(CustomLaunchMutation::Missing);
        };
        if current != expected_revision {
            return Ok(CustomLaunchMutation::Conflict { current });
        }
        tx.execute(
            "DELETE FROM plugin_custom_launch_recipes
             WHERE owner_key = ?1 AND recipe_id = ?2 AND revision = ?3",
            params![
                owner_key,
                recipe_id,
                i64::try_from(expected_revision).map_err(failed)?
            ],
        )
        .map_err(failed)?;
        tx.commit().map_err(failed)?;
        Ok(CustomLaunchMutation::Applied {
            revision: expected_revision,
        })
    }

    fn custom_launch_recipes(
        &self,
        owner_key: &str,
        game_id: u64,
    ) -> Result<Vec<gamenode_core::library::CustomLaunchRecipe>, StorageError> {
        let conn = self.conn();
        let mut statement = conn
            .prepare(
                "SELECT recipe_json FROM plugin_custom_launch_recipes
                 WHERE owner_key = ?1 ORDER BY recipe_id",
            )
            .map_err(failed)?;
        let rows = statement
            .query_map([owner_key], |row| row.get::<_, String>(0))
            .map_err(failed)?;
        rows.map(|row| {
            let json = row.map_err(failed)?;
            let recipe: gamenode_core::library::CustomLaunchRecipe =
                serde_json::from_str(&json).map_err(failed)?;
            Ok((recipe.game_id == game_id).then_some(recipe))
        })
        .collect::<Result<Vec<_>, StorageError>>()
        .map(|recipes| recipes.into_iter().flatten().collect())
    }

    fn launch_recipe_selection(
        &self,
        entry_key: &str,
    ) -> Result<Option<LaunchRecipeSelection>, StorageError> {
        self.conn()
            .query_row(
                "SELECT entry_key, owner_key, plugin_id, publisher_fingerprint, recipe_id,
                        recipe_revision, base_target_revision, approved_plan_fingerprint
                 FROM library_launch_recipe_selection WHERE entry_key = ?1",
                [entry_key],
                |row| {
                    Ok(LaunchRecipeSelection {
                        entry_key: row.get(0)?,
                        owner_key: row.get(1)?,
                        plugin_id: row.get(2)?,
                        publisher_fingerprint: row.get(3)?,
                        recipe_id: row.get(4)?,
                        recipe_revision: row.get::<_, i64>(5)? as u64,
                        base_target_revision: row.get::<_, i64>(6)? as u64,
                        approved_plan_fingerprint: row.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(failed)
    }

    fn save_launch_recipe_selection(
        &self,
        selection: &LaunchRecipeSelection,
        expected_target_revision: u64,
    ) -> Result<LaunchRecipeSelectionWrite, StorageError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        let entry: Option<(String, Option<String>)> = tx
            .query_row(
                "SELECT li.installation_id, le.target_json
                 FROM library_installations li
                 JOIN library_entries le ON le.key = li.entry_key
                 WHERE li.entry_key = ?1",
                [&selection.entry_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(failed)?;
        let Some((installation_id, target_json)) = entry else {
            return Ok(LaunchRecipeSelectionWrite::RecipeMissing);
        };
        let Some(target_json) = target_json else {
            return Ok(LaunchRecipeSelectionWrite::Conflict);
        };
        let target: gamenode_core::library::DetectedTarget =
            serde_json::from_str(&target_json).map_err(failed)?;
        if target.revision != expected_target_revision
            || selection.base_target_revision != expected_target_revision
            || selection.entry_key.is_empty()
            || selection.owner_key.is_empty()
            || selection.plugin_id.is_empty()
            || selection.publisher_fingerprint.is_empty()
            || selection.recipe_id.is_empty()
            || selection.recipe_revision == 0
        {
            return Ok(LaunchRecipeSelectionWrite::Conflict);
        }
        let recipe_json: Option<String> = tx
            .query_row(
                "SELECT recipe_json FROM plugin_custom_launch_recipes
                 WHERE owner_key = ?1 AND recipe_id = ?2 AND installation_id = ?3
                       AND revision = ?4",
                params![
                    selection.owner_key,
                    selection.recipe_id,
                    installation_id,
                    selection.recipe_revision as i64,
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(failed)?;
        let Some(recipe_json) = recipe_json else {
            return Ok(LaunchRecipeSelectionWrite::RecipeMissing);
        };
        let recipe: gamenode_core::library::CustomLaunchRecipe =
            serde_json::from_str(&recipe_json).map_err(failed)?;
        if recipe.installation_id != installation_id
            || recipe.base_target_revision != expected_target_revision
            || !recipe.validate()
        {
            return Ok(LaunchRecipeSelectionWrite::Conflict);
        }
        tx.execute(
            "INSERT INTO library_launch_recipe_selection
                 (entry_key, owner_key, plugin_id, publisher_fingerprint, recipe_id,
                  recipe_revision, base_target_revision, approved_plan_fingerprint)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '')
             ON CONFLICT(entry_key) DO UPDATE SET
                 owner_key = excluded.owner_key,
                 plugin_id = excluded.plugin_id,
                 publisher_fingerprint = excluded.publisher_fingerprint,
                 recipe_id = excluded.recipe_id,
                 recipe_revision = excluded.recipe_revision,
                 base_target_revision = excluded.base_target_revision,
                 approved_plan_fingerprint = ''",
            params![
                selection.entry_key,
                selection.owner_key,
                selection.plugin_id,
                selection.publisher_fingerprint,
                selection.recipe_id,
                selection.recipe_revision as i64,
                selection.base_target_revision as i64,
            ],
        )
        .map_err(failed)?;
        tx.commit().map_err(failed)?;
        Ok(LaunchRecipeSelectionWrite::Applied)
    }

    fn clear_launch_recipe_selection(
        &self,
        entry_key: &str,
        expected_target_revision: u64,
    ) -> Result<LaunchRecipeSelectionWrite, StorageError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        let target_json: Option<Option<String>> = tx
            .query_row(
                "SELECT target_json FROM library_entries WHERE key = ?1",
                [entry_key],
                |row| row.get(0),
            )
            .optional()
            .map_err(failed)?;
        let Some(target_json) = target_json else {
            return Ok(LaunchRecipeSelectionWrite::RecipeMissing);
        };
        match target_json {
            Some(target_json) => {
                let target: gamenode_core::library::DetectedTarget =
                    serde_json::from_str(&target_json).map_err(failed)?;
                if target.revision != expected_target_revision {
                    return Ok(LaunchRecipeSelectionWrite::Conflict);
                }
            }
            None if expected_target_revision != 0 => {
                return Ok(LaunchRecipeSelectionWrite::Conflict);
            }
            None => {}
        }
        tx.execute(
            "DELETE FROM library_launch_recipe_selection WHERE entry_key = ?1",
            [entry_key],
        )
        .map_err(failed)?;
        tx.commit().map_err(failed)?;
        Ok(LaunchRecipeSelectionWrite::Applied)
    }

    fn approve_launch_recipe_plan(
        &self,
        entry_key: &str,
        recipe_id: &str,
        recipe_revision: u64,
        expected_target_revision: u64,
        fingerprint: &str,
    ) -> Result<LaunchRecipeSelectionWrite, StorageError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        let selection: Option<(u64, u64)> = tx
            .query_row(
                "SELECT recipe_revision, base_target_revision
                 FROM library_launch_recipe_selection
                 WHERE entry_key = ?1 AND recipe_id = ?2",
                params![entry_key, recipe_id],
                |row| Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as u64)),
            )
            .optional()
            .map_err(failed)?;
        let Some((current_recipe_revision, base_revision)) = selection else {
            return Ok(LaunchRecipeSelectionWrite::RecipeMissing);
        };
        let target_json: Option<String> = tx
            .query_row(
                "SELECT target_json FROM library_entries WHERE key = ?1",
                [entry_key],
                |row| row.get(0),
            )
            .optional()
            .map_err(failed)?
            .flatten();
        let Some(target_json) = target_json else {
            return Ok(LaunchRecipeSelectionWrite::RecipeMissing);
        };
        let target: gamenode_core::library::DetectedTarget =
            serde_json::from_str(&target_json).map_err(failed)?;
        if current_recipe_revision != recipe_revision
            || base_revision != expected_target_revision
            || target.revision != expected_target_revision
            || fingerprint.is_empty()
        {
            return Ok(LaunchRecipeSelectionWrite::Conflict);
        }
        tx.execute(
            "UPDATE library_launch_recipe_selection SET approved_plan_fingerprint = ?2
             WHERE entry_key = ?1",
            params![entry_key, fingerprint],
        )
        .map_err(failed)?;
        tx.commit().map_err(failed)?;
        Ok(LaunchRecipeSelectionWrite::Applied)
    }

    fn runtime_resources(&self, owner_key: &str) -> Result<Vec<RuntimeResource>, StorageError> {
        if owner_key.is_empty() {
            return Err(failed("runtime owner is required"));
        }
        let conn = self.conn();
        let mut statement = conn
            .prepare(
                "SELECT runtime_json FROM plugin_runtime_resources
                 WHERE owner_key = ?1 ORDER BY runtime_id",
            )
            .map_err(failed)?;
        let rows = statement
            .query_map([owner_key], |row| row.get::<_, String>(0))
            .map_err(failed)?;
        rows.map(|row| {
            let json = row.map_err(failed)?;
            let runtime: RuntimeResource = serde_json::from_str(&json).map_err(failed)?;
            if !runtime.validate() {
                return Err(failed("stored runtime resource is invalid"));
            }
            Ok(runtime)
        })
        .collect()
    }

    fn runtime_resource(
        &self,
        owner_key: &str,
        runtime_id: &str,
    ) -> Result<Option<RuntimeResource>, StorageError> {
        if owner_key.is_empty() || runtime_id.is_empty() {
            return Err(failed("runtime owner and id are required"));
        }
        let conn = self.conn();
        let json: Option<String> = conn
            .query_row(
                "SELECT runtime_json FROM plugin_runtime_resources
                 WHERE owner_key = ?1 AND runtime_id = ?2",
                params![owner_key, runtime_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(failed)?;
        json.map(|json| {
            let runtime: RuntimeResource = serde_json::from_str(&json).map_err(failed)?;
            if runtime.validate() {
                Ok(runtime)
            } else {
                Err(failed("stored runtime resource is invalid"))
            }
        })
        .transpose()
    }

    fn save_runtime_resource(
        &self,
        owner_key: &str,
        runtime: &RuntimeResource,
    ) -> Result<RuntimeMutation, StorageError> {
        if owner_key.is_empty() || !runtime.validate() {
            return Err(failed("invalid runtime resource"));
        }
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        let current: Option<u64> = tx
            .query_row(
                "SELECT revision FROM plugin_runtime_resources
                 WHERE owner_key = ?1 AND runtime_id = ?2",
                params![owner_key, runtime.id],
                |row| row.get::<_, i64>(0).map(|value| value as u64),
            )
            .optional()
            .map_err(failed)?;
        let expected = match current {
            Some(revision) => revision
                .checked_add(1)
                .ok_or_else(|| failed("runtime revision is exhausted"))?,
            None => 1,
        };
        if runtime.revision != expected {
            return Ok(match current {
                Some(current) => RuntimeMutation::Conflict { current },
                None => RuntimeMutation::Conflict { current: 0 },
            });
        }
        tx.execute(
            "INSERT INTO plugin_runtime_resources
                (owner_key, runtime_id, revision, runtime_json)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(owner_key, runtime_id) DO UPDATE SET
                revision = excluded.revision,
                runtime_json = excluded.runtime_json",
            params![
                owner_key,
                runtime.id,
                i64::try_from(runtime.revision).map_err(failed)?,
                serde_json::to_string(runtime).map_err(failed)?,
            ],
        )
        .map_err(failed)?;
        bump_runtime_catalog_revision(&tx, owner_key)?;
        tx.commit().map_err(failed)?;
        Ok(RuntimeMutation::Applied {
            revision: runtime.revision,
        })
    }

    fn remove_runtime_resource(
        &self,
        owner_key: &str,
        runtime_id: &str,
        expected_revision: u64,
    ) -> Result<RuntimeMutation, StorageError> {
        if owner_key.is_empty() || runtime_id.is_empty() || expected_revision == 0 {
            return Err(failed("invalid runtime removal"));
        }
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        let current: Option<u64> = tx
            .query_row(
                "SELECT revision FROM plugin_runtime_resources
                 WHERE owner_key = ?1 AND runtime_id = ?2",
                params![owner_key, runtime_id],
                |row| row.get::<_, i64>(0).map(|value| value as u64),
            )
            .optional()
            .map_err(failed)?;
        let Some(current) = current else {
            return Ok(RuntimeMutation::Missing);
        };
        if current != expected_revision {
            return Ok(RuntimeMutation::Conflict { current });
        }
        // Bindings intentionally outlive a runtime so the game remains identifiable and can be
        // rebound later. This delete does not touch games, saves or play history.
        tx.execute(
            "DELETE FROM plugin_runtime_resources
             WHERE owner_key = ?1 AND runtime_id = ?2 AND revision = ?3",
            params![
                owner_key,
                runtime_id,
                i64::try_from(expected_revision).map_err(failed)?
            ],
        )
        .map_err(failed)?;
        bump_runtime_catalog_revision(&tx, owner_key)?;
        tx.commit().map_err(failed)?;
        Ok(RuntimeMutation::Applied {
            revision: expected_revision,
        })
    }

    fn runtime_content_bindings(
        &self,
        owner_key: &str,
        game_id: u64,
        installation_id: &str,
    ) -> Result<Vec<RuntimeContentBinding>, StorageError> {
        if owner_key.is_empty() || game_id == 0 || installation_id.is_empty() {
            return Err(failed("runtime binding identity is required"));
        }
        let conn = self.conn();
        let mut statement = conn
            .prepare(
                "SELECT binding_json FROM plugin_runtime_content_bindings
                 WHERE owner_key = ?1 AND game_id = ?2 AND installation_id = ?3
                 ORDER BY content_id, runtime_id",
            )
            .map_err(failed)?;
        let rows = statement
            .query_map(
                params![owner_key, db_id(game_id)?, installation_id],
                |row| row.get::<_, String>(0),
            )
            .map_err(failed)?;
        rows.map(|row| {
            let json = row.map_err(failed)?;
            let binding: RuntimeContentBinding = serde_json::from_str(&json).map_err(failed)?;
            if !binding.validate() {
                return Err(failed("stored runtime binding is invalid"));
            }
            Ok(binding)
        })
        .collect()
    }

    fn runtime_catalog_revision(&self, owner_key: &str) -> Result<u64, StorageError> {
        if owner_key.is_empty() {
            return Err(failed("runtime owner is required"));
        }
        let conn = self.conn();
        let revision: Option<i64> = conn
            .query_row(
                "SELECT catalog_revision FROM plugin_runtime_catalog_state WHERE owner_key = ?1",
                [owner_key],
                |row| row.get(0),
            )
            .optional()
            .map_err(failed)?;
        revision
            .map(u64::try_from)
            .transpose()
            .map_err(failed)
            .map(|revision| revision.unwrap_or(0))
    }

    fn save_runtime_content_binding(
        &self,
        owner_key: &str,
        binding: &RuntimeContentBinding,
    ) -> Result<RuntimeMutation, StorageError> {
        if owner_key.is_empty() || !binding.validate() {
            return Err(failed("invalid runtime content binding"));
        }
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        let runtime_exists: Option<i64> = tx
            .query_row(
                "SELECT 1 FROM plugin_runtime_resources
                 WHERE owner_key = ?1 AND runtime_id = ?2",
                params![owner_key, binding.runtime_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(failed)?;
        if runtime_exists.is_none() {
            return Ok(RuntimeMutation::Missing);
        }
        let current: Option<u64> = tx
            .query_row(
                "SELECT revision FROM plugin_runtime_content_bindings
                 WHERE owner_key = ?1 AND game_id = ?2 AND installation_id = ?3
                   AND content_id = ?4 AND runtime_id = ?5",
                params![
                    owner_key,
                    db_id(binding.game_id)?,
                    binding.installation_id,
                    binding.content_id,
                    binding.runtime_id,
                ],
                |row| row.get::<_, i64>(0).map(|value| value as u64),
            )
            .optional()
            .map_err(failed)?;
        let expected = match current {
            Some(revision) => revision
                .checked_add(1)
                .ok_or_else(|| failed("runtime binding revision is exhausted"))?,
            None => 1,
        };
        if binding.revision != expected {
            return Ok(match current {
                Some(current) => RuntimeMutation::Conflict { current },
                None => RuntimeMutation::Conflict { current: 0 },
            });
        }
        let mut stored_binding = binding.clone();
        if current.is_none() {
            // Adding another runtime for an existing content_id must use its already-current
            // locator. A caller can move/rename it by revision-updating an existing binding first;
            // a new runtime binding cannot accidentally roll the shared locator back from stale
            // data.
            let sibling_json: Option<String> = tx
                .query_row(
                    "SELECT binding_json FROM plugin_runtime_content_bindings
                     WHERE owner_key = ?1 AND game_id = ?2 AND installation_id = ?3
                       AND content_id = ?4 ORDER BY runtime_id LIMIT 1",
                    params![
                        owner_key,
                        db_id(binding.game_id)?,
                        binding.installation_id,
                        binding.content_id,
                    ],
                    |row| row.get(0),
                )
                .optional()
                .map_err(failed)?;
            if let Some(json) = sibling_json {
                let sibling: RuntimeContentBinding = serde_json::from_str(&json).map_err(failed)?;
                stored_binding.content = sibling.content;
                stored_binding.content_kind = sibling.content_kind;
            }
        }
        tx.execute(
            "INSERT INTO plugin_runtime_content_bindings
                (owner_key, game_id, installation_id, content_id, runtime_id, revision, binding_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(owner_key, game_id, installation_id, content_id, runtime_id) DO UPDATE SET
                revision = excluded.revision,
                binding_json = excluded.binding_json",
            params![
                owner_key,
                db_id(binding.game_id)?,
                binding.installation_id,
                binding.content_id,
                binding.runtime_id,
                i64::try_from(binding.revision).map_err(failed)?,
                serde_json::to_string(&stored_binding).map_err(failed)?,
            ],
        )
        .map_err(failed)?;
        // A content locator is keyed by stable content_id, not by the runtime. Refresh every
        // sibling binding in the same transaction so choosing another runtime cannot resolve the
        // same moved/renamed content through a stale handle. Sibling revisions advance so stale
        // clients cannot overwrite the new location.
        let siblings = {
            let mut statement = tx
                .prepare(
                    "SELECT runtime_id, revision, binding_json
                     FROM plugin_runtime_content_bindings
                     WHERE owner_key = ?1 AND game_id = ?2 AND installation_id = ?3
                       AND content_id = ?4 AND runtime_id <> ?5",
                )
                .map_err(failed)?;
            let rows = statement
                .query_map(
                    params![
                        owner_key,
                        db_id(binding.game_id)?,
                        binding.installation_id,
                        binding.content_id,
                        binding.runtime_id,
                    ],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .map_err(failed)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(failed)?
        };
        for (runtime_id, revision, json) in siblings {
            let mut sibling: RuntimeContentBinding = serde_json::from_str(&json).map_err(failed)?;
            if sibling.content != stored_binding.content
                || sibling.content_kind != stored_binding.content_kind
            {
                sibling.content = stored_binding.content.clone();
                sibling.content_kind = stored_binding.content_kind.clone();
                sibling.revision = u64::try_from(revision)
                    .map_err(failed)?
                    .checked_add(1)
                    .ok_or_else(|| failed("runtime binding revision is exhausted"))?;
                tx.execute(
                    "UPDATE plugin_runtime_content_bindings
                     SET revision = ?1, binding_json = ?2
                     WHERE owner_key = ?3 AND game_id = ?4 AND installation_id = ?5
                       AND content_id = ?6 AND runtime_id = ?7 AND revision = ?8",
                    params![
                        i64::try_from(sibling.revision).map_err(failed)?,
                        serde_json::to_string(&sibling).map_err(failed)?,
                        owner_key,
                        db_id(binding.game_id)?,
                        binding.installation_id,
                        binding.content_id,
                        runtime_id,
                        revision,
                    ],
                )
                .map_err(failed)?;
            }
        }
        tx.commit().map_err(failed)?;
        Ok(RuntimeMutation::Applied {
            revision: binding.revision,
        })
    }

    fn remove_runtime_content_binding(
        &self,
        owner_key: &str,
        binding: &RuntimeContentBinding,
    ) -> Result<RuntimeMutation, StorageError> {
        if owner_key.is_empty() || !binding.validate() {
            return Err(failed("invalid runtime binding removal"));
        }
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        let current: Option<u64> = tx
            .query_row(
                "SELECT revision FROM plugin_runtime_content_bindings
                 WHERE owner_key = ?1 AND game_id = ?2 AND installation_id = ?3
                   AND content_id = ?4 AND runtime_id = ?5",
                params![
                    owner_key,
                    db_id(binding.game_id)?,
                    binding.installation_id,
                    binding.content_id,
                    binding.runtime_id,
                ],
                |row| row.get::<_, i64>(0).map(|value| value as u64),
            )
            .optional()
            .map_err(failed)?;
        let Some(current) = current else {
            return Ok(RuntimeMutation::Missing);
        };
        if current != binding.revision {
            return Ok(RuntimeMutation::Conflict { current });
        }
        tx.execute(
            "DELETE FROM plugin_runtime_content_bindings
             WHERE owner_key = ?1 AND game_id = ?2 AND installation_id = ?3
               AND content_id = ?4 AND runtime_id = ?5 AND revision = ?6",
            params![
                owner_key,
                db_id(binding.game_id)?,
                binding.installation_id,
                binding.content_id,
                binding.runtime_id,
                i64::try_from(binding.revision).map_err(failed)?,
            ],
        )
        .map_err(failed)?;
        tx.commit().map_err(failed)?;
        Ok(RuntimeMutation::Applied {
            revision: binding.revision,
        })
    }

    fn save_scan_failure(&self, provider: &str, error: &str) -> Result<(), StorageError> {
        self.conn()
            .execute(
                "INSERT INTO provider_scans (provider, last_success_at, last_error) VALUES (?1, NULL, ?2)
                 ON CONFLICT(provider) DO UPDATE SET last_error = excluded.last_error",
                params![provider, error],
            )
            .map(drop)
            .map_err(failed)
    }

    fn save_catalog(&self, provider: &str, update: &CatalogUpdate) -> Result<(), StorageError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        // Owned goes first so a shared merge sees the new owned facts and lets owned win.
        if let Some(owned) = &update.owned {
            save_catalog_fact(&tx, provider, owned, update.now, CatalogFact::Owned)?;
        }
        if let Some(shared) = &update.shared {
            save_catalog_fact(&tx, provider, shared, update.now, CatalogFact::Shared)?;
        }
        tx.commit().map_err(failed)
    }

    fn clear_catalog(&self, provider: &str) -> Result<(), StorageError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        tx.execute(
            "UPDATE library_entries SET owned = 0, family_shared = 0 WHERE provider = ?1",
            [provider],
        )
        .map_err(failed)?;
        tx.execute(
            "UPDATE provider_scans SET last_catalog_at = NULL, catalog_error = NULL,
                    last_family_at = NULL, family_error = NULL
             WHERE provider = ?1",
            [provider],
        )
        .map_err(failed)?;
        tx.commit().map_err(failed)
    }

    fn load_store_accounts(&self) -> Result<Vec<StoreAccount>, StorageError> {
        self.conn()
            .prepare(
                "SELECT provider, account_id, account_name, last_verified_at
                 FROM store_accounts ORDER BY provider",
            )
            .map_err(failed)?
            .query_map([], |row| {
                Ok(StoreAccount {
                    provider: row.get(0)?,
                    account_id: row.get(1)?,
                    account_name: row.get(2)?,
                    last_verified_at: row.get(3)?,
                })
            })
            .map_err(failed)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(failed)
    }

    fn save_store_account(&self, account: &StoreAccount) -> Result<(), StorageError> {
        self.conn()
            .execute(
                "INSERT INTO store_accounts (provider, account_id, account_name, last_verified_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(provider) DO UPDATE SET
                    account_id = excluded.account_id,
                    account_name = excluded.account_name,
                    last_verified_at = excluded.last_verified_at",
                params![
                    account.provider,
                    account.account_id,
                    account.account_name,
                    account.last_verified_at
                ],
            )
            .map(drop)
            .map_err(failed)
    }

    fn clear_store_account(&self, provider: &str) -> Result<(), StorageError> {
        self.conn()
            .execute("DELETE FROM store_accounts WHERE provider = ?1", [provider])
            .map(drop)
            .map_err(failed)
    }

    fn save_auto_matches(&self, matches: &[AutoMatch]) -> Result<(), StorageError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        {
            let mut put = tx
                .prepare(
                    "INSERT INTO auto_matches (category, uid, game_ids_json, checked_at)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(category, uid) DO UPDATE SET
                        game_ids_json = excluded.game_ids_json, checked_at = excluded.checked_at",
                )
                .map_err(failed)?;
            for m in matches {
                let ids = serde_json::to_string(&m.game_ids).map_err(failed)?;
                put.execute(params![m.category, m.uid, ids, m.checked_at])
                    .map_err(failed)?;
            }
        }
        tx.commit().map_err(failed)
    }

    fn save_chosen_match(
        &self,
        account_id: &str,
        choice: &ChosenMatch,
    ) -> Result<(), StorageError> {
        let game = choice.game_id.map(db_id).transpose()?;
        self.conn()
            .execute(
                "INSERT INTO chosen_matches (account_id, entry_key, game_id, chosen_at, source)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(account_id, entry_key) DO UPDATE SET
                    game_id = excluded.game_id, chosen_at = excluded.chosen_at,
                    source = excluded.source",
                params![
                    account_id,
                    choice.entry_key,
                    game,
                    choice.chosen_at,
                    choice.source.as_str()
                ],
            )
            .map(drop)
            .map_err(failed)
    }

    fn delete_chosen_match(&self, account_id: &str, entry_key: &str) -> Result<(), StorageError> {
        self.conn()
            .execute(
                "DELETE FROM chosen_matches WHERE account_id = ?1 AND entry_key = ?2",
                params![account_id, entry_key],
            )
            .map(drop)
            .map_err(failed)
    }

    fn save_favorite(&self, favorite: &FavoriteRecord) -> Result<(), StorageError> {
        let game_id = db_id(favorite.game_id)?;
        let conn = self.conn();
        // A confirmed "not favorite" has nothing to remember and must not shadow a later read.
        if !favorite.is_favorite && !favorite.pending {
            return conn
                .execute(
                    "DELETE FROM favorites WHERE account_id = ?1 AND game_id = ?2",
                    params![favorite.account_id, game_id],
                )
                .map(drop)
                .map_err(failed);
        }
        conn.execute(
            "INSERT INTO favorites (account_id, game_id, is_favorite, pending, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(account_id, game_id) DO UPDATE SET
                 is_favorite = excluded.is_favorite,
                 pending = excluded.pending,
                 updated_at = excluded.updated_at",
            params![
                favorite.account_id,
                game_id,
                favorite.is_favorite,
                favorite.pending,
                favorite.updated_at
            ],
        )
        .map(drop)
        .map_err(failed)
    }

    fn save_view(&self, account_id: &str, view: &SavedView) -> Result<i64, StorageError> {
        let payload = serde_json::to_string(&view.payload).map_err(failed)?;
        let conn = self.conn();
        if view.id != 0 {
            let updated = conn
                .execute(
                    "UPDATE saved_views SET name = ?3, payload = ?4, position = ?5
                     WHERE account_id = ?1 AND id = ?2",
                    params![account_id, view.id, view.name, payload, view.position],
                )
                .map_err(failed)?;
            if updated == 0 {
                return Err(failed("saved view not found"));
            }
            return Ok(view.id);
        }

        // A new view without an explicit position goes after the account's last one.
        let position: i64 = if view.position > 0 {
            view.position
        } else {
            conn.query_row(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM saved_views WHERE account_id = ?1",
                [account_id],
                |row| row.get(0),
            )
            .map_err(failed)?
        };
        conn.execute(
            "INSERT INTO saved_views (account_id, name, payload, position, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![account_id, view.name, payload, position, view.created_at],
        )
        .map_err(failed)?;
        Ok(conn.last_insert_rowid())
    }

    fn delete_view(&self, account_id: &str, id: i64) -> Result<(), StorageError> {
        self.conn()
            .execute(
                "DELETE FROM saved_views WHERE account_id = ?1 AND id = ?2",
                params![account_id, id],
            )
            .map(drop)
            .map_err(failed)
    }

    fn set_hidden(
        &self,
        account_id: &str,
        keys: &[String],
        hidden: bool,
        now: i64,
    ) -> Result<(), StorageError> {
        if keys.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        {
            let mut hide = tx
                .prepare(
                    "INSERT INTO hidden_entries (account_id, entry_key, hidden_at)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(account_id, entry_key) DO UPDATE SET
                        hidden_at = excluded.hidden_at",
                )
                .map_err(failed)?;
            let mut reveal = tx
                .prepare("DELETE FROM hidden_entries WHERE account_id = ?1 AND entry_key = ?2")
                .map_err(failed)?;
            for key in keys {
                if hidden {
                    hide.execute(params![account_id, key, now])
                        .map_err(failed)?;
                } else {
                    reveal.execute(params![account_id, key]).map_err(failed)?;
                }
            }
        }
        tx.commit().map_err(failed)
    }

    fn replace_account_library(
        &self,
        account_id: &str,
        entries: &[AccountEntry],
        collections: &[Collection],
        read_at: i64,
    ) -> Result<(), StorageError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        tx.execute(
            "DELETE FROM account_library_entries WHERE account_id = ?1",
            [account_id],
        )
        .map_err(failed)?;
        tx.execute(
            "DELETE FROM account_collections WHERE account_id = ?1",
            [account_id],
        )
        .map_err(failed)?;
        {
            let mut put_collection = tx
                .prepare(
                    "INSERT INTO account_collections
                        (account_id, collection_id, name, default_status, position)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )
                .map_err(failed)?;
            for (position, collection) in collections.iter().enumerate() {
                let position = i64::try_from(position).map_err(failed)?;
                put_collection
                    .execute(params![
                        account_id,
                        collection.id,
                        collection.name,
                        collection.default_status.map(EntryStatus::api_name),
                        position
                    ])
                    .map_err(failed)?;
            }

            let mut put_entry = tx
                .prepare(
                    "INSERT INTO account_library_entries
                        (account_id, game_id, entry_id, status, collection_ids)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(account_id, game_id) DO UPDATE SET
                        entry_id = excluded.entry_id,
                        status = excluded.status,
                        collection_ids = excluded.collection_ids",
                )
                .map_err(failed)?;
            for entry in entries {
                let game_id = db_id(entry.game_id)?;
                let collection_ids =
                    serde_json::to_string(&entry.collection_ids).map_err(failed)?;
                put_entry
                    .execute(params![
                        account_id,
                        game_id,
                        entry.id,
                        entry.status.map(EntryStatus::api_name),
                        collection_ids
                    ])
                    .map_err(failed)?;
            }
        }
        tx.execute(
            "INSERT INTO account_library_reads (account_id, read_at) VALUES (?1, ?2)
             ON CONFLICT(account_id) DO UPDATE SET read_at = excluded.read_at",
            params![account_id, read_at],
        )
        .map_err(failed)?;
        tx.commit().map_err(failed)
    }

    fn save_account_entry(
        &self,
        account_id: &str,
        game_id: u64,
        entry: Option<&AccountEntry>,
    ) -> Result<(), StorageError> {
        let game_id = db_id(game_id)?;
        let Some(entry) = entry else {
            return self
                .conn()
                .execute(
                    "DELETE FROM account_library_entries WHERE account_id = ?1 AND game_id = ?2",
                    params![account_id, game_id],
                )
                .map(drop)
                .map_err(failed);
        };
        let collection_ids = serde_json::to_string(&entry.collection_ids).map_err(failed)?;
        self.conn()
            .execute(
                "INSERT INTO account_library_entries
                    (account_id, game_id, entry_id, status, collection_ids)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(account_id, game_id) DO UPDATE SET
                    entry_id = excluded.entry_id,
                    status = excluded.status,
                    collection_ids = excluded.collection_ids",
                params![
                    account_id,
                    game_id,
                    entry.id,
                    entry.status.map(EntryStatus::api_name),
                    collection_ids
                ],
            )
            .map(drop)
            .map_err(failed)
    }

    fn save_games(&self, games: &[GameRecord]) -> Result<(), StorageError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        {
            // Every right-hand side sees the row as it was before the update, so the cover state
            // is compared with the old URL.
            let mut put = tx
                .prepare(
                    "INSERT INTO games (game_id, name, release_year, cover_url, platforms)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(game_id) DO UPDATE SET
                        name = excluded.name,
                        release_year = excluded.release_year,
                        cover_file = CASE WHEN games.cover_url IS excluded.cover_url
                                          THEN games.cover_file ELSE NULL END,
                        cover_failed_at = CASE WHEN games.cover_url IS excluded.cover_url
                                               THEN games.cover_failed_at ELSE NULL END,
                        cover_url = excluded.cover_url,
                        platforms = excluded.platforms",
                )
                .map_err(failed)?;
            for game in games {
                put.execute(params![
                    db_id(game.game_id)?,
                    game.name,
                    game.release_year,
                    game.cover_url,
                    serde_json::to_string(&game.platforms).map_err(failed)?
                ])
                .map_err(failed)?;
            }
        }
        tx.commit().map_err(failed)
    }

    fn save_cover_ready(&self, game: u64, file: &str) -> Result<(), StorageError> {
        self.conn()
            .execute(
                "UPDATE games SET cover_file = ?2, cover_failed_at = NULL WHERE game_id = ?1",
                params![db_id(game)?, file],
            )
            .map(drop)
            .map_err(failed)
    }

    fn save_cover_failed(&self, game: u64, at: i64) -> Result<(), StorageError> {
        self.conn()
            .execute(
                "UPDATE games SET cover_failed_at = ?2 WHERE game_id = ?1",
                params![db_id(game)?, at],
            )
            .map(drop)
            .map_err(failed)
    }

    fn load_layout(&self) -> Result<LibraryLayout, StorageError> {
        let stored = read_setting(&self.conn(), KEY_LAYOUT).map_err(failed)?;
        // A layout nobody can read falls back to the default instead of blocking startup.
        Ok(stored
            .and_then(|json| {
                serde_json::from_str(&json)
                    .inspect_err(
                        |error| tracing::warn!(%error, "ignored an unreadable saved layout"),
                    )
                    .ok()
            })
            .unwrap_or_default())
    }

    fn save_layout(&self, layout: &LibraryLayout) -> Result<(), StorageError> {
        let json = serde_json::to_string(layout).map_err(failed)?;
        write_setting(&self.conn(), KEY_LAYOUT, &json).map_err(failed)
    }

    fn save_excluded(&self, excluded: &[String]) -> Result<(), StorageError> {
        let json = serde_json::to_string(excluded).map_err(failed)?;
        write_setting(&self.conn(), KEY_EXCLUDED, &json).map_err(failed)
    }

    fn save_display_name(
        &self,
        entry_key: &str,
        expected_revision: u64,
        display_name: Option<&str>,
    ) -> Result<DisplayWrite, StorageError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        let current: Option<(Option<String>, i64)> = tx
            .query_row(
                "SELECT display_name, revision FROM library_display_overrides WHERE entry_key = ?1",
                [entry_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(failed)?;
        let (current_name, current_revision) = match current {
            Some((name, revision)) => (name, u64::try_from(revision).map_err(failed)?),
            None => (None, 0),
        };
        if current_revision != expected_revision {
            return Ok(DisplayWrite::Conflict {
                current: current_revision,
            });
        }
        if current_name.as_deref() == display_name {
            return Ok(DisplayWrite::Unchanged {
                revision: current_revision,
            });
        }
        let revision = current_revision + 1;
        tx.execute(
            "INSERT INTO library_display_overrides (entry_key, display_name, revision)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(entry_key) DO UPDATE SET
                display_name = excluded.display_name, revision = excluded.revision",
            params![
                entry_key,
                display_name,
                i64::try_from(revision).map_err(failed)?
            ],
        )
        .map_err(failed)?;
        tx.commit().map_err(failed)?;
        Ok(DisplayWrite::Applied { revision })
    }

    fn save_artwork(
        &self,
        entry_key: &str,
        expected_revision: u64,
        file: Option<&str>,
    ) -> Result<ArtworkWrite, StorageError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        let current: Option<(Option<String>, i64)> = tx
            .query_row(
                "SELECT file, revision FROM library_artwork_overrides WHERE entry_key = ?1",
                [entry_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(failed)?;
        let (current_file, current_revision) = match current {
            Some((file, revision)) => (file, u64::try_from(revision).map_err(failed)?),
            None => (None, 0),
        };
        if current_revision != expected_revision {
            return Ok(ArtworkWrite::Conflict {
                current: current_revision,
            });
        }
        if current_file.as_deref() == file {
            return Ok(ArtworkWrite::Unchanged {
                revision: current_revision,
            });
        }
        let revision = current_revision + 1;
        tx.execute(
            "INSERT INTO library_artwork_overrides (entry_key, file, revision)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(entry_key) DO UPDATE SET
                file = excluded.file, revision = excluded.revision",
            params![entry_key, file, i64::try_from(revision).map_err(failed)?],
        )
        .map_err(failed)?;
        tx.commit().map_err(failed)?;
        Ok(ArtworkWrite::Applied {
            revision,
            replaced: current_file,
        })
    }

    fn installation_ids(
        &self,
        entry_keys: &[String],
    ) -> Result<Vec<(String, String)>, StorageError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        let mut ids = Vec::with_capacity(entry_keys.len());
        {
            // 128 random bits from SQLite's CSPRNG; an (astronomically unlikely) duplicate id
            // is ignored and the key simply has no id until the next request.
            let mut assign = tx
                .prepare(
                    "INSERT OR IGNORE INTO library_installations (entry_key, installation_id)
                     VALUES (?1, lower(hex(randomblob(16))))",
                )
                .map_err(failed)?;
            let mut read = tx
                .prepare("SELECT installation_id FROM library_installations WHERE entry_key = ?1")
                .map_err(failed)?;
            for key in entry_keys {
                assign.execute([key]).map_err(failed)?;
                if let Some(id) = read
                    .query_row([key], |row| row.get::<_, String>(0))
                    .optional()
                    .map_err(failed)?
                {
                    ids.push((key.clone(), id));
                }
            }
        }
        tx.commit().map_err(failed)?;
        Ok(ids)
    }

    fn load_last_account(&self) -> Result<Option<LastAccount>, StorageError> {
        let conn = self.conn();
        let id = read_setting(&conn, KEY_ACCOUNT_ID).map_err(failed)?;
        let name = read_setting(&conn, KEY_ACCOUNT_NAME).map_err(failed)?;
        Ok(match (id, name) {
            (Some(id), Some(name)) if !id.is_empty() => Some(LastAccount { id, name }),
            _ => None,
        })
    }

    fn save_last_account(&self, account: &LastAccount) -> Result<(), StorageError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        write_setting(&tx, KEY_ACCOUNT_ID, &account.id).map_err(failed)?;
        write_setting(&tx, KEY_ACCOUNT_NAME, &account.name).map_err(failed)?;
        tx.commit().map_err(failed)
    }

    fn clear_last_account(&self) -> Result<(), StorageError> {
        self.conn()
            .execute(
                "DELETE FROM settings WHERE key IN (?1, ?2)",
                params![KEY_ACCOUNT_ID, KEY_ACCOUNT_NAME],
            )
            .map(drop)
            .map_err(failed)
    }
}

#[cfg(test)]
mod tests;
