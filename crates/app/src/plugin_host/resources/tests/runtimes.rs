use super::*;
use gamenode_core::library::LibraryStore as _;
use gamenode_plugin_sdk::proto::{
    ExtensionNamespacePatch, ExtensionPatch, ExtensionPayload, extension_namespace_patch::Operation,
};
use gamenode_plugin_sdk::resources::{RuntimeArgumentTemplate as SdkArgument, RuntimeSpec};
use prost_types::{Struct, Value, value::Kind};
use sha2::{Digest, Sha256};

struct PersistentRuntimeHost {
    database: std::sync::Arc<gamenode_platform::db::SqliteSettingsStore>,
}

#[tonic::async_trait]
impl LibraryHost for PersistentRuntimeHost {
    fn favorites(&self, _: &str) -> Vec<(u64, bool)> {
        Vec::new()
    }

    fn set_favorite(
        &self,
        _: &str,
        _: u64,
        _: bool,
        _: &dyn Fn() -> Result<(), Status>,
    ) -> Result<FavoriteWrite, Status> {
        Err(Status::unimplemented("fixture"))
    }

    async fn search_catalog(&self, _: String, _: usize) -> Result<Vec<CatalogGame>, Status> {
        Ok(Vec::new())
    }

    fn assign_match(
        &self,
        _: &str,
        _: u64,
        _: u64,
        _: &dyn Fn() -> Result<(), Status>,
    ) -> Result<MatchWrite, Status> {
        Err(Status::unimplemented("fixture"))
    }

    async fn read_artwork(&self, _: String, _: u64) -> Result<StoredArtwork, Status> {
        Err(Status::not_found("fixture"))
    }

    async fn set_artwork(
        &self,
        _: String,
        _: u64,
        _: Option<Prepared>,
        _: CommitGuard,
    ) -> Result<ArtworkWrite, Status> {
        Err(Status::unimplemented("fixture"))
    }

    async fn set_display_name(
        &self,
        _: String,
        _: u64,
        _: Option<String>,
        _: CommitGuard,
    ) -> Result<DisplayWrite, Status> {
        Err(Status::unimplemented("fixture"))
    }

    async fn runtime_resources(
        &self,
        owner_key: String,
    ) -> Result<Vec<gamenode_core::library::RuntimeResource>, Status> {
        let database = self.database.clone();
        tokio::task::spawn_blocking(move || database.library_store().runtime_resources(&owner_key))
            .await
            .map_err(|error| Status::internal(error.to_string()))?
            .map_err(|error| Status::internal(error.to_string()))
    }

    async fn runtime_resource(
        &self,
        owner_key: String,
        runtime_id: String,
    ) -> Result<Option<gamenode_core::library::RuntimeResource>, Status> {
        let database = self.database.clone();
        tokio::task::spawn_blocking(move || {
            database
                .library_store()
                .runtime_resource(&owner_key, &runtime_id)
        })
        .await
        .map_err(|error| Status::internal(error.to_string()))?
        .map_err(|error| Status::internal(error.to_string()))
    }

    async fn save_runtime_resource(
        &self,
        owner_key: String,
        runtime: gamenode_core::library::RuntimeResource,
    ) -> Result<gamenode_core::library::RuntimeMutation, Status> {
        let database = self.database.clone();
        tokio::task::spawn_blocking(move || {
            database
                .library_store()
                .save_runtime_resource(&owner_key, &runtime)
        })
        .await
        .map_err(|error| Status::internal(error.to_string()))?
        .map_err(|error| Status::internal(error.to_string()))
    }

    async fn runtime_catalog_revision(&self, owner_key: String) -> Result<u64, Status> {
        let database = self.database.clone();
        tokio::task::spawn_blocking(move || {
            database
                .library_store()
                .runtime_catalog_revision(&owner_key)
        })
        .await
        .map_err(|error| Status::internal(error.to_string()))?
        .map_err(|error| Status::internal(error.to_string()))
    }
}

fn extension(note: &str) -> ExtensionPayload {
    let schema = br#"{"type":"object","properties":{"note":{"type":"string","x-gamenode-classification":"public"}},"required":["note"],"additionalProperties":false}"#;
    ExtensionPayload {
        schema_id: "fixture.plugin.runtime.v1".into(),
        schema_version: 1,
        schema_digest: Sha256::digest(schema).to_vec(),
        data: Some(Struct {
            fields: BTreeMap::from([(
                "note".into(),
                Value {
                    kind: Some(Kind::StringValue(note.into())),
                },
            )]),
        }),
    }
}

