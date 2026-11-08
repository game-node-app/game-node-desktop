//! SQLite persistence: ordered, transactional migrations and the foundation settings repository.

use std::path::Path;
use std::sync::{Arc, Mutex};

use gamenode_core::ports::{SettingsStore, StorageError};
use gamenode_core::{Settings, WindowGeometry};
use rusqlite::{Connection, OptionalExtension, params};

/// One schema change. Versions start at 1 and increase by exactly one.
#[derive(Clone, Copy, Debug)]
pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

/// The latest supported schema version.
pub const CURRENT_SCHEMA_VERSION: u32 = 8;

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

    // Inspect without writing, so a newer or foreign database stays byte-for-byte unchanged.
    let has_history = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations'",
            [],
            |_| Ok(()),
        )
        .optional()
        .map_err(failed)?
        .is_some();
    let applied = if has_history {
        let found: Option<u32> = conn
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .map_err(failed)?;
        let found = found.unwrap_or(0);
        if found > supported {
            return Err(StorageError::NewerSchema { found, supported });
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

    for migration in migrations.iter().filter(|m| m.version > applied) {
        run_migration(conn, migration).map_err(|_| StorageError::MigrationFailed {
            version: migration.version,
        })?;
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

/// Every key the foundation may store; nothing else belongs in `settings`.
pub const SETTING_KEYS: [&str; 14] = [
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
    crate::library_db::KEY_LAYOUT,
    crate::library_db::KEY_EXCLUDED,
    crate::library_db::KEY_ACCOUNT_ID,
    crate::library_db::KEY_ACCOUNT_NAME,
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
        Ok(Settings { window })
    }

    fn save(&self, settings: &Settings) -> Result<(), StorageError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        {
            let mut put = tx
                .prepare("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
                .map_err(failed)?;
            let mut delete = tx
                .prepare("DELETE FROM settings WHERE key = ?1")
                .map_err(failed)?;
            match settings.window {
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
        }
        tx.commit().map_err(failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

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
        assert_eq!(
            versions(&store.conn()),
            vec![
                (1, "initial_settings".to_string()),
                (2, "library".to_string()),
                (3, "playtime".to_string()),
                (4, "drm_free_backups".to_string()),
                (5, "game_tools".to_string()),
                (6, "store_accounts_and_catalog".to_string()),
                (7, "favorites".to_string()),
                (8, "ports".to_string())
            ]
        );
    }

    #[test]
    fn reopening_keeps_settings_and_does_not_reapply_migrations() {
        let dir = TempDir::new().unwrap();
        let saved = Settings {
            window: Some(geometry()),
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
    fn legacy_renderer_row_is_ignored_when_loading_settings() {
        let dir = TempDir::new().unwrap();
        let store = SqliteSettingsStore::open(&db_path(&dir)).unwrap();
        let geometry = geometry();
        store
            .save(&Settings {
                window: Some(geometry),
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
                }
            );
        }
    }

    const V9_OK: Migration = Migration {
        version: 9,
        name: "add_notes",
        sql: "CREATE TABLE notes (id INTEGER PRIMARY KEY) STRICT;",
    };
    const V9_BAD: Migration = Migration {
        version: 9,
        name: "broken",
        sql: "CREATE TABLE half_done (id INTEGER PRIMARY KEY); INSERT INTO missing_table VALUES (1);",
    };

    #[test]
    fn older_database_is_upgraded_in_order_preserving_settings() {
        let dir = TempDir::new().unwrap();
        let saved = Settings {
            window: Some(geometry()),
        };
        SqliteSettingsStore::open(&db_path(&dir))
            .unwrap()
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
            V9_OK,
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
                (9, "add_notes".into())
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
        let saved = Settings { window: None };
        SqliteSettingsStore::open(&db_path(&dir))
            .unwrap()
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
                V9_BAD,
            ],
        )
        .unwrap_err();
        assert_eq!(err, StorageError::MigrationFailed { version: 9 });

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
    fn newer_schema_is_rejected_and_the_file_is_unchanged() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        drop(
            open_with(
                &path,
                &[
                    MIGRATIONS[0],
                    MIGRATIONS[1],
                    MIGRATIONS[2],
                    MIGRATIONS[3],
                    MIGRATIONS[4],
                    MIGRATIONS[5],
                    MIGRATIONS[6],
                    MIGRATIONS[7],
                    V9_OK,
                ],
            )
            .unwrap(),
        );
        let before = std::fs::read(&path).unwrap();

        let err = open(&path).unwrap_err(); // this build only knows version 8
        assert_eq!(
            err,
            StorageError::NewerSchema {
                found: 9,
                supported: 8
            }
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "a newer database is not modified"
        );
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
                found: 8,
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
        };
        drop(Connection::open(db_path(&dir)).unwrap());
        SqliteSettingsStore::new(open_with(&db_path(&dir), step_one()).unwrap())
            .save(&saved)
            .unwrap();

        let store = SqliteSettingsStore::open(&db_path(&dir)).unwrap();
        assert_eq!(store.load().unwrap(), saved);
        assert_eq!(
            versions(&store.conn()),
            vec![
                (1, "initial_settings".to_string()),
                (2, "library".to_string()),
                (3, "playtime".to_string()),
                (4, "drm_free_backups".to_string()),
                (5, "game_tools".to_string()),
                (6, "store_accounts_and_catalog".to_string()),
                (7, "favorites".to_string()),
                (8, "ports".to_string()),
            ]
        );
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
                found: 8,
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
            version: 9,
            ..V9_OK
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
            ]
        );

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
        assert_eq!(title, "{\"title\":\"Game 1\"}");
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
            ]
        );

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
            ]
        );

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
                "{\"title\":\"Game 1\"}",
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
    fn database_never_holds_credentials_or_callback_urls() {
        let dir = TempDir::new().unwrap();
        let store = SqliteSettingsStore::open(&db_path(&dir)).unwrap();
        store
            .save(&Settings {
                window: Some(geometry()),
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
                "schema_migrations",
                "settings",
                "store_accounts",
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
