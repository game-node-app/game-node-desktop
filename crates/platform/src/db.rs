//! SQLite persistence: ordered, transactional migrations and the foundation settings repository.

use std::path::Path;
use std::sync::{Arc, Mutex};

use gamenode_core::ports::{SettingsStore, StorageError};
use gamenode_core::{AppPreferences, Settings, WindowGeometry};
use rusqlite::{Connection, OptionalExtension, params};

/// One schema change. Versions start at 1 and increase by exactly one.
#[derive(Clone, Copy, Debug)]
pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

/// The latest supported schema version.
pub const CURRENT_SCHEMA_VERSION: u32 = 26;

/// Every migration this build knows. Append new ones; never edit or reorder released ones.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_settings",
        sql: include_str!("../migrations/0001_initial_settings.sql"),
    },
    Migration {
        version: 2,
        name: "library",
        sql: include_str!("../migrations/0002_library.sql"),
    },
    Migration {
        version: 3,
        name: "playtime",
        sql: include_str!("../migrations/0003_playtime.sql"),
    },
    Migration {
        version: 4,
        name: "drm_free_backups",
        sql: include_str!("../migrations/0004_drm_free_backups.sql"),
    },
    Migration {
        version: 5,
        name: "game_tools",
        sql: include_str!("../migrations/0005_game_tools.sql"),
    },
    Migration {
        version: 6,
        name: "store_accounts_and_catalog",
        sql: include_str!("../migrations/0006_store_accounts_and_catalog.sql"),
    },
    Migration {
        version: 7,
        name: "favorites",
        sql: include_str!("../migrations/0007_favorites.sql"),
    },
    Migration {
        version: 8,
        name: "ports",
        sql: include_str!("../migrations/0008_ports.sql"),
    },
    Migration {
        version: 9,
        name: "achievements",
        sql: include_str!("../migrations/0009_achievements.sql"),
    },
    Migration {
        version: 10,
        name: "library_organization",
        sql: include_str!("../migrations/0010_library_organization.sql"),
    },
    Migration {
        version: 11,
        name: "game_platforms",
        sql: include_str!("../migrations/0011_game_platforms.sql"),
    },
    Migration {
        version: 12,
        name: "achievement_rarity",
        sql: include_str!("../migrations/0012_achievement_rarity.sql"),
    },
    Migration {
        version: 13,
        name: "backup_metadata",
        sql: include_str!("../migrations/0013_backup_metadata.sql"),
    },
    Migration {
        version: 14,
        name: "local_cover",
        sql: include_str!("../migrations/0014_local_cover.sql"),
    },
    Migration {
        version: 15,
        name: "app_preferences",
        sql: include_str!("../migrations/0015_app_preferences.sql"),
    },
    Migration {
        version: 16,
        name: "sync_summary",
        sql: include_str!("../migrations/0016_sync_summary.sql"),
    },
    Migration {
        version: 17,
        name: "library_installations",
        sql: include_str!("../migrations/0017_library_installations.sql"),
    },
    Migration {
        version: 18,
        name: "library_display_overrides",
        sql: include_str!("../migrations/0018_library_display_overrides.sql"),
    },
    Migration {
        version: 19,
        name: "chosen_match_source",
        sql: include_str!("../migrations/0019_chosen_match_source.sql"),
    },
    Migration {
        version: 20,
        name: "library_artwork_overrides",
        sql: include_str!("../migrations/0020_library_artwork_overrides.sql"),
    },
    Migration {
        version: 21,
        name: "library_entry_targets",
        sql: include_str!("../migrations/0021_library_entry_targets.sql"),
    },
    Migration {
        version: 22,
        name: "plugin_custom_launch_recipes",
        sql: include_str!("../migrations/0022_plugin_custom_launch_recipes.sql"),
    },
    Migration {
        version: 23,
        name: "plugin_runtime_resources",
        sql: include_str!("../migrations/0023_plugin_runtime_resources.sql"),
    },
    Migration {
        version: 24,
        name: "library_launch_recipe_selection",
        sql: include_str!("../migrations/0024_library_launch_recipe_selection.sql"),
    },
    Migration {
        version: 25,
        name: "plugin_bundles",
        sql: include_str!("../migrations/0025_plugin_bundles.sql"),
    },
    Migration {
        version: 26,
        name: "plugin_bundle_history",
        sql: include_str!("../migrations/0026_plugin_bundle_history.sql"),
    },
];

const BOOKKEEPING: &str = "CREATE TABLE IF NOT EXISTS schema_migrations (
    version    INTEGER PRIMARY KEY NOT NULL,
    name       TEXT NOT NULL,
    applied_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT";

/// The highest schema version in `migrations`.
pub fn latest_version(migrations: &[Migration]) -> u32 {
    migrations.last().map_or(0, |m| m.version)
}

fn failed(error: impl std::fmt::Display) -> StorageError {
    StorageError::Failed(error.to_string())
}

/// Brings the database up to date with `migrations`.
///
/// * A database whose recorded version is newer than `migrations` is rejected untouched.
/// * Each pending migration runs in its own transaction together with its history row; a
///   failure rolls that migration back and leaves earlier ones committed.
pub fn migrate(conn: &mut Connection, migrations: &[Migration]) -> Result<(), StorageError> {
    for (index, migration) in migrations.iter().enumerate() {
        if migration.version != index as u32 + 1 {
            return Err(StorageError::Failed(
                "migration list is not sequential".into(),
            ));
        }
    }
    let supported = latest_version(migrations);

    // Inspect the full recorded history without writing, so mismatched, foreign or newer
    // databases stay byte-for-byte unchanged.
    let history = migration_history_from_connection(conn)?;
    let applied = if let Some(history) = &history {
        let found = history.last().map_or(0, |(version, _)| *version);
        if found > supported {
            return Err(StorageError::NewerSchema { found, supported });
        }
        let expected = migrations
            .iter()
            .take(history.len())
            .map(|migration| (migration.version, migration.name.to_owned()))
            .collect::<Vec<_>>();
        if history != &expected {
            return Err(StorageError::Failed(
                "the local database migration history does not match this version; the database was left unchanged".into(),
            ));
        }
        found
    } else {
        let foreign: i64 = conn
            .query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| row.get(0))
            .map_err(failed)?;
        if foreign > 0 {
            return Err(StorageError::Failed(
                "the file is not a GameNode database".into(),
            ));
        }
        conn.execute_batch(BOOKKEEPING).map_err(failed)?;
        0
    };

    if applied < supported
        && history.is_some()
        && let Some(path) = conn.path().filter(|path| is_persistent_database_path(path))
    {
        create_verified_migration_snapshot(
            conn,
            Path::new(path),
            history.as_deref().unwrap_or_default(),
        )?;
    }

    for migration in migrations.iter().filter(|m| m.version > applied) {
        run_migration(conn, migration).map_err(|_| StorageError::MigrationFailed {
            version: migration.version,
        })?;
    }
    Ok(())
}

fn migration_history_from_connection(
    conn: &Connection,
) -> Result<Option<Vec<(u32, String)>>, StorageError> {
    let has_history = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations'",
            [],
            |_| Ok(()),
        )
        .optional()
        .map_err(failed)?
        .is_some();
    if !has_history {
        return Ok(None);
    }

    let mut statement = conn
        .prepare("SELECT version, name FROM schema_migrations ORDER BY version")
        .map_err(failed)?;
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(failed)?;
    rows.map(|row| row.map_err(failed))
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn is_persistent_database_path(path: &str) -> bool {
    path != ":memory:" && !path.starts_with("file::memory:") && Path::new(path).is_file()
}

fn create_verified_migration_snapshot(
    conn: &Connection,
    database_path: &Path,
    expected_history: &[(u32, String)],
) -> Result<(), StorageError> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let parent = database_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let database_name = database_path
        .file_name()
        .ok_or_else(|| failed("could not identify the local database file"))?
        .to_string_lossy();
    let prefix = format!("{database_name}.pre-migration-");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(failed)?
        .as_nanos();
    let stem = format!("{prefix}{timestamp}-{}", std::process::id());
    let mut temporary = parent.join(format!(".{stem}.tmp"));
    let mut snapshot = parent.join(format!("{stem}.sqlite"));
    for suffix in 0..8 {
        if !temporary.exists() && !snapshot.exists() {
            break;
        }
        temporary = parent.join(format!(".{stem}-{suffix}.tmp"));
        snapshot = parent.join(format!("{stem}-{suffix}.sqlite"));
    }
    if temporary.exists() || snapshot.exists() {
        return Err(failed(
            "could not reserve a migration snapshot file; the migration was not run",
        ));
    }

    let result = (|| -> Result<(), StorageError> {
        conn.backup(rusqlite::MAIN_DB, &temporary, Some(|_| {}))
            .map_err(failed)?;
        let snapshot_conn =
            Connection::open_with_flags(&temporary, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(failed)?;
        let integrity: String = snapshot_conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(failed)?;
        if integrity != "ok"
            || migration_history_from_connection(&snapshot_conn)?.as_deref()
                != Some(expected_history)
        {
            return Err(failed(
                "the pre-migration snapshot did not pass verification; the migration was not run",
            ));
        }
        drop(snapshot_conn);
        std::fs::File::open(&temporary)
            .and_then(|file| file.sync_all())
            .map_err(failed)?;
        std::fs::rename(&temporary, &snapshot).map_err(failed)?;
        #[cfg(unix)]
        std::fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(failed)?;
        Ok(())
    })();

    if let Err(error) = result {
        let _ = std::fs::remove_file(&temporary);
        return Err(failed(format!(
            "could not create a verified pre-migration snapshot; the migration was not run ({error})"
        )));
    }

    if let Ok(entries) = std::fs::read_dir(parent) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name.starts_with(&prefix) && path != snapshot {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    Ok(())
}

fn run_migration(conn: &mut Connection, migration: &Migration) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    tx.execute_batch(migration.sql)?;
    tx.execute(
        "INSERT INTO schema_migrations (version, name) VALUES (?1, ?2)",
        params![migration.version, migration.name],
    )?;
    tx.commit()
}

/// Opens (creating if necessary) and migrates the database at `path`.
pub fn open(path: &Path) -> Result<Connection, StorageError> {
    open_with(path, MIGRATIONS)
}

pub fn open_with(path: &Path, migrations: &[Migration]) -> Result<Connection, StorageError> {
    let mut conn = Connection::open(path).map_err(failed)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(failed)?;
    migrate(&mut conn, migrations)?;
    Ok(conn)
}

const KEY_RENDERER: &str = "renderer";
const KEY_X: &str = "window.x";
const KEY_Y: &str = "window.y";
const KEY_WIDTH: &str = "window.width";
const KEY_HEIGHT: &str = "window.height";
const KEY_MAXIMIZED: &str = "window.maximized";
pub(crate) const KEY_DISCOVERY_LAST_CHECKED: &str = "discovery.lastCheckedAt";

const KEY_PREF_START_WITH_WINDOWS: &str = "app.startWithWindows";
const KEY_PREF_START_MINIMIZED: &str = "app.startMinimized";
const KEY_PREF_CLOSE_TO_TRAY: &str = "app.closeToTray";
const KEY_PREF_LOCALE: &str = "app.locale";
const KEY_PREF_SHOW_STATUS_BAR: &str = "app.showStatusBar";
const KEY_PREF_START_SCOPE: &str = "app.startScope";
const KEY_PREF_DISCOVERY_ON_START: &str = "app.discoveryOnStart";
const KEY_PREF_DISCOVERY_REALTIME: &str = "app.discoveryRealtime";
const KEY_PREF_TOOL_AUTO_CHECK: &str = "app.toolAutoCheck";
const KEY_PREF_TOOL_DEFAULT_CHANNEL: &str = "app.toolDefaultChannel";
const KEY_PREF_SHARED_EXCLUDED: &str = "app.sharedExcluded";

/// Every key the foundation may store; nothing else belongs in `settings`.
pub const SETTING_KEYS: [&str; 26] = [
    "backup_settings",
    crate::playtime_db::KEY_PLAYTIME_SOURCE,
    crate::playtime_db::KEY_SYNC_ENABLED,
    crate::playtime_db::KEY_SYNC_EXPLAINED,
    KEY_RENDERER,
    KEY_X,
    KEY_Y,
    KEY_WIDTH,
    KEY_HEIGHT,
    KEY_MAXIMIZED,
    KEY_DISCOVERY_LAST_CHECKED,
    crate::library_db::KEY_LAYOUT,
    crate::library_db::KEY_EXCLUDED,
    crate::library_db::KEY_ACCOUNT_ID,
    crate::library_db::KEY_ACCOUNT_NAME,
    KEY_PREF_START_WITH_WINDOWS,
    KEY_PREF_START_MINIMIZED,
    KEY_PREF_CLOSE_TO_TRAY,
    KEY_PREF_LOCALE,
    KEY_PREF_SHOW_STATUS_BAR,
    KEY_PREF_START_SCOPE,
    KEY_PREF_DISCOVERY_ON_START,
    KEY_PREF_DISCOVERY_REALTIME,
    KEY_PREF_TOOL_AUTO_CHECK,
    KEY_PREF_TOOL_DEFAULT_CHANNEL,
    KEY_PREF_SHARED_EXCLUDED,
];

/// [`SettingsStore`] backed by the application database.
///
/// The connection is shared with [`crate::library_db::SqliteLibraryStore`] (see
/// [`library_store`](Self::library_store)), so settings and library writes are serialized.
pub struct SqliteSettingsStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteSettingsStore {
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    /// The library repository on the same database connection.
    pub fn library_store(&self) -> crate::library_db::SqliteLibraryStore {
        crate::library_db::SqliteLibraryStore::new(Arc::clone(&self.conn))
    }

    pub fn local_game_store(&self) -> crate::local_game_db::SqliteLocalGameStore {
        crate::local_game_db::SqliteLocalGameStore::new(Arc::clone(&self.conn))
    }

