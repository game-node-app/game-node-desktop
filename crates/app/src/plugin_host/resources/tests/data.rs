//! Shared Data RPCs through a local ResourceHost gRPC server.

use super::*;
use crate::plugin_host::{
    grants::PluginIdentity,
    resources::ResourceApi,
    shared_data::{DataBroker, Publication},
    sharing_policy::{
        ActivationReadGrant, Authorizer, SharedSharingPolicies, SharingClass, SharingPolicies,
        SourceRecipientPolicy,
    },
};
use gamenode_platform::plugin_shared_data::{OwnerScope, SharedDataStore};
use gamenode_plugin_protocol::{
    capability as cap,
    prost_types::{self, Value, value::Kind},
};
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Channel;

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

fn text(value: &str) -> Value {
    Value {
        kind: Some(Kind::StringValue(value.to_owned())),
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
        fixture.leases.clone(),
        fixture.processes.clone(),
    )
    .with_shared_data(data, authorizer)
    .with_shared_pool(fixture.shared_pool.clone())
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
    assert_eq!(provenance.producer_plugin_id, "fixture.plugin");
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

pub(super) async fn consumer_session(fixture: &Fixture) -> Session {
    consumer_session_with_grants(fixture, &[cap::STORAGE_WRITE]).await
}

pub(super) async fn consumer_session_with_grants(
    fixture: &Fixture,
    grants: &[&'static str],
) -> Session {
    let mut capabilities = vec![cap::CONTEXT];
    capabilities.extend_from_slice(grants);
    fixture
        .api
        .register(Registration {
            provider_id: "consumer".into(),
            plugin_id: "org.example.consumer".into(),
            kind: crate::plugin_host::provider::RegistrationKind::Provider,
            declared_developer: "Consumer developer".into(),
            package_digest: vec![8; 32],
            origin: RegistrationOrigin::ThirdParty {
                publisher_fingerprint: "cd".repeat(32),
            },
            capabilities: capabilities.iter().map(|cap| (*cap).into()).collect(),
            shared_catalog: false,
            playtime: Default::default(),
            services: Arc::new(NoServices),
        })
        .unwrap();
    let launch = fixture.api.begin_launch("consumer").unwrap();
    let credential = (0..launch.credential.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&launch.credential[i..i + 2], 16).unwrap())
        .collect();
    let session = Session::open_with_options(
        Launch {
            endpoint: fixture.endpoint.clone(),
            credential,
            operation_id: "consumer-launch".into(),
        },
        "org.example.consumer",
        "consumer",
        "1.0",
        &capabilities,
        SessionOptions {
            min_api_major: 1,
            min_api_minor: cap::THIRD_PARTY_API_MINOR,
            max_api_major: 1,
            max_api_minor: gamenode_plugin_protocol::API_MINOR,
            ..SessionOptions::default()
        },
    )
    .await
    .unwrap();
    launch.ready.await.unwrap();
    session
}

pub(super) fn authenticated_identity(
    fixture: &Fixture,
    context: &ServiceContext,
) -> PluginIdentity {
    let caller = fixture
        .api
        .authorize_resource_session(Some(context))
        .unwrap();
    ResourceApi::data_identity(&caller).unwrap()
}

fn list_request(context: &ServiceContext, dataset_id: &str) -> DataListRequest {
    DataListRequest {
        page: Some(PageRequest {
            context: Some(context.clone()),
            page_size: 10,
            ..Default::default()
        }),
        dataset_id: dataset_id.into(),
        ..Default::default()
    }
}

fn assert_projected(record: &DataRecord, receipt: &DataMutationReply, fps: f64) {
    assert_eq!(record.record_id, receipt.record_id);
    assert_eq!(record.revision, receipt.revision);
    assert_eq!(record.value, Some(value(&[("fps", number(fps))])));
    assert!(record.opaque_payload.is_empty());
    let provenance = record.provenance.as_ref().unwrap();
    assert_eq!(provenance.producer_plugin_id, "fixture.plugin");
    assert_eq!(provenance.publisher_fingerprint, "ab".repeat(32));
}

#[tokio::test]
async fn persisted_custom_policy_filters_authenticated_public_exchange_and_revokes_access() {
    use futures_util::StreamExt;

    let fixture = Fixture::new(&[cap::DATA_PUBLISH]).await;
    let consumer = consumer_session(&fixture).await;
    let context = consumer.host.resource_context().unwrap().clone();
    let source = authenticated_identity(&fixture, &fixture.context());
    let recipient = authenticated_identity(&fixture, &context);
    let dataset = "org.example.metrics.v1";
    let other_dataset = "org.example.private.v1";
    let broker = Arc::new(DataBroker::new());
    let dir = tempfile::tempdir().unwrap();
    let policy = |history, live| {
        SourceRecipientPolicy::new(
            source.clone(),
            recipient.clone(),
            SharingClass::Custom,
            dataset.into(),
            ["fps".into()],
            history,
            live,
        )
        .unwrap()
    };
    {
        let store = SharedSharingPolicies::open(dir.path()).unwrap();
        store
            .replace_recipient_policies(
                &recipient,
                0,
                ActivationReadGrant::default(),
                vec![policy(true, false)],
            )
            .unwrap();
    }
    let store = Arc::new(SharedSharingPolicies::open(dir.path()).unwrap());
    let resource = service(&fixture, broker, store.clone());
    let (mut consumer_rpc, server) = local_client(resource.clone()).await;
    let mut producer_rpc = consumer_rpc.clone();
    let publish = |sequence, fps| {
        let key = format!("metrics-{sequence}");
        let mut request = data_publish(
            &fixture,
            &key,
            value(&[("fps", number(fps)), ("device_id", number(42.0))]),
        );
        request.sequence = sequence;
        request
    };
    let first = producer_rpc
        .data_publish(publish(1, 60.0))
        .await
        .unwrap()
        .into_inner();
    let mut private = publish(1, 1.0);
    private.dataset_id = other_dataset.into();
    private.schema_id = other_dataset.into();
    private.mutation = Some(fixture.mutation("private-record", 0));
    producer_rpc.data_publish(private).await.unwrap();

    let listed = consumer_rpc
        .data_list(list_request(&context, dataset))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(listed.records.len(), 1);
    assert_projected(&listed.records[0], &first, 60.0);
    let get = DataGetRequest {
        context: Some(context.clone()),
        dataset_id: dataset.into(),
        record_id: first.record_id.clone(),
        ..Default::default()
    };
    let fetched = consumer_rpc
        .data_get(get.clone())
        .await
        .unwrap()
        .into_inner();
    assert_projected(&fetched.record.unwrap(), &first, 60.0);
    let denied = consumer_rpc
        .data_list(list_request(&context, other_dataset))
        .await
        .unwrap()
        .into_inner();
    assert!(denied.records.is_empty());

    let watch = |after_revision| DataWatchRequest {
        context: Some(context.clone()),
        dataset_id: dataset.into(),
        after_revision,
        ..Default::default()
    };
    let mut history_only =
        ResourceHost::data_watch(&resource, Request::new(watch(listed.revision)))
            .await
            .unwrap()
            .into_inner();
    producer_rpc.data_publish(publish(2, 65.0)).await.unwrap();
    assert!(
        tokio::time::timeout(Duration::ZERO, history_only.next())
            .await
            .is_err()
    );

    let revision = store.recipient_snapshot(&recipient).unwrap().revision;
    let revision = store
        .replace_recipient_policies(
            &recipient,
            revision,
            ActivationReadGrant::default(),
            vec![policy(false, true)],
        )
        .unwrap();
    let listed = consumer_rpc
        .data_list(list_request(&context, dataset))
        .await
        .unwrap()
        .into_inner();
    assert!(listed.records.is_empty());
    let mut live = consumer_rpc
        .data_watch(watch(listed.revision))
        .await
        .unwrap()
        .into_inner();
    let third = producer_rpc
        .data_publish(publish(3, 70.0))
        .await
        .unwrap()
        .into_inner();
    let event = tokio::time::timeout(Duration::from_secs(2), live.message())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(event.event_type, "published");
    assert_projected(&event.record.unwrap(), &third, 70.0);

    let revision = store
        .replace_recipient_policies(
            &recipient,
            revision,
            ActivationReadGrant::default(),
            vec![policy(true, true)],
        )
        .unwrap();
    let listed = consumer_rpc
        .data_list(list_request(&context, dataset))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(listed.records.len(), 3);
    assert_projected(&listed.records[2], &third, 70.0);
    let mut live = consumer_rpc
        .data_watch(watch(listed.revision))
        .await
        .unwrap()
        .into_inner();
    // Keep this second ResourceHost stream unpolled so revocation precedes dequeue.
    let mut pending = ResourceHost::data_watch(&resource, Request::new(watch(listed.revision)))
        .await
        .unwrap()
        .into_inner();
    let fourth = producer_rpc
        .data_publish(publish(4, 75.0))
        .await
        .unwrap()
        .into_inner();
    let event = tokio::time::timeout(Duration::from_secs(2), live.message())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(event.event_type, "published");
    assert_projected(&event.record.unwrap(), &fourth, 75.0);

    let mut durable = publish(5, 60.0);
    durable.durability = StorageDurability::Persistent as i32;
    let error = producer_rpc.data_publish(durable).await.unwrap_err();
    assert_eq!(error.code(), Code::PermissionDenied);
    assert!(error.message().contains("storage.persist"));
    let error = consumer_rpc
        .storage_put(StoragePutRequest {
            mutation: Some(MutationContext {
                context: Some(context.clone()),
                idempotency_key: "durable-copy".into(),
                ..Default::default()
            }),
            key: "copy".into(),
            value: vec![60],
            durability: StorageDurability::Persistent as i32,
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::PermissionDenied);

    let mut page = list_request(&context, dataset);
    page.page.as_mut().unwrap().page_size = 1;
    let listed = consumer_rpc
        .data_list(page.clone())
        .await
        .unwrap()
        .into_inner();
    assert_eq!(listed.revision, fourth.revision);
    assert!(!listed.next_cursor.is_empty());
    page.page.as_mut().unwrap().cursor = listed.next_cursor;
    store
        .replace_recipient_policies(&recipient, revision, ActivationReadGrant::default(), vec![])
        .unwrap();
    assert_eq!(
        pending.next().await.unwrap().unwrap_err().code(),
        Code::FailedPrecondition
    );
    assert!(pending.next().await.is_none());
    assert_eq!(
        consumer_rpc.data_list(page).await.unwrap_err().code(),
        Code::FailedPrecondition
    );
    assert_eq!(
        consumer_rpc.data_get(get).await.unwrap_err().code(),
        Code::NotFound
    );
    let denied = consumer_rpc
        .data_list(list_request(&context, dataset))
        .await
        .unwrap()
        .into_inner();
    assert!(denied.records.is_empty());
    server.abort();
}

#[tokio::test]
async fn recipient_must_explicitly_persist_a_projected_shared_record() {
    use gamenode_plugin_protocol::prost::Message;

    let fixture = Fixture::new(&[cap::DATA_PUBLISH]).await;
    let consumer = consumer_session_with_grants(
        &fixture,
        &[cap::STORAGE_READ, cap::STORAGE_WRITE, cap::STORAGE_PERSIST],
    )
    .await;
    let consumer_context = consumer.host.resource_context().unwrap().clone();
    let source = authenticated_identity(&fixture, &fixture.context());
    let recipient = authenticated_identity(&fixture, &consumer_context);
    fixture
        .shared_policies
        .set_source_recipient_policy(
            SourceRecipientPolicy::new(
                source,
                recipient,
                SharingClass::Custom,
                "org.example.metrics.v1".into(),
                ["fps".into()],
                true,
                false,
            )
            .unwrap(),
        )
        .unwrap();

    let published = fixture
        .client()
        .data_publish(data_publish(
            &fixture,
            "retained-shared-record",
            value(&[("fps", number(60.0)), ("device_id", number(42.0))]),
        ))
        .await
        .unwrap()
        .into_inner();
    let read = consumer
        .host
        .resource_client()
        .data_get(DataGetRequest {
            context: Some(consumer_context.clone()),
            dataset_id: "org.example.metrics.v1".into(),
            record_id: published.record_id,
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner()
        .record
        .unwrap();
    assert_eq!(read.value.as_ref().unwrap().fields.len(), 1);
    assert!(read.value.as_ref().unwrap().fields.contains_key("fps"));

    let stored = consumer
        .host
        .resource_client()
        .storage_put(StoragePutRequest {
            mutation: Some(MutationContext {
                context: Some(consumer_context.clone()),
                idempotency_key: "retain-projected-report".into(),
                expected_revision: 0,
                ..Default::default()
            }),
            key: "shared/metrics/latest".into(),
            value: read.encode_to_vec(),
            durability: StorageDurability::Persistent as i32,
            ttl_seconds: 3600,
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(stored.durability, StorageDurability::Persistent as i32);
    assert!(stored.expires_at > 0);
    assert!(stored.usage_bytes > 0);

    let restored = consumer
        .host
        .resource_client()
        .storage_get(StorageGetRequest {
            context: Some(consumer_context),
            key: "shared/metrics/latest".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert!(restored.found);
    assert_eq!(restored.durability, StorageDurability::Persistent as i32);
    assert_eq!(restored.expires_at, stored.expires_at);
    let retained = DataRecord::decode(restored.value.as_slice()).unwrap();
    assert_eq!(retained.value.unwrap().fields.len(), 1);
}

#[tokio::test]
async fn consented_runtime_session_data_projection_hides_handles_and_credentials() {
    let fixture = Fixture::new(&[cap::DATA_PUBLISH]).await;
    let consumer = consumer_session(&fixture).await;
    let context = consumer.host.resource_context().unwrap().clone();
    let source = authenticated_identity(&fixture, &fixture.context());
    let recipient = authenticated_identity(&fixture, &context);
    let dataset = "org.example.runtime-sessions.v1";
    let policy = SourceRecipientPolicy::new(
        source,
        recipient,
        SharingClass::Custom,
        dataset.into(),
        ["game_id".into(), "content_id".into(), "transition".into()],
        true,
        true,
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SharedSharingPolicies::open(dir.path()).unwrap());
    store
        .replace_recipient_policies(
            &authenticated_identity(&fixture, &context),
            0,
            ActivationReadGrant::default(),
            vec![policy],
        )
        .unwrap();
    let resource = service(&fixture, Arc::new(DataBroker::new()), store);
    let (mut client, server) = local_client(resource).await;

    let mut request = data_publish(
        &fixture,
        "runtime-session-observation",
        value(&[
            ("game_id", text("620")),
            ("content_id", text("rom:zelda")),
            ("transition", text("started")),
            ("process_handle", text("opaque-process-handle")),
            ("credential", text("launch-secret")),
        ]),
    );
    request.dataset_id = dataset.into();
    request.schema_id = dataset.into();
    let published = client.data_publish(request).await.unwrap().into_inner();
    let listed = client
        .data_list(list_request(&context, dataset))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(listed.records.len(), 1);
    assert_eq!(
        listed.records[0].value,
        Some(value(&[
            ("game_id", text("620")),
            ("content_id", text("rom:zelda")),
            ("transition", text("started")),
        ]))
    );
    assert_eq!(listed.records[0].record_id, published.record_id);
    assert!(listed.records[0].opaque_payload.is_empty());
    let fetched = client
        .data_get(DataGetRequest {
            context: Some(context.clone()),
            dataset_id: dataset.into(),
            record_id: published.record_id,
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner()
        .record
        .unwrap();
    assert_eq!(fetched.value, listed.records[0].value);

    let mut live = client
        .data_watch(DataWatchRequest {
            context: Some(context),
            dataset_id: dataset.into(),
            after_revision: listed.revision,
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    let mut paused_request = data_publish(
        &fixture,
        "runtime-session-paused",
        value(&[
            ("game_id", text("620")),
            ("content_id", text("rom:zelda")),
            ("transition", text("paused")),
            ("process_handle", text("opaque-process-handle")),
            ("credential", text("launch-secret")),
        ]),
    );
    paused_request.dataset_id = dataset.into();
    paused_request.schema_id = dataset.into();
    paused_request.sequence = 2;
    paused_request.source_observation_id = "runtime-session-paused".into();
    client.data_publish(paused_request).await.unwrap();
    let event = tokio::time::timeout(Duration::from_secs(2), live.message())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(event.event_type, "published");
    assert_eq!(
        event.record.unwrap().value,
        Some(value(&[
            ("game_id", text("620")),
            ("content_id", text("rom:zelda")),
            ("transition", text("paused")),
        ]))
    );
    server.abort();
}

#[tokio::test]
async fn persisted_common_policy_requires_explicit_selectors_on_public_reads() {
    let fixture = Fixture::new(&[]).await;
    let recipient = authenticated_identity(&fixture, &fixture.context());
    let source = PluginIdentity {
        plugin_id: "org.example.adapter".into(),
        publisher_fingerprint: "1234".into(),
        profile_id: recipient.profile_id.clone(),
    };
    let dataset = "org.example.metrics.v1";
    let broker = Arc::new(DataBroker::new());
    let now = unix_now().unwrap();
    let category = CommonDataCategory::GamePerformanceHistory;
    // DataPublish is custom-only. Stand in for a host domain adapter producing common records.
    let mut common = publication(&source, &[("fps", serde_json::json!(60))]);
    common.class = SharingClass::Common;
    common.category = Some(category);
    let first = broker.publish(common.clone(), now).unwrap();
    let second = broker.publish(common.clone(), now).unwrap();
    common.owner.profile_id = "different-profile".into();
    let foreign = broker.publish(common, now).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let grant = ActivationReadGrant::new([category], [first.record_id.clone()]).unwrap();
    SharedSharingPolicies::open(dir.path())
        .unwrap()
        .replace_recipient_policies(&recipient, 0, grant, vec![])
        .unwrap();
    let store = Arc::new(SharedSharingPolicies::open(dir.path()).unwrap());
    let (mut client, server) = local_client(service(&fixture, broker, store)).await;
    let context = fixture.context();
    let mut list = list_request(&context, dataset);
    assert!(
        client
            .data_list(list.clone())
            .await
            .unwrap()
            .into_inner()
            .records
            .is_empty()
    );

    let mut get = DataGetRequest {
        context: Some(context),
        dataset_id: dataset.into(),
        record_id: first.record_id.clone(),
        ..Default::default()
    };
    assert_eq!(
        client.data_get(get.clone()).await.unwrap_err().code(),
        Code::NotFound
    );
    let selected = CommonDataSelector {
        categories: vec![],
        selected_record_ids: vec![first.record_id.clone(), second.record_id.clone()],
    };
    get.common_data_categories = Some(selected.clone());
    let fetched = client.data_get(get).await.unwrap().into_inner();
    assert_eq!(fetched.record.unwrap().record_id, first.record_id);

    list.common_data_categories = Some(selected);
    let selected = client.data_list(list.clone()).await.unwrap().into_inner();
    assert_eq!(selected.records.len(), 1);
    assert_eq!(selected.records[0].record_id, first.record_id);

    list.common_data_categories = Some(CommonDataSelector {
        categories: vec![category as i32],
        selected_record_ids: vec![foreign.record_id],
    });
    let selected = client.data_list(list).await.unwrap().into_inner();
    let ids: Vec<_> = selected.records.into_iter().map(|r| r.record_id).collect();
    assert_eq!(ids, [first.record_id, second.record_id]);
    server.abort();
}

fn durable_owner(fixture: &Fixture, context: &ServiceContext) -> OwnerScope {
    let caller = fixture
        .api
        .authorize_resource_session(Some(context))
        .unwrap();
    let namespace = caller.namespace.as_ref().expect("third-party namespace");
    OwnerScope::new(*namespace.storage_owner().key(), caller.profile_epoch).unwrap()
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn durable_request(
    context: &ServiceContext,
    key: &str,
    resource_id: &str,
    fps: f64,
) -> DataPublishRequest {
    DataPublishRequest {
        mutation: Some(MutationContext {
            context: Some(context.clone()),
            idempotency_key: key.into(),
            ..Default::default()
        }),
        dataset_id: "org.example.metrics.v1".into(),
        schema_id: "org.example.metrics.v1".into(),
        schema_version: 1,
        value: Some(value(&[("fps", number(fps))])),
        opaque_payload: Vec::new(),
        durability: StorageDurability::Persistent as i32,
        ttl_seconds: 300,
        resource_id: resource_id.into(),
        observed_at: 1_700_000_000,
        sequence: 1,
        source_observation_id: key.into(),
    }
}

#[tokio::test]
async fn durable_publish_without_storage_persist_fails_without_session_fallback() {
    let fixture = Fixture::new(&[cap::DATA_PUBLISH]).await;
    let mut request = data_publish(&fixture, "durable-denied", value(&[("fps", number(60.0))]));
    request.durability = StorageDurability::Persistent as i32;

    let error = fixture.client().data_publish(request).await.unwrap_err();
    assert_eq!(error.code(), Code::PermissionDenied);
    assert!(
        error.message().contains("storage.persist"),
        "unexpected message: {}",
        error.message()
    );

    // Nothing was retained for the session and no pool file was created.
    let listed = fixture
        .client()
        .data_list(list_request(&fixture.context(), "org.example.metrics.v1"))
        .await
        .unwrap()
        .into_inner();
    assert!(listed.records.is_empty());
    assert!(
        SharedDataStore::open_existing(&fixture.data_dir)
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn session_only_publish_is_never_persisted() {
    let fixture = Fixture::new(&[cap::DATA_PUBLISH, cap::STORAGE_PERSIST]).await;
    let published = fixture
        .client()
        .data_publish(data_publish(
            &fixture,
            "session-only",
            value(&[("fps", number(60.0))]),
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        published.durability,
        StorageDurability::SessionOnly as i32,
        "the reply reports the mode actually used"
    );

    let listed = fixture
        .client()
        .data_list(list_request(&fixture.context(), "org.example.metrics.v1"))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(listed.records.len(), 1, "the session copy is readable");
    assert!(
        SharedDataStore::open_existing(&fixture.data_dir)
            .unwrap()
            .is_none(),
        "session-only data never reaches the durable pool"
    );
}

#[tokio::test]
async fn durable_publish_survives_producer_disconnect() {
    let fixture = Fixture::new(&[cap::DATA_PUBLISH]).await;
    let producer =
        consumer_session_with_grants(&fixture, &[cap::DATA_PUBLISH, cap::STORAGE_PERSIST]).await;
    let context = producer.host.resource_context().unwrap().clone();
    let owner = durable_owner(&fixture, &context);

    let published = producer
        .host
        .resource_client()
        .data_publish(durable_request(
            &context,
            "durable-survives",
            "runtime-primary",
            60.0,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(published.durability, StorageDurability::Persistent as i32);
    assert!(!published.record_id.is_empty());
    assert!(published.expires_at > 0);

    drop(producer);

    // Reopen the pool as a restarted host would and read the producer's record directly.
    let store = SharedDataStore::open(&fixture.data_dir).unwrap();
    let records = store
        .list_dataset(&owner, "org.example.metrics.v1", unix_seconds())
        .unwrap()
        .records;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].record_id, published.record_id);
    assert_eq!(records[0].revision, published.revision);
    assert_eq!(records[0].expires_at, Some(published.expires_at));
    assert_eq!(
        records[0].provenance.source_plugin_id,
        "org.example.consumer"
    );
    assert_eq!(
        records[0].provenance.source_publisher_fingerprint,
        "cd".repeat(32)
    );
}

#[tokio::test]
async fn durable_resource_replacement_is_owner_scoped() {
    let fixture = Fixture::new(&[cap::DATA_PUBLISH, cap::STORAGE_PERSIST]).await;
    let other =
        consumer_session_with_grants(&fixture, &[cap::DATA_PUBLISH, cap::STORAGE_PERSIST]).await;
    let other_context = other.host.resource_context().unwrap().clone();
    let own_owner = durable_owner(&fixture, &fixture.context());
    let other_owner = durable_owner(&fixture, &other_context);

    let first = fixture
        .client()
        .data_publish(durable_request(
            &fixture.context(),
            "durable-first",
            "runtime-primary",
            60.0,
        ))
        .await
        .unwrap()
        .into_inner();
    let foreign = other
        .host
        .resource_client()
        .data_publish(durable_request(
            &other_context,
            "durable-foreign",
            "runtime-primary",
            1.0,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_ne!(first.record_id, foreign.record_id);

    let replaced = fixture
        .client()
        .data_publish(durable_request(
            &fixture.context(),
            "durable-replacement",
            "runtime-primary",
            75.0,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(replaced.record_id, first.record_id);
    assert!(replaced.revision > first.revision);

    let store = SharedDataStore::open(&fixture.data_dir).unwrap();
    let now = unix_seconds();
    let own = store
        .list_dataset(&own_owner, "org.example.metrics.v1", now)
        .unwrap()
        .records;
    assert_eq!(own.len(), 1, "replacement is in place");
    assert_eq!(own[0].record_id, first.record_id);
    assert_eq!(own[0].revision, replaced.revision);
    let fields: serde_json::Value = serde_json::from_str(&own[0].fields_json).unwrap();
    assert_eq!(fields["fps"], 75.0);

    let foreign_records = store
        .list_dataset(&other_owner, "org.example.metrics.v1", now)
        .unwrap()
        .records;
    assert_eq!(foreign_records.len(), 1, "the other producer is untouched");
    assert_eq!(foreign_records[0].record_id, foreign.record_id);
    assert_eq!(foreign_records[0].revision, foreign.revision);
    let fields: serde_json::Value = serde_json::from_str(&foreign_records[0].fields_json).unwrap();
    assert_eq!(fields["fps"], 1.0);
}
