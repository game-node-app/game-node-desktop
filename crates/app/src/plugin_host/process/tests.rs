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
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let endpoint = directory.path().join("plugins.sock");
    let root = super::super::PluginApi::default();
    root.providers
        .register(Registration {
            provider_id: "fixture".into(),
            plugin_id: "fixture.plugin".into(),
            capabilities: vec![capability::CONTEXT.into()],
            services: Arc::new(Services),
        })
        .unwrap();
    let server = super::super::PluginApiServer::start(
        &tokio::runtime::Handle::current(),
        &endpoint,
        root.clone(),
    )
    .unwrap();
    let manager = ProviderProcesses::new(
        root.providers,
        endpoint,
        vec![ProcessConfig {
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
            ],
            startup_timeout: timeout,
        }],
    )
    .unwrap();
    (manager, server, directory)
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
