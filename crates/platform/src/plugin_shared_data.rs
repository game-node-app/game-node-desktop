//! Host-owned durable pool for public shared-data records.
//!
//! The pool lives in its own SQLite file, `plugin-shared-data.gnpersistence`, below the application
//! data directory. It is separate from the application settings/library database and from the
//! private `plugin-data` stores, so a public publication can outlive its producer session without
//! making private storage public.
//!
//! The store accepts only host-resolved values: an opaque owner key, host-assigned provenance and
//! an already validated payload. It never accepts a plugin id from an RPC, a path, a plugin
//! selector for another owner, or a session id; session-only rows are never written here. Every
//! method performs blocking SQLite work and must run on the host's storage worker, never on the UI
//! thread. No method or error message discloses a physical path.
//!
//! A record with a resource id is replaced in place for the same owner, dataset and resource:
//! its record id stays stable, its record revision increases by one and it gets the next pool
//! revision. A record without a resource id is appended with a new host-assigned record id. The
//! pool revision is persisted in `shared_state` and shared by lists and watches; pruning or
//! protection changes also advance it when the visible pool changed.
//!
//! Uninstall retention can archive every row of one owner. Archived rows stay attributed to the
//! original owner and remain on disk, but ordinary reads, lists, usage and automatic pruning skip
//! them. A separate host review restores them (`reuse_archived_owner`) or the host deletes them;
//! a plugin can never archive, reuse or inspect archived records through an RPC.

use std::fs;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

/// File name of the shared-data pool inside the application data directory.
pub const FILE_NAME: &str = "plugin-shared-data.gnpersistence";
/// Pre-release builds used the generic SQLite extension. Existing pools are renamed on open.
pub const LEGACY_FILE_NAME: &str = "plugin-shared-data.sqlite3";

/// The schema version this build writes.
pub const CURRENT_SCHEMA_VERSION: u32 = 3;

/// The longest dataset, schema and resource identifier.
pub const MAX_IDENTIFIER_BYTES: usize = 256;
/// The longest sharing class string.
pub const MAX_CLASS_BYTES: usize = 32;
/// The largest payload (structured JSON or opaque bytes) of one record.
pub const MAX_RECORD_BYTES: usize = 1024 * 1024;
/// The hard per-owner logical byte ceiling, regardless of a lower user policy.
pub const MAX_OWNER_LOGICAL_BYTES: u64 = 64 * 1024 * 1024;
/// The longest age a user retention policy may select (365 days).
pub const MAX_RETENTION_AGE_SECONDS: u64 = 365 * 24 * 60 * 60;
/// The longest time-to-live a producer may request (30 days).
pub const MAX_TTL_SECONDS: u64 = 30 * 24 * 60 * 60;
/// How many rows one bounded prune may remove.
pub const MAX_PRUNE_ROWS: u64 = 1024;
/// Charged per record on top of its identifiers, provenance and payload.
const RECORD_OVERHEAD_BYTES: u64 = 256;

struct Migration {
    version: u32,
    sql: &'static str,
}

/// Every schema step this build knows. Versions start at 1 and increase by exactly one.
///
/// Version 1 is the initial pool; version 2 adds host-controlled retention protection, which is
/// never supplied by a plugin. An existing database is upgraded in order and keeps its records.
const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: "CREATE TABLE shared_state (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 revision INTEGER NOT NULL CHECK (revision >= 0)
             ) STRICT;
             INSERT INTO shared_state (id, revision) VALUES (1, 0);
             CREATE TABLE shared_owners (
                 owner_key BLOB PRIMARY KEY CHECK (length(owner_key) = 32),
                 profile_epoch INTEGER NOT NULL CHECK (profile_epoch > 0),
                 updated_at INTEGER NOT NULL
             ) STRICT;
             CREATE TABLE shared_records (
                 record_id TEXT PRIMARY KEY,
                 owner_key BLOB NOT NULL
                     REFERENCES shared_owners(owner_key) ON DELETE CASCADE
                     CHECK (length(owner_key) = 32),
                 dataset_id TEXT NOT NULL,
                 resource_id TEXT,
                 category INTEGER CHECK (category IS NULL OR category >= 0),
                 class TEXT NOT NULL,
                 schema_id TEXT NOT NULL,
                 schema_version INTEGER NOT NULL CHECK (schema_version > 0),
                 fields_json TEXT NOT NULL,
                 opaque_payload BLOB NOT NULL,
                 source_plugin_id TEXT NOT NULL,
                 source_publisher_fingerprint TEXT NOT NULL,
                 source_profile_id TEXT NOT NULL,
                 declared_developer TEXT NOT NULL,
                 origin TEXT NOT NULL,
                 package_version TEXT NOT NULL,
                 package_digest BLOB NOT NULL,
                 observed_at INTEGER NOT NULL CHECK (observed_at >= 0),
                 received_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL,
                 expires_at INTEGER,
                 sequence INTEGER NOT NULL CHECK (sequence >= 0),
                 source_observation_id TEXT NOT NULL,
                 revision INTEGER NOT NULL CHECK (revision >= 1),
                 pool_revision INTEGER NOT NULL CHECK (pool_revision >= 1),
                 logical_bytes INTEGER NOT NULL CHECK (logical_bytes >= 0),
                 UNIQUE (owner_key, dataset_id, resource_id)
             ) STRICT;
             CREATE INDEX shared_records_owner_dataset
                 ON shared_records(owner_key, dataset_id, received_at, record_id);
             CREATE INDEX shared_records_owner_expiry
                 ON shared_records(owner_key, expires_at);
             CREATE INDEX shared_records_owner_updated
                 ON shared_records(owner_key, updated_at);
             CREATE TABLE shared_retention (
                 owner_key BLOB PRIMARY KEY
                     REFERENCES shared_owners(owner_key) ON DELETE CASCADE,
                 max_age_seconds INTEGER CHECK (max_age_seconds IS NULL OR max_age_seconds > 0),
                 max_bytes INTEGER CHECK (max_bytes IS NULL OR max_bytes > 0),
                 updated_at INTEGER NOT NULL
             ) STRICT;",
    },
    Migration {
        version: 2,
        sql: "ALTER TABLE shared_records
                 ADD COLUMN user_protected INTEGER NOT NULL DEFAULT 0
                     CHECK (user_protected IN (0, 1));
             CREATE INDEX shared_records_owner_protection
                 ON shared_records(owner_key, user_protected, updated_at);",
    },
    Migration {
        version: 3,
        sql: "ALTER TABLE shared_records
                 ADD COLUMN archived INTEGER NOT NULL DEFAULT 0
                     CHECK (archived IN (0, 1));
              CREATE INDEX shared_records_owner_archived
                  ON shared_records(owner_key, archived, updated_at);",
    },
];

const RECORD_COLUMNS: &str = "record_id, dataset_id, category, class, schema_id, schema_version, \
     fields_json, opaque_payload, resource_id, observed_at, received_at, updated_at, expires_at, \
     sequence, source_observation_id, revision, pool_revision, logical_bytes, user_protected, \
     source_plugin_id, source_publisher_fingerprint, source_profile_id, declared_developer, \
     origin, package_version, package_digest, archived";

