//! Host-owned worker for the durable public shared-data pool.
//!
//! A durable publication is written to the platform [`SharedDataStore`], never to the in-memory
//! broker. The store is opened lazily on the storage worker the first time a publication is
//! accepted, so a host that only sees session-only publications creates no file. Owner scopes,
//! provenance and the pool location come from the authenticated host context; a plugin RPC never
//! supplies or receives them. Every SQLite call runs on a blocking worker, never the UI thread.
//!
//! Reads go the other way: the worker loads the requested rows and hands them to the broker,
//! which applies the same owner/grant checks and field projections as a session record. A read
//! never creates the pool file; a host without one simply has no durable rows.

use super::grants::PluginIdentity;
use super::shared_data::{DurableRecord, Provenance};
use super::sharing_policy::SharingClass;
use gamenode_platform::plugin_shared_data::{
    OwnerScope, RecordProvenance, RecordWrite, SharedDataError, SharedDataStore, SharedRecord,
    WriteOutcome,
};
use gamenode_plugin_protocol::proto::CommonDataCategory;
use serde_json::Map;
use std::path::PathBuf;
use std::sync::Arc;
use tonic::Status;

/// The durable public pool. A disabled pool refuses durable publications and reads as empty; it
/// never downgrades a publication to session-only.
pub struct DurableSharedPool {
    /// The application data folder the pool file lives in; absent when the host has none.
    dir: Option<PathBuf>,
    store: Arc<tokio::sync::Mutex<Option<SharedDataStore>>>,
}

impl Default for DurableSharedPool {
    fn default() -> Self {
        Self::disabled()
    }
}

impl DurableSharedPool {
    /// A pool that refuses durable publications, for hosts and tests without a data folder.
    pub fn disabled() -> Self {
        Self {
            dir: None,
            store: Arc::default(),
        }
    }

    /// Durable records go below `dir` (the application data folder).
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: Some(dir.into()),
            store: Arc::default(),
        }
    }

    /// A host-assigned publication ready for the durable pool. Returns the accepted record as the
    /// broker may project it, so a live watch can deliver the same publication.
    pub(crate) async fn persist(
        self: &Arc<Self>,
        write: DurableWrite,
    ) -> Result<DurableRecord, Status> {
        let dir = self.dir.clone().ok_or_else(|| {
            Status::failed_precondition("durable shared data is unavailable on this host")
        })?;
        let mut store = self.store.clone().lock_owned().await;
        tokio::task::spawn_blocking(move || {
            if store.is_none() {
                *store = Some(SharedDataStore::open(&dir).map_err(pool_status)?);
            }
            let store = store.as_mut().expect("the pool was just opened");
            let outcome = store.write(
                write.owner,
                RecordWrite {
                    dataset_id: &write.dataset_id,
                    category: None,
                    class: SharingClass::Custom.as_name(),
                    schema_id: &write.schema_id,
                    schema_version: write.schema_version,
                    fields_json: &write.fields_json,
                    opaque_payload: &write.opaque_payload,
                    resource_id: write.resource_id.as_deref(),
                    observed_at: write.observed_at,
                    sequence: write.sequence,
                    source_observation_id: &write.source_observation_id,
                    expires_at: write.expires_at,
                    now: write.now,
                    provenance: &write.provenance,
                    expected_revision: None,
                },
            )
            .map_err(pool_status)?;
            match outcome {
                WriteOutcome::Stored(receipt) => durable_record(receipt.record),
                WriteOutcome::Conflict { current_revision } => {
                    Err(Status::failed_precondition(format!(
                        "published shared record changed concurrently; current revision is {current_revision}"
                    )))
                }
                WriteOutcome::OverQuota { .. } => Err(Status::resource_exhausted(
                    "shared-data retention quota is exhausted",
                )),
            }
        })
        .await
        .map_err(|_| Status::unavailable("the durable shared-data worker is unavailable"))?
    }

    /// One durable record by host id across every producer. `None` when the pool has no such
    /// unexpired record or this host never persisted one.
    pub(crate) async fn get_any(
        self: &Arc<Self>,
        record_id: String,
        now: i64,
    ) -> Result<Option<DurableRecord>, Status> {
        let Some(Some(record)) = self
            .read(move |store| store.get_any(&record_id, now))
            .await?
        else {
            return Ok(None);
        };
        durable_record(record).map(Some)
    }

    /// Every producer's unexpired records of one dataset in stable receipt order, together with
    /// the persisted pool sequence the page was read at. Both come from one worker call, so a list
    /// revision can seed the broker before the page is authorized and projected.
    pub(crate) async fn list_dataset_any(
        self: &Arc<Self>,
        dataset_id: String,
        now: i64,
    ) -> Result<(Vec<DurableRecord>, u64), Status> {
        let Some((records, pool_revision)) = self
            .read(move |store| {
                let records = store.list_dataset_any(&dataset_id, now)?;
                Ok((records, store.pool_revision()?))
            })
            .await?
        else {
            return Ok((Vec::new(), 0));
        };
        let records = records
            .into_iter()
            .map(durable_record)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((records, pool_revision))
    }

    /// The persisted pool sequence, or zero when this host never persisted a record. Reads never
    /// create the pool file.
    pub(crate) async fn pool_revision(self: &Arc<Self>) -> Result<u64, Status> {
        Ok(self.read(|store| store.pool_revision()).await?.unwrap_or(0))
    }

    /// Runs one blocking pool read on the storage worker. `None` means this host has no pool
    /// file; a read never creates one.
    async fn read<T, F>(self: &Arc<Self>, read: F) -> Result<Option<T>, Status>
    where
        T: Send + 'static,
        F: FnOnce(&SharedDataStore) -> Result<T, SharedDataError> + Send + 'static,
    {
        let Some(dir) = self.dir.clone() else {
            return Ok(None);
        };
        let mut store = self.store.clone().lock_owned().await;
        tokio::task::spawn_blocking(move || {
            if store.is_none() {
                match SharedDataStore::open_existing(&dir).map_err(pool_status)? {
                    Some(opened) => *store = Some(opened),
                    None => return Ok(None),
                }
            }
            let store = store.as_ref().expect("the pool is open");
            read(store).map(Some).map_err(pool_status)
        })
        .await
        .map_err(|_| Status::unavailable("the durable shared-data worker is unavailable"))?
    }
}

