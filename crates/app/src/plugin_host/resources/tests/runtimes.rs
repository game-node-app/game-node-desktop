//! Cross-identity common runtime catalog and content-availability projections over public RPCs.

use super::*;
use crate::plugin_host::sharing_policy::ActivationReadGrant;
use gamenode_core::library::{
    LibraryStore, RuntimeContentBinding, RuntimeMutation, RuntimeOwnerIdentity, RuntimeResource,
};
use gamenode_platform::db::SqliteSettingsStore;
use gamenode_plugin_protocol::prost_types::{self, Value, value::Kind};
use sha2::{Digest, Sha256};
use std::path::Path;

struct SqliteRuntimeHost {
    store: gamenode_platform::library_db::SqliteLibraryStore,
}

fn storage_status(error: impl std::fmt::Display) -> Status {
    Status::internal(error.to_string())
}

#[tonic::async_trait]
impl LibraryHost for SqliteRuntimeHost {
    fn favorites(&self, _provider_id: &str) -> Vec<(u64, bool)> {
        Vec::new()
    }
    fn set_favorite(
        &self,
        _provider_id: &str,
        _game_id: u64,
        _favorite: bool,
        _guard: &dyn Fn() -> Result<(), Status>,
    ) -> Result<FavoriteWrite, Status> {
        Err(Status::unavailable("not used by runtime fixture"))
    }
    async fn search_catalog(
        &self,
        _query: String,
        _limit: usize,
    ) -> Result<Vec<CatalogGame>, Status> {
        Err(Status::unavailable("not used by runtime fixture"))
    }
    fn assign_match(
        &self,
        _entry_key: &str,
        _game_id: u64,
        _expected_revision: u64,
        _guard: &dyn Fn() -> Result<(), Status>,
    ) -> Result<MatchWrite, Status> {
        Err(Status::unavailable("not used by runtime fixture"))
    }
    async fn read_artwork(
        &self,
        _entry_key: String,
        _max_bytes: u64,
    ) -> Result<StoredArtwork, Status> {
        Err(Status::unavailable("not used by runtime fixture"))
    }
    async fn set_artwork(
        &self,
        _entry_key: String,
        _expected_revision: u64,
        _image: Option<Prepared>,
        _guard: CommitGuard,
    ) -> Result<ArtworkWrite, Status> {
        Err(Status::unavailable("not used by runtime fixture"))
    }
    async fn set_display_name(
        &self,
        _entry_key: String,
        _expected_revision: u64,
        _display_name: Option<String>,
        _guard: CommitGuard,
    ) -> Result<DisplayWrite, Status> {
        Err(Status::unavailable("not used by runtime fixture"))
    }
    async fn register_runtime_owner(&self, owner: RuntimeOwnerIdentity) -> Result<(), Status> {
        self.store
            .register_runtime_owner(&owner)
            .map_err(storage_status)
    }
    async fn runtime_catalog_records(
        &self,
        profile_id: String,
    ) -> Result<Vec<gamenode_core::library::RuntimeCatalogRecord>, Status> {
        self.store
            .runtime_catalog_records(&profile_id)
            .map_err(storage_status)
    }
    async fn runtime_common_revision(&self) -> Result<u64, Status> {
        self.store.runtime_common_revision().map_err(storage_status)
    }
    async fn runtime_resources(&self, owner: String) -> Result<Vec<RuntimeResource>, Status> {
        self.store.runtime_resources(&owner).map_err(storage_status)
    }
    async fn runtime_resource(
        &self,
        owner: String,
        id: String,
    ) -> Result<Option<RuntimeResource>, Status> {
        self.store
            .runtime_resource(&owner, &id)
            .map_err(storage_status)
    }
    async fn save_runtime_resource(
        &self,
        owner: String,
        runtime: RuntimeResource,
    ) -> Result<RuntimeMutation, Status> {
        self.store
            .save_runtime_resource(&owner, &runtime)
            .map_err(storage_status)
    }
    async fn remove_runtime_resource(
        &self,
        owner: String,
        id: String,
        revision: u64,
    ) -> Result<RuntimeMutation, Status> {
        self.store
            .remove_runtime_resource(&owner, &id, revision)
            .map_err(storage_status)
    }
    async fn runtime_content_bindings(
        &self,
        owner: String,
        game: u64,
        installation: String,
    ) -> Result<Vec<RuntimeContentBinding>, Status> {
        self.store
            .runtime_content_bindings(&owner, game, &installation)
            .map_err(storage_status)
    }
    async fn save_runtime_content_binding(
        &self,
        owner: String,
        binding: RuntimeContentBinding,
    ) -> Result<RuntimeMutation, Status> {
        self.store
            .save_runtime_content_binding(&owner, &binding)
            .map_err(storage_status)
    }
    async fn remove_runtime_content_binding(
        &self,
        owner: String,
        binding: RuntimeContentBinding,
    ) -> Result<RuntimeMutation, Status> {
        self.store
            .remove_runtime_content_binding(&owner, &binding)
            .map_err(storage_status)
    }
    async fn runtime_catalog_revision(&self, owner: String) -> Result<u64, Status> {
        self.store
            .runtime_catalog_revision(&owner)
            .map_err(storage_status)
    }
}

