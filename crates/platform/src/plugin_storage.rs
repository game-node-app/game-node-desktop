//! Durable private key/value storage of third-party plugins.
//!
//! Every plugin has one store per GameNode account profile, in its own SQLite file below
//! `plugin-data/<publisher fingerprint>/<plugin id>/<profile id>/`. The host decides the
//! directory from the authenticated identity ([`namespace_dir`]); a plugin never names a path or
//! another namespace, and nothing here returns one. A store is opened on a blocking worker, and
//! each [`DurableStore::put`] is one transaction, so a write that would pass the quota changes
//! nothing and a crash leaves either the old value or the new one.
//!
//! The quota counts the logical bytes (key, value and a fixed per-entry overhead), not the
//! file size. Revisions are handed in by the caller, who keeps them growing across deletions and
//! restarts through [`DurableStore::high_water`].

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};

/// The longest key, in bytes.
pub const MAX_KEY_BYTES: usize = 256;
/// The largest value, in bytes.
pub const MAX_VALUE_BYTES: usize = 1024 * 1024;
/// The logical bytes a plugin may keep durably per profile.
pub const QUOTA_BYTES: u64 = 16 * 1024 * 1024;
/// Charged per entry on top of its key and value, so many tiny entries cannot dodge the quota.
pub const ENTRY_OVERHEAD_BYTES: u64 = 64;
/// The longest an entry may live before it expires (30 days).
pub const MAX_TTL_SECONDS: u64 = 30 * 24 * 60 * 60;

const SCHEMA_VERSION: i64 = 5;
const STORE_FILE: &str = "storage.sqlite3";
/// Longest directory name derived from an identity before it is replaced by a digest.
const MAX_COMPONENT_BYTES: usize = 120;

/// Why the store could not be used. Messages never contain a path.
#[derive(Debug, thiserror::Error)]
pub enum PluginStorageError {
    #[error("the plugin storage could not be opened")]
    Open(#[source] io::Error),
    #[error("the plugin storage failed")]
    Database(#[source] rusqlite::Error),
    #[error("the plugin storage was written by a newer version of GameNode")]
    NewerVersion,
}

impl From<rusqlite::Error> for PluginStorageError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

/// What the quota charges for one entry.
pub fn charge(key: &str, value_len: usize) -> u64 {
    key.len() as u64 + value_len as u64 + ENTRY_OVERHEAD_BYTES
}

/// A directory name derived from one identity part: readable characters stay, anything else
/// becomes `%XX`, and a name that would be too long or only dots is replaced by a digest.
fn component(part: &str) -> String {
    let mut name = String::with_capacity(part.len());
    for byte in part.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_')
            || (byte == b'.' && !name.is_empty())
        {
            name.push(byte as char);
        } else {
            name.push_str(&format!("%{byte:02X}"));
        }
    }
    if name.is_empty() || name.len() > MAX_COMPONENT_BYTES || name.ends_with('.') {
        let digest = Sha256::digest(part.as_bytes());
        let hex: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
        return format!("h-{hex}");
    }
    name
}

/// The directory of one plugin's store in one profile below `root` (`plugin-data`). Pure: it
/// creates nothing.
pub fn namespace_dir(root: &Path, publisher: &str, plugin: &str, profile: &str) -> PathBuf {
    root.join(component(publisher))
        .join(component(plugin))
        .join(component(profile))
}

/// A stored value as read back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stored {
    pub value: Vec<u8>,
    pub revision: u64,
    /// Unix seconds, `None` when the entry does not expire.
    pub expires_at: Option<i64>,
}

/// The result of a write that did not fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PutResult {
    /// Stored. `usage` is the logical bytes the namespace now holds.
    Stored { usage: u64 },
    /// Nothing changed: the namespace would exceed the quota. `usage` is what it holds.
    OverQuota { usage: u64 },
}

/// One plugin's durable store in one profile.
pub struct DurableStore {
    conn: Connection,
}

