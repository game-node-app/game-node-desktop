use super::super::provider::{HostServices, Lease, Registration};
use super::*;
use gamenode_plugin_protocol::proto::*;
use gamenode_plugin_sdk::{Error, HostClient, ProviderPlugin, Session, capability};
use tonic::Status;

struct Services;

#[tonic::async_trait]
impl HostServices for Services {
    async fn context(&self, lease: Lease) -> Result<ProviderContext, Status> {
        lease.check()?;
        Ok(ProviderContext {
            provider_id: "fixture".into(),
            ..Default::default()
        })
    }
    async fn account(&self, _: Lease, _: account_request::Action) -> Result<AccountReply, Status> {
        Err(Status::unimplemented("fixture"))
    }
    async fn secret(&self, _: Lease, _: SecretRequest) -> Result<SecretReply, Status> {
        Err(Status::unimplemented("fixture"))
    }
    async fn login(&self, _: Lease, _: LoginRequest) -> Result<LoginReply, Status> {
        Err(Status::unimplemented("fixture"))
    }
    async fn actions(&self, _: Lease, _: ActionDeclaration) -> Result<ActionReply, Status> {
        Err(Status::unimplemented("fixture"))
    }
}

struct ChildPlugin;

#[tonic::async_trait]
impl ProviderPlugin for ChildPlugin {
    async fn handle(
        &self,
        command: ProviderCommand,
        host: HostClient,
    ) -> Result<OperationResult, Error> {
        if std::env::var("GAMENODE_TEST_PLUGIN_MODE").as_deref() == Ok("crash-once")
            && let Some(path) = std::env::var_os("GAMENODE_TEST_PLUGIN_CRASH_MARKER")
            && std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .is_ok()
        {
            std::process::exit(23);
        }
        assert_eq!(host.provider_context().await?.provider_id, "fixture");
        if std::env::var("GAMENODE_TEST_PLUGIN_MODE").as_deref() == Ok("provider-catalog")
            && command.operation == ProviderOperation::SyncCatalog as i32
        {
            let completion = host
                .catalog_chunks(
                    CatalogCategory::Owned,
                    vec![gamenode_plugin_protocol::models::CatalogRecord::new(
                        "fixture-game",
                        "Fixture Game",
                    )],
                )
                .await?;
            return Ok(OperationResult {
                disposition: OperationDisposition::Completed as i32,
                catalogs: vec![completion],
                ..Default::default()
            });
        }
        Ok(OperationResult {
            disposition: OperationDisposition::Completed as i32,
            ..Default::default()
        })
    }
}

// The test harness itself is a fixture executable. Only supervised child runs
// receive the explicit marker; ordinary suite execution does not connect out.
#[tokio::test]
async fn fixture_process() {
    if std::env::var("GAMENODE_TEST_PLUGIN_PROCESS").as_deref() != Ok("fixture") {
        return;
    }
    if let Ok(mode) = std::env::var("GAMENODE_TEST_PLUGIN_MODE")
        && mode.starts_with("extension-")
    {
        extension_fixture_process(&mode).await;
        return;
    }
    if std::env::var("GAMENODE_TEST_PLUGIN_MODE").as_deref() == Ok("stall") {
        tokio::time::sleep(Duration::from_secs(30)).await;
        return;
    }
    if std::env::var("GAMENODE_TEST_PLUGIN_MODE").as_deref() == Ok("linger") {
        // Retain the session even after the host invalidates it. This forces
        // supervision to stop and reap the old process before relaunching.
        let _session = Session::open(
            gamenode_plugin_sdk::Launch::from_env().unwrap(),
            "fixture.plugin",
            "fixture",
            "1.0",
            &[capability::CONTEXT],
        )
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_secs(30)).await;
        return;
    }
    let capabilities: &[&str] =
        if std::env::var("GAMENODE_TEST_PLUGIN_MODE").as_deref() == Ok("provider-catalog") {
            &[capability::CONTEXT, capability::CATALOG]
        } else {
            &[capability::CONTEXT]
        };
    let session = Session::open(
        gamenode_plugin_sdk::Launch::from_env().unwrap(),
        "fixture.plugin",
        "fixture",
        "1.0",
        capabilities,
    )
    .await
    .unwrap();
    if std::env::var("GAMENODE_TEST_PLUGIN_MODE").as_deref() == Ok("tree") {
        let output = std::env::var_os("GAMENODE_TEST_PLUGIN_TREE_OUTPUT").unwrap();
        let child = std::process::Command::new("sh")
            .args([
                "-c",
                "trap '' TERM; while :; do printf x >> \"$1\"; sleep 0.05; done",
                "plugin-child",
            ])
            .arg(&output)
            .spawn()
            .unwrap();
        let pid_path = std::env::var_os("GAMENODE_TEST_PLUGIN_TREE_PID").unwrap();
        std::fs::write(pid_path, child.id().to_string()).unwrap();
        std::mem::forget(child);
    }
    gamenode_plugin_sdk::run(session, Arc::new(ChildPlugin))
        .await
        .unwrap();
}

