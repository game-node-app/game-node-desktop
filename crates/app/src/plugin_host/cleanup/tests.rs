use std::path::PathBuf;
use std::sync::Arc;

use gamenode_platform::plugin_shared_data::OwnerScope;
use gamenode_platform::plugin_storage::{
    DurableStore, QUOTA_BYTES, RetentionPolicy, namespace_dir,
};
use gamenode_plugin_protocol::capability as cap;

use crate::plugin_host::shared_pool::DurableWrite;
use crate::plugin_host::{
    PluginApi,
    provider::{ExtensionServices, Registration, RegistrationKind, RegistrationOrigin},
    runtime::ThirdPartyPluginLifecycle,
};

const ONE: &str = "org.example.one";
const TWO: &str = "org.example.two";

struct Fixture {
    dir: tempfile::TempDir,
    root: PluginApi,
    state: crate::plugin_host::profile::PluginStateLayout,
    registry: gamenode_platform::plugin_registry::PluginPackageRegistry,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("app-data");
        std::fs::create_dir_all(&data_dir).unwrap();
        let root = PluginApi {
            shared_pool: Arc::new(crate::plugin_host::shared_pool::DurableSharedPool::new(
                &data_dir,
            )),
            storage: Arc::new(crate::plugin_host::storage::PluginStorage::new(
                dir.path().join("plugin-data"),
            )),
            ..Default::default()
        };
        let state = crate::plugin_host::profile::PluginStateLayout::new(dir.path().join("state"));
        root.set_state_layout(state.clone());
        root.providers.profiles().update(Some("account-one"), true);
        let registry = gamenode_platform::plugin_registry::PluginPackageRegistry::at(
            dir.path().join("registry"),
        )
        .unwrap();
        let fixture = Self {
            dir,
            root,
            state,
            registry,
        };
        fixture.register(ONE, "provider-one", "aa");
        fixture.register(TWO, "provider-two", "bb");
        fixture
    }

    fn register(&self, plugin_id: &str, provider_id: &str, fingerprint_byte: &str) {
        self.root
            .providers
            .register(Registration {
                provider_id: provider_id.into(),
                plugin_id: plugin_id.into(),
                declared_developer: "Example Developer".into(),
                package_digest: vec![7; 32],
                kind: RegistrationKind::Provider,
                origin: RegistrationOrigin::ThirdParty {
                    publisher_fingerprint: fingerprint_byte.repeat(32),
                },
                capabilities: vec![cap::CONTEXT.into()],
                shared_catalog: false,
                playtime: Default::default(),
                services: Arc::new(ExtensionServices),
            })
            .unwrap();
    }

    fn lifecycle(&self) -> ThirdPartyPluginLifecycle {
        let processes = crate::plugin_host::process::ProviderProcesses::new_for_lifecycle(
            self.root.providers.clone(),
            self.dir.path().join("plugins.sock"),
            vec![],
            self.state.clone(),
        )
        .unwrap();
        let lifecycle = ThirdPartyPluginLifecycle::new(
            self.root.providers.clone(),
            processes,
            self.state.clone(),
            self.registry.clone(),
            crate::plugin_host::managed_packages::ManagedPackageLifecycle::new(
                self.registry.clone(),
            ),
        );
        lifecycle.set_private_storage(self.root.storage.clone());
        lifecycle
    }

    fn fingerprint(&self, plugin_id: &str) -> String {
        if plugin_id == ONE {
            "aa".repeat(32)
        } else {
            "bb".repeat(32)
        }
    }

    fn owner(&self, plugin_id: &str) -> OwnerScope {
        let resolver = self
            .root
            .providers
            .profiles()
            .resolver(plugin_id, &self.fingerprint(plugin_id), None)
            .unwrap();
        OwnerScope::new(
            *resolver.storage_owner().key(),
            self.root.providers.profiles().require().unwrap().epoch(),
        )
        .unwrap()
    }

    fn profile_id(&self) -> String {
        self.root
            .providers
            .profiles()
            .require()
            .unwrap()
            .profile_id()
            .to_owned()
    }

    fn private_dir(&self, plugin_id: &str) -> PathBuf {
        namespace_dir(
            &self.dir.path().join("plugin-data"),
            &self.fingerprint(plugin_id),
            plugin_id,
            &self.profile_id(),
        )
    }

    fn write_private(
        &self,
        plugin_id: &str,
        key: &str,
        value: &[u8],
        expires_at: Option<i64>,
        protected: bool,
    ) {
        let mut store = DurableStore::open(&self.private_dir(plugin_id)).unwrap();
        store
            .put(key, value, 1, expires_at, 100, QUOTA_BYTES)
            .unwrap();
        if protected {
            assert!(store.set_user_protected(key, true).unwrap());
        }
    }

    async fn publish(&self, plugin_id: &str, resource: &str, expires_at: Option<i64>) -> String {
        use gamenode_platform::plugin_shared_data::RecordProvenance;
        self.root
            .shared_pool
            .persist(DurableWrite {
                owner: self.owner(plugin_id),
                dataset_id: "org.example.catalog".to_owned(),
                schema_id: "org.example.catalog.v1".to_owned(),
                schema_version: 1,
                fields_json: format!(r#"{{"title":"{resource}"}}"#),
                opaque_payload: Vec::new(),
                resource_id: Some(resource.to_owned()),
                observed_at: 100,
                sequence: 1,
                source_observation_id: format!("observation-{resource}"),
                expires_at,
                now: 100,
                provenance: RecordProvenance {
                    source_plugin_id: plugin_id.to_owned(),
                    source_publisher_fingerprint: self.fingerprint(plugin_id),
                    source_profile_id: "account-one".to_owned(),
                    declared_developer: "Example Developer".to_owned(),
                    origin: "registry".to_owned(),
                    package_version: "1.0.0".to_owned(),
                    package_digest: vec![7; 32],
                },
            })
            .await
            .unwrap()
            .record_id
    }

    fn candidate_id(
        review: &crate::plugin_host::cleanup::StorageCleanupReview,
        kind: &str,
        plugin_id: &str,
    ) -> String {
        review
            .candidates
            .iter()
            .find(|candidate| candidate.kind == kind && candidate.plugin_id == plugin_id)
            .unwrap_or_else(|| panic!("no {kind} candidate for {plugin_id}"))
            .candidate_id
            .clone()
    }
}