    /// The playtime repository on the same serialized connection.
    pub fn playtime_store(&self) -> crate::playtime_db::SqlitePlaytimeStore {
        crate::playtime_db::SqlitePlaytimeStore::new(Arc::clone(&self.conn))
    }

    /// The game tools repository on the same serialized connection.
    pub fn tool_store(&self) -> crate::tools_db::SqliteToolStore {
        crate::tools_db::SqliteToolStore::new(Arc::clone(&self.conn))
    }

    /// The PC ports repository on the same serialized connection.
    pub fn ports_store(&self) -> crate::ports_db::SqlitePortStore {
        crate::ports_db::SqlitePortStore::new(Arc::clone(&self.conn))
    }

    /// The achievement cache on the same serialized connection.
    pub fn achievement_store(&self) -> crate::achievement_db::SqliteAchievementStore {
        crate::achievement_db::SqliteAchievementStore::new(Arc::clone(&self.conn))
    }

    /// The plugin bundle mapping on the same serialized connection.
    pub fn bundle_store(&self) -> crate::plugin_bundle_db::SqlitePluginBundleStore {
        crate::plugin_bundle_db::SqlitePluginBundleStore::new(Arc::clone(&self.conn))
    }

    /// Opens and migrates the database at `path`.
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        open(path).map(Self::new)
    }

    pub(crate) fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        lock(&self.conn)
    }
}

pub(crate) fn lock(conn: &Mutex<Connection>) -> std::sync::MutexGuard<'_, Connection> {
    conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn read(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
        row.get(0)
    })
    .optional()
}

/// Reads one JSON-valued preference key. A missing or unreadable value falls back to that
/// preference's own default, never to another field's value or a failed load.
fn preference<T: serde::de::DeserializeOwned>(
    conn: &Connection,
    key: &str,
    fallback: T,
) -> Result<T, StorageError> {
    Ok(read(conn, key)
        .map_err(failed)?
        .and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or(fallback))
}

/// The JSON value stored for one preference. These types always serialize; an unexpected failure
/// stores nothing usable and the field loads as its default.
fn preference_value<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

