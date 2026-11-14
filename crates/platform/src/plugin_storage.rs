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

const SCHEMA_VERSION: i64 = 1;
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
        if version < SCHEMA_VERSION {
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