impl DurableStore {
    /// Opens the store in `dir`, or `None` when this namespace never stored anything durably.
    /// Reading never creates files.
    pub fn open_existing(dir: &Path) -> Result<Option<Self>, PluginStorageError> {
        if !dir.join(STORE_FILE).is_file() {
            return Ok(None);
        }
        Self::open(dir).map(Some)
    }

    /// Opens the store in `dir`, creating the directories and the file.
    pub fn open(dir: &Path) -> Result<Self, PluginStorageError> {
        fs::create_dir_all(dir).map_err(PluginStorageError::Open)?;
        let conn = Connection::open(dir.join(STORE_FILE))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(PluginStorageError::NewerVersion);
        }
        if version < 1 {
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS meta (
                     id INTEGER PRIMARY KEY CHECK (id = 1),
                     revision INTEGER NOT NULL CHECK (revision >= 0)
                 ) STRICT;
                 INSERT OR IGNORE INTO meta (id, revision) VALUES (1, 0);
                 CREATE TABLE IF NOT EXISTS entries (
                     key TEXT PRIMARY KEY,
                     value BLOB NOT NULL,
                     revision INTEGER NOT NULL CHECK (revision >= 1),
                     expires_at INTEGER
                 ) STRICT;
                 PRAGMA user_version = 1;
                 COMMIT;",
            )?;
        }
        if version < 2 {
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS file_roots (
                     id TEXT NOT NULL UNIQUE,
                     installation_id TEXT NOT NULL,
                     game_id TEXT NOT NULL,
                     kind TEXT NOT NULL,
                     root TEXT NOT NULL,
                     root_volume INTEGER NOT NULL,
                     root_node INTEGER NOT NULL,
                     operations INTEGER NOT NULL CHECK (operations BETWEEN 1 AND 7),
                     affected TEXT NOT NULL,
                     approved_at INTEGER NOT NULL,
                     revoked_at INTEGER,
                     PRIMARY KEY (installation_id, kind)
                 ) STRICT;
                 PRAGMA user_version = 2;
                 COMMIT;",
            )?;
        }
        if version < 3 {
            // How the approved folder was chosen: `user` for a folder the user picked or the
            // host suggested, `declared` for one the plugin suggested and the user took.
            conn.execute_batch(
                "BEGIN;
                 ALTER TABLE file_roots ADD COLUMN origin TEXT NOT NULL DEFAULT 'user';
                 PRAGMA user_version = 3;
                 COMMIT;",
            )?;
        }
        if version < 4 {
            // Confirmations the user asked to remember (launch or install of one game by this
            // plugin). The row is the approval: revoking deletes it.
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS intent_approvals (
                     id TEXT NOT NULL UNIQUE,
                     game_id TEXT NOT NULL,
                     installation_id TEXT NOT NULL,
                     operation TEXT NOT NULL CHECK (operation IN ('launch', 'install')),
                     target_revision INTEGER NOT NULL CHECK (target_revision >= 0),
                     game_name TEXT NOT NULL,
                     approved_at INTEGER NOT NULL,
                     PRIMARY KEY (installation_id, operation)
                 ) STRICT;
                 PRAGMA user_version = 4;
                 COMMIT;",
            )?;
        }
        if version < 5 {
            // Save profiles this plugin contributed: where a game's saves live for one runtime.
            // They reference folder approvals of the same namespace by id, never a path.
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS save_profiles (
                     id TEXT PRIMARY KEY,
                     installation_id TEXT NOT NULL,
                     game_id TEXT NOT NULL,
                     name TEXT NOT NULL,
                     name_key TEXT NOT NULL,
                     runtime_id TEXT NOT NULL,
                     format_id TEXT NOT NULL,
                     format_version INTEGER NOT NULL CHECK (format_version >= 0),
                     metadata TEXT NOT NULL,
                     directories TEXT NOT NULL,
                     revision INTEGER NOT NULL CHECK (revision >= 1),
                     created_at INTEGER NOT NULL,
                     UNIQUE (installation_id, name_key)
                 ) STRICT;
                 PRAGMA user_version = 5;
                 COMMIT;",
            )?;
        }
        Ok(Self { conn })
    }

    /// The highest revision this store ever recorded, deletions included.
    pub fn high_water(&self) -> Result<u64, PluginStorageError> {
        let revision: i64 =
            self.conn
                .query_row("SELECT revision FROM meta WHERE id = 1", [], |row| {
                    row.get(0)
                })?;
        Ok(revision as u64)
    }

    /// The entry under `key`, unless it expired at `now` (Unix seconds).
    pub fn get(&self, key: &str, now: i64) -> Result<Option<Stored>, PluginStorageError> {
        let found = self
            .conn
            .query_row(
                "SELECT value, revision, expires_at FROM entries WHERE key = ?1",
                [key],
                |row| {
                    Ok(Stored {
                        value: row.get(0)?,
                        revision: row.get::<_, i64>(1)? as u64,
                        expires_at: row.get(2)?,
                    })
                },
            )
            .optional()?;
        Ok(found.filter(|stored| stored.expires_at.is_none_or(|at| at > now)))
    }

    /// The logical bytes held at `now`, expired entries not counted.
    pub fn usage(&self, now: i64) -> Result<u64, PluginStorageError> {
        usage_of(&self.conn, now, None)
    }

    /// Stores `value` under `key` with `revision`, replacing any entry, unless the namespace
    /// would hold more than `quota` bytes afterwards. Expired entries are dropped in the same
    /// transaction. The caller has already checked the expected revision.
    pub fn put(
        &mut self,
        key: &str,
        value: &[u8],
        revision: u64,
        expires_at: Option<i64>,
        now: i64,
        quota: u64,
    ) -> Result<PutResult, PluginStorageError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM entries WHERE expires_at IS NOT NULL AND expires_at <= ?1",
            [now],
        )?;
        let others = usage_of(&tx, now, Some(key))?;
        let current = usage_of(&tx, now, None)?;
        if others.saturating_add(charge(key, value.len())) > quota {
            return Ok(PutResult::OverQuota { usage: current });
        }
        tx.execute(
            "INSERT INTO entries (key, value, revision, expires_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(key) DO UPDATE SET
                 value = excluded.value,
                 revision = excluded.revision,
                 expires_at = excluded.expires_at",
            params![key, value, revision as i64, expires_at],
        )?;
        tx.execute(
            "UPDATE meta SET revision = MAX(revision, ?1) WHERE id = 1",
            [revision as i64],
        )?;
        tx.commit()?;
        Ok(PutResult::Stored {
            usage: others + charge(key, value.len()),
        })
    }

    /// Deletes `key`. Returns the revision it had, or `None` when there was no such entry.
    pub fn remove(&mut self, key: &str) -> Result<Option<u64>, PluginStorageError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let revision: Option<i64> = tx
            .query_row(
                "SELECT revision FROM entries WHERE key = ?1",
                [key],
                |row| row.get(0),
            )
            .optional()?;
        tx.execute("DELETE FROM entries WHERE key = ?1", [key])?;
        tx.commit()?;
        Ok(revision.map(|revision| revision as u64))
    }
}