/// Every preference as its own `settings` row, in a stable order.
fn preference_rows(preferences: &AppPreferences) -> [(&'static str, String); 11] {
    [
        (
            KEY_PREF_START_WITH_WINDOWS,
            preference_value(&preferences.start_with_windows),
        ),
        (
            KEY_PREF_START_MINIMIZED,
            preference_value(&preferences.start_minimized),
        ),
        (
            KEY_PREF_CLOSE_TO_TRAY,
            preference_value(&preferences.close_to_tray),
        ),
        (KEY_PREF_LOCALE, preference_value(&preferences.locale)),
        (
            KEY_PREF_SHOW_STATUS_BAR,
            preference_value(&preferences.show_status_bar),
        ),
        (
            KEY_PREF_START_SCOPE,
            preference_value(&preferences.start_scope),
        ),
        (
            KEY_PREF_DISCOVERY_ON_START,
            preference_value(&preferences.discovery_on_start),
        ),
        (
            KEY_PREF_DISCOVERY_REALTIME,
            preference_value(&preferences.discovery_realtime),
        ),
        (
            KEY_PREF_TOOL_AUTO_CHECK,
            preference_value(&preferences.tool_auto_check),
        ),
        (
            KEY_PREF_TOOL_DEFAULT_CHANNEL,
            preference_value(&preferences.tool_default_channel),
        ),
        (
            KEY_PREF_SHARED_EXCLUDED,
            preference_value(&preferences.shared_excluded),
        ),
    ]
}

fn write_preferences(conn: &Connection, preferences: &AppPreferences) -> Result<(), StorageError> {
    let mut put = conn
        .prepare("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        .map_err(failed)?;
    for (key, value) in preference_rows(preferences) {
        put.execute(params![key, value]).map_err(failed)?;
    }
    Ok(())
}

fn write_window(conn: &Connection, window: Option<WindowGeometry>) -> Result<(), StorageError> {
    let mut put = conn
        .prepare("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        .map_err(failed)?;
    let mut delete = conn
        .prepare("DELETE FROM settings WHERE key = ?1")
        .map_err(failed)?;
    match window {
        Some(w) => {
            put.execute(params![KEY_WIDTH, w.width.to_string()])
                .map_err(failed)?;
            put.execute(params![KEY_HEIGHT, w.height.to_string()])
                .map_err(failed)?;
            put.execute(params![KEY_MAXIMIZED, if w.maximized { "1" } else { "0" }])
                .map_err(failed)?;
            for (key, value) in [(KEY_X, w.x), (KEY_Y, w.y)] {
                match value {
                    Some(v) => put.execute(params![key, v.to_string()]).map_err(failed)?,
                    None => delete.execute([key]).map_err(failed)?,
                };
            }
        }
        None => {
            for key in [KEY_X, KEY_Y, KEY_WIDTH, KEY_HEIGHT, KEY_MAXIMIZED] {
                delete.execute([key]).map_err(failed)?;
            }
        }
    }
    Ok(())
}

impl SettingsStore for SqliteSettingsStore {
    fn load(&self) -> Result<Settings, StorageError> {
        let conn = self.conn();
        let get = |key| read(&conn, key).map_err(failed);
        let number = |key| -> Result<Option<i64>, StorageError> {
            Ok(get(key)?.and_then(|v| v.parse().ok()))
        };
        let window = match (number(KEY_WIDTH)?, number(KEY_HEIGHT)?) {
            (Some(width @ 1..=100_000), Some(height @ 1..=100_000)) => Some(WindowGeometry {
                x: number(KEY_X)?.and_then(|v| i32::try_from(v).ok()),
                y: number(KEY_Y)?.and_then(|v| i32::try_from(v).ok()),
                width: width as u32,
                height: height as u32,
                maximized: get(KEY_MAXIMIZED)?.as_deref() == Some("1"),
            }),
            _ => None,
        };
        let preferences = AppPreferences {
            start_with_windows: preference(
                &conn,
                KEY_PREF_START_WITH_WINDOWS,
                AppPreferences::default().start_with_windows,
            )?,
            start_minimized: preference(
                &conn,
                KEY_PREF_START_MINIMIZED,
                AppPreferences::default().start_minimized,
            )?,
            close_to_tray: preference(
                &conn,
                KEY_PREF_CLOSE_TO_TRAY,
                AppPreferences::default().close_to_tray,
            )?,
            locale: preference(&conn, KEY_PREF_LOCALE, AppPreferences::default().locale)?,
            show_status_bar: preference(
                &conn,
                KEY_PREF_SHOW_STATUS_BAR,
                AppPreferences::default().show_status_bar,
            )?,
            start_scope: preference(
                &conn,
                KEY_PREF_START_SCOPE,
                AppPreferences::default().start_scope,
            )?,
            discovery_on_start: preference(
                &conn,
                KEY_PREF_DISCOVERY_ON_START,
                AppPreferences::default().discovery_on_start,
            )?,
            discovery_realtime: preference(
                &conn,
                KEY_PREF_DISCOVERY_REALTIME,
                AppPreferences::default().discovery_realtime,
            )?,
            tool_auto_check: preference(
                &conn,
                KEY_PREF_TOOL_AUTO_CHECK,
                AppPreferences::default().tool_auto_check,
            )?,
            tool_default_channel: preference(
                &conn,
                KEY_PREF_TOOL_DEFAULT_CHANNEL,
                AppPreferences::default().tool_default_channel,
            )?,
            shared_excluded: preference(
                &conn,
                KEY_PREF_SHARED_EXCLUDED,
                AppPreferences::default().shared_excluded,
            )?,
        };
        Ok(Settings {
            window,
            preferences,
        })
    }

    fn save(&self, settings: &Settings) -> Result<(), StorageError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        write_window(&tx, settings.window)?;
        write_preferences(&tx, &settings.preferences)?;
        tx.commit().map_err(failed)
    }

    fn save_window(&self, window: Option<WindowGeometry>) -> Result<(), StorageError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        write_window(&tx, window)?;
        tx.commit().map_err(failed)
    }

    fn save_preferences(&self, preferences: &AppPreferences) -> Result<(), StorageError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        write_preferences(&tx, preferences)?;
        tx.commit().map_err(failed)
    }

    fn load_discovery_last_checked_at(&self) -> Result<Option<i64>, StorageError> {
        let conn = self.conn();
        Ok(read(&conn, KEY_DISCOVERY_LAST_CHECKED)
            .map_err(failed)?
            .and_then(|value| value.parse().ok()))
    }

    fn save_discovery_last_checked_at(&self, at: i64) -> Result<(), StorageError> {
        self.conn()
            .execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![KEY_DISCOVERY_LAST_CHECKED, at.to_string()],
            )
            .map(drop)
            .map_err(failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gamenode_core::{Locale, StartScope, ToolChannel};
    use rusqlite::types::ValueRef;
    use sha2::{Digest, Sha256};
    use tempfile::TempDir;

    #[test]
    fn shipped_migrations_have_pinned_versions_names_and_sha256() {
        let expected: &[(u32, &str, &str)] = &[
            (
                1,
                "initial_settings",
                "7b2a5ad625231bfeb90fda643fccbcc002a2e0f7ebf263f5bb28d064bda4d28c",
            ),
            (
                2,
                "library",
                "65792096f8ab65454754859815d39171c44c908b319901fb2289477af7cc36d0",
            ),
            (
                3,
                "playtime",
                "dd07f5a7f4729c0c96042c780b9e2f70916a6d5a1378fbad2a991c331573bc2b",
            ),
            (
                4,
                "drm_free_backups",
                "762aaa51096ac00b04c348ab8459ecc4b28021a70759e8ec0e7df413c648ed27",
            ),
            (
                5,
                "game_tools",
                "050b3530674c7e4ba55b612035334de9db0b0f5eb1a885f4a106d238258f7cec",
            ),
            (
                6,
                "store_accounts_and_catalog",
                "e9c7347c027545367177a6e2b6b5d79e131cb6f09ac76e808f20392b668cf37b",
            ),
            (
                7,
                "favorites",
                "e5d6983699c00e425cb3496ea916ab4c2358487a022116bc81da26fabf463bb8",
            ),
            (
                8,
                "ports",
                "528fd37f9b5c90f71c2409faa120ee86a600f2646fcdef2b50c65db72a97d244",
            ),
            (
                9,
                "achievements",
                "cf2636fb541ea76d52565025958043ae989f1956c9ed746f4025081c5f0f1ad8",
            ),
            (
                10,
                "library_organization",
                "0c76ddb272825004121c405f0f106b55e5d06efcd0bd7d708327faa8855f3122",
            ),
            (
                11,
                "game_platforms",
                "90c41bbf41c534c5ebb15e7cc9934bc3104f273aa4ae51df36b4bf1e79289833",
            ),
            (
                12,
                "achievement_rarity",
                "c763e8534e424af2db51187f89870e93cb4c16504114465113be11be52aea18a",
            ),
            (
                13,
                "backup_metadata",
                "8c07c9263930d88f12e8fbe86ffb729b72cc32632228e7a56d5922149f163a5d",
            ),
            (
                14,
                "local_cover",
                "0c2f5ed702bb3fd50064c6df603a97e11c709a245f18ad823f982754587fb05c",
            ),
            (
                15,
                "app_preferences",
                "af0f6df34d58e7e298549f277240b5a92ff78ae26398507528bf40fddb442231",
            ),
            (
                16,
                "sync_summary",
                "75be332fee38fff0b1627ad095cbac3288b062321135f989d6721ff424cf65b4",
            ),
            (
                17,
                "library_installations",
                "d2015385d8ea88b8b8313e8b8ab68d2d0991aa57dedc3d0bb8ec53b9e7f6a4c1",
            ),
            (
                18,
                "library_display_overrides",
                "3af7b42d870a0a9d27f5f618f1be768ad8738b620414519e003ced935a5d3086",
            ),
            (
                19,
                "chosen_match_source",
                "8d89fb6ed96fd3d11d707a9914b5be3f2c358a19db580137f6d993eab86c1c20",
            ),
            (
                20,
                "library_artwork_overrides",
                "df4489fba8c4919a5dac008426b0b4e197a0020d8d9541f55ad83cb92dc2d7f3",
            ),
            (
                21,
                "library_entry_targets",
                "32fe99e9201ca0872089eda9c4f9f1050f9b74ef8786f6e4ce3b3ee26f1ae074",
            ),
            (
                22,
                "plugin_custom_launch_recipes",
                "08b87ebc20b8ee86939f19e855e0a149f922ea8d2f4a190e03d1d8b88edfd028",
            ),
            (
                23,
                "plugin_runtime_resources",
                "5e1077e7dc8997eb08b3b6790932c6fc0f03e7c1c1fe92223d067cd288b72573",
            ),
            (
                24,
                "library_launch_recipe_selection",
                "134c3902d5f4919cde7ab9d86370045b30674f0bb99d56c910c60af2c120b820",
            ),
            (
                25,
                "plugin_bundles",
                "f5c58f3128a9561e4c129565be57b5f40fba7269108bc66dfad03efd7541c22c",
            ),
            (
                26,
                "plugin_bundle_history",
                "55793358d3b21538e3ea73bf7389138900d1bc56253ab6844285dd61351f0ddd",
            ),
        ];
        assert_eq!(MIGRATIONS.len(), expected.len());
        for (migration, (version, name, digest)) in MIGRATIONS.iter().zip(expected) {
            assert_eq!(migration.version, *version);
            assert_eq!(migration.name, *name);
            let actual = format!("{:x}", Sha256::digest(migration.sql.as_bytes()));
            assert_eq!(actual, *digest, "{}", migration.name);
        }
    }

    #[test]
    fn fresh_and_main_upgrade_databases_have_the_same_complete_schema() {
        let dir = TempDir::new().unwrap();
        let fresh = db_path(&dir).with_file_name("fresh.sqlite");
        let upgraded = db_path(&dir).with_file_name("main.sqlite");
        let fresh_conn = open(&fresh).unwrap();
        let main_conn = open_with(&upgraded, &MIGRATIONS[..16]).unwrap();
        assert_eq!(
            versions(&main_conn).last(),
            Some(&(16, "sync_summary".to_owned()))
        );
        drop(main_conn);
        let main_conn = open(&upgraded).unwrap();
        assert_eq!(versions(&main_conn), versions(&fresh_conn));
        assert_eq!(schema_rows(&fresh_conn), schema_rows(&main_conn));
    }

    fn db_path(dir: &TempDir) -> std::path::PathBuf {
        dir.path().join("test.sqlite3")
    }

    fn versions(conn: &Connection) -> Vec<(u32, String)> {
        let mut stmt = conn
            .prepare("SELECT version, name FROM schema_migrations ORDER BY version")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn migration_history(migrations: &[Migration]) -> Vec<(u32, String)> {
        migrations
            .iter()
            .map(|migration| (migration.version, migration.name.to_owned()))
            .collect()
    }

    fn quote_identifier(name: &str) -> String {
        format!("\"{}\"", name.replace('"', "\"\""))
    }

    fn table_names(conn: &Connection) -> Vec<String> {
        let mut statement = conn
            .prepare(
                "SELECT name FROM sqlite_master
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
                 ORDER BY name",
            )
            .unwrap();
        statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn table_columns(conn: &Connection, table: &str) -> Vec<String> {
        let mut statement = conn
            .prepare(&format!("PRAGMA table_info({})", quote_identifier(table)))
            .unwrap();
        statement
            .query_map([], |row| row.get(1))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn encoded_value(value: ValueRef<'_>) -> Vec<u8> {
        let (tag, bytes): (u8, Vec<u8>) = match value {
            ValueRef::Null => (b'n', Vec::new()),
            ValueRef::Integer(number) => (b'i', number.to_le_bytes().to_vec()),
            ValueRef::Real(number) => (b'r', number.to_le_bytes().to_vec()),
            ValueRef::Text(text) => (b't', text.to_vec()),
            ValueRef::Blob(blob) => (b'b', blob.to_vec()),
        };
        let mut encoded = Vec::with_capacity(bytes.len() + 9);
        encoded.push(tag);
        encoded.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        encoded.extend_from_slice(&bytes);
        encoded
    }

    /// Row count and a SHA-256 over the requested columns of every row, independent of row order.
    fn table_digest(conn: &Connection, table: &str, columns: &[String]) -> (i64, String) {
        let projection = columns
            .iter()
            .map(|column| quote_identifier(column))
            .collect::<Vec<_>>()
            .join(", ");
        let mut statement = conn
            .prepare(&format!(
                "SELECT {projection} FROM {}",
                quote_identifier(table)
            ))
            .unwrap();
        let mut rows = statement
            .query_map([], |row| {
                let mut encoded = Vec::new();
                for index in 0..columns.len() {
                    encoded.extend_from_slice(&encoded_value(row.get_ref(index)?));
                }
                Ok(encoded)
            })
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>();
        rows.sort();
        let mut hasher = Sha256::new();
        for row in &rows {
            hasher.update((row.len() as u64).to_le_bytes());
            hasher.update(row);
        }
        (rows.len() as i64, format!("{:x}", hasher.finalize()))
    }

    /// Representative, fictional data for every table that `main` shipped (schema versions 1-16).
    ///
    /// `main` is `origin/main` at `e37ccd1`; the tests below build this shape with
    /// `&MIGRATIONS[..16]`, whose SQL is byte-identical to `main`'s shipped migration files.
    fn seed_main_shaped_database(conn: &Connection) {
        conn.execute_batch(
            r#"
            INSERT INTO settings (key, value) VALUES ('window.width', '1280');
            INSERT INTO settings (key, value) VALUES ('account.last_id', 'acct-fixture');
            INSERT INTO library_entries
                (key, provider, store_id, name, install_dir, fields_json, installed,
                 first_seen, last_seen, owned, family_shared)
            VALUES ('fixture:1', 'fixture', '1', 'Fixture Game', '/games/fixture',
                    '{"state_flags":"4"}', 1, 100, 200, 1, 1);
            INSERT INTO provider_scans
                (provider, last_success_at, last_error, last_catalog_at, catalog_error,
                 last_family_at, family_error)
            VALUES ('fixture', 200, 'scan warning', 190, 'catalog warning', 195, NULL);
            INSERT INTO auto_matches (category, uid, game_ids_json, checked_at)
            VALUES (1, 'uid-1', '[42]', 110);
            INSERT INTO chosen_matches (account_id, entry_key, game_id, chosen_at)
            VALUES ('acct-fixture', 'fixture:1', 42, 120);
            INSERT INTO games
                (game_id, name, release_year, cover_url, cover_file, cover_failed_at, platforms)
            VALUES (42, 'Fixture Game', 2020, 'https://images.example/42.jpg', '42.jpg', NULL,
                    '["windows","linux"]');
            INSERT INTO playtime_sessions
                (id, account_id, entry_key, game_id, source, platform_id, started_at,
                 checkpoint_at, ended_at)
            VALUES ('session-1', 'acct-fixture', 'fixture:1', 42, 'fixture', 6, 100, 200, 300);
            INSERT INTO playtime_totals
                (account_id, entry_key, total_seconds, last_played_at, accepted_game_id,
                 accepted_seconds, rejected_seconds)
            VALUES ('acct-fixture', 'fixture:1', 3600, 300, 42, 3600, 0);
            INSERT INTO manual_registrations (id, file_identity, removed, payload)
            VALUES ('reg-1', 'identity-1', 0, '{"title":"Fixture Registration"}');
            INSERT INTO discovery_watches (id, payload)
            VALUES ('watch-1', '{"path":"/games/fixture"}');
            INSERT INTO discovery_candidates (fingerprint, payload)
            VALUES ('candidate-1', '{"title":"Fixture Candidate"}');
            INSERT INTO local_inferences (account_id, installation_id, payload)
            VALUES ('acct-fixture', 'installation-1', '{"game_id":42}');
            INSERT INTO save_profiles (id, installation_id, revision, payload)
            VALUES ('profile-1', 'installation-1', 3, '{"label":"Fixture Save"}');
            INSERT INTO backup_jobs
                (id, installation_id, profile_id, revision, play_period, destination,
                 state, payload)
            VALUES ('backup-job-1', 'installation-1', 'profile-1', 3, '2020-01-01',
                    '/backups', 'completed', '{"ok":true}');
            INSERT INTO backup_destinations (path) VALUES ('/backups');
            INSERT INTO backup_version_meta
                (installation, profile, backup_id, reason, play_seconds, created_at)
            VALUES ('installation-1', 'profile-1', 'backup-1', 'manual', 3600, 300);
            INSERT INTO tool_installations
                (entry_key, tool_id, channel, version, target_exe, proxy_dll, state,
                 installed_at, updated_at)
            VALUES ('fixture:1', 'fixture-tool', 'stable', '1.0.0', 'bin/game.exe', 'dxgi.dll',
                    'installed', 1000, 1000);
            INSERT INTO tool_files
                (entry_key, tool_id, relative_path, installed_sha256, backup_path,
                 original_sha256, prior_owner_tool_id, created_dirs_json, ini_patches_json,
                 ini_section, ini_key, ini_value, ini_prior_value)
            VALUES ('fixture:1', 'fixture-tool', 'dxgi.dll', 'installed-sha', '/backups/dxgi.dll',
                    'original-sha', NULL, '["bin"]', '[]', 'section', 'key', 'value', 'prior');
            INSERT INTO tool_releases_cache
                (tool_id, channel, version, tag_name, name, published_at, download_url,
                 asset_name, size_bytes, sha256, cached_path, fetched_at)
            VALUES ('fixture-tool', 'stable', '1.0.0', 'v1.0.0', 'Fixture Tool',
                    '2026-01-01', 'https://downloads.example/tool.7z', 'tool.7z', 1024,
                    'release-sha', NULL, 1000);
            INSERT INTO store_accounts (provider, account_id, account_name, last_verified_at)
            VALUES ('fixture', 'store-account-1', 'Fixture Account', 900);
            INSERT INTO favorites (account_id, game_id, is_favorite, pending, updated_at)
            VALUES ('acct-fixture', 42, 1, 0, 170);
            INSERT INTO port_catalog_snapshot (id, payload, fetched_at, entry_count)
            VALUES (1, '{"apps":["qcat_1"]}', 420, 1);
            INSERT INTO port_overlay (id, version, etag, payload)
            VALUES (1, 1, 'etag-v1', 'schema_version = 1');
            INSERT INTO port_install_jobs
                (id, catalog_id, slug, port_name, release_version, state, phase,
                 install_folder, created_paths, registration_id, result_seen,
                 plan_json, release_json, created_at, updated_at, finished_at)
            VALUES ('port-job-1', 'qcat_1', 'fixture-port', 'Fixture Port', 'v1', 'completed',
                    '"completed"', '/ports/fixture', '["/ports/fixture"]', 'port-reg-1', 1,
                    '{}', '{}', 430, 440, 440);
            INSERT INTO port_installations
                (registration_id, catalog_id, slug, port_name, installed_version,
                 install_folder, environment_json, job_id)
            VALUES ('port-reg-1', 'qcat_1', 'fixture-port', 'Fixture Port', 'v1',
                    '/ports/fixture', '{"ROLE":"gameplay"}', 'port-job-1');
            INSERT INTO port_preferences (id, view_mode, filters_json, install_parent)
            VALUES (1, 'list', '{"installed":true}', '/ports');
            INSERT INTO achievement_games
                (provider, store_account_id, entry_id, state, language, last_success_at,
                 last_attempt_at, failure)
            VALUES ('fixture', 'store-account-1', '1', 'ready', 'english', 500, 500, NULL);
            INSERT INTO achievements
                (provider, store_account_id, entry_id, achievement_id, name, description,
                 hidden, icon_url, icon_gray_url, unlocked, unlocked_at, position,
                 global_percent)
            VALUES ('fixture', 'store-account-1', '1', 'achievement-1', 'First', 'Do it', 0,
                    'https://images.example/icon.png', 'https://images.example/gray.png',
                    1, 510, 0, 420);
            INSERT INTO saved_views (account_id, name, payload, position, created_at)
            VALUES ('acct-fixture', 'Fixture View', '{"sort":"name"}', 0, 600);
            INSERT INTO hidden_entries (account_id, entry_key, hidden_at)
            VALUES ('acct-fixture', 'fixture:1', 610);
            INSERT INTO account_library_entries
                (account_id, game_id, entry_id, status, collection_ids)
            VALUES ('acct-fixture', 42, '1', 'owned', '["collection-1"]');
            INSERT INTO account_collections
                (account_id, collection_id, name, default_status, position)
            VALUES ('acct-fixture', 'collection-1', 'Fixture Collection', 'owned', 0);
            INSERT INTO account_library_reads (account_id, read_at)
            VALUES ('acct-fixture', 620);
            INSERT INTO sync_summaries (account_id, completed_at, accepted_games, stores, held)
            VALUES ('acct-fixture', 700, 1, 1, 0);
            "#,
        )
        .unwrap();
    }

    struct MainShapedSnapshot {
        tables: Vec<(String, Vec<String>, i64, String)>,
        history: Vec<(u32, String, i64)>,
    }

    fn snapshot_table_data(conn: &Connection) -> MainShapedSnapshot {
        let mut history_statement = conn
            .prepare("SELECT version, name, applied_at FROM schema_migrations ORDER BY version")
            .unwrap();
        let history = history_statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let mut tables = Vec::new();
        for table in table_names(conn) {
            if table == "schema_migrations" {
                continue;
            }
            let columns = table_columns(conn, &table);
            let (rows, digest) = table_digest(conn, &table, &columns);
            assert!(
                rows > 0,
                "the fixture has no representative row for {table}"
            );
            tables.push((table, columns, rows, digest));
        }
        MainShapedSnapshot { tables, history }
    }

    fn snapshot_fingerprint(snapshot: &MainShapedSnapshot) -> String {
        let mut hasher = Sha256::new();
        for (table, _, rows, digest) in &snapshot.tables {
            hasher.update(format!("{table}:{rows}:{digest}\n"));
        }
        format!("{:x}", hasher.finalize())
    }

    fn geometry() -> WindowGeometry {
        WindowGeometry {
            x: Some(-40),
            y: Some(25),
            width: 1280,
            height: 800,
            maximized: true,
        }
    }

    #[test]
    fn fresh_database_gets_the_schema_and_defaults() {
        let dir = TempDir::new().unwrap();
        let store = SqliteSettingsStore::open(&db_path(&dir)).unwrap();
        assert_eq!(store.load().unwrap(), Settings::default());
        assert_eq!(versions(&store.conn()), migration_history(MIGRATIONS));
    }

    #[test]
    fn backup_metadata_migration_adds_the_table_and_defaults_legacy_retention() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        {
            let conn = open_with(&path, &MIGRATIONS[..12]).unwrap();
            conn.execute(
                "INSERT INTO settings (key, value) VALUES ('backup_settings', ?1)",
                [r#"{"automatic":true,"destination":"D:\\Backups"}"#],
            )
            .unwrap();
        }

        let store = SqliteSettingsStore::open(&path).unwrap();
        let conn = store.conn();
        assert_eq!(
            versions(&conn).last(),
            Some(&(26, "plugin_bundle_history".to_owned()))
        );
        let payload: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key='backup_settings'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<gamenode_core::drm_free::BackupSettings>(&payload)
                .unwrap()
                .retention,
            10
        );
        let columns: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('backup_version_meta')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(columns, 6);
    }

    #[test]
    fn local_cover_migration_makes_the_field_explicit_and_keeps_legacy_rows() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        {
            let conn = open_with(&path, &MIGRATIONS[..13]).unwrap();
            conn.execute(
                "INSERT INTO manual_registrations (id, file_identity, removed, payload)
                 VALUES ('reg1', 'fi', 0, ?1)",
                [r#"{"id":"reg1","title":"Game","executable":"C:/Game/game.exe","fileIdentity":"fi","root":"C:/Game","workingDirectory":"C:/Game","arguments":[],"gameplay":[],"launchers":[],"revision":1,"removed":false,"available":true}"#],
            )
            .unwrap();
        }

        let store = SqliteSettingsStore::open(&path).unwrap();
        let conn = store.conn();
        assert_eq!(
            versions(&conn).last(),
            Some(&(26, "plugin_bundle_history".to_owned()))
        );
        let kind: String = conn
            .query_row(
                "SELECT json_type(payload, '$.customCover') FROM manual_registrations WHERE id='reg1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(kind, "null");
        let payload: String = conn
            .query_row(
                "SELECT payload FROM manual_registrations WHERE id='reg1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let registration: gamenode_core::drm_free::Registration =
            serde_json::from_str(&payload).unwrap();
        assert_eq!(registration.custom_cover, None);
    }

    #[test]
    fn reopening_keeps_settings_and_does_not_reapply_migrations() {
        let dir = TempDir::new().unwrap();
        let saved = Settings {
            window: Some(geometry()),
            ..Settings::default()
        };
        {
            let store = SqliteSettingsStore::open(&db_path(&dir)).unwrap();
            store.save(&saved).unwrap();
            // A marker proves migration 1 is not rerun (it would fail: table exists).
            store
                .conn()
                .execute(
                    "UPDATE schema_migrations SET applied_at = 7 WHERE version = 1",
                    [],
                )
                .unwrap();
        }
        let store = SqliteSettingsStore::open(&db_path(&dir)).unwrap();
        assert_eq!(store.load().unwrap(), saved);
        let applied_at: i64 = store
            .conn()
            .query_row("SELECT applied_at FROM schema_migrations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(applied_at, 7);
        assert_eq!(versions(&store.conn()).len(), MIGRATIONS.len());
    }

    #[test]
    fn window_without_position_and_cleared_geometry_round_trip() {
        let dir = TempDir::new().unwrap();
        let store = SqliteSettingsStore::open(&db_path(&dir)).unwrap();
        let mut s = Settings {
            window: Some(geometry()),
            ..Settings::default()
        };
        s.window.as_mut().unwrap().x = None;
        s.window.as_mut().unwrap().y = None;
        store.save(&s).unwrap();
        assert_eq!(store.load().unwrap(), s);
        s.window = None;
        store.save(&s).unwrap();
        assert_eq!(store.load().unwrap().window, None);
    }

    #[test]
    fn corrupt_values_fall_back_to_defaults_instead_of_failing_startup() {
        let dir = TempDir::new().unwrap();
        let store = SqliteSettingsStore::open(&db_path(&dir)).unwrap();
        {
            let conn = store.conn();
            conn.execute(
                "UPDATE settings SET value = 'turbo' WHERE key = 'renderer'",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO settings VALUES ('window.width', 'wide'), ('window.height', '600')",
                [],
            )
            .unwrap();
        }
        assert_eq!(store.load().unwrap(), Settings::default());
    }

    #[test]
    fn an_old_database_gains_every_preference_default() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        drop(open_with(&path, &MIGRATIONS[..14]).unwrap());

        let store = SqliteSettingsStore::open(&path).unwrap();
        assert_eq!(
            versions(&store.conn()).last(),
            Some(&(26, "plugin_bundle_history".to_owned()))
        );
        assert_eq!(store.load().unwrap().preferences, AppPreferences::default());

        // Every preference has its own explicit row after the migration.
        let stored: Vec<String> = store
            .conn()
            .prepare("SELECT key FROM settings WHERE key LIKE 'app.%' ORDER BY key")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            stored,
            [
                "app.closeToTray",
                "app.discoveryOnStart",
                "app.discoveryRealtime",
                "app.locale",
                "app.sharedExcluded",
                "app.showStatusBar",
                "app.startMinimized",
                "app.startScope",
                "app.startWithWindows",
                "app.toolAutoCheck",
                "app.toolDefaultChannel",
            ]
        );
    }

    #[test]
    fn each_unreadable_preference_falls_back_alone() {
        let dir = TempDir::new().unwrap();
        let store = SqliteSettingsStore::open(&db_path(&dir)).unwrap();
        {
            let conn = store.conn();
            conn.execute(
                "UPDATE settings SET value = '\"klingon\"' WHERE key = 'app.locale'",
                [],
            )
            .unwrap();
            conn.execute(
                "UPDATE settings SET value = 'false' WHERE key = 'app.showStatusBar'",
                [],
            )
            .unwrap();
            conn.execute(
                "UPDATE settings SET value = 'not json' WHERE key = 'app.sharedExcluded'",
                [],
            )
            .unwrap();
            conn.execute(
                "UPDATE settings SET value = '{}' WHERE key = 'app.startWithWindows'",
                [],
            )
            .unwrap();
        }

        let preferences = store.load().unwrap().preferences;
        assert_eq!(preferences.locale, Locale::System, "unreadable enum");
        assert!(!preferences.show_status_bar, "readable value is kept");
        assert!(
            preferences.shared_excluded.is_empty(),
            "unreadable set falls back alone"
        );
        assert!(
            preferences.start_with_windows,
            "unreadable bool falls back alone"
        );
        assert_eq!(preferences.start_scope, StartScope::Last);
        assert_eq!(preferences.tool_default_channel, ToolChannel::Stable);
    }

    #[test]
    fn every_preference_round_trips_and_saving_preferences_keeps_the_window() {
        let dir = TempDir::new().unwrap();
        let store = SqliteSettingsStore::open(&db_path(&dir)).unwrap();
        let saved = Settings {
            window: Some(geometry()),
            ..Settings::default()
        };
        store.save(&saved).unwrap();
        assert_eq!(store.load().unwrap(), saved);

        let changed = AppPreferences {
            start_with_windows: false,
            start_minimized: false,
            close_to_tray: false,
            locale: Locale::PtBr,
            show_status_bar: false,
            start_scope: StartScope::Installed,
            discovery_on_start: false,
            discovery_realtime: false,
            tool_auto_check: false,
            tool_default_channel: ToolChannel::Nightly,
            shared_excluded: std::collections::BTreeSet::from([
                "epic".to_owned(),
                "steam".to_owned(),
            ]),
        };
        store.save_preferences(&changed).unwrap();

        let loaded = store.load().unwrap();
        assert_eq!(loaded.preferences, changed);
        assert_eq!(loaded.window, saved.window, "window geometry is untouched");

        // Reopening the same file keeps every preference.
        drop(store);
        let reopened = SqliteSettingsStore::open(&db_path(&dir)).unwrap();
        assert_eq!(reopened.load().unwrap().preferences, changed);
    }

    #[test]
    fn window_and_preference_saves_are_independent_in_either_order() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        let store = SqliteSettingsStore::open(&path).unwrap();
        let first = WindowGeometry {
            x: Some(3),
            y: Some(4),
            width: 1024,
            height: 768,
            maximized: false,
        };
        store
            .save(&Settings {
                window: Some(first),
                ..Settings::default()
            })
            .unwrap();

        let preferences = AppPreferences {
            locale: Locale::PtBr,
            show_status_bar: false,
            ..AppPreferences::default()
        };
        store.save_preferences(&preferences).unwrap();
        let second = WindowGeometry {
            x: Some(7),
            y: None,
            width: 1280,
            height: 800,
            maximized: true,
        };
        store.save_window(Some(second)).unwrap();

        // Reopen the file so the assertion covers durable rows, not one writer's view.
        drop(store);
        let reopened = SqliteSettingsStore::open(&path).unwrap();
        let loaded = reopened.load().unwrap();
        assert_eq!(loaded.window, Some(second));
        assert_eq!(loaded.preferences, preferences);

        // The opposite order keeps both values too.
        let third = WindowGeometry {
            x: Some(9),
            y: Some(10),
            width: 1440,
            height: 900,
            maximized: false,
        };
        reopened.save_window(Some(third)).unwrap();
        let later = AppPreferences {
            locale: Locale::En,
            ..preferences
        };
        reopened.save_preferences(&later).unwrap();
        drop(reopened);

        let reloaded = SqliteSettingsStore::open(&path).unwrap();
        let loaded = reloaded.load().unwrap();
        assert_eq!(loaded.window, Some(third));
        assert_eq!(loaded.preferences, later);

        // Clearing geometry also leaves preferences alone.
        reloaded.save_window(None).unwrap();
        drop(reloaded);
        let cleared = SqliteSettingsStore::open(&path).unwrap();
        let loaded = cleared.load().unwrap();
        assert_eq!(loaded.window, None);
        assert_eq!(loaded.preferences, later);
    }

    #[test]
    fn preferences_are_independent_of_the_signed_in_account() {
        use gamenode_core::library::{LastAccount, LibraryStore};

        let dir = TempDir::new().unwrap();
        let store = SqliteSettingsStore::open(&db_path(&dir)).unwrap();
        let preferences = AppPreferences {
            locale: Locale::En,
            ..AppPreferences::default()
        };
        store.save_preferences(&preferences).unwrap();

        let library = store.library_store();
        library
            .save_last_account(&LastAccount {
                id: "account-1".to_owned(),
                name: "First".to_owned(),
            })
            .unwrap();
        library.clear_last_account().unwrap();

        assert_eq!(store.load().unwrap().preferences, preferences);
    }

    #[test]
    fn legacy_renderer_row_is_ignored_when_loading_settings() {
        let dir = TempDir::new().unwrap();
        let store = SqliteSettingsStore::open(&db_path(&dir)).unwrap();
        let geometry = geometry();
        store
            .save(&Settings {
                window: Some(geometry),
                ..Settings::default()
            })
            .unwrap();

        for legacy in ["software", "gpu", "turbo", "invalid"] {
            store
                .conn()
                .execute(
                    "UPDATE settings SET value = ?1 WHERE key = 'renderer'",
                    [legacy],
                )
                .unwrap();
            assert_eq!(
                store.load().unwrap(),
                Settings {
                    window: Some(geometry),
                    ..Settings::default()
                }
            );
        }
    }

    const V17_OK: Migration = Migration {
        version: 17,
        name: "add_notes",
        sql: "CREATE TABLE notes (id INTEGER PRIMARY KEY) STRICT;",
    };
    const V17_BAD: Migration = Migration {
        version: 17,
        name: "broken",
        sql: "CREATE TABLE half_done (id INTEGER PRIMARY KEY); INSERT INTO missing_table VALUES (1);",
    };

    #[test]
    fn older_database_is_upgraded_in_order_preserving_settings() {
        let dir = TempDir::new().unwrap();
        let saved = Settings {
            window: Some(geometry()),
            ..Settings::default()
        };
        SqliteSettingsStore::open_with_for_test(&db_path(&dir), &MIGRATIONS[..16])
            .save(&saved)
            .unwrap();

        let all = [
            MIGRATIONS[0],
            MIGRATIONS[1],
            MIGRATIONS[2],
            MIGRATIONS[3],
            MIGRATIONS[4],
            MIGRATIONS[5],
            MIGRATIONS[6],
            MIGRATIONS[7],
            MIGRATIONS[8],
            MIGRATIONS[9],
            MIGRATIONS[10],
            MIGRATIONS[11],
            MIGRATIONS[12],
            MIGRATIONS[13],
            MIGRATIONS[14],
            MIGRATIONS[15],
            V17_OK,
        ];
        let conn = open_with(&db_path(&dir), &all).unwrap();
        assert_eq!(
            versions(&conn),
            vec![
                (1, "initial_settings".into()),
                (2, "library".into()),
                (3, "playtime".into()),
                (4, "drm_free_backups".into()),
                (5, "game_tools".into()),
                (6, "store_accounts_and_catalog".into()),
                (7, "favorites".into()),
                (8, "ports".into()),
                (9, "achievements".into()),
                (10, "library_organization".into()),
                (11, "game_platforms".into()),
                (12, "achievement_rarity".into()),
                (13, "backup_metadata".into()),
                (14, "local_cover".into()),
                (15, "app_preferences".into()),
                (16, "sync_summary".into()),
                (17, "add_notes".into())
            ]
        );
        conn.execute("INSERT INTO notes DEFAULT VALUES", [])
            .unwrap();
        drop(conn);
        assert_eq!(
            SqliteSettingsStore::open_with_for_test(&db_path(&dir), &all)
                .load()
                .unwrap(),
            saved
        );
    }

    impl SqliteSettingsStore {
        fn open_with_for_test(path: &Path, migrations: &[Migration]) -> Self {
            Self::new(open_with(path, migrations).unwrap())
        }
    }

    #[test]
    fn failed_migration_rolls_back_and_keeps_earlier_data() {
        let dir = TempDir::new().unwrap();
        let saved = Settings {
            window: None,
            ..Settings::default()
        };
        SqliteSettingsStore::open_with_for_test(&db_path(&dir), &MIGRATIONS[..16])
            .save(&saved)
            .unwrap();

        let err = open_with(
            &db_path(&dir),
            &[
                MIGRATIONS[0],
                MIGRATIONS[1],
                MIGRATIONS[2],
                MIGRATIONS[3],
                MIGRATIONS[4],
                MIGRATIONS[5],
                MIGRATIONS[6],
                MIGRATIONS[7],
                MIGRATIONS[8],
                MIGRATIONS[9],
                MIGRATIONS[10],
                MIGRATIONS[11],
                MIGRATIONS[12],
                MIGRATIONS[13],
                MIGRATIONS[14],
                MIGRATIONS[15],
                V17_BAD,
            ],
        )
        .unwrap_err();
        assert_eq!(err, StorageError::MigrationFailed { version: 17 });

        let conn = Connection::open(db_path(&dir)).unwrap();
        assert_eq!(
            versions(&conn),
            vec![
                (1, "initial_settings".to_string()),
                (2, "library".to_string()),
                (3, "playtime".to_string()),
                (4, "drm_free_backups".to_string()),
                (5, "game_tools".to_string()),
                (6, "store_accounts_and_catalog".to_string()),
                (7, "favorites".to_string()),
                (8, "ports".to_string()),
                (9, "achievements".to_string()),
                (10, "library_organization".to_string()),
                (11, "game_platforms".to_string()),
                (12, "achievement_rarity".to_string()),
                (13, "backup_metadata".to_string()),
                (14, "local_cover".to_string()),
                (15, "app_preferences".to_string()),
                (16, "sync_summary".to_string())
            ]
        );
        let leaked: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'half_done'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(leaked, 0, "the partial migration was rolled back");
        assert_eq!(SqliteSettingsStore::new(conn).load().unwrap(), saved);
    }

    #[test]
    fn mismatched_migration_history_is_rejected_without_changing_the_file() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        drop(open_with(&path, &MIGRATIONS[..3]).unwrap());
        Connection::open(&path)
            .unwrap()
            .execute(
                "UPDATE schema_migrations SET name = 'unexpected_name' WHERE version = 2",
                [],
            )
            .unwrap();
        let before = std::fs::read(&path).unwrap();

        assert!(matches!(open(&path), Err(StorageError::Failed(_))));
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn integration_branch_migration_history_is_rejected_without_changing_the_file() {
        // This is the unpublished history used by the integration branch before reconciliation.
        // Several version numbers overlap with main but identify different migrations.
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL);
             CREATE TABLE integration_user_data (id TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO integration_user_data VALUES ('sentinel', 'preserve me');",
        )
        .unwrap();
        let integration_history = [
            "initial_settings",
            "library",
            "playtime",
            "drm_free_backups",
            "game_tools",
            "store_accounts_and_catalog",
            "favorites",
            "ports",
            "achievements",
            "library_installations",
            "library_display_overrides",
            "chosen_match_source",
            "library_artwork_overrides",
            "library_entry_targets",
            "library_organization",
            "plugin_custom_launch_recipes",
            "plugin_runtime_resources",
            "library_launch_recipe_selection",
        ];
        for (index, name) in integration_history.iter().enumerate() {
            conn.execute(
                "INSERT INTO schema_migrations (version, name) VALUES (?1, ?2)",
                params![index as u32 + 1, name],
            )
            .unwrap();
        }
        drop(conn);

        let before = std::fs::read(&path).unwrap();
        assert!(matches!(open(&path), Err(StorageError::Failed(_))));
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn failed_upgrade_keeps_the_latest_verified_snapshot_with_user_data() {
        const V18_BAD: Migration = Migration {
            version: 18,
            name: "broken_second_upgrade",
            sql: "CREATE TABLE half_done (id INTEGER PRIMARY KEY); INSERT INTO missing_table VALUES (1);",
        };

        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        let saved = Settings {
            window: Some(geometry()),
            ..Settings::default()
        };
        SqliteSettingsStore::open_with_for_test(&path, &MIGRATIONS[..16])
            .save(&saved)
            .unwrap();

        let mut through_v17 = MIGRATIONS[..16].to_vec();
        through_v17.push(V17_OK);
        drop(open_with(&path, &through_v17).unwrap());

        let mut failing_upgrade = through_v17.clone();
        failing_upgrade.push(V18_BAD);
        let error = open_with(&path, &failing_upgrade).unwrap_err();
        assert_eq!(error, StorageError::MigrationFailed { version: 18 });
        assert_eq!(
            error.to_string(),
            "local database migration 18 failed and was rolled back"
        );

        let snapshot_prefix = format!(
            "{}.pre-migration-",
            path.file_name().unwrap().to_string_lossy()
        );
        let snapshots = std::fs::read_dir(dir.path())
            .unwrap()
            .map(Result::unwrap)
            .map(|entry| entry.path())
            .filter(|candidate| {
                candidate
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(&snapshot_prefix))
            })
            .collect::<Vec<_>>();
        assert_eq!(snapshots.len(), 1, "only the latest snapshot is retained");

        let snapshot =
            Connection::open_with_flags(&snapshots[0], rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        let integrity: String = snapshot
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .unwrap();
        assert_eq!(integrity, "ok");
        assert_eq!(versions(&snapshot), migration_history(&through_v17));
        assert_eq!(SqliteSettingsStore::new(snapshot).load().unwrap(), saved);

        let upgraded = Connection::open(path).unwrap();
        assert_eq!(versions(&upgraded), migration_history(&through_v17));
        let leaked: i64 = upgraded
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'half_done'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(leaked, 0, "the failed migration rolled back");
    }

    #[test]
    fn newer_schema_is_rejected_and_the_file_is_unchanged() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        let mut future = MIGRATIONS.to_vec();
        future.push(Migration {
            version: CURRENT_SCHEMA_VERSION + 1,
            ..V17_OK
        });
        drop(open_with(&path, &future).unwrap());
        let before = std::fs::read(&path).unwrap();

        assert_eq!(
            open(&path).unwrap_err(),
            StorageError::NewerSchema {
                found: CURRENT_SCHEMA_VERSION + 1,
                supported: CURRENT_SCHEMA_VERSION,
            }
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn a_favorites_build_refuses_a_ports_database_and_leaves_it_unchanged() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        drop(open(&path).unwrap());
        let before = std::fs::read(&path).unwrap();

        // The build that shipped the favorites schema knows version 7 only.
        let err = open_with(&path, &MIGRATIONS[..7]).unwrap_err();
        assert_eq!(
            err,
            StorageError::NewerSchema {
                found: CURRENT_SCHEMA_VERSION,
                supported: 7
            }
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "a newer database is not modified"
        );
    }

    /// The migrations a step 1 build shipped.
    fn step_one() -> &'static [Migration] {
        &MIGRATIONS[..1]
    }

    #[test]
    fn a_step_one_database_gains_the_library_tables_and_keeps_its_settings() {
        let dir = TempDir::new().unwrap();
        let saved = Settings {
            window: Some(geometry()),
            ..Settings::default()
        };
        drop(Connection::open(db_path(&dir)).unwrap());
        SqliteSettingsStore::new(open_with(&db_path(&dir), step_one()).unwrap())
            .save(&saved)
            .unwrap();

        let store = SqliteSettingsStore::open(&db_path(&dir)).unwrap();
        assert_eq!(store.load().unwrap(), saved);
        assert_eq!(versions(&store.conn()), migration_history(MIGRATIONS));
        for table in [
            "library_entries",
            "provider_scans",
            "auto_matches",
            "chosen_matches",
            "games",
            "store_accounts",
            "favorites",
            "port_catalog_snapshot",
            "port_overlay",
            "port_install_jobs",
            "port_installations",
            "port_preferences",
            "saved_views",
            "hidden_entries",
            "account_library_entries",
            "account_collections",
            "account_library_reads",
        ] {
            let rows: i64 = store
                .conn()
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(rows, 0, "{table} starts empty");
        }
    }

    #[test]
    fn a_step_one_build_leaves_a_library_database_untouched() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        drop(open(&path).unwrap());
        let before = std::fs::read(&path).unwrap();

        let err = open_with(&path, step_one()).unwrap_err();
        assert_eq!(
            err,
            StorageError::NewerSchema {
                found: CURRENT_SCHEMA_VERSION,
                supported: 1
            }
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn a_foreign_database_is_not_adopted() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        Connection::open(&path)
            .unwrap()
            .execute_batch("CREATE TABLE other (x)")
            .unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(matches!(open(&path), Err(StorageError::Failed(_))));
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn migration_lists_must_be_sequential() {
        let dir = TempDir::new().unwrap();
        let skipping = Migration {
            version: 4,
            ..V17_OK
        };
        assert!(open_with(&db_path(&dir), &[MIGRATIONS[0], MIGRATIONS[1], skipping]).is_err());
    }

    #[test]
    fn v4_database_is_upgraded_to_v5_creating_tool_tables_and_preserving_prior_data() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        // Build a v4 database with prior data
        let v4_migrations = &MIGRATIONS[..4];
        {
            let conn = open_with(&path, v4_migrations).unwrap();
            conn.execute(
                "INSERT INTO manual_registrations (id, file_identity, removed, payload) VALUES ('reg1', 'exe_hash_1', 0, '{\"title\":\"Game 1\"}')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO settings (key, value) VALUES ('window.width', '1920')",
                [],
            )
            .unwrap();
        }

        // Open with full migrations (v5)
        let store = SqliteSettingsStore::open(&path).unwrap();
        let conn = store.conn();

        // Check versions
        assert_eq!(versions(&conn), migration_history(MIGRATIONS));

        // Verify tool tables exist and are empty
        for table in ["tool_installations", "tool_files", "tool_releases_cache"] {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, 0, "{table} starts empty");
        }

        // Verify prior data preserved
        let title: String = conn
            .query_row(
                "SELECT payload FROM manual_registrations WHERE id = 'reg1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(title, "{\"title\":\"Game 1\",\"customCover\":null}");
        let width: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'window.width'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(width, "1920");

        // Verify inserting and querying tool_installations, tool_files, and tool_releases_cache works
        conn.execute(
            "INSERT INTO tool_installations (entry_key, tool_id, channel, version, target_exe, proxy_dll, state, installed_at, updated_at) VALUES ('steam:123', 'optiscaler', 'stable', '0.7.9', 'bin/game.exe', 'dxgi.dll', 'installed', 1000, 1000)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tool_files (entry_key, tool_id, relative_path, installed_sha256, backup_path, original_sha256, prior_owner_tool_id, created_dirs_json, ini_patches_json) VALUES ('steam:123', 'optiscaler', 'dxgi.dll', 'sha256_mock', NULL, NULL, NULL, '[]', '[]')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tool_releases_cache (tool_id, channel, version, tag_name, name, published_at, download_url, asset_name, size_bytes, sha256, cached_path, fetched_at) VALUES ('optiscaler', 'stable', '0.7.9', 'v0.7.9', 'OptiScaler 0.7.9', '2026-01-01', 'https://github.com/...', 'asset.7z', 1024, 'sha', NULL, 1000)",
            [],
        )
        .unwrap();

        let tool_version: String = conn
            .query_row(
                "SELECT version FROM tool_installations WHERE entry_key = 'steam:123' AND tool_id = 'optiscaler'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tool_version, "0.7.9");
    }

    #[test]
    fn v5_database_is_upgraded_to_v6_preserving_library_matches_covers_and_playtime() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        {
            let conn = open_with(&path, &MIGRATIONS[..5]).unwrap();
            conn.execute(
                "INSERT INTO settings (key, value) VALUES ('window.width', '1920')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO library_entries
                    (key, provider, store_id, name, install_dir, fields_json, installed, first_seen, last_seen)
                 VALUES ('steam:620', 'steam', '620', 'Portal 2', 'C:/Games/Portal 2',
                         '{\"state_flags\":\"4\"}', 1, 100, 150)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO provider_scans (provider, last_success_at, last_error)
                 VALUES ('steam', 150, NULL)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO auto_matches (category, uid, game_ids_json, checked_at)
                 VALUES (1, '620', '[72]', 120)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO chosen_matches (account_id, entry_key, game_id, chosen_at)
                 VALUES ('acct', 'steam:620', 72, 130)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO games (game_id, name, release_year, cover_url, cover_file, cover_failed_at)
                 VALUES (72, 'Portal 2', 2011, 'https://images.example/co1.jpg', '72.jpg', NULL)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO playtime_sessions
                    (id, account_id, entry_key, game_id, source, platform_id, started_at, checkpoint_at, ended_at)
                 VALUES ('s1', 'acct', 'steam:620', 72, 'steam', 6, 100, 200, 300)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO playtime_totals
                    (account_id, entry_key, total_seconds, last_played_at, accepted_game_id,
                     accepted_seconds, rejected_seconds)
                 VALUES ('acct', 'steam:620', 3600, 300, 72, 3600, NULL)",
                [],
            )
            .unwrap();
        }

        let store = SqliteSettingsStore::open(&path).unwrap();
        let conn = store.conn();
        assert_eq!(versions(&conn), migration_history(MIGRATIONS));

        let entry: (String, String, i64, i64, i64, i64) = conn
            .query_row(
                "SELECT name, install_dir, installed, owned, family_shared, first_seen
                 FROM library_entries WHERE key = 'steam:620'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            entry,
            ("Portal 2".into(), "C:/Games/Portal 2".into(), 1, 0, 0, 100),
            "installed entries stay visible and ownership starts false"
        );

        type StoredScan = (
            Option<i64>,
            Option<String>,
            Option<i64>,
            Option<String>,
            Option<i64>,
        );
        let scan: StoredScan = conn
            .query_row(
                "SELECT last_success_at, last_error, last_catalog_at, catalog_error, last_family_at
                 FROM provider_scans WHERE provider = 'steam'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(scan, (Some(150), None, None, None, None));
        let family_error: Option<String> = conn
            .query_row(
                "SELECT family_error FROM provider_scans WHERE provider = 'steam'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(family_error, None, "catalog sync state starts empty");

        assert_eq!(
            conn.query_row("SELECT checked_at FROM auto_matches", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            120
        );
        assert_eq!(
            conn.query_row(
                "SELECT game_id FROM chosen_matches WHERE account_id = 'acct'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            72
        );
        assert_eq!(
            conn.query_row("SELECT cover_file FROM games WHERE game_id = 72", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "72.jpg"
        );
        assert_eq!(
            conn.query_row("SELECT total_seconds FROM playtime_totals", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            3600
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM playtime_sessions", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM store_accounts", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row(
                "SELECT value FROM settings WHERE key = 'window.width'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "1920"
        );
    }

    #[test]
    fn v7_database_is_upgraded_to_v8_creating_port_tables_and_preserving_all_data() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        {
            let conn = open_with(&path, &MIGRATIONS[..7]).unwrap();
            conn.execute_batch(
                r#"
                INSERT INTO settings (key, value) VALUES ('window.width', '1920');
                INSERT INTO library_entries
                    (key, provider, store_id, name, install_dir, fields_json, installed,
                     first_seen, last_seen, owned, family_shared)
                VALUES ('steam:620', 'steam', '620', 'Portal 2', 'C:/Games/Portal 2',
                        '{"state_flags":"4"}', 1, 100, 150, 1, 0);
                INSERT INTO provider_scans
                    (provider, last_success_at, last_error, last_catalog_at, catalog_error,
                     last_family_at, family_error)
                VALUES ('steam', 150, NULL, 140, NULL, 145, NULL);
                INSERT INTO auto_matches (category, uid, game_ids_json, checked_at)
                VALUES (1, '620', '[72]', 120);
                INSERT INTO chosen_matches (account_id, entry_key, game_id, chosen_at)
                VALUES ('acct', 'steam:620', 72, 130);
                INSERT INTO games
                    (game_id, name, release_year, cover_url, cover_file, cover_failed_at)
                VALUES (72, 'Portal 2', 2011, 'https://images.example/co1.jpg', '72.jpg', NULL);
                INSERT INTO playtime_sessions
                    (id, account_id, entry_key, game_id, source, platform_id, started_at,
                     checkpoint_at, ended_at)
                VALUES ('s1', 'acct', 'steam:620', 72, 'steam', 6, 100, 200, 300);
                INSERT INTO playtime_totals
                    (account_id, entry_key, total_seconds, last_played_at, accepted_game_id,
                     accepted_seconds, rejected_seconds)
                VALUES ('acct', 'steam:620', 3600, 300, 72, 3600, NULL);
                INSERT INTO manual_registrations (id, file_identity, removed, payload)
                VALUES ('reg1', 'exe_hash_1', 0, '{"title":"Game 1"}');
                INSERT INTO discovery_watches (id, payload) VALUES ('w1', '{"path":"D:/Games"}');
                INSERT INTO discovery_candidates (fingerprint, payload)
                VALUES ('fp1', '{"title":"Found"}');
                INSERT INTO local_inferences (account_id, installation_id, payload)
                VALUES ('acct', 'reg1', '{"game_id":72}');
                INSERT INTO save_profiles (id, installation_id, revision, payload)
                VALUES ('p1', 'reg1', 1, '{"label":"Save"}');
                INSERT INTO backup_jobs
                    (id, installation_id, profile_id, revision, play_period, destination,
                     state, payload)
                VALUES ('j1', 'reg1', 'p1', 1, NULL, 'D:/Backups', 'Completed', '{"ok":true}');
                INSERT INTO backup_destinations (path) VALUES ('D:/Backups');
                INSERT INTO store_accounts (provider, account_id, account_name, last_verified_at)
                VALUES ('steam', '76561198000000000', 'Ana', 160);
                INSERT INTO tool_installations
                    (entry_key, tool_id, channel, version, target_exe, proxy_dll, state,
                     installed_at, updated_at)
                VALUES ('steam:620', 'optiscaler', 'stable', '0.7.9', 'bin/game.exe', 'dxgi.dll',
                        'installed', 1000, 1000);
                INSERT INTO tool_files
                    (entry_key, tool_id, relative_path, installed_sha256, backup_path,
                     original_sha256, prior_owner_tool_id, created_dirs_json, ini_patches_json,
                     ini_section, ini_key, ini_value, ini_prior_value)
                VALUES ('steam:620', 'optiscaler', 'dxgi.dll', 'sha256_mock', NULL, NULL, NULL,
                        '[]', '[]', NULL, NULL, NULL, NULL);
                INSERT INTO tool_releases_cache
                    (tool_id, channel, version, tag_name, name, published_at, download_url,
                     asset_name, size_bytes, sha256, cached_path, fetched_at)
                VALUES ('optiscaler', 'stable', '0.7.9', 'v0.7.9', 'OptiScaler 0.7.9',
                        '2026-01-01', 'https://github.com/example/asset.7z', 'asset.7z', 1024,
                        'sha', NULL, 1000);
                INSERT INTO favorites (account_id, game_id, is_favorite, pending, updated_at)
                VALUES ('acct', 72, 1, 0, 170);
                "#,
            )
            .unwrap();
        }

        let store = SqliteSettingsStore::open(&path).unwrap();
        let conn = store.conn();

        assert_eq!(versions(&conn), migration_history(MIGRATIONS));

        // Every table that existed at the favorites schema keeps its row.
        for table in [
            "auto_matches",
            "backup_destinations",
            "backup_jobs",
            "chosen_matches",
            "discovery_candidates",
            "discovery_watches",
            "favorites",
            "games",
            "library_entries",
            "local_inferences",
            "manual_registrations",
            "playtime_sessions",
            "playtime_totals",
            "provider_scans",
            "save_profiles",
            "store_accounts",
            "tool_files",
            "tool_installations",
            "tool_releases_cache",
        ] {
            let rows: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(rows, 1, "{table} keeps its row");
        }

        // Values in every pre-existing table survive the upgrade unchanged.
        let checks = [
            (
                "SELECT value FROM settings WHERE key = 'window.width'",
                "1920",
            ),
            (
                "SELECT name FROM library_entries WHERE key = 'steam:620'",
                "Portal 2",
            ),
            (
                "SELECT CAST(last_catalog_at AS TEXT) FROM provider_scans WHERE provider = 'steam'",
                "140",
            ),
            (
                "SELECT game_ids_json FROM auto_matches WHERE uid = '620'",
                "[72]",
            ),
            (
                "SELECT CAST(game_id AS TEXT) FROM chosen_matches WHERE account_id = 'acct'",
                "72",
            ),
            ("SELECT cover_file FROM games WHERE game_id = 72", "72.jpg"),
            (
                "SELECT CAST(ended_at AS TEXT) FROM playtime_sessions WHERE id = 's1'",
                "300",
            ),
            (
                "SELECT CAST(total_seconds AS TEXT) FROM playtime_totals",
                "3600",
            ),
            (
                "SELECT payload FROM manual_registrations WHERE id = 'reg1'",
                "{\"title\":\"Game 1\",\"customCover\":null}",
            ),
            (
                "SELECT payload FROM discovery_watches WHERE id = 'w1'",
                "{\"path\":\"D:/Games\"}",
            ),
            (
                "SELECT payload FROM discovery_candidates WHERE fingerprint = 'fp1'",
                "{\"title\":\"Found\"}",
            ),
            (
                "SELECT payload FROM local_inferences WHERE installation_id = 'reg1'",
                "{\"game_id\":72}",
            ),
            (
                "SELECT CAST(revision AS TEXT) FROM save_profiles WHERE id = 'p1'",
                "1",
            ),
            ("SELECT state FROM backup_jobs WHERE id = 'j1'", "Completed"),
            ("SELECT path FROM backup_destinations", "D:/Backups"),
            (
                "SELECT account_name FROM store_accounts WHERE provider = 'steam'",
                "Ana",
            ),
            (
                "SELECT version FROM tool_installations WHERE entry_key = 'steam:620'",
                "0.7.9",
            ),
            (
                "SELECT installed_sha256 FROM tool_files WHERE relative_path = 'dxgi.dll'",
                "sha256_mock",
            ),
            (
                "SELECT name FROM tool_releases_cache WHERE tool_id = 'optiscaler'",
                "OptiScaler 0.7.9",
            ),
            (
                "SELECT CAST(pending AS TEXT) FROM favorites WHERE account_id = 'acct'",
                "0",
            ),
        ];
        for (sql, expected) in checks {
            let found: String = conn.query_row(sql, [], |r| r.get(0)).unwrap();
            assert_eq!(found, expected, "{sql}");
        }

        // The port tables start empty after the upgrade...
        for table in [
            "port_catalog_snapshot",
            "port_overlay",
            "port_install_jobs",
            "port_installations",
            "port_preferences",
        ] {
            let rows: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(rows, 0, "{table} starts empty");
        }

        // ...and accept the rows the port store writes.
        conn.execute(
            "INSERT INTO port_catalog_snapshot (id, payload, fetched_at, entry_count)
             VALUES (1, '{\"apps\":[]}', 170, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO port_overlay (id, version, etag, payload)
             VALUES (1, 1, NULL, 'schema_version = 1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO port_install_jobs
                (id, catalog_id, slug, port_name, release_version, state, phase, install_folder,
                 created_paths, registration_id, result_seen, failure_reason, failure_output,
                 plan_json, release_json, created_at, updated_at, finished_at)
             VALUES ('job1', 'qcat_1', 'bloodborne', 'Bloodborne', 'v0.9.38', 'completed',
                     '\"completed\"', 'D:/Ports/Bloodborne', '[\"D:/Ports/Bloodborne\"]', 'reg1',
                     0, NULL, NULL, '{}', '{}', 100, 200, 200)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO port_installations
                (registration_id, catalog_id, slug, port_name, installed_version,
                 install_folder, environment_json, job_id)
             VALUES ('reg1', 'qcat_1', 'bloodborne', 'Bloodborne PC', 'v0.9.38',
                     'D:/Ports/Bloodborne', '{\"GAMENODE_TEST\":\"1\"}', 'job1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO port_preferences (id, view_mode, filters_json, install_parent)
             VALUES (1, 'list', '{\"guided\":true}', 'D:/Ports')",
            [],
        )
        .unwrap();

        let job: (String, String, Option<String>) = conn
            .query_row(
                "SELECT state, created_paths, registration_id
                 FROM port_install_jobs WHERE id = 'job1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            job,
            (
                "completed".into(),
                "[\"D:/Ports/Bloodborne\"]".into(),
                Some("reg1".into())
            )
        );
        let provenance: (String, String) = conn
            .query_row(
                "SELECT port_name, environment_json FROM port_installations
                 WHERE registration_id = 'reg1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            provenance,
            ("Bloodborne PC".into(), "{\"GAMENODE_TEST\":\"1\"}".into())
        );
        let preferences: (String, String) = conn
            .query_row(
                "SELECT view_mode, install_parent FROM port_preferences WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(preferences, ("list".into(), "D:/Ports".into()));

        // Provenance is not tied to a library registration by a foreign key, so removing a
        // registration cannot delete it.
        let foreign_keys: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_foreign_key_list('port_installations')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(foreign_keys, 0);
        conn.execute("DELETE FROM manual_registrations WHERE id = 'reg1'", [])
            .unwrap();
        let provenance_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM port_installations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(provenance_rows, 1);
    }

    #[test]
    fn v6_database_upgrades_through_favorites_to_schema8_without_losing_steam_data() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        {
            let conn = open_with(&path, &MIGRATIONS[..6]).unwrap();
            conn.execute_batch(
                r#"
                INSERT INTO settings (key, value) VALUES ('window.width', '1600');
                INSERT INTO library_entries
                    (key, provider, store_id, name, install_dir, fields_json, installed,
                     first_seen, last_seen, owned, family_shared)
                VALUES ('steam:620', 'steam', '620', 'Portal 2', 'D:/Steam/Portal 2',
                        '{"state_flags":"4"}', 1, 100, 200, 1, 1);
                INSERT INTO provider_scans (provider, last_success_at, last_catalog_at)
                VALUES ('steam', 200, 190);
                INSERT INTO store_accounts (provider, account_id, account_name, last_verified_at)
                VALUES ('steam', '76561198000000000', 'Ana', 210);
                "#,
            )
            .unwrap();
        }

        let store = SqliteSettingsStore::open(&path).unwrap();
        let conn = store.conn();
        assert_eq!(CURRENT_SCHEMA_VERSION, 26);
        assert_eq!(
            versions(&conn).last(),
            Some(&(26, "plugin_bundle_history".to_owned()))
        );
        let steam: (String, String, i64, i64, i64) = conn
            .query_row(
                "SELECT name, install_dir, installed, owned, family_shared
                 FROM library_entries WHERE key = 'steam:620'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(
            steam,
            ("Portal 2".into(), "D:/Steam/Portal 2".into(), 1, 1, 1)
        );
        let account: (String, String, i64) = conn
            .query_row(
                "SELECT account_id, account_name, last_verified_at
                 FROM store_accounts WHERE provider = 'steam'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(account, ("76561198000000000".into(), "Ana".into(), 210));
        assert_eq!(
            conn.query_row(
                "SELECT last_success_at FROM provider_scans WHERE provider='steam'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            200
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM favorites", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0,
            "the intervening v7 migration creates the favorites table"
        );
        for table in [
            "port_catalog_snapshot",
            "port_overlay",
            "port_install_jobs",
            "port_installations",
            "port_preferences",
            "achievement_games",
            "achievements",
        ] {
            assert_eq!(
                conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                0,
                "new table {table} starts empty"
            );
        }
    }

    #[test]
    fn a_populated_schema8_profile_upgrades_to_schema9_preserving_steam_and_ports_rows() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        {
            let store = SqliteSettingsStore::new(open_with(&path, &MIGRATIONS[..8]).unwrap());
            let conn = store.conn();
            conn.execute_batch(
                r#"
                INSERT INTO settings (key, value) VALUES ('window.width', '1440');
                INSERT INTO library_entries
                    (key, provider, store_id, name, install_dir, fields_json, installed,
                     first_seen, last_seen, owned, family_shared)
                VALUES ('steam:570', 'steam', '570', 'Dota 2', 'E:/Steam/Dota 2',
                        '{"state_flags":"5"}', 1, 300, 400, 1, 0);
                INSERT INTO store_accounts (provider, account_id, account_name, last_verified_at)
                VALUES ('steam', '76561198000000001', 'Bia', 410);
                INSERT INTO port_catalog_snapshot (id, payload, fetched_at, entry_count)
                VALUES (1, '{"apps":["qcat_1"]}', 420, 1);
                INSERT INTO port_overlay (id, version, etag, payload)
                VALUES (1, 1, 'etag-v1', 'schema_version = 1');
                INSERT INTO port_install_jobs
                    (id, catalog_id, slug, port_name, release_version, state, phase,
                     install_folder, created_paths, registration_id, result_seen,
                     plan_json, release_json, created_at, updated_at, finished_at)
                VALUES ('job-v8', 'qcat_1', 'elden-ring', 'Elden Ring', 'v1', 'completed',
                        '"completed"', 'E:/Ports/Elden Ring', '["E:/Ports/Elden Ring"]',
                        'port-reg-1', 1, '{}', '{}', 430, 440, 440);
                INSERT INTO port_installations
                    (registration_id, catalog_id, slug, port_name, installed_version,
                     install_folder, environment_json, job_id)
                VALUES ('port-reg-1', 'qcat_1', 'elden-ring', 'Elden Ring PC', 'v1',
                        'E:/Ports/Elden Ring', '{"ROLE":"gameplay"}', 'job-v8');
                INSERT INTO port_preferences (id, view_mode, filters_json, install_parent)
                VALUES (1, 'list', '{"installed":true}', 'E:/Ports');
                "#,
            )
            .unwrap();
            assert_eq!(versions(&conn).last(), Some(&(8, "ports".to_owned())));
        }

        let reopened = SqliteSettingsStore::open(&path).unwrap();
        let conn = reopened.conn();
        assert_eq!(CURRENT_SCHEMA_VERSION, 26);
        assert_eq!(
            versions(&conn).last(),
            Some(&(26, "plugin_bundle_history".to_owned()))
        );
        assert_eq!(
            conn.query_row(
                "SELECT name FROM library_entries WHERE key='steam:570'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "Dota 2"
        );
        assert_eq!(
            conn.query_row(
                "SELECT account_name FROM store_accounts WHERE provider='steam'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "Bia"
        );
        assert_eq!(
            conn.query_row(
                "SELECT payload FROM port_catalog_snapshot WHERE id=1",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "{\"apps\":[\"qcat_1\"]}"
        );
        assert_eq!(
            conn.query_row("SELECT payload FROM port_overlay WHERE id=1", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
            "schema_version = 1"
        );
        assert_eq!(
            conn.query_row(
                "SELECT created_paths FROM port_install_jobs WHERE id='job-v8'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "[\"E:/Ports/Elden Ring\"]"
        );
        assert_eq!(
            conn.query_row("SELECT environment_json FROM port_installations WHERE registration_id='port-reg-1'", [], |r| r.get::<_, String>(0)).unwrap(),
            "{\"ROLE\":\"gameplay\"}"
        );
        let preferences: (String, String) = conn
            .query_row(
                "SELECT view_mode, install_parent FROM port_preferences WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(preferences, ("list".into(), "E:/Ports".into()));
        for table in ["achievement_games", "achievements"] {
            assert_eq!(
                conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                0,
                "the new achievement table {table} starts empty"
            );
        }
    }

    #[test]
    fn a_populated_schema9_profile_upgrades_to_schema10_keeping_every_row() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        {
            let conn = open_with(&path, &MIGRATIONS[..9]).unwrap();
            conn.execute_batch(
                r#"
                INSERT INTO settings (key, value) VALUES ('window.width', '1920');
                INSERT INTO library_entries
                    (key, provider, store_id, name, install_dir, fields_json, installed,
                     first_seen, last_seen, owned, family_shared)
                VALUES ('steam:620', 'steam', '620', 'Portal 2', 'C:/Games/Portal 2',
                        '{"state_flags":"4"}', 1, 100, 150, 1, 0);
                INSERT INTO provider_scans
                    (provider, last_success_at, last_error, last_catalog_at, catalog_error,
                     last_family_at, family_error)
                VALUES ('steam', 150, NULL, 140, NULL, 145, NULL);
                INSERT INTO auto_matches (category, uid, game_ids_json, checked_at)
                VALUES (1, '620', '[72]', 120);
                INSERT INTO chosen_matches (account_id, entry_key, game_id, chosen_at)
                VALUES ('acct', 'steam:620', 72, 130);
                INSERT INTO games
                    (game_id, name, release_year, cover_url, cover_file, cover_failed_at)
                VALUES (72, 'Portal 2', 2011, 'https://images.example/co1.jpg', '72.jpg', NULL);
                INSERT INTO playtime_sessions
                    (id, account_id, entry_key, game_id, source, platform_id, started_at,
                     checkpoint_at, ended_at)
                VALUES ('s1', 'acct', 'steam:620', 72, 'steam', 6, 100, 200, 300);
                INSERT INTO playtime_totals
                    (account_id, entry_key, total_seconds, last_played_at, accepted_game_id,
                     accepted_seconds, rejected_seconds)
                VALUES ('acct', 'steam:620', 3600, 300, 72, 3600, NULL);
                INSERT INTO manual_registrations (id, file_identity, removed, payload)
                VALUES ('reg1', 'exe_hash_1', 0, '{"title":"Game 1"}');
                INSERT INTO discovery_watches (id, payload) VALUES ('w1', '{"path":"D:/Games"}');
                INSERT INTO discovery_candidates (fingerprint, payload)
                VALUES ('fp1', '{"title":"Found"}');
                INSERT INTO local_inferences (account_id, installation_id, payload)
                VALUES ('acct', 'reg1', '{"game_id":72}');
                INSERT INTO save_profiles (id, installation_id, revision, payload)
                VALUES ('p1', 'reg1', 1, '{"label":"Save"}');
                INSERT INTO backup_jobs
                    (id, installation_id, profile_id, revision, play_period, destination,
                     state, payload)
                VALUES ('j1', 'reg1', 'p1', 1, NULL, 'D:/Backups', 'Completed', '{"ok":true}');
                INSERT INTO backup_destinations (path) VALUES ('D:/Backups');
                INSERT INTO store_accounts (provider, account_id, account_name, last_verified_at)
                VALUES ('steam', '76561198000000000', 'Ana', 160);
                INSERT INTO tool_installations
                    (entry_key, tool_id, channel, version, target_exe, proxy_dll, state,
                     installed_at, updated_at)
                VALUES ('steam:620', 'optiscaler', 'stable', '0.7.9', 'bin/game.exe', 'dxgi.dll',
                        'installed', 1000, 1000);
                INSERT INTO tool_files
                    (entry_key, tool_id, relative_path, installed_sha256, backup_path,
                     original_sha256, prior_owner_tool_id, created_dirs_json, ini_patches_json,
                     ini_section, ini_key, ini_value, ini_prior_value)
                VALUES ('steam:620', 'optiscaler', 'dxgi.dll', 'sha256_mock', NULL, NULL, NULL,
                        '[]', '[]', NULL, NULL, NULL, NULL);
                INSERT INTO tool_releases_cache
                    (tool_id, channel, version, tag_name, name, published_at, download_url,
                     asset_name, size_bytes, sha256, cached_path, fetched_at)
                VALUES ('optiscaler', 'stable', '0.7.9', 'v0.7.9', 'OptiScaler 0.7.9',
                        '2026-01-01', 'https://github.com/example/asset.7z', 'asset.7z', 1024,
                        'sha', NULL, 1000);
                INSERT INTO favorites (account_id, game_id, is_favorite, pending, updated_at)
                VALUES ('acct', 72, 1, 0, 170);
                INSERT INTO port_catalog_snapshot (id, payload, fetched_at, entry_count)
                VALUES (1, '{"apps":["qcat_1"]}', 420, 1);
                INSERT INTO port_overlay (id, version, etag, payload)
                VALUES (1, 1, 'etag-v1', 'schema_version = 1');
                INSERT INTO port_install_jobs
                    (id, catalog_id, slug, port_name, release_version, state, phase,
                     install_folder, created_paths, registration_id, result_seen,
                     plan_json, release_json, created_at, updated_at, finished_at)
                VALUES ('job-v9', 'qcat_1', 'elden-ring', 'Elden Ring', 'v1', 'completed',
                        '"completed"', 'E:/Ports/Elden Ring', '["E:/Ports/Elden Ring"]',
                        'port-reg-1', 1, '{}', '{}', 430, 440, 440);
                INSERT INTO port_installations
                    (registration_id, catalog_id, slug, port_name, installed_version,
                     install_folder, environment_json, job_id)
                VALUES ('port-reg-1', 'qcat_1', 'elden-ring', 'Elden Ring PC', 'v1',
                        'E:/Ports/Elden Ring', '{"ROLE":"gameplay"}', 'job-v9');
                INSERT INTO port_preferences (id, view_mode, filters_json, install_parent)
                VALUES (1, 'list', '{"installed":true}', 'E:/Ports');
                INSERT INTO achievement_games
                    (provider, store_account_id, entry_id, state, language, last_success_at,
                     last_attempt_at, failure)
                VALUES ('steam', '76561198000000000', '620', 'ready', 'english', 500, 500, NULL);
                INSERT INTO achievements
                    (provider, store_account_id, entry_id, achievement_id, name, description,
                     hidden, icon_url, icon_gray_url, unlocked, unlocked_at, position)
                VALUES ('steam', '76561198000000000', '620', 'ach1', 'First', 'Do it', 0,
                        'https://images.example/icon.png', 'https://images.example/gray.png',
                        1, 510, 0);
                "#,
            )
            .unwrap();
            assert_eq!(
                versions(&conn).last(),
                Some(&(9, "achievements".to_owned()))
            );
        }

        let store = SqliteSettingsStore::open(&path).unwrap();
        let conn = store.conn();
        assert_eq!(CURRENT_SCHEMA_VERSION, 26);
        assert_eq!(
            versions(&conn).last(),
            Some(&(26, "plugin_bundle_history".to_owned()))
        );

        // Every table that existed at schema 9 keeps its row.
        for table in [
            "achievement_games",
            "achievements",
            "auto_matches",
            "backup_destinations",
            "backup_jobs",
            "chosen_matches",
            "discovery_candidates",
            "discovery_watches",
            "favorites",
            "games",
            "library_entries",
            "local_inferences",
            "manual_registrations",
            "playtime_sessions",
            "playtime_totals",
            "port_catalog_snapshot",
            "port_install_jobs",
            "port_installations",
            "port_overlay",
            "port_preferences",
            "provider_scans",
            "save_profiles",
            "settings",
            "store_accounts",
            "tool_files",
            "tool_installations",
            "tool_releases_cache",
        ] {
            let rows: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            // The schema-9 profile keeps its two window rows; the migration also seeds the
            // eleven application-preference keys.
            let expected = if table == "settings" { 13 } else { 1 };
            assert_eq!(rows, expected, "{table} keeps its row");
        }

        let checks = [
            (
                "SELECT value FROM settings WHERE key = 'window.width'",
                "1920",
            ),
            (
                "SELECT name FROM library_entries WHERE key = 'steam:620'",
                "Portal 2",
            ),
            (
                "SELECT CAST(last_catalog_at AS TEXT) FROM provider_scans WHERE provider = 'steam'",
                "140",
            ),
            (
                "SELECT game_ids_json FROM auto_matches WHERE uid = '620'",
                "[72]",
            ),
            (
                "SELECT CAST(game_id AS TEXT) FROM chosen_matches WHERE account_id = 'acct'",
                "72",
            ),
            ("SELECT cover_file FROM games WHERE game_id = 72", "72.jpg"),
            (
                "SELECT CAST(ended_at AS TEXT) FROM playtime_sessions WHERE id = 's1'",
                "300",
            ),
            (
                "SELECT CAST(total_seconds AS TEXT) FROM playtime_totals",
                "3600",
            ),
            (
                "SELECT payload FROM manual_registrations WHERE id = 'reg1'",
                "{\"title\":\"Game 1\",\"customCover\":null}",
            ),
            (
                "SELECT payload FROM discovery_watches WHERE id = 'w1'",
                "{\"path\":\"D:/Games\"}",
            ),
            (
                "SELECT payload FROM discovery_candidates WHERE fingerprint = 'fp1'",
                "{\"title\":\"Found\"}",
            ),
            (
                "SELECT payload FROM local_inferences WHERE installation_id = 'reg1'",
                "{\"game_id\":72}",
            ),
            (
                "SELECT CAST(revision AS TEXT) FROM save_profiles WHERE id = 'p1'",
                "1",
            ),
            ("SELECT state FROM backup_jobs WHERE id = 'j1'", "Completed"),
            ("SELECT path FROM backup_destinations", "D:/Backups"),
            (
                "SELECT account_name FROM store_accounts WHERE provider = 'steam'",
                "Ana",
            ),
            (
                "SELECT version FROM tool_installations WHERE entry_key = 'steam:620'",
                "0.7.9",
            ),
            (
                "SELECT installed_sha256 FROM tool_files WHERE relative_path = 'dxgi.dll'",
                "sha256_mock",
            ),
            (
                "SELECT name FROM tool_releases_cache WHERE tool_id = 'optiscaler'",
                "OptiScaler 0.7.9",
            ),
            (
                "SELECT CAST(pending AS TEXT) FROM favorites WHERE account_id = 'acct'",
                "0",
            ),
            (
                "SELECT payload FROM port_catalog_snapshot WHERE id = 1",
                "{\"apps\":[\"qcat_1\"]}",
            ),
            (
                "SELECT created_paths FROM port_install_jobs WHERE id = 'job-v9'",
                "[\"E:/Ports/Elden Ring\"]",
            ),
            (
                "SELECT environment_json FROM port_installations WHERE registration_id = 'port-reg-1'",
                "{\"ROLE\":\"gameplay\"}",
            ),
            (
                "SELECT view_mode FROM port_preferences WHERE id = 1",
                "list",
            ),
            (
                "SELECT name FROM achievements WHERE achievement_id = 'ach1'",
                "First",
            ),
        ];
        for (sql, expected) in checks {
            let found: String = conn.query_row(sql, [], |r| r.get(0)).unwrap();
            assert_eq!(found, expected, "{sql}");
        }

        // The organization and account-library tables exist and start empty. They accept the
        // shapes the store writes: per-account view names and a complete-read marker.
        for table in [
            "saved_views",
            "hidden_entries",
            "account_library_entries",
            "account_collections",
            "account_library_reads",
        ] {
            let rows: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(rows, 0, "{table} starts empty");
        }
        conn.execute(
            "INSERT INTO saved_views (account_id, name, payload, position, created_at)
             VALUES ('acct', 'Steam 10 h+', '{}', 0, 100)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO saved_views (account_id, name, payload, position, created_at)
             VALUES ('other', 'Steam 10 h+', '{}', 0, 100)",
            [],
        )
        .unwrap();
        assert!(
            conn.execute(
                "INSERT INTO saved_views (account_id, name, payload, position, created_at)
                 VALUES ('acct', 'Steam 10 h+', '{}', 1, 100)",
                [],
            )
            .is_err(),
            "view names are unique per account"
        );
        conn.execute(
            "INSERT INTO account_library_reads (account_id, read_at) VALUES ('acct', 300)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO account_library_entries
                (account_id, game_id, entry_id, status, collection_ids)
             VALUES ('acct', 72, 'entry-1', 'playing', '[\"coop\"]')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO account_collections
                (account_id, collection_id, name, default_status, position)
             VALUES ('acct', 'coop', 'Coop', NULL, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO hidden_entries (account_id, entry_key, hidden_at)
             VALUES ('acct', 'steam:620', 300)",
            [],
        )
        .unwrap();
    }

    #[test]
    fn a_populated_main_shaped_database_upgrades_to_the_complete_schema_without_losing_data() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        let before = {
            let conn = open_with(&path, &MIGRATIONS[..16]).unwrap();
            assert_eq!(versions(&conn), migration_history(&MIGRATIONS[..16]));
            seed_main_shaped_database(&conn);
            snapshot_table_data(&conn)
        };
        assert_eq!(before.history.len(), 16);
        println!(
            "main-shaped fixture: {} tables, {} rows, fingerprint {}",
            before.tables.len(),
            before.tables.iter().map(|(.., rows, _)| rows).sum::<i64>(),
            snapshot_fingerprint(&before),
        );

        let upgraded = open(&path).unwrap();

        // The runner appends this build's migrations to main's history without rewriting it.
        assert_eq!(
            versions(&upgraded),
            migration_history(MIGRATIONS),
            "the full migration history is recorded in order"
        );
        let history_after: Vec<(u32, String, i64)> = upgraded
            .prepare("SELECT version, name, applied_at FROM schema_migrations ORDER BY version")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            &history_after[..16],
            before.history.as_slice(),
            "main's recorded versions, names and applied times are untouched"
        );

        // Every table that main shipped still exists with the same rows in the same columns.
        let columns_after = table_columns_by_table(&upgraded);
        for (table, columns, rows, digest) in &before.tables {
            let current_columns = columns_after
                .get(table)
                .unwrap_or_else(|| panic!("{table} disappeared during the upgrade"));
            for column in columns {
                assert!(
                    current_columns.contains(column),
                    "{table}.{column} disappeared during the upgrade"
                );
            }
            let (rows_after, digest_after) = table_digest(&upgraded, table, columns);
            assert_eq!(rows_after, *rows, "{table} kept its row count");
            assert_eq!(digest_after, *digest, "{table} kept its row data");
        }

        // Tables added by this build start empty and the additive columns use their defaults.
        for table in table_names(&upgraded) {
            if table == "schema_migrations" || before.tables.iter().any(|(old, ..)| *old == table) {
                continue;
            }
            let (rows, _) = table_digest(&upgraded, &table, &table_columns(&upgraded, &table));
            assert_eq!(rows, 0, "{table} is new in this build and starts empty");
        }
        let (source, target): (String, Option<String>) = upgraded
            .query_row(
                "SELECT source, (SELECT target_json FROM library_entries WHERE key = 'fixture:1')
                 FROM chosen_matches WHERE account_id = 'acct-fixture'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(source, "user", "pre-existing choices stay user-made");
        assert_eq!(target, None, "an entry starts without a detected target");
        println!(
            "upgraded database: {} tables, history {} entries (1-{}), main tables preserved, new tables empty",
            table_names(&upgraded).len(),
            versions(&upgraded).len(),
            versions(&upgraded).last().unwrap().0,
        );

        // The upgraded database has the same complete schema as a database created now.
        let fresh_path = dir.path().join("fresh.sqlite3");
        let fresh = open(&fresh_path).unwrap();
        assert_eq!(schema_rows(&upgraded), schema_rows(&fresh));

        // The verified pre-migration snapshot was kept beside the upgraded database.
        let snapshots = snapshots_for(&dir, &path);
        assert_eq!(snapshots.len(), 1, "one verified snapshot is retained");
        let snapshot =
            Connection::open_with_flags(&snapshots[0], rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        let integrity: String = snapshot
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .unwrap();
        assert_eq!(integrity, "ok");
        assert_eq!(versions(&snapshot), migration_history(&MIGRATIONS[..16]));
        for (table, columns, rows, digest) in &before.tables {
            assert_eq!(
                table_digest(&snapshot, table, columns),
                (*rows, digest.clone()),
                "the snapshot preserves {table}"
            );
        }
    }

    #[test]
    fn a_main_shaped_build_refuses_the_upgraded_database_untouched() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        {
            let conn = open_with(&path, &MIGRATIONS[..16]).unwrap();
            seed_main_shaped_database(&conn);
        }
        drop(open(&path).unwrap());
        let before = std::fs::read(&path).unwrap();

        // Simulates the shipped main build (`origin/main` at `e37ccd1`, schema 16), whose runner
        // refuses any database whose highest recorded version is newer than its own list.
        let error = open_with(&path, &MIGRATIONS[..16]).unwrap_err();
        assert_eq!(
            error,
            StorageError::NewerSchema {
                found: CURRENT_SCHEMA_VERSION,
                supported: 16
            }
        );
        println!(
            "older build refusal: {error}; sha256 before {} after {}",
            format!("{:x}", Sha256::digest(&before)),
            format!("{:x}", Sha256::digest(std::fs::read(&path).unwrap())),
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "an older build leaves the upgraded database byte-for-byte unchanged"
        );
        assert_eq!(
            versions(&Connection::open(&path).unwrap()),
            migration_history(MIGRATIONS),
            "refusal neither truncates nor rewrites the history"
        );
    }

    fn table_columns_by_table(conn: &Connection) -> std::collections::HashMap<String, Vec<String>> {
        table_names(conn)
            .into_iter()
            .map(|table| {
                let columns = table_columns(conn, &table);
                (table, columns)
            })
            .collect()
    }

    fn schema_rows(conn: &Connection) -> Vec<(String, String, String, String)> {
        let mut statement = conn
            .prepare(
                "SELECT type, name, tbl_name, COALESCE(sql, '') FROM sqlite_master
                 WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
            )
            .unwrap();
        statement
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn snapshots_for(dir: &TempDir, database: &Path) -> Vec<std::path::PathBuf> {
        let prefix = format!(
            "{}.pre-migration-",
            database.file_name().unwrap().to_string_lossy()
        );
        std::fs::read_dir(dir.path())
            .unwrap()
            .map(Result::unwrap)
            .map(|entry| entry.path())
            .filter(|candidate| {
                candidate
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(&prefix))
            })
            .collect()
    }

    #[test]
    fn a_populated_schema11_profile_upgrades_to_schema12_with_nullable_rarity() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        {
            let conn = open_with(&path, &MIGRATIONS[..11]).unwrap();
            conn.execute_batch(
                r#"
                INSERT INTO achievement_games
                    (provider, store_account_id, entry_id, state, language, last_success_at,
                     last_attempt_at, failure)
                VALUES ('steam', '76561198000000000', '620', 'ready', 'english', 500, 500, NULL);
                INSERT INTO achievements
                    (provider, store_account_id, entry_id, achievement_id, name, description,
                     hidden, icon_url, icon_gray_url, unlocked, unlocked_at, position)
                VALUES ('steam', '76561198000000000', '620', 'ach1', 'First', 'Do it', 0,
                        'https://images.example/icon.png', 'https://images.example/gray.png',
                        1, 510, 0);
                "#,
            )
            .unwrap();
            assert_eq!(
                versions(&conn).last(),
                Some(&(11, "game_platforms".to_owned()))
            );
        }

        let store = SqliteSettingsStore::open(&path).unwrap();
        let conn = store.conn();
        assert_eq!(CURRENT_SCHEMA_VERSION, 26);
        assert_eq!(
            versions(&conn).last(),
            Some(&(26, "plugin_bundle_history".to_owned()))
        );
        let (name, rarity): (String, Option<i64>) = conn
            .query_row(
                "SELECT name, global_percent FROM achievements WHERE achievement_id = 'ach1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(name, "First");
        assert_eq!(rarity, None, "an upgraded row starts without rarity");
        conn.execute(
            "UPDATE achievements SET global_percent = 420 WHERE achievement_id = 'ach1'",
            [],
        )
        .unwrap();
        let rarity: i64 = conn
            .query_row(
                "SELECT global_percent FROM achievements WHERE achievement_id = 'ach1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(rarity, 420);
    }

    #[test]
    fn a_populated_schema24_profile_upgrades_to_schema25_with_empty_bundle_tables() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        {
            let conn = open_with(&path, &MIGRATIONS[..24]).unwrap();
            conn.execute_batch(
                r#"
                INSERT INTO settings (key, value) VALUES ('window.width', '1280');
                INSERT INTO library_entries
                    (key, provider, store_id, name, install_dir, fields_json, installed,
                     first_seen, last_seen, owned, family_shared)
                VALUES ('steam:620', 'steam', '620', 'Portal 2', 'D:/Steam/Portal 2',
                        '{}', 1, 100, 200, 1, 0);
                INSERT INTO plugin_custom_launch_recipes
                    (owner_key, recipe_id, installation_id, revision, recipe_json)
                VALUES ('owner', 'recipe', 'installation', 1, '{"recipe_id":"recipe"}');
                "#,
            )
            .unwrap();
            assert_eq!(
                versions(&conn).last(),
                Some(&(24, "library_launch_recipe_selection".to_owned()))
            );
        }

        let store = SqliteSettingsStore::open(&path).unwrap();
        let conn = store.conn();
        assert_eq!(CURRENT_SCHEMA_VERSION, 26);
        assert_eq!(
            versions(&conn).last(),
            Some(&(26, "plugin_bundle_history".to_owned()))
        );
        for table in [
            "plugin_bundles",
            "plugin_bundle_components",
            "plugin_bundle_history",
            "plugin_bundle_history_components",
        ] {
            let (rows, strict): (i64, i64) = conn
                .query_row(
                    &format!("SELECT COUNT(*), (SELECT strict FROM pragma_table_list WHERE name = '{table}') FROM {table}"),
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(rows, 0, "{table} starts empty");
            assert_eq!(strict, 1, "{table} is STRICT");
        }
        let foreign_key: String = conn
            .query_row(
                "SELECT \"table\" FROM pragma_foreign_key_list('plugin_bundle_components')
                 WHERE \"from\" = 'bundle_id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(foreign_key, "plugin_bundles");

        // The rows that existed before the upgrade are untouched.
        let width: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'window.width'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(width, "1280");
        let entry: String = conn
            .query_row(
                "SELECT name FROM library_entries WHERE key = 'steam:620'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(entry, "Portal 2");
        let recipe: String = conn
            .query_row(
                "SELECT recipe_json FROM plugin_custom_launch_recipes WHERE recipe_id = 'recipe'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(recipe, "{\"recipe_id\":\"recipe\"}");
    }

    #[test]
    fn database_never_holds_credentials_or_callback_urls() {
        let dir = TempDir::new().unwrap();
        let store = SqliteSettingsStore::open(&db_path(&dir)).unwrap();
        store
            .save(&Settings {
                window: Some(geometry()),
                ..Settings::default()
            })
            .unwrap();
        let conn = store.conn();

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let mut sorted = tables.clone();
        sorted.sort();
        assert_eq!(
            sorted,
            [
                "account_collections",
                "account_library_entries",
                "account_library_reads",
                "achievement_games",
                "achievements",
                "auto_matches",
                "backup_destinations",
                "backup_jobs",
                "backup_version_meta",
                "chosen_matches",
                "discovery_candidates",
                "discovery_watches",
                "favorites",
                "games",
                "hidden_entries",
                "library_artwork_overrides",
                "library_display_overrides",
                "library_entries",
                "library_installations",
                "library_launch_recipe_selection",
                "local_inferences",
                "manual_registrations",
                "playtime_sessions",
                "playtime_totals",
                "plugin_bundle_components",
                "plugin_bundle_history",
                "plugin_bundle_history_components",
                "plugin_bundles",
                "plugin_custom_launch_recipes",
                "plugin_runtime_catalog_state",
                "plugin_runtime_content_bindings",
                "plugin_runtime_resources",
                "port_catalog_snapshot",
                "port_install_jobs",
                "port_installations",
                "port_overlay",
                "port_preferences",
                "provider_scans",
                "save_profiles",
                "saved_views",
                "schema_migrations",
                "settings",
                "sqlite_sequence",
                "store_accounts",
                "sync_summaries",
                "tool_files",
                "tool_installations",
                "tool_releases_cache",
            ]
        );
        let schema: String = conn
            .query_row(
                "SELECT group_concat(sql, ' ') FROM sqlite_master",
                [],
                |r| r.get(0),
            )
            .unwrap();
        for forbidden in [
            "token",
            "cookie",
            "secret",
            "password",
            "credential",
            "callback",
            "session_token",
        ] {
            assert!(
                !schema.to_lowercase().contains(forbidden),
                "schema mentions {forbidden}"
            );
        }
        let keys: Vec<String> = conn
            .prepare("SELECT key FROM settings")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(
            keys.iter().all(|k| SETTING_KEYS.contains(&k.as_str())),
            "{keys:?}"
        );
        let values: Vec<String> = conn
            .prepare("SELECT value FROM settings")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(values.iter().all(|v| !v.contains("://")));
    }
}
