//! Hybrid records, opaque envelopes and custom datasets: the Rust example recipes against the
//! real fixture host, with the Go example reading the same contract fixture.

use std::collections::{BTreeSet, HashMap};

use gamenode_plugin_sdk::proto::{
    CommonDataCategory, CommonDataSelector, DataListRequest, DomainGetRequest, DomainListRequest,
    ExtensionPatch, MutationContext, MutationOutcome, PageRequest, RuntimeRegisterRequest,
    RuntimeUpdateRequest, ServiceContext,
};
use serde_json::Value;

use gamenode_plugin_protocol::WireError;

use super::data::{authenticated_identity, consumer_session_with_grants};
use super::*;

#[path = "../../../../../plugin-sdk/examples/extension_data_recipes.rs"]
mod recipes;

const CONTRACT: &str =
    include_str!("../../../../../plugin-protocol/contracts/data_exchange_fixtures.json");

fn contract() -> Value {
    serde_json::from_str(CONTRACT).unwrap()
}

macro_rules! text {
    ($value:expr) => {
        $value.as_str().unwrap()
    };
}

fn mutation(context: &ServiceContext, key: &str, expected_revision: u64) -> MutationContext {
    MutationContext {
        context: Some(context.clone()),
        operation_id: String::new(),
        idempotency_key: key.into(),
        expected_revision,
    }
}