/// One fully resolved durable publication. All values are owned so the write can leave this task.
pub(crate) struct DurableWrite {
    pub owner: OwnerScope,
    pub dataset_id: String,
    pub schema_id: String,
    pub schema_version: u32,
    pub fields_json: String,
    pub opaque_payload: Vec<u8>,
    pub resource_id: Option<String>,
    pub observed_at: i64,
    pub sequence: u64,
    pub source_observation_id: String,
    pub expires_at: Option<i64>,
    pub now: i64,
    pub provenance: RecordProvenance,
}

/// Converts one stored row into the broker's durable input. A malformed row cannot come from this
/// host's publication path, so it fails closed instead of being silently skipped or exposed.
fn durable_record(record: SharedRecord) -> Result<DurableRecord, Status> {
    let class = SharingClass::from_name(&record.class)
        .ok_or_else(|| Status::unavailable("the durable shared-data pool is unavailable"))?;
    let fields = if record.fields_json.is_empty() {
        Map::new()
    } else {
        serde_json::from_str(&record.fields_json)
            .map_err(|_| Status::unavailable("the durable shared-data pool is unavailable"))?
    };
    let source = PluginIdentity {
        plugin_id: record.provenance.source_plugin_id,
        publisher_fingerprint: record.provenance.source_publisher_fingerprint,
        profile_id: record.provenance.source_profile_id,
    };
    Ok(DurableRecord {
        record_id: record.record_id,
        owner: source.clone(),
        dataset_id: record.dataset_id,
        category: record
            .category
            .and_then(|category| CommonDataCategory::try_from(category).ok()),
        class,
        schema_id: record.schema_id,
        schema_version: record.schema_version,
        fields,
        opaque_payload: record.opaque_payload,
        resource_id: record.resource_id.unwrap_or_default(),
        observed_at: record.observed_at,
        received_at: record.received_at,
        published_at: record.updated_at,
        sequence: record.sequence,
        source_observation_id: record.source_observation_id,
        expires_at: record.expires_at,
        revision: record.revision,
        pool_revision: record.pool_revision,
        provenance: Provenance {
            source,
            forwarded_by: Vec::new(),
            declared_developer: record.provenance.declared_developer,
            origin: record.provenance.origin,
            package_version: record.provenance.package_version,
            package_digest: record.provenance.package_digest,
            source_record_ids: Vec::new(),
        },
    })
}

/// Maps a store failure to a status without disclosing a path.
fn pool_status(error: SharedDataError) -> Status {
    match error {
        SharedDataError::RecordTooLarge => {
            Status::resource_exhausted("publication exceeds the size limit")
        }
        SharedDataError::InvalidRecord => Status::invalid_argument("shared record is invalid"),
        SharedDataError::InvalidOwner => {
            Status::failed_precondition("shared-data owner scope is invalid")
        }
        SharedDataError::StaleProfileEpoch => {
            Status::failed_precondition("shared-data owner scope is stale")
        }
        SharedDataError::Open(_)
        | SharedDataError::Database(_)
        | SharedDataError::NewerVersion
        | SharedDataError::ForeignDatabase
        | SharedDataError::MigrationFailed { .. } => {
            Status::unavailable("the durable shared-data pool is unavailable")
        }
    }
}
