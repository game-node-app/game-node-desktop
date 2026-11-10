//! SQLite repository for the library cache, the remembered layout and the last signed-in account.
//!
//! It shares the connection of [`SqliteSettingsStore`](crate::db::SqliteSettingsStore), so every
//! write is ordered with settings writes. Nothing here stores a credential or callback URL.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use gamenode_core::library::{
    AutoMatch, CatalogError, CatalogRecord, CatalogUpdate, ChosenMatch, FavoriteRecord, GameRecord,
    LastAccount, LibraryCache, LibraryLayout, LibraryStore, ProviderScanState, StoreAccount,
    StoredEntry, entry_key,
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
                family_shared, first_seen, last_seen
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
            family_shared: read_flag(row.get(8)?, &key, "family_shared"),
            key,
            provider: row.get(1)?,
            store_id: row.get(2)?,
            name: row.get(3)?,
            install_dir: row.get(4)?,
            fields,
            installed: row.get::<_, i64>(6)? != 0,
            first_seen: row.get(9)?,
            last_seen: row.get(10)?,
        })
    })?;
    rows.collect()
}

#[derive(Clone, Copy)]
enum CatalogFact {
    Owned,
    Family,
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
    const UPSERT_FAMILY: &str = "INSERT INTO library_entries
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
            CatalogFact::Family => UPSERT_FAMILY,
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
                CatalogFact::Family => {
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
                CatalogFact::Family => {
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
                CatalogFact::Family => {
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
                    last_family_at: row.get(5)?,
                    family_error: read_catalog_error(&provider, "family_error", row.get(6)?),
                    provider,
                    last_success_at: row.get(1)?,
                    last_error: row.get(2)?,
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
                    "SELECT entry_key, game_id, chosen_at FROM chosen_matches
                     WHERE account_id = ?1 ORDER BY entry_key",
                )
                .map_err(failed)?
                .query_map([account], |row| {
                    Ok(ChosenMatch {
                        entry_key: row.get(0)?,
                        game_id: row.get::<_, Option<i64>>(1)?.and_then(game_id),
                        chosen_at: row.get(2)?,
                    })
                })
                .map_err(failed)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(failed)?,
        };

        let games = conn
            .prepare(
                "SELECT game_id, name, release_year, cover_url, cover_file, cover_failed_at
                 FROM games ORDER BY game_id",
            )
            .map_err(failed)?
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    GameRecord {
                        game_id: 0,
                        name: row.get(1)?,
                        release_year: row.get(2)?,
                        cover_url: row.get(3)?,
                        cover_file: row.get(4)?,
                        cover_failed_at: row.get(5)?,
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

        Ok(LibraryCache {
            entries,
            scans,
            auto_matches,
            chosen_matches,
            favorites,
            games,
            excluded,
        })
    }

    fn save_scan(&self, provider: &str, records: &[Record], now: i64) -> Result<(), StorageError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        tx.execute(
            "UPDATE library_entries SET installed = 0 WHERE provider = ?1",
            [provider],
        )
        .map_err(failed)?;
        {
            let mut upsert = tx
                .prepare(
                    "INSERT INTO library_entries
                        (key, provider, store_id, name, install_dir, fields_json, installed, first_seen, last_seen)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?7)
                     ON CONFLICT(key) DO UPDATE SET
                        name = excluded.name,
                        install_dir = excluded.install_dir,
                        fields_json = excluded.fields_json,
                        installed = 1,
                        last_seen = excluded.last_seen",
                )
                .map_err(failed)?;
            for record in records {
                let fields = serde_json::to_string(&record.fields).map_err(failed)?;
                upsert
                    .execute(params![
                        entry_key(provider, &record.id),
                        provider,
                        record.id,
                        record.name,
                        record.install_dir,
                        fields,
                        now
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
        // Owned goes first so a family merge sees the new owned facts and lets owned win.
        if let Some(owned) = &update.owned {
            save_catalog_fact(&tx, provider, owned, update.now, CatalogFact::Owned)?;
        }
        if let Some(family) = &update.family {
            save_catalog_fact(&tx, provider, family, update.now, CatalogFact::Family)?;
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
                "INSERT INTO chosen_matches (account_id, entry_key, game_id, chosen_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(account_id, entry_key) DO UPDATE SET
                    game_id = excluded.game_id, chosen_at = excluded.chosen_at",
                params![account_id, choice.entry_key, game, choice.chosen_at],
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

    fn save_games(&self, games: &[GameRecord]) -> Result<(), StorageError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        {
            // Every right-hand side sees the row as it was before the update, so the cover state
            // is compared with the old URL.
            let mut put = tx
                .prepare(
                    "INSERT INTO games (game_id, name, release_year, cover_url)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(game_id) DO UPDATE SET
                        name = excluded.name,
                        release_year = excluded.release_year,
                        cover_file = CASE WHEN games.cover_url IS excluded.cover_url
                                          THEN games.cover_file ELSE NULL END,
                        cover_failed_at = CASE WHEN games.cover_url IS excluded.cover_url
                                               THEN games.cover_failed_at ELSE NULL END,
                        cover_url = excluded.cover_url",
                )
                .map_err(failed)?;
            for game in games {
                put.execute(params![
                    db_id(game.game_id)?,
                    game.name,
                    game.release_year,
                    game.cover_url
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