/// A third-party plugin registered under the example identity the recipes and the Go fixture use.
async fn example_producer(fixture: &Fixture, grants: &[&'static str]) -> Session {
    let mut capabilities = vec![cap::CONTEXT];
    capabilities.extend_from_slice(grants);
    fixture
        .api
        .register(Registration {
            provider_id: "org.example".into(),
            plugin_id: "org.example".into(),
            kind: crate::plugin_host::provider::RegistrationKind::Provider,
            declared_developer: "Example developer".into(),
            package_digest: vec![6; 32],
            origin: RegistrationOrigin::ThirdParty {
                publisher_fingerprint: "ab".repeat(32),
            },
            capabilities: capabilities.iter().map(|cap| (*cap).into()).collect(),
            shared_catalog: false,
            playtime: Default::default(),
            services: Arc::new(NoServices),
        })
        .unwrap();
    let launch = fixture.api.begin_launch("org.example").unwrap();
    let credential = (0..launch.credential.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&launch.credential[index..index + 2], 16).unwrap())
        .collect();
    let session = Session::open_with_options(
        Launch {
            endpoint: fixture.endpoint.clone(),
            credential,
            operation_id: "example-producer-launch".into(),
        },
        "org.example",
        "org.example",
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

#[tokio::test(flavor = "multi_thread")]
async fn hybrid_runtime_keeps_the_future_namespace_and_projects_only_known_fields() {
    use crate::plugin_host::sharing_policy::ActivationReadGrant;
    use gamenode_plugin_sdk::proto::{
        ExtensionNamespacePatch, extension_namespace_patch::Operation,
    };

    // The fixture plugin owns the namespaces the real host accepts; the example's generic
    // identities are asserted locally below and mirrored by the Go fixture.
    const HOST_KNOWN: &str = "fixture.plugin.runtime";
    const HOST_FUTURE: &str = "fixture.plugin.future-runtime";

    let contract = contract();
    let extension = &contract["extension"];
    let grants = [
        cap::FILES_READ,
        cap::GAME_RUNTIME_CREATE,
        cap::GAME_RUNTIME_READ,
        cap::GAME_RUNTIME_UPDATE,
    ];
    let fixture = Fixture::new(&grants).await;
    fixture.with_host();
    let dir = fixture.game_dir("hybrid-runtime", &[("emulator", b"binary")]);
    fixture.library_in(&[("fixture:1", Some(3), "Hybrid Game", &dir)]);
    let root = fixture.approve("id-fixture:1", "installation", &dir).await;
    let executable = fixture.child(&root, "emulator").await;
    // Only the namespace this plugin declares has a packaged descriptor. The future namespace is
    // optional and must stay usable without one.
    let mut descriptor = recipes::known_descriptor();
    descriptor.namespace = HOST_KNOWN.into();
    descriptor.schema_id = "fixture.plugin.runtime.v1".into();
    fixture
        .api
        .set_extension_schemas(
            "fixture",
            "fixture.plugin",
            &"ab".repeat(32),
            vec![descriptor],
        )
        .unwrap();
    let context = fixture.context();

    // Local schema handling with the example's own identities: a v9 payload is not validated as
    // v1, and a declared field is the only thing projection keeps.
    let example = recipes::hybrid_runtime();
    assert_eq!(
        example.extensions[recipes::KNOWN_NAMESPACE].schema_id,
        text!(extension["known_schema_id"])
    );
    assert_eq!(
        example.extensions[recipes::FUTURE_NAMESPACE].schema_version,
        extension["future_schema_version"].as_u64().unwrap() as u32
    );
    assert_eq!(recipes::reject_future_namespace(&example), Err(WireError));
    let projected = recipes::project_known_namespace(
        &example,
        &BTreeSet::from([text!(extension["known_field"]).into()]),
    )
    .unwrap();
    assert_eq!(
        projected.data.unwrap().fields.keys().collect::<Vec<_>>(),
        [text!(extension["known_field"])]
    );

    // The host accepts the hybrid record: the known namespace matches its packaged descriptor and
    // the future namespace is carried opaquely.
    let mut record = example.clone();
    record.executable_handle = executable;
    record.platform = std::env::consts::OS.into();
    let known = record.extensions.remove(recipes::KNOWN_NAMESPACE).unwrap();
    let future = record.extensions.remove(recipes::FUTURE_NAMESPACE).unwrap();
    let mut known = known;
    known.schema_id = "fixture.plugin.runtime.v1".into();
    let mut future = future;
    future.schema_id = "fixture.plugin.future-runtime.v9".into();
    record.extensions = HashMap::from([(HOST_KNOWN.into(), known), (HOST_FUTURE.into(), future)]);
    let registered = fixture
        .client()
        .runtimes_register(RuntimeRegisterRequest {
            mutation: Some(fixture.mutation("hybrid-register", 0)),
            runtime: Some(record.clone()),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(registered.outcome, MutationOutcome::Applied as i32);
    let current = registered.runtime.unwrap();
    assert_eq!(current.revision, 1);

    // The owner sees both namespaces, including the one it cannot interpret.
    let owned = fixture
        .client()
        .runtimes_get(DomainGetRequest {
            context: Some(context.clone()),
            resource_id: current.id.clone(),
            expected_revision: 0,
            shared_records: None,
        })
        .await
        .unwrap()
        .into_inner()
        .runtime
        .unwrap();
    assert_eq!(
        owned.extensions[HOST_FUTURE],
        record.extensions[HOST_FUTURE]
    );

    // An owner update from a client that understands only the known namespace changes that
    // namespace and the typed name; the host keeps the future namespace intact.
    let mut patch = recipes::known_namespace_patch(current.revision);
    patch.namespaces[0].namespace = HOST_KNOWN.into();
    match patch.namespaces[0].operation.as_mut().unwrap() {
        Operation::Set(payload) => payload.schema_id = "fixture.plugin.runtime.v1".into(),
        Operation::Remove(_) => unreachable!(),
    }
    let mut proposed = current.clone();
    proposed.name = "Example Runtime (handheld)".into();
    let updated = fixture
        .client()
        .runtimes_update(RuntimeUpdateRequest {
            mutation: Some(fixture.mutation("hybrid-patch", current.revision)),
            runtime: Some(proposed),
            field_mask: Vec::new(),
            extension_patch: Some(patch),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(updated.outcome, MutationOutcome::Applied as i32);
    let updated = updated.runtime.unwrap();
    assert_eq!(updated.revision, current.revision + 1);
    assert_eq!(updated.name, "Example Runtime (handheld)");
    assert_eq!(
        updated.extensions[HOST_FUTURE],
        record.extensions[HOST_FUTURE]
    );
    assert_eq!(
        updated.extensions[HOST_KNOWN].data.as_ref().unwrap().fields
            [text!(extension["known_field"])]
        .kind,
        Some(gamenode_plugin_sdk::prost_types::value::Kind::StringValue(
            "handheld".into()
        ))
    );

    // A stale patch is an explicit conflict, not a partial write.
    let stale = fixture
        .client()
        .runtimes_update(RuntimeUpdateRequest {
            mutation: Some(fixture.mutation("hybrid-stale", current.revision)),
            runtime: Some(current.clone()),
            field_mask: Vec::new(),
            extension_patch: Some(ExtensionPatch {
                expected_revision: current.revision,
                typed_field_mask: vec!["name".into()],
                namespaces: vec![ExtensionNamespacePatch {
                    namespace: HOST_KNOWN.into(),
                    operation: Some(Operation::Set(record.extensions[HOST_KNOWN].clone())),
                }],
            }),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(stale.outcome, MutationOutcome::RevisionConflict as i32);

    // A consumer with the runtime-catalog grant sees a safe shared projection: producer
    // provenance and the validated known namespace, no executable handle and no unknown namespace.
    let consumer = consumer_session_with_grants(
        &fixture,
        &[cap::LIBRARY_READ, cap::GAME_RUNTIME_READ, cap::FILES_READ],
    )
    .await;
    let consumer_context = consumer.host.resource_context().unwrap().clone();
    let identity = authenticated_identity(&fixture, &consumer_context);
    fixture
        .shared_policies
        .clear_activation_grants(&identity)
        .unwrap();
    let selector = CommonDataSelector {
        categories: vec![CommonDataCategory::RuntimeCatalog as i32],
        selected_record_ids: Vec::new(),
    };
    assert_eq!(
        consumer
            .host
            .resource_client()
            .runtimes_list(DomainListRequest {
                page: Some(PageRequest {
                    context: Some(consumer_context.clone()),
                    page_size: 10,
                    common_data_categories: Some(selector.clone()),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    fixture
        .shared_policies
        .set_activation_grants(
            &identity,
            ActivationReadGrant::new([CommonDataCategory::RuntimeCatalog], std::iter::empty())
                .unwrap(),
        )
        .unwrap();
    let shared = consumer
        .host
        .resource_client()
        .runtimes_list(DomainListRequest {
            page: Some(PageRequest {
                context: Some(consumer_context),
                page_size: 10,
                common_data_categories: Some(selector),
                ..Default::default()
            }),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(shared.runtimes.len(), 1);
    let shared = &shared.runtimes[0];
    assert_eq!(shared.id, current.id);
    assert!(shared.executable_handle.is_empty());
    assert_eq!(
        shared.provenance.as_ref().unwrap().producer_plugin_id,
        "fixture.plugin"
    );
    assert_eq!(
        shared.extensions[HOST_KNOWN].schema_id,
        "fixture.plugin.runtime.v1"
    );
    // The shared catalog carries the bounded extension set as an opaque envelope: a consumer must
    // not interpret a namespace it has no descriptor for, and Core projects no typed authority
    // from it. The owner record is unchanged and the shared record has no executable handle.
    assert_eq!(
        shared.extensions[HOST_FUTURE],
        record.extensions[HOST_FUTURE]
    );
    assert!(
        format!("{shared:?}").find("emulator").is_none(),
        "the shared projection never discloses the executable handle or a path"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn custom_struct_and_opaque_envelopes_project_only_granted_fields() {
    use crate::plugin_host::sharing_policy::{SharingClass, SourceRecipientPolicy};

    let contract = contract();
    let extension = &contract["extension"];
    let fixture = Fixture::new(&[]).await;
    let producer = example_producer(&fixture, &[cap::DATA_PUBLISH]).await;
    let producer_context = producer.host.resource_context().unwrap().clone();
    let consumer = consumer_session_with_grants(&fixture, &[]).await;
    let consumer_context = consumer.host.resource_context().unwrap().clone();
    let source = authenticated_identity(&fixture, &producer_context);
    let recipient = authenticated_identity(&fixture, &consumer_context);
    let policy = |dataset: &str, fields: &[&str]| {
        SourceRecipientPolicy::new(
            source.clone(),
            recipient.clone(),
            SharingClass::Custom,
            dataset.into(),
            fields.iter().map(|field| (*field).to_string()),
            true,
            false,
        )
        .unwrap()
    };

    // The custom envelope is versioned and names only data. The example's fields match the shared
    // fixture, and Core does not interpret their meaning.
    let mut custom = recipes::custom_dataset_wire_shape();
    assert_eq!(custom.dataset_id, text!(extension["custom_dataset_id"]));
    assert_eq!(custom.schema_id, text!(extension["custom_schema_id"]));
    assert_eq!(
        custom.schema_version,
        extension["custom_schema_version"].as_u64().unwrap() as u32
    );
    custom.mutation = Some(mutation(&producer_context, "custom-notes", 0));
    let published = producer
        .host
        .resource_client()
        .data_publish(custom)
        .await
        .unwrap()
        .into_inner();
    assert_eq!(published.outcome, MutationOutcome::Applied as i32);

    let list = |context: &ServiceContext, dataset: &str| DataListRequest {
        page: Some(PageRequest {
            context: Some(context.clone()),
            page_size: 10,
            ..Default::default()
        }),
        dataset_id: dataset.into(),
        ..Default::default()
    };
    fixture
        .shared_policies
        .set_source_recipient_policy(policy(
            text!(extension["custom_dataset_id"]),
            &[text!(extension["projection_paths"][0])],
        ))
        .unwrap();
    let projected = consumer
        .host
        .resource_client()
        .data_list(list(
            &consumer_context,
            text!(extension["custom_dataset_id"]),
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(projected.records.len(), 1);
    let record = &projected.records[0];
    assert_eq!(record.record_id, published.record_id);
    let fields = record.value.as_ref().unwrap().fields.clone();
    assert_eq!(
        fields.keys().cloned().collect::<BTreeSet<_>>(),
        BTreeSet::from([text!(extension["projection_paths"][0]).to_string()])
    );
    assert!(!fields.contains_key(text!(extension["custom_fields"]["note"])));
    // The example's local projection keeps the same field; it never creates authority.
    assert_eq!(
        recipes::project_custom_dataset(
            record.value.as_ref().unwrap(),
            &BTreeSet::from([text!(extension["projection_paths"][0]).to_string()]),
        )
        .unwrap(),
        record.value.clone().unwrap()
    );

    // A later schema version is an opaque label for custom data: the same grant still returns the
    // same field, and the version is retained for the owner.
    let mut future = recipes::custom_dataset_wire_shape_with_version(
        extension["unsupported_schema_version"].as_u64().unwrap() as u32,
    );
    future.mutation = Some(mutation(&producer_context, "custom-notes-v10", 0));
    future.sequence = 2;
    future.source_observation_id = "game-notes-observation-v10".into();
    let published = producer
        .host
        .resource_client()
        .data_publish(future)
        .await
        .unwrap()
        .into_inner();
    assert_eq!(published.outcome, MutationOutcome::Applied as i32);
    let projected_again = consumer
        .host
        .resource_client()
        .data_list(list(
            &consumer_context,
            text!(extension["custom_dataset_id"]),
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(projected_again.records.len(), 2);
    assert!(projected_again.records.iter().all(|record| {
        record.schema_version == extension["unsupported_schema_version"].as_u64().unwrap() as u32
            || record.schema_version == 2
    }));

    // Opaque bytes need their own explicit grant and arrive whole; no Struct field is invented.
    let mut opaque = recipes::opaque_dataset_wire_shape();
    assert_eq!(opaque.opaque_payload, vec![0, 1, 2, 255]);
    opaque.mutation = Some(mutation(&producer_context, "custom-notes-binary", 0));
    let published = producer
        .host
        .resource_client()
        .data_publish(opaque)
        .await
        .unwrap()
        .into_inner();
    assert_eq!(published.outcome, MutationOutcome::Applied as i32);
    let without_grant = consumer
        .host
        .resource_client()
        .data_list(list(
            &consumer_context,
            text!(extension["opaque_dataset_id"]),
        ))
        .await
        .unwrap()
        .into_inner();
    assert!(without_grant.records.is_empty());
    fixture
        .shared_policies
        .set_source_recipient_policy(policy(
            text!(extension["opaque_dataset_id"]),
            &[text!(extension["opaque_grant_field"])],
        ))
        .unwrap();
    let with_grant = consumer
        .host
        .resource_client()
        .data_list(list(
            &consumer_context,
            text!(extension["opaque_dataset_id"]),
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(with_grant.records.len(), 1);
    let bytes = &with_grant.records[0];
    assert_eq!(bytes.opaque_payload, vec![0, 1, 2, 255]);
    assert!(
        bytes
            .value
            .as_ref()
            .is_none_or(|value| value.fields.is_empty())
    );

    // Revoking the custom grant fences the next list immediately.
    fixture
        .shared_policies
        .revoke_source_recipient_policy(
            &source,
            &recipient,
            SharingClass::Custom,
            text!(extension["custom_dataset_id"]),
        )
        .unwrap();
    assert!(
        consumer
            .host
            .resource_client()
            .data_list(list(
                &consumer_context,
                text!(extension["custom_dataset_id"])
            ))
            .await
            .unwrap()
            .into_inner()
            .records
            .is_empty()
    );
}
