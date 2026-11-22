//! SDK reference-host fixture for the package provider contract.
//!
//! The fixture runs the real `gamenode-plugin-sdk` client and dispatcher against an in-process
//! reference host over a Unix socket. Discovery is served by a static source, so the test proves
//! the provider returns normalized `OperationResult.discovery` records and declines every other
//! operation without calling the retired `SubmitDiscovery` RPC.

use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use gamenode_plugin_protocol::{API_MAJOR, LEGACY_API_MINOR};
use gamenode_plugin_sdk::{Launch, Session, capability, proto::*};
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::{ReceiverStream, UnixListenerStream};
use tonic::{Request, Response, Status};

use super::plugin::{DiscoverySource, PLUGIN_ID, PROVIDER_ID, PackageManagerPlugin};
use super::scan::{MAX_RECORDS, PackageCandidate, ScanFailure};

const SESSION_ID: &str = "packagekit-session";
const CREDENTIAL: [u8; 32] = [9; 32];
const ACCOUNT_EPOCH: u64 = 3;
const PROFILE_EPOCH: u64 = 5;
const FIXTURE_TIMEOUT: Duration = Duration::from_secs(5);

struct StaticSource(Result<Vec<PackageCandidate>, ScanFailure>);

#[async_trait]
impl DiscoverySource for StaticSource {
    async fn discover(&self) -> Result<Vec<PackageCandidate>, ScanFailure> {
        self.0.clone()
    }
}

#[derive(Clone)]
struct ReferenceHost {
    completed: mpsc::Sender<(String, OperationResult)>,
    submit_discovery_calls: Arc<AtomicUsize>,
    /// The provider context the host reports, which the plugin must validate before scanning.
    context_provider_id: &'static str,
    context_platform: &'static str,
}

fn authorize(context: Option<&ServiceContext>) -> Result<&ServiceContext, Status> {
    let context = context.ok_or_else(|| Status::unauthenticated("missing context"))?;
    if context.session_id != SESSION_ID
        || context.session_credential != CREDENTIAL
        || context.provider_id != PROVIDER_ID
    {
        return Err(Status::permission_denied("wrong session"));
    }
    if context.account_epoch != ACCOUNT_EPOCH || context.profile_epoch != PROFILE_EPOCH {
        return Err(Status::failed_precondition("stale context"));
    }
    Ok(context)
}

/// The SDK sends the ready event without account/profile epochs; only the session identity is
/// available at that point.
fn ready_session(context: Option<&ServiceContext>) -> Result<(), Status> {
    let context = context.ok_or_else(|| Status::unauthenticated("missing context"))?;
    if context.session_id != SESSION_ID
        || context.session_credential != CREDENTIAL
        || context.provider_id != PROVIDER_ID
    {
        return Err(Status::permission_denied("wrong session"));
    }
    Ok(())
}

#[tonic::async_trait]
impl plugin_host_server::PluginHost for ReferenceHost {
    type WatchPermissionChangesStream =
        tokio_stream::Iter<std::vec::IntoIter<Result<PermissionChange, Status>>>;

    async fn open_session(
        &self,
        request: Request<OpenSessionRequest>,
    ) -> Result<Response<OpenSessionReply>, Status> {
        let hello = request.into_inner();
        // `Session::open` keeps the legacy unspecified kind; both forms are a provider session.
        let provider_kind = hello.session_kind == SessionKind::Unspecified as i32
            || hello.session_kind == SessionKind::Provider as i32;
        if hello.plugin_id != PLUGIN_ID
            || hello.session_credential != CREDENTIAL
            || !provider_kind
            || hello.target_game.is_some()
            || hello.requested_capabilities != [capability::CONTEXT, capability::DISCOVERY]
        {
            return Err(Status::permission_denied("invalid launch"));
        }
        Ok(Response::new(OpenSessionReply {
            resource_context: None,
            session_id: SESSION_ID.into(),
            granted_capabilities: hello.requested_capabilities,
            api_major: API_MAJOR,
            api_minor: LEGACY_API_MINOR,
            features: Vec::new(),
            support_window: None,
        }))
    }