async fn assert_extension_actions_denied(
    channel: tonic::transport::Channel,
    context: &ServiceContext,
) {
    let target = context.target_game.as_ref().unwrap();
    let mut resource = resource_host_client::ResourceHostClient::new(channel);
    let launch = resource
        .games_request_launch(GameLaunchRequest {
            context: Some(context.clone()),
            installation_id: target.installation_id.clone(),
            game_id: target.game_id.clone(),
            expected_revision: target.revision.parse().unwrap(),
            idempotency_key: "probe-launch".into(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    let install = resource
        .games_request_install(GameInstallRequest {
            context: Some(context.clone()),
            installation_id: target.installation_id.clone(),
            game_id: target.game_id.clone(),
            idempotency_key: "probe-install".into(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    for denied in [launch, install] {
        assert_eq!(denied.code(), tonic::Code::PermissionDenied);
        assert_eq!(
            denied.message(),
            "extension session has no active target authority"
        );
    }
}

async fn extension_fixture_process(mode: &str) {
    let launch = gamenode_plugin_sdk::Launch::from_env().unwrap();
    let target = launch.selected_target().unwrap();
    let version = std::env::var("GAMENODE_TEST_PLUGIN_VERSION").unwrap();
    if matches!(
        mode,
        "extension-stall" | "extension-probe" | "extension-gated"
    ) {
        assert!(std::env::var_os("GAMENODE_PLUGIN_USER_INTENT_ID").is_none());
        let channel = gamenode_plugin_sdk::connect(&launch.endpoint)
            .await
            .unwrap();
        let minor = gamenode_plugin_protocol::API_MINOR;
        let reply = plugin_host_client::PluginHostClient::new(channel.clone())
            .open_session(OpenSessionRequest {
                api_major: 1,
                api_minor: minor,
                min_api_major: 1,
                min_api_minor: minor,
                max_api_major: 1,
                max_api_minor: minor,
                plugin_id: "fixture.plugin".into(),
                plugin_version: version,
                session_credential: launch.credential,
                requested_capabilities: vec![
                    capability::CONTEXT.into(),
                    capability::GAME_LAUNCH.into(),
                    capability::GAMES_INSTALL.into(),
                ],
                session_kind: SessionKind::Extension as i32,
                target_game: Some(target.clone()),
                ..Default::default()
            })
            .await
            .unwrap()
            .into_inner();
        assert!(
            reply
                .granted_capabilities
                .contains(&capability::GAME_LAUNCH.into())
        );
        assert!(
            reply
                .granted_capabilities
                .contains(&capability::GAMES_INSTALL.into())
        );
        let context = reply.resource_context.unwrap();
        assert!(context.provider_id.is_empty());
        assert_extension_actions_denied(channel.clone(), &context).await;
        let marker = std::env::var_os("GAMENODE_TEST_PLUGIN_OPEN_MARKER").unwrap();
        std::fs::write(&marker, b"OpenSession accepted, both actions denied").unwrap();
        if mode == "extension-stall" {
            tokio::time::sleep(Duration::from_secs(30)).await;
            return;
        }
        if mode == "extension-gated" {
            let ready = std::path::Path::new(&marker).with_file_name("allow-ready");
            while !ready.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
        let (events, receive) = tokio::sync::mpsc::channel(1);
        events
            .send(ProviderEvent {
                context: Some(ServiceContext {
                    operation_id: "initialization".into(),
                    profile_epoch: 0,
                    ..context
                }),
                event: Some(provider_event::Event::Ready(true)),
            })
            .await
            .unwrap();
        let mut commands = provider_host_client::ProviderHostClient::new(channel)
            .exchange(tokio_stream::wrappers::ReceiverStream::new(receive))
            .await
            .unwrap()
            .into_inner();
        while commands
            .message()
            .await
            .is_ok_and(|command| command.is_some())
        {}
        drop(events);
        return;
    }
    let mut wrong_target = target.clone();
    wrong_target.game_id.push('9');
    assert!(
        Session::open_extension(
            gamenode_plugin_sdk::Launch {
                endpoint: launch.endpoint.clone(),
                credential: launch.credential.clone(),
                operation_id: launch.operation_id.clone(),
            },
            "fixture.plugin",
            "1.0.0",
            wrong_target,
            &[capability::GAME_LAUNCH],
        )
        .await
        .is_err()
    );
    assert!(
        Session::open_extension(
            gamenode_plugin_sdk::Launch {
                endpoint: launch.endpoint.clone(),
                credential: launch.credential.clone(),
                operation_id: launch.operation_id.clone(),
            },
            "other.plugin",
            "1.0.0",
            target.clone(),
            &[capability::GAME_LAUNCH],
        )
        .await
        .is_err()
    );
    let mut session = Session::open_extension_from_launch(
        launch,
        "fixture.plugin",
        &version,
        &[capability::GAME_LAUNCH],
    )
    .await
    .unwrap();
    let context = session.host.resource_context().unwrap();
    assert!(context.provider_id.is_empty());
    assert_eq!(context.target_game.as_ref(), Some(&target));
    assert!(session.host.provider_context().await.is_err());
    let output = std::env::var_os("GAMENODE_TEST_PLUGIN_TREE_OUTPUT").unwrap();
    let mut child = std::process::Command::new("sh")
        .args([
            "-c",
            r#"trap '' TERM; while :; do printf x >> "$1"; sleep 0.05; done"#,
            "plugin-child",
        ])
        .arg(&output)
        .spawn()
        .unwrap();
    let pid_path = std::env::var_os("GAMENODE_TEST_PLUGIN_TREE_PID").unwrap();
    std::fs::write(&pid_path, child.id().to_string()).unwrap();
    // The fixture child outlives this session; reap it after the host tears down the tree.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    if mode == "extension-crash" {
        let marker = std::env::var_os("GAMENODE_TEST_PLUGIN_CRASH_MARKER").unwrap();
        while !std::path::Path::new(&marker).exists() {
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
        std::process::exit(23);
    }
    while session
        .commands
        .message()
        .await
        .is_ok_and(|command| command.is_some())
    {}
}

async fn manager(
    executable: PathBuf,
    mode: &str,
    timeout: Duration,
) -> (
    ProviderProcesses,
    super::super::PluginApiServer,
    tempfile::TempDir,
) {
    manager_with_origin(
        executable,
        mode,
        timeout,
        crate::plugin_host::provider::RegistrationOrigin::Official,
    )
    .await
}

async fn manager_with_origin(
    executable: PathBuf,
    mode: &str,
    timeout: Duration,
    origin: crate::plugin_host::provider::RegistrationOrigin,
) -> (
    ProviderProcesses,
    super::super::PluginApiServer,
    tempfile::TempDir,
) {
    manager_with_origin_and_lifecycle(executable, mode, timeout, origin, false).await
}

async fn manager_with_origin_and_lifecycle(
    executable: PathBuf,
    mode: &str,
    timeout: Duration,
    origin: crate::plugin_host::provider::RegistrationOrigin,
    managed_lifecycle: bool,
) -> (
    ProviderProcesses,
    super::super::PluginApiServer,
    tempfile::TempDir,
) {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let endpoint = directory.path().join("plugins.sock");
    let root = super::super::PluginApi::default();
    let extension = mode.starts_with("extension-");
    let slot_id = if extension {
        super::super::provider::extension_registration_id("fixture.plugin")
    } else {
        "fixture".into()
    };
    let mut capabilities = vec![capability::CONTEXT.into()];
    if extension {
        capabilities.push(capability::GAME_LAUNCH.into());
    }
    root.providers
        .register(Registration {
            declared_developer: "Fixture developer".into(),
            package_digest: Vec::new(),
            provider_id: slot_id.clone(),
            plugin_id: "fixture.plugin".into(),
            kind: if extension {
                crate::plugin_host::provider::RegistrationKind::Extension
            } else {
                crate::plugin_host::provider::RegistrationKind::Provider
            },
            origin,
            capabilities: capabilities.clone(),
            shared_catalog: false,
            playtime: Default::default(),
            services: Arc::new(Services),
        })
        .unwrap();
    let server = super::super::PluginApiServer::start(
        &tokio::runtime::Handle::current(),
        &endpoint,
        root.clone(),
    )
    .unwrap();
    capabilities.push("library.read".into());
    let config = vec![ProcessConfig {
        provider_id: slot_id,
        executable,
        arguments: vec![
            "--exact".into(),
            "plugin_host::process::tests::fixture_process".into(),
            "--nocapture".into(),
        ],
        environment: vec![
            ("GAMENODE_TEST_PLUGIN_PROCESS".into(), "fixture".into()),
            ("GAMENODE_TEST_PLUGIN_MODE".into(), mode.into()),
            (
                "GAMENODE_TEST_PLUGIN_CRASH_MARKER".into(),
                directory.path().join("crash-once").into_os_string(),
            ),
            (
                "GAMENODE_TEST_PLUGIN_TREE_PID".into(),
                directory.path().join("tree-child-pid").into_os_string(),
            ),
            (
                "GAMENODE_TEST_PLUGIN_TREE_OUTPUT".into(),
                directory.path().join("tree-child-output").into_os_string(),
            ),
        ],
        startup_timeout: timeout,
        manifest_capabilities: capabilities,
        required_capabilities: vec![],
        blocked_capabilities: vec![],
        integrity: None,
    }];
    let manager = if managed_lifecycle {
        ProviderProcesses::new_for_lifecycle(
            root.providers,
            endpoint,
            config,
            crate::plugin_host::profile::PluginStateLayout::new(if extension {
                directory.path().join("plugin-data/state")
            } else {
                directory.path().join("plugin-data")
            }),
        )
    } else {
        ProviderProcesses::new(root.providers, endpoint, config)
    }
    .unwrap();
    (manager, server, directory)
}

#[tokio::test]
async fn managed_package_integrity_failure_prevents_process_start() {
    let (mut processes, _server, directory) = manager(
        std::env::current_exe().unwrap(),
        "fixture",
        Duration::from_secs(5),
    )
    .await;
    let inner = Arc::get_mut(&mut processes.inner).unwrap();
    let slot = Arc::get_mut(inner.slots.get_mut().unwrap().get_mut("fixture").unwrap()).unwrap();
    slot.config.get_mut().unwrap().integrity = Some(PackageIntegrity {
        version: "1.0.0".into(),
        package_root: directory.path().to_owned(),
        publisher_key: [0; 32],
        emulation_policy: gamenode_platform::plugin_environment::EmulationPolicy::RequireNative,
    });
    let error = processes.ensure_started("fixture").await.unwrap_err();
    assert_eq!(error.code(), tonic::Code::FailedPrecondition);
    assert!(processes.process_id("fixture").await.is_none());
    processes.shutdown().await;
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn incompatible_signed_target_prevents_process_start() {
    use base64::Engine;
    use ring::signature::KeyPair;
    use sha2::Digest;
    use std::collections::BTreeMap;

    let package = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(package.path().join("bin")).unwrap();
    std::fs::create_dir_all(package.path().join("licenses")).unwrap();
    let executable = package.path().join("bin/fixture");
    std::fs::write(&executable, b"fixture executable").unwrap();
    std::fs::write(package.path().join("licenses/LICENSE"), b"fixture license").unwrap();

    let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[42; 32]).unwrap();
    let public_key = key.public_key().as_ref();
    let manifest = serde_json::to_vec(&serde_json::json!({
        "schema": 1,
        "id": "org.example.fixture",
        "kind": "provider",
        "provider_id": "fixture",
        "name": "Fixture",
        "version": "1.0.0",
        "api": {"min": "1.6.0", "max": "1.6.0"},
        "targets": [{"os": "windows", "arch": "x86_64", "abi": "msvc", "entrypoint": "bin/fixture"}],
        "publisher": {
            "key_id": "fixture-key",
            "public_key": base64::engine::general_purpose::STANDARD.encode(public_key)
        },
        "license": {"expression": "MIT", "files": ["licenses/LICENSE"]}
    }))
    .unwrap();
    std::fs::write(package.path().join("plugin.json"), &manifest).unwrap();
    let payloads = [
        ("plugin.json", manifest.as_slice()),
        ("bin/fixture", b"fixture executable".as_slice()),
        ("licenses/LICENSE", b"fixture license".as_slice()),
    ];
    let files = payloads
        .into_iter()
        .map(|(path, bytes)| {
            (
                path.to_owned(),
                format!("{:x}", sha2::Sha256::digest(bytes)),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let integrity = serde_json::to_vec(&serde_json::json!({"schema": 1, "files": files})).unwrap();
    let mut signed = gamenode_platform::plugin_packages::SIGNATURE_DOMAIN_PREFIX.to_vec();
    signed.extend_from_slice(&integrity);
    std::fs::write(package.path().join("integrity.json"), &integrity).unwrap();
    std::fs::write(
        package.path().join("signature.ed25519"),
        key.sign(&signed).as_ref(),
    )
    .unwrap();

    let (mut processes, _server, _directory) =
        manager(executable, "normal", Duration::from_secs(5)).await;
    let inner = Arc::get_mut(&mut processes.inner).unwrap();
    let slot = Arc::get_mut(inner.slots.get_mut().unwrap().get_mut("fixture").unwrap()).unwrap();
    slot.config.get_mut().unwrap().integrity = Some(PackageIntegrity {
        version: "1.0.0".into(),
        package_root: package.path().to_owned(),
        publisher_key: public_key.try_into().unwrap(),
        emulation_policy: gamenode_platform::plugin_environment::EmulationPolicy::RequireNative,
    });
    let error = processes.ensure_started("fixture").await.unwrap_err();
    assert_eq!(error.code(), tonic::Code::FailedPrecondition);
    assert!(error.message().contains("OperatingSystemMismatch"));
    assert!(processes.process_id("fixture").await.is_none());
    processes.shutdown().await;
}

#[tokio::test]
async fn a_stopped_provider_refuses_work_until_started_with_a_fresh_session() {
    let (processes, _server, _directory) = manager(
        std::env::current_exe().unwrap(),
        "normal",
        Duration::from_secs(10),
    )
    .await;
    processes.ensure_started("fixture").await.unwrap();
    let first_pid = processes.process_id("fixture").await.unwrap();

    processes.stop_provider("fixture").await;
    assert!(processes.is_stopped("fixture"));
    assert_eq!(processes.process_id("fixture").await, None);
    assert!(!std::path::Path::new(&format!("/proc/{first_pid}")).exists());

    // While stopped, commands fail with the stable error instead of relaunching the process.
    let error = processes
        .execute("fixture", ProviderOperation::Initialize)
        .await
        .unwrap_err();
    assert_eq!(error.code(), tonic::Code::Unavailable);
    assert_eq!(error.message(), "plugin stopped");
    assert_eq!(processes.process_id("fixture").await, None);

    // Starting creates a fresh session rather than reusing the stopped process.
    processes.start_provider("fixture").await.unwrap();
    assert!(!processes.is_stopped("fixture"));
    let second_pid = processes.process_id("fixture").await.unwrap();
    assert_ne!(first_pid, second_pid);
    assert!(!std::path::Path::new(&format!("/proc/{first_pid}")).exists());
    processes
        .execute("fixture", ProviderOperation::Initialize)
        .await
        .unwrap();
    processes.shutdown().await;
}

#[tokio::test]
async fn concurrent_work_reuses_one_child_and_shutdown_reaps_it() {
    let (processes, _server, _directory) = manager(
        std::env::current_exe().unwrap(),
        "normal",
        Duration::from_secs(10),
    )
    .await;
    let (first, second) = tokio::join!(
        processes.execute("fixture", ProviderOperation::Initialize),
        processes.execute("fixture", ProviderOperation::Initialize)
    );
    first.unwrap();
    second.unwrap();
    let pid = processes.process_id("fixture").await.unwrap();
    processes
        .execute("fixture", ProviderOperation::Initialize)
        .await
        .unwrap();
    assert_eq!(processes.process_id("fixture").await, Some(pid));
    processes.shutdown().await;
    assert_eq!(processes.process_id("fixture").await, None);
    assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
    assert!(!processes.inner.api.connected("fixture"));
}

#[tokio::test]
async fn shutdown_reaps_plugin_descendants_after_the_main_process_exits() {
    let (processes, _server, directory) = manager(
        std::env::current_exe().unwrap(),
        "tree",
        Duration::from_secs(10),
    )
    .await;
    processes
        .execute("fixture", ProviderOperation::Initialize)
        .await
        .unwrap();
    let pid_path = directory.path().join("tree-child-pid");
    tokio::time::timeout(Duration::from_secs(3), async {
        while !pid_path.is_file() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let child_pid: u32 = std::fs::read_to_string(pid_path).unwrap().parse().unwrap();
    let output = directory.path().join("tree-child-output");
    tokio::time::timeout(Duration::from_secs(3), async {
        while std::fs::metadata(&output).map_or(true, |metadata| metadata.len() == 0) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(std::path::Path::new(&format!("/proc/{child_pid}")).exists());

    processes.stop_provider("fixture").await;
    let stopped_at = std::fs::metadata(&output).unwrap().len();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(std::fs::metadata(&output).unwrap().len(), stopped_at);
    processes.shutdown().await;
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn profile_change_stops_third_party_process_but_not_official_processes() {
    let (official, _official_server, _official_directory) = manager(
        std::env::current_exe().unwrap(),
        "linger",
        Duration::from_secs(10),
    )
    .await;
    official.ensure_started("fixture").await.unwrap();
    let official_pid = official.process_id("fixture").await.unwrap();
    let (processes, _server, _directory) = manager_with_origin(
        std::env::current_exe().unwrap(),
        "linger",
        Duration::from_secs(10),
        crate::plugin_host::provider::RegistrationOrigin::ThirdParty {
            publisher_fingerprint: "fixture-publisher".into(),
        },
    )
    .await;
    let profiles = processes.inner.api.profiles();
    profiles.update(Some("account-one"), true);
    processes.ensure_started("fixture").await.unwrap();
    let old_pid = processes.process_id("fixture").await.unwrap();

    profiles.update(None, false);
    tokio::time::timeout(Duration::from_secs(5), async {
        while processes.process_id("fixture").await.is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("sign-out should stop the third-party process");
    assert!(!std::path::Path::new(&format!("/proc/{old_pid}")).exists());
    assert_eq!(official.process_id("fixture").await, Some(official_pid));
    assert!(std::path::Path::new(&format!("/proc/{official_pid}")).exists());
    processes.shutdown().await;
    official.shutdown().await;
}

#[tokio::test]
async fn managed_lifecycle_refuses_disabled_plugins_and_requires_an_active_profile() {
    let (processes, _server, directory) = manager_with_origin_and_lifecycle(
        std::env::current_exe().unwrap(),
        "linger",
        Duration::from_secs(10),
        crate::plugin_host::provider::RegistrationOrigin::ThirdParty {
            publisher_fingerprint: "fixture-publisher".into(),
        },
        true,
    )
    .await;
    let state =
        crate::plugin_host::profile::PluginStateLayout::new(directory.path().join("plugin-data"));

    assert_eq!(
        processes
            .ensure_started("fixture")
            .await
            .unwrap_err()
            .code(),
        tonic::Code::Unavailable
    );
    let profiles = processes.inner.api.profiles();
    profiles.update(Some("account-one"), true);
    assert_eq!(
        processes
            .ensure_started("fixture")
            .await
            .unwrap_err()
            .code(),
        tonic::Code::PermissionDenied
    );
    let profile = profiles.current().unwrap();
    state
        .open_activation(&profile)
        .unwrap()
        .set_enabled("fixture.plugin", true)
        .unwrap();
    processes.ensure_started("fixture").await.unwrap();
    assert!(processes.process_id("fixture").await.is_some());
    processes.shutdown().await;
}

#[tokio::test]
async fn update_readiness_can_start_a_disabled_plugin_without_persisting_enablement() {
    let (processes, _server, directory) = manager_with_origin_and_lifecycle(
        std::env::current_exe().unwrap(),
        "linger",
        Duration::from_secs(10),
        crate::plugin_host::provider::RegistrationOrigin::ThirdParty {
            publisher_fingerprint: "fixture-publisher".into(),
        },
        true,
    )
    .await;
    let profiles = processes.inner.api.profiles();
    profiles.update(Some("account-one"), true);
    let state =
        crate::plugin_host::profile::PluginStateLayout::new(directory.path().join("plugin-data"));
    let profile = profiles.current().unwrap();
    assert!(
        !state
            .open_activation(&profile)
            .unwrap()
            .is_enabled("fixture.plugin")
    );
    assert_eq!(
        processes
            .ensure_started("fixture")
            .await
            .unwrap_err()
            .code(),
        tonic::Code::PermissionDenied
    );

    processes.ensure_ready_for_update("fixture").await.unwrap();
    assert!(processes.inner.api.connected("fixture"));
    processes.stop_provider("fixture").await;
    assert!(
        !state
            .open_activation(&profile)
            .unwrap()
            .is_enabled("fixture.plugin")
    );
    processes.shutdown().await;
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn activation_persists_per_account_and_remove_stops_unregisters_and_deletes_packages() {
    let (processes, _server, directory) = manager_with_origin_and_lifecycle(
        std::env::current_exe().unwrap(),
        "linger",
        Duration::from_secs(10),
        crate::plugin_host::provider::RegistrationOrigin::ThirdParty {
            publisher_fingerprint: "fixture-publisher".into(),
        },
        true,
    )
    .await;
    let api = processes.inner.api.clone();
    api.profiles().update(Some("account-one"), true);
    let package_source = directory.path().join("package-source");
    std::fs::create_dir(&package_source).unwrap();
    std::fs::write(package_source.join("plugin.bin"), b"fixture").unwrap();
    let registry = gamenode_platform::plugin_registry::PluginPackageRegistry::at(
        directory.path().join("packages"),
    )
    .unwrap();
    registry
        .install_verified("fixture.plugin", "1.0.0", &package_source, true)
        .unwrap();
    let lifecycle =
        std::sync::Arc::new(crate::plugin_host::runtime::ThirdPartyPluginLifecycle::new(
            api.clone(),
            processes.clone(),
            crate::plugin_host::profile::PluginStateLayout::new(
                directory.path().join("plugin-data"),
            ),
            registry.clone(),
            crate::plugin_host::managed_packages::ManagedPackageLifecycle::new(registry.clone()),
        ));
    lifecycle.spawn_profile_lifecycle(&tokio::runtime::Handle::current());

    lifecycle.enable("fixture.plugin".into()).await.unwrap();
    let pid = processes.process_id("fixture").await.unwrap();
    assert!(lifecycle.enabled("fixture.plugin".into()).await.unwrap());

    api.profiles().update(Some("account-two"), true);
    tokio::time::timeout(Duration::from_secs(5), async {
        while processes.process_id("fixture").await.is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("changing accounts should stop the old process");
    assert!(!lifecycle.enabled("fixture.plugin".into()).await.unwrap());
    api.profiles().update(Some("account-one"), true);
    tokio::time::timeout(Duration::from_secs(5), async {
        while processes.process_id("fixture").await.is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the previously enabled plugin should restart for its account");
    assert!(processes.process_id("fixture").await.is_some());
    assert!(lifecycle.enabled("fixture.plugin".into()).await.unwrap());

    lifecycle.disable("fixture.plugin".into()).await.unwrap();
    assert!(processes.process_id("fixture").await.is_none());
    assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
    assert!(!lifecycle.enabled("fixture.plugin".into()).await.unwrap());

    lifecycle.enable("fixture.plugin".into()).await.unwrap();
    lifecycle.remove("fixture.plugin".into()).await.unwrap();
    assert!(processes.process_id("fixture").await.is_none());
    assert!(
        api.third_party_provider_ids_for_plugin("fixture.plugin")
            .is_empty()
    );
    assert!(registry.active("fixture.plugin").unwrap().is_none());
    processes.shutdown().await;
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn failed_update_rolls_back_version_and_keeps_changed_permission_revoked() {
    let (processes, _server, directory) = manager_with_origin_and_lifecycle(
        std::env::current_exe().unwrap(),
        "linger",
        Duration::from_secs(10),
        crate::plugin_host::provider::RegistrationOrigin::ThirdParty {
            publisher_fingerprint: "fixture-publisher".into(),
        },
        true,
    )
    .await;
    let api = processes.inner.api.clone();
    api.profiles().update(Some("account-one"), true);
    let state =
        crate::plugin_host::profile::PluginStateLayout::new(directory.path().join("plugin-data"));
    let profile = api.profiles().current().unwrap();
    let identity = crate::plugin_host::grants::PluginIdentity {
        plugin_id: "fixture.plugin".into(),
        publisher_fingerprint: "fixture-publisher".into(),
        profile_id: profile.profile_id().into(),
    };
    let permission = "library.read";
    let scope = crate::plugin_host::grants::GrantScope::Declared {
        value: "fixture provider data".into(),
    };
    let request = crate::plugin_host::grants::PermissionRequest {
        permission_id: permission.into(),
        required: false,
        allowed_scopes: std::collections::BTreeSet::from([scope.clone()]),
    };
    state
        .open_grants(&profile)
        .unwrap()
        .replace_grants(
            identity.clone(),
            &[request],
            &[crate::plugin_host::grants::PermissionDecision {
                permission_id: permission.into(),
                scope,
                granted: true,
            }],
        )
        .unwrap();

    let package_source = directory.path().join("update-source");
    std::fs::create_dir(&package_source).unwrap();
    std::fs::write(package_source.join("plugin.bin"), b"fixture update").unwrap();
    let registry = gamenode_platform::plugin_registry::PluginPackageRegistry::at(
        directory.path().join("update-registry"),
    )
    .unwrap();
    registry
        .install_verified("fixture.plugin", "1.0.0", &package_source, true)
        .unwrap();
    let candidate = registry
        .install_verified("fixture.plugin", "2.0.0", &package_source, false)
        .unwrap();
    let lifecycle = crate::plugin_host::runtime::ThirdPartyPluginLifecycle::new(
        api,
        processes.clone(),
        state.clone(),
        registry.clone(),
        crate::plugin_host::managed_packages::ManagedPackageLifecycle::new(registry.clone()),
    );
    lifecycle.enable("fixture.plugin".into()).await.unwrap();
    assert!(processes.process_id("fixture").await.is_some());

    let mut candidate_config = processes.config("fixture").unwrap();
    candidate_config.environment = vec![("GAMENODE_TEST_PLUGIN_MODE".into(), "stall".into())];
    candidate_config.startup_timeout = Duration::from_millis(150);
    candidate_config.blocked_capabilities = vec![permission.into()];
    let error = lifecycle
        .activate_update(
            "fixture.plugin".into(),
            "1.0.0".into(),
            candidate,
            candidate_config,
            None,
        )
        .await
        .unwrap_err();

    assert!(matches!(
        error.code(),
        tonic::Code::DeadlineExceeded | tonic::Code::Unavailable
    ));
    assert_eq!(
        registry.active("fixture.plugin").unwrap().unwrap().version,
        "1.0.0"
    );
    let persisted = state.open_grants(&profile).unwrap();
    assert_eq!(
        persisted.effective_grants(&identity)[0].state,
        crate::plugin_host::grants::PermissionState::Denied
    );
    assert_eq!(
        processes
            .config("fixture")
            .unwrap()
            .environment
            .iter()
            .find(|(key, _)| key == "GAMENODE_TEST_PLUGIN_MODE")
            .map(|(_, value)| value),
        Some(&std::ffi::OsString::from("linger"))
    );
    assert!(processes.process_id("fixture").await.is_some());
    processes.shutdown().await;
}

#[test]
fn interrupted_update_recovery_keeps_last_known_good_and_revoked_grants() {
    let directory = tempfile::tempdir().unwrap();
    let package_source = directory.path().join("package-source");
    std::fs::create_dir(&package_source).unwrap();
    std::fs::write(package_source.join("plugin.bin"), b"complete package").unwrap();
    let registry_root = directory.path().join("packages");
    let registry =
        gamenode_platform::plugin_registry::PluginPackageRegistry::at(&registry_root).unwrap();
    registry
        .install_verified("fixture.plugin", "1.0.0", &package_source, true)
        .unwrap();
    registry
        .install_verified("fixture.plugin", "2.0.0", &package_source, false)
        .unwrap();

    let profiles = crate::plugin_host::profile::PluginProfiles::default();
    profiles.update(Some("account-one"), true);
    let profile = profiles.current().unwrap();
    let state =
        crate::plugin_host::profile::PluginStateLayout::new(directory.path().join("plugin-data"));
    let identity = crate::plugin_host::grants::PluginIdentity {
        plugin_id: "fixture.plugin".into(),
        publisher_fingerprint: "fixture-publisher".into(),
        profile_id: profile.profile_id().into(),
    };
    let permission = "library.read";
    let scope = crate::plugin_host::grants::GrantScope::Declared {
        value: "fixture provider data".into(),
    };
    let request = crate::plugin_host::grants::PermissionRequest {
        permission_id: permission.into(),
        required: false,
        allowed_scopes: std::collections::BTreeSet::from([scope.clone()]),
    };
    let mut grants = state.open_grants(&profile).unwrap();
    grants
        .replace_grants(
            identity.clone(),
            &[request],
            &[crate::plugin_host::grants::PermissionDecision {
                permission_id: permission.into(),
                scope,
                granted: true,
            }],
        )
        .unwrap();
    grants
        .revoke_permissions(
            &identity,
            &std::collections::BTreeSet::from([permission.to_owned()]),
        )
        .unwrap();
    let _interrupted = registry
        .begin_activation("fixture.plugin", "1.0.0", "2.0.0")
        .unwrap();
    assert_eq!(
        registry.active("fixture.plugin").unwrap().unwrap().version,
        "2.0.0"
    );
    drop(registry);

    let recovered =
        gamenode_platform::plugin_registry::PluginPackageRegistry::at(&registry_root).unwrap();
    assert_eq!(
        recovered.active("fixture.plugin").unwrap().unwrap().version,
        "1.0.0"
    );
    let grants = state.open_grants(&profile).unwrap();
    assert_eq!(
        grants.effective_grants(&identity)[0].state,
        crate::plugin_host::grants::PermissionState::Denied
    );
}

#[tokio::test]
async fn missing_executable_and_handshake_timeout_release_the_launch() {
    for (executable, mode) in [
        (
            PathBuf::from("/definitely-missing-gamenode-plugin"),
            "normal",
        ),
        (std::env::current_exe().unwrap(), "stall"),
    ] {
        let (processes, _server, _directory) =
            manager(executable, mode, Duration::from_millis(200)).await;
        assert!(processes.ensure_started("fixture").await.is_err());
        processes.stop_provider("fixture").await;
        assert!(!processes.inner.api.connected("fixture"));
        assert!(processes.inner.api.begin_launch("fixture").is_ok());
        processes.inner.api.disconnect("fixture").await;
    }
}

#[tokio::test]
async fn crashed_provider_restarts_without_reusing_its_old_session() {
    let (processes, _server, _directory) = manager(
        std::env::current_exe().unwrap(),
        "crash-once",
        Duration::from_secs(10),
    )
    .await;
    assert!(
        processes
            .execute("fixture", ProviderOperation::Initialize)
            .await
            .is_err()
    );
    tokio::time::timeout(Duration::from_secs(3), async {
        while processes.process_id("fixture").await.is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    processes
        .execute("fixture", ProviderOperation::Initialize)
        .await
        .unwrap();
    assert!(processes.process_id("fixture").await.is_some());
    processes.shutdown().await;
}

#[tokio::test]
async fn incompatible_child_and_missing_endpoint_release_the_launch() {
    let (processes, _server, _directory) = manager(
        PathBuf::from("/usr/bin/true"),
        "normal",
        Duration::from_secs(2),
    )
    .await;
    // A normal executable that does not implement the plugin protocol exits without a session.
    assert!(processes.ensure_started("fixture").await.is_err());
    assert!(!processes.inner.api.connected("fixture"));
    processes.shutdown().await;

    let (processes, server, _directory) = manager(
        std::env::current_exe().unwrap(),
        "normal",
        Duration::from_millis(200),
    )
    .await;
    drop(server);
    assert!(processes.ensure_started("fixture").await.is_err());
    assert!(!processes.inner.api.connected("fixture"));
    processes.shutdown().await;
}

#[tokio::test]
async fn invalidated_live_child_is_reaped_before_a_fresh_session_starts() {
    let (processes, _server, _directory) = manager(
        std::env::current_exe().unwrap(),
        "linger",
        Duration::from_secs(10),
    )
    .await;
    processes.ensure_started("fixture").await.unwrap();
    let old_pid = processes.process_id("fixture").await.unwrap();
    processes.inner.api.invalidate_profile().await;
    assert!(!processes.inner.api.connected("fixture"));
    assert!(std::path::Path::new(&format!("/proc/{old_pid}")).exists());
    tokio::time::timeout(Duration::from_secs(8), processes.ensure_started("fixture"))
        .await
        .unwrap()
        .unwrap();
    let new_pid = processes.process_id("fixture").await.unwrap();
    assert_ne!(old_pid, new_pid);
    assert!(!std::path::Path::new(&format!("/proc/{old_pid}")).exists());
    assert!(processes.inner.api.connected("fixture"));
    processes.shutdown().await;
    assert!(!std::path::Path::new(&format!("/proc/{new_pid}")).exists());
}

struct ExtensionFixture {
    processes: ProviderProcesses,
    _server: super::super::PluginApiServer,
    directory: tempfile::TempDir,
    registry: gamenode_platform::plugin_registry::PluginPackageRegistry,
    lifecycle: Arc<super::super::runtime::ThirdPartyPluginLifecycle>,
    state: super::super::profile::PluginStateLayout,
    slot: String,
    selected: Arc<AtomicBool>,
}

impl ExtensionFixture {
    async fn new(mode: &str) -> Self {
        let (processes, server, directory) = manager_with_origin_and_lifecycle(
            std::env::current_exe().unwrap(),
            mode,
            Duration::from_secs(5),
            super::super::provider::RegistrationOrigin::ThirdParty {
                publisher_fingerprint: Self::publisher_fingerprint(),
            },
            true,
        )
        .await;
        let paths = gamenode_platform::paths::DataPaths::at(directory.path());
        let state =
            super::super::profile::PluginStateLayout::new(paths.plugin_data_dir().join("state"));
        let api = &processes.inner.api;
        api.profiles().update(Some("account-one"), true);
        let registry =
            gamenode_platform::plugin_registry::PluginPackageRegistry::open(&paths).unwrap();
        let lifecycle = Arc::new(super::super::runtime::ThirdPartyPluginLifecycle::new(
            api.clone(),
            processes.clone(),
            state.clone(),
            registry.clone(),
            super::super::managed_packages::ManagedPackageLifecycle::new(registry.clone()),
        ));
        let fixture = Self {
            processes,
            _server: server,
            directory,
            registry,
            lifecycle,
            state,
            slot: super::super::provider::extension_registration_id("fixture.plugin"),
            selected: Arc::new(AtomicBool::new(true)),
        };
        use ring::signature::KeyPair;
        std::fs::create_dir_all(fixture.state.trust_dir()).unwrap();
        super::super::publisher_trust::PublisherTrustStore::open(fixture.state.trust_dir())
            .unwrap()
            .confirm_first_key(
                "fixture.plugin",
                Self::key().public_key().as_ref(),
                &Self::publisher_fingerprint(),
            )
            .unwrap();
        fixture.grant_action();
        fixture
    }

    fn api(&self) -> &ProviderApi {
        &self.processes.inner.api
    }

    fn key() -> ring::signature::Ed25519KeyPair {
        ring::signature::Ed25519KeyPair::from_seed_unchecked(&[23; 32]).unwrap()
    }

    fn publisher_fingerprint() -> String {
        use ring::signature::KeyPair;
        super::super::publisher_trust::key_fingerprint(Self::key().public_key().as_ref())
    }

    fn grant_action(&self) {
        use super::super::grants::{
            GrantScope, PermissionDecision, PermissionRequest, PluginIdentity,
        };
        let profile = self.api().profiles().require().unwrap();
        let scope = GrantScope::Declared {
            value: "selected game".into(),
        };
        self.state
            .open_grants(&profile)
            .unwrap()
            .replace_grants(
                PluginIdentity {
                    plugin_id: "fixture.plugin".into(),
                    publisher_fingerprint: Self::publisher_fingerprint(),
                    profile_id: profile.profile_id().into(),
                },
                &[capability::GAME_LAUNCH, capability::GAMES_INSTALL].map(|permission| {
                    PermissionRequest {
                        permission_id: permission.into(),
                        required: permission == capability::GAME_LAUNCH,
                        allowed_scopes: BTreeSet::from([scope.clone()]),
                    }
                }),
                &[capability::GAME_LAUNCH, capability::GAMES_INSTALL].map(|permission| {
                    PermissionDecision {
                        permission_id: permission.into(),
                        scope: scope.clone(),
                        granted: true,
                    }
                }),
            )
            .unwrap();
    }

    fn signed_package(&self, version: &str, mode: &str) -> tempfile::TempDir {
        use base64::Engine;
        use ring::signature::KeyPair;
        use sha2::Digest;
        use std::os::unix::fs::PermissionsExt;
        let package = tempfile::tempdir().unwrap();
        let key = Self::key();
        let publisher_key: [u8; 32] = key.public_key().as_ref().try_into().unwrap();
        let manifest = serde_json::to_vec(&serde_json::json!({
            "schema": 1,
            "id": "fixture.plugin",
            "kind": "extension",
            "name": "Fixture",
            "version": version,
            "api": {"min": "1.6.0", "max": "1.6.0"},
            "targets": [{"os": "linux", "arch": "x86_64", "abi": "gnu", "entrypoint": "plugin"}],
            "permissions": [
                {"id": capability::GAME_LAUNCH, "required": true, "scope": "selected game", "reason": "Start the selected game"},
                {"id": capability::GAMES_INSTALL, "required": false, "scope": "selected game", "reason": "Install the selected game"}
            ],
            "publisher": {
                "key_id": "fixture-key",
                "public_key": base64::engine::general_purpose::STANDARD.encode(publisher_key)
            },
            "license": {"expression": "MIT", "files": ["LICENSE"]}
        })).unwrap();
        // Sign the fixture launcher itself so candidates reached through the real updater
        // use the same arguments and deterministic markers as direct supervisor tests.
        let shell_quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
        let mut executable =
            String::from("#!/bin/sh\nexport GAMENODE_TEST_PLUGIN_PROCESS=fixture\n");
        for (key, file) in [
            ("GAMENODE_TEST_PLUGIN_OPEN_MARKER", "opened-without-ready"),
            ("GAMENODE_TEST_PLUGIN_CRASH_MARKER", "crash-once"),
            ("GAMENODE_TEST_PLUGIN_TREE_PID", "tree-child-pid"),
            ("GAMENODE_TEST_PLUGIN_TREE_OUTPUT", "tree-child-output"),
        ] {
            let path = self.directory.path().join(file);
            executable.push_str(&format!(
                "export {key}={}\n",
                shell_quote(path.to_str().unwrap())
            ));
        }
        executable.push_str(&format!(
            "export GAMENODE_TEST_PLUGIN_MODE={} GAMENODE_TEST_PLUGIN_VERSION={}\nexec {} --exact plugin_host::process::tests::fixture_process --nocapture\n",
            shell_quote(mode), shell_quote(version),
            shell_quote(std::env::current_exe().unwrap().to_str().unwrap()),
        ));
        let files = [
            ("plugin.json", manifest.as_slice()),
            ("plugin", executable.as_bytes()),
            ("LICENSE", b"fixture".as_slice()),
        ]
        .into_iter()
        .map(|(name, data)| {
            std::fs::write(package.path().join(name), data).unwrap();
            (name, format!("{:x}", sha2::Sha256::digest(data)))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
        std::fs::set_permissions(
            package.path().join("plugin"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let integrity =
            serde_json::to_vec(&serde_json::json!({"schema": 1, "files": files})).unwrap();
        let mut signed = gamenode_platform::plugin_packages::SIGNATURE_DOMAIN_PREFIX.to_vec();
        signed.extend_from_slice(&integrity);
        std::fs::write(package.path().join("integrity.json"), integrity).unwrap();
        std::fs::write(
            package.path().join("signature.ed25519"),
            key.sign(&signed).as_ref(),
        )
        .unwrap();

        package
    }

    async fn package(
        &self,
        version: &str,
        mode: &str,
        active: bool,
    ) -> (
        gamenode_platform::plugin_registry::InstalledPluginVersion,
        ProcessConfig,
    ) {
        use ring::signature::KeyPair;
        let publisher_key = Self::key().public_key().as_ref().try_into().unwrap();
        let package = self.signed_package(version, mode);
        let installed = self
            .registry
            .install_verified("fixture.plugin", version, package.path(), active)
            .unwrap();
        let packages =
            super::super::managed_packages::ManagedPackageLifecycle::new(self.registry.clone());
        let report = packages
            .inspect_candidate(
                &installed.package_path,
                publisher_key,
                gamenode_platform::plugin_environment::EmulationPolicy::RequireNative,
            )
            .await
            .unwrap();
        let mut config =
            super::super::runtime::managed_process_config(&installed, &report, publisher_key)
                .unwrap();
        if mode == "extension-stall" {
            config.startup_timeout = Duration::from_millis(700);
        }
        (installed, config)
    }

    fn launch(&self) -> ExtensionLaunch {
        let profile = self.api().profiles().require().unwrap();
        let selected = self.selected.clone();
        ExtensionLaunch {
            target: ExtensionTarget {
                context: TargetGameContext {
                    game_id: "42".into(),
                    installation_id: "steam:42".into(),
                    revision: "1".into(),
                    profile_id: profile.profile_id().into(),
                },
                profile_epoch: profile.epoch(),
                still_selected: Arc::new(move || selected.load(Ordering::Acquire)),
            },
            required_permission: capability::GAME_LAUNCH,
        }
    }

    fn intent(&self) -> ExtensionIntent {
        ExtensionIntent {
            id: "one-use".into(),
            store: Arc::new(super::super::intents::Intents::default()),
        }
    }

    async fn start(&self) -> ServiceContext {
        let marker = self.directory.path().join("tree-child-pid");
        let _ = std::fs::remove_file(&marker);
        self.processes
            .ensure_extension_started(&self.slot, self.launch(), self.intent())
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !marker.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("child did not verify its negotiated context");
        self.api()
            .resource_session_contexts()
            .into_iter()
            .next()
            .unwrap()
    }

    async fn assert_stopped(&self, context: &ServiceContext) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while self.processes.process_id(&self.slot).await.is_some() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("extension did not stop");
        assert!(
            self.api()
                .authorize_resource_session(Some(context))
                .is_err()
        );
        let output = self.directory.path().join("tree-child-output");
        let size = std::fs::metadata(&output).map_or(0, |metadata| metadata.len());
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(
            std::fs::metadata(&output).map_or(0, |metadata| metadata.len()),
            size
        );
    }
}

#[tokio::test]
async fn extension_process_fences_disable_profile_revocation_target_crash_and_removal() {
    let f = ExtensionFixture::new("extension-crash").await;
    let (installed, config) = f.package("1.0.0", "extension-crash", true).await;
    f.processes.replace_config(&f.slot, config).await.unwrap();
    assert!(f.lifecycle.extension_runtime_available("fixture.plugin"));
    assert!(f.processes.ensure_started(&f.slot).await.is_err());
    assert!(
        f.processes
            .ensure_extension_started(&f.slot, f.launch(), f.intent())
            .await
            .is_err()
    );

    f.lifecycle.enable("fixture.plugin".into()).await.unwrap();
    let unavailable = ProviderProcesses::new_for_lifecycle(
        f.api().clone(),
        PathBuf::new(),
        vec![f.processes.config(&f.slot).unwrap()],
        f.state.clone(),
    )
    .unwrap();
    assert!(!unavailable.extension_runtime_available(&f.slot));
    assert_eq!(
        unavailable
            .ensure_extension_started(&f.slot, f.launch(), f.intent())
            .await
            .unwrap_err()
            .code(),
        tonic::Code::Unavailable,
    );
    let mut foreign = f.launch();
    foreign.target.context.profile_id = "other-profile".into();
    assert!(
        f.processes
            .ensure_extension_started(&f.slot, foreign, f.intent())
            .await
            .is_err()
    );

    let context = f.start().await;
    f.lifecycle.disable("fixture.plugin".into()).await.unwrap();
    f.assert_stopped(&context).await;
    f.lifecycle.enable("fixture.plugin".into()).await.unwrap();

    let context = f.start().await;
    let old_launch = f.launch();
    f.api().profiles().update(Some("account-two"), true);
    f.assert_stopped(&context).await;
    assert!(!f.lifecycle.enabled("fixture.plugin".into()).await.unwrap());
    assert!(
        f.processes
            .ensure_extension_started(&f.slot, old_launch, f.intent())
            .await
            .is_err()
    );
    f.api().profiles().update(Some("account-one"), true);
    assert!(f.lifecycle.enabled("fixture.plugin".into()).await.unwrap());
    assert!(f.processes.process_id(&f.slot).await.is_none());

    let context = f.start().await;
    f.api().profiles().update(None, false);
    f.assert_stopped(&context).await;
    f.api().profiles().update(Some("account-one"), true);

    let context = f.start().await;
    f.selected.store(false, Ordering::Release);
    assert!(f.api().authorize_resource_session(Some(&context)).is_err());
    f.assert_stopped(&context).await;
    assert!(
        f.processes
            .ensure_extension_started(&f.slot, f.launch(), f.intent())
            .await
            .is_err()
    );
    f.selected.store(true, Ordering::Release);

    let context = f.start().await;
    f.api()
        .revoke_session_permission(&f.slot, capability::GAME_LAUNCH)
        .await;
    f.assert_stopped(&context).await;

    let context = f.start().await;
    let profile = f.api().profiles().require().unwrap();
    let snapshot = f
        .lifecycle
        .permission_snapshot("fixture.plugin".into())
        .await
        .unwrap();
    f.lifecycle
        .replace_permissions(
            &tokio::sync::Mutex::new(()),
            "fixture.plugin".into(),
            profile.profile_id().into(),
            snapshot.publisher_fingerprint.clone(),
            snapshot.grant_revision,
            Vec::new(),
        )
        .await
        .unwrap();
    f.assert_stopped(&context).await;
    assert!(
        f.processes
            .ensure_extension_started(&f.slot, f.launch(), f.intent())
            .await
            .is_err()
    );

    f.grant_action();
    let context = f.start().await;
    let marker = f.directory.path().join("crash-once");
    std::fs::write(&marker, b"crash").unwrap();
    f.assert_stopped(&context).await;
    std::fs::remove_file(marker).unwrap();

    let context = f.start().await;
    f.lifecycle.remove("fixture.plugin".into()).await.unwrap();
    f.assert_stopped(&context).await;
    assert!(!f.lifecycle.extension_runtime_available("fixture.plugin"));
    assert!(!installed.package_path.exists());
}

fn extension_updates(f: &ExtensionFixture) -> super::super::updates::PluginUpdateService {
    let paths = gamenode_platform::paths::DataPaths::at(f.directory.path());
    let updates = super::super::updates::PluginUpdateService::open(&paths).unwrap();
    updates.set_lifecycle(f.lifecycle.clone());
    updates
}

#[tokio::test]
async fn extension_process_idle_update_and_rollback_require_selection_and_run_no_actions() {
    let f = ExtensionFixture::new("extension-hold").await;
    let (original, original_config) = f.package("1.0.0", "extension-probe", true).await;
    f.processes
        .replace_config(&f.slot, original_config)
        .await
        .unwrap();
    let staged = f.signed_package("2.0.0", "extension-probe");
    let paths = gamenode_platform::paths::DataPaths::at(f.directory.path());
    let disconnected = super::super::updates::PluginUpdateService::open(&paths).unwrap();
    assert_eq!(
        disconnected.managed_inventory().await.unwrap()[0].runtime_available,
        Some(false)
    );
    let updates = extension_updates(&f);
    let marker = f.directory.path().join("opened-without-ready");

    // Declining a staged update leaves both the process slot and registry untouched.
    updates
        .stage_extension_update_for_test(&original, staged.path())
        .await;
    assert!(
        updates
            .approve("fixture.plugin", "2.0.0", None)
            .await
            .is_err()
    );
    updates.decline("fixture.plugin").await;
    assert!(
        updates
            .approve("fixture.plugin", "2.0.0", Some(f.launch().target))
            .await
            .is_err()
    );
    assert!(!marker.exists());
    assert_eq!(
        f.registry.versions("fixture.plugin").unwrap(),
        vec![original.clone()]
    );
    assert_eq!(
        f.registry.active("fixture.plugin").unwrap(),
        Some(original.clone())
    );

    // Readiness is also available for a disabled extension and preserves that preference.
    updates
        .stage_extension_update_for_test(&original, staged.path())
        .await;
    assert_eq!(
        updates
            .approve("fixture.plugin", "2.0.0", Some(f.launch().target))
            .await
            .unwrap(),
        "2.0.0"
    );
    assert!(marker.exists());
    assert!(f.processes.process_id(&f.slot).await.is_none());
    let candidate = f.registry.active("fixture.plugin").unwrap().unwrap();
    assert_eq!(candidate.version, "2.0.0");
    let inventory = updates.managed_inventory().await.unwrap();
    assert_eq!(inventory[0].runtime_available, Some(true));
    assert_eq!(inventory[0].enabled, Some(false));
    assert!(inventory[0].provider_id.is_empty());

    f.lifecycle.enable("fixture.plugin".into()).await.unwrap();
    std::fs::remove_file(&marker).unwrap();
    let review = updates
        .review_rollback("fixture.plugin", "1.0.0")
        .await
        .unwrap();
    assert!(review.extension_targets.is_some());
    assert!(
        updates
            .approve_rollback("fixture.plugin", "1.0.0", true, None)
            .await
            .is_err()
    );
    updates.decline("fixture.plugin").await;
    assert!(
        updates
            .approve_rollback("fixture.plugin", "1.0.0", true, Some(f.launch().target))
            .await
            .is_err()
    );
    assert!(!marker.exists());
    assert_eq!(
        f.registry.active("fixture.plugin").unwrap(),
        Some(candidate)
    );

    updates
        .review_rollback("fixture.plugin", "1.0.0")
        .await
        .unwrap();
    assert_eq!(
        updates
            .approve_rollback("fixture.plugin", "1.0.0", true, Some(f.launch().target))
            .await
            .unwrap(),
        "1.0.0"
    );
    assert!(marker.exists());
    assert!(f.processes.process_id(&f.slot).await.is_none());
    assert_eq!(f.registry.active("fixture.plugin").unwrap(), Some(original));
    assert_eq!(
        updates.managed_inventory().await.unwrap()[0].enabled,
        Some(true)
    );
}

#[tokio::test]
async fn extension_readiness_denies_actions_even_after_ready() {
    let f = ExtensionFixture::new("extension-hold").await;
    let (_, config) = f.package("1.0.0", "extension-probe", true).await;
    f.processes
        .ensure_extension_ready_for_update(&f.slot, f.launch().target, config)
        .await
        .unwrap();
    let context = f
        .api()
        .resource_session_contexts()
        .into_iter()
        .next()
        .unwrap();
    let endpoint = f.directory.path().join("plugins.sock");
    let channel = gamenode_plugin_sdk::connect(&endpoint).await.unwrap();
    assert_extension_actions_denied(channel, &context).await;
    f.processes.stop_provider(&f.slot).await;
    assert!(!f.api().connected(&f.slot));
}

#[tokio::test]
async fn extension_process_update_conditions_keep_previous_version_without_ready() {
    for invalidation in ["none", "target", "profile", "grants"] {
        let f = ExtensionFixture::new("extension-hold").await;
        let (original, original_config) = f.package("1.0.0", "extension-hold", true).await;
        f.processes
            .replace_config(&f.slot, original_config.clone())
            .await
            .unwrap();
        let mode = if invalidation == "grants" {
            "extension-gated"
        } else {
            "extension-stall"
        };
        let (candidate, mut config) = f.package("2.0.0", mode, false).await;
        if invalidation != "none" {
            config.startup_timeout = Duration::from_secs(5);
        }
        let marker = f.directory.path().join("opened-without-ready");
        let lifecycle = f.lifecycle.clone();
        let target = f.launch().target;
        let activation = tokio::spawn(async move {
            lifecycle
                .activate_update(
                    "fixture.plugin".into(),
                    "1.0.0".into(),
                    candidate,
                    config,
                    Some(target),
                )
                .await
        });
        tokio::time::timeout(Duration::from_secs(6), async {
            while !marker.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("candidate did not complete OpenSession");
        assert_eq!(
            f.registry.active("fixture.plugin").unwrap(),
            Some(original.clone())
        );
        assert!(!f.api().profiles().transition_is_locked());
        tokio::time::timeout(
            Duration::from_secs(2),
            tokio::task::spawn_blocking(|| {
                let _publisher = super::super::managed_packages::lock_publisher_identity();
            }),
        )
        .await
        .unwrap()
        .unwrap();
        match invalidation {
            "target" => f.selected.store(false, Ordering::Release),
            "profile" => {
                f.api().profiles().update(Some("account-two"), true);
                f.api().profiles().update(Some("account-one"), true);
            }
            "grants" => {
                use super::super::grants::{PermissionState, PluginIdentity};
                let profile = f.api().profiles().require().unwrap();
                let identity = PluginIdentity {
                    plugin_id: "fixture.plugin".into(),
                    publisher_fingerprint: ExtensionFixture::publisher_fingerprint(),
                    profile_id: profile.profile_id().to_owned(),
                };
                f.state
                    .open_grants(&profile)
                    .unwrap()
                    .revoke_permissions(
                        &identity,
                        &BTreeSet::from([capability::GAMES_INSTALL.into()]),
                    )
                    .unwrap();
                std::fs::write(f.directory.path().join("allow-ready"), b"ready").unwrap();
                let error = activation.await.unwrap().unwrap_err();
                assert_eq!(error.message(), "extension grants changed during readiness");
                assert!(
                    f.state
                        .open_grants(&profile)
                        .unwrap()
                        .effective_grants(&identity)
                        .iter()
                        .any(|grant| grant.permission_id == capability::GAMES_INSTALL
                            && grant.state == PermissionState::Denied)
                );
                assert!(f.processes.process_id(&f.slot).await.is_none());
                assert_eq!(f.registry.active("fixture.plugin").unwrap(), Some(original));
                assert_eq!(
                    f.processes.config(&f.slot).unwrap().executable,
                    original_config.executable
                );
                continue;
            }
            _ => {}
        }
        assert!(activation.await.unwrap().is_err());
        assert!(f.processes.process_id(&f.slot).await.is_none());
        assert_eq!(f.registry.active("fixture.plugin").unwrap(), Some(original));
        assert_eq!(
            f.processes.config(&f.slot).unwrap().executable,
            original_config.executable
        );
    }
}
