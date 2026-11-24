use super::*;
use gamenode_core::library::LibraryStore as _;
use gamenode_plugin_sdk::proto::{
    ExtensionNamespacePatch, ExtensionPatch, ExtensionPayload, extension_namespace_patch::Operation,
};
use gamenode_plugin_sdk::resources::{
    RuntimeArgumentTemplate as SdkArgument, RuntimeExecutableHints, RuntimeSpec,
};
use prost_types::{Struct, Value, value::Kind};
use sha2::{Digest, Sha256};
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tonic::Code;

use super::super::mutations::mutation_owner;
use crate::plugin_host::runtime_access::{InstalledProgram, RuntimeAccessRequest, RuntimeApprover};

#[derive(Default)]
pub(super) struct FakeRuntimeApprover {
    chosen: StdMutex<Option<std::path::PathBuf>>,
    requests: StdMutex<Vec<RuntimeAccessRequest>>,
    decline: AtomicBool,
    /// Revokes a grant or changes the profile while the picker is open.
    race: StdMutex<Option<(ProviderApi, Option<&'static str>)>>,
}

#[tonic::async_trait]
impl RuntimeApprover for FakeRuntimeApprover {
    async fn choose_executable(
        &self,
        request: RuntimeAccessRequest,
    ) -> Result<Option<std::path::PathBuf>, Status> {
        self.requests.lock().unwrap().push(request);
        let race = self.race.lock().unwrap().clone();
        if let Some((api, permission)) = race {
            if let Some(permission) = permission {
                api.revoke_session_permission("fixture", permission).await;
            } else {
                api.profiles().update(Some("changed"), true);
            }
        }
        if self.decline.load(Ordering::SeqCst) {
            return Ok(None);
        }
        Ok(self.chosen.lock().unwrap().clone())
    }
}

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

impl Fixture {
    /// Connects a runtime picker that approves `path`.
    pub(super) fn runtime_approving(&self, path: &std::path::Path) -> Arc<FakeRuntimeApprover> {
        let approver = Arc::new(FakeRuntimeApprover::default());
        *approver.chosen.lock().unwrap() = Some(path.to_path_buf());
        self.files.set_runtime_approver(approver.clone());
        approver
    }

    pub(super) fn runtime_request(
        &self,
        name: &str,
        hints: &[&str],
        key: &str,
        expected_revision: u64,
    ) -> RuntimeRequestAccessRequest {
        RuntimeRequestAccessRequest {
            mutation: Some(self.mutation(key, expected_revision)),
            runtime: Some(RuntimeRecord {
                name: name.into(),
                version: "1.0".into(),
                platform: "linux".into(),
                supported_formats: vec!["rom".into()],
                argument_template: vec![RuntimeArgumentTemplate {
                    value: Some(runtime_argument_template::Value::Literal("--safe".into())),
                }],
                ..Default::default()
            }),
            hints: Some(RuntimeAccessHints {
                executable_names: hints.iter().map(|hint| (*hint).into()).collect(),
                desktop_ids: Vec::new(),
            }),
        }
    }

