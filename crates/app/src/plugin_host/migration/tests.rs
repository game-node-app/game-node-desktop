use std::path::PathBuf;
use std::sync::Arc;

use gamenode_platform::plugin_shared_data::{OwnerScope, RecordProvenance};
use gamenode_platform::plugin_storage::{DurableStore, QUOTA_BYTES, namespace_dir};
use gamenode_plugin_protocol::capability as cap;
use gamenode_plugin_protocol::proto::{
    ExtensionFieldClassification, ExtensionSchemaDescriptor, ExtensionSchemaField,
};
use sha2::Digest;

use crate::plugin_host::grants::{
    GrantScope, PermissionDecision, PermissionRequest, PluginIdentity,
};
use crate::plugin_host::publisher_trust::{PublisherTrustStore, key_fingerprint};
use crate::plugin_host::runtime::ThirdPartyPluginLifecycle;
use crate::plugin_host::shared_pool::DurableWrite;
use crate::plugin_host::{
    PluginApi,
    provider::{ExtensionServices, Registration, RegistrationKind, RegistrationOrigin},
};

const PLUGIN: &str = "org.example.plugin";
const PROVIDER: &str = "provider-one";
const DATASET: &str = "org.example.plugin.catalog";
const SCHEMA: &str = "org.example.plugin.catalog.v1";

struct Fixture {
    dir: tempfile::TempDir,
    root: PluginApi,
    state: crate::plugin_host::profile::PluginStateLayout,
    registry: gamenode_platform::plugin_registry::PluginPackageRegistry,
    old_fingerprint: String,
    new_fingerprint: String,
}

impl Fixture {
    async fn new() -> Self {
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
        std::fs::create_dir_all(dir.path().join("state")).unwrap();
        root.set_state_layout(state.clone());
        root.providers.profiles().update(Some("account-one"), true);
        let registry = gamenode_platform::plugin_registry::PluginPackageRegistry::at(
            dir.path().join("registry"),
        )
        .unwrap();
        let old_key = [1; 32];
        let new_key = [2; 32];
        let old_fingerprint = key_fingerprint(&old_key);
        let new_fingerprint = key_fingerprint(&new_key);
        // The user re-trusted a changed publisher key: the trust store records the replaced key
        // and the registry now holds the new identity.
        {
            let mut trust = PublisherTrustStore::open(state.trust_dir()).unwrap();
            trust
                .confirm_first_key(PLUGIN, &old_key, &old_fingerprint)
                .unwrap();
            trust
                .retrust_key(PLUGIN, &new_key, &new_fingerprint)
                .unwrap();
        }
        let fixture = Self {
            dir,
            root,
            state,
            registry,
            old_fingerprint,
            new_fingerprint,
        };
        fixture.register(&fixture.new_fingerprint);
        fixture.declare_schemas();
        fixture
    }

    fn register(&self, fingerprint: &str) {
        self.root
            .providers
            .register(Registration {
                provider_id: PROVIDER.into(),
                plugin_id: PLUGIN.into(),
                declared_developer: "Example Developer".into(),
                package_digest: vec![7; 32],
                kind: RegistrationKind::Provider,
                origin: RegistrationOrigin::ThirdParty {
                    publisher_fingerprint: fingerprint.to_owned(),
                },
                capabilities: vec![cap::CONTEXT.into()],
                shared_catalog: false,
                playtime: Default::default(),
                services: Arc::new(ExtensionServices),
            })
            .unwrap();
    }

    fn declare_schemas(&self) {
        self.root
            .providers
            .set_extension_schemas(PROVIDER, PLUGIN, &self.new_fingerprint, vec![descriptor(1)])
            .unwrap();
    }

    fn lifecycle(&self) -> ThirdPartyPluginLifecycle {
        self.lifecycle_with_private(true)
    }

    fn lifecycle_without_private(&self) -> ThirdPartyPluginLifecycle {
        self.lifecycle_with_private(false)
    }

