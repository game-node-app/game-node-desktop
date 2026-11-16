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
use sha2::Digest;

/// Maximum serialized payload size accepted for an individual telemetry record.
pub const MAX_RECORD_BYTES: usize = 1024 * 1024;
/// Maximum retained logical payload bytes for one plugin/profile owner.
pub const OWNER_QUOTA_BYTES: u64 = 16 * 1024 * 1024;
/// Maximum retained record count per authenticated plugin/profile owner.
pub const OWNER_QUOTA_RECORDS: u64 = 16_384;
/// Maximum number of records returned from one page.
pub const MAX_PAGE_SIZE: usize = 256;

const SCHEMA_VERSION: i64 = 2;

/// Host-authenticated attribution for a producer. Construct this from the
/// resolved package/session identity; never copy it from a plugin payload.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TelemetryOwner {
    /// Authenticated producer/source plugin id.
    pub plugin_id: String,
    /// Developer/name declared in the integrity-signed package manifest.
    pub developer: String,
    /// Origin declared in package metadata (or a host-defined local-import origin).
    pub origin: String,
    /// Host-known installation path, such as manual import or signed update feed.
    pub install_source: String,
    pub publisher_fingerprint: String,
    pub package_version: String,
    /// SHA-256 digest of the exact installed package payload.
    pub package_digest: String,
    pub profile_id: String,
}