    /// The current host returns this RPC as unimplemented; a plugin that used it would fail here.
    async fn submit_discovery(
        &self,
        _: Request<SubmitDiscoveryRequest>,
    ) -> Result<Response<SubmitDiscoveryReply>, Status> {
        self.submit_discovery_calls.fetch_add(1, Ordering::SeqCst);
        Err(Status::permission_denied("not granted"))
    }

    async fn describe_api(
        &self,
        request: Request<DescribeApiRequest>,
    ) -> Result<Response<DescribeApiReply>, Status> {
        if request.get_ref().session_id != SESSION_ID
            || request.get_ref().session_credential != CREDENTIAL
        {
            return Err(Status::unauthenticated("invalid fixture"));
        }
        Ok(Response::new(DescribeApiReply {
            api_major: API_MAJOR,
            api_minor: LEGACY_API_MINOR,
            support_window: None,
            features: Vec::new(),
            methods: Vec::new(),
        }))
    }

    async fn get_permissions(
        &self,
        request: Request<GetPermissionsRequest>,
    ) -> Result<Response<GetPermissionsReply>, Status> {
        if request.get_ref().session_id != SESSION_ID
            || request.get_ref().session_credential != CREDENTIAL
        {
            return Err(Status::unauthenticated("invalid fixture"));
        }
        Ok(Response::new(GetPermissionsReply {
            revision: 1,
            grants: [capability::CONTEXT, capability::DISCOVERY]
                .into_iter()
                .map(|permission_id| PermissionGrant {
                    permission_id: permission_id.into(),
                    state: "granted".into(),
                    scope: "provider".into(),
                    reason: String::new(),
                    revision: 1,
                })
                .collect(),
        }))
    }

    async fn watch_permission_changes(
        &self,
        _: Request<WatchPermissionChangesRequest>,
    ) -> Result<Response<Self::WatchPermissionChangesStream>, Status> {
        Ok(Response::new(tokio_stream::iter(Vec::new())))
    }
}

#[tonic::async_trait]
impl provider_host_server::ProviderHost for ReferenceHost {
    type ExchangeStream = ReceiverStream<Result<ProviderCommand, Status>>;