/// Why the shared-data pool could not be used. Messages never contain a path.
#[derive(Debug, thiserror::Error)]
pub enum SharedDataError {
    #[error("the shared-data pool could not be opened")]
    Open(#[source] std::io::Error),
    #[error("the shared-data pool operation failed")]
    Database(#[from] rusqlite::Error),
    #[error("the shared-data pool was written by a newer version of GameNode")]
    NewerVersion,
    #[error("the file is not a GameNode shared-data pool")]
    ForeignDatabase,
    #[error("the shared-data schema migration failed")]
    MigrationFailed { version: u32 },
    #[error("invalid shared-data owner scope")]
    InvalidOwner,
    #[error("invalid shared-data record")]
    InvalidRecord,
    #[error("the shared-data record exceeds the size limit")]
    RecordTooLarge,
    #[error("the shared-data owner scope is stale")]
    StaleProfileEpoch,
    #[error("archived shared data requires an explicit host reuse decision")]
    ArchivedRecordExists,
    #[error("a publisher migration needs two different owner scopes")]
    InvalidMigration,
}

/// Host-resolved ownership. The opaque key is derived by the host from the authenticated plugin,
/// publisher and profile; the epoch fences operations queued for an earlier profile session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnerScope {
    key: [u8; 32],
    profile_epoch: u64,
}

impl OwnerScope {
    pub fn new(key: [u8; 32], profile_epoch: u64) -> Result<Self, SharedDataError> {
        if profile_epoch == 0 || profile_epoch > i64::MAX as u64 {
            return Err(SharedDataError::InvalidOwner);
        }
        Ok(Self { key, profile_epoch })
    }

    fn key(&self) -> &[u8] {
        &self.key
    }
}

/// Provenance assigned by the host from the signed producer package and authenticated session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordProvenance {
    pub source_plugin_id: String,
    pub source_publisher_fingerprint: String,
    pub source_profile_id: String,
    pub declared_developer: String,
    pub origin: String,
    pub package_version: String,
    pub package_digest: Vec<u8>,
}

impl RecordProvenance {
    fn validate(&self) -> Result<(), SharedDataError> {
        for value in [
            &self.source_plugin_id,
            &self.source_publisher_fingerprint,
            &self.source_profile_id,
            &self.declared_developer,
            &self.origin,
            &self.package_version,
        ] {
            validate_text(value, MAX_IDENTIFIER_BYTES)?;
        }
        if self.package_digest.len() != 32 {
            return Err(SharedDataError::InvalidRecord);
        }
        Ok(())
    }
}

/// One host-validated publication for the durable pool.
pub struct RecordWrite<'a> {
    pub dataset_id: &'a str,
    pub category: Option<i32>,
    pub class: &'a str,
    pub schema_id: &'a str,
    pub schema_version: u32,
    /// A non-empty JSON object for structured data, otherwise empty.
    pub fields_json: &'a str,
    /// Opaque bytes for a binary record, otherwise empty.
    pub opaque_payload: &'a [u8],
    /// Some for a producer resource that is replaced in place; None appends a new record.
    pub resource_id: Option<&'a str>,
    pub observed_at: i64,
    pub sequence: u64,
    pub source_observation_id: &'a str,
    /// Unix seconds, already bounded by [`MAX_TTL_SECONDS`] from `now`.
    pub expires_at: Option<i64>,
    /// Unix seconds of the host's receipt.
    pub now: i64,
    pub provenance: &'a RecordProvenance,
    /// Some requires the current record revision; None replaces unconditionally.
    pub expected_revision: Option<u64>,
}

/// One durable record as read back. Only logical values are exposed; no path is included.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedRecord {
    pub record_id: String,
    pub dataset_id: String,
    pub category: Option<i32>,
    pub class: String,
    pub schema_id: String,
    pub schema_version: u32,
    pub fields_json: String,
    pub opaque_payload: Vec<u8>,
    pub resource_id: Option<String>,
    pub observed_at: i64,
    pub received_at: i64,
    pub updated_at: i64,
    pub expires_at: Option<i64>,
    pub sequence: u64,
    pub source_observation_id: String,
    /// Increments only when this owner/dataset/resource record is replaced.
    pub revision: u64,
    /// Pool sequence of the latest write to this record.
    pub pool_revision: u64,
    /// Logical bytes this record is charged.
    pub size_bytes: u64,
    /// Host-only retention protection; a plugin cannot set it.
    pub user_protected: bool,
    /// True while the record is retained from an uninstalled producer. Ordinary reads and usage
    /// exclude archived rows; a host review restores them or the host deletes them.
    pub archived: bool,
    pub provenance: RecordProvenance,
}

/// A stable page of one owner's dataset plus the pool revision it was read at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatasetRecords {
    pub records: Vec<SharedRecord>,
    pub pool_revision: u64,
}

/// The result of a successful write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteReceipt {
    pub record: SharedRecord,
    pub replaced: bool,
    /// Rows removed by the bounded retention pass that accompanied the write.
    pub pruned_records: u64,
    pub reclaimed_bytes: u64,
}

/// The outcome of a publication attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteOutcome {
    Stored(WriteReceipt),
    /// The expected record revision did not match. Nothing changed.
    Conflict {
        current_revision: u64,
    },
    /// Even after bounded pruning the owner would exceed its byte limit. Nothing changed.
    OverQuota {
        usage: OwnerUsage,
    },
}

/// The result of a bounded prune.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PruneOutcome {
    pub removed_records: u64,
    pub reclaimed_bytes: u64,
    pub pool_revision: u64,
}

/// The result of archiving or reusing one owner's retained records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchiveOutcome {
    /// Rows whose archived flag actually changed.
    pub records: u64,
    pub pool_revision: u64,
}

/// User-selected limits. `None` leaves that dimension at the host default: no age limit and
/// [`MAX_OWNER_LOGICAL_BYTES`] for size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RetentionPolicy {
    pub max_age_seconds: Option<u64>,
    pub max_bytes: Option<u64>,
}

/// Logical usage of one owner. `max_bytes` and `max_age_seconds` are the effective limits.
///
/// `records` and `logical_bytes` count only rows visible to ordinary reads and quota. Archived
/// rows retained for an uninstalled producer are reported separately and are never pruned
/// automatically.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnerUsage {
    pub records: u64,
    pub logical_bytes: u64,
    pub max_bytes: u64,
    pub max_age_seconds: Option<u64>,
    pub oldest_updated_at: Option<i64>,
    pub archived_records: u64,
    pub archived_logical_bytes: u64,
    /// Live records the user protected from pruning.
    pub protected_records: u64,
    /// SQLite page allocation for the whole pool; shared across owners.
    pub physical_bytes_estimate: u64,
}

/// One owner's live records as a reviewed publisher migration sees them: logical metadata only,
/// never a payload or a path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordMetadata {
    pub record_id: String,
    pub dataset_id: String,
    pub resource_id: Option<String>,
    pub class: String,
    pub schema_id: String,
    pub schema_version: u32,
    pub observed_at: i64,
    pub received_at: i64,
    pub updated_at: i64,
    pub expires_at: Option<i64>,
    pub sequence: u64,
    pub size_bytes: u64,
    pub user_protected: bool,
}

/// Everything the old publisher identity holds in the durable pool for one migration preview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationSource {
    /// Live, unexpired records; archived and expired rows are only counted.
    pub records: Vec<RecordMetadata>,
    pub expired_records: u64,
    pub archived_records: u64,
    pub logical_bytes: u64,
}

/// One record copied into the new publisher identity. Provenance keeps the original source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigratedRecord {
    pub source_record_id: String,
    pub record_id: String,
    pub dataset_id: String,
    pub resource_id: Option<String>,
    pub schema_id: String,
    pub schema_version: u32,
    pub size_bytes: u64,
}

/// Why one candidate record was not copied. Nothing changed for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationSkip {
    pub source_record_id: String,
    pub dataset_id: String,
    pub resource_id: Option<String>,
    /// `conflict` or `over_quota`.
    pub reason: String,
}

