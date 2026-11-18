//! Shared Data RPCs through a local ResourceHost gRPC server.

use super::*;
use crate::plugin_host::{
    grants::PluginIdentity,
    resources::ResourceApi,
    shared_data::{DataBroker, Publication},
    sharing_policy::{
        AccessDecision, AccessRequest, AccessScope, Authorizer, SharingClass, SharingPolicies,
    },
};
use gamenode_plugin_protocol::{
    capability as cap,
    prost_types::{self, Value, value::Kind},
};
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Channel;

fn identity(id: &str) -> PluginIdentity {
    PluginIdentity {
        plugin_id: format!("org.example.{id}"),
        publisher_fingerprint: format!("SHA256:{id}"),
        profile_id: "profile-main".into(),
    }
}

fn value(fields: &[(&str, Value)]) -> prost_types::Struct {
    prost_types::Struct {
        fields: fields
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect(),
    }
}

fn number(value: f64) -> Value {
    Value {
        kind: Some(Kind::NumberValue(value)),
    }
}

fn publication(owner: &PluginIdentity, fields: &[(&str, serde_json::Value)]) -> Publication {
    Publication {
        owner: owner.clone(),
        dataset_id: "org.example.metrics.v1".into(),
        category: None,
        class: SharingClass::Custom,
        schema_id: "org.example.metrics.v1".into(),
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

struct SelectedRecordAuthorizer {
    revision: AtomicU64,
    record_ids: BTreeSet<String>,
    dataset_ids: BTreeSet<String>,
    fields: BTreeSet<String>,
}

impl Authorizer for SelectedRecordAuthorizer {
    fn revision(&self, _recipient: &PluginIdentity) -> u64 {
        self.revision.load(Ordering::SeqCst)
    }

    fn decide(&self, request: &AccessRequest<'_>) -> AccessDecision {
        let id = match &request.scope {
            AccessScope::SelectedRecord(id) => Some(id),
            AccessScope::Owner | AccessScope::Category(_) | AccessScope::Dataset(_) => None,
        };
        let dataset_allowed = matches!(&request.scope, AccessScope::Dataset(dataset)
            if self.dataset_ids.contains(dataset));
        let fields = request
            .fields
            .intersection(&self.fields)
            .cloned()
            .collect::<BTreeSet<_>>();
        let allowed = request.class == SharingClass::Custom
            && (id.is_some_and(|id| self.record_ids.contains(id)) || dataset_allowed)
            && !fields.is_empty();
        AccessDecision {
            allowed,
            fields,
            history: allowed,
            live: allowed,
            revision: self.revision(request.recipient),
        }
    }
}

fn service(
    fixture: &Fixture,
    data: Arc<DataBroker>,
    authorizer: Arc<dyn Authorizer>,
) -> ResourceApi {
    ResourceApi::new(
        fixture.api.clone(),
        fixture.feed.clone(),
        fixture.storage.clone(),
        fixture.files.clone(),
        fixture.network.clone(),
        fixture.notifications.clone(),
        fixture.jobs.clone(),
        fixture.intents.clone(),
        fixture.saves.clone(),
        fixture.snapshots.clone(),
    )
    .with_shared_data(data, authorizer)
}

async fn local_client(service: ResourceApi) -> (ResourceHostClient<Channel>, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(resource_host_server::ResourceHostServer::new(service))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });
    let client = ResourceHostClient::connect(format!("http://{address}"))
        .await
        .unwrap();
    (client, server)
}

fn data_publish(fixture: &Fixture, key: &str, value: prost_types::Struct) -> DataPublishRequest {
    DataPublishRequest {
        mutation: Some(fixture.mutation(key, 0)),
        dataset_id: "org.example.metrics.v1".into(),
        schema_id: "org.example.metrics.v1".into(),
        schema_version: 1,
        value: Some(value),
        durability: StorageDurability::SessionOnly as i32,
        ttl_seconds: 300,
        resource_id: "runtime-primary".into(),
        observed_at: 1_700_000_000,
        sequence: 1,
        source_observation_id: key.into(),
        ..Default::default()
    }
}