    fn lifecycle_with_private(&self, private: bool) -> ThirdPartyPluginLifecycle {
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
        if private {
            lifecycle.set_private_storage(self.root.storage.clone());
        }
        lifecycle
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

    fn owner(&self, fingerprint: &str) -> OwnerScope {
        let resolver = self
            .root
            .providers
            .profiles()
            .resolver(PLUGIN, fingerprint, None)
            .unwrap();
        OwnerScope::new(
            *resolver.storage_owner().key(),
            self.root.providers.profiles().require().unwrap().epoch(),
        )
        .unwrap()
    }

    fn private_dir(&self, fingerprint: &str) -> PathBuf {
        namespace_dir(
            &self.dir.path().join("plugin-data"),
            fingerprint,
            PLUGIN,
            &self.profile_id(),
        )
    }

    fn write_private(&self, fingerprint: &str, key: &str, value: &[u8]) {
        let mut store = DurableStore::open(&self.private_dir(fingerprint)).unwrap();
        store.put(key, value, 1, None, 100, QUOTA_BYTES).unwrap();
    }

    async fn publish(
        &self,
        fingerprint: &str,
        dataset: &str,
        resource: &str,
        schema_id: &str,
        schema_version: u32,
    ) -> String {
        let record = self
            .root
            .shared_pool
            .persist(DurableWrite {
                owner: self.owner(fingerprint),
                dataset_id: dataset.to_owned(),
                schema_id: schema_id.to_owned(),
                schema_version,
                fields_json: format!(r#"{{"title":"{resource}"}}"#),
                opaque_payload: Vec::new(),
                resource_id: Some(resource.to_owned()),
                observed_at: 100,
                sequence: 1,
                source_observation_id: format!("observation-{resource}"),
                expires_at: None,
                now: 100,
                provenance: RecordProvenance {
                    source_plugin_id: PLUGIN.to_owned(),
                    source_publisher_fingerprint: fingerprint.to_owned(),
                    source_profile_id: self.profile_id(),
                    declared_developer: "Example Developer".to_owned(),
                    origin: "registry".to_owned(),
                    package_version: "1.0.0".to_owned(),
                    package_digest: vec![7; 32],
                },
            })
            .await
            .unwrap();
        record.record_id
    }

    async fn seed_old_identity(&self) {
        self.write_private(&self.old_fingerprint, "settings", b"old");
        self.publish(&self.old_fingerprint, DATASET, "disc-1", SCHEMA, 1)
            .await;
        self.publish(
            &self.old_fingerprint,
            "com.other.data",
            "foreign-1",
            "com.other.data.v1",
            1,
        )
        .await;
    }

    fn grant_storage_persist(&self, fingerprint: &str) {
        let profile = self.root.providers.profiles().require().unwrap();
        let mut grants = self.state.open_grants(&profile).unwrap();
        grants
            .replace_grants(
                PluginIdentity {
                    plugin_id: PLUGIN.into(),
                    publisher_fingerprint: fingerprint.to_owned(),
                    profile_id: profile.profile_id().to_owned(),
                },
                &[PermissionRequest {
                    permission_id: cap::STORAGE_PERSIST.into(),
                    required: true,
                    allowed_scopes: [GrantScope::OwnPrivateNamespace].into_iter().collect(),
                }],
                &[PermissionDecision {
                    permission_id: cap::STORAGE_PERSIST.into(),
                    scope: GrantScope::OwnPrivateNamespace,
                    granted: true,
                }],
            )
            .unwrap();
    }

    async fn target_records(&self) -> u64 {
        self.root
            .shared_pool
            .usage(self.owner(&self.new_fingerprint), 100)
            .await
            .unwrap()
            .map_or(0, |usage| usage.records)
    }
}

fn descriptor(version: u32) -> ExtensionSchemaDescriptor {
    let schema_json = br#"{"type":"object","properties":{"title":{"type":"string"}}}"#.to_vec();
    ExtensionSchemaDescriptor {
        namespace: "org.example.plugin".into(),
        schema_id: SCHEMA.into(),
        schema_version: version,
        schema_digest: sha2::Sha256::digest(&schema_json).to_vec(),
        schema_json,
        fields: vec![ExtensionSchemaField {
            path: "title".into(),
            classification: ExtensionFieldClassification::Public as i32,
        }],
        optional: true,
    }
}

#[tokio::test]
async fn preview_reports_compatible_data_and_changes_nothing() {
    let fixture = Fixture::new().await;
    fixture.seed_old_identity().await;
    let lifecycle = fixture.lifecycle();

    let preview = lifecycle
        .publisher_migration_preview_at(PLUGIN.to_owned(), 100)
        .await
        .unwrap();
    assert_eq!(preview.plugin_id, PLUGIN);
    assert_eq!(preview.profile_id, fixture.profile_id());
    assert_eq!(
        preview.source_publisher_fingerprint,
        fixture.old_fingerprint
    );
    assert_eq!(
        preview.target_publisher_fingerprint,
        fixture.new_fingerprint
    );
    assert_eq!(
        preview.historical_publisher_fingerprints,
        vec![fixture.old_fingerprint.clone()]
    );
    assert_eq!(preview.shared_records, 2);
    assert_eq!(preview.shared_eligible_records, 1);
    assert_eq!(preview.shared_conflict_records, 0);
    assert_eq!(preview.shared_incompatible_schema_records, 1);
    assert_eq!(preview.shared_target_records, 0);
    assert_eq!(preview.private_records, 1);
    assert_eq!(preview.private_eligible_records, 1);
    assert!(preview.private_eligible_bytes > 0);
    assert!(preview.private_store_available);
    assert!(preview.can_migrate);
    assert!(preview.secrets_excluded);
    assert!(preview.grants_excluded);
    assert!(preview.sharing_review_required);
    assert!(preview.retention_review_required);
    let dataset = preview
        .datasets
        .iter()
        .find(|dataset| dataset.dataset_id == DATASET)
        .unwrap();
    assert_eq!(dataset.eligible_records, 1);
    let foreign = preview
        .datasets
        .iter()
        .find(|dataset| dataset.dataset_id == "com.other.data")
        .unwrap();
    assert_eq!(foreign.incompatible_schema_records, 1);
    assert_eq!(foreign.incompatible_schema_ids, vec!["com.other.data.v1"]);
    assert!(!preview.token.is_empty());

    // A preview never writes: the new identity has nothing and the old identity keeps everything.
    assert_eq!(fixture.target_records().await, 0);
    assert!(!fixture.private_dir(&fixture.new_fingerprint).exists());
    assert_eq!(
        fixture
            .root
            .shared_pool
            .usage(fixture.owner(&fixture.old_fingerprint), 100)
            .await
            .unwrap()
            .unwrap()
            .records,
        2
    );
    let source_store = DurableStore::open(&fixture.private_dir(&fixture.old_fingerprint)).unwrap();
    assert!(source_store.get("settings", 100).unwrap().is_some());
}

#[tokio::test]
async fn rejection_and_cancellation_leave_every_store_untouched() {
    let fixture = Fixture::new().await;
    fixture.seed_old_identity().await;
    let lifecycle = fixture.lifecycle();
    let preview = lifecycle
        .publisher_migration_preview_at(PLUGIN.to_owned(), 100)
        .await
        .unwrap();

    let rejected = lifecycle
        .publisher_migration_apply_at(preview.token.clone(), fixture.profile_id(), false, 100)
        .await
        .unwrap_err();
    assert_eq!(rejected.code(), tonic::Code::FailedPrecondition);
    assert_eq!(fixture.target_records().await, 0);
    assert!(!fixture.private_dir(&fixture.new_fingerprint).exists());

    // Cancelling drops the preview: a later confirmation has nothing to apply.
    lifecycle
        .publisher_migration_cancel(preview.token.clone())
        .await
        .unwrap();
    let cancelled = lifecycle
        .publisher_migration_apply_at(preview.token.clone(), fixture.profile_id(), true, 100)
        .await
        .unwrap_err();
    assert_eq!(cancelled.code(), tonic::Code::FailedPrecondition);
    assert_eq!(fixture.target_records().await, 0);
    assert!(!fixture.private_dir(&fixture.new_fingerprint).exists());

    // An unconfirmed apply of another preview also changes nothing.
    let preview = lifecycle
        .publisher_migration_preview_at(PLUGIN.to_owned(), 100)
        .await
        .unwrap();
    let unknown = lifecycle
        .publisher_migration_apply_at("not-a-token".into(), fixture.profile_id(), true, 100)
        .await
        .unwrap_err();
    assert_eq!(unknown.code(), tonic::Code::FailedPrecondition);
    assert_eq!(fixture.target_records().await, 0);
    // The issued preview is still applicable after the unknown token failed.
    let report = lifecycle
        .publisher_migration_apply_at(preview.token, fixture.profile_id(), true, 100)
        .await
        .unwrap();
    assert_eq!(report.copied_records, 2);
}

#[tokio::test]
async fn apply_copies_compatible_data_preserves_provenance_and_keeps_the_source() {
    let fixture = Fixture::new().await;
    fixture.seed_old_identity().await;
    let original_ids = fixture
        .root
        .shared_pool
        .list_dataset_any(DATASET.to_owned(), 100)
        .await
        .unwrap()
        .0
        .into_iter()
        .map(|record| record.record_id)
        .collect::<Vec<_>>();
    let lifecycle = fixture.lifecycle();
    let preview = lifecycle
        .publisher_migration_preview_at(PLUGIN.to_owned(), 100)
        .await
        .unwrap();
    let report = lifecycle
        .publisher_migration_apply_at(preview.token.clone(), fixture.profile_id(), true, 100)
        .await
        .unwrap();

    assert_eq!(report.migration_id, preview.migration_id);
    assert_eq!(report.copied_records, 2, "one private entry and one record");
    assert_eq!(report.conflict_records, 0);
    assert_eq!(report.incompatible_schema_records, 1);
    assert!(report.copied_bytes > 0);
    assert_eq!(report.results.len(), 2);
    assert!(
        report
            .results
            .iter()
            .all(|result| result.status == "completed")
    );
    assert!(report.secrets_excluded && report.grants_excluded);
    assert!(report.sharing_review_required && report.retention_review_required);

    // The new identity has exactly the compatible durable record, with the original provenance.
    assert_eq!(fixture.target_records().await, 1);
    let (records, _) = fixture
        .root
        .shared_pool
        .list_dataset_any(DATASET.to_owned(), 100)
        .await
        .unwrap();
    let migrated = records
        .iter()
        .find(|record| {
            record.provenance.source.publisher_fingerprint == fixture.old_fingerprint
                && !original_ids.contains(&record.record_id)
        })
        .expect("the copy is a new row with the original source provenance");
    assert_eq!(
        migrated.provenance.source.publisher_fingerprint,
        fixture.old_fingerprint
    );
    assert_eq!(migrated.provenance.package_version, "1.0.0");
    assert_eq!(records.len(), 2, "the original row and one copy");
    assert!(
        records
            .iter()
            .all(|record| record.dataset_id != "com.other.data"),
        "an incompatible schema is not copied"
    );

    // The private copy exists and the old identity keeps its own data.
    let target = DurableStore::open(&fixture.private_dir(&fixture.new_fingerprint)).unwrap();
    assert_eq!(target.get("settings", 100).unwrap().unwrap().value, b"old");
    drop(target);
    let source = DurableStore::open(&fixture.private_dir(&fixture.old_fingerprint)).unwrap();
    assert_eq!(source.get("settings", 100).unwrap().unwrap().value, b"old");
    drop(source);
    assert_eq!(
        fixture
            .root
            .shared_pool
            .usage(fixture.owner(&fixture.old_fingerprint), 100)
            .await
            .unwrap()
            .unwrap()
            .records,
        2
    );

    // Single use: a retry needs a new preview.
    let reused = lifecycle
        .publisher_migration_apply_at(preview.token, fixture.profile_id(), true, 100)
        .await
        .unwrap_err();
    assert_eq!(reused.code(), tonic::Code::FailedPrecondition);
}

#[tokio::test]
async fn another_profile_and_another_identity_are_isolated_and_grants_are_not_migrated() {
    let fixture = Fixture::new().await;
    fixture.seed_old_identity().await;
    fixture.grant_storage_persist(&fixture.old_fingerprint);
    let lifecycle = fixture.lifecycle();
    let preview = lifecycle
        .publisher_migration_preview_at(PLUGIN.to_owned(), 100)
        .await
        .unwrap();
    let first_profile = fixture.profile_id();
    let first_owner = fixture.owner(&fixture.old_fingerprint);
    let first_target_dir = fixture.private_dir(&fixture.new_fingerprint);

    // The review is bound to the profile that created it.
    fixture
        .root
        .providers
        .profiles()
        .update(Some("account-two"), true);
    let other_profile = fixture.profile_id();
    assert_ne!(other_profile, first_profile);
    let stale = lifecycle
        .publisher_migration_apply_at(preview.token.clone(), first_profile.clone(), true, 100)
        .await
        .unwrap_err();
    assert_eq!(stale.code(), tonic::Code::FailedPrecondition);
    assert!(!first_target_dir.exists());
    assert_eq!(
        fixture
            .root
            .shared_pool
            .usage(first_owner, 100)
            .await
            .unwrap()
            .unwrap()
            .records,
        2,
        "another profile's old identity keeps its records"
    );
    fixture
        .root
        .providers
        .profiles()
        .update(Some("account-one"), true);
    assert_eq!(fixture.profile_id(), first_profile);

    // A fresh review in the original profile applies; grants are still only the old identity's.
    let preview = lifecycle
        .publisher_migration_preview_at(PLUGIN.to_owned(), 100)
        .await
        .unwrap();
    let report = lifecycle
        .publisher_migration_apply_at(preview.token, first_profile, true, 100)
        .await
        .unwrap();
    assert_eq!(report.copied_records, 2);
    let (_, old_grants) = fixture
        .root
        .providers
        .profiles()
        .granted_permissions(PLUGIN, &fixture.old_fingerprint)
        .unwrap();
    assert!(old_grants.contains(cap::STORAGE_PERSIST));
    let (_, new_grants) = fixture
        .root
        .providers
        .profiles()
        .granted_permissions(PLUGIN, &fixture.new_fingerprint)
        .unwrap();
    assert!(
        new_grants.is_empty(),
        "a data migration never migrates an activation grant"
    );
}

#[tokio::test]
async fn rollback_removes_only_the_copies_and_keeps_the_old_identity_data() {
    let fixture = Fixture::new().await;
    fixture.seed_old_identity().await;
    let lifecycle = fixture.lifecycle();
    let preview = lifecycle
        .publisher_migration_preview_at(PLUGIN.to_owned(), 100)
        .await
        .unwrap();
    let report = lifecycle
        .publisher_migration_apply_at(preview.token, fixture.profile_id(), true, 100)
        .await
        .unwrap();
    assert_eq!(fixture.target_records().await, 1);

    let unconfirmed = lifecycle
        .publisher_migration_rollback(
            PLUGIN.to_owned(),
            report.migration_id.clone(),
            fixture.profile_id(),
            false,
        )
        .await
        .unwrap_err();
    assert_eq!(unconfirmed.code(), tonic::Code::FailedPrecondition);
    assert_eq!(fixture.target_records().await, 1);

    let wrong_id = lifecycle
        .publisher_migration_rollback(
            PLUGIN.to_owned(),
            "deadbeef".into(),
            fixture.profile_id(),
            true,
        )
        .await
        .unwrap_err();
    assert_eq!(wrong_id.code(), tonic::Code::FailedPrecondition);

    let rolled_back = lifecycle
        .publisher_migration_rollback(
            PLUGIN.to_owned(),
            report.migration_id,
            fixture.profile_id(),
            true,
        )
        .await
        .unwrap();
    assert_eq!(rolled_back.removed_records, 2);
    assert_eq!(rolled_back.kept_protected_records, 0);
    assert!(rolled_back.reclaimed_bytes > 0);
    assert_eq!(fixture.target_records().await, 0);
    let target = DurableStore::open(&fixture.private_dir(&fixture.new_fingerprint)).unwrap();
    assert!(target.get("settings", 100).unwrap().is_none());
    drop(target);
    // The old identity's original data is untouched by the migration and the rollback.
    assert_eq!(
        fixture
            .root
            .shared_pool
            .usage(fixture.owner(&fixture.old_fingerprint), 100)
            .await
            .unwrap()
            .unwrap()
            .records,
        2
    );
    let source = DurableStore::open(&fixture.private_dir(&fixture.old_fingerprint)).unwrap();
    assert_eq!(source.get("settings", 100).unwrap().unwrap().value, b"old");
}

#[tokio::test]
async fn a_missing_private_store_is_a_reported_partial_result() {
    let fixture = Fixture::new().await;
    fixture.seed_old_identity().await;
    let lifecycle = fixture.lifecycle_without_private();
    let preview = lifecycle
        .publisher_migration_preview_at(PLUGIN.to_owned(), 100)
        .await
        .unwrap();
    assert!(!preview.private_store_available);
    let report = lifecycle
        .publisher_migration_apply_at(preview.token, fixture.profile_id(), true, 100)
        .await
        .unwrap();
    assert_eq!(report.results.len(), 2);
    let private = report
        .results
        .iter()
        .find(|result| result.store == "private_persistence")
        .unwrap();
    assert_eq!(private.status, "unavailable");
    let durable = report
        .results
        .iter()
        .find(|result| result.store == "durable_shared")
        .unwrap();
    assert_eq!(durable.status, "completed");
    assert_eq!(durable.copied_records, 1);
    assert_eq!(report.copied_records, 1, "the durable copy still applies");
    assert_eq!(fixture.target_records().await, 1);
}

#[tokio::test]
async fn a_newer_declared_schema_version_is_reported_as_incompatible() {
    let fixture = Fixture::new().await;
    fixture
        .publish(&fixture.old_fingerprint, DATASET, "disc-1", SCHEMA, 2)
        .await;
    let lifecycle = fixture.lifecycle();
    let preview = lifecycle
        .publisher_migration_preview_at(PLUGIN.to_owned(), 100)
        .await
        .unwrap();
    assert_eq!(preview.shared_records, 1);
    assert_eq!(preview.shared_eligible_records, 0);
    assert_eq!(preview.shared_incompatible_schema_records, 1);
    assert!(!preview.can_migrate || preview.private_eligible_records > 0);
    let dataset = &preview.datasets[0];
    assert_eq!(dataset.incompatible_schema_ids, vec![SCHEMA]);
}
