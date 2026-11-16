//! Owner-scoped durable plugin records, separate from the plugin's private KV store.
//!
//! This layer accepts a host-resolved namespace and never accepts a path, plugin id from an RPC,
//! or a cross-owner selector. Callers must open and use it on a blocking worker. The application
//! host remains responsible for authenticating the namespace, checking `storage.persist` and
//! retention consent, and deciding whether archived records may be explicitly reused.

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};

const FILE_NAME: &str = "plugin-persistence.sqlite3";
const SCHEMA_VERSION: i64 = 1;
pub const MAX_CATEGORY_BYTES: usize = 128;
pub const MAX_LOGICAL_KEY_BYTES: usize = 256;
pub const MAX_RECORD_BYTES: usize = 1024 * 1024;
pub const MAX_PAGE_SIZE: u32 = 200;
pub const MAX_OWNER_LOGICAL_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_CATEGORY_LOGICAL_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_RETENTION_AGE_SECONDS: u64 = 365 * 24 * 60 * 60;
const RECORD_OVERHEAD_BYTES: u64 = 128;

/// Namespace resolved by the host from the authenticated plugin and account profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnerNamespace {
    pub publisher_id: String,
    pub plugin_id: String,
    pub profile_id: String,
}

impl OwnerNamespace {
    fn validate(&self) -> Result<(), PersistenceError> {
        for value in [&self.publisher_id, &self.plugin_id, &self.profile_id] {
            if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
                return Err(PersistenceError::InvalidNamespace);
            }
        }
        Ok(())
    }
}

/// Provenance copied from the authenticated producer package by the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordProvenance {
    pub source_plugin_id: String,
    pub source_publisher_id: String,
    pub source_version: String,
    pub package_id: String,
}