#[tokio::test]
async fn data_publish_replays_identically_and_rejects_key_reuse_for_changed_request() {
    let fixture = Fixture::new(&[cap::DATA_PUBLISH]).await;
    let broker = Arc::new(DataBroker::new());
    let authorizer: Arc<dyn Authorizer> = Arc::new(SharingPolicies::default());
    let (mut client, server) = local_client(service(&fixture, broker, authorizer)).await;
    let request = data_publish(&fixture, "data-idempotent", value(&[("fps", number(60.0))]));

    let first = client
        .data_publish(request.clone())
        .await
        .unwrap()
        .into_inner();
    let replay = client
        .data_publish(request.clone())
        .await
        .unwrap()
        .into_inner();
    assert_eq!(replay.record_id, first.record_id);
    assert_eq!(replay.revision, first.revision);

    let mut changed = request;
    changed.ttl_seconds += 1;
    let error = client.data_publish(changed).await.unwrap_err();
    assert_eq!(error.code(), tonic::Code::FailedPrecondition);
    server.abort();
}

#[tokio::test]
async fn data_publish_requires_exactly_one_nonempty_payload() {
    let fixture = Fixture::new(&[cap::DATA_PUBLISH]).await;
    let broker = Arc::new(DataBroker::new());
    let authorizer: Arc<dyn Authorizer> = Arc::new(SharingPolicies::default());
    let (mut client, server) = local_client(service(&fixture, broker, authorizer)).await;

    let mut neither = data_publish(&fixture, "payload-neither", prost_types::Struct::default());
    neither.value = None;
    let error = client.data_publish(neither).await.unwrap_err();
    assert_eq!(error.code(), tonic::Code::InvalidArgument);

    let mut both = data_publish(&fixture, "payload-both", value(&[("fps", number(60.0))]));
    both.opaque_payload = vec![1];
    let error = client.data_publish(both).await.unwrap_err();
    assert_eq!(error.code(), tonic::Code::InvalidArgument);

    let empty = data_publish(
        &fixture,
        "payload-empty-struct",
        prost_types::Struct::default(),
    );
    let error = client.data_publish(empty).await.unwrap_err();
    assert_eq!(error.code(), tonic::Code::InvalidArgument);

    server.abort();
}

#[tokio::test]
async fn data_publish_preserves_observation_metadata_and_host_provenance() {
    let fixture = Fixture::new(&[cap::DATA_PUBLISH]).await;
    let broker = Arc::new(DataBroker::new());
    let authorizer: Arc<dyn Authorizer> = Arc::new(SharingPolicies::default());
    let (mut client, server) = local_client(service(&fixture, broker, authorizer)).await;
    let request = data_publish(&fixture, "data-metadata", value(&[("fps", number(60.0))]));

    let published = client.data_publish(request).await.unwrap().into_inner();
    let listed = client
        .data_list(DataListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                page_size: 10,
                ..Default::default()
            }),
            dataset_id: "org.example.metrics.v1".into(),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    let record = listed
        .records
        .iter()
        .find(|record| record.record_id == published.record_id)
        .unwrap();
    assert_eq!(record.resource_id, "runtime-primary");
    assert_eq!(record.observed_at, 1_700_000_000);
    assert_eq!(record.sequence, 1);
    assert_eq!(record.source_observation_id, "data-metadata");
    assert!(record.received_at > 0);
    let provenance = record.provenance.as_ref().unwrap();
    assert_eq!(provenance.producer_plugin_id, "org.example.fixture");
    assert!(!provenance.package_version.is_empty());
    assert_eq!(provenance.package_digest.len(), 32);
    server.abort();
}

