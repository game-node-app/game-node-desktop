//! Shared-data query and subscription broker for domain and Data projections.
//!
//! This module is the authorization and projection boundary behind the single Data RPC API. The
//! caller must resolve [`PluginIdentity`] from the authenticated session before invoking it.
//! Record identity and provenance are host-owned; plugin payloads are either bounded dataset
//! fields or opaque bytes. The broker is wired into `ResourceHost` and shares its policy index
//! with domain projections.

use super::grants::PluginIdentity;
use super::sharing_policy::{
    AccessDecision, AccessRequest, AccessScope, Authorizer, Delivery, SharingClass,
};
use gamenode_plugin_protocol::proto::{CommonDataCategory, CommonDataSelector};
use serde_json::{Map, Value};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use thiserror::Error;

pub const MAX_RECORD_BYTES: usize = 1024 * 1024;
pub const MAX_RECORDS: usize = 16_384;
pub const MAX_RETAINED_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_PAGE_SIZE: usize = 256;
pub const MAX_SUBSCRIPTION_QUEUE: usize = 128;
pub const MAX_TTL_SECONDS: u64 = 30 * 24 * 60 * 60;
pub const MAX_RESOURCE_ID_BYTES: usize = 256;
pub const MAX_OBSERVATION_ID_BYTES: usize = 256;
const MAX_FIELD_DEPTH: usize = 32;
const MAX_FIELD_ITEMS: usize = 4096;
const MAX_FIELD_KEY_BYTES: usize = 256;

const RESERVED_FIELDS: &[&str] = &[
    "source",
    "owner",
    "owner_plugin_id",
    "plugin_id",
    "publisher",
    "publisher_fingerprint",
    "profile_id",
    "provenance",
    "record_id",
    "dataset_id",
    "schema_id",
    "schema_version",
    "revision",
    "published_at",
    "expires_at",
    "creator",
    "permissions",
    "scope",
    "executable",
    "executable_handle",
    "argv",
    "cwd",
    "path",
    "launch_target",
    "operation",
    "command",
    "grant",
    "permission",
];

/// A host-resolved publication. `owner` must come from the authenticated producer session, and
/// callers must validate publication permission before calling [`DataBroker::publish`].
#[derive(Clone, Debug)]
pub struct Publication {
    pub owner: PluginIdentity,
    pub dataset_id: String,
    pub category: Option<CommonDataCategory>,
    pub class: SharingClass,
    pub schema_id: String,
    pub schema_version: u32,
    pub fields: Map<String, Value>,
    /// Optional schema payload. A record carries either structured fields or opaque bytes.
    pub opaque_payload: Vec<u8>,
    /// Producer observation metadata. Sequence streams are scoped to the authenticated owner,
    /// dataset and resource, never to plugin-supplied identity fields.
    pub resource_id: String,
    pub observed_at: i64,
    pub sequence: u64,
    pub source_observation_id: String,
    /// Host session that owns this session-retained record; absent only for host-fed projections.
    pub session_id: Option<String>,
    /// Identity details resolved from the signed package and active authenticated session.
    pub publisher: Option<PublisherMetadata>,
    pub expires_at: Option<i64>,
}

#[derive(Clone, Debug, Default)]
pub struct PublisherMetadata {
    pub declared_developer: String,
    pub origin: String,
    pub package_version: String,
    pub package_digest: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicationReceipt {
    pub record_id: String,
    pub revision: u64,
    pub duplicate: bool,
}

/// Provenance is kept outside the plugin field map and only extended by the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provenance {
    pub source: PluginIdentity,
    pub forwarded_by: Vec<PluginIdentity>,
    pub declared_developer: String,
    pub origin: String,
    pub package_version: String,
    pub package_digest: Vec<u8>,
    pub source_record_ids: Vec<String>,
}

#[derive(Clone, Debug)]
struct StoredRecord {
    record_id: String,
    owner: PluginIdentity,
    dataset_id: String,
    category: Option<CommonDataCategory>,
    class: SharingClass,
    schema_id: String,
    schema_version: u32,
    fields: Map<String, Value>,
    opaque_payload: Vec<u8>,
    resource_id: String,
    observed_at: i64,
    received_at: i64,
    sequence: u64,
    source_observation_id: String,
    session_id: Option<String>,
    published_at: i64,
    expires_at: Option<i64>,
    revision: u64,
    provenance: Provenance,
    bytes: usize,
}

/// A projected result. Its fields and provenance are private so an RPC adapter cannot accept a
/// plugin-constructed projection as an onward-publication proof.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectedRecord {
    record_id: String,
    dataset_id: String,
    category: Option<CommonDataCategory>,
    schema_id: String,
    schema_version: u32,
    fields: Map<String, Value>,
    opaque_payload: Vec<u8>,
    resource_id: String,
    observed_at: i64,
    received_at: i64,
    sequence: u64,
    source_observation_id: String,
    published_at: i64,
    revision: u64,
    expires_at: Option<i64>,
    provenance: Provenance,
    recipient: PluginIdentity,
    policy_revision: u64,
}

impl ProjectedRecord {
    pub fn record_id(&self) -> &str {
        &self.record_id
    }

    pub fn dataset_id(&self) -> &str {
        &self.dataset_id
    }

    pub fn category(&self) -> Option<CommonDataCategory> {
        self.category
    }

    pub fn schema(&self) -> (&str, u32) {
        (&self.schema_id, self.schema_version)
    }