fn selector(categories: &[CommonDataCategory]) -> CommonDataSelector {
    CommonDataSelector {
        categories: categories.iter().map(|category| *category as i32).collect(),
        selected_record_ids: Vec::new(),
    }
}

fn value_string(value: &str) -> Value {
    Value {
        kind: Some(Kind::StringValue(value.into())),
    }
}

fn runtime_record(handle: String, id: &str) -> RuntimeRecord {
    let schema = compatibility_schema_json();
    RuntimeRecord {
        id: id.into(),
        name: "Fixture SNES Runtime".into(),
        version: "1.0".into(),
        executable_handle: handle,
        supported_formats: vec!["snes-rom".into()],
        extensions: [(
            "org.example.compat".into(),
            ExtensionPayload {
                schema_id: "org.example.compat.v1".into(),
                schema_version: 1,
                schema_digest: Sha256::digest(schema).to_vec(),
                data: Some(prost_types::Struct {
                    fields: [
                        ("mode".into(), value_string("accurate")),
                        ("notes".into(), value_string("private note")),
                    ]
                    .into(),
                }),
            },
        )]
        .into(),
        platform: "linux".into(),
        ..Default::default()
    }
}

fn compatibility_schema_json() -> &'static [u8] {
    br#"{"type":"object","properties":{"mode":{"type":"string","maxLength":40,"x-gamenode-classification":"public"},"notes":{"type":"string","maxLength":80,"x-gamenode-classification":"sensitive"}},"required":["mode"],"additionalProperties":false}"#
}

fn compatibility_schema() -> ExtensionSchemaDescriptor {
    ExtensionSchemaDescriptor {
        namespace: "org.example.compat".into(),
        schema_id: "org.example.compat.v1".into(),
        schema_version: 1,
        schema_digest: Sha256::digest(compatibility_schema_json()).to_vec(),
        schema_json: compatibility_schema_json().to_vec(),
        fields: vec![
            ExtensionSchemaField {
                path: "mode".into(),
                classification: ExtensionFieldClassification::Public as i32,
            },
            ExtensionSchemaField {
                path: "notes".into(),
                classification: ExtensionFieldClassification::Sensitive as i32,
            },
        ],
        optional: true,
    }
}

fn file_in(dir: &Path, name: &str, contents: &[u8]) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(name), contents).unwrap();
}