#[tokio::test]
async fn cleanup_requires_confirmation_and_touches_only_selected_owners() {
    let fixture = Fixture::new();
    fixture.write_private(ONE, "unprotected", b"old", Some(150), false);
    fixture.write_private(ONE, "protected", b"keep", None, true);
    fixture.write_private(TWO, "other", b"other", None, false);
    // Make the private retention policy the path that removes ONE's old entry, as automatic
    // per-namespace retention would.
    let mut store = DurableStore::open(&fixture.private_dir(ONE)).unwrap();
    store
        .set_retention_policy(
            "private",
            RetentionPolicy {
                max_age_seconds: 10,
                max_bytes: 0,
            },
            100,
            2,
        )
        .unwrap();
    drop(store);

    let _expired = fixture.publish(ONE, "expired", Some(150)).await;
    let _live = fixture.publish(ONE, "live", None).await;
    let _two = fixture.publish(TWO, "other", None).await;

    let lifecycle = fixture.lifecycle();
    let review = lifecycle.storage_cleanup_review_at(100).await.unwrap();
    assert_eq!(review.candidates.len(), 4);
    let one_private = Fixture::candidate_id(&review, "private_persistence", ONE);
    let two_private = Fixture::candidate_id(&review, "private_persistence", TWO);
    let one_durable = Fixture::candidate_id(&review, "durable_shared", ONE);
    let two_durable = Fixture::candidate_id(&review, "durable_shared", TWO);
    let all = vec![
        one_private.clone(),
        two_private.clone(),
        one_durable.clone(),
        two_durable.clone(),
    ];

    // Unconfirmed, unknown-token and out-of-review selections change nothing.
    let unconfirmed = lifecycle
        .storage_cleanup_apply_at(review.token.clone(), all.clone(), false, 200)
        .await
        .unwrap_err();
    assert_eq!(unconfirmed.code(), tonic::Code::FailedPrecondition);
    let unknown = lifecycle
        .storage_cleanup_apply_at("not-a-token".into(), all.clone(), true, 200)
        .await
        .unwrap_err();
    assert_eq!(unknown.code(), tonic::Code::FailedPrecondition);
    let outside = lifecycle
        .storage_cleanup_apply_at(review.token.clone(), vec!["deadbeef".into()], true, 200)
        .await
        .unwrap_err();
    assert_eq!(outside.code(), tonic::Code::InvalidArgument);
    assert_eq!(
        fixture
            .root
            .shared_pool
            .usage(fixture.owner(ONE), 100)
            .await
            .unwrap()
            .unwrap()
            .records,
        2,
        "nothing was pruned before confirmation"
    );

    let report = lifecycle
        .storage_cleanup_apply_at(
            review.token.clone(),
            vec![one_private, one_durable],
            true,
            200,
        )
        .await
        .unwrap();
    assert_eq!(
        report.removed_records, 2,
        "one private and one expired durable"
    );
    assert!(report.reclaimed_bytes > 0);
    assert_eq!(report.skipped_protected_records, 1);
    assert_eq!(report.results.len(), 2);
    assert!(
        report
            .results
            .iter()
            .all(|result| result.status == "completed")
    );

    // ONE's protected private entry and live durable record survive; TWO is untouched.
    let store = DurableStore::open(&fixture.private_dir(ONE)).unwrap();
    assert!(store.get("unprotected", 200).unwrap().is_none());
    assert!(store.get("protected", 200).unwrap().is_some());
    drop(store);
    let usage = fixture
        .root
        .shared_pool
        .usage(fixture.owner(ONE), 200)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(usage.records, 1, "the live durable record remains");
    assert_eq!(
        fixture
            .root
            .shared_pool
            .usage(fixture.owner(TWO), 200)
            .await
            .unwrap()
            .unwrap()
            .records,
        1,
        "another owner is never crossed"
    );
    assert!(fixture.private_dir(TWO).exists());
    let store = DurableStore::open(&fixture.private_dir(TWO)).unwrap();
    assert!(store.get("other", 200).unwrap().is_some());

    // The token is single-use; retry needs a new review.
    let reused = lifecycle
        .storage_cleanup_apply_at(review.token, vec![all[2].clone()], true, 200)
        .await
        .unwrap_err();
    assert_eq!(reused.code(), tonic::Code::FailedPrecondition);
}

