//! SQLite persistence: ordered, transactional migrations and the foundation settings repository.

use std::path::Path;
use std::sync::Mutex;

use gamenode_core::ports::{SettingsStore, StorageError};
use gamenode_core::{RendererPreference, Settings, WindowGeometry};
use rusqlite::{Connection, OptionalExtension, params};

/// One schema change. Versions start at 1 and increase by exactly one.
#[derive(Clone, Copy, Debug)]
pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

/// Every migration this build knows. Append new ones; never edit or reorder released ones.
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "initial_settings",
    sql: include_str!("../migrations/0001_initial_settings.sql"),
}];

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
pub const SETTING_KEYS: [&str; 6] = [
    KEY_RENDERER,
    KEY_X,
    KEY_Y,
    KEY_WIDTH,
    KEY_HEIGHT,
    KEY_MAXIMIZED,
];

/// [`SettingsStore`] backed by the application database.
pub struct SqliteSettingsStore {
    conn: Mutex<Connection>,
}

impl SqliteSettingsStore {
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: Mutex::new(conn),
        }
    }

    /// Opens and migrates the database at `path`.
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        open(path).map(Self::new)
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
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
        let renderer = get(KEY_RENDERER)?
            .and_then(|v| RendererPreference::parse(&v))
            .unwrap_or_default();
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
        Ok(Settings { renderer, window })
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
            put.execute(params![KEY_RENDERER, settings.renderer.as_str()])
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
            vec![(1, "initial_settings".to_string())]
        );
    }

    #[test]
    fn reopening_keeps_settings_and_does_not_reapply_migrations() {
        let dir = TempDir::new().unwrap();
        let saved = Settings {
            renderer: RendererPreference::Software,
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
        assert_eq!(versions(&store.conn()).len(), 1);
    }

    #[test]
    fn window_without_position_and_cleared_geometry_round_trip() {
        let dir = TempDir::new().unwrap();
        let store = SqliteSettingsStore::open(&db_path(&dir)).unwrap();
        let mut s = Settings {
            renderer: RendererPreference::Gpu,
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

    const V2_OK: Migration = Migration {
        version: 2,
        name: "add_notes",
        sql: "CREATE TABLE notes (id INTEGER PRIMARY KEY) STRICT;",
    };
    const V2_BAD: Migration = Migration {
        version: 2,
        name: "broken",
        sql: "CREATE TABLE half_done (id INTEGER PRIMARY KEY); INSERT INTO missing_table VALUES (1);",
    };

    #[test]
    fn older_database_is_upgraded_in_order_preserving_settings() {
        let dir = TempDir::new().unwrap();
        let saved = Settings {
            renderer: RendererPreference::Software,
            window: Some(geometry()),
        };
        SqliteSettingsStore::open(&db_path(&dir))
            .unwrap()
            .save(&saved)
            .unwrap();

        let both = [MIGRATIONS[0], V2_OK];
        let conn = open_with(&db_path(&dir), &both).unwrap();
        assert_eq!(
            versions(&conn),
            vec![(1, "initial_settings".into()), (2, "add_notes".into())]
        );
        conn.execute("INSERT INTO notes DEFAULT VALUES", [])
            .unwrap();
        drop(conn);
        assert_eq!(
            SqliteSettingsStore::open_with_for_test(&db_path(&dir), &both)
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
            renderer: RendererPreference::Software,
            window: None,
        };
        SqliteSettingsStore::open(&db_path(&dir))
            .unwrap()
            .save(&saved)
            .unwrap();

        let err = open_with(&db_path(&dir), &[MIGRATIONS[0], V2_BAD]).unwrap_err();
        assert_eq!(err, StorageError::MigrationFailed { version: 2 });

        let conn = Connection::open(db_path(&dir)).unwrap();
        assert_eq!(versions(&conn), vec![(1, "initial_settings".to_string())]);
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
        drop(open_with(&path, &[MIGRATIONS[0], V2_OK]).unwrap());
        let before = std::fs::read(&path).unwrap();

        let err = open(&path).unwrap_err(); // this build only knows version 1
        assert_eq!(
            err,
            StorageError::NewerSchema {
                found: 2,
                supported: 1
            }
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "a newer database is not modified"
        );
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
            version: 3,
            ..V2_OK
        };
        assert!(open_with(&db_path(&dir), &[MIGRATIONS[0], skipping]).is_err());
    }

    #[test]
    fn database_never_holds_credentials_or_callback_urls() {
        let dir = TempDir::new().unwrap();
        let store = SqliteSettingsStore::open(&db_path(&dir)).unwrap();
        store
            .save(&Settings {
                renderer: RendererPreference::Software,
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
        assert_eq!(tables, ["schema_migrations", "settings"]);
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
            "session",
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