#[tokio::test]
async fn data_publish_round_trips_opaque_bytes_and_enforces_sequence_idempotency() {
    let fixture = Fixture::new(&[cap::DATA_PUBLISH]).await;
    let broker = Arc::new(DataBroker::new());
    let authorizer: Arc<dyn Authorizer> = Arc::new(SharingPolicies::default());
    let (mut client, server) = local_client(service(&fixture, broker, authorizer)).await;
    let mut request = DataPublishRequest {
        mutation: Some(fixture.mutation("opaque-first", 0)),
        dataset_id: "org.example.metrics.v1".into(),
        schema_id: "org.example.metrics.v1".into(),
        schema_version: 1,
        durability: StorageDurability::SessionOnly as i32,
        ttl_seconds: 300,
        opaque_payload: vec![0, 1, 2, 255],
        resource_id: "runtime-primary".into(),
        observed_at: 1_700_000_000,
        sequence: 1,
        source_observation_id: "opaque-observation-1".into(),
        ..Default::default()
    };
    let first = client
        .data_publish(request.clone())
        .await
        .unwrap()
        .into_inner();
    assert_eq!(first.outcome, MutationOutcome::Applied as i32);

    let replay = client
        .data_publish(request.clone())
        .await
        .unwrap()
        .into_inner();
    assert_eq!(replay.record_id, first.record_id);
    let mut changed_payload = request.clone();
    changed_payload.opaque_payload[3] = 254;
    let error = client.data_publish(changed_payload).await.unwrap_err();
    assert_eq!(error.code(), tonic::Code::FailedPrecondition);

    request.mutation = Some(fixture.mutation("opaque-replay", 0));
    let duplicate = client
        .data_publish(request.clone())
        .await
        .unwrap()
        .into_inner();
    assert_eq!(duplicate.record_id, first.record_id);
    assert_eq!(duplicate.outcome, MutationOutcome::AlreadyApplied as i32);

    let got = client
        .data_get(DataGetRequest {
            context: Some(fixture.context()),
            dataset_id: request.dataset_id.clone(),
            record_id: first.record_id.clone(),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner()
        .record
        .unwrap();
    assert!(got.value.is_none());
    assert_eq!(got.opaque_payload, vec![0, 1, 2, 255]);
    assert_eq!(got.sequence, 1);
    assert_eq!(got.source_observation_id, "opaque-observation-1");

    request.mutation = Some(fixture.mutation("opaque-conflict", 0));
    request.source_observation_id = "different-observation".into();
    let conflict = client.data_publish(request).await.unwrap().into_inner();
    assert_eq!(conflict.outcome, MutationOutcome::RevisionConflict as i32);
    assert_eq!(conflict.revision, 1);
    server.abort();
}

#[tokio::test]
async fn publisher_session_disconnect_removes_its_session_retained_data() {
    let fixture = Fixture::new(&[cap::DATA_PUBLISH]).await;
    let broker = Arc::new(DataBroker::new());
    let authorizer: Arc<dyn Authorizer> = Arc::new(SharingPolicies::default());
    let (mut client, server) = local_client(service(&fixture, broker.clone(), authorizer)).await;
    let mut request = data_publish(&fixture, "session-cleanup", value(&[("fps", number(60.0))]));
    request.ttl_seconds = 0;
    let published = client.data_publish(request).await.unwrap().into_inner();
    assert!(!published.record_id.is_empty());
    assert_eq!(published.expires_at, 0);

    let session_id = fixture.context().session_id;
    fixture.api.disconnect("fixture").await;
    // ResourceApi registered this same broker with ProviderApi, so close_session finds that the
    // disconnect callback already removed the record and its sequence cursor.
    assert_eq!(broker.close_session(&session_id), 0);
    server.abort();
}

#[tokio::test]
async fn data_publish_list_get_and_watch_use_the_snapshot_revision() {
    let fixture = Fixture::new(&[cap::DATA_PUBLISH]).await;
    let broker = Arc::new(DataBroker::new());
    let authorizer: Arc<dyn Authorizer> = Arc::new(SharingPolicies::default());
    let (mut client, server) = local_client(service(&fixture, broker, authorizer)).await;

    let first = client
        .data_publish(data_publish(
            &fixture,
            "data-first",
            value(&[("fps", number(60.0))]),
        ))
        .await
        .unwrap()
        .into_inner();
    assert!(!first.record_id.is_empty());
    assert!(first.expires_at > 0);

    let context = Some(fixture.context());
    let list = client
        .data_list(DataListRequest {
            page: Some(PageRequest {
                context: context.clone(),
                page_size: 10,
                ..Default::default()
            }),
            dataset_id: "org.example.metrics.v1".into(),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(list.records.len(), 1);
    assert_eq!(list.records[0].record_id, first.record_id);
    assert!(list.revision >= list.records[0].revision);

    let mut watch = client
        .data_watch(DataWatchRequest {
            context,
            dataset_id: "org.example.metrics.v1".into(),
            after_revision: list.revision,
            common_data_categories: None,
        })
        .await
        .unwrap()
        .into_inner();
    let mut second_request = data_publish(&fixture, "data-second", value(&[("fps", number(75.0))]));
    second_request.sequence = 2;
    let second = client
        .data_publish(second_request)
        .await
        .unwrap()
        .into_inner();
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), watch.message())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(event.event_type, "published");
    assert_eq!(event.revision, second.revision);
    assert_eq!(event.record.unwrap().record_id, second.record_id);

    let got = client
        .data_get(DataGetRequest {
            context: Some(fixture.context()),
            dataset_id: "org.example.metrics.v1".into(),
            record_id: second.record_id,
            common_data_categories: None,
        })
        .await
        .unwrap()
        .into_inner()
        .record
        .unwrap();
    assert_eq!(got.publisher_id, "fixture.plugin");
    assert_eq!(
        got.value.unwrap().fields["fps"].kind,
        Some(Kind::NumberValue(75.0))
    );
    server.abort();
}

#[tokio::test]
async fn data_projection_filters_both_aliases_and_denial_keeps_owner_records() {
    let fixture = Fixture::new(&[]).await;
    let caller = fixture
        .api
        .authorize_resource_session(Some(&fixture.context()))
        .unwrap();
    let recipient = caller.namespace.as_ref().unwrap().identity().clone();
    let producer = identity("producer");
    let broker = Arc::new(DataBroker::new());
    let allowed = broker
        .publish(
            publication(
                &producer,
                &[
                    ("fps", serde_json::json!(60)),
                    ("device_id", serde_json::json!("filtered")),
                ],
            ),
            100,
        )
        .unwrap();
    let denied = broker
        .publish(
            publication(&producer, &[("fps", serde_json::json!(1))]),
            100,
        )
        .unwrap();
    let private = broker
        .publish(
            publication(
                &recipient,
                &[("private_note", serde_json::json!("still here"))],
            ),
            100,
        )
        .unwrap();
    let authorizer: Arc<dyn Authorizer> = Arc::new(SelectedRecordAuthorizer {
        revision: AtomicU64::new(0),
        record_ids: [allowed.record_id.clone()].into_iter().collect(),
        dataset_ids: BTreeSet::new(),
        fields: ["fps".to_owned()].into_iter().collect(),
    });
    let (mut client, server) = local_client(service(&fixture, broker, authorizer)).await;
    let selected = CommonDataSelector {
        categories: vec![],
        selected_record_ids: vec![allowed.record_id.clone(), denied.record_id.clone()],
    };
    let list = client
        .data_list(DataListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                page_size: 10,
                ..Default::default()
            }),
            dataset_id: "org.example.metrics.v1".into(),
            common_data_categories: Some(selected.clone()),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(list.records.len(), 2);
    assert!(
        list.records
            .iter()
            .any(|record| record.record_id == private.record_id)
    );
    let shared = list
        .records
        .iter()
        .find(|record| record.record_id == allowed.record_id)
        .unwrap();
    let fields = &shared.value.as_ref().unwrap().fields;
    assert!(fields.contains_key("fps"));
    assert!(!fields.contains_key("device_id"));
    assert!(
        !list
            .records
            .iter()
            .any(|record| record.record_id == denied.record_id)
    );

    let fetched = client
        .data_get(DataGetRequest {
            context: Some(fixture.context()),
            dataset_id: "org.example.metrics.v1".into(),
            record_id: allowed.record_id,
            common_data_categories: None,
        })
        .await
        .unwrap()
        .into_inner()
        .record
        .unwrap();
    let fetched_fields = &fetched.value.unwrap().fields;
    assert!(fetched_fields.contains_key("fps"));
    assert!(!fetched_fields.contains_key("device_id"));
    server.abort();
}

#[tokio::test]
async fn custom_dataset_policy_authorizes_list_get_and_live_watch_only_for_that_dataset() {
    let fixture = Fixture::new(&[]).await;
    let producer = identity("dataset_producer");
    let broker = Arc::new(DataBroker::new());
    let mut first_publication = publication(&producer, &[("fps", serde_json::json!(120))]);
    first_publication.dataset_id = "org.example.metrics.v1".into();
    first_publication.schema_id = first_publication.dataset_id.clone();
    let first = broker.publish(first_publication, 100).unwrap();
    let mut other_publication = publication(&producer, &[("fps", serde_json::json!(1))]);
    other_publication.dataset_id = "org.example.private.v1".into();
    other_publication.schema_id = other_publication.dataset_id.clone();
    let other = broker.publish(other_publication, 100).unwrap();
    let authorizer: Arc<dyn Authorizer> = Arc::new(SelectedRecordAuthorizer {
        revision: AtomicU64::new(7),
        record_ids: BTreeSet::new(),
        dataset_ids: ["org.example.metrics.v1".to_owned()].into_iter().collect(),
        fields: ["fps".to_owned()].into_iter().collect(),
    });
    let (mut client, server) = local_client(service(&fixture, broker.clone(), authorizer)).await;

    let listed = client
        .data_list(DataListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                page_size: 10,
                ..Default::default()
            }),
            dataset_id: "org.example.metrics.v1".into(),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(listed.records.len(), 1);
    assert_eq!(listed.records[0].record_id, first.record_id);
    assert_eq!(listed.records[0].value.as_ref().unwrap().fields.len(), 1);

    let denied = client
        .data_list(DataListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                page_size: 10,
                ..Default::default()
            }),
            dataset_id: "org.example.private.v1".into(),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert!(denied.records.is_empty());
    assert!(
        !denied
            .records
            .iter()
            .any(|record| record.record_id == other.record_id)
    );

    let fetched = client
        .data_get(DataGetRequest {
            context: Some(fixture.context()),
            dataset_id: "org.example.metrics.v1".into(),
            record_id: first.record_id.clone(),
            common_data_categories: None,
        })
        .await
        .unwrap()
        .into_inner()
        .record
        .unwrap();
    assert_eq!(fetched.record_id, first.record_id);
    assert_eq!(fetched.value.unwrap().fields.len(), 1);

    let mut watch = client
        .data_watch(DataWatchRequest {
            context: Some(fixture.context()),
            dataset_id: "org.example.metrics.v1".into(),
            after_revision: listed.revision,
            common_data_categories: None,
        })
        .await
        .unwrap()
        .into_inner();
    let mut next_publication = publication(&producer, &[("fps", serde_json::json!(144))]);
    next_publication.dataset_id = "org.example.metrics.v1".into();
    next_publication.schema_id = next_publication.dataset_id.clone();
    let next = broker.publish(next_publication, 101).unwrap();
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), watch.message())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(event.record.unwrap().record_id, next.record_id);
    server.abort();
}