    /// A fixture with one approved installation folder holding `emulator`, registered as the
    /// caller's runtime. The returned path is the installation folder.
    pub(super) async fn approved_runtime(
        grants: &[&'static str],
    ) -> (Self, RuntimeRecord, std::path::PathBuf) {
        let fixture = Self::new(grants).await;
        let dir = fixture.game_dir(
            "runtime",
            &[("emulator", b"binary"), ("content.rom", b"rom")],
        );
        fixture.library_in(&[("fixture:1", Some(3), "Emulated", &dir)]);
        let root = fixture.approve("id-fixture:1", "installation", &dir).await;
        let executable = fixture.child(&root, "emulator").await;
        fixture.with_host();
        let runtime = fixture.runtime_record(executable).await;
        (fixture, runtime, dir)
    }

    /// Registers an already approved executable handle as this plugin's runtime.
    async fn runtime_record(&self, executable_handle: String) -> RuntimeRecord {
        self.host
            .resources()
            .unwrap()
            .runtime_register(
                RuntimeSpec {
                    name: "Runtime".into(),
                    version: "1.0".into(),
                    executable_handle,
                    supported_formats: vec!["rom".into()],
                    platform: "linux".into(),
                    argument_template: vec![SdkArgument::Literal("--safe".into())],
                    extensions: HashMap::new(),
                },
                0,
                "register-runtime",
            )
            .await
            .unwrap()
            .runtime
            .unwrap()
    }

    /// Points a runtime at another approved executable and returns the revised record.
    pub(super) async fn update_runtime_executable(
        &self,
        runtime: &RuntimeRecord,
        executable_handle: String,
    ) -> RuntimeRecord {
        self.host
            .resources()
            .unwrap()
            .runtime_update(
                &runtime.id,
                RuntimeSpec {
                    name: runtime.name.clone(),
                    version: runtime.version.clone(),
                    executable_handle,
                    supported_formats: runtime.supported_formats.clone(),
                    platform: runtime.platform.clone(),
                    argument_template: vec![SdkArgument::Literal("--safe".into())],
                    extensions: HashMap::new(),
                },
                ["executable_handle"],
                runtime.revision,
                "update-runtime",
            )
            .await
            .unwrap()
            .runtime
            .unwrap()
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

fn executable_in(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[tokio::test]
async fn a_runtime_request_prompts_once_creates_the_owned_runtime_and_returns_no_path() {
    let fixture = Fixture::new(&[
        cap::GAME_RUNTIME_CREATE,
        cap::GAME_RUNTIME_READ,
        cap::FILES_READ,
    ])
    .await;
    let dir = fixture.game_dir("runtime", &[("dolphin-emu", b"binary")]);
    let executable = executable_in(&dir, "dolphin-emu");
    let host = fixture.with_host();
    host.installed_programs
        .lock()
        .unwrap()
        .push(InstalledProgram {
            display_name: "Dolphin Emulator".into(),
            executable: executable.to_string_lossy().into_owned(),
            desktop_file: "/usr/share/applications/dolphin-emu.desktop".into(),
            package_id: "dolphin-emu;2503-1;x86_64;extra".into(),
            package_manager: "pacman".into(),
        });
    let approver = fixture.runtime_approving(&executable);

    let reply = fixture
        .client()
        .runtimes_request_access(fixture.runtime_request(
            "Dolphin",
            &["dolphin-emu"],
            "request-runtime-1",
            0,
        ))
        .await
        .unwrap()
        .into_inner();
    let runtime = reply.runtime.expect("created runtime");
    assert_eq!(reply.outcome, MutationOutcome::Applied as i32);
    assert!(!runtime.id.is_empty());
    assert!(runtime.executable_handle.is_empty());
    assert_eq!(runtime.name, "Dolphin");
    assert!(!format!("{runtime:?}").contains(&dir.to_string_lossy().to_string()));

    let requests = approver.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].plugin_id, "fixture.plugin");
    assert_eq!(requests[0].candidates[0].path, executable.to_string_lossy());
    assert_eq!(requests[0].candidates[0].display_name, "Dolphin Emulator");
    assert_eq!(requests[0].candidates[0].version, "2503-1");

    // The ordinary read projection still gives no executable handle.
    let read = fixture
        .host
        .resources()
        .unwrap()
        .runtime_get(&runtime.id, None)
        .await
        .unwrap();
    assert!(read.executable_handle.is_empty());

    // The host can resolve the approved program for a launch, and the plugin does not get the
    // path through the runtime record.
    let caller = fixture.caller();
    assert_eq!(
        fixture
            .resource_api
            .resolve_runtime_executable(&caller, &runtime.id)
            .await
            .unwrap(),
        executable.canonicalize().unwrap().to_str().unwrap()
    );
    let approvals = fixture
        .resource_api
        .in_namespace(&caller, |namespace| namespace.runtime_approvals())
        .await
        .unwrap();
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals[0].runtime_id, runtime.id);
    assert!(approvals[0].revoked_at.is_none());

    // The same key is answered from the recorded outcome without a second prompt.
    let replay = fixture
        .client()
        .runtimes_request_access(fixture.runtime_request(
            "Dolphin",
            &["dolphin-emu"],
            "request-runtime-1",
            0,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(replay.outcome, MutationOutcome::Applied as i32);
    assert_eq!(replay.runtime.unwrap().id, runtime.id);
    assert_eq!(approver.requests.lock().unwrap().len(), 1);

    // A different key returns the valid owned runtime without prompting as well.
    let reused = fixture
        .client()
        .runtimes_request_access(fixture.runtime_request(
            "Dolphin",
            &["dolphin-emu"],
            "request-runtime-2",
            0,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(reused.outcome, MutationOutcome::AlreadyApplied as i32);
    assert_eq!(reused.runtime.unwrap().id, runtime.id);
    assert_eq!(approver.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn the_sdk_runtime_request_access_helper_sends_the_picker_request_without_a_handle() {
    let fixture = Fixture::new(&[cap::GAME_RUNTIME_CREATE, cap::FILES_READ]).await;
    let dir = fixture.game_dir("runtime", &[("dolphin-emu", b"binary")]);
    let executable = executable_in(&dir, "dolphin-emu");
    fixture.with_host();
    let approver = fixture.runtime_approving(&executable);
    let sdk = fixture.host.resources().unwrap();
    let spec = RuntimeSpec {
        name: "Dolphin via SDK".into(),
        version: "1.0".into(),
        executable_handle: String::new(),
        supported_formats: vec!["rom".into()],
        platform: "linux".into(),
        argument_template: vec![SdkArgument::Literal("--safe".into())],
        extensions: Default::default(),
    };
    let reply = sdk
        .runtime_request_access(
            spec,
            RuntimeExecutableHints {
                executable_names: vec!["dolphin-emu".into()],
                desktop_ids: Vec::new(),
            },
            0,
            "sdk-runtime-request",
        )
        .await
        .unwrap();
    assert_eq!(reply.outcome, MutationOutcome::Applied as i32);
    assert!(reply.runtime.unwrap().executable_handle.is_empty());
    assert_eq!(approver.requests.lock().unwrap().len(), 1);

    let refused = sdk
        .runtime_request_access(
            RuntimeSpec {
                name: "Bad".into(),
                version: "1.0".into(),
                executable_handle: "plugin-supplied-handle".into(),
                supported_formats: vec!["rom".into()],
                platform: "linux".into(),
                argument_template: Vec::new(),
                extensions: Default::default(),
            },
            RuntimeExecutableHints::default(),
            0,
            "sdk-runtime-handle",
        )
        .await
        .unwrap_err();
    assert_eq!(refused.code(), Code::InvalidArgument);
    assert_eq!(approver.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn an_unavailable_packagekit_inventory_still_offers_manual_browse() {
    let fixture = Fixture::new(&[cap::GAME_RUNTIME_CREATE, cap::FILES_READ]).await;
    let dir = fixture.game_dir("runtime", &[("dolphin-emu", b"binary")]);
    let executable = executable_in(&dir, "dolphin-emu");
    let host = fixture.with_host();
    host.inventory_down.store(true, Ordering::SeqCst);
    let approver = fixture.runtime_approving(&executable);
    let reply = fixture
        .client()
        .runtimes_request_access(fixture.runtime_request(
            "Dolphin",
            &["dolphin-emu"],
            "inventory-down",
            0,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(reply.outcome, MutationOutcome::Applied as i32);
    let requests = approver.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].candidates.is_empty());
}

#[tokio::test]
async fn a_declined_runtime_request_creates_no_runtime_or_approval() {
    let fixture = Fixture::new(&[cap::GAME_RUNTIME_CREATE, cap::FILES_READ]).await;
    let dir = fixture.game_dir("runtime", &[("dolphin-emu", b"binary")]);
    let executable = executable_in(&dir, "dolphin-emu");
    fixture.with_host();
    let approver = fixture.runtime_approving(&executable);
    approver.decline.store(true, Ordering::SeqCst);
    let caller = fixture.caller();
    let owner = mutation_owner(&caller);

    let status = fixture
        .client()
        .runtimes_request_access(fixture.runtime_request(
            "Dolphin",
            &["dolphin-emu"],
            "declined-runtime",
            0,
        ))
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    assert!(
        fixture
            .resource_api
            .library
            .host()
            .unwrap()
            .runtime_resources(owner)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        fixture
            .resource_api
            .in_namespace(&caller, |namespace| namespace.runtime_approvals())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_changed_executable_refuses_launch_and_requires_fresh_approval() {
    let fixture = Fixture::new(&[
        cap::GAME_RUNTIME_CREATE,
        cap::GAME_RUNTIME_READ,
        cap::FILES_READ,
    ])
    .await;
    let dir = fixture.game_dir("runtime", &[("dolphin-emu", b"binary")]);
    let executable = executable_in(&dir, "dolphin-emu");
    fixture.with_host();
    let approver = fixture.runtime_approving(&executable);
    let caller = fixture.caller();
    let runtime = fixture
        .client()
        .runtimes_request_access(fixture.runtime_request(
            "Dolphin",
            &["dolphin-emu"],
            "first-approval",
            0,
        ))
        .await
        .unwrap()
        .into_inner()
        .runtime
        .unwrap();
    assert_eq!(
        fixture
            .resource_api
            .resolve_runtime_executable(&caller, &runtime.id)
            .await
            .unwrap(),
        executable.canonicalize().unwrap().to_str().unwrap()
    );

    std::fs::write(&executable, b"replaced binary").unwrap();
    let refused = fixture
        .resource_api
        .resolve_runtime_executable(&caller, &runtime.id)
        .await
        .unwrap_err();
    assert_eq!(refused.code(), Code::FailedPrecondition);
    assert!(!refused.message().contains(dir.to_string_lossy().as_ref()));

    // The next request reapproves the same runtime; the user sees the picker again.
    let reapproved = fixture
        .client()
        .runtimes_request_access(fixture.runtime_request(
            "Dolphin",
            &["dolphin-emu"],
            "second-approval",
            1,
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(reapproved.outcome, MutationOutcome::Applied as i32);
    let reapproved = reapproved.runtime.unwrap();
    assert_eq!(reapproved.id, runtime.id);
    assert_eq!(reapproved.revision, runtime.revision + 1);
    assert_eq!(approver.requests.lock().unwrap().len(), 2);
    assert_eq!(
        fixture
            .resource_api
            .resolve_runtime_executable(&caller, &reapproved.id)
            .await
            .unwrap(),
        executable.canonicalize().unwrap().to_str().unwrap()
    );
}

#[tokio::test]
async fn a_grant_revoked_or_profile_changed_while_the_picker_is_open_creates_nothing() {
    for (permission, profile_change) in [(Some(cap::GAME_RUNTIME_CREATE), false), (None, true)] {
        let fixture = Fixture::new(&[cap::GAME_RUNTIME_CREATE, cap::FILES_READ]).await;
        let dir = fixture.game_dir("runtime", &[("dolphin-emu", b"binary")]);
        let executable = executable_in(&dir, "dolphin-emu");
        fixture.with_host();
        let approver = fixture.runtime_approving(&executable);
        *approver.race.lock().unwrap() = Some((fixture.api.clone(), permission));
        let caller = fixture.caller();
        let owner = mutation_owner(&caller);
        let status = fixture
            .client()
            .runtimes_request_access(fixture.runtime_request(
                "Dolphin",
                &["dolphin-emu"],
                if profile_change {
                    "changed-profile"
                } else {
                    "revoked-grant"
                },
                0,
            ))
            .await
            .unwrap_err();
        assert!(
            matches!(
                status.code(),
                Code::PermissionDenied | Code::FailedPrecondition
            ),
            "{status:?}"
        );
        assert!(
            fixture
                .resource_api
                .library
                .host()
                .unwrap()
                .runtime_resources(owner)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(approver.requests.lock().unwrap().len(), 1);
    }
}
