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
    async fn handle(&self, _: ProviderCommand, host: HostClient) -> Result<OperationResult, Error> {
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
    let session = Session::open(
        gamenode_plugin_sdk::Launch::from_env().unwrap(),
        "fixture.plugin",
        "fixture",
        "1.0",
        &[capability::CONTEXT],
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
    root.providers
        .register(Registration {
            provider_id: "fixture".into(),
            plugin_id: "fixture.plugin".into(),
            kind: crate::plugin_host::provider::RegistrationKind::Provider,
            origin,
            capabilities: vec![capability::CONTEXT.into()],
            shared_catalog: false,
            services: Arc::new(Services),
        })
        .unwrap();
    let server = super::super::PluginApiServer::start(
        &tokio::runtime::Handle::current(),
        &endpoint,
        root.clone(),
    )
    .unwrap();
    let config = vec![ProcessConfig {
        provider_id: "fixture".into(),
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
        manifest_capabilities: vec![capability::CONTEXT.into(), "library.read".into()],
        required_capabilities: vec![],
        blocked_capabilities: vec![],
        integrity: None,
    }];
    let manager = if managed_lifecycle {
        ProviderProcesses::new_for_lifecycle(
            root.providers,
            endpoint,
            config,
            crate::plugin_host::profile::PluginStateLayout::new(
                directory.path().join("plugin-data"),
            ),
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
    Arc::get_mut(&mut processes.inner)
        .unwrap()
        .slots
        .get_mut("fixture")
        .unwrap()
        .config
        .get_mut()
        .unwrap()
        .integrity = Some(PackageIntegrity {
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
    Arc::get_mut(&mut processes.inner)
        .unwrap()
        .slots
        .get_mut("fixture")
        .unwrap()
        .config
        .get_mut()
        .unwrap()
        .integrity = Some(PackageIntegrity {
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