/// The file operations a root approval allows.
pub mod operation {
    pub const READ: u8 = 1;
    pub const WRITE: u8 = 2;
    pub const DELETE: u8 = 4;
}

/// A folder the user approved for one plugin and one installation of a game.
///
/// It is kept per plugin and account profile, apart from the plugin's key/value data and not
/// charged to its quota. The plugin never sees `root`; it only receives handles issued for the
/// approval while it is current.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileApproval {
    /// Opaque random id; a new approval of the same installation and kind gets a new one.
    pub id: String,
    pub installation_id: String,
    /// The GameNode game the installation was matched to when approved (empty if none).
    pub game_id: String,
    /// What the folder is: `installation`, `save` or `prefix`.
    pub kind: String,
    /// The canonical location, valid Unicode.
    pub root: String,
    pub root_volume: u64,
    pub root_node: u64,
    /// A combination of [`operation`] bits.
    pub operations: u8,
    /// The library entries that overlapped the folder when the user approved it, sorted. The
    /// approval holds only while this is still the complete set.
    pub affected: Vec<String>,
    /// Unix seconds.
    pub approved_at: i64,
    /// Unix seconds; a revoked approval opens nothing.
    pub revoked_at: Option<i64>,
    /// How the folder was chosen: [`ORIGIN_USER`] or [`ORIGIN_DECLARED`].
    pub origin: String,
}