#[tokio::test]
async fn two_identities_share_only_opted_in_runtime_metadata_and_availability() {
    const A: &[&str] = &[
        cap::FILES_READ,
        cap::GAME_RUNTIME_READ,
        cap::GAME_RUNTIME_CREATE,
        cap::GAME_RUNTIME_UPDATE,
        cap::DISCOVERY,
        cap::LIBRARY_READ,
    ];
    const B: &[&str] = &[
        cap::FILES_READ,
        cap::GAME_RUNTIME_READ,
        cap::COMMON_RUNTIME_CATALOG_READ,
        cap::COMMON_CONTENT_AVAILABILITY_READ,
        cap::LIBRARY_READ,
        cap::LIBRARY_READ_ALL,
    ];
    let fixture = Fixture::new(A).await;
    fixture
        .api
        .set_extension_schemas(
            "fixture",
            "fixture.plugin",
            &"ab".repeat(32),
            vec![compatibility_schema()],
        )
        .unwrap();
    let second = fixture.second_identity(B).await;
    let recipient = fixture
        .api
        .authorize_resource(Some(&second.context), cap::COMMON_RUNTIME_CATALOG_READ)
        .unwrap()
        .namespace
        .unwrap()
        .identity()
        .clone();
    fixture
        .shared_policies
        .replace_recipient_policies(
            &recipient,
            0,
            ActivationReadGrant::new(
                [
                    CommonDataCategory::RuntimeCatalog,
                    CommonDataCategory::ContentAvailability,
                ],
                std::iter::empty(),
            )
            .unwrap(),
            vec![],
        )
        .unwrap();
    let persisted =
        crate::plugin_host::sharing_policy::SharedSharingPolicies::open(&fixture.plugin_data)
            .unwrap()
            .recipient_snapshot(&recipient)
            .unwrap();
    assert!(
        persisted
            .categories
            .contains(&CommonDataCategory::RuntimeCatalog)
    );
    assert!(persisted.source_recipient_policies.is_empty());

    let runtime_db =
        SqliteSettingsStore::open(&fixture._temp.path().join("runtime-catalog.sqlite3")).unwrap();
    fixture.feed.set_host(Arc::new(SqliteRuntimeHost {
        store: runtime_db.library_store(),
    }));
    let game_dir = fixture.game_dir("shared-game", &[("game.sfc", b"ROM bytes")]);
    file_in(&game_dir, "emulator", b"executable bytes");
    fixture.library_in(&[("fixture:1", Some(88), "Shared SNES Game", &game_dir)]);
    fixture.user_approving(&game_dir);
    let approved_root = fixture.root_handle("id-fixture:1", &["read"]).await;
    let executable = fixture.child(&approved_root, "emulator").await;
    let rom = fixture.child(&approved_root, "game.sfc").await;

    let registered = fixture
        .client()
        .runtimes_register(RuntimeRegisterRequest {
            mutation: Some(fixture.mutation("runtime-a", 0)),
            runtime: Some(runtime_record(executable, "runtime-snes")),
        })
        .await
        .unwrap()
        .into_inner()
        .runtime
        .unwrap();
    assert_eq!(
        registered.extensions["org.example.compat"]
            .data
            .as_ref()
            .unwrap()
            .fields["mode"],
        value_string("accurate")
    );

    let content_reply = fixture
        .client()
        .games_register_content(ContentRegistrationRequest {
            mutation: Some(fixture.mutation("content-a", 0)),
            content_id: String::new(),
            game_id: "88".into(),
            installation_id: "id-fixture:1".into(),
            content_handle: rom,
            content_kind: "snes-rom".into(),
            runtime_id: registered.id.clone(),
            supported_platforms: vec!["linux".into()],
        })
        .await
        .unwrap();
    let content_id = content_reply.into_inner().resource_id;

    let mut client_b = second.host.resource_client();
    let private = client_b
        .runtimes_list(DomainListRequest {
            page: Some(PageRequest {
                context: Some(second.context.clone()),
                ..Default::default()
            }),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert!(
        private.runtimes.is_empty(),
        "private-only default must hide A's runtime"
    );
    let catalog_selector = selector(&[CommonDataCategory::RuntimeCatalog]);
    let shared = client_b
        .runtimes_list(DomainListRequest {
            page: Some(PageRequest {
                context: Some(second.context.clone()),
                common_data_categories: Some(catalog_selector.clone()),
                ..Default::default()
            }),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(shared.runtimes.len(), 1);
    let projected = &shared.runtimes[0];
    assert!(projected.id.starts_with("shared-"));
    assert!(projected.executable_handle.is_empty());
    assert!(projected.argument_template.is_empty());
    assert_eq!(
        projected.extensions["org.example.compat"].schema_id,
        "org.example.compat.v1"
    );
    assert!(
        projected.extensions["org.example.compat"]
            .data
            .as_ref()
            .unwrap()
            .fields
            .contains_key("mode")
    );
    assert!(
        !projected.extensions["org.example.compat"]
            .data
            .as_ref()
            .unwrap()
            .fields
            .contains_key("notes")
    );
    assert_eq!(
        projected.provenance.as_ref().unwrap().producer_plugin_id,
        "fixture.plugin"
    );
    assert!(!format!("{projected:?}").contains("executable bytes"));
    assert!(!format!("{projected:?}").contains(game_dir.to_string_lossy().as_ref()));
    let fetched = client_b
        .runtimes_get(DomainGetRequest {
            context: Some(second.context.clone()),
            resource_id: projected.id.clone(),
            shared_records: Some(catalog_selector.clone()),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner()
        .runtime
        .unwrap();
    assert_eq!(fetched.id, projected.id);
    assert!(fetched.executable_handle.is_empty());
    let denied_mutation = client_b
        .runtimes_remove(DomainRemoveRequest {
            mutation: Some(MutationContext {
                context: Some(second.context.clone()),
                idempotency_key: "forbidden-remove".into(),
                expected_revision: fetched.revision,
                ..Default::default()
            }),
            resource_id: projected.id.clone(),
        })
        .await
        .unwrap_err();
    assert_eq!(denied_mutation.code(), tonic::Code::PermissionDenied);

    let mut watch = client_b
        .runtimes_watch(DomainWatchRequest {
            context: Some(second.context.clone()),
            after_revision: 0,
            shared_records: Some(catalog_selector),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    let first_event = tokio::time::timeout(Duration::from_secs(5), watch.message())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let runtime_event = if first_event.runtime.is_some() {
        first_event
    } else {
        tokio::time::timeout(Duration::from_secs(5), watch.message())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    };
    let Some(watched) = runtime_event.runtime else {
        panic!("runtime watch upsert expected")
    };
    assert_eq!(watched.id, projected.id);
    assert!(watched.executable_handle.is_empty());
    assert!(!format!("{watched:?}").contains("executable bytes"));
    assert!(!format!("{watched:?}").contains(game_dir.to_string_lossy().as_ref()));

    let availability_selector = selector(&[CommonDataCategory::ContentAvailability]);
    let library = client_b
        .library_list(LibraryListRequest {
            page: Some(PageRequest {
                context: Some(second.context.clone()),
                common_data_categories: Some(availability_selector.clone()),
                ..Default::default()
            }),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    let entry = library
        .entries
        .iter()
        .find(|entry| entry.game_id == "88")
        .unwrap();
    assert_eq!(entry.runtime_availability.len(), 1);
    assert_eq!(entry.runtime_availability[0].content_id, content_id);
    assert!(!format!("{entry:?}").contains("ROM bytes"));
    assert!(!format!("{entry:?}").contains(game_dir.to_string_lossy().as_ref()));
    let library_get = client_b
        .library_get(LibraryGetRequest {
            context: Some(second.context.clone()),
            game_id: "88".into(),
            common_data_categories: Some(availability_selector.clone()),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner()
        .entry
        .unwrap();
    assert_eq!(library_get.runtime_availability, entry.runtime_availability);
    assert!(!format!("{library_get:?}").contains("ROM bytes"));
    assert!(!format!("{library_get:?}").contains(game_dir.to_string_lossy().as_ref()));
    let mut library_watch = client_b
        .library_watch(LibraryWatchRequest {
            context: Some(second.context),
            after_revision: library.revision,
            common_data_categories: Some(availability_selector),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    fixture
        .client()
        .runtimes_update(RuntimeUpdateRequest {
            mutation: Some(fixture.mutation("runtime-update", 1)),
            runtime: Some(RuntimeRecord {
                id: "runtime-snes".into(),
                name: "Fixture SNES Runtime Updated".into(),
                version: "1.0".into(),
                platform: "linux".into(),
                supported_formats: vec!["snes-rom".into()],
                ..Default::default()
            }),
            field_mask: vec!["name".into()],
        })
        .await
        .unwrap();
    let library_event = tokio::time::timeout(Duration::from_secs(5), library_watch.message())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let event_entry = library_event.entry.unwrap();
    assert_eq!(event_entry.runtime_availability.len(), 1);
    assert!(!format!("{event_entry:?}").contains("ROM bytes"));
    assert!(!format!("{event_entry:?}").contains(game_dir.to_string_lossy().as_ref()));
}