impl TelemetryOwner {
    fn validate(&self) -> Result<(), TelemetryError> {
        for (name, value, limit) in [
            ("plugin id", self.plugin_id.as_str(), 256),
            ("developer", self.developer.as_str(), 256),
            ("origin", self.origin.as_str(), 2048),
            ("install source", self.install_source.as_str(), 128),
            (
                "publisher fingerprint",
                self.publisher_fingerprint.as_str(),
                256,
            ),
            ("package version", self.package_version.as_str(), 128),
            ("package digest", self.package_digest.as_str(), 64),
            ("profile id", self.profile_id.as_str(), 256),
        ] {
            if value.is_empty() || value.len() > limit || value.chars().any(char::is_control) {
                return Err(TelemetryError::InvalidIdentity(name));
            }
        }
        if self.package_digest.len() != 64
            || !self
                .package_digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(TelemetryError::InvalidIdentity("package digest"));
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
    /// Host-generated, globally namespaced record identifier.
    pub record_id: String,
    pub record: TelemetryRecord,
    pub owner: TelemetryOwner,
    /// Host receipt time in Unix seconds.
    pub received_at: i64,
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
                     developer TEXT NOT NULL,
                     origin TEXT NOT NULL,
                     install_source TEXT NOT NULL,
                     publisher_fingerprint TEXT NOT NULL,
                     package_version TEXT NOT NULL,
                     package_digest TEXT NOT NULL,
                     profile_id TEXT NOT NULL,
                     schema_id TEXT NOT NULL,
                     schema_version INTEGER NOT NULL CHECK (schema_version >= 0),
                     observed_at INTEGER NOT NULL,
                     expires_at INTEGER,
                     sequence INTEGER,
                     revision INTEGER NOT NULL CHECK (revision >= 1),
                     received_at INTEGER NOT NULL,
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
                 CREATE TABLE telemetry_sequences (
                     plugin_id TEXT NOT NULL,
                     publisher_fingerprint TEXT NOT NULL,
                     profile_id TEXT NOT NULL,
                     family TEXT NOT NULL,
                     resource_id TEXT NOT NULL,
                     sequence INTEGER NOT NULL CHECK (sequence >= 0),
                     PRIMARY KEY (plugin_id, publisher_fingerprint, profile_id,
                                  family, resource_id)
                 ) STRICT;
                 PRAGMA user_version = 2;
                 COMMIT;",
            )?;
        } else if version < 2 {
            connection.execute_batch(
                "BEGIN IMMEDIATE;
                 ALTER TABLE telemetry_records ADD COLUMN developer TEXT NOT NULL DEFAULT '';
                 ALTER TABLE telemetry_records ADD COLUMN origin TEXT NOT NULL DEFAULT '';
                 ALTER TABLE telemetry_records ADD COLUMN install_source TEXT NOT NULL DEFAULT '';
                 ALTER TABLE telemetry_records ADD COLUMN package_digest TEXT NOT NULL DEFAULT '';
                 ALTER TABLE telemetry_records ADD COLUMN received_at INTEGER NOT NULL DEFAULT 0;
                 CREATE TABLE telemetry_sequences (
                     plugin_id TEXT NOT NULL,
                     publisher_fingerprint TEXT NOT NULL,
                     profile_id TEXT NOT NULL,
                     family TEXT NOT NULL,
                     resource_id TEXT NOT NULL,
                     sequence INTEGER NOT NULL CHECK (sequence >= 0),
                     PRIMARY KEY (plugin_id, publisher_fingerprint, profile_id,
                                  family, resource_id)
                 ) STRICT;
                 INSERT INTO telemetry_sequences
                     (plugin_id, publisher_fingerprint, profile_id, family, resource_id, sequence)
                 SELECT plugin_id, publisher_fingerprint, profile_id, family, resource_id,
                        MAX(sequence)
                 FROM telemetry_records WHERE sequence IS NOT NULL
                 GROUP BY plugin_id, publisher_fingerprint, profile_id, family, resource_id;
                 PRAGMA user_version = 2;
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
        let count: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM telemetry_records
             WHERE plugin_id = ?1 AND publisher_fingerprint = ?2 AND profile_id = ?3",
            params![
                owner.plugin_id,
                owner.publisher_fingerprint,
                owner.profile_id
            ],
            |row| row.get(0),
        )?;
        if u64::try_from(count).map_err(|_| TelemetryError::InvalidRecord("stored record count"))?
            >= OWNER_QUOTA_RECORDS
        {
            return Err(TelemetryError::QuotaExceeded);
        }
        let payload_bytes =
            i64::try_from(payload.len()).map_err(|_| TelemetryError::RecordTooLarge)?;
        let projected = used.saturating_add(payload.len() as u64);
        if projected > OWNER_QUOTA_BYTES {
            return Err(TelemetryError::QuotaExceeded);
        }
        if let Some(sequence) = record.sequence {
            let latest: Option<i64> = transaction
                .query_row(
                    "SELECT sequence FROM telemetry_sequences
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
                .optional()?;
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
        let record_id = host_record_id(owner, revision);
        transaction.execute(
            "INSERT INTO telemetry_records
             (record_id, family, resource_id, plugin_id, developer, origin, install_source,
              publisher_fingerprint, package_version, package_digest, profile_id, schema_id,
              schema_version, observed_at, expires_at, sequence, revision, received_at,
              payload, payload_bytes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                     ?15, ?16, ?17, ?18, ?19, ?20)",
            params![
                record_id,
                record.family.as_str(),
                record.resource_id,
                owner.plugin_id,
                owner.developer,
                owner.origin,
                owner.install_source,
                owner.publisher_fingerprint,
                owner.package_version,
                owner.package_digest,
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
                now,
                payload,
                payload_bytes,
            ],
        )?;
        if let Some(sequence) = record.sequence {
            transaction.execute(
                "INSERT INTO telemetry_sequences
                 (plugin_id, publisher_fingerprint, profile_id, family, resource_id, sequence)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(plugin_id, publisher_fingerprint, profile_id, family, resource_id)
                 DO UPDATE SET sequence = excluded.sequence",
                params![
                    owner.plugin_id,
                    owner.publisher_fingerprint,
                    owner.profile_id,
                    record.family.as_str(),
                    record.resource_id,
                    i64::try_from(sequence).map_err(|_| {
                        TelemetryError::InvalidRecord("sequence exceeds SQLite range")
                    })?,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(StoredTelemetryRecord {
            record_id,
            record: record.clone(),
            owner: owner.clone(),
            received_at: now,
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
                        expires_at, sequence, revision, payload, payload_bytes, developer, origin,
                        install_source, package_digest, received_at
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
                    expires_at, sequence, revision, payload, payload_bytes, developer, origin,
                    install_source, package_digest, received_at
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
    if record.schema_version == 0 {
        return Err(TelemetryError::InvalidRecord("schema version"));
    }
    if record.sequence == Some(0) {
        return Err(TelemetryError::InvalidRecord("sequence"));
    }
    if record.family == TelemetryFamily::GamePresence && record.expires_at.is_none() {
        return Err(TelemetryError::InvalidRecord(
            "presence records require an expiry",
        ));
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
    const RESERVED_FIELDS: &[&str] = &[
        "source",
        "developer",
        "origin",
        "plugin_id",
        "owner_plugin_id",
        "publisher",
        "publisher_fingerprint",
        "version",
        "package",
        "package_version",
        "package_digest",
        "install_source",
        "profile_id",
        "provenance",
        "record_id",
        "schema_id",
        "schema_version",
        "resource_id",
        "observed_at",
        "received_at",
        "expires_at",
        "sequence",
    ];
    if record
        .payload
        .as_object()
        .is_some_and(|fields| RESERVED_FIELDS.iter().any(|key| fields.contains_key(*key)))
    {
        return Err(TelemetryError::InvalidRecord(
            "payload contains a host-owned attribution or envelope field",
        ));
    }
    Ok(())
}

fn host_record_id(owner: &TelemetryOwner, revision: u64) -> String {
    let mut identity = sha2::Sha256::new();
    identity.update(b"gamenode.telemetry-record.v1\0");
    for part in [
        owner.plugin_id.as_bytes(),
        owner.publisher_fingerprint.as_bytes(),
        owner.profile_id.as_bytes(),
    ] {
        identity.update((part.len() as u64).to_be_bytes());
        identity.update(part);
    }
    let namespace = identity.finalize();
    let namespace = namespace
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("telemetry-{namespace}-{revision}")
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
    let developer: String = row.get(15)?;
    let origin: String = row.get(16)?;
    let install_source: String = row.get(17)?;
    let package_digest: String = row.get(18)?;
    let received_at: i64 = row.get(19)?;
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
        record_id: row.get(0)?,
        record: TelemetryRecord {
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
            developer,
            origin,
            install_source,
            publisher_fingerprint: row.get(4)?,
            package_version: row.get(5)?,
            package_digest,
            profile_id: row.get(6)?,
        },
        received_at,
        revision,
        payload_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn owner() -> TelemetryOwner {
        TelemetryOwner {
            plugin_id: "org.example.collector".into(),
            developer: "Example Developer".into(),
            origin: "https://plugins.example/collector".into(),
            install_source: "manual-import".into(),
            publisher_fingerprint: "SHA256:publisher-key".into(),
            package_version: "1.2.3".into(),
            package_digest: "a".repeat(64),
            profile_id: "profile-a".into(),
        }
    }

    fn record(sequence: Option<u64>, expires_at: Option<i64>) -> TelemetryRecord {
        TelemetryRecord {
            family: TelemetryFamily::GamePerformance,
            resource_id: "series-1".into(),
            schema_id: "org.example.performance.v1".into(),
            schema_version: 1,
            observed_at: 100,
            expires_at,
            sequence,
            payload: json!({"frames_per_second": 60.0, "cpu_percent": 12.5}),
        }
    }

    #[test]
    fn publication_assigns_provenance_record_id_and_receipt_time() {
        let temp = tempfile::tempdir().unwrap();
        let store = PluginTelemetryStore::open(&temp.path().join("telemetry.sqlite")).unwrap();
        let owner = owner();
        let stored = store
            .publish(&owner, &record(Some(1), Some(200)), 110)
            .unwrap();

        assert!(stored.record_id.starts_with("telemetry-"));
        assert_ne!(stored.record_id, "plugin-selected-id");
        assert_eq!(stored.owner, owner);
        assert_eq!(stored.received_at, 110);
        assert_eq!(stored.record.sequence, Some(1));
        assert_eq!(stored.record.schema_version, 1);
    }

    #[test]
    fn forged_provenance_and_envelope_fields_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let store = PluginTelemetryStore::open(&temp.path().join("telemetry.sqlite")).unwrap();
        let mut forged = record(None, Some(200));
        forged.payload = json!({
            "frames_per_second": 60.0,
            "publisher_fingerprint": "SHA256:attacker",
            "package_version": "9.9.9"
        });

        assert!(matches!(
            store.publish(&owner(), &forged, 110),
            Err(TelemetryError::InvalidRecord(
                "payload contains a host-owned attribution or envelope field"
            ))
        ));
        assert!(
            store
                .list(TelemetryFamily::GamePerformance, None, None, 0, 10, 110)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn duplicate_and_out_of_order_sequences_stay_rejected_after_expiry_and_restart() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("telemetry.sqlite");
        let owner = owner();
        let store = PluginTelemetryStore::open(&path).unwrap();
        store
            .publish(&owner, &record(Some(2), Some(105)), 100)
            .unwrap();
        assert!(matches!(
            store.publish(&owner, &record(Some(2), Some(205)), 110),
            Err(TelemetryError::SequenceConflict)
        ));
        assert!(matches!(
            store.publish(&owner, &record(Some(1), Some(205)), 110),
            Err(TelemetryError::SequenceConflict)
        ));
        assert_eq!(store.expire(106).unwrap(), 1);
        drop(store);

        let reopened = PluginTelemetryStore::open(&path).unwrap();
        assert!(matches!(
            reopened.publish(&owner, &record(Some(1), Some(205)), 110),
            Err(TelemetryError::SequenceConflict)
        ));
        assert_eq!(
            reopened
                .publish(&owner, &record(Some(3), Some(205)), 110)
                .unwrap()
                .record
                .sequence,
            Some(3)
        );
    }

    #[test]
    fn oversized_records_and_non_expiring_presence_are_refused() {
        let temp = tempfile::tempdir().unwrap();
        let store = PluginTelemetryStore::open(&temp.path().join("telemetry.sqlite")).unwrap();
        let mut oversized = record(None, Some(200));
        oversized.payload = json!({"padding": "x".repeat(MAX_RECORD_BYTES)});
        assert!(matches!(
            store.publish(&owner(), &oversized, 110),
            Err(TelemetryError::RecordTooLarge)
        ));

        let mut presence = record(Some(1), None);
        presence.family = TelemetryFamily::GamePresence;
        assert!(matches!(
            store.publish(&owner(), &presence, 110),
            Err(TelemetryError::InvalidRecord(
                "presence records require an expiry"
            ))
        ));
    }

    #[test]
    fn expired_records_are_hidden_and_reclaimed_by_ttl() {
        let temp = tempfile::tempdir().unwrap();
        let store = PluginTelemetryStore::open(&temp.path().join("telemetry.sqlite")).unwrap();
        let stored = store
            .publish(&owner(), &record(None, Some(120)), 110)
            .unwrap();

        assert!(store.get(&stored.record_id, 119).unwrap().is_some());
        assert!(store.get(&stored.record_id, 120).unwrap().is_none());
        assert!(
            store
                .list(TelemetryFamily::GamePerformance, None, None, 0, 10, 120)
                .unwrap()
                .is_empty()
        );
        assert_eq!(store.expire(120).unwrap(), 1);
    }

    #[test]
    fn schema_upgrade_preserves_records_and_seeds_sequence_watermarks() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("telemetry.sqlite");
        let owner = owner();
        let old = Connection::open(&path).unwrap();
        old.execute_batch(
            "CREATE TABLE telemetry_records (
                 record_id TEXT PRIMARY KEY,
                 family TEXT NOT NULL,
                 resource_id TEXT NOT NULL,
                 plugin_id TEXT NOT NULL,
                 publisher_fingerprint TEXT NOT NULL,
                 package_version TEXT NOT NULL,
                 profile_id TEXT NOT NULL,
                 schema_id TEXT NOT NULL,
                 schema_version INTEGER NOT NULL,
                 observed_at INTEGER NOT NULL,
                 expires_at INTEGER,
                 sequence INTEGER,
                 revision INTEGER NOT NULL,
                 payload BLOB NOT NULL,
                 payload_bytes INTEGER NOT NULL
             );
             CREATE TABLE telemetry_revisions (
                 plugin_id TEXT NOT NULL,
                 publisher_fingerprint TEXT NOT NULL,
                 profile_id TEXT NOT NULL,
                 revision INTEGER NOT NULL,
                 PRIMARY KEY (plugin_id, publisher_fingerprint, profile_id)
             );
             INSERT INTO telemetry_records VALUES (
                 'legacy-record', 'game_performance', 'series-1',
                 'org.example.collector', 'SHA256:publisher-key', '1.2.3', 'profile-a',
                 'org.example.performance.v1', 1, 100, 200, 2, 1, X'7B7D', 2
             );
             PRAGMA user_version = 1;",
        )
        .unwrap();
        drop(old);

        let store = PluginTelemetryStore::open(&path).unwrap();
        let legacy = store.get("legacy-record", 110).unwrap().unwrap();
        assert_eq!(legacy.received_at, 0, "legacy receipt time remains unknown");
        assert_eq!(legacy.record.sequence, Some(2));
        assert!(matches!(
            store.publish(&owner, &record(Some(2), Some(200)), 110),
            Err(TelemetryError::SequenceConflict)
        ));
    }
}