fn runtime_schema() -> ExtensionSchemaDescriptor {
    let schema = br#"{"type":"object","properties":{"note":{"type":"string","x-gamenode-classification":"public"}},"required":["note"],"additionalProperties":false}"#;
    ExtensionSchemaDescriptor {
        namespace: "fixture.plugin.runtime".into(),
        schema_id: "fixture.plugin.runtime.v1".into(),
        schema_version: 1,
        schema_digest: Sha256::digest(schema).to_vec(),
        schema_json: schema.to_vec(),
        fields: vec![ExtensionSchemaField {
            path: "note".into(),
            classification: ExtensionFieldClassification::Public as i32,
        }],
        optional: true,
    }
}

fn extension_for(schema_id: &str, note: &str) -> ExtensionPayload {
    ExtensionPayload {
        schema_id: schema_id.into(),
        schema_version: 1,
        schema_digest: vec![9; 32],
        data: Some(Struct {
            fields: BTreeMap::from([(
                "note".into(),
                Value {
                    kind: Some(Kind::StringValue(note.into())),
                },
            )]),
        }),
    }
}

fn runtime(handle: String) -> RuntimeSpec {
    RuntimeSpec {
        name: "Runtime A".into(),
        version: "1.0".into(),
        executable_handle: handle,
        supported_formats: vec!["snes-rom".into()],
        platform: "linux".into(),
        argument_template: vec![SdkArgument::Literal("--safe".into())],
        extensions: HashMap::from([
            ("fixture.plugin.runtime".into(), extension("known-v1")),
            (
                "fixture.plugin.future".into(),
                extension_for("fixture.plugin.future.v1", "unknown-to-old-client"),
            ),
        ]),
    }
}