    async fn exchange(
        &self,
        request: Request<tonic::Streaming<ProviderEvent>>,
    ) -> Result<Response<Self::ExchangeStream>, Status> {
        let mut input = request.into_inner();
        let ready = input
            .message()
            .await?
            .ok_or_else(|| Status::unauthenticated("no ready event"))?;
        let context = ready
            .context
            .clone()
            .ok_or_else(|| Status::unauthenticated("no session"))?;
        ready_session(Some(&context))?;
        if !matches!(ready.event, Some(provider_event::Event::Ready(true))) {
            return Err(Status::permission_denied("invalid ready event"));
        }
        let (tx, rx) = mpsc::channel(8);
        let completed = self.completed.clone();
        tokio::spawn(async move {
            let command = |operation, id: &str| ProviderCommand {
                context: Some(ServiceContext {
                    operation_id: id.into(),
                    account_epoch: ACCOUNT_EPOCH,
                    profile_epoch: PROFILE_EPOCH,
                    ..context.clone()
                }),
                operation: operation as i32,
                target_operation_id: String::new(),
                settings: None,
                runtime_session_result: None,
                runtime_session_context_result: None,
                achievements: None,
            };
            let operations = [
                (ProviderOperation::Initialize, "initialize"),
                (ProviderOperation::Discover, "discover"),
                (ProviderOperation::SignIn, "sign-in"),
            ];
            for (operation, id) in operations {
                tx.send(Ok(command(operation, id))).await.unwrap();
            }
            let mut results = 0;
            while let Some(event) = input.message().await.unwrap() {
                if let Some(provider_event::Event::Result(result)) = event.event {
                    let context = authorize(event.context.as_ref()).unwrap();
                    completed
                        .send((context.operation_id.clone(), result))
                        .await
                        .unwrap();
                    results += 1;
                    if results == operations.len() {
                        tx.send(Ok(command(ProviderOperation::Shutdown, "shutdown")))
                            .await
                            .unwrap();
                        break;
                    }
                }
            }
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn get_context(
        &self,
        request: Request<ServiceContext>,
    ) -> Result<Response<ProviderContext>, Status> {
        authorize(Some(request.get_ref()))?;
        Ok(Response::new(ProviderContext {
            provider_id: self.context_provider_id.into(),
            platform: self.context_platform.into(),
            max_records: MAX_RECORDS as u32,
            max_message_bytes: 8 * 1024 * 1024,
            catalog_chunk_records: 256,
            secret_storage: SecretStorage::PrivateFile as i32,
            ..Default::default()
        }))
    }

    async fn account(&self, _: Request<AccountRequest>) -> Result<Response<AccountReply>, Status> {
        Err(Status::permission_denied("not granted"))
    }

    async fn secret(&self, _: Request<SecretRequest>) -> Result<Response<SecretReply>, Status> {
        Err(Status::permission_denied("not granted"))
    }

    async fn login(&self, _: Request<LoginRequest>) -> Result<Response<LoginReply>, Status> {
        Err(Status::permission_denied("not granted"))
    }

    async fn declare_actions(
        &self,
        _: Request<ActionRequest>,
    ) -> Result<Response<ActionReply>, Status> {
        Err(Status::permission_denied("not granted"))
    }
}

/// Runs the provider against the reference host and returns its operation results.
async fn run_fixture(
    source: Arc<dyn DiscoverySource>,
    context_provider_id: &'static str,
    context_platform: &'static str,
) -> (BTreeMap<String, OperationResult>, Arc<AtomicUsize>) {
    let temp = tempfile::tempdir().unwrap();
    let endpoint = temp.path().join("reference.sock");
    let listener = tokio::net::UnixListener::bind(&endpoint).unwrap();
    let (tx, mut completed) = mpsc::channel(16);
    let submit_discovery_calls = Arc::new(AtomicUsize::new(0));
    let reference = ReferenceHost {
        completed: tx,
        submit_discovery_calls: submit_discovery_calls.clone(),
        context_provider_id,
        context_platform,
    };
    let (shutdown, stopped) = oneshot::channel();
    let server = tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(plugin_host_server::PluginHostServer::new(reference.clone()))
            .add_service(provider_host_server::ProviderHostServer::new(reference))
            .serve_with_incoming_shutdown(UnixListenerStream::new(listener), async {
                let _ = stopped.await;
            }),
    );
    let session = Session::open(
        Launch {
            endpoint,
            credential: CREDENTIAL.to_vec(),
            operation_id: "launch".into(),
        },
        PLUGIN_ID,
        PROVIDER_ID,
        "0.1.0",
        &[capability::CONTEXT, capability::DISCOVERY],
    )
    .await
    .unwrap();
    let plugin = PackageManagerPlugin::with_source(source, FIXTURE_TIMEOUT);
    tokio::time::timeout(
        FIXTURE_TIMEOUT,
        gamenode_plugin_sdk::run(session, Arc::new(plugin)),
    )
    .await
    .unwrap()
    .unwrap();

    let mut results = BTreeMap::new();
    for _ in 0..3 {
        let (id, result) = completed.recv().await.unwrap();
        results.insert(id, result);
    }
    let _ = shutdown.send(());
    server.await.unwrap().unwrap();
    (results, submit_discovery_calls)
}

fn candidate(id: &str, name: &str, install_dir: &str) -> PackageCandidate {
    PackageCandidate {
        id: id.into(),
        name: Some(name.into()),
        install_dir: install_dir.into(),
        fields: BTreeMap::from([
            ("package_id".into(), id.into()),
            ("package_manager".into(), "pacman".into()),
            (
                "desktop_file".into(),
                format!("/usr/share/applications/{id}.desktop"),
            ),
            ("executable".into(), format!("{install_dir}/{id}")),
        ]),
    }
}

#[tokio::test]
async fn provider_returns_normalized_discovery_records_and_declines_other_operations() {
    let records = vec![
        candidate("supertux", "SuperTux", "/usr/bin"),
        candidate("0ad", "0 A.D.", "/usr/bin"),
    ];
    let (results, submit_discovery) =
        run_fixture(Arc::new(StaticSource(Ok(records))), PROVIDER_ID, "linux").await;

    assert_eq!(
        results["initialize"].disposition,
        OperationDisposition::Completed as i32
    );
    let discovery = results["discover"]
        .discovery
        .as_ref()
        .expect("Discover returns records through OperationResult.discovery");
    assert_eq!(discovery.provider_id, PROVIDER_ID);
    assert_eq!(discovery.records.len(), 2);
    assert_eq!(discovery.records[0].id, "supertux");
    assert_eq!(discovery.records[0].name, "SuperTux");
    assert_eq!(discovery.records[0].install_dir, "/usr/bin");
    assert_eq!(discovery.records[0].fields["package_manager"], "pacman");
    assert!(discovery.records[0].target.is_none());
    assert!(discovery.install_root.is_none());

    // Only discovery is granted: the account, catalog, login and action operations are declined.
    let declined = &results["sign-in"];
    assert_eq!(declined.disposition, OperationDisposition::Failed as i32);
    assert_eq!(declined.failure.as_ref().unwrap().code, "denied");
    assert!(declined.discovery.is_none());
    assert_eq!(submit_discovery.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn provider_reports_a_successful_empty_scan_as_completed() {
    let (results, submit_discovery) =
        run_fixture(Arc::new(StaticSource(Ok(Vec::new()))), PROVIDER_ID, "linux").await;

    let completion = &results["discover"];
    assert_eq!(
        completion.disposition,
        OperationDisposition::Completed as i32
    );
    let discovery = completion
        .discovery
        .as_ref()
        .expect("an empty scan still returns a discovery result");
    assert_eq!(discovery.provider_id, PROVIDER_ID);
    assert!(discovery.records.is_empty());
    assert!(completion.failure.is_none());
    assert_eq!(submit_discovery.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn provider_reports_a_failed_scan_without_returning_stale_or_partial_records() {
    let (results, submit_discovery) = run_fixture(
        Arc::new(StaticSource(Err(ScanFailure::PackageKitUnavailable))),
        PROVIDER_ID,
        "linux",
    )
    .await;

    let discovery = &results["discover"];
    assert_eq!(discovery.disposition, OperationDisposition::Failed as i32);
    assert!(discovery.discovery.is_none());
    assert_eq!(discovery.failure.as_ref().unwrap().code, "unavailable");
    // The host keeps the previous successful records itself because this result carries none.
    assert_eq!(submit_discovery.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn provider_validates_the_trusted_context_before_scanning() {
    for (provider_id, platform) in [(PROVIDER_ID, "windows"), ("steam", "linux")] {
        let (results, submit_discovery) = run_fixture(
            Arc::new(StaticSource(Ok(vec![candidate(
                "supertux", "SuperTux", "/usr/bin",
            )]))),
            provider_id,
            platform,
        )
        .await;

        let discovery = &results["discover"];
        assert_eq!(
            discovery.disposition,
            OperationDisposition::Failed as i32,
            "{provider_id}/{platform} must be denied"
        );
        assert!(discovery.discovery.is_none());
        assert_eq!(discovery.failure.as_ref().unwrap().code, "denied");
        assert_eq!(submit_discovery.load(Ordering::SeqCst), 0);
    }
}
