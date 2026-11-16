//! Bounded persistence for plugin-produced hardware, diagnostic, presence, and
//! performance records.
//!
//! This module is a storage boundary, not a collector: it never launches a
//! benchmark, SMART utility, game process, or instrumentation hook. The host
//! must construct [`TelemetryOwner`] from the authenticated plugin session and
//! perform consent, field projection, and schema validation before publishing
//! or returning a record. All methods are synchronous SQLite operations and
//! must be called from a blocking worker.

use std::fs;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

/// Maximum serialized payload size accepted for an individual telemetry record.
pub const MAX_RECORD_BYTES: usize = 1024 * 1024;
/// Maximum retained logical payload bytes for one plugin/profile owner.
pub const OWNER_QUOTA_BYTES: u64 = 16 * 1024 * 1024;
/// Maximum number of records returned from one page.
pub const MAX_PAGE_SIZE: usize = 256;

const SCHEMA_VERSION: i64 = 1;

/// Host-authenticated attribution for a producer. Construct this from the
/// resolved package/session identity; never copy it from a plugin payload.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TelemetryOwner {
    pub plugin_id: String,
    pub publisher_fingerprint: String,
    pub package_version: String,
    pub profile_id: String,
}

impl TelemetryOwner {
    fn validate(&self) -> Result<(), TelemetryError> {
        for (name, value, limit) in [
            ("plugin id", self.plugin_id.as_str(), 256),
            (
                "publisher fingerprint",
                self.publisher_fingerprint.as_str(),
                256,
            ),
            ("package version", self.package_version.as_str(), 128),
            ("profile id", self.profile_id.as_str(), 256),
        ] {
            if value.is_empty() || value.len() > limit || value.chars().any(char::is_control) {
                return Err(TelemetryError::InvalidIdentity(name));
            }
        }
        Ok(())
    }
}

/// A persisted producer family. The family and all attribution fields are
/// separate from the extension payload and cannot be shadowed by it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryFamily {
    StorageDiagnostic,
    Computer,
    GamePresence,
    GamePerformance,
}

impl TelemetryFamily {
    fn as_str(self) -> &'static str {
        match self {
            Self::StorageDiagnostic => "storage_diagnostic",
            Self::Computer => "computer",
            Self::GamePresence => "game_presence",
            Self::GamePerformance => "game_performance",
        }
    }
}

/// One host-attributed record. `resource_id` identifies the volume, computer,
/// game/session, or performance series according to `family`; its meaning is
/// interpreted by the host API layer. `sequence` is optional for families that
/// do not publish ordered batches.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TelemetryRecord {
    pub record_id: String,
    pub family: TelemetryFamily,
    pub resource_id: String,
    pub schema_id: String,
    pub schema_version: u32,
    pub observed_at: i64,
    pub expires_at: Option<i64>,
    pub sequence: Option<u64>,
    pub payload: serde_json::Value,
}