/// The result of copying one old publisher identity's records to its successor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationOutcome {
    pub copied: Vec<MigratedRecord>,
    pub skipped: Vec<MigrationSkip>,
    pub pool_revision: u64,
}

/// The result of removing one migration's copies. The old identity's rows stay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MigrationRevertOutcome {
    pub removed_records: u64,
    pub reclaimed_bytes: u64,
    /// Copies the user protected since the migration; rollback keeps them.
    pub kept_protected_records: u64,
    pub pool_revision: u64,
}

/// The host-owned durable public pool.
pub struct SharedDataStore {
    conn: Connection,
}

impl SharedDataStore {
    /// Opens the pool below `data_dir`, creating the directory, file and schema on first use.
    /// Blocking: call this on the storage worker.
    pub fn open(data_dir: &Path) -> Result<Self, SharedDataError> {
        Self::open_with(data_dir, MIGRATIONS)
    }

    /// Opens an existing pool without creating anything; `None` when this host never persisted a
    /// shared record.
    pub fn open_existing(data_dir: &Path) -> Result<Option<Self>, SharedDataError> {
        Self::migrate_legacy_name(data_dir)?;
        if !data_dir.join(FILE_NAME).is_file() {
            return Ok(None);
        }
        Self::open(data_dir).map(Some)
    }

    /// Renames a pre-release `plugin-shared-data.sqlite3` pool, including its WAL sidecars, before
    /// the new name is opened. A failed rename is an error instead of silently starting empty.
    fn migrate_legacy_name(data_dir: &Path) -> Result<(), SharedDataError> {
        let current = data_dir.join(FILE_NAME);
        let legacy = data_dir.join(LEGACY_FILE_NAME);
        if current.exists() || !legacy.is_file() {
            return Ok(());
        }
        fs::rename(&legacy, &current).map_err(SharedDataError::Open)?;
        for suffix in ["-wal", "-shm"] {
            let from = data_dir.join(format!("{LEGACY_FILE_NAME}{suffix}"));
            if from.is_file() {
                fs::rename(&from, data_dir.join(format!("{FILE_NAME}{suffix}")))
                    .map_err(SharedDataError::Open)?;
            }
        }
        Ok(())
    }

    fn open_with(data_dir: &Path, migrations: &[Migration]) -> Result<Self, SharedDataError> {
        Self::migrate_legacy_name(data_dir)?;
        fs::create_dir_all(data_dir).map_err(SharedDataError::Open)?;
        let mut conn = Connection::open(data_dir.join(FILE_NAME))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;

        // Read the guard before any pragma that could write, so a newer database stays untouched.
        let supported = migrations.last().map_or(0, |migration| migration.version) as i64;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > supported {
            return Err(SharedDataError::NewerVersion);
        }
        if version == 0 {
            let tables: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )?;
            if tables > 0 {
                return Err(SharedDataError::ForeignDatabase);
            }
        }

