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
    }
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
    let second = client
        .data_publish(data_publish(
            &fixture,
            "data-second",
            value(&[("fps", number(75.0))]),
        ))
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