/// A record after loading from the host-owned store.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredTelemetryRecord {
    pub record: TelemetryRecord,
    pub owner: TelemetryOwner,
    pub revision: u64,
    pub payload_bytes: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    #[error("invalid telemetry identity: {0}")]
    InvalidIdentity(&'static str),
    #[error("invalid telemetry record: {0}")]
    InvalidRecord(&'static str),
    #[error("telemetry record exceeds the per-record size limit")]
    RecordTooLarge,
    #[error("telemetry owner quota is exhausted")]
    QuotaExceeded,
    #[error("telemetry sequence is duplicate or out of order")]
    SequenceConflict,
    #[error("telemetry record does not exist")]
    NotFound,
    #[error("telemetry database failed")]
    Database(#[from] rusqlite::Error),
    #[error("telemetry database could not be opened")]
    Open(#[source] std::io::Error),
    #[error("telemetry database was created by a newer version of GameNode")]
    NewerVersion,
    #[error("telemetry record encoding failed")]
    Encoding(#[from] serde_json::Error),
}

/// SQLite-backed records produced by third-party plugins.
///
/// The database owns only plugin-published data. Callers remain responsible
/// for authorizing the producer and each consumer before calling these methods.
pub struct PluginTelemetryStore {
    connection: Mutex<Connection>,
}

impl PluginTelemetryStore {
    /// Opens or creates a store at `path`. Filesystem and SQLite work are
    /// blocking and should be invoked from `spawn_blocking` or the host's
    /// equivalent worker.
    pub fn open(path: &Path) -> Result<Self, TelemetryError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(TelemetryError::Open)?;
        }
        let connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(TelemetryError::NewerVersion);
        }
        if version < 1 {
            connection.execute_batch(
                "BEGIN;
                 CREATE TABLE telemetry_records (
                     record_id TEXT PRIMARY KEY,
                     family TEXT NOT NULL,
                     resource_id TEXT NOT NULL,
                     plugin_id TEXT NOT NULL,
                     publisher_fingerprint TEXT NOT NULL,
                     package_version TEXT NOT NULL,
                     profile_id TEXT NOT NULL,
                     schema_id TEXT NOT NULL,
                     schema_version INTEGER NOT NULL CHECK (schema_version >= 0),
                     observed_at INTEGER NOT NULL,
                     expires_at INTEGER,
                     sequence INTEGER,
                     revision INTEGER NOT NULL CHECK (revision >= 1),
                     payload BLOB NOT NULL,
                     payload_bytes INTEGER NOT NULL CHECK (payload_bytes >= 0),
                     CHECK (expires_at IS NULL OR expires_at > observed_at)
                 ) STRICT;
                 CREATE INDEX telemetry_owner_records
                     ON telemetry_records(plugin_id, publisher_fingerprint, profile_id, revision);
                 CREATE INDEX telemetry_family_resource
                     ON telemetry_records(family, resource_id, revision);
                 CREATE UNIQUE INDEX telemetry_sequence
                     ON telemetry_records(plugin_id, publisher_fingerprint, profile_id,
                                          family, resource_id, sequence)
                     WHERE sequence IS NOT NULL;
                 CREATE TABLE telemetry_revisions (
                     plugin_id TEXT NOT NULL,
                     publisher_fingerprint TEXT NOT NULL,
                     profile_id TEXT NOT NULL,
                     revision INTEGER NOT NULL CHECK (revision >= 0),
                     PRIMARY KEY (plugin_id, publisher_fingerprint, profile_id)
                 ) STRICT;
                 PRAGMA user_version = 1;
                 COMMIT;",
            )?;
        }
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    /// Saves a host-attributed record, assigning its monotonically increasing
    /// revision within the producer's plugin/publisher/profile namespace.
    /// Expired records are reclaimed before quota accounting. Publication is
    /// append-only; callers use a new host-generated record ID for each update.
    pub fn publish(
        &self,
        owner: &TelemetryOwner,
        record: &TelemetryRecord,
        now: i64,
    ) -> Result<StoredTelemetryRecord, TelemetryError> {
        owner.validate()?;
        validate_record(record, now)?;
        let payload = serde_json::to_vec(&record.payload)?;
        if payload.len() > MAX_RECORD_BYTES {
            return Err(TelemetryError::RecordTooLarge);
        }
        let mut connection = self.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "DELETE FROM telemetry_records WHERE expires_at IS NOT NULL AND expires_at <= ?1",
            [now],
        )?;
        let used: i64 = transaction.query_row(
            "SELECT COALESCE(SUM(payload_bytes), 0) FROM telemetry_records
             WHERE plugin_id = ?1 AND publisher_fingerprint = ?2 AND profile_id = ?3",
            params![
                owner.plugin_id,
                owner.publisher_fingerprint,
                owner.profile_id
            ],
            |row| row.get(0),
        )?;
        let used =
            u64::try_from(used).map_err(|_| TelemetryError::InvalidRecord("stored byte count"))?;
        let payload_bytes =
            i64::try_from(payload.len()).map_err(|_| TelemetryError::RecordTooLarge)?;
        let projected = used.saturating_add(payload.len() as u64);
        if projected > OWNER_QUOTA_BYTES {
            return Err(TelemetryError::QuotaExceeded);
        }
        if let Some(sequence) = record.sequence {
            let latest: Option<i64> = transaction
                .query_row(
                    "SELECT MAX(sequence) FROM telemetry_records
                     WHERE plugin_id = ?1 AND publisher_fingerprint = ?2 AND profile_id = ?3
                       AND family = ?4 AND resource_id = ?5",
                    params![
                        owner.plugin_id,
                        owner.publisher_fingerprint,
                        owner.profile_id,
                        record.family.as_str(),
                        record.resource_id
                    ],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();
            if latest
                .map(u64::try_from)
                .transpose()
                .map_err(|_| TelemetryError::InvalidRecord("stored sequence"))?
                .is_some_and(|current| sequence <= current)
            {
                return Err(TelemetryError::SequenceConflict);
            }
        }
        let revision = next_revision(&transaction, owner)?;
        transaction.execute(
            "INSERT INTO telemetry_records
             (record_id, family, resource_id, plugin_id, publisher_fingerprint,
              package_version, profile_id, schema_id, schema_version, observed_at,
              expires_at, sequence, revision, payload, payload_bytes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                record.record_id,
                record.family.as_str(),
                record.resource_id,
                owner.plugin_id,
                owner.publisher_fingerprint,
                owner.package_version,
                owner.profile_id,
                record.schema_id,
                i64::from(record.schema_version),
                record.observed_at,
                record.expires_at,
                record
                    .sequence
                    .map(i64::try_from)
                    .transpose()
                    .map_err(|_| TelemetryError::InvalidRecord("sequence exceeds SQLite range"))?,
                i64::try_from(revision)
                    .map_err(|_| TelemetryError::InvalidRecord("revision exceeds SQLite range"))?,
                payload,
                payload_bytes,
            ],
        )?;
        transaction.commit()?;
        Ok(StoredTelemetryRecord {
            record: record.clone(),
            owner: owner.clone(),
            revision,
            payload_bytes: payload.len() as u64,
        })
    }

    /// Loads one record by its host-assigned ID. The caller must still apply
    /// consumer authorization and projections before returning it.
    pub fn get(
        &self,
        record_id: &str,
        now: i64,
    ) -> Result<Option<StoredTelemetryRecord>, TelemetryError> {
        if record_id.is_empty() || record_id.len() > 256 {
            return Err(TelemetryError::InvalidRecord("record id"));
        }
        let connection = self.lock();
        let row = connection
            .query_row(
                "SELECT record_id, family, resource_id, plugin_id, publisher_fingerprint,
                        package_version, profile_id, schema_id, schema_version, observed_at,
                        expires_at, sequence, revision, payload, payload_bytes
                 FROM telemetry_records WHERE record_id = ?1
                   AND (expires_at IS NULL OR expires_at > ?2)",
                params![record_id, now],
                stored_record_from_row,
            )
            .optional()?;
        Ok(row)
    }

    /// Lists a bounded page for a family and optional resource. `owner` is an
    /// optional host-resolved source filter; `None` means all sources and must
    /// only be used after the host has established a matching read grant.
    pub fn list(
        &self,
        family: TelemetryFamily,
        resource_id: Option<&str>,
        owner: Option<&TelemetryOwner>,
        after_revision: u64,
        page_size: usize,
        now: i64,
    ) -> Result<Vec<StoredTelemetryRecord>, TelemetryError> {
        if let Some(owner) = owner {
            owner.validate()?;
        }
        if resource_id.is_some_and(|id| id.len() > 256 || id.chars().any(char::is_control)) {
            return Err(TelemetryError::InvalidRecord("resource id"));
        }
        let page_size = page_size.clamp(1, MAX_PAGE_SIZE);
        let connection = self.lock();
        let mut statement = connection.prepare(
            "SELECT record_id, family, resource_id, plugin_id, publisher_fingerprint,
                    package_version, profile_id, schema_id, schema_version, observed_at,
                    expires_at, sequence, revision, payload, payload_bytes
             FROM telemetry_records
             WHERE family = ?1 AND revision > ?2
               AND (?3 IS NULL OR resource_id = ?3)
               AND (?4 IS NULL OR plugin_id = ?4)
               AND (?5 IS NULL OR publisher_fingerprint = ?5)
               AND (?6 IS NULL OR profile_id = ?6)
               AND (expires_at IS NULL OR expires_at > ?7)
             ORDER BY revision ASC LIMIT ?8",
        )?;
        let rows = statement.query_map(
            params![
                family.as_str(),
                i64::try_from(after_revision)
                    .map_err(|_| TelemetryError::InvalidRecord("cursor exceeds SQLite range"))?,
                resource_id,
                owner.map(|value| value.plugin_id.as_str()),
                owner.map(|value| value.publisher_fingerprint.as_str()),
                owner.map(|value| value.profile_id.as_str()),
                now,
                i64::try_from(page_size)
                    .map_err(|_| TelemetryError::InvalidRecord("page size exceeds SQLite range"))?,
            ],
            stored_record_from_row,
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Removes expired records and returns the number of rows reclaimed.
    pub fn expire(&self, now: i64) -> Result<u64, TelemetryError> {
        let connection = self.lock();
        let count = connection.execute(
            "DELETE FROM telemetry_records WHERE expires_at IS NOT NULL AND expires_at <= ?1",
            [now],
        )?;
        Ok(count as u64)
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn validate_record(record: &TelemetryRecord, now: i64) -> Result<(), TelemetryError> {
    for (name, value, limit) in [
        ("record id", record.record_id.as_str(), 256),
        ("resource id", record.resource_id.as_str(), 256),
        ("schema id", record.schema_id.as_str(), 256),
    ] {
        if value.is_empty() || value.len() > limit || value.chars().any(char::is_control) {
            return Err(TelemetryError::InvalidRecord(name));
        }
    }
    if record.observed_at <= 0 || record.observed_at > now.saturating_add(300) {
        return Err(TelemetryError::InvalidRecord("observation time"));
    }
    if record
        .expires_at
        .is_some_and(|expiry| expiry <= now || expiry <= record.observed_at)
    {
        return Err(TelemetryError::InvalidRecord("expiry"));
    }
    if !record.payload.is_object() {
        return Err(TelemetryError::InvalidRecord("payload must be an object"));
    }
    Ok(())
}

fn next_revision(
    transaction: &rusqlite::Transaction<'_>,
    owner: &TelemetryOwner,
) -> Result<u64, TelemetryError> {
    transaction.execute(
        "INSERT OR IGNORE INTO telemetry_revisions
         (plugin_id, publisher_fingerprint, profile_id, revision) VALUES (?1, ?2, ?3, 0)",
        params![
            owner.plugin_id,
            owner.publisher_fingerprint,
            owner.profile_id
        ],
    )?;
    transaction.execute(
        "UPDATE telemetry_revisions SET revision = revision + 1
         WHERE plugin_id = ?1 AND publisher_fingerprint = ?2 AND profile_id = ?3",
        params![
            owner.plugin_id,
            owner.publisher_fingerprint,
            owner.profile_id
        ],
    )?;
    let revision: i64 = transaction.query_row(
        "SELECT revision FROM telemetry_revisions
         WHERE plugin_id = ?1 AND publisher_fingerprint = ?2 AND profile_id = ?3",
        params![
            owner.plugin_id,
            owner.publisher_fingerprint,
            owner.profile_id
        ],
        |row| row.get(0),
    )?;
    u64::try_from(revision).map_err(|_| TelemetryError::InvalidRecord("stored revision"))
}

fn stored_record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredTelemetryRecord> {
    let family: String = row.get(1)?;
    let family = match family.as_str() {
        "storage_diagnostic" => TelemetryFamily::StorageDiagnostic,
        "computer" => TelemetryFamily::Computer,
        "game_presence" => TelemetryFamily::GamePresence,
        "game_performance" => TelemetryFamily::GamePerformance,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    let payload: Vec<u8> = row.get(13)?;
    let payload = serde_json::from_slice(&payload).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(13, rusqlite::types::Type::Blob, Box::new(error))
    })?;
    let sequence: Option<i64> = row.get(11)?;
    let revision: i64 = row.get(12)?;
    let payload_bytes: i64 = row.get(14)?;
    let sequence = sequence.map(u64::try_from).transpose().map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            11,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })?;
    let revision = u64::try_from(revision).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            12,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })?;
    let payload_bytes = u64::try_from(payload_bytes).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            14,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })?;
    Ok(StoredTelemetryRecord {
        record: TelemetryRecord {
            record_id: row.get(0)?,
            family,
            resource_id: row.get(2)?,
            schema_id: row.get(7)?,
            schema_version: row.get(8)?,
            observed_at: row.get(9)?,
            expires_at: row.get(10)?,
            sequence,
            payload,
        },
        owner: TelemetryOwner {
            plugin_id: row.get(3)?,
            publisher_fingerprint: row.get(4)?,
            package_version: row.get(5)?,
            profile_id: row.get(6)?,
        },
        revision,
        payload_bytes,
    })
}