impl RecordProvenance {
    fn validate(&self) -> Result<(), PersistenceError> {
        for value in [
            &self.source_plugin_id,
            &self.source_publisher_id,
            &self.source_version,
            &self.package_id,
        ] {
            if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
                return Err(PersistenceError::InvalidProvenance);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistedRecord {
    pub category: String,
    pub logical_key: String,
    pub value: Vec<u8>,
    pub revision: u64,
    pub provenance: RecordProvenance,
    pub created_at: i64,
    pub updated_at: i64,
    pub expires_at: Option<i64>,
    pub user_protected: bool,
    /// Archived records are invisible to ordinary plugin reads and survive uninstall retention.
    pub archived: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PutOutcome {
    Stored {
        revision: u64,
        logical_bytes: u64,
    },
    Conflict {
        current_revision: u64,
    },
    OverQuota {
        category_bytes: u64,
        owner_bytes: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CategoryUsage {
    pub category: String,
    pub logical_bytes: u64,
    pub records: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Usage {
    pub categories: Vec<CategoryUsage>,
    pub logical_bytes: u64,
    /// SQLite page allocation for the complete namespace database. This is an estimate for
    /// plugin persistence because SQLite metadata and free pages are shared across categories.
    pub physical_bytes_estimate: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetentionPolicy {
    pub category: String,
    pub max_age_seconds: Option<u64>,
    pub max_logical_bytes: Option<u64>,
}

#[derive(Debug, thiserror::Error)]
pub enum PersistenceError {
    #[error("invalid plugin persistence namespace")]
    InvalidNamespace,
    #[error("invalid plugin persistence provenance")]
    InvalidProvenance,
    #[error("invalid plugin persistence selector or payload")]
    InvalidInput,
    #[error("plugin persistence record is protected by the user")]
    Protected,
    #[error("archived plugin persistence requires an explicit host reuse decision")]
    ArchivedRecordExists,
    #[error("plugin persistence database could not be opened")]
    Open(#[source] std::io::Error),
    #[error("plugin persistence database operation failed")]
    Database(#[from] rusqlite::Error),
    #[error("plugin persistence database was written by a newer host")]
    NewerVersion,
}

/// One logical owner/profile persistence database.
pub struct PluginPersistence {
    conn: Connection,
}

impl PluginPersistence {
    /// Opens the persistence file below `root` using only the host-authenticated namespace.
    /// No path is returned to API callers. This function performs blocking filesystem work.
    pub fn open(root: &Path, owner: &OwnerNamespace) -> Result<Self, PersistenceError> {
        owner.validate()?;
        let dir = owner_dir(root, owner);
        fs::create_dir_all(&dir).map_err(PersistenceError::Open)?;
        let conn = Connection::open(dir.join(FILE_NAME))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(PersistenceError::NewerVersion);
        }
        if version < 1 {
            conn.execute_batch(
                "BEGIN IMMEDIATE;
                 CREATE TABLE IF NOT EXISTS records (
                   category TEXT NOT NULL,
                   logical_key TEXT NOT NULL,
                   value BLOB NOT NULL,
                   revision INTEGER NOT NULL CHECK (revision > 0),
                   source_plugin_id TEXT NOT NULL,
                   source_publisher_id TEXT NOT NULL,
                   source_version TEXT NOT NULL,
                   package_id TEXT NOT NULL,
                   created_at INTEGER NOT NULL,
                   updated_at INTEGER NOT NULL,
                   expires_at INTEGER,
                   user_protected INTEGER NOT NULL DEFAULT 0 CHECK (user_protected IN (0, 1)),
                   archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
                   PRIMARY KEY (category, logical_key)
                 ) STRICT;
                 CREATE INDEX IF NOT EXISTS records_expiry ON records(category, expires_at);
                 CREATE TABLE IF NOT EXISTS retention (
                   category TEXT PRIMARY KEY,
                   max_age_seconds INTEGER,
                   max_logical_bytes INTEGER,
                   updated_at INTEGER NOT NULL
                 ) STRICT;
                 PRAGMA user_version = 1;
                 COMMIT;",
            )?;
        }
        Ok(Self { conn })
    }

    /// Reads one active, unexpired record. Archived and expired values are not returned.
    pub fn get(
        &self,
        category: &str,
        logical_key: &str,
        now: i64,
    ) -> Result<Option<PersistedRecord>, PersistenceError> {
        validate_selector(category, logical_key)?;
        self.conn
            .query_row(
                "SELECT category, logical_key, value, revision, source_plugin_id,
                        source_publisher_id, source_version, package_id, created_at,
                        updated_at, expires_at, user_protected, archived
                 FROM records WHERE category=?1 AND logical_key=?2 AND archived=0
                   AND (expires_at IS NULL OR expires_at>?3)",
                params![category, logical_key, now],
                record_from,
            )
            .optional()
            .map_err(Into::into)
    }

    /// Stores one host-attributed record with optimistic revision control and bounded logical
    /// quotas. A revision of zero creates; updates must present the current revision.
    pub fn put(
        &mut self,
        category: &str,
        logical_key: &str,
        value: &[u8],
        provenance: &RecordProvenance,
        expected_revision: u64,
        expires_at: Option<i64>,
        now: i64,
    ) -> Result<PutOutcome, PersistenceError> {
        validate_selector(category, logical_key)?;
        provenance.validate()?;
        if value.len() > MAX_RECORD_BYTES || expires_at.is_some_and(|expiry| expiry <= now) {
            return Err(PersistenceError::InvalidInput);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM records WHERE archived=0 AND expires_at IS NOT NULL AND expires_at<=?1 AND user_protected=0",
            [now],
        )?;
        let archived: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM records WHERE category=?1 AND logical_key=?2 AND archived=1)",
            params![category, logical_key],
            |row| row.get(0),
        )?;
        if archived {
            return Err(PersistenceError::ArchivedRecordExists);
        }
        let existing: Option<(u64, i64, i64)> = tx
            .query_row(
                "SELECT revision, created_at, user_protected FROM records WHERE category=?1 AND logical_key=?2 AND archived=0",
                params![category, logical_key],
                |row| Ok((row.get::<_, i64>(0)? as u64, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let current = existing.as_ref().map_or(0, |entry| entry.0);
        if existing.as_ref().is_some_and(|entry| entry.2 != 0) {
            return Err(PersistenceError::Protected);
        }
        if current != expected_revision {
            return Ok(PutOutcome::Conflict {
                current_revision: current,
            });
        }
        let revision = current.saturating_add(1);
        let created_at = existing.map_or(now, |entry| entry.1);
        let candidate_charge = record_charge(category, logical_key, value.len(), provenance);
        let category_bytes = usage_bytes(&tx, Some(category), Some((category, logical_key)), now)?;
        let owner_bytes = usage_bytes(&tx, None, Some((category, logical_key)), now)?;
        let category_quota = tx
            .query_row(
                "SELECT max_logical_bytes FROM retention WHERE category=?1",
                [category],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten()
            .map(|value| value.max(0) as u64)
            .unwrap_or(MAX_CATEGORY_LOGICAL_BYTES)
            .min(MAX_CATEGORY_LOGICAL_BYTES);
        if category_bytes.saturating_add(candidate_charge) > category_quota
            || owner_bytes.saturating_add(candidate_charge) > MAX_OWNER_LOGICAL_BYTES
        {
            return Ok(PutOutcome::OverQuota {
                category_bytes,
                owner_bytes,
            });
        }
        tx.execute(
            "INSERT INTO records(category,logical_key,value,revision,source_plugin_id,
               source_publisher_id,source_version,package_id,created_at,updated_at,expires_at,
               user_protected,archived)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,0)
             ON CONFLICT(category,logical_key) DO UPDATE SET value=excluded.value,
               revision=excluded.revision, source_plugin_id=excluded.source_plugin_id,
               source_publisher_id=excluded.source_publisher_id, source_version=excluded.source_version,
               package_id=excluded.package_id, updated_at=excluded.updated_at,
               expires_at=excluded.expires_at, user_protected=excluded.user_protected, archived=0",
            params![category, logical_key, value, revision as i64, provenance.source_plugin_id,
                provenance.source_publisher_id, provenance.source_version, provenance.package_id,
                created_at, now, expires_at, false],
        )?;
        tx.commit()?;
        Ok(PutOutcome::Stored {
            revision,
            logical_bytes: category_bytes.saturating_add(candidate_charge),
        })
    }

    /// Returns one logical page, ordered by key. `after_key` is an opaque host-issued cursor
    /// value bound by the host to this owner and category; it is never interpreted as a path.
    pub fn list(
        &self,
        category: &str,
        after_key: Option<&str>,
        page_size: u32,
        now: i64,
    ) -> Result<Vec<PersistedRecord>, PersistenceError> {
        validate_category(category)?;
        if page_size == 0 || page_size > MAX_PAGE_SIZE {
            return Err(PersistenceError::InvalidInput);
        }
        let mut statement = self.conn.prepare(
            "SELECT category, logical_key, value, revision, source_plugin_id,
                    source_publisher_id, source_version, package_id, created_at,
                    updated_at, expires_at, user_protected, archived
             FROM records WHERE category=?1 AND archived=0
               AND (expires_at IS NULL OR expires_at>?2) AND (?3 IS NULL OR logical_key>?3)
             ORDER BY logical_key LIMIT ?4",
        )?;
        statement
            .query_map(params![category, now, after_key, page_size], record_from)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Deletes one record only when its revision matches; protected user records are retained.
    pub fn delete(
        &mut self,
        category: &str,
        logical_key: &str,
        expected_revision: u64,
    ) -> Result<bool, PersistenceError> {
        validate_selector(category, logical_key)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let found: Option<(u64, bool)> = tx
            .query_row(
                "SELECT revision,user_protected FROM records WHERE category=?1 AND logical_key=?2 AND archived=0",
                params![category, logical_key],
                |row| Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? != 0)),
            )
            .optional()?;
        let Some((revision, protected)) = found else {
            return Ok(false);
        };
        if protected {
            return Err(PersistenceError::Protected);
        }
        if revision != expected_revision {
            return Ok(false);
        }
        tx.execute(
            "DELETE FROM records WHERE category=?1 AND logical_key=?2 AND revision=?3",
            params![category, logical_key, expected_revision as i64],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Clears unprotected, active records in one category. Archived and user-protected records
    /// remain inert and are reserved for explicit host administration.
    pub fn clear_category(&mut self, category: &str) -> Result<u64, PersistenceError> {
        validate_category(category)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let removed = tx.execute(
            "DELETE FROM records WHERE category=?1 AND archived=0 AND user_protected=0",
            [category],
        )? as u64;
        tx.commit()?;
        Ok(removed)
    }

    /// Applies a host-only user protection decision. Plugin publication never controls this bit.
    pub fn set_user_protected(
        &mut self,
        category: &str,
        logical_key: &str,
        protected: bool,
    ) -> Result<bool, PersistenceError> {
        validate_selector(category, logical_key)?;
        let changed = self.conn.execute(
            "UPDATE records SET user_protected=?3 WHERE category=?1 AND logical_key=?2 AND archived=0",
            params![category, logical_key, protected],
        )?;
        Ok(changed != 0)
    }

    /// Sets a bounded category retention policy. `None` leaves that dimension uncapped up to
    /// the hard host maximum; user-level UI policy and permission checks belong to the host.
    pub fn set_retention_policy(
        &mut self,
        category: &str,
        max_age_seconds: Option<u64>,
        max_logical_bytes: Option<u64>,
        now: i64,
    ) -> Result<(), PersistenceError> {
        validate_category(category)?;
        if max_age_seconds.is_some_and(|age| age == 0 || age > MAX_RETENTION_AGE_SECONDS)
            || max_logical_bytes
                .is_some_and(|bytes| bytes == 0 || bytes > MAX_CATEGORY_LOGICAL_BYTES)
        {
            return Err(PersistenceError::InvalidInput);
        }
        self.conn.execute(
            "INSERT INTO retention(category,max_age_seconds,max_logical_bytes,updated_at)
             VALUES(?1,?2,?3,?4) ON CONFLICT(category) DO UPDATE SET
             max_age_seconds=excluded.max_age_seconds,max_logical_bytes=excluded.max_logical_bytes,
             updated_at=excluded.updated_at",
            params![
                category,
                max_age_seconds.map(|v| v as i64),
                max_logical_bytes.map(|v| v as i64),
                now
            ],
        )?;
        Ok(())
    }

    pub fn retention_policy(
        &self,
        category: &str,
    ) -> Result<Option<RetentionPolicy>, PersistenceError> {
        validate_category(category)?;
        self.conn
            .query_row(
                "SELECT category,max_age_seconds,max_logical_bytes FROM retention WHERE category=?1",
                [category],
                |row| Ok(RetentionPolicy {
                    category: row.get(0)?,
                    max_age_seconds: row.get::<_, Option<i64>>(1)?.map(|v| v.max(0) as u64),
                    max_logical_bytes: row.get::<_, Option<i64>>(2)?.map(|v| v.max(0) as u64),
                }),
            )
            .optional()
            .map_err(Into::into)
    }

    /// Applies expiry and age/size retention atomically. It deletes only unprotected, active
    /// records; archived and user-protected records are not reclaimed by owner policy.
    pub fn cleanup_category(&mut self, category: &str, now: i64) -> Result<u64, PersistenceError> {
        validate_category(category)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Include expired rows in reclaimed logical bytes, although ordinary usage reports omit
        // them. This reports the logical amount removed, not SQLite pages that may remain allocated.
        let before = usage_bytes(&tx, Some(category), None, i64::MIN)?;
        tx.execute(
            "DELETE FROM records WHERE category=?1 AND archived=0 AND user_protected=0
               AND expires_at IS NOT NULL AND expires_at<=?2",
            params![category, now],
        )?;
        let max_age = tx
            .query_row(
                "SELECT max_age_seconds FROM retention WHERE category=?1",
                [category],
                |r| r.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten();
        if let Some(age) = max_age {
            tx.execute(
                "DELETE FROM records WHERE category=?1 AND archived=0 AND user_protected=0 AND updated_at<=?2",
                params![category, now.saturating_sub(age)],
            )?;
        }
        let max_bytes = tx
            .query_row(
                "SELECT max_logical_bytes FROM retention WHERE category=?1",
                [category],
                |r| r.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten()
            .unwrap_or(MAX_CATEGORY_LOGICAL_BYTES as i64)
            .max(0) as u64;
        while usage_bytes(&tx, Some(category), None, now)? > max_bytes {
            let victim: Option<String> = tx.query_row(
                "SELECT logical_key FROM records WHERE category=?1 AND archived=0 AND user_protected=0
                 ORDER BY updated_at ASC, logical_key ASC LIMIT 1",
                [category], |r| r.get(0),
            ).optional()?;
            let Some(victim) = victim else { break };
            tx.execute(
                "DELETE FROM records WHERE category=?1 AND logical_key=?2",
                params![category, victim],
            )?;
        }
        let after = usage_bytes(&tx, Some(category), None, i64::MIN)?;
        tx.commit()?;
        Ok(before.saturating_sub(after))
    }

    /// Makes this owner's records inert for uninstall retention. Ordinary plugin queries exclude
    /// them; a later install needs a separate host-authorized reuse flow.
    pub fn archive_all(&mut self, now: i64) -> Result<u64, PersistenceError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let count = tx.execute(
            "UPDATE records SET archived=1,updated_at=?1 WHERE archived=0",
            [now],
        )? as u64;
        tx.commit()?;
        Ok(count)
    }

    /// Host-side reclaim of archived records in this owner namespace only.
    pub fn reclaim_archived(&mut self) -> Result<u64, PersistenceError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let count = tx.execute(
            "DELETE FROM records WHERE archived=1 AND user_protected=0",
            [],
        )? as u64;
        tx.commit()?;
        Ok(count)
    }

    pub fn usage(&self, now: i64) -> Result<Usage, PersistenceError> {
        let mut statement = self.conn.prepare(
            "SELECT category,COALESCE(SUM(length(value)+length(category)+length(logical_key)+
                  length(source_plugin_id)+length(source_publisher_id)+length(source_version)+length(package_id)+?1),0),COUNT(*)
             FROM records WHERE archived=0 AND (expires_at IS NULL OR expires_at>?2) GROUP BY category ORDER BY category",
        )?;
        let categories = statement
            .query_map(params![RECORD_OVERHEAD_BYTES as i64, now], |row| {
                Ok(CategoryUsage {
                    category: row.get(0)?,
                    logical_bytes: row.get::<_, i64>(1)?.max(0) as u64,
                    records: row.get::<_, i64>(2)?.max(0) as u64,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let logical_bytes = categories.iter().map(|c| c.logical_bytes).sum();
        let page_count: i64 = self.conn.query_row("PRAGMA page_count", [], |r| r.get(0))?;
        let page_size: i64 = self.conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        Ok(Usage {
            categories,
            logical_bytes,
            physical_bytes_estimate: page_count.max(0) as u64 * page_size.max(0) as u64,
        })
    }
}

fn owner_dir(root: &Path, owner: &OwnerNamespace) -> PathBuf {
    root.join(component(&owner.publisher_id))
        .join(component(&owner.plugin_id))
        .join(component(&owner.profile_id))
}

fn component(value: &str) -> String {
    if value.len() <= 100
        && !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        value.to_owned()
    } else {
        let digest = Sha256::digest(value.as_bytes());
        format!(
            "h-{}",
            digest[..16]
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        )
    }
}

fn validate_category(category: &str) -> Result<(), PersistenceError> {
    if category.is_empty()
        || category.len() > MAX_CATEGORY_BYTES
        || category
            .chars()
            .any(|ch| ch.is_control() || ch == '/' || ch == '\\')
    {
        return Err(PersistenceError::InvalidInput);
    }
    Ok(())
}

fn validate_selector(category: &str, key: &str) -> Result<(), PersistenceError> {
    validate_category(category)?;
    if key.is_empty() || key.len() > MAX_LOGICAL_KEY_BYTES || key.chars().any(char::is_control) {
        return Err(PersistenceError::InvalidInput);
    }
    Ok(())
}

fn record_charge(
    category: &str,
    key: &str,
    value_len: usize,
    provenance: &RecordProvenance,
) -> u64 {
    category.len() as u64
        + key.len() as u64
        + value_len as u64
        + provenance.source_plugin_id.len() as u64
        + provenance.source_publisher_id.len() as u64
        + provenance.source_version.len() as u64
        + provenance.package_id.len() as u64
        + RECORD_OVERHEAD_BYTES
}

fn usage_bytes(
    conn: &Connection,
    category: Option<&str>,
    except: Option<(&str, &str)>,
    now: i64,
) -> Result<u64, rusqlite::Error> {
    let query = "SELECT COALESCE(SUM(length(value)+length(category)+length(logical_key)+
         length(source_plugin_id)+length(source_publisher_id)+length(source_version)+length(package_id)+?1),0)
         FROM records WHERE archived=0 AND (expires_at IS NULL OR expires_at>?2)
         AND (?3 IS NULL OR category=?3) AND (?4 IS NULL OR category<>?4 OR logical_key<>?5)";
    let bytes: i64 = conn.query_row(
        query,
        params![
            RECORD_OVERHEAD_BYTES as i64,
            now,
            category,
            except.map(|e| e.0),
            except.map(|e| e.1)
        ],
        |r| r.get(0),
    )?;
    Ok(bytes.max(0) as u64)
}

fn record_from(row: &rusqlite::Row<'_>) -> rusqlite::Result<PersistedRecord> {
    Ok(PersistedRecord {
        category: row.get(0)?,
        logical_key: row.get(1)?,
        value: row.get(2)?,
        revision: row.get::<_, i64>(3)?.max(0) as u64,
        provenance: RecordProvenance {
            source_plugin_id: row.get(4)?,
            source_publisher_id: row.get(5)?,
            source_version: row.get(6)?,
            package_id: row.get(7)?,
        },
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        expires_at: row.get(10)?,
        user_protected: row.get::<_, i64>(11)? != 0,
        archived: row.get::<_, i64>(12)? != 0,
    })
}
