//! Host-owned worker for the durable public shared-data pool.
//!
//! A durable publication is written to the platform [`SharedDataStore`], never to the in-memory
//! broker. The store is opened lazily on the storage worker the first time a publication is
//! accepted, so a host that only sees session-only publications creates no file. Owner scopes,
//! provenance and the pool location come from the authenticated host context; a plugin RPC never
//! supplies or receives them. Every SQLite call runs on a blocking worker, never the UI thread.

use gamenode_platform::plugin_shared_data::{
    OwnerScope, RecordProvenance, RecordWrite, SharedDataError, SharedDataStore, WriteOutcome,
};
use std::path::PathBuf;
use std::sync::Arc;
use tonic::Status;

/// The durable public pool. A disabled pool refuses durable publications; it never downgrades
/// them to session-only.
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

    /// A host-assigned publication ready for the durable pool.
    pub(crate) async fn persist(
        self: &Arc<Self>,
        write: DurableWrite,
    ) -> Result<DurableReceipt, Status> {
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
                    class: "custom",
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
                WriteOutcome::Stored(receipt) => Ok(DurableReceipt {
                    record_id: receipt.record.record_id,
                    revision: receipt.record.revision,
                    expires_at: receipt.record.expires_at,
                }),
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

/// The accepted publication as the caller may see it: host ids and the actual durability only.
pub(crate) struct DurableReceipt {
    pub record_id: String,
    pub revision: u64,
    pub expires_at: Option<i64>,
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