        for (index, migration) in migrations.iter().enumerate() {
            if migration.version != index as u32 + 1 {
                return Err(SharedDataError::MigrationFailed {
                    version: migration.version,
                });
            }
        }

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;",
        )?;
        for migration in migrations
            .iter()
            .filter(|migration| migration.version as i64 > version)
        {
            apply(
                conn.transaction_with_behavior(TransactionBehavior::Immediate)?,
                migration,
            )
            .map_err(|_| SharedDataError::MigrationFailed {
                version: migration.version,
            })?;
        }
        Ok(Self { conn })
    }

    /// The current pool revision. It only grows and survives restarts.
    pub fn pool_revision(&self) -> Result<u64, SharedDataError> {
        let revision: i64 = self.conn.query_row(
            "SELECT revision FROM shared_state WHERE id = 1",
            [],
            |row| row.get(0),
        )?;
        Ok(revision.max(0) as u64)
    }

    /// Stores a publication, replacing the owner's record for the same dataset and resource.
    pub fn write(
        &mut self,
        owner: OwnerScope,
        write: RecordWrite<'_>,
    ) -> Result<WriteOutcome, SharedDataError> {
        validate_write(&write)?;
        write.provenance.validate()?;
        let charge = record_charge(&write)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        check_owner(&tx, &owner, write.now, true)?;

        let existing = match write.resource_id {
            Some(resource_id) => tx
                .query_row(
                    "SELECT record_id, revision, archived FROM shared_records
                     WHERE owner_key = ?1 AND dataset_id = ?2 AND resource_id = ?3",
                    params![owner.key(), write.dataset_id, resource_id],
                    |row| {
                        Ok(ExistingRecord {
                            record_id: row.get(0)?,
                            revision: row.get::<_, i64>(1)?.max(0) as u64,
                            archived: row.get::<_, i64>(2)? != 0,
                        })
                    },
                )
                .optional()?,
            None => None,
        };
        // A producer that was uninstalled with retained data, then reinstalled, must not replace
        // an archived row before the host reviews reuse. Refuse the write without changing it.
        if existing.as_ref().is_some_and(|record| record.archived) {
            return Err(SharedDataError::ArchivedRecordExists);
        }
        let current_revision = existing.as_ref().map_or(0, |record| record.revision);
        if let Some(expected) = write.expected_revision
            && expected != current_revision
        {
            return Ok(WriteOutcome::Conflict { current_revision });
        }

        let usage_before = owner_usage(&tx, &owner, write.now)?;
        let mut pruned = PruneProgress::default();
        prune_expired(&tx, &owner, write.now, &mut pruned)?;
        prune_aged(&tx, &owner, write.now, &mut pruned)?;

        let keep_id = existing.as_ref().map(|record| record.record_id.as_str());
        let mut live_bytes = usage_excluding(&tx, &owner, write.now, keep_id)?;
        while live_bytes.saturating_add(charge) > usage_before.max_bytes
            && pruned.records < MAX_PRUNE_ROWS
        {
            let Some(victim) = oldest_unprotected(&tx, &owner, write.now, keep_id)? else {
                break;
            };
            delete_record(&tx, &victim)?;
            pruned.records += 1;
            pruned.bytes = pruned.bytes.saturating_add(victim.size_bytes);
            live_bytes = live_bytes.saturating_sub(victim.size_bytes);
        }
        if live_bytes.saturating_add(charge) > usage_before.max_bytes {
            return Ok(WriteOutcome::OverQuota {
                usage: usage_before,
            });
        }

        let pool_revision = advance_pool(&tx)?;
        // Durable ids use their own prefix so they can never collide with the in-memory broker's
        // `shared-` ids, which are numbered in a separate sequence.
        let record_id = existing.as_ref().map_or_else(
            || format!("durable-{pool_revision:016x}"),
            |record| record.record_id.clone(),
        );
        let revision = current_revision.saturating_add(1);
        if let Some(existing) = &existing {
            tx.execute(
                "UPDATE shared_records SET
                     dataset_id = ?2, resource_id = ?3, category = ?4, class = ?5,
                     schema_id = ?6, schema_version = ?7, fields_json = ?8,
                     opaque_payload = ?9, source_plugin_id = ?10,
                     source_publisher_fingerprint = ?11, source_profile_id = ?12,
                     declared_developer = ?13, origin = ?14, package_version = ?15,
                     package_digest = ?16, observed_at = ?17, updated_at = ?18,
                     expires_at = ?19, sequence = ?20, source_observation_id = ?21,
                     revision = ?22, pool_revision = ?23, logical_bytes = ?24
                 WHERE owner_key = ?25 AND record_id = ?1",
                params![
                    existing.record_id,
                    write.dataset_id,
                    write.resource_id,
                    write.category,
                    write.class,
                    write.schema_id,
                    i64::from(write.schema_version),
                    write.fields_json,
                    write.opaque_payload,
                    write.provenance.source_plugin_id,
                    write.provenance.source_publisher_fingerprint,
                    write.provenance.source_profile_id,
                    write.provenance.declared_developer,
                    write.provenance.origin,
                    write.provenance.package_version,
                    write.provenance.package_digest,
                    write.observed_at,
                    write.now,
                    write.expires_at,
                    write.sequence as i64,
                    write.source_observation_id,
                    revision as i64,
                    pool_revision as i64,
                    charge as i64,
                    owner.key(),
                ],
            )?;
        } else {
            tx.execute(
                "INSERT INTO shared_records
                     (record_id, owner_key, dataset_id, resource_id, category, class,
                      schema_id, schema_version, fields_json, opaque_payload,
                      source_plugin_id, source_publisher_fingerprint, source_profile_id,
                      declared_developer, origin, package_version, package_digest,
                      observed_at, received_at, updated_at, expires_at, sequence,
                      source_observation_id, revision, pool_revision, logical_bytes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                         ?16, ?17, ?18, ?19, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
                params![
                    record_id,
                    owner.key(),
                    write.dataset_id,
                    write.resource_id,
                    write.category,
                    write.class,
                    write.schema_id,
                    i64::from(write.schema_version),
                    write.fields_json,
                    write.opaque_payload,
                    write.provenance.source_plugin_id,
                    write.provenance.source_publisher_fingerprint,
                    write.provenance.source_profile_id,
                    write.provenance.declared_developer,
                    write.provenance.origin,
                    write.provenance.package_version,
                    write.provenance.package_digest,
                    write.observed_at,
                    write.now,
                    write.expires_at,
                    write.sequence as i64,
                    write.source_observation_id,
                    revision as i64,
                    pool_revision as i64,
                    charge as i64,
                ],
            )?;
        }
        let record = record_by_id(&tx, &owner, &record_id, write.now)?
            .ok_or(SharedDataError::InvalidRecord)?;
        tx.commit()?;
        Ok(WriteOutcome::Stored(WriteReceipt {
            record,
            replaced: existing.is_some(),
            pruned_records: pruned.records,
            reclaimed_bytes: pruned.bytes,
        }))
    }

    /// Reads one unexpired record only inside this owner scope.
    pub fn get(
        &self,
        owner: &OwnerScope,
        record_id: &str,
        now: i64,
    ) -> Result<Option<SharedRecord>, SharedDataError> {
        validate_now(now)?;
        validate_text(record_id, 64)?;
        check_owner(&self.conn, owner, now, false)?;
        record_by_id(&self.conn, owner, record_id, now)
    }

    /// Reads one unexpired record by host id across every owner. This is a host-only read for the
    /// public read path: the caller must authorize the returned source identity against the
    /// current grant before disclosing any field. Record ids are globally unique.
    pub fn get_any(
        &self,
        record_id: &str,
        now: i64,
    ) -> Result<Option<SharedRecord>, SharedDataError> {
        validate_now(now)?;
        validate_text(record_id, 64)?;
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {RECORD_COLUMNS} FROM shared_records
                     WHERE record_id = ?1 AND archived = 0
                       AND (expires_at IS NULL OR expires_at > ?2)"
                ),
                params![record_id, now],
                record_from,
            )
            .optional()?)
    }

    /// Lists one dataset's unexpired records across every owner in the same stable order as the
    /// owner-scoped list. Host-only: the public read path authorizes each record's source identity
    /// against the current grant before disclosing it.
    pub fn list_dataset_any(
        &self,
        dataset_id: &str,
        now: i64,
    ) -> Result<Vec<SharedRecord>, SharedDataError> {
        validate_now(now)?;
        validate_text(dataset_id, MAX_IDENTIFIER_BYTES)?;
        let mut statement = self.conn.prepare(&format!(
            "SELECT {RECORD_COLUMNS} FROM shared_records
             WHERE dataset_id = ?1 AND archived = 0
               AND (expires_at IS NULL OR expires_at > ?2)
             ORDER BY received_at ASC, record_id ASC"
        ))?;
        statement
            .query_map(params![dataset_id, now], record_from)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Lists one owner's dataset in a stable order (oldest receipt first, then record id) and
    /// returns the pool revision at which the page was read.
    pub fn list_dataset(
        &self,
        owner: &OwnerScope,
        dataset_id: &str,
        now: i64,
    ) -> Result<DatasetRecords, SharedDataError> {
        validate_now(now)?;
        validate_text(dataset_id, MAX_IDENTIFIER_BYTES)?;
        check_owner(&self.conn, owner, now, false)?;
        let mut statement = self.conn.prepare(&format!(
            "SELECT {RECORD_COLUMNS} FROM shared_records
             WHERE owner_key = ?1 AND dataset_id = ?2 AND archived = 0
               AND (expires_at IS NULL OR expires_at > ?3)
             ORDER BY received_at ASC, record_id ASC"
        ))?;
        let records = statement
            .query_map(params![owner.key(), dataset_id, now], record_from)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(DatasetRecords {
            records,
            pool_revision: self.pool_revision()?,
        })
    }

    /// Logical usage and effective limits of one owner. Expired rows are not counted.
    pub fn usage(&self, owner: &OwnerScope, now: i64) -> Result<OwnerUsage, SharedDataError> {
        validate_now(now)?;
        check_owner(&self.conn, owner, now, false)?;
        owner_usage(&self.conn, owner, now)
    }

    /// Sets the owner's age/byte limits and immediately runs a bounded prune.
    pub fn set_retention(
        &mut self,
        owner: &OwnerScope,
        policy: RetentionPolicy,
        now: i64,
    ) -> Result<PruneOutcome, SharedDataError> {
        validate_now(now)?;
        if policy
            .max_age_seconds
            .is_some_and(|age| age == 0 || age > MAX_RETENTION_AGE_SECONDS)
            || policy
                .max_bytes
                .is_some_and(|bytes| bytes == 0 || bytes > MAX_OWNER_LOGICAL_BYTES)
        {
            return Err(SharedDataError::InvalidRecord);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        check_owner(&tx, owner, now, true)?;
        tx.execute(
            "INSERT INTO shared_retention
                 (owner_key, max_age_seconds, max_bytes, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(owner_key) DO UPDATE SET
                 max_age_seconds = excluded.max_age_seconds,
                 max_bytes = excluded.max_bytes,
                 updated_at = excluded.updated_at",
            params![
                owner.key(),
                policy.max_age_seconds.map(|age| age as i64),
                policy.max_bytes.map(|bytes| bytes as i64),
                now
            ],
        )?;
        let (pruned, removed) = prune_owner(&tx, owner, now)?;
        let pool_revision = if removed {
            advance_pool(&tx)?
        } else {
            current_pool_revision(&tx)?
        };
        tx.commit()?;
        Ok(PruneOutcome {
            removed_records: pruned.records,
            reclaimed_bytes: pruned.bytes,
            pool_revision,
        })
    }

    /// Expires and prunes this owner only, expired first, then age, then the oldest unprotected
    /// rows until the byte limit is met or [`MAX_PRUNE_ROWS`] is reached.
    pub fn prune(&mut self, owner: &OwnerScope, now: i64) -> Result<PruneOutcome, SharedDataError> {
        validate_now(now)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        check_owner(&tx, owner, now, true)?;
        let (pruned, removed) = prune_owner(&tx, owner, now)?;
        let pool_revision = if removed {
            advance_pool(&tx)?
        } else {
            current_pool_revision(&tx)?
        };
        tx.commit()?;
        Ok(PruneOutcome {
            removed_records: pruned.records,
            reclaimed_bytes: pruned.bytes,
            pool_revision,
        })
    }

    /// Removes every durable record of one owner and reports the bytes actually reclaimed.
    ///
    /// This is the host's user-initiated cleanup: it removes retained public records without
    /// uninstalling the producer, including protected and expired rows. Another owner's records,
    /// the owner's retention policy and the private `plugin-data` stores are untouched. Blocking:
    /// call this on the storage worker.
    pub fn delete_owner(
        &mut self,
        owner: &OwnerScope,
        now: i64,
    ) -> Result<PruneOutcome, SharedDataError> {
        validate_now(now)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        check_owner(&tx, owner, now, false)?;
        let (records, bytes): (i64, i64) = tx.query_row(
            "SELECT COUNT(*), COALESCE(SUM(logical_bytes), 0) FROM shared_records
             WHERE owner_key = ?1",
            [owner.key()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if records > 0 {
            tx.execute(
                "DELETE FROM shared_records WHERE owner_key = ?1",
                [owner.key()],
            )?;
            let pool_revision = advance_pool(&tx)?;
            tx.commit()?;
            return Ok(PruneOutcome {
                removed_records: records.max(0) as u64,
                reclaimed_bytes: bytes.max(0) as u64,
                pool_revision,
            });
        }
        let pool_revision = current_pool_revision(&tx)?;
        tx.commit()?;
        Ok(PruneOutcome {
            removed_records: 0,
            reclaimed_bytes: 0,
            pool_revision,
        })
    }

    /// Reads one owner's live records and the counts of rows that a migration cannot copy, without
    /// loading or disclosing a payload. Host-only: the caller binds the owner from authenticated
    /// identity. Blocking: call this on the storage worker.
    pub fn migration_source(
        &self,
        owner: &OwnerScope,
        now: i64,
    ) -> Result<MigrationSource, SharedDataError> {
        validate_now(now)?;
        check_owner(&self.conn, owner, now, false)?;
        let (expired_records, archived_records, logical_bytes): (i64, i64, i64) =
            self.conn.query_row(
                "SELECT
                     COALESCE(SUM(CASE WHEN archived = 0 AND expires_at IS NOT NULL
                                        AND expires_at <= ?2 THEN 1 ELSE 0 END), 0),
                     COALESCE(SUM(CASE WHEN archived = 1 THEN 1 ELSE 0 END), 0),
                     COALESCE(SUM(CASE WHEN archived = 0
                                        AND (expires_at IS NULL OR expires_at > ?2)
                                   THEN logical_bytes ELSE 0 END), 0)
                 FROM shared_records WHERE owner_key = ?1",
                params![owner.key(), now],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
        let mut statement = self.conn.prepare(&format!(
            "SELECT {RECORD_COLUMNS} FROM shared_records
             WHERE owner_key = ?1 AND archived = 0
               AND (expires_at IS NULL OR expires_at > ?2)
             ORDER BY dataset_id ASC, received_at ASC, record_id ASC"
        ))?;
        let records = statement
            .query_map(params![owner.key(), now], record_from)?
            .map(|record| record.map(metadata_from))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(MigrationSource {
            records,
            expired_records: expired_records.max(0) as u64,
            archived_records: archived_records.max(0) as u64,
            logical_bytes: logical_bytes.max(0) as u64,
        })
    }

    /// Copies one owner's live records to another owner after a reviewed publisher re-trust.
    ///
    /// This is the host's migration, never a plugin route. Provenance keeps the original source
    /// identity and package version, and the old owner's rows stay untouched. A record whose
    /// dataset and resource already exist on the new owner is skipped as a conflict, and a record
    /// that would exceed the new owner's byte limit is skipped as over quota; neither changes
    /// anything. One transaction covers every copy. Blocking: call this on the storage worker.
    pub fn migrate_records(
        &mut self,
        source: &OwnerScope,
        target: &OwnerScope,
        record_ids: &[String],
        now: i64,
    ) -> Result<MigrationOutcome, SharedDataError> {
        validate_now(now)?;
        if source.key() == target.key() {
            return Err(SharedDataError::InvalidMigration);
        }
        let selected: std::collections::BTreeSet<&str> =
            record_ids.iter().map(String::as_str).collect();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let records = records_for_owner(&tx, source, now)?
            .into_iter()
            .filter(|record| selected.contains(record.record_id.as_str()))
            .collect::<Vec<_>>();
        let max_bytes = effective_max_bytes(retention_policy(&tx, target)?);
        let mut usage = usage_excluding(&tx, target, now, None)?;
        // Creating the successor owner row only happens on the confirmed mutation.
        check_owner(&tx, target, now, true)?;
        let mut outcome = MigrationOutcome {
            copied: Vec::new(),
            skipped: Vec::new(),
            pool_revision: 0,
        };
        for record in records {
            if record.resource_id.is_some() && resource_exists(&tx, target, &record)? {
                outcome.skipped.push(MigrationSkip {
                    source_record_id: record.record_id,
                    dataset_id: record.dataset_id,
                    resource_id: record.resource_id,
                    reason: "conflict".to_owned(),
                });
                continue;
            }
            if usage.saturating_add(record.size_bytes) > max_bytes {
                outcome.skipped.push(MigrationSkip {
                    source_record_id: record.record_id,
                    dataset_id: record.dataset_id,
                    resource_id: record.resource_id,
                    reason: "over_quota".to_owned(),
                });
                continue;
            }
            let pool_revision = advance_pool(&tx)?;
            let record_id = format!("durable-{pool_revision:016x}");
            insert_migrated_record(&tx, target, &record_id, &record, pool_revision, now)?;
            usage = usage.saturating_add(record.size_bytes);
            outcome.copied.push(MigratedRecord {
                source_record_id: record.record_id,
                record_id,
                dataset_id: record.dataset_id,
                resource_id: record.resource_id,
                schema_id: record.schema_id,
                schema_version: record.schema_version,
                size_bytes: record.size_bytes,
            });
            outcome.pool_revision = pool_revision;
        }
        if outcome.pool_revision == 0 {
            outcome.pool_revision = current_pool_revision(&tx)?;
        }
        tx.commit()?;
        Ok(outcome)
    }

    /// Removes one migration's copies from the new publisher identity after a reviewed rollback.
    ///
    /// A copy is identified by the original provenance the migration preserved, so the new
    /// identity's own publications are never touched, and the old identity's rows are never
    /// touched either. A copy the user protected since the migration stays. Blocking: call this on
    /// the storage worker.
    pub fn revert_migrated(
        &mut self,
        target: &OwnerScope,
        source_publisher_fingerprint: &str,
        source_plugin_id: &str,
        now: i64,
    ) -> Result<MigrationRevertOutcome, SharedDataError> {
        validate_now(now)?;
        validate_text(source_publisher_fingerprint, MAX_IDENTIFIER_BYTES)?;
        validate_text(source_plugin_id, MAX_IDENTIFIER_BYTES)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        check_owner(&tx, target, now, false)?;
        let (records, bytes): (i64, i64) = tx.query_row(
            "SELECT COUNT(*), COALESCE(SUM(logical_bytes), 0) FROM shared_records
             WHERE owner_key = ?1 AND source_publisher_fingerprint = ?2
               AND source_plugin_id = ?3 AND user_protected = 0",
            params![target.key(), source_publisher_fingerprint, source_plugin_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let removed = tx.execute(
            "DELETE FROM shared_records
             WHERE owner_key = ?1 AND source_publisher_fingerprint = ?2
               AND source_plugin_id = ?3 AND user_protected = 0",
            params![target.key(), source_publisher_fingerprint, source_plugin_id],
        )? as u64;
        let kept: i64 = tx.query_row(
            "SELECT COUNT(*) FROM shared_records
             WHERE owner_key = ?1 AND source_publisher_fingerprint = ?2
               AND source_plugin_id = ?3 AND user_protected = 1",
            params![target.key(), source_publisher_fingerprint, source_plugin_id],
            |row| row.get(0),
        )?;
        let pool_revision = if removed > 0 {
            advance_pool(&tx)?
        } else {
            current_pool_revision(&tx)?
        };
        tx.commit()?;
        Ok(MigrationRevertOutcome {
            removed_records: records.max(0) as u64,
            reclaimed_bytes: bytes.max(0) as u64,
            kept_protected_records: kept.max(0) as u64,
            pool_revision,
        })
    }

    /// Makes every record of one owner inert for uninstall retention.
    ///
    /// Archived rows stay attributed to the original owner and on disk, but ordinary reads,
    /// lists and usage skip them, automatic retention never prunes them and a producer cannot
    /// replace an archived resource record. A later install needs the separate host reuse review.
    /// Blocking: call this on the storage worker.
    pub fn archive_owner(
        &mut self,
        owner: &OwnerScope,
        now: i64,
    ) -> Result<ArchiveOutcome, SharedDataError> {
        validate_now(now)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        check_owner(&tx, owner, now, false)?;
        let archived = tx.execute(
            "UPDATE shared_records SET archived = 1
             WHERE owner_key = ?1 AND archived = 0",
            [owner.key()],
        )? as u64;
        let pool_revision = if archived > 0 {
            advance_pool(&tx)?
        } else {
            current_pool_revision(&tx)?
        };
        tx.commit()?;
        Ok(ArchiveOutcome {
            records: archived,
            pool_revision,
        })
    }

    /// Restores one owner's archived records after an explicit host review.
    ///
    /// This is the only way retained records become visible again. It never touches another
    /// owner. Blocking: call this on the storage worker.
    pub fn reuse_archived_owner(
        &mut self,
        owner: &OwnerScope,
        now: i64,
    ) -> Result<ArchiveOutcome, SharedDataError> {
        validate_now(now)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        check_owner(&tx, owner, now, false)?;
        let reused = tx.execute(
            "UPDATE shared_records SET archived = 0
             WHERE owner_key = ?1 AND archived = 1",
            [owner.key()],
        )? as u64;
        let pool_revision = if reused > 0 {
            advance_pool(&tx)?
        } else {
            current_pool_revision(&tx)?
        };
        tx.commit()?;
        Ok(ArchiveOutcome {
            records: reused,
            pool_revision,
        })
    }

    /// Reclaims one owner's archived, unprotected rows in a reviewed host cleanup. A protected
    /// archived row stays. Blocking: call this on the storage worker.
    pub fn reclaim_archived_owner(
        &mut self,
        owner: &OwnerScope,
    ) -> Result<PruneOutcome, SharedDataError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        check_owner(&tx, owner, 0, false)?;
        let (records, bytes): (i64, i64) = tx.query_row(
            "SELECT COUNT(*), COALESCE(SUM(logical_bytes), 0) FROM shared_records
             WHERE owner_key = ?1 AND archived = 1 AND user_protected = 0",
            [owner.key()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let removed = tx.execute(
            "DELETE FROM shared_records
             WHERE owner_key = ?1 AND archived = 1 AND user_protected = 0",
            [owner.key()],
        )? as u64;
        let pool_revision = if removed > 0 {
            advance_pool(&tx)?
        } else {
            current_pool_revision(&tx)?
        };
        tx.commit()?;
        Ok(PruneOutcome {
            removed_records: records.max(0) as u64,
            reclaimed_bytes: bytes.max(0) as u64,
            pool_revision,
        })
    }

    /// Marks a record as protected from retention pruning, or releases it. Host-only: no plugin
    /// request reaches this method. It never changes the payload or the record revision.
    pub fn set_user_protected(
        &mut self,
        owner: &OwnerScope,
        record_id: &str,
        protected: bool,
        now: i64,
    ) -> Result<bool, SharedDataError> {
        validate_now(now)?;
        validate_text(record_id, 64)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        check_owner(&tx, owner, now, true)?;
        let changed = tx.execute(
            "UPDATE shared_records SET user_protected = ?3
             WHERE owner_key = ?1 AND record_id = ?2 AND user_protected != ?3",
            params![owner.key(), record_id, protected],
        )?;
        if changed > 0 {
            advance_pool(&tx)?;
        }
        tx.commit()?;
        Ok(changed > 0)
    }
}

#[derive(Clone, Debug)]
struct ExistingRecord {
    record_id: String,
    revision: u64,
    archived: bool,
}

#[derive(Clone, Debug)]
struct Victim {
    record_id: String,
    size_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct PruneProgress {
    records: u64,
    bytes: u64,
}

fn apply(tx: rusqlite::Transaction<'_>, migration: &Migration) -> rusqlite::Result<()> {
    tx.execute_batch(migration.sql)?;
    tx.pragma_update(None, "user_version", migration.version)?;
    tx.commit()
}

fn validate_write(write: &RecordWrite<'_>) -> Result<(), SharedDataError> {
    validate_now(write.now)?;
    validate_text(write.dataset_id, MAX_IDENTIFIER_BYTES)?;
    validate_text(write.schema_id, MAX_IDENTIFIER_BYTES)?;
    validate_text(write.class, MAX_CLASS_BYTES)?;
    // A producer observation id is optional without a sequence, matching the session broker.
    if !write.source_observation_id.is_empty() {
        validate_text(write.source_observation_id, MAX_IDENTIFIER_BYTES)?;
    }
    if let Some(resource_id) = write.resource_id {
        validate_text(resource_id, MAX_IDENTIFIER_BYTES)?;
    }
    if write.schema_version == 0 || write.category.is_some_and(|category| category < 0) {
        return Err(SharedDataError::InvalidRecord);
    }
    if write.observed_at < 0 {
        return Err(SharedDataError::InvalidRecord);
    }
    let structured = !write.fields_json.is_empty();
    let opaque = !write.opaque_payload.is_empty();
    if structured == opaque {
        return Err(SharedDataError::InvalidRecord);
    }
    if write
        .fields_json
        .len()
        .saturating_add(write.opaque_payload.len())
        > MAX_RECORD_BYTES
    {
        return Err(SharedDataError::RecordTooLarge);
    }
    if structured {
        let fields: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(write.fields_json).map_err(|_| SharedDataError::InvalidRecord)?;
        if fields.is_empty() {
            return Err(SharedDataError::InvalidRecord);
        }
    }
    if let Some(expires_at) = write.expires_at
        && (expires_at <= write.now
            || expires_at.saturating_sub(write.now) > MAX_TTL_SECONDS as i64)
    {
        return Err(SharedDataError::InvalidRecord);
    }
    Ok(())
}

fn validate_now(now: i64) -> Result<(), SharedDataError> {
    if now <= 0 {
        return Err(SharedDataError::InvalidRecord);
    }
    Ok(())
}

fn validate_text(value: &str, max_bytes: usize) -> Result<(), SharedDataError> {
    if value.is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        return Err(SharedDataError::InvalidRecord);
    }
    Ok(())
}

fn record_charge(write: &RecordWrite<'_>) -> Result<u64, SharedDataError> {
    if write.sequence > i64::MAX as u64 {
        return Err(SharedDataError::InvalidRecord);
    }
    let provenance = write.provenance;
    let bytes = write.dataset_id.len() as u64
        + write.class.len() as u64
        + write.schema_id.len() as u64
        + write.resource_id.map_or(0, |id| id.len() as u64)
        + write.source_observation_id.len() as u64
        + write.fields_json.len() as u64
        + write.opaque_payload.len() as u64
        + provenance.source_plugin_id.len() as u64
        + provenance.source_publisher_fingerprint.len() as u64
        + provenance.source_profile_id.len() as u64
        + provenance.declared_developer.len() as u64
        + provenance.origin.len() as u64
        + provenance.package_version.len() as u64
        + provenance.package_digest.len() as u64
        + RECORD_OVERHEAD_BYTES;
    Ok(bytes)
}

/// Checks the owner generation. A lower epoch is stale and fails closed; a higher epoch is the
/// current profile session and may advance the stored generation on a write.
fn check_owner(
    conn: &Connection,
    owner: &OwnerScope,
    now: i64,
    advance: bool,
) -> Result<(), SharedDataError> {
    let stored: Option<i64> = conn
        .query_row(
            "SELECT profile_epoch FROM shared_owners WHERE owner_key = ?1",
            [owner.key()],
            |row| row.get(0),
        )
        .optional()?;
    match stored {
        Some(stored) if owner.profile_epoch < stored as u64 => {
            Err(SharedDataError::StaleProfileEpoch)
        }
        Some(stored) if advance && owner.profile_epoch > stored as u64 => {
            conn.execute(
                "UPDATE shared_owners SET profile_epoch = ?2, updated_at = ?3
                 WHERE owner_key = ?1",
                params![owner.key(), owner.profile_epoch as i64, now],
            )?;
            Ok(())
        }
        Some(_) => Ok(()),
        None if advance => {
            conn.execute(
                "INSERT INTO shared_owners (owner_key, profile_epoch, updated_at)
                 VALUES (?1, ?2, ?3)",
                params![owner.key(), owner.profile_epoch as i64, now],
            )?;
            Ok(())
        }
        None => Ok(()),
    }
}

fn retention_policy(
    conn: &Connection,
    owner: &OwnerScope,
) -> Result<RetentionPolicy, SharedDataError> {
    let row: Option<(Option<i64>, Option<i64>)> = conn
        .query_row(
            "SELECT max_age_seconds, max_bytes FROM shared_retention WHERE owner_key = ?1",
            [owner.key()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let (max_age_seconds, max_bytes) = row.unwrap_or((None, None));
    Ok(RetentionPolicy {
        max_age_seconds: max_age_seconds.and_then(|age| u64::try_from(age).ok()),
        max_bytes: max_bytes.and_then(|bytes| u64::try_from(bytes).ok()),
    })
}

fn effective_max_bytes(policy: RetentionPolicy) -> u64 {
    policy
        .max_bytes
        .unwrap_or(MAX_OWNER_LOGICAL_BYTES)
        .min(MAX_OWNER_LOGICAL_BYTES)
}

fn owner_usage(
    conn: &Connection,
    owner: &OwnerScope,
    now: i64,
) -> Result<OwnerUsage, SharedDataError> {
    let policy = retention_policy(conn, owner)?;
    let (records, logical_bytes, oldest_updated_at, protected_records): (
        i64,
        i64,
        Option<i64>,
        i64,
    ) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(logical_bytes), 0), MIN(updated_at),
                COALESCE(SUM(user_protected), 0)
         FROM shared_records
         WHERE owner_key = ?1 AND archived = 0
           AND (expires_at IS NULL OR expires_at > ?2)",
        params![owner.key(), now],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    let (archived_records, archived_logical_bytes): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(logical_bytes), 0)
         FROM shared_records
         WHERE owner_key = ?1 AND archived = 1
           AND (expires_at IS NULL OR expires_at > ?2)",
        params![owner.key(), now],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let page_count: i64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0))?;
    let page_size: i64 = conn.query_row("PRAGMA page_size", [], |row| row.get(0))?;
    Ok(OwnerUsage {
        records: records.max(0) as u64,
        logical_bytes: logical_bytes.max(0) as u64,
        max_bytes: effective_max_bytes(policy),
        max_age_seconds: policy.max_age_seconds,
        oldest_updated_at,
        archived_records: archived_records.max(0) as u64,
        archived_logical_bytes: archived_logical_bytes.max(0) as u64,
        protected_records: protected_records.max(0) as u64,
        physical_bytes_estimate: page_count.max(0) as u64 * page_size.max(0) as u64,
    })
}

fn usage_excluding(
    conn: &Connection,
    owner: &OwnerScope,
    now: i64,
    except: Option<&str>,
) -> Result<u64, SharedDataError> {
    let bytes: i64 = conn.query_row(
        "SELECT COALESCE(SUM(logical_bytes), 0) FROM shared_records
         WHERE owner_key = ?1 AND archived = 0
           AND (expires_at IS NULL OR expires_at > ?2)
           AND (?3 IS NULL OR record_id != ?3)",
        params![owner.key(), now, except],
        |row| row.get(0),
    )?;
    Ok(bytes.max(0) as u64)
}

fn prune_expired(
    conn: &Connection,
    owner: &OwnerScope,
    now: i64,
    pruned: &mut PruneProgress,
) -> Result<(), SharedDataError> {
    let limit = MAX_PRUNE_ROWS.saturating_sub(pruned.records);
    if limit == 0 {
        return Ok(());
    }
    let victims = victims(
        conn,
        "SELECT record_id, logical_bytes FROM shared_records
         WHERE owner_key = ?1 AND user_protected = 0 AND archived = 0
           AND expires_at IS NOT NULL AND expires_at <= ?2
         ORDER BY expires_at ASC, updated_at ASC, record_id ASC LIMIT ?3",
        params![owner.key(), now, limit as i64],
    )?;
    delete_victims(conn, victims, pruned)
}

fn prune_aged(
    conn: &Connection,
    owner: &OwnerScope,
    now: i64,
    pruned: &mut PruneProgress,
) -> Result<(), SharedDataError> {
    let limit = MAX_PRUNE_ROWS.saturating_sub(pruned.records);
    if limit == 0 {
        return Ok(());
    }
    let Some(max_age) = retention_policy(conn, owner)?.max_age_seconds else {
        return Ok(());
    };
    let cutoff = now.saturating_sub(max_age.min(i64::MAX as u64) as i64);
    let victims = victims(
        conn,
        "SELECT record_id, logical_bytes FROM shared_records
         WHERE owner_key = ?1 AND user_protected = 0 AND archived = 0 AND updated_at <= ?2
         ORDER BY updated_at ASC, received_at ASC, record_id ASC LIMIT ?3",
        params![owner.key(), cutoff, limit as i64],
    )?;
    delete_victims(conn, victims, pruned)
}

fn oldest_unprotected(
    conn: &Connection,
    owner: &OwnerScope,
    now: i64,
    except: Option<&str>,
) -> Result<Option<Victim>, SharedDataError> {
    Ok(conn
        .query_row(
            "SELECT record_id, logical_bytes FROM shared_records
             WHERE owner_key = ?1 AND user_protected = 0 AND archived = 0
               AND (expires_at IS NULL OR expires_at > ?2)
               AND (?3 IS NULL OR record_id != ?3)
             ORDER BY updated_at ASC, received_at ASC, record_id ASC LIMIT 1",
            params![owner.key(), now, except],
            |row| {
                Ok(Victim {
                    record_id: row.get(0)?,
                    size_bytes: row.get::<_, i64>(1)?.max(0) as u64,
                })
            },
        )
        .optional()?)
}

fn prune_owner(
    conn: &Connection,
    owner: &OwnerScope,
    now: i64,
) -> Result<(PruneProgress, bool), SharedDataError> {
    let mut pruned = PruneProgress::default();
    prune_expired(conn, owner, now, &mut pruned)?;
    prune_aged(conn, owner, now, &mut pruned)?;
    let max_bytes = effective_max_bytes(retention_policy(conn, owner)?);
    let mut usage = usage_excluding(conn, owner, now, None)?;
    while usage > max_bytes && pruned.records < MAX_PRUNE_ROWS {
        let Some(victim) = oldest_unprotected(conn, owner, now, None)? else {
            break;
        };
        delete_record(conn, &victim)?;
        pruned.records += 1;
        pruned.bytes = pruned.bytes.saturating_add(victim.size_bytes);
        usage = usage.saturating_sub(victim.size_bytes);
    }
    Ok((pruned, pruned.records > 0))
}

fn victims(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<Victim>, SharedDataError> {
    let mut statement = conn.prepare(sql)?;
    let rows = statement.query_map(params, |row| {
        Ok(Victim {
            record_id: row.get(0)?,
            size_bytes: row.get::<_, i64>(1)?.max(0) as u64,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn delete_victims(
    conn: &Connection,
    victims: Vec<Victim>,
    pruned: &mut PruneProgress,
) -> Result<(), SharedDataError> {
    for victim in victims {
        delete_record(conn, &victim)?;
        pruned.records += 1;
        pruned.bytes = pruned.bytes.saturating_add(victim.size_bytes);
    }
    Ok(())
}

fn delete_record(conn: &Connection, victim: &Victim) -> Result<(), SharedDataError> {
    conn.execute(
        "DELETE FROM shared_records WHERE record_id = ?1",
        [&victim.record_id],
    )?;
    Ok(())
}

fn current_pool_revision(conn: &Connection) -> Result<u64, SharedDataError> {
    let revision: i64 = conn.query_row(
        "SELECT revision FROM shared_state WHERE id = 1",
        [],
        |row| row.get(0),
    )?;
    Ok(revision.max(0) as u64)
}

fn advance_pool(conn: &Connection) -> Result<u64, SharedDataError> {
    conn.execute(
        "UPDATE shared_state SET revision = revision + 1 WHERE id = 1",
        [],
    )?;
    current_pool_revision(conn)
}

fn record_by_id(
    conn: &Connection,
    owner: &OwnerScope,
    record_id: &str,
    now: i64,
) -> Result<Option<SharedRecord>, SharedDataError> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {RECORD_COLUMNS} FROM shared_records
                 WHERE owner_key = ?1 AND record_id = ?2 AND archived = 0
                   AND (expires_at IS NULL OR expires_at > ?3)"
            ),
            params![owner.key(), record_id, now],
            record_from,
        )
        .optional()?)
}

fn metadata_from(record: SharedRecord) -> RecordMetadata {
    RecordMetadata {
        record_id: record.record_id,
        dataset_id: record.dataset_id,
        resource_id: record.resource_id,
        class: record.class,
        schema_id: record.schema_id,
        schema_version: record.schema_version,
        observed_at: record.observed_at,
        received_at: record.received_at,
        updated_at: record.updated_at,
        expires_at: record.expires_at,
        sequence: record.sequence,
        size_bytes: record.size_bytes,
        user_protected: record.user_protected,
    }
}

fn records_for_owner(
    conn: &Connection,
    owner: &OwnerScope,
    now: i64,
) -> Result<Vec<SharedRecord>, SharedDataError> {
    let mut statement = conn.prepare(&format!(
        "SELECT {RECORD_COLUMNS} FROM shared_records
         WHERE owner_key = ?1 AND archived = 0
           AND (expires_at IS NULL OR expires_at > ?2)
         ORDER BY dataset_id ASC, received_at ASC, record_id ASC"
    ))?;
    let records = statement
        .query_map(params![owner.key(), now], record_from)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(records)
}

fn resource_exists(
    conn: &Connection,
    target: &OwnerScope,
    record: &SharedRecord,
) -> Result<bool, SharedDataError> {
    let Some(resource_id) = record.resource_id.as_deref() else {
        return Ok(false);
    };
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM shared_records
         WHERE owner_key = ?1 AND dataset_id = ?2 AND resource_id = ?3)",
        params![target.key(), record.dataset_id, resource_id],
        |row| row.get(0),
    )?)
}

/// Inserts one copied record under the new owner with a new host record id, its own initial
/// revision and the original provenance and observation metadata.
fn insert_migrated_record(
    conn: &Connection,
    target: &OwnerScope,
    record_id: &str,
    record: &SharedRecord,
    pool_revision: u64,
    now: i64,
) -> Result<(), SharedDataError> {
    conn.execute(
        "INSERT INTO shared_records
             (record_id, owner_key, dataset_id, resource_id, category, class,
              schema_id, schema_version, fields_json, opaque_payload,
              source_plugin_id, source_publisher_fingerprint, source_profile_id,
              declared_developer, origin, package_version, package_digest,
              observed_at, received_at, updated_at, expires_at, sequence,
              source_observation_id, revision, pool_revision, logical_bytes,
              user_protected, archived)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                 ?17, ?18, ?19, ?19, ?20, ?21, ?22, 1, ?23, ?24, 0, 0)",
        params![
            record_id,
            target.key(),
            record.dataset_id,
            record.resource_id,
            record.category,
            record.class,
            record.schema_id,
            i64::from(record.schema_version),
            record.fields_json,
            record.opaque_payload,
            record.provenance.source_plugin_id,
            record.provenance.source_publisher_fingerprint,
            record.provenance.source_profile_id,
            record.provenance.declared_developer,
            record.provenance.origin,
            record.provenance.package_version,
            record.provenance.package_digest,
            record.observed_at,
            now,
            record.expires_at,
            record.sequence as i64,
            record.source_observation_id,
            pool_revision as i64,
            record.size_bytes as i64,
        ],
    )?;
    Ok(())
}

fn record_from(row: &rusqlite::Row<'_>) -> rusqlite::Result<SharedRecord> {
    Ok(SharedRecord {
        record_id: row.get(0)?,
        dataset_id: row.get(1)?,
        category: row.get(2)?,
        class: row.get(3)?,
        schema_id: row.get(4)?,
        schema_version: row.get::<_, i64>(5)?.max(0) as u32,
        fields_json: row.get(6)?,
        opaque_payload: row.get(7)?,
        resource_id: row.get(8)?,
        observed_at: row.get(9)?,
        received_at: row.get(10)?,
        updated_at: row.get(11)?,
        expires_at: row.get(12)?,
        sequence: row.get::<_, i64>(13)?.max(0) as u64,
        source_observation_id: row.get(14)?,
        revision: row.get::<_, i64>(15)?.max(0) as u64,
        pool_revision: row.get::<_, i64>(16)?.max(0) as u64,
        size_bytes: row.get::<_, i64>(17)?.max(0) as u64,
        user_protected: row.get::<_, i64>(18)? != 0,
        archived: row.get::<_, i64>(26)? != 0,
        provenance: RecordProvenance {
            source_plugin_id: row.get(19)?,
            source_publisher_fingerprint: row.get(20)?,
            source_profile_id: row.get(21)?,
            declared_developer: row.get(22)?,
            origin: row.get(23)?,
            package_version: row.get(24)?,
            package_digest: row.get(25)?,
        },
    })
}

#[cfg(test)]
mod tests;