    pub fn fields(&self) -> &Map<String, Value> {
        &self.fields
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn published_at(&self) -> i64 {
        self.published_at
    }

    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    pub fn opaque_payload(&self) -> &[u8] {
        &self.opaque_payload
    }

    pub fn resource_id(&self) -> &str {
        &self.resource_id
    }

    pub fn observed_at(&self) -> i64 {
        self.observed_at
    }

    pub fn received_at(&self) -> i64 {
        self.received_at
    }

    pub fn expires_at(&self) -> Option<i64> {
        self.expires_at
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn source_observation_id(&self) -> &str {
        &self.source_observation_id
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueryResult {
    pub records: Vec<ProjectedRecord>,
    pub next_cursor: Option<QueryCursor>,
    /// Broker-wide revision used to bind the next page and a subsequent watch.
    pub revision: u64,
}

/// Opaque in-process cursor bound to caller, query, data snapshot, and current policy revision.
/// A protocol adapter must encode it as a host-authenticated token before crossing RPC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryCursor {
    caller: PluginIdentity,
    dataset_id: String,
    record_id: Option<String>,
    category: Option<CommonDataCategory>,
    selector: QuerySelector,
    delivery: Delivery,
    snapshot_revision: u64,
    after_revision: u64,
    policy_revision: u64,
}

#[derive(Clone, Debug)]
pub struct QueryRequest {
    pub caller: PluginIdentity,
    pub dataset_id: String,
    /// Optional exact record lookup. Used by Data.Get so owner reads select one record too.
    pub record_id: Option<String>,
    pub category: Option<CommonDataCategory>,
    /// `None` and an empty selector both mean owner-private records only.
    pub selector: Option<CommonDataSelector>,
    pub delivery: Delivery,
    pub page_size: usize,
    pub cursor: Option<QueryCursor>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct QuerySelector {
    categories: BTreeSet<CommonDataCategory>,
    record_ids: BTreeSet<String>,
}

impl QuerySelector {
    fn decode(selector: Option<&CommonDataSelector>) -> Result<Self, BrokerError> {
        let Some(selector) = selector else {
            return Ok(Self::default());
        };
        if selector.categories.len() > 64 || selector.selected_record_ids.len() > 1000 {
            return Err(BrokerError::InvalidSelector);
        }
        let mut decoded = Self::default();
        for raw in &selector.categories {
            let category =
                CommonDataCategory::try_from(*raw).map_err(|_| BrokerError::InvalidSelector)?;
            if category == CommonDataCategory::Unspecified || !decoded.categories.insert(category) {
                return Err(BrokerError::InvalidSelector);
            }
        }
        for record_id in &selector.selected_record_ids {
            if record_id.trim().is_empty()
                || record_id.len() > 256
                || !decoded.record_ids.insert(record_id.clone())
            {
                return Err(BrokerError::InvalidSelector);
            }
        }
        Ok(decoded)
    }

    fn scopes(&self, record: &StoredRecord) -> Vec<AccessScope> {
        // Category and selected-record grants are independent. If both are selected and the
        // category grant denies, the record selector still gets its own authorization check.
        let mut scopes = Vec::with_capacity(2);
        if record.class == SharingClass::Common {
            if record
                .category
                .is_some_and(|category| self.categories.contains(&category))
            {
                scopes.push(AccessScope::Category(
                    record.category.expect("checked above"),
                ));
            }
        }
        if self.record_ids.contains(&record.record_id) {
            scopes.push(AccessScope::SelectedRecord(record.record_id.clone()));
        }
        if record.class != SharingClass::Common {
            // A Data/domain request names its dataset explicitly. Sensitive/custom policies may
            // authorize that exact dataset; a Common record never gets a dataset-wide shortcut.
            scopes.push(AccessScope::Dataset(record.dataset_id.clone()));
        }
        scopes
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum BrokerError {
    #[error("publication identity or dataset is invalid")]
    InvalidPublication,
    #[error("publication sequence is duplicate or out of order; current sequence is {0}")]
    SequenceConflict(u64),
    #[error("publication payload contains a host-owned field")]
    ReservedField,
    #[error("publication exceeds the size limit")]
    RecordTooLarge,
    #[error("shared-data retention quota is exhausted")]
    QuotaExceeded,
    #[error("common-data selector is malformed")]
    InvalidSelector,
    #[error("cursor is stale or belongs to another query")]
    StaleCursor,
    #[error("current authorization changed while the query was running")]
    AuthorizationChanged,
    #[error("subscription cursor must match the current broker revision")]
    StaleSubscription,
    #[error("subscriber is not the authenticated recipient of this projection")]
    WrongRecipient,
    #[error("projection has expired")]
    Expired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubscriptionEnd {
    Revoked,
    FellBehind,
}

struct BrokerState {
    revision: u64,
    bytes: usize,
    rows: VecDeque<StoredRecord>,
    sequences: HashMap<PublicationStream, SequenceCursor>,
    subscribers: Vec<Weak<Mutex<SubscriptionState>>>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct PublicationStream {
    owner: String,
    publisher_fingerprint: String,
    profile: String,
    dataset_id: String,
    resource_id: String,
}

#[derive(Clone, Debug)]
struct SequenceCursor {
    sequence: u64,
    record_id: String,
    session_id: Option<String>,
}

struct SubscriptionState {
    request: QueryRequest,
    selector: QuerySelector,
    authorizer: Arc<dyn Authorizer>,
    policy_revision: u64,
    after_revision: u64,
    queue_capacity: usize,
    queue: VecDeque<ProjectedRecord>,
    ended: Option<SubscriptionEnd>,
    notify: Arc<tokio::sync::Notify>,
}

/// Bounded in-memory broker. Persistent domain/telemetry stores can feed records here after
/// resolving the producer identity and before returning shared projections.
pub struct DataBroker {
    state: Mutex<BrokerState>,
}

impl Default for DataBroker {
    fn default() -> Self {
        Self::new()
    }
}

impl DataBroker {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(BrokerState {
                revision: 0,
                bytes: 0,
                rows: VecDeque::new(),
                sequences: HashMap::new(),
                subscribers: Vec::new(),
            }),
        }
    }

    /// Stores a host-authenticated publication. The ID, revision, timestamp and provenance are
    /// assigned by the broker; payload fields cannot overwrite them.
    pub fn publish(
        &self,
        publication: Publication,
        now: i64,
    ) -> Result<PublicationReceipt, BrokerError> {
        let (record, subscribers) = {
            let mut state = self.lock();
            let (record, duplicate) = Self::insert(&mut state, publication, None, now)?;
            if duplicate {
                return Ok(PublicationReceipt {
                    record_id: record.record_id,
                    revision: record.revision,
                    duplicate: true,
                });
            }
            let subscribers = state.subscribers.clone();
            (record, subscribers)
        };
        self.dispatch(&record, subscribers, now);
        Ok(PublicationReceipt {
            record_id: record.record_id,
            revision: record.revision,
            duplicate: false,
        })
    }

    /// Republishes only fields that the recipient actually received. Provenance remains source
    /// attributed and the current authenticated publisher is appended by the host.
    pub fn publish_onward(
        &self,
        publisher: &PluginIdentity,
        projected: &ProjectedRecord,
        authorizer: &dyn Authorizer,
        now: i64,
    ) -> Result<PublicationReceipt, BrokerError> {
        if &projected.recipient != publisher {
            return Err(BrokerError::WrongRecipient);
        }
        if authorizer.revision(publisher) != projected.policy_revision {
            return Err(BrokerError::AuthorizationChanged);
        }
        if projected.expires_at.is_some_and(|expiry| expiry <= now) {
            return Err(BrokerError::Expired);
        }
        let mut provenance = projected.provenance.clone();
        if provenance.forwarded_by.last() != Some(publisher) {
            provenance.forwarded_by.push(publisher.clone());
        }
        provenance
            .source_record_ids
            .push(projected.record_id.clone());
        let publication = Publication {
            owner: publisher.clone(),
            dataset_id: projected.dataset_id.clone(),
            category: projected.category,
            class: SharingClass::Custom,
            schema_id: projected.schema_id.clone(),
            schema_version: projected.schema_version,
            fields: projected.fields.clone(),
            opaque_payload: projected.opaque_payload.clone(),
            resource_id: projected.resource_id.clone(),
            observed_at: projected.observed_at,
            sequence: projected.sequence,
            source_observation_id: projected.source_observation_id.clone(),
            session_id: None,
            publisher: None,
            expires_at: projected.expires_at,
        };
        let (record, subscribers) = {
            let mut state = self.lock();
            let (record, duplicate) = Self::insert(&mut state, publication, Some(provenance), now)?;
            if duplicate {
                return Ok(PublicationReceipt {
                    record_id: record.record_id,
                    revision: record.revision,
                    duplicate: true,
                });
            }
            let subscribers = state.subscribers.clone();
            (record, subscribers)
        };
        self.dispatch(&record, subscribers, now);
        Ok(PublicationReceipt {
            record_id: record.record_id,
            revision: record.revision,
            duplicate: false,
        })
    }

    /// Domain and Data aliases deliberately share the same authorization and projection path.
    pub fn query_domain(
        &self,
        request: QueryRequest,
        authorizer: &dyn Authorizer,
        now: i64,
    ) -> Result<QueryResult, BrokerError> {
        self.query(request, authorizer, now)
    }

    pub fn query_data(
        &self,
        request: QueryRequest,
        authorizer: &dyn Authorizer,
        now: i64,
    ) -> Result<QueryResult, BrokerError> {
        self.query(request, authorizer, now)
    }

    fn query(
        &self,
        request: QueryRequest,
        authorizer: &dyn Authorizer,
        now: i64,
    ) -> Result<QueryResult, BrokerError> {
        validate_query(&request)?;
        if request.delivery == Delivery::Live {
            return Err(BrokerError::InvalidPublication);
        }
        let selector = QuerySelector::decode(request.selector.as_ref())?;
        let policy_revision = authorizer.revision(&request.caller);
        let (snapshot_revision, after_revision, rows) = {
            let mut state = self.lock();
            prune_expired_locked(&mut state, now);
            let snapshot = state.revision;
            let after = match &request.cursor {
                Some(cursor) => {
                    if cursor.caller != request.caller
                        || cursor.dataset_id != request.dataset_id
                        || cursor.record_id != request.record_id
                        || cursor.category != request.category
                        || cursor.selector != selector
                        || cursor.delivery != request.delivery
                        || cursor.snapshot_revision != snapshot
                        || cursor.policy_revision != policy_revision
                    {
                        return Err(BrokerError::StaleCursor);
                    }
                    cursor.after_revision
                }
                None => 0,
            };
            let rows = state
                .rows
                .iter()
                .filter(|record| {
                    record.revision > after
                        && record.revision <= snapshot
                        && record.dataset_id == request.dataset_id
                        && request
                            .record_id
                            .as_ref()
                            .is_none_or(|record_id| &record.record_id == record_id)
                        && request
                            .category
                            .is_none_or(|category| record.category == Some(category))
                        && record.expires_at.is_none_or(|expiry| expiry > now)
                })
                .cloned()
                .collect::<Vec<_>>();
            (snapshot, after, rows)
        };

        let mut projected = Vec::new();
        for record in rows {
            if let Some(result) = project_for_query(
                &request.caller,
                &selector,
                &record,
                request.delivery,
                policy_revision,
                authorizer,
            )? {
                projected.push(result);
            }
        }
        if authorizer.revision(&request.caller) != policy_revision {
            return Err(BrokerError::AuthorizationChanged);
        }

        let page_size = request.page_size.clamp(1, MAX_PAGE_SIZE);
        let mut after = after_revision;
        let mut iter = projected
            .into_iter()
            .filter(|record| record.revision > after_revision);
        let mut records = Vec::with_capacity(page_size);
        for _ in 0..page_size {
            if let Some(record) = iter.next() {
                after = record.revision;
                records.push(record);
            } else {
                break;
            }
        }
        let has_more = iter.next().is_some();
        let next_cursor = has_more.then(|| QueryCursor {
            caller: request.caller.clone(),
            dataset_id: request.dataset_id.clone(),
            record_id: request.record_id.clone(),
            category: request.category,
            selector,
            delivery: request.delivery,
            snapshot_revision,
            after_revision: after,
            policy_revision,
        });
        Ok(QueryResult {
            records,
            next_cursor,
            revision: snapshot_revision,
        })
    }

    /// Opens a live subscription from the exact current broker revision. `after_revision == 0`
    /// starts live-only at the current snapshot; nonzero stale/future positions are fenced.
    pub fn subscribe(
        &self,
        request: QueryRequest,
        after_revision: u64,
        queue_capacity: usize,
        authorizer: Arc<dyn Authorizer>,
    ) -> Result<Subscription, BrokerError> {
        validate_query(&request)?;
        let selector = QuerySelector::decode(request.selector.as_ref())?;
        let policy_revision = authorizer.revision(&request.caller);
        let mut broker_state = self.lock();
        if after_revision != 0 && after_revision != broker_state.revision {
            return Err(BrokerError::StaleSubscription);
        }
        let after_revision = broker_state.revision;
        let mut request = request;
        request.delivery = Delivery::Live;
        request.cursor = None;
        let inner = Arc::new(Mutex::new(SubscriptionState {
            request,
            selector,
            authorizer: authorizer.clone(),
            policy_revision,
            after_revision,
            queue_capacity: queue_capacity.clamp(1, MAX_SUBSCRIPTION_QUEUE),
            queue: VecDeque::new(),
            ended: None,
            notify: Arc::new(tokio::sync::Notify::new()),
        }));
        broker_state.subscribers.push(Arc::downgrade(&inner));
        drop(broker_state);
        Ok(Subscription { inner, authorizer })
    }

    /// Expire retained records and advance the revision fence if anything changed.
    pub fn expire(&self, now: i64) -> usize {
        let mut state = self.lock();
        prune_expired_locked(&mut state, now)
    }

    /// Removes session-only publications before a disconnected producer can leave data behind.
    /// Existing watches are fenced so no queued copy can outlive the producer session.
    pub fn close_session(&self, session_id: &str) -> usize {
        if session_id.is_empty() {
            return 0;
        }
        let mut state = self.lock();
        let before = state.rows.len();
        let removed_bytes = state
            .rows
            .iter()
            .filter(|record| record.session_id.as_deref() == Some(session_id))
            .map(|record| record.bytes)
            .sum::<usize>();
        state
            .rows
            .retain(|record| record.session_id.as_deref() != Some(session_id));
        state.bytes = state.bytes.saturating_sub(removed_bytes);
        state
            .sequences
            .retain(|_, cursor| cursor.session_id.as_deref() != Some(session_id));
        let removed = before - state.rows.len();
        if removed == 0 {
            return 0;
        }
        state.revision = state.revision.saturating_add(1);
        for subscriber in &state.subscribers {
            if let Some(inner) = subscriber.upgrade() {
                let mut subscription = lock(&inner);
                subscription.queue.clear();
                subscription.ended = Some(SubscriptionEnd::FellBehind);
                subscription.notify.notify_waiters();
            }
        }
        removed
    }

    fn insert(
        state: &mut BrokerState,
        publication: Publication,
        onward: Option<Provenance>,
        now: i64,
    ) -> Result<(StoredRecord, bool), BrokerError> {
        validate_publication(&publication, now)?;
        prune_expired_locked(state, now);
        let stream = (publication.sequence > 0).then(|| PublicationStream {
            owner: publication.owner.plugin_id.clone(),
            publisher_fingerprint: publication.owner.publisher_fingerprint.clone(),
            profile: publication.owner.profile_id.clone(),
            dataset_id: publication.dataset_id.clone(),
            resource_id: publication.resource_id.clone(),
        });
        if let Some(stream) = &stream {
            let current_sequence = state.sequences.get(stream).map(|cursor| cursor.sequence);
            let prior_observation = state.rows.iter().find(|record| {
                record.owner.plugin_id == publication.owner.plugin_id
                    && record.owner.publisher_fingerprint == publication.owner.publisher_fingerprint
                    && record.owner.profile_id == publication.owner.profile_id
                    && record.dataset_id == publication.dataset_id
                    && record.resource_id == publication.resource_id
                    && record.source_observation_id == publication.source_observation_id
            });
            if let Some(prior) = prior_observation {
                if same_observation(prior, &publication) {
                    return Ok((prior.clone(), true));
                }
                return Err(BrokerError::SequenceConflict(
                    current_sequence.unwrap_or(prior.sequence),
                ));
            }
            if let Some(cursor) = state.sequences.get(stream)
                && publication.sequence <= cursor.sequence
            {
                return Err(BrokerError::SequenceConflict(cursor.sequence));
            }
        }
        let bytes =
            publication_bytes(&publication)?.saturating_add(provenance_bytes(onward.as_ref()));
        if bytes > MAX_RECORD_BYTES {
            return Err(BrokerError::RecordTooLarge);
        }
        if state.rows.len() >= MAX_RECORDS || state.bytes.saturating_add(bytes) > MAX_RETAINED_BYTES
        {
            return Err(BrokerError::QuotaExceeded);
        }
        state.revision = state.revision.saturating_add(1);
        let revision = state.revision;
        let owner = publication.owner;
        let publisher = publication.publisher.clone().unwrap_or_default();
        let record = StoredRecord {
            record_id: format!("shared-{revision:016x}"),
            provenance: onward.unwrap_or_else(|| Provenance {
                source: owner.clone(),
                forwarded_by: Vec::new(),
                declared_developer: publisher.declared_developer,
                origin: publisher.origin,
                package_version: publisher.package_version,
                package_digest: publisher.package_digest,
                source_record_ids: Vec::new(),
            }),
            owner,
            dataset_id: publication.dataset_id,
            category: publication.category,
            class: publication.class,
            schema_id: publication.schema_id,
            schema_version: publication.schema_version,
            fields: publication.fields,
            opaque_payload: publication.opaque_payload,
            resource_id: publication.resource_id,
            observed_at: publication.observed_at,
            received_at: now,
            sequence: publication.sequence,
            source_observation_id: publication.source_observation_id,
            session_id: publication.session_id,
            published_at: now,
            expires_at: publication.expires_at,
            revision,
            bytes,
        };
        state.bytes = state.bytes.saturating_add(bytes);
        state.rows.push_back(record.clone());
        if let Some(stream) = stream {
            state.sequences.insert(
                stream,
                SequenceCursor {
                    sequence: record.sequence,
                    record_id: record.record_id.clone(),
                    session_id: record.session_id.clone(),
                },
            );
        }
        Ok((record, false))
    }

    fn dispatch(
        &self,
        record: &StoredRecord,
        subscribers: Vec<Weak<Mutex<SubscriptionState>>>,
        now: i64,
    ) {
        for subscriber in subscribers {
            let Some(inner) = subscriber.upgrade() else {
                continue;
            };
            let mut state = lock(&inner);
            if state.ended.is_some() {
                continue;
            }
            if record.revision <= state.after_revision {
                continue;
            }
            if state
                .request
                .category
                .is_some_and(|category| record.category != Some(category))
                || state.request.dataset_id != record.dataset_id
                || record.expires_at.is_some_and(|expiry| expiry <= now)
            {
                continue;
            }
            let request = state.request.clone();
            let selector = state.selector.clone();
            let initial_revision = state.policy_revision;
            let authorizer = state.authorizer.clone();
            state.after_revision = record.revision;
            drop(state);

            if authorizer.revision(&request.caller) != initial_revision {
                let mut state = lock(&inner);
                state.queue.clear();
                state.ended = Some(SubscriptionEnd::Revoked);
                continue;
            }
            let projected = project_for_query(
                &request.caller,
                &selector,
                record,
                Delivery::Live,
                initial_revision,
                authorizer.as_ref(),
            );
            let mut state = lock(&inner);
            match projected {
                Err(_) => {
                    state.queue.clear();
                    state.ended = Some(SubscriptionEnd::Revoked);
                }
                Ok(Some(projection)) => {
                    if state.queue.len() >= state.queue_capacity {
                        state.queue.clear();
                        state.ended = Some(SubscriptionEnd::FellBehind);
                    } else {
                        state.queue.push_back(projection);
                        state.notify.notify_one();
                    }
                }
                Ok(None) => {}
            }
            if state.ended.is_some() {
                continue;
            }
        }
        // Prune dead/closed handles without replacing the vector copied by another concurrent
        // publisher or dropping subscriptions registered while this event was being projected.
        self.lock().subscribers.retain(|subscriber| {
            subscriber
                .upgrade()
                .is_some_and(|inner| lock(&inner).ended.is_none())
        });
    }

    fn lock(&self) -> MutexGuard<'_, BrokerState> {
        lock(&self.state)
    }
}

/// One bounded live stream. Calling `next` rechecks the policy revision before releasing queued
/// projections, so revocation fences already-buffered events as well as future ones.
pub struct Subscription {
    inner: Arc<Mutex<SubscriptionState>>,
    authorizer: Arc<dyn Authorizer>,
}

impl Subscription {
    pub fn next(&self, now: i64) -> Result<Option<ProjectedRecord>, SubscriptionEnd> {
        let mut state = lock(&self.inner);
        if self.authorizer.revision(&state.request.caller) != state.policy_revision {
            state.queue.clear();
            state.ended = Some(SubscriptionEnd::Revoked);
        }
        if let Some(ended) = state.ended {
            return Err(ended);
        }
        while state
            .queue
            .front()
            .is_some_and(|record| record.expires_at.is_some_and(|expiry| expiry <= now))
        {
            state.queue.pop_front();
        }
        Ok(state.queue.pop_front())
    }

    pub fn is_ended(&self) -> Option<SubscriptionEnd> {
        lock(&self.inner).ended
    }

    /// Waits for one event without blocking a worker thread. The caller should also recheck
    /// provider-session grants on its existing periodic authorization interval.
    pub async fn recv(&self, now: i64) -> Result<Option<ProjectedRecord>, SubscriptionEnd> {
        loop {
            let notify = lock(&self.inner).notify.clone();
            let notified = notify.notified();
            if let Some(record) = self.next(now)? {
                return Ok(Some(record));
            }
            notified.await;
        }
    }
}

fn project_for_query(
    caller: &PluginIdentity,
    selector: &QuerySelector,
    record: &StoredRecord,
    delivery: Delivery,
    policy_revision: u64,
    authorizer: &dyn Authorizer,
) -> Result<Option<ProjectedRecord>, BrokerError> {
    let mut available: BTreeSet<String> = record.fields.keys().cloned().collect();
    if !record.opaque_payload.is_empty() {
        available.insert("opaque_payload".to_owned());
    }
    let scopes = if &record.owner == caller {
        vec![AccessScope::Owner]
    } else {
        selector.scopes(record)
    };
    if scopes.is_empty() {
        return Ok(None);
    }
    let mut selected_decision = None;
    for scope in scopes {
        let decision = if scope == AccessScope::Owner {
            AccessDecision {
                allowed: true,
                fields: available.clone(),
                history: true,
                live: true,
                revision: policy_revision,
            }
        } else {
            let request = AccessRequest {
                recipient: caller,
                source: &record.owner,
                record_id: &record.record_id,
                dataset_id: &record.dataset_id,
                category: record.category,
                scope,
                class: record.class,
                fields: &available,
                delivery,
            };
            authorizer.decide(&request)
        };
        if decision.revision != policy_revision {
            return Err(BrokerError::AuthorizationChanged);
        }
        let has_visible_field = available
            .iter()
            .any(|field| decision.fields.contains(field));
        if decision.allowed
            && has_visible_field
            && (delivery != Delivery::History || decision.history)
            && (delivery != Delivery::Live || decision.live)
        {
            selected_decision = Some(decision);
            break;
        }
    }
    let Some(decision) = selected_decision else {
        return Ok(None);
    };
    let fields: Map<String, Value> = record
        .fields
        .iter()
        .filter(|(key, _)| decision.fields.contains(*key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let opaque_payload = if decision.fields.contains("opaque_payload") {
        record.opaque_payload.clone()
    } else {
        Vec::new()
    };
    if fields.is_empty() && opaque_payload.is_empty() {
        return Ok(None);
    }
    Ok(Some(ProjectedRecord {
        record_id: record.record_id.clone(),
        dataset_id: record.dataset_id.clone(),
        category: record.category,
        schema_id: record.schema_id.clone(),
        schema_version: record.schema_version,
        fields,
        opaque_payload,
        resource_id: record.resource_id.clone(),
        observed_at: record.observed_at,
        received_at: record.received_at,
        sequence: record.sequence,
        source_observation_id: record.source_observation_id.clone(),
        published_at: record.published_at,
        revision: record.revision,
        expires_at: record.expires_at,
        provenance: record.provenance.clone(),
        recipient: caller.clone(),
        policy_revision,
    }))
}

fn validate_query(request: &QueryRequest) -> Result<(), BrokerError> {
    if request.dataset_id.trim().is_empty()
        || request.dataset_id.len() > 256
        || request.dataset_id.chars().any(char::is_control)
    {
        return Err(BrokerError::InvalidPublication);
    }
    Ok(())
}

fn retain_live_sequences(state: &mut BrokerState) {
    let live_records: HashSet<&str> = state
        .rows
        .iter()
        .map(|record| record.record_id.as_str())
        .collect();
    state
        .sequences
        .retain(|_, cursor| live_records.contains(cursor.record_id.as_str()));
}

fn prune_expired_locked(state: &mut BrokerState, now: i64) -> usize {
    let before = state.rows.len();
    let expired_bytes = state
        .rows
        .iter()
        .filter(|record| record.expires_at.is_some_and(|expiry| expiry <= now))
        .map(|record| record.bytes)
        .sum::<usize>();
    state
        .rows
        .retain(|record| record.expires_at.is_none_or(|expiry| expiry > now));
    retain_live_sequences(state);
    state.bytes = state.bytes.saturating_sub(expired_bytes);
    let removed = before - state.rows.len();
    if removed > 0 {
        state.revision = state.revision.saturating_add(1);
    }
    removed
}

fn publication_bytes(publication: &Publication) -> Result<usize, BrokerError> {
    let fields = serde_json::to_vec(&publication.fields)
        .map_err(|_| BrokerError::InvalidPublication)?
        .len();
    let mut bytes = fields
        .saturating_add(publication.opaque_payload.len())
        .saturating_add(publication.owner.plugin_id.len())
        .saturating_add(publication.owner.publisher_fingerprint.len())
        .saturating_add(publication.owner.profile_id.len())
        .saturating_add(publication.dataset_id.len())
        .saturating_add(publication.schema_id.len())
        .saturating_add(publication.resource_id.len())
        .saturating_add(publication.source_observation_id.len())
        .saturating_add(128); // Protobuf tags, timestamps, counters, enum and host record ID.
    if let Some(session_id) = &publication.session_id {
        bytes = bytes.saturating_add(session_id.len());
    }
    if let Some(publisher) = &publication.publisher {
        bytes = bytes
            .saturating_add(publisher.declared_developer.len())
            .saturating_add(publisher.origin.len())
            .saturating_add(publisher.package_version.len())
            .saturating_add(publisher.package_digest.len());
    }
    Ok(bytes)
}

fn provenance_bytes(provenance: Option<&Provenance>) -> usize {
    let Some(provenance) = provenance else {
        return 0;
    };
    let identity_bytes = |identity: &PluginIdentity| {
        identity
            .plugin_id
            .len()
            .saturating_add(identity.publisher_fingerprint.len())
            .saturating_add(identity.profile_id.len())
    };
    provenance
        .forwarded_by
        .iter()
        .map(identity_bytes)
        .chain(provenance.source_record_ids.iter().map(String::len))
        .fold(
            provenance
                .source
                .plugin_id
                .len()
                .saturating_add(provenance.source.publisher_fingerprint.len())
                .saturating_add(provenance.source.profile_id.len())
                .saturating_add(provenance.declared_developer.len())
                .saturating_add(provenance.origin.len())
                .saturating_add(provenance.package_version.len())
                .saturating_add(provenance.package_digest.len()),
            usize::saturating_add,
        )
}

fn same_observation(record: &StoredRecord, publication: &Publication) -> bool {
    record.owner == publication.owner
        && record.dataset_id == publication.dataset_id
        && record.schema_id == publication.schema_id
        && record.schema_version == publication.schema_version
        && record.fields == publication.fields
        && record.opaque_payload == publication.opaque_payload
        && record.resource_id == publication.resource_id
        && record.observed_at == publication.observed_at
        && record.sequence == publication.sequence
        && record.source_observation_id == publication.source_observation_id
}

fn validate_publication(publication: &Publication, now: i64) -> Result<(), BrokerError> {
    let structured = !publication.fields.is_empty();
    let opaque = !publication.opaque_payload.is_empty();
    if publication.owner.validate().is_err()
        || publication.dataset_id.trim().is_empty()
        || publication.dataset_id.len() > 256
        || publication.dataset_id.chars().any(char::is_control)
        || publication.schema_id.trim().is_empty()
        || publication.schema_id.len() > 256
        || publication.schema_id.chars().any(char::is_control)
        || publication.schema_version == 0
        || (publication.publisher.is_some()
            && !publication
                .schema_id
                .starts_with(&format!("{}.", publication.dataset_id))
            && publication.schema_id != publication.dataset_id)
        || now <= 0
        || publication.expires_at.is_some_and(|expiry| expiry <= now)
        || publication
            .expires_at
            .is_some_and(|expiry| expiry.saturating_sub(now) > MAX_TTL_SECONDS as i64)
        || (structured == opaque)
        || (publication.class == SharingClass::Common && publication.category.is_none())
    {
        return Err(BrokerError::InvalidPublication);
    }
    if publication.publisher.is_some() && publication.sequence == 0 {
        return Err(BrokerError::InvalidPublication);
    }
    if publication
        .fields
        .keys()
        .any(|key| RESERVED_FIELDS.contains(&key.as_str()))
    {
        return Err(BrokerError::ReservedField);
    }
    if publication.sequence > 0
        && (publication.resource_id.trim().is_empty()
            || publication.resource_id.len() > MAX_RESOURCE_ID_BYTES
            || publication.resource_id.chars().any(char::is_control)
            || publication.source_observation_id.trim().is_empty()
            || publication.source_observation_id.len() > MAX_OBSERVATION_ID_BYTES
            || publication
                .source_observation_id
                .chars()
                .any(char::is_control)
            || publication.observed_at < 0)
    {
        return Err(BrokerError::InvalidPublication);
    }
    if let Some(session_id) = &publication.session_id
        && (session_id.trim().is_empty()
            || session_id.len() > 128
            || session_id.chars().any(char::is_control))
    {
        return Err(BrokerError::InvalidPublication);
    }
    if let Some(publisher) = &publication.publisher
        && (publisher.package_digest.len() != 32
            || publisher.declared_developer.trim().is_empty()
            || publisher.declared_developer.len() > 256
            || publisher.origin.trim().is_empty()
            || publisher.origin.len() > 512
            || publisher.package_version.trim().is_empty()
            || publisher.package_version.len() > 128)
    {
        return Err(BrokerError::InvalidPublication);
    }
    let mut item_count = 0;
    validate_field_object(&publication.fields, 1, &mut item_count)?;
    let bytes = publication_bytes(publication)?;
    if bytes > MAX_RECORD_BYTES {
        return Err(BrokerError::RecordTooLarge);
    }
    Ok(())
}

fn validate_field_object(
    fields: &Map<String, Value>,
    depth: usize,
    item_count: &mut usize,
) -> Result<(), BrokerError> {
    if depth > MAX_FIELD_DEPTH {
        return Err(BrokerError::InvalidPublication);
    }
    for (key, value) in fields {
        *item_count += 1;
        if *item_count > MAX_FIELD_ITEMS
            || key.is_empty()
            || key.len() > MAX_FIELD_KEY_BYTES
            || key.chars().any(char::is_control)
        {
            return Err(BrokerError::InvalidPublication);
        }
        validate_field_value(value, depth + 1, item_count)?;
    }
    Ok(())
}

fn validate_field_value(
    value: &Value,
    depth: usize,
    item_count: &mut usize,
) -> Result<(), BrokerError> {
    if depth > MAX_FIELD_DEPTH {
        return Err(BrokerError::InvalidPublication);
    }
    match value {
        Value::Object(fields) => validate_field_object(fields, depth, item_count),
        Value::Array(values) => {
            for child in values {
                *item_count += 1;
                if *item_count > MAX_FIELD_ITEMS {
                    return Err(BrokerError::InvalidPublication);
                }
                validate_field_value(child, depth + 1, item_count)?;
            }
            Ok(())
        }
        Value::Number(number) if number.as_f64().is_none_or(|number| !number.is_finite()) => {
            Err(BrokerError::InvalidPublication)
        }
        _ => Ok(()),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TestAuthorizer {
        revision: AtomicU64,
        categories: BTreeSet<CommonDataCategory>,
        record_ids: BTreeSet<String>,
        classes: BTreeSet<SharingClass>,
        fields: BTreeSet<String>,
        history: bool,
        live: bool,
    }

    impl TestAuthorizer {
        fn new(
            categories: impl IntoIterator<Item = CommonDataCategory>,
            record_ids: impl IntoIterator<Item = String>,
            classes: impl IntoIterator<Item = SharingClass>,
            fields: impl IntoIterator<Item = String>,
            history: bool,
            live: bool,
        ) -> Self {
            Self {
                revision: AtomicU64::new(1),
                categories: categories.into_iter().collect(),
                record_ids: record_ids.into_iter().collect(),
                classes: classes.into_iter().collect(),
                fields: fields.into_iter().collect(),
                history,
                live,
            }
        }
    }

    impl Authorizer for TestAuthorizer {
        fn revision(&self, _recipient: &PluginIdentity) -> u64 {
            self.revision.load(Ordering::SeqCst)
        }

        fn decide(&self, request: &AccessRequest<'_>) -> AccessDecision {
            let scope_allowed = match &request.scope {
                AccessScope::Owner => false,
                AccessScope::Category(category) => {
                    request.class == SharingClass::Common && self.categories.contains(category)
                }
                AccessScope::SelectedRecord(record_id) => {
                    request.class == SharingClass::Common && self.record_ids.contains(record_id)
                }
                AccessScope::Dataset(_) => false,
            };
            let fields = request
                .fields
                .intersection(&self.fields)
                .cloned()
                .collect::<BTreeSet<_>>();
            let allowed =
                scope_allowed && self.classes.contains(&request.class) && !fields.is_empty();
            AccessDecision {
                allowed,
                fields,
                history: allowed && self.history,
                live: allowed && self.live,
                revision: self.revision(request.recipient),
            }
        }
    }

    fn identity(id: &str) -> PluginIdentity {
        PluginIdentity {
            plugin_id: format!("org.example.{id}"),
            publisher_fingerprint: format!("SHA256:{id}"),
            profile_id: "profile-main".into(),
        }
    }

    fn publication(owner: &PluginIdentity, fields: &[(&str, Value)]) -> Publication {
        Publication {
            owner: owner.clone(),
            dataset_id: "performance.v1".into(),
            category: Some(CommonDataCategory::GamePerformanceHistory),
            class: SharingClass::Common,
            schema_id: "org.example.performance.v1".into(),
            schema_version: 1,
            fields: fields
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect(),
            opaque_payload: Vec::new(),
            resource_id: String::new(),
            observed_at: 0,
            sequence: 0,
            source_observation_id: String::new(),
            session_id: None,
            publisher: None,
            expires_at: None,
        }
    }

    fn sequenced_publication(
        owner: &PluginIdentity,
        sequence: u64,
        observation_id: &str,
        session_id: &str,
        fields: &[(&str, Value)],
    ) -> Publication {
        let mut publication = publication(owner, fields);
        publication.dataset_id = "org.example.metrics.v1".into();
        publication.schema_id = "org.example.metrics.v1".into();
        publication.resource_id = "runtime-main".into();
        publication.observed_at = 1_700_000_000;
        publication.sequence = sequence;
        publication.source_observation_id = observation_id.into();
        publication.session_id = Some(session_id.into());
        publication.publisher = Some(PublisherMetadata {
            declared_developer: "Example Developer".into(),
            origin: "signed-package".into(),
            package_version: "1.2.3".into(),
            package_digest: vec![7; 32],
        });
        publication
    }

    fn selector(
        categories: &[CommonDataCategory],
        record_ids: &[String],
    ) -> Option<CommonDataSelector> {
        Some(CommonDataSelector {
            categories: categories.iter().map(|category| *category as i32).collect(),
            selected_record_ids: record_ids.to_vec(),
        })
    }

    fn request(
        caller: &PluginIdentity,
        selector: Option<CommonDataSelector>,
        delivery: Delivery,
        page_size: usize,
        cursor: Option<QueryCursor>,
    ) -> QueryRequest {
        QueryRequest {
            caller: caller.clone(),
            dataset_id: "performance.v1".into(),
            record_id: None,
            category: Some(CommonDataCategory::GamePerformanceHistory),
            selector,
            delivery,
            page_size,
            cursor,
        }
    }

    fn authorizer(
        categories: impl IntoIterator<Item = CommonDataCategory>,
        record_ids: impl IntoIterator<Item = String>,
        classes: impl IntoIterator<Item = SharingClass>,
        fields: impl IntoIterator<Item = String>,
        history: bool,
        live: bool,
    ) -> Arc<TestAuthorizer> {
        Arc::new(TestAuthorizer::new(
            categories, record_ids, classes, fields, history, live,
        ))
    }

    #[test]
    fn denied_common_query_keeps_owner_private_records_available() {
        let broker = DataBroker::new();
        let consumer = identity("consumer");
        let producer = identity("producer");
        broker
            .publish(
                publication(&consumer, &[("fps", serde_json::json!(60))]),
                100,
            )
            .unwrap();
        broker
            .publish(
                publication(&producer, &[("fps", serde_json::json!(120))]),
                100,
            )
            .unwrap();
        let authorizer = authorizer(
            [],
            [],
            [SharingClass::Common],
            ["fps".to_owned()],
            true,
            true,
        );

        let result = broker
            .query_data(
                request(
                    &consumer,
                    selector(&[CommonDataCategory::GamePerformanceHistory], &[]),
                    Delivery::Query,
                    10,
                    None,
                ),
                authorizer.as_ref(),
                101,
            )
            .unwrap();
        assert_eq!(result.records.len(), 1);
        assert_eq!(
            result.records[0].fields().get("fps"),
            Some(&serde_json::json!(60))
        );
        assert_eq!(result.records[0].provenance().source, consumer);
    }

    #[test]
    fn category_and_selected_record_grants_are_independent_and_aliases_share_projection() {
        let broker = DataBroker::new();
        let producer = identity("producer");
        let consumer = identity("consumer");
        let receipt = broker
            .publish(
                publication(
                    &producer,
                    &[
                        ("fps", serde_json::json!(60)),
                        ("device_id", serde_json::json!("secret")),
                    ],
                ),
                100,
            )
            .unwrap();
        // Category is selected but denied. The independently selected record grant still allows
        // this exact record, and the field mask is enforced through both public aliases.
        let authorizer = authorizer(
            [],
            [receipt.record_id.clone()],
            [SharingClass::Common],
            ["fps".to_owned()],
            true,
            true,
        );
        let selected = selector(
            &[CommonDataCategory::GamePerformanceHistory],
            &[receipt.record_id],
        );
        let domain = broker
            .query_domain(
                request(&consumer, selected.clone(), Delivery::History, 10, None),
                authorizer.as_ref(),
                101,
            )
            .unwrap();
        let data = broker
            .query_data(
                request(&consumer, selected, Delivery::History, 10, None),
                authorizer.as_ref(),
                101,
            )
            .unwrap();
        for result in [&domain, &data] {
            assert_eq!(result.records.len(), 1);
            assert_eq!(result.records[0].fields().len(), 1);
            assert!(result.records[0].fields().contains_key("fps"));
            assert!(!result.records[0].fields().contains_key("device_id"));
        }
    }

    #[test]
    fn cursor_is_fenced_by_new_publications_and_policy_revision() {
        let broker = DataBroker::new();
        let producer = identity("producer");
        let consumer = identity("consumer");
        for fps in [60, 61] {
            broker
                .publish(
                    publication(&producer, &[("fps", serde_json::json!(fps))]),
                    100,
                )
                .unwrap();
        }
        let authorizer = authorizer(
            [CommonDataCategory::GamePerformanceHistory],
            [],
            [SharingClass::Common],
            ["fps".to_owned()],
            true,
            true,
        );
        let first = broker
            .query_data(
                request(
                    &consumer,
                    selector(&[CommonDataCategory::GamePerformanceHistory], &[]),
                    Delivery::History,
                    1,
                    None,
                ),
                authorizer.as_ref(),
                101,
            )
            .unwrap();
        let cursor = first.next_cursor.unwrap();
        broker
            .publish(
                publication(&producer, &[("fps", serde_json::json!(62))]),
                102,
            )
            .unwrap();
        assert_eq!(
            broker.query_data(
                request(
                    &consumer,
                    selector(&[CommonDataCategory::GamePerformanceHistory], &[]),
                    Delivery::History,
                    1,
                    Some(cursor),
                ),
                authorizer.as_ref(),
                103,
            ),
            Err(BrokerError::StaleCursor)
        );
    }

    #[test]
    fn revoked_subscription_discards_queued_events_and_is_fenced() {
        let broker = DataBroker::new();
        let producer = identity("producer");
        let consumer = identity("consumer");
        broker
            .publish(
                publication(&producer, &[("fps", serde_json::json!(60))]),
                100,
            )
            .unwrap();
        let authorizer = authorizer(
            [CommonDataCategory::GamePerformanceHistory],
            [],
            [SharingClass::Common],
            ["fps".to_owned()],
            true,
            true,
        );
        let current_revision = broker.lock().revision;
        let subscription = broker
            .subscribe(
                request(
                    &consumer,
                    selector(&[CommonDataCategory::GamePerformanceHistory], &[]),
                    Delivery::Live,
                    10,
                    None,
                ),
                current_revision,
                4,
                authorizer.clone(),
            )
            .unwrap();
        broker
            .publish(
                publication(&producer, &[("fps", serde_json::json!(61))]),
                101,
            )
            .unwrap();
        assert!(subscription.next(101).unwrap().is_some());
        broker
            .publish(
                publication(&producer, &[("fps", serde_json::json!(62))]),
                102,
            )
            .unwrap();
        authorizer.revision.fetch_add(1, Ordering::SeqCst);
        assert_eq!(subscription.next(103), Err(SubscriptionEnd::Revoked));
        assert_eq!(subscription.is_ended(), Some(SubscriptionEnd::Revoked));
    }

    #[test]
    fn bounded_live_queue_closes_instead_of_dropping_or_growing() {
        let broker = DataBroker::new();
        let producer = identity("producer");
        let consumer = identity("consumer");
        let authorizer = authorizer(
            [CommonDataCategory::GamePerformanceHistory],
            [],
            [SharingClass::Common],
            ["fps".to_owned()],
            true,
            true,
        );
        let subscription = broker
            .subscribe(
                request(
                    &consumer,
                    selector(&[CommonDataCategory::GamePerformanceHistory], &[]),
                    Delivery::Live,
                    10,
                    None,
                ),
                0,
                1,
                authorizer,
            )
            .unwrap();
        for fps in [60, 61] {
            broker
                .publish(
                    publication(&producer, &[("fps", serde_json::json!(fps))]),
                    100,
                )
                .unwrap();
        }
        assert_eq!(subscription.is_ended(), Some(SubscriptionEnd::FellBehind));
        assert_eq!(subscription.next(101), Err(SubscriptionEnd::FellBehind));
    }

    #[test]
    fn onward_publication_keeps_filtered_fields_and_source_provenance_without_common_expansion() {
        let broker = DataBroker::new();
        let producer = identity("producer");
        let consumer = identity("consumer");
        let next_consumer = identity("next-consumer");
        let receipt = broker
            .publish(
                publication(
                    &producer,
                    &[
                        ("fps", serde_json::json!(60)),
                        ("device_id", serde_json::json!("secret")),
                    ],
                ),
                100,
            )
            .unwrap();
        let auth = authorizer(
            [CommonDataCategory::GamePerformanceHistory],
            [],
            [SharingClass::Common],
            ["fps".to_owned()],
            true,
            true,
        );
        let projected = broker
            .query_data(
                request(
                    &consumer,
                    selector(&[CommonDataCategory::GamePerformanceHistory], &[]),
                    Delivery::Query,
                    10,
                    None,
                ),
                auth.as_ref(),
                101,
            )
            .unwrap()
            .records
            .into_iter()
            .find(|record| record.record_id() == receipt.record_id)
            .unwrap();
        assert_eq!(projected.fields().len(), 1);
        let onward_receipt = broker
            .publish_onward(&consumer, &projected, auth.as_ref(), 102)
            .unwrap();
        {
            let state = broker.lock();
            let onward = state.rows.back().unwrap();
            assert_eq!(onward.owner, consumer);
            assert_eq!(onward.class, SharingClass::Custom);
            assert_eq!(onward.provenance.source, producer);
            assert_eq!(onward.provenance.forwarded_by, vec![identity("consumer")]);
            assert_eq!(onward.fields.len(), 1);
            assert!(onward.fields.contains_key("fps"));
            assert!(!onward.fields.contains_key("device_id"));
        }
        // A broad Common category grant does not reveal the forwarded Custom record.
        let next_auth = authorizer(
            [CommonDataCategory::GamePerformanceHistory],
            [],
            [SharingClass::Common],
            ["fps".to_owned()],
            true,
            true,
        );
        let result = broker
            .query_data(
                request(
                    &next_consumer,
                    selector(&[CommonDataCategory::GamePerformanceHistory], &[]),
                    Delivery::Query,
                    10,
                    None,
                ),
                next_auth.as_ref(),
                103,
            )
            .unwrap();
        assert!(
            result
                .records
                .iter()
                .all(|record| record.record_id() != onward_receipt.record_id)
        );
    }

    #[test]
    fn host_authority_fields_and_common_records_without_category_are_refused() {
        let broker = DataBroker::new();
        let producer = identity("producer");
        let mut forged = publication(&producer, &[("fps", serde_json::json!(60))]);
        forged.fields.insert(
            "provenance".into(),
            serde_json::json!({"source":"attacker"}),
        );
        assert_eq!(broker.publish(forged, 100), Err(BrokerError::ReservedField));

        let mut uncategorized = publication(&producer, &[("fps", serde_json::json!(60))]);
        uncategorized.category = None;
        assert_eq!(
            broker.publish(uncategorized, 100),
            Err(BrokerError::InvalidPublication)
        );
    }

    #[test]
    fn sequenced_publications_are_idempotent_and_reject_out_of_order_observations() {
        let broker = DataBroker::new();
        let producer = identity("producer");
        let first = sequenced_publication(
            &producer,
            4,
            "observation-4",
            "session-producer",
            &[("fps", serde_json::json!(60))],
        );
        let receipt = broker.publish(first.clone(), 1_700_000_001).unwrap();
        assert!(!receipt.duplicate);

        let duplicate = broker.publish(first, 1_700_000_002).unwrap();
        assert!(duplicate.duplicate);
        assert_eq!(duplicate.record_id, receipt.record_id);
        assert_eq!(duplicate.revision, receipt.revision);

        let conflict = sequenced_publication(
            &producer,
            4,
            "different-observation",
            "session-producer",
            &[("fps", serde_json::json!(61))],
        );
        assert_eq!(
            broker.publish(conflict, 1_700_000_003),
            Err(BrokerError::SequenceConflict(4))
        );

        let next = sequenced_publication(
            &producer,
            5,
            "observation-5",
            "session-producer",
            &[("fps", serde_json::json!(62))],
        );
        assert!(!broker.publish(next, 1_700_000_004).unwrap().duplicate);

        let out_of_order = sequenced_publication(
            &producer,
            3,
            "observation-3",
            "session-producer",
            &[("fps", serde_json::json!(59))],
        );
        assert_eq!(
            broker.publish(out_of_order, 1_700_000_005),
            Err(BrokerError::SequenceConflict(5))
        );
    }

    #[test]
    fn closing_a_publisher_session_removes_records_and_fences_watches() {
        let broker = DataBroker::new();
        let producer = identity("producer");
        let receipt = broker
            .publish(
                sequenced_publication(
                    &producer,
                    1,
                    "observation-1",
                    "session-producer",
                    &[("fps", serde_json::json!(60))],
                ),
                1_700_000_001,
            )
            .unwrap();
        let authorizer: Arc<dyn Authorizer> =
            authorizer([], [], [], Vec::<String>::new(), false, false);
        let watch = broker
            .subscribe(
                request(
                    &producer,
                    selector(&[CommonDataCategory::GamePerformanceHistory], &[]),
                    Delivery::Live,
                    10,
                    None,
                ),
                0,
                10,
                authorizer,
            )
            .unwrap();

        assert_eq!(broker.close_session("session-producer"), 1);
        assert_eq!(watch.is_ended(), Some(SubscriptionEnd::FellBehind));
        assert_eq!(broker.close_session("session-producer"), 0);
        assert_eq!(broker.expire(1_700_000_002), 0);
        assert!(receipt.revision < 3);
    }

    #[test]
    fn publication_structural_budgets_apply_at_the_broker_boundary() {
        let broker = DataBroker::new();
        let producer = identity("producer");

        let mut too_deep = serde_json::json!("leaf");
        for _ in 0..=MAX_FIELD_DEPTH {
            too_deep = serde_json::json!({"nested": too_deep});
        }
        let deep = publication(&producer, &[("value", too_deep)]);
        assert_eq!(
            broker.publish(deep, 100),
            Err(BrokerError::InvalidPublication)
        );

        let too_many_items = serde_json::json!({
            "values": vec![serde_json::Value::Null; MAX_FIELD_ITEMS],
        });
        let oversized_shape = publication(&producer, &[("value", too_many_items)]);
        assert_eq!(
            broker.publish(oversized_shape, 100),
            Err(BrokerError::InvalidPublication)
        );

        let invalid_key = publication(&producer, &[("bad\nkey", serde_json::json!(true))]);
        assert_eq!(
            broker.publish(invalid_key, 100),
            Err(BrokerError::InvalidPublication)
        );

        let forged_execution = publication(&producer, &[("command", serde_json::json!("run"))]);
        assert_eq!(
            broker.publish(forged_execution, 100),
            Err(BrokerError::ReservedField)
        );
    }
}