#[tokio::test]
async fn runtime_namespace_patches_are_owner_checked_revisioned_and_preserve_old_client_data() {
    let fixture = Fixture::new(&[
        cap::FILES_READ,
        cap::GAME_RUNTIME_CREATE,
        cap::GAME_RUNTIME_READ,
        cap::GAME_RUNTIME_UPDATE,
    ])
    .await;
    fixture
        .api
        .set_extension_schemas(
            "fixture",
            "fixture.plugin",
            &"ab".repeat(32),
            vec![runtime_schema()],
        )
        .unwrap();
    let database_path = fixture._temp.path().join("runtime.sqlite3");
    let database = std::sync::Arc::new(
        gamenode_platform::db::SqliteSettingsStore::open(&database_path).unwrap(),
    );
    fixture
        .feed
        .set_host(std::sync::Arc::new(PersistentRuntimeHost { database }));
    let dir = fixture.game_dir("runtime", &[("emulator", b"binary")]);
    fixture.library_in(&[("fixture:1", None, "Game", &dir)]);
    fixture.user_approving(&dir);
    let root = fixture.root_handle("id-fixture:1", &["read"]).await;
    let executable = fixture.child(&root, "emulator").await;
    let resources = fixture.host.resources().unwrap();
    let mut watch = resources.runtimes_watch(0, None).await.unwrap();
    let registered = resources
        .runtime_register(runtime(executable), 0, "register-runtime")
        .await
        .unwrap();
    assert_eq!(registered.outcome, MutationOutcome::Applied as i32);
    let first = registered.runtime.unwrap();
    assert_eq!(first.revision, 1);
    let mut invalid_known = extension("wrong shape");
    invalid_known.data.as_mut().unwrap().fields.insert(
        "note".into(),
        Value {
            kind: Some(Kind::NumberValue(7.0)),
        },
    );
    assert!(
        resources
            .runtime_update_patch(
                &first.id,
                runtime(String::new()),
                ExtensionPatch {
                    expected_revision: 1,
                    typed_field_mask: vec!["name".into()],
                    namespaces: vec![ExtensionNamespacePatch {
                        namespace: "fixture.plugin.runtime".into(),
                        operation: Some(Operation::Set(invalid_known)),
                    }],
                },
                std::iter::empty::<String>(),
                1,
                "reject-invalid-signed-schema-payload",
            )
            .await
            .is_err()
    );
    assert_eq!(
        resources
            .runtime_get(&first.id, None)
            .await
            .unwrap()
            .revision,
        1
    );
    assert_eq!(
        first.extensions["fixture.plugin.future"],
        extension_for("fixture.plugin.future.v1", "unknown-to-old-client")
    );
    let page = resources.runtimes_page(20, "", None).await.unwrap();
    assert_eq!(page.runtimes.len(), 1);
    assert_eq!(page.runtimes[0].id, first.id);
    assert_eq!(
        page.runtimes[0].extensions["fixture.plugin.future"],
        extension_for("fixture.plugin.future.v1", "unknown-to-old-client"),
        "the owner can list a usable typed runtime with its opaque optional namespace"
    );
    let event = watch
        .message()
        .await
        .unwrap()
        .expect("runtime registration should publish a watch event");
    assert_eq!(
        event.runtime.unwrap().extensions["fixture.plugin.future"],
        extension_for("fixture.plugin.future.v1", "unknown-to-old-client")
    );

    // An update from a client unaware of extensions changes a typed field only.
    let mut old = runtime(String::new());
    old.extensions.clear();
    let edited = resources
        .runtime_update(&first.id, old.clone(), ["name"], 1, "old-client-update")
        .await
        .unwrap();
    assert_eq!(edited.outcome, MutationOutcome::Applied as i32);
    let second = edited.runtime.unwrap();
    assert_eq!(
        second.extensions["fixture.plugin.future"],
        extension_for("fixture.plugin.future.v1", "unknown-to-old-client")
    );

    old.name = "Runtime patched".into();
    let patched = resources
        .runtime_update_patch(
            &first.id,
            old,
            ExtensionPatch {
                expected_revision: 2,
                typed_field_mask: vec!["name".into()],
                namespaces: vec![ExtensionNamespacePatch {
                    namespace: "fixture.plugin.runtime".into(),
                    operation: Some(Operation::Set(extension("known-v2"))),
                }],
            },
            std::iter::empty::<String>(),
            2,
            "patch-known-namespace",
        )
        .await
        .unwrap();
    assert_eq!(patched.outcome, MutationOutcome::Applied as i32);
    let third = patched.runtime.unwrap();
    assert_eq!(third.revision, 3);
    assert_eq!(third.name, "Runtime patched");
    assert_eq!(
        third.extensions["fixture.plugin.runtime"],
        extension("known-v2")
    );
    assert_eq!(
        third.extensions["fixture.plugin.future"],
        extension_for("fixture.plugin.future.v1", "unknown-to-old-client")
    );

    // A stale writer cannot change either the typed fields or namespace payloads.
    let stale = resources
        .runtime_update_patch(
            &first.id,
            runtime(String::new()),
            ExtensionPatch {
                expected_revision: 2,
                typed_field_mask: vec!["name".into()],
                namespaces: vec![ExtensionNamespacePatch {
                    namespace: "fixture.plugin.runtime".into(),
                    operation: Some(Operation::Set(extension("stale"))),
                }],
            },
            std::iter::empty::<String>(),
            2,
            "stale-patch",
        )
        .await
        .unwrap();
    assert_eq!(stale.outcome, MutationOutcome::RevisionConflict as i32);
    let current = resources.runtime_get(&first.id, None).await.unwrap();
    assert_eq!(current.revision, 3);
    assert_eq!(
        current.extensions["fixture.plugin.runtime"],
        extension("known-v2")
    );
    assert_eq!(
        current.extensions["fixture.plugin.future"],
        extension_for("fixture.plugin.future.v1", "unknown-to-old-client")
    );

    // Reopening SQLite between RPCs models a host restart while the client keeps its record ID.
    let database = std::sync::Arc::new(
        gamenode_platform::db::SqliteSettingsStore::open(&database_path).unwrap(),
    );
    fixture
        .feed
        .set_host(std::sync::Arc::new(PersistentRuntimeHost { database }));
    let reopened = resources.runtime_get(&first.id, None).await.unwrap();
    assert_eq!(reopened.revision, 3);
    assert_eq!(
        reopened.extensions["fixture.plugin.runtime"],
        extension("known-v2")
    );
    assert_eq!(
        reopened.extensions["fixture.plugin.future"],
        extension_for("fixture.plugin.future.v1", "unknown-to-old-client")
    );

    let foreign_namespace = resources
        .runtime_update_patch(
            &first.id,
            runtime(String::new()),
            ExtensionPatch {
                expected_revision: 3,
                namespaces: vec![ExtensionNamespacePatch {
                    namespace: "other.plugin.runtime".into(),
                    operation: Some(Operation::Set(extension("foreign"))),
                }],
                ..Default::default()
            },
            std::iter::empty::<String>(),
            3,
            "foreign-namespace-patch",
        )
        .await
        .unwrap_err();
    assert_eq!(foreign_namespace.code(), Code::PermissionDenied);
    let current = resources.runtime_get(&first.id, None).await.unwrap();
    assert_eq!(current.revision, 3);
    assert_eq!(
        current.extensions["fixture.plugin.runtime"],
        extension("known-v2")
    );

    // A writer without the update grant is rejected before touching the store.
    let readonly = Fixture::new(&[cap::FILES_READ, cap::GAME_RUNTIME_READ]).await;
    let denied = readonly
        .host
        .resources()
        .unwrap()
        .runtime_update_patch(
            &first.id,
            runtime(String::new()),
            ExtensionPatch::default(),
            std::iter::empty::<String>(),
            3,
            "read-only-patch",
        )
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
    assert_eq!(
        resources
            .runtime_get(&first.id, None)
            .await
            .unwrap()
            .revision,
        3
    );
}