/// The user picked the folder, or took the one the host suggested.
pub const ORIGIN_USER: &str = "user";
/// The user took a folder the plugin had suggested (declared, not verified by the host).
pub const ORIGIN_DECLARED: &str = "declared";

fn approval_from(row: &rusqlite::Row<'_>) -> rusqlite::Result<FileApproval> {
    let affected: String = row.get(8)?;
    Ok(FileApproval {
        id: row.get(0)?,
        installation_id: row.get(1)?,
        game_id: row.get(2)?,
        kind: row.get(3)?,
        root: row.get(4)?,
        root_volume: row.get::<_, i64>(5)? as u64,
        root_node: row.get::<_, i64>(6)? as u64,
        operations: row.get::<_, i64>(7)? as u8,
        affected: serde_json::from_str(&affected).unwrap_or_default(),
        approved_at: row.get(9)?,
        revoked_at: row.get(10)?,
        origin: row.get(11)?,
    })
}

const APPROVAL_COLUMNS: &str = "id, installation_id, game_id, kind, root, root_volume, root_node, \
     operations, affected, approved_at, revoked_at, origin";

impl DurableStore {
    /// Records `approval`, replacing the earlier approval of the same installation and kind.
    pub fn save_approval(&mut self, approval: &FileApproval) -> Result<(), PluginStorageError> {
        let affected = serde_json::to_string(&approval.affected).unwrap_or_else(|_| "[]".into());
        self.conn.execute(
            "INSERT INTO file_roots (id, installation_id, game_id, kind, root, root_volume,
                 root_node, operations, affected, approved_at, revoked_at, origin)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(installation_id, kind) DO UPDATE SET
                 id = excluded.id, game_id = excluded.game_id, root = excluded.root,
                 root_volume = excluded.root_volume, root_node = excluded.root_node,
                 operations = excluded.operations, affected = excluded.affected,
                 approved_at = excluded.approved_at, revoked_at = excluded.revoked_at,
                 origin = excluded.origin",
            params![
                approval.id,
                approval.installation_id,
                approval.game_id,
                approval.kind,
                approval.root,
                approval.root_volume as i64,
                approval.root_node as i64,
                i64::from(approval.operations),
                affected,
                approval.approved_at,
                approval.revoked_at,
                approval.origin,
            ],
        )?;
        Ok(())
    }

    /// The approval of `installation_id` for `kind`, revoked or not.
    pub fn approval_for(
        &self,
        installation_id: &str,
        kind: &str,
    ) -> Result<Option<FileApproval>, PluginStorageError> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {APPROVAL_COLUMNS} FROM file_roots
                     WHERE installation_id = ?1 AND kind = ?2"
                ),
                params![installation_id, kind],
                approval_from,
            )
            .optional()?)
    }

    /// The approval with this id, revoked or not.
    pub fn approval(&self, id: &str) -> Result<Option<FileApproval>, PluginStorageError> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {APPROVAL_COLUMNS} FROM file_roots WHERE id = ?1"),
                [id],
                approval_from,
            )
            .optional()?)
    }

    /// Every approval, in a stable order.
    pub fn approvals(&self) -> Result<Vec<FileApproval>, PluginStorageError> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {APPROVAL_COLUMNS} FROM file_roots ORDER BY installation_id, kind"
        ))?;
        let rows = statement.query_map([], approval_from)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Revokes an approval; its handles stop working at once. Returns whether it was active.
    pub fn revoke_approval(&mut self, id: &str, now: i64) -> Result<bool, PluginStorageError> {
        let changed = self.conn.execute(
            "UPDATE file_roots SET revoked_at = ?2 WHERE id = ?1 AND revoked_at IS NULL",
            params![id, now],
        )?;
        Ok(changed > 0)
    }
}

