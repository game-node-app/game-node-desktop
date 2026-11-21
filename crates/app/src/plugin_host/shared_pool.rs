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
    ArchiveOutcome, OwnerScope, OwnerUsage, PruneOutcome, RecordProvenance, RecordWrite,
    SharedDataError, SharedDataStore, SharedRecord, WriteOutcome,
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

    /// Logical usage and effective retention limits of one producer, read on the storage worker.
    /// `None` means this host never persisted a record; a read never creates the pool.
    pub(crate) async fn usage(
        self: &Arc<Self>,
        owner: OwnerScope,
        now: i64,
    ) -> Result<Option<OwnerUsage>, Status> {
        self.read(move |store| store.usage(&owner, now)).await
    }

    /// Removes one producer's durable records and reports the bytes actually reclaimed. Another
    /// owner's rows and the private plugin stores are untouched. `None` means this host never
    /// persisted a record.
    pub(crate) async fn delete_owner(
        self: &Arc<Self>,
        owner: OwnerScope,
        now: i64,
    ) -> Result<Option<PruneOutcome>, Status> {
        self.mutate_opened(move |store| store.delete_owner(&owner, now))
            .await
    }

    /// Expires and prunes one producer's records under its effective limits. User-protected and
    /// archived rows stay. `None` means this host never persisted a record.
    pub(crate) async fn prune_owner(
        self: &Arc<Self>,
        owner: OwnerScope,
        now: i64,
    ) -> Result<Option<PruneOutcome>, Status> {
        self.mutate_opened(move |store| store.prune(&owner, now))
            .await
    }

    /// Reclaims one producer's archived, unprotected records in a reviewed host cleanup. `None`
    /// means this host never persisted a record.
    pub(crate) async fn reclaim_archived_owner(
        self: &Arc<Self>,
        owner: OwnerScope,
    ) -> Result<Option<PruneOutcome>, Status> {
        self.mutate_opened(move |store| store.reclaim_archived_owner(&owner))
            .await
    }

    /// Makes one producer's durable records inert for uninstall retention. `None` means this host
    /// never persisted a record. Another owner's rows and the private plugin stores are untouched.
    pub(crate) async fn archive_owner(
        self: &Arc<Self>,
        owner: OwnerScope,
        now: i64,
    ) -> Result<Option<ArchiveOutcome>, Status> {
        self.mutate_opened(move |store| store.archive_owner(&owner, now))
            .await
    }

    /// Restores one producer's archived records after an explicit host review. `None` means this
    /// host never persisted a record. A plugin RPC can never reach this decision.
    pub(crate) async fn reuse_archived_owner(
        self: &Arc<Self>,
        owner: OwnerScope,
        now: i64,
    ) -> Result<Option<ArchiveOutcome>, Status> {
        self.mutate_opened(move |store| store.reuse_archived_owner(&owner, now))
            .await
    }

    /// Runs one blocking mutation on the storage worker after opening an existing pool. An absent
    /// pool is not created and answers `None`.
    async fn mutate_opened<T, F>(self: &Arc<Self>, mutate: F) -> Result<Option<T>, Status>
    where
        T: Send + 'static,
        F: FnOnce(&mut SharedDataStore) -> Result<T, SharedDataError> + Send + 'static,
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
            let store = store.as_mut().expect("the pool is open");
            mutate(store).map(Some).map_err(pool_status)
        })
        .await
        .map_err(|_| Status::unavailable("the durable shared-data worker is unavailable"))?
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
        SharedDataError::ArchivedRecordExists => Status::failed_precondition(
            "archived shared data requires a host reuse review before it can be replaced",
        ),
        SharedDataError::Open(_)
        | SharedDataError::Database(_)
        | SharedDataError::NewerVersion
        | SharedDataError::ForeignDatabase
        | SharedDataError::MigrationFailed { .. } => {
            Status::unavailable("the durable shared-data pool is unavailable")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn owner(byte: u8) -> OwnerScope {
        OwnerScope::new([byte; 32], 1).unwrap()
    }

    fn write(plugin: &str, resource: &str, fields_json: &str) -> DurableWrite {
        DurableWrite {
            owner: owner(if plugin == "producer" { 1 } else { 2 }),
            dataset_id: "org.example.catalog".to_owned(),
            schema_id: "org.example.catalog.v1".to_owned(),
            schema_version: 1,
            fields_json: fields_json.to_owned(),
            opaque_payload: Vec::new(),
            resource_id: Some(resource.to_owned()),
            observed_at: 100,
            sequence: 1,
            source_observation_id: "observation-1".to_owned(),
            expires_at: None,
            now: 100,
            provenance: RecordProvenance {
                source_plugin_id: plugin.to_owned(),
                source_publisher_fingerprint: format!("publisher-{plugin}"),
                source_profile_id: "profile-1".to_owned(),
                declared_developer: "Example Developer".to_owned(),
                origin: "registry".to_owned(),
                package_version: "1.2.3".to_owned(),
                package_digest: vec![7; 32],
            },
        }
    }

    #[tokio::test]
    async fn usage_and_deletion_are_owner_scoped_and_never_touch_private_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let pool = Arc::new(DurableSharedPool::new(dir.path()));
        let producer = owner(1);
        let other = owner(2);
        let producer_record = pool
            .persist(write("producer", "disc-1", r#"{"title":"Dolphin"}"#))
            .await
            .unwrap();
        let other_record = pool
            .persist(write("other", "disc-2", r#"{"title":"Other"}"#))
            .await
            .unwrap();
        assert_ne!(producer_record.record_id, other_record.record_id);

        let usage = pool.usage(producer, 110).await.unwrap().unwrap();
        assert_eq!(usage.records, 1);
        let expected_bytes = usage.logical_bytes;
        assert!(expected_bytes > 0);
        let other_usage = pool.usage(other, 110).await.unwrap().unwrap();
        assert_eq!(other_usage.records, 1);
        let expected_other_bytes = other_usage.logical_bytes;

        // Representative private stores beside the public pool must survive the user cleanup.
        let storage_dir = dir
            .path()
            .join("plugin-data")
            .join("publisher")
            .join("plugin");
        fs::create_dir_all(&storage_dir).unwrap();
        let storage_file = storage_dir.join("storage.sqlite3");
        let persistence_file = dir.path().join("plugin-persistence.sqlite3");
        fs::write(&storage_file, b"private-storage").unwrap();
        fs::write(&persistence_file, b"private-persistence").unwrap();

        let deleted = pool.delete_owner(producer, 120).await.unwrap().unwrap();
        assert_eq!(deleted.removed_records, 1);
        assert_eq!(deleted.reclaimed_bytes, expected_bytes);
        assert_eq!(pool.usage(producer, 120).await.unwrap().unwrap().records, 0);
        let remaining = pool.usage(other, 120).await.unwrap().unwrap();
        assert_eq!(remaining.records, 1);
        assert_eq!(remaining.logical_bytes, expected_other_bytes);
        assert_eq!(fs::read(&storage_file).unwrap(), b"private-storage");
        assert_eq!(fs::read(&persistence_file).unwrap(), b"private-persistence");
        assert_eq!(
            pool.delete_owner(producer, 130)
                .await
                .unwrap()
                .unwrap()
                .removed_records,
            0
        );
    }

    #[tokio::test]
    async fn archiving_and_reuse_are_owner_scoped_and_leave_private_files_alone() {
        let dir = tempfile::TempDir::new().unwrap();
        let pool = Arc::new(DurableSharedPool::new(dir.path()));
        let producer = owner(1);
        let other = owner(2);
        let producer_record = pool
            .persist(write("producer", "disc-1", r#"{"title":"Dolphin"}"#))
            .await
            .unwrap();
        pool.persist(write("other", "disc-2", r#"{"title":"Other"}"#))
            .await
            .unwrap();

        let storage_dir = dir
            .path()
            .join("plugin-data")
            .join("publisher")
            .join("plugin");
        fs::create_dir_all(&storage_dir).unwrap();
        let storage_file = storage_dir.join("storage.sqlite3");
        fs::write(&storage_file, b"private-storage").unwrap();

        let live_before = pool.usage(producer, 110).await.unwrap().unwrap();
        assert_eq!(live_before.records, 1);
        assert_eq!(live_before.archived_records, 0);
        let archived = pool.archive_owner(producer, 120).await.unwrap().unwrap();
        assert_eq!(archived.records, 1);
        assert!(
            pool.get_any(producer_record.record_id.clone(), 120)
                .await
                .unwrap()
                .is_none(),
            "an archived row is not readable across owners"
        );
        let usage = pool.usage(producer, 120).await.unwrap().unwrap();
        assert_eq!(usage.records, 0);
        assert_eq!(usage.archived_records, 1);
        assert_eq!(usage.archived_logical_bytes, live_before.logical_bytes);
        let other_usage = pool.usage(other, 120).await.unwrap().unwrap();
        assert_eq!(other_usage.records, 1);
        assert_eq!(other_usage.archived_records, 0);

        let error = pool
            .persist(write("producer", "disc-1", r#"{"title":"Replacement"}"#))
            .await
            .unwrap_err();
        assert_eq!(error.code(), tonic::Code::FailedPrecondition);

        let reused = pool
            .reuse_archived_owner(producer, 130)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reused.records, 1);
        assert_eq!(pool.usage(producer, 130).await.unwrap().unwrap().records, 1);
        let reused_record = pool
            .get_any(producer_record.record_id, 130)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            reused_record.fields.get("title"),
            Some(&serde_json::Value::String("Dolphin".to_owned()))
        );
        assert_eq!(fs::read(&storage_file).unwrap(), b"private-storage");
        assert_eq!(pool.usage(other, 130).await.unwrap().unwrap().records, 1);
    }

    #[tokio::test]
    async fn reading_usage_of_an_absent_pool_creates_nothing() {
        let dir = tempfile::TempDir::new().unwrap();
        let pool = Arc::new(DurableSharedPool::new(dir.path()));
        assert!(pool.usage(owner(1), 100).await.unwrap().is_none());
        assert!(pool.delete_owner(owner(1), 100).await.unwrap().is_none());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
