//! Bounded, session-scoped published plugin data.
//!
//! Published records are separate from the producer's private `Storage` namespace. The host
//! assigns record identity, revision and provenance, validates source sequence numbers, expires
//! records by TTL and removes records when their authenticated publishing session closes.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use gamenode_plugin_protocol::{prost::Message, proto::DataRecord};
use tonic::Status;

pub const MAX_DATASET_ID_BYTES: usize = 128;
pub const MAX_SCHEMA_ID_BYTES: usize = 256;
pub const MAX_RESOURCE_ID_BYTES: usize = 256;
pub const MAX_OBSERVATION_ID_BYTES: usize = 256;
pub const MAX_RECORD_BYTES: usize = 64 * 1024;
pub const MAX_RECORDS_PER_PLUGIN: usize = 1_000;
pub const MAX_RECORDS_PER_PROFILE: usize = 10_000;
pub const MAX_BYTES_PER_PLUGIN: usize = 8 * 1024 * 1024;
pub const MAX_BYTES_PER_PROFILE: usize = 64 * 1024 * 1024;
pub const MAX_TTL_SECONDS: u64 = 30 * 24 * 60 * 60;

type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

fn system_clock() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs().min(i64::MAX as u64) as i64)
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct OwnerKey {
    profile_id: String,
    plugin_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct StreamKey {
    owner: OwnerKey,
    dataset_id: String,
    resource_id: String,
}

#[derive(Clone, Debug)]
struct StoredRecord {
    owner: OwnerKey,
    profile_epoch: u64,
    session_id: String,
    record: DataRecord,
    encoded_bytes: usize,
}

#[derive(Default)]
struct State {
    records: BTreeMap<String, StoredRecord>,
    streams: BTreeMap<StreamKey, (u64, String)>,
    revisions: HashMap<String, u64>,
    bytes_by_owner: HashMap<OwnerKey, usize>,
    bytes_by_profile: HashMap<String, usize>,
}

/// In-memory published data. Durable publication is a separate `storage.persist` capability and
/// is refused by this store until the persistence adapter is implemented.
pub struct SharedDataStore {
    state: Mutex<State>,
    clock: Clock,
}

impl Default for SharedDataStore {
    fn default() -> Self {
        Self {
            state: Mutex::default(),
            clock: Arc::new(system_clock),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Publisher {
    pub profile_id: String,
    pub profile_epoch: u64,
    pub plugin_id: String,
    pub publisher_fingerprint: String,
    pub package_version: String,
    pub declared_developer: String,
    pub origin: String,
    pub package_digest: Vec<u8>,
    pub session_id: String,
}

#[derive(Clone, Debug)]
pub enum PublishOutcome {
    Applied(DataRecord),
    Duplicate(DataRecord),
    Conflict { current_sequence: u64 },
}

impl SharedDataStore {
    #[cfg(test)]
    pub fn with_clock(clock: impl Fn() -> i64 + Send + Sync + 'static) -> Self {
        Self {
            state: Mutex::default(),
            clock: Arc::new(clock),
        }
    }

    /// Validate and retain one session-scoped record. Provenance and ids are overwritten from
    /// the authenticated `Publisher`; plugin supplied ownership fields are never accepted.
    pub fn publish(
        &self,
        publisher: &Publisher,
        mut record: DataRecord,
        ttl_seconds: u64,
        is_session_open: impl Fn(&str) -> bool,
    ) -> Result<PublishOutcome, Status> {
        validate_record(&record, ttl_seconds)?;
        if publisher.profile_epoch == 0
            || !valid_text(&publisher.profile_id, 256)
            || !valid_text(&publisher.plugin_id, 256)
            || !valid_text(&publisher.publisher_fingerprint, 128)
            || !valid_text(&publisher.package_version, 128)
            || !valid_text(&publisher.declared_developer, 256)
            || !valid_text(&publisher.origin, 512)
            || !valid_text(&publisher.session_id, 128)
            || publisher.package_digest.len() != 32
        {
            return Err(Status::invalid_argument(
                "invalid authenticated publisher metadata",
            ));
        }

        let owner = OwnerKey {
            profile_id: publisher.profile_id.clone(),
            plugin_id: publisher.plugin_id.clone(),
        };
        let stream = StreamKey {
            owner: owner.clone(),
            dataset_id: record.dataset_id.clone(),
            resource_id: record.resource_id.clone(),
        };
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        self.prune_locked(&mut state, is_session_open);
        self.prune_profile_epoch_locked(&mut state, &publisher.profile_id, publisher.profile_epoch);

        if let Some((sequence, source_id)) = state.streams.get(&stream) {
            if *source_id == record.source_observation_id {
                if let Some(existing) = state.records.values().find(|item| {
                    item.owner == owner
                        && item.record.dataset_id == record.dataset_id
                        && item.record.resource_id == record.resource_id
                        && item.record.source_observation_id == record.source_observation_id
                }) {
                    return Ok(PublishOutcome::Duplicate(existing.record.clone()));
                }
            }
            if record.sequence <= *sequence {
                return Ok(PublishOutcome::Conflict {
                    current_sequence: *sequence,
                });
            }
        }
        let received_at = (self.clock)();
        record.record_id = gamenode_platform::random::session_id();
        record.publisher_id.clone_from(&publisher.plugin_id);
        record.revision = state
            .revisions
            .get(&publisher.profile_id)
            .copied()
            .unwrap_or_default()
            .saturating_add(1);
        record.received_at = received_at;
        record.expires_at = received_at.saturating_add(ttl_seconds.min(i64::MAX as u64) as i64);
        record.provenance = Some(gamenode_plugin_protocol::proto::DataProvenance {
            producer_plugin_id: publisher.plugin_id.clone(),
            declared_developer: publisher.declared_developer.clone(),
            origin: publisher.origin.clone(),
            publisher_fingerprint: publisher.publisher_fingerprint.clone(),
            package_version: publisher.package_version.clone(),
            package_digest: publisher.package_digest.clone(),
            resource_id: record.resource_id.clone(),
            observed_at: record.observed_at,
            received_at,
            source_record_ids: Vec::new(),
        });
        let encoded_bytes = record.encoded_len();
        if state
            .records
            .values()
            .filter(|item| item.owner.profile_id == publisher.profile_id)
            .count()
            >= MAX_RECORDS_PER_PROFILE
            || state
                .records
                .values()
                .filter(|item| item.owner == owner)
                .count()
                >= MAX_RECORDS_PER_PLUGIN
            || state
                .bytes_by_owner
                .get(&owner)
                .copied()
                .unwrap_or_default()
                .saturating_add(encoded_bytes)
                > MAX_BYTES_PER_PLUGIN
            || state
                .bytes_by_profile
                .get(&publisher.profile_id)
                .copied()
                .unwrap_or_default()
                .saturating_add(encoded_bytes)
                > MAX_BYTES_PER_PROFILE
        {
            return Err(Status::resource_exhausted(
                "published data record quota reached",
            ));
        }

        // Identity is host assigned before quota accounting so the encoded bound includes every
        // host-added field, including provenance.
        state
            .revisions
            .insert(publisher.profile_id.clone(), record.revision);
        state.streams.insert(
            stream,
            (record.sequence, record.source_observation_id.clone()),
        );
        *state.bytes_by_owner.entry(owner.clone()).or_default() += encoded_bytes;
        *state
            .bytes_by_profile
            .entry(publisher.profile_id.clone())
            .or_default() += encoded_bytes;
        state.records.insert(
            record.record_id.clone(),
            StoredRecord {
                owner,
                profile_epoch: publisher.profile_epoch,
                session_id: publisher.session_id.clone(),
                record: record.clone(),
                encoded_bytes,
            },
        );
        Ok(PublishOutcome::Applied(record))
    }

    /// Private-only query for a publisher. Common queries are layered on top of this method and
    /// must be authorized by the caller before other owners are selected.
    pub fn list_private(
        &self,
        profile_id: &str,
        profile_epoch: u64,
        plugin_id: &str,
        dataset_id: &str,
        is_session_open: impl Fn(&str) -> bool,
    ) -> Vec<DataRecord> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        self.prune_locked(&mut state, is_session_open);
        self.prune_profile_epoch_locked(&mut state, profile_id, profile_epoch);
        state
            .records
            .values()
            .filter(|item| {
                item.owner.profile_id == profile_id
                    && item.owner.plugin_id == plugin_id
                    && item.profile_epoch == profile_epoch
                    && item.record.dataset_id == dataset_id
            })
            .map(|item| item.record.clone())
            .collect()
    }

    pub fn get_private(
        &self,
        profile_id: &str,
        profile_epoch: u64,
        plugin_id: &str,
        dataset_id: &str,
        record_id: &str,
        is_session_open: impl Fn(&str) -> bool,
    ) -> Option<DataRecord> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        self.prune_locked(&mut state, is_session_open);
        self.prune_profile_epoch_locked(&mut state, profile_id, profile_epoch);
        state.records.get(record_id).and_then(|item| {
            (item.owner.profile_id == profile_id
                && item.owner.plugin_id == plugin_id
                && item.profile_epoch == profile_epoch
                && item.record.dataset_id == dataset_id)
                .then(|| item.record.clone())
        })
    }

    fn prune_locked(&self, state: &mut State, is_session_open: impl Fn(&str) -> bool) {
        let now = (self.clock)();
        let expired: Vec<_> = state
            .records
            .iter()
            .filter(|(_, item)| item.record.expires_at <= now || !is_session_open(&item.session_id))
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            if let Some(item) = state.records.remove(&id) {
                if let Some(bytes) = state.bytes_by_owner.get_mut(&item.owner) {
                    *bytes = bytes.saturating_sub(item.encoded_bytes);
                }
                if let Some(bytes) = state.bytes_by_profile.get_mut(&item.owner.profile_id) {
                    *bytes = bytes.saturating_sub(item.encoded_bytes);
                }
            }
        }
        state.bytes_by_owner.retain(|_, bytes| *bytes != 0);
        state.bytes_by_profile.retain(|_, bytes| *bytes != 0);
        let live_streams: std::collections::BTreeSet<_> = state
            .records
            .values()
            .map(|item| StreamKey {
                owner: item.owner.clone(),
                dataset_id: item.record.dataset_id.clone(),
                resource_id: item.record.resource_id.clone(),
            })
            .collect();
        state
            .streams
            .retain(|stream, _| live_streams.contains(stream));
    }

    fn prune_profile_epoch_locked(&self, state: &mut State, profile_id: &str, profile_epoch: u64) {
        let stale: Vec<_> = state
            .records
            .iter()
            .filter(|(_, item)| {
                item.owner.profile_id == profile_id && item.profile_epoch != profile_epoch
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale {
            if let Some(item) = state.records.remove(&id) {
                if let Some(bytes) = state.bytes_by_owner.get_mut(&item.owner) {
                    *bytes = bytes.saturating_sub(item.encoded_bytes);
                }
                if let Some(bytes) = state.bytes_by_profile.get_mut(profile_id) {
                    *bytes = bytes.saturating_sub(item.encoded_bytes);
                }
            }
        }
        state.bytes_by_owner.retain(|_, bytes| *bytes != 0);
        state.bytes_by_profile.retain(|_, bytes| *bytes != 0);
        let live_streams: std::collections::BTreeSet<_> = state
            .records
            .values()
            .map(|item| StreamKey {
                owner: item.owner.clone(),
                dataset_id: item.record.dataset_id.clone(),
                resource_id: item.record.resource_id.clone(),
            })
            .collect();
        state
            .streams
            .retain(|stream, _| live_streams.contains(stream));
    }
}

fn validate_record(record: &DataRecord, ttl_seconds: u64) -> Result<(), Status> {
    if !valid_text(&record.dataset_id, MAX_DATASET_ID_BYTES)
        || !valid_text(&record.schema_id, MAX_SCHEMA_ID_BYTES)
        || record.schema_version == 0
        || !valid_text(&record.resource_id, MAX_RESOURCE_ID_BYTES)
        || !valid_text(&record.source_observation_id, MAX_OBSERVATION_ID_BYTES)
        || record.sequence == 0
        || record.observed_at < 0
        || ttl_seconds == 0
        || ttl_seconds > MAX_TTL_SECONDS
        || (record.value.is_some() == !record.opaque_payload.is_empty())
    {
        return Err(Status::invalid_argument("invalid published data record"));
    }
    if record.encoded_len() > MAX_RECORD_BYTES {
        return Err(Status::invalid_argument(
            "published data record exceeds size limit",
        ));
    }
    if !record
        .schema_id
        .starts_with(&format!("{}.", record.dataset_id))
        && record.schema_id != record.dataset_id
    {
        return Err(Status::invalid_argument(
            "schema id is outside the dataset namespace",
        ));
    }
    Ok(())
}

fn valid_text(value: &str, max_bytes: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= max_bytes
        && !value.chars().any(|character| character.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn publisher(plugin_id: &str, session_id: &str) -> Publisher {
        Publisher {
            profile_id: "profile-a".into(),
            profile_epoch: 2,
            plugin_id: plugin_id.into(),
            publisher_fingerprint: format!("publisher-{plugin_id}"),
            package_version: "1.2.3".into(),
            declared_developer: "Example Developer".into(),
            origin: "signed-package".into(),
            package_digest: vec![7; 32],
            session_id: session_id.into(),
        }
    }

    fn record(sequence: u64, source_id: &str) -> DataRecord {
        DataRecord {
            publisher_id: "forged-publisher".into(),
            dataset_id: "demo.sensors".into(),
            schema_id: "demo.sensors.v1".into(),
            schema_version: 1,
            opaque_payload: vec![1, 2, 3],
            resource_id: "runtime-a".into(),
            observed_at: 995,
            sequence,
            source_observation_id: source_id.into(),
            ..Default::default()
        }
    }

    #[test]
    fn publisher_identity_and_provenance_are_host_assigned_and_records_are_private_by_default() {
        let store = SharedDataStore::with_clock(|| 1_000);
        let published = match store
            .publish(
                &publisher("producer", "session-producer"),
                record(1, "obs-1"),
                60,
                |_| true,
            )
            .unwrap()
        {
            PublishOutcome::Applied(record) => record,
            other => panic!("expected a new record, got {other:?}"),
        };
        assert_eq!(published.publisher_id, "producer");
        assert_eq!(published.provenance.unwrap().producer_plugin_id, "producer");
        assert!(
            store
                .list_private("profile-a", 2, "consumer", "demo.sensors", |_| true)
                .is_empty()
        );
        assert_eq!(
            store
                .list_private("profile-a", 2, "producer", "demo.sensors", |_| true)
                .len(),
            1
        );
    }

    #[test]
    fn duplicates_are_idempotent_and_out_of_order_sequences_conflict() {
        let store = SharedDataStore::with_clock(|| 1_000);
        let owner = publisher("producer", "session-producer");
        let first = match store
            .publish(&owner, record(2, "obs-2"), 60, |_| true)
            .unwrap()
        {
            PublishOutcome::Applied(record) => record,
            other => panic!("expected applied, got {other:?}"),
        };
        assert!(matches!(
            store.publish(&owner, record(2, "obs-2"), 60, |_| true).unwrap(),
            PublishOutcome::Duplicate(record) if record.record_id == first.record_id
        ));
        assert!(matches!(
            store
                .publish(&owner, record(1, "obs-1"), 60, |_| true)
                .unwrap(),
            PublishOutcome::Conflict {
                current_sequence: 2
            }
        ));
    }

    #[test]
    fn expired_or_disconnected_records_are_removed_and_cannot_cross_profile_epochs() {
        let now = Arc::new(std::sync::atomic::AtomicI64::new(1_000));
        let clock_now = now.clone();
        let store = SharedDataStore::with_clock(move || {
            clock_now.load(std::sync::atomic::Ordering::Relaxed)
        });
        let first = store
            .publish(
                &publisher("producer", "session-producer"),
                record(1, "obs-1"),
                10,
                |_| true,
            )
            .unwrap();
        let first_id = match first {
            PublishOutcome::Applied(record) => record.record_id,
            other => panic!("expected initial record, got {other:?}"),
        };
        assert!(
            store
                .get_private(
                    "profile-a",
                    3,
                    "producer",
                    "demo.sensors",
                    &first_id,
                    |_| true,
                )
                .is_none()
        );
        assert!(
            store
                .list_private("profile-a", 2, "producer", "demo.sensors", |session| {
                    session != "session-producer"
                })
                .is_empty()
        );
        store
            .publish(
                &publisher("producer", "session-producer"),
                record(2, "obs-2"),
                10,
                |_| true,
            )
            .unwrap();
        now.store(1_011, std::sync::atomic::Ordering::Relaxed);
        assert!(
            store
                .list_private("profile-a", 2, "producer", "demo.sensors", |_| true)
                .is_empty()
        );
    }

    #[test]
    fn rejects_malformed_envelopes_and_oversized_payloads() {
        let store = SharedDataStore::with_clock(|| 1_000);
        assert!(
            store
                .publish(
                    &publisher("producer", "session-producer"),
                    record(0, "obs-0"),
                    60,
                    |_| true
                )
                .is_err()
        );
        let mut malformed = record(1, "obs-1");
        malformed.value = Some(Default::default());
        assert!(
            store
                .publish(
                    &publisher("producer", "session-producer"),
                    malformed,
                    60,
                    |_| true
                )
                .is_err()
        );
        let mut oversized = record(1, "obs-2");
        oversized.opaque_payload = vec![0; MAX_RECORD_BYTES];
        assert!(
            store
                .publish(
                    &publisher("producer", "session-producer"),
                    oversized,
                    60,
                    |_| true
                )
                .is_err()
        );
    }
}