/// A confirmation the user asked the host to remember: this plugin may launch (or install) this
/// game without asking again, while the game's detected target is the one approved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntentApproval {
    /// Opaque random id the settings list uses to revoke it.
    pub id: String,
    pub game_id: String,
    pub installation_id: String,
    /// `launch` or `install`.
    pub operation: String,
    /// The detected-target revision approved (0 for an install, which has none).
    pub target_revision: u64,
    /// The game's name when approved, for the list; plain text.
    pub game_name: String,
    pub approved_at: i64,
}

const INTENT_COLUMNS: &str =
    "id, game_id, installation_id, operation, target_revision, game_name, approved_at";

fn intent_from(row: &rusqlite::Row<'_>) -> rusqlite::Result<IntentApproval> {
    Ok(IntentApproval {
        id: row.get(0)?,
        game_id: row.get(1)?,
        installation_id: row.get(2)?,
        operation: row.get(3)?,
        target_revision: row.get::<_, i64>(4)? as u64,
        game_name: row.get(5)?,
        approved_at: row.get(6)?,
    })
}

impl DurableStore {
    /// Remembers `approval`, replacing the earlier one for the same installation and operation.
    pub fn save_intent_approval(
        &mut self,
        approval: &IntentApproval,
    ) -> Result<(), PluginStorageError> {
        self.conn.execute(
            "INSERT INTO intent_approvals (id, game_id, installation_id, operation,
                 target_revision, game_name, approved_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(installation_id, operation) DO UPDATE SET
                 id = excluded.id, game_id = excluded.game_id,
                 target_revision = excluded.target_revision, game_name = excluded.game_name,
                 approved_at = excluded.approved_at",
            params![
                approval.id,
                approval.game_id,
                approval.installation_id,
                approval.operation,
                approval.target_revision as i64,
                approval.game_name,
                approval.approved_at,
            ],
        )?;
        Ok(())
    }

    /// The remembered approval of `installation_id` for `operation`.
    pub fn intent_approval(
        &self,
        installation_id: &str,
        operation: &str,
    ) -> Result<Option<IntentApproval>, PluginStorageError> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {INTENT_COLUMNS} FROM intent_approvals
                     WHERE installation_id = ?1 AND operation = ?2"
                ),
                params![installation_id, operation],
                intent_from,
            )
            .optional()?)
    }

    /// Every remembered approval, in a stable order.
    pub fn intent_approvals(&self) -> Result<Vec<IntentApproval>, PluginStorageError> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {INTENT_COLUMNS} FROM intent_approvals ORDER BY installation_id, operation"
        ))?;
        let rows = statement.query_map([], intent_from)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Forgets an approval; the next request asks the user again. Returns whether it existed.
    pub fn revoke_intent_approval(&mut self, id: &str) -> Result<bool, PluginStorageError> {
        let changed = self
            .conn
            .execute("DELETE FROM intent_approvals WHERE id = ?1", [id])?;
        Ok(changed > 0)
    }
}

/// A save profile a plugin contributed, as stored. `directories` and `metadata` are JSON the host
/// wrote and validated; the store only keeps them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveProfileRow {
    pub id: String,
    pub installation_id: String,
    /// The GameNode game the installation was matched to when created (empty if none). A later
    /// rematch does not change it.
    pub game_id: String,
    pub name: String,
    pub runtime_id: String,
    pub format_id: String,
    pub format_version: u32,
    pub metadata: String,
    pub directories: String,
    pub revision: u64,
    pub created_at: i64,
}

/// How writing a profile ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileSaved {
    Saved,
    /// The installation already has a profile with this name (compared without case).
    NameTaken,
}

const PROFILE_COLUMNS: &str = "id, installation_id, game_id, name, runtime_id, format_id, \
     format_version, metadata, directories, revision, created_at";