#[tokio::test]
async fn cleanup_reports_a_partial_failure_and_consumes_the_review() {
    let fixture = Fixture::new();
    fixture.write_private(ONE, "one", b"one", None, false);
    fixture.write_private(TWO, "two", b"two", None, false);
    let lifecycle = fixture.lifecycle();
    let review = lifecycle.storage_cleanup_review_at(100).await.unwrap();
    let one = Fixture::candidate_id(&review, "private_persistence", ONE);
    let two = Fixture::candidate_id(&review, "private_persistence", TWO);

    // TWO disappears between the review and the confirmation. Its owner cannot be resolved, so
    // the candidate fails without touching either store.
    fixture
        .root
        .providers
        .unregister_third_party_plugin(TWO)
        .await
        .unwrap();
    let report = lifecycle
        .storage_cleanup_apply_at(review.token.clone(), vec![one.clone(), two], true, 200)
        .await
        .unwrap();
    assert_eq!(report.results.len(), 2);
    assert_eq!(report.results[0].status, "completed");
    assert_eq!(report.results[1].status, "failed");
    assert!(report.results[1].error.is_some());
    assert_eq!(report.removed_records, 0, "nothing was over a limit");
    assert!(fixture.private_dir(TWO).exists());
    assert!(fixture.private_dir(ONE).exists());

    let reused = lifecycle
        .storage_cleanup_apply_at(review.token, vec![one], true, 200)
        .await
        .unwrap_err();
    assert_eq!(reused.code(), tonic::Code::FailedPrecondition);
}

#[tokio::test]
async fn cleanup_reclaims_archived_rows_only_when_that_owner_is_selected() {
    let fixture = Fixture::new();
    fixture.write_private(ONE, "kept", b"archived", None, false);
    fixture.write_private(TWO, "kept", b"live", None, false);
    fixture.publish(ONE, "archived", None).await;
    let mut store = DurableStore::open(&fixture.private_dir(ONE)).unwrap();
    store.archive_entries(2).unwrap();
    drop(store);
    fixture
        .root
        .shared_pool
        .archive_owner(fixture.owner(ONE), 100)
        .await
        .unwrap();

    let lifecycle = fixture.lifecycle();
    let review = lifecycle.storage_cleanup_review_at(100).await.unwrap();
    let one_private = Fixture::candidate_id(&review, "private_persistence", ONE);
    let two_private = Fixture::candidate_id(&review, "private_persistence", TWO);
    let report = lifecycle
        .storage_cleanup_apply_at(review.token, vec![one_private, two_private], true, 100)
        .await
        .unwrap();
    assert_eq!(report.removed_records, 1, "only ONE's archived private row");
    assert_eq!(
        report.reclaimed_bytes,
        gamenode_platform::plugin_storage::charge("kept", 8)
    );
    let remaining = DurableStore::open(&fixture.private_dir(ONE)).unwrap();
    assert_eq!(remaining.archived_usage().unwrap().records, 0);
    assert_eq!(
        fixture
            .root
            .shared_pool
            .usage(fixture.owner(ONE), 100)
            .await
            .unwrap()
            .unwrap()
            .archived_records,
        1,
        "the durable candidate was not selected"
    );
    let live = DurableStore::open(&fixture.private_dir(TWO)).unwrap();
    assert!(live.get("kept", 100).unwrap().is_some());

    let review = lifecycle.storage_cleanup_review_at(100).await.unwrap();
    let durable = Fixture::candidate_id(&review, "durable_shared", ONE);
    let report = lifecycle
        .storage_cleanup_apply_at(review.token, vec![durable], true, 100)
        .await
        .unwrap();
    assert_eq!(report.removed_records, 1);
    assert_eq!(
        fixture
            .root
            .shared_pool
            .usage(fixture.owner(ONE), 100)
            .await
            .unwrap()
            .unwrap()
            .archived_records,
        0
    );
}