fn profile_from(row: &rusqlite::Row<'_>) -> rusqlite::Result<SaveProfileRow> {
    Ok(SaveProfileRow {
        id: row.get(0)?,
        installation_id: row.get(1)?,
        game_id: row.get(2)?,
        name: row.get(3)?,
        runtime_id: row.get(4)?,
        format_id: row.get(5)?,
        format_version: row.get::<_, i64>(6)? as u32,
        metadata: row.get(7)?,
        directories: row.get(8)?,
        revision: row.get::<_, i64>(9)? as u64,
        created_at: row.get(10)?,
    })
}

impl DurableStore {
    /// Inserts or replaces the profile with `row.id`, and raises the store's revision high-water
    /// mark to `row.revision` so no revision is ever handed out twice.
    pub fn save_profile(
        &mut self,
        row: &SaveProfileRow,
    ) -> Result<ProfileSaved, PluginStorageError> {
        let tx = self.conn.transaction()?;
        let name_key = row.name.to_lowercase();
        let written = tx.execute(
            "INSERT INTO save_profiles (id, installation_id, game_id, name, name_key, runtime_id,
                 format_id, format_version, metadata, directories, revision, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(id) DO UPDATE SET
                 name = excluded.name, name_key = excluded.name_key,
                 runtime_id = excluded.runtime_id, format_id = excluded.format_id,
                 format_version = excluded.format_version, metadata = excluded.metadata,
                 directories = excluded.directories, revision = excluded.revision",
            params![
                row.id,
                row.installation_id,
                row.game_id,
                row.name,
                name_key,
                row.runtime_id,
                row.format_id,
                i64::from(row.format_version),
                row.metadata,
                row.directories,
                row.revision as i64,
                row.created_at,
            ],
        );
        match written {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(error, _))
                if error.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                return Ok(ProfileSaved::NameTaken);
            }
            Err(other) => return Err(other.into()),
        }
        tx.execute(
            "UPDATE meta SET revision = MAX(revision, ?1) WHERE id = 1",
            [row.revision as i64],
        )?;
        tx.commit()?;
        Ok(ProfileSaved::Saved)
    }

    /// The profile with this id.
    pub fn save_profile_row(&self, id: &str) -> Result<Option<SaveProfileRow>, PluginStorageError> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {PROFILE_COLUMNS} FROM save_profiles WHERE id = ?1"),
                [id],
                profile_from,
            )
            .optional()?)
    }

    /// Every profile, ordered by installation and id.
    pub fn save_profile_rows(&self) -> Result<Vec<SaveProfileRow>, PluginStorageError> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {PROFILE_COLUMNS} FROM save_profiles ORDER BY installation_id, id"
        ))?;
        let rows = statement.query_map([], profile_from)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Removes the profile, raising the revision high-water mark to `revision` (the removal's own
    /// revision). Returns whether it existed.
    pub fn delete_save_profile(
        &mut self,
        id: &str,
        revision: u64,
    ) -> Result<bool, PluginStorageError> {
        let tx = self.conn.transaction()?;
        let removed = tx.execute("DELETE FROM save_profiles WHERE id = ?1", [id])?;
        tx.execute(
            "UPDATE meta SET revision = MAX(revision, ?1) WHERE id = 1",
            [revision as i64],
        )?;
        tx.commit()?;
        Ok(removed > 0)
    }
}

/// The charged bytes of the live entries, optionally leaving out one key.
fn usage_of(conn: &Connection, now: i64, except: Option<&str>) -> Result<u64, PluginStorageError> {
    let total: i64 = conn.query_row(
        "SELECT COALESCE(SUM(length(CAST(key AS BLOB)) + length(value) + ?1), 0)
         FROM entries
         WHERE (expires_at IS NULL OR expires_at > ?2) AND (?3 IS NULL OR key != ?3)",
        params![ENTRY_OVERHEAD_BYTES as i64, now, except],
        |row| row.get(0),
    )?;
    Ok(total as u64)
}

#[cfg(test)]
mod tests;
