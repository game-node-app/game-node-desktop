//! The SDK and a fixture plugin exercise the public wire contract without Core,
//! the real app, its database or its platform adapters.
#![cfg(unix)]

use std::sync::Arc;

use gamenode_plugin_sdk::{
    Error, HostClient, Launch, ProviderPlugin, Session, capability, models, prost_types, proto::*,
};
use tokio::sync::{Semaphore, mpsc, oneshot};
use tokio_stream::wrappers::{ReceiverStream, UnixListenerStream};
use tonic::{Request, Response, Status};

#[derive(Clone)]
struct ReferenceHost {
    completed: mpsc::Sender<(String, OperationResult)>,
}

fn authorize(context: Option<&ServiceContext>) -> Result<&ServiceContext, Status> {
    let context = context.ok_or_else(|| Status::unauthenticated("missing context"))?;
    if context.session_id != "session"
        || context.session_credential != [9; 32]
        || !(context.provider_id == "fixture"
            || (context.provider_id.is_empty()
                && context.session_kind == SessionKind::Extension as i32))
    {
        return Err(Status::permission_denied("wrong session"));
    }
    if context.account_epoch != 7 || context.profile_epoch != 11 {
        return Err(Status::failed_precondition("stale context"));
    }
    Ok(context)
}

#[test]
fn reference_host_rejects_stale_epochs_and_unknown_operations() {
    let current = ServiceContext {
        session_id: "session".into(),
        session_credential: vec![9; 32],
        provider_id: "fixture".into(),
        operation_id: "current".into(),
        account_epoch: 7,
        profile_epoch: 11,
        session_kind: SessionKind::Provider as i32,
        target_game: None,
    };
    assert!(authorize(Some(&current)).is_ok());
    let stale = ServiceContext {
        account_epoch: 6,
        ..current
    };
    assert_eq!(
        authorize(Some(&stale)).unwrap_err().code(),
        tonic::Code::FailedPrecondition
    );
    assert!(ProviderOperation::try_from(999).is_err());
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
        let valid_provider = hello.session_kind == SessionKind::Provider as i32
            && hello.requested_capabilities == [capability::CONTEXT, capability::CONFIGURE];
        let valid_extension = hello.session_kind == SessionKind::Extension as i32
            && hello.target_game.is_some()
            && hello.requested_capabilities == [capability::GAME_LAUNCH, capability::CONFIGURE];
        if hello.plugin_id != "fixture.plugin"
            || hello.session_credential != [9; 32]
            || !(valid_provider || valid_extension)
        {
            return Err(Status::permission_denied("invalid launch"));
        }
        Ok(Response::new(OpenSessionReply {
            session_id: "session".into(),
            granted_capabilities: hello.requested_capabilities,
            api_major: 1,
            api_minor: 4,
            features: hello
                .features
                .iter()
                .map(|feature| FeatureStatus {
                    id: feature.id.clone(),
                    availability: FeatureAvailability::Supported as i32,
                    introduced_minor: 4,
                })
                .collect(),
            support_window: None,
        }))
    }

    async fn submit_discovery(
        &self,
        _: Request<SubmitDiscoveryRequest>,
    ) -> Result<Response<SubmitDiscoveryReply>, Status> {
        Err(Status::permission_denied("not granted"))
    }
    async fn describe_api(
        &self,
        request: Request<DescribeApiRequest>,
    ) -> Result<Response<DescribeApiReply>, Status> {
        if request.get_ref().session_id != "session"
            || request.get_ref().session_credential != [9; 32]
        {
            return Err(Status::unauthenticated("invalid fixture"));
        }
        Ok(Response::new(DescribeApiReply {
            api_major: 1,
            api_minor: 4,
            support_window: None,
            features: vec![],
            methods: vec![],
        }))
    }
    async fn get_permissions(
        &self,
        request: Request<GetPermissionsRequest>,
    ) -> Result<Response<GetPermissionsReply>, Status> {
        if request.get_ref().session_id != "session"
            || request.get_ref().session_credential != [9; 32]
        {
            return Err(Status::unauthenticated("invalid fixture"));
        }
        Ok(Response::new(GetPermissionsReply {
            revision: 3,
            grants: vec![PermissionGrant {
                permission_id: capability::CONTEXT.into(),
                state: "granted".into(),
                scope: "provider".into(),
                reason: String::new(),
                revision: 3,
            }],
        }))
    }
    async fn watch_permission_changes(
        &self,
        request: Request<WatchPermissionChangesRequest>,
    ) -> Result<Response<Self::WatchPermissionChangesStream>, Status> {
        if request.get_ref().session_id != "session"
            || request.get_ref().session_credential != [9; 32]
        {
            return Err(Status::unauthenticated("invalid fixture"));
        }
        Ok(Response::new(tokio_stream::iter(vec![Ok(
            PermissionChange {
                revision: 4,
                grants: vec![],
                revoked: true,
            },
        )])))
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
            .ok_or_else(|| Status::unauthenticated("no session"))?;
        if context.session_id != "session"
            || context.session_credential != [9; 32]
            || !matches!(ready.event, Some(provider_event::Event::Ready(true)))
        {
            return Err(Status::permission_denied("invalid ready event"));
        }
        let (tx, rx) = mpsc::channel(16);
        let completed = self.completed.clone();
        tokio::spawn(async move {
            let command = |operation, id: &str| ProviderCommand {
                context: Some(ServiceContext {
                    operation_id: id.into(),
                    account_epoch: 7,
                    profile_epoch: 11,
                    ..context.clone()
                }),
                operation: operation as i32,
                target_operation_id: if matches!(
                    ProviderOperation::try_from(operation),
                    Ok(ProviderOperation::ConfigurationOpen
                        | ProviderOperation::ConfigurationFocus
                        | ProviderOperation::ConfigurationClose)
                ) {
                    id.into()
                } else {
                    String::new()
                },
                settings: None,
            };
            for (operation, id) in [
                (ProviderOperation::Initialize, "initialize"),
                (ProviderOperation::Discover, "discover"),
                (ProviderOperation::SignIn, "login"),
                (ProviderOperation::FocusSignIn, "focus"),
                (ProviderOperation::CancelSignIn, "cancel"),
                (ProviderOperation::Check, "check"),
                (ProviderOperation::SyncCatalog, "catalog"),
                (ProviderOperation::ReadCounter, "counter"),
                (ProviderOperation::SignOut, "sign-out"),
                (ProviderOperation::ConfigurationOpen, "config-open"),
                (ProviderOperation::ConfigurationFocus, "config-focus"),
                (ProviderOperation::ConfigurationClose, "config-close"),
            ] {
                tx.send(Ok(command(operation, id))).await.unwrap();
            }
            let mut count = 0;
            while let Some(event) = input.message().await.unwrap() {
                let context = authorize(event.context.as_ref()).unwrap();
                if let Some(provider_event::Event::Result(result)) = event.event {
                    completed
                        .send((context.operation_id.clone(), result))
                        .await
                        .unwrap();
                    count += 1;
                    if count == 12 {
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
            provider_id: "fixture".into(),
            platform: "linux".into(),
            max_records: 50_000,
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

struct FixturePlugin {
    started: Semaphore,
    cancelled: Semaphore,
}

#[tonic::async_trait]
impl ProviderPlugin for FixturePlugin {
    async fn handle(
        &self,
        command: ProviderCommand,
        host: HostClient,
    ) -> Result<OperationResult, Error> {
        let context = host.provider_context().await?;
        assert_eq!(context.provider_id, "fixture");
        assert_eq!(
            host.secret(SecretOperation::Read, "session", None).await,
            Err(Error::Denied)
        );
        let result = match ProviderOperation::try_from(command.operation).unwrap() {
            ProviderOperation::Initialize => OperationResult {
                disposition: OperationDisposition::Completed as i32,
                status: Some(AccountStatus {
                    state: AccountConnection::NotConnected as i32,
                    storage: SecretStorage::PrivateFile as i32,
                    ..Default::default()
                }),
                ..Default::default()
            },
            ProviderOperation::Discover => OperationResult {
                disposition: OperationDisposition::Completed as i32,
                discovery: Some(DiscoveryResult {
                    provider_id: "fixture".into(),
                    records: vec![],
                }),
                ..Default::default()
            },
            ProviderOperation::SignIn => {
                self.started.add_permits(1);
                self.cancelled.acquire().await.unwrap().forget();
                OperationResult {
                    disposition: OperationDisposition::Cancelled as i32,
                    login_outcome: Some(AccountLoginOutcome::Cancelled as i32),
                    ..Default::default()
                }
            }
            ProviderOperation::CancelSignIn => {
                self.started.acquire().await.unwrap().forget();
                self.cancelled.add_permits(1);
                OperationResult {
                    disposition: OperationDisposition::Completed as i32,
                    ..Default::default()
                }
            }
            ProviderOperation::FocusSignIn => OperationResult {
                disposition: OperationDisposition::Completed as i32,
                focused: Some(true),
                ..Default::default()
            },
            ProviderOperation::Check => OperationResult {
                disposition: OperationDisposition::Failed as i32,
                status: Some(AccountStatus {
                    state: AccountConnection::Unreachable as i32,
                    storage: SecretStorage::PrivateFile as i32,
                    ..Default::default()
                }),
                failure: Some(PluginFailure {
                    code: "session_unreachable".into(),
                    message: "session check could not reach provider".into(),
                }),
                ..Default::default()
            },
            ProviderOperation::SyncCatalog => OperationResult {
                disposition: OperationDisposition::Completed as i32,
                catalogs: vec![
                    CatalogCompletion {
                        category: CatalogCategory::Owned as i32,
                        outcome: Some(catalog_completion::Outcome::CompletedRecords(0)),
                    },
                    CatalogCompletion {
                        category: CatalogCategory::Family as i32,
                        outcome: Some(catalog_completion::Outcome::Failed(
                            CatalogFailure::FamilyUnavailable as i32,
                        )),
                    },
                ],
                ..Default::default()
            },
            ProviderOperation::ReadCounter => OperationResult {
                disposition: OperationDisposition::Completed as i32,
                counters: vec![CounterObservation {
                    id: "620".into(),
                    seconds: 3_600,
                    last_played: 1_700_000_000,
                }],
                ..Default::default()
            },
            ProviderOperation::SignOut => OperationResult {
                disposition: OperationDisposition::Completed as i32,
                signed_out: true,
                status: Some(AccountStatus {
                    state: AccountConnection::NotConnected as i32,
                    storage: SecretStorage::PrivateFile as i32,
                    ..Default::default()
                }),
                ..Default::default()
            },
            _ => return Err(Error::Protocol),
        };
        Ok(result)
    }
}

struct ExtensionFixture;

#[tonic::async_trait]
impl gamenode_plugin_sdk::ExtensionPlugin for ExtensionFixture {
    async fn handle(&self, _: ProviderCommand, _: HostClient) -> Result<OperationResult, Error> {
        Ok(OperationResult {
            disposition: OperationDisposition::Completed as i32,
            ..Default::default()
        })
    }
    async fn configure(
        &self,
        command: ProviderCommand,
        host: HostClient,
    ) -> Result<OperationResult, Error> {
        let context = host.context();
        Ok(OperationResult {
            disposition: OperationDisposition::Completed as i32,
            configuration: Some(ConfigurationResult {
                session_id: context.session_id.clone(),
                profile_id: String::new(),
                window_id: command.target_operation_id,
                accepted: true,
                failure: String::new(),
            }),
            ..Default::default()
        })
    }
}

#[tokio::test]
async fn independent_plugin_uses_brokers_and_cancels_a_pending_login() {
    let temp = tempfile::tempdir().unwrap();
    let endpoint = temp.path().join("reference.sock");
    let listener = tokio::net::UnixListener::bind(&endpoint).unwrap();
    let (tx, mut completed) = mpsc::channel(16);
    let reference = ReferenceHost { completed: tx };
    let (shutdown, stopped) = oneshot::channel();
    let server = tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(plugin_host_server::PluginHostServer::new(reference.clone()))
            .add_service(provider_host_server::ProviderHostServer::new(reference))
            .serve_with_incoming_shutdown(UnixListenerStream::new(listener), async {
                let _ = stopped.await;
            }),
    );
    let session = Session::open_with_options(
        Launch {
            endpoint,
            credential: vec![9; 32],
            operation_id: "launch".into(),
        },
        "fixture.plugin",
        "fixture",
        "1.0",
        &[capability::CONTEXT, capability::CONFIGURE],
        gamenode_plugin_sdk::SessionOptions {
            min_api_major: 1,
            min_api_minor: 1,
            max_api_major: 1,
            max_api_minor: 4,
            features: vec![(capability::FEATURE_API_METADATA.into(), true)],
            session_kind: SessionKind::Provider,
            target_game: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(session.host.describe_api().await.unwrap().api_minor, 4);
    assert_eq!(
        session.host.get_permissions().await.unwrap().grants[0].permission_id,
        capability::CONTEXT
    );
    let mut changes = session.host.watch_permission_changes(3).await.unwrap();
    assert_eq!(changes.message().await.unwrap().unwrap().revision, 4);
    let plugin = Arc::new(FixturePlugin {
        started: Semaphore::new(0),
        cancelled: Semaphore::new(0),
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        gamenode_plugin_sdk::run(session, plugin),
    )
    .await
    .unwrap()
    .unwrap();
    let mut results = std::collections::BTreeMap::new();
    for _ in 0..12 {
        let (id, result) = completed.recv().await.unwrap();
        results.insert(id, result);
    }
    assert_eq!(
        results["initialize"].status.as_ref().unwrap().state,
        AccountConnection::NotConnected as i32
    );
    assert!(
        results["discover"]
            .discovery
            .as_ref()
            .unwrap()
            .records
            .is_empty()
    );
    assert_eq!(
        results["login"].login_outcome,
        Some(AccountLoginOutcome::Cancelled as i32)
    );
    assert_eq!(results["focus"].focused, Some(true));
    assert_eq!(
        results["check"].disposition,
        OperationDisposition::Failed as i32
    );
    assert_eq!(
        results["check"].failure.as_ref().unwrap().code,
        "session_unreachable"
    );
    let catalog = &results["catalog"].catalogs;
    assert_eq!(
        catalog[0].outcome,
        Some(catalog_completion::Outcome::CompletedRecords(0))
    );
    assert_eq!(
        catalog[1].outcome,
        Some(catalog_completion::Outcome::Failed(
            CatalogFailure::FamilyUnavailable as i32
        ))
    );
    assert_eq!(results["counter"].counters[0].seconds, 3_600);
    assert!(results["sign-out"].signed_out);
    assert_eq!(
        results["config-open"]
            .configuration
            .as_ref()
            .unwrap()
            .window_id,
        "config-open"
    );
    assert!(
        !results["config-open"]
            .configuration
            .as_ref()
            .unwrap()
            .accepted
    );
    let _ = shutdown.send(());
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn extension_session_keeps_target_scope_through_lifecycle_dispatch() {
    let temp = tempfile::tempdir().unwrap();
    let endpoint = temp.path().join("extension.sock");
    let listener = tokio::net::UnixListener::bind(&endpoint).unwrap();
    let (tx, mut completed) = mpsc::channel(16);
    let reference = ReferenceHost { completed: tx };
    let (shutdown, stopped) = oneshot::channel();
    let server = tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(plugin_host_server::PluginHostServer::new(reference.clone()))
            .add_service(provider_host_server::ProviderHostServer::new(reference))
            .serve_with_incoming_shutdown(UnixListenerStream::new(listener), async {
                let _ = stopped.await;
            }),
    );
    let target = TargetGameContext {
        game_id: "steam:620".into(),
        installation_id: "install-1".into(),
        profile_id: "default".into(),
        revision: "r7".into(),
    };
    let session = Session::open_extension(
        Launch {
            endpoint,
            credential: vec![9; 32],
            operation_id: "extension-launch".into(),
        },
        "fixture.plugin",
        "1.0",
        target.clone(),
        &[capability::GAME_LAUNCH, capability::CONFIGURE],
    )
    .await
    .unwrap();
    assert_eq!(session.host.context().provider_id, "");
    assert_eq!(session.host.context().target_game.as_ref(), Some(&target));
    gamenode_plugin_sdk::run_extension(session, Arc::new(ExtensionFixture))
        .await
        .unwrap();
    let mut results = std::collections::BTreeMap::new();
    for _ in 0..12 {
        let (operation, result) = completed.recv().await.unwrap();
        results.insert(operation, result);
    }
    assert_eq!(
        results["config-open"]
            .configuration
            .as_ref()
            .unwrap()
            .window_id,
        "config-open"
    );
    let _ = shutdown.send(());
    server.await.unwrap().unwrap();
}

#[test]
fn public_models_do_not_expose_session_secrets_in_diagnostics() {
    let secret = models::SessionCredential::new("fixture-secret").unwrap();
    assert!(!format!("{secret:?}").contains("fixture-secret"));
}

#[test]
fn detailed_errors_keep_safe_categories_and_structured_denials() {
    use gamenode_plugin_protocol::prost::Message;
    use gamenode_plugin_sdk::DetailedError;
    let detail = DenialDetail {
        reason: DenialReason::Revoked as i32,
        permission_id: "files.read".into(),
        grant_revision: 8,
    };
    let status = Status::with_details(
        tonic::Code::PermissionDenied,
        "private request value",
        detail.encode_to_vec().into(),
    );
    assert_eq!(
        DetailedError::from_status(&status),
        DetailedError::Denied(Some(detail))
    );
    assert_eq!(
        DetailedError::from_status(&Status::unauthenticated("token")),
        DetailedError::Unauthenticated
    );
    assert_eq!(
        DetailedError::from_status(&Status::failed_precondition("stale")),
        DetailedError::Stale
    );
    assert_eq!(
        DetailedError::from_status(&Status::unimplemented("future")),
        DetailedError::Unsupported
    );
    assert_eq!(
        DetailedError::from_status(&Status::invalid_argument("invalid")),
        DetailedError::Invalid
    );
    assert_eq!(
        DetailedError::from_status(&Status::unavailable("offline")),
        DetailedError::Unavailable
    );
    assert!(
        !format!(
            "{:?}",
            DetailedError::from_status(&Status::permission_denied("private request value"))
        )
        .contains("private request value")
    );
}

#[test]
fn sdk_extension_helpers_preserve_unselected_application_namespaces() {
    use gamenode_plugin_sdk::extensions::ExtensionSet;
    use std::collections::HashMap;
    let payload = |schema_id: &str, note: &str| ExtensionPayload {
        schema_id: schema_id.into(),
        schema_version: 1,
        schema_digest: vec![3; 32],
        data: Some(prost_types::Struct {
            fields: std::collections::BTreeMap::from([(
                "note".into(),
                prost_types::Value {
                    kind: Some(prost_types::value::Kind::StringValue(note.into())),
                },
            )]),
        }),
    };
    let wire = HashMap::from([
        (
            "org.example.selected".into(),
            payload("org.example.selected.v1", "old"),
        ),
        (
            "org.other.unknown".into(),
            payload("org.other.unknown.v8", "keep"),
        ),
    ]);
    let mut extensions = ExtensionSet::from_wire(wire).unwrap();
    let patch = ExtensionPatch {
        expected_revision: 4,
        typed_field_mask: vec![],
        namespaces: vec![ExtensionNamespacePatch {
            namespace: "org.example.selected".into(),
            operation: Some(extension_namespace_patch::Operation::Set(payload(
                "org.example.selected.v1",
                "new",
            ))),
        }],
    };
    assert_eq!(extensions.apply_patch(&patch, 4), Ok(5));
    assert_eq!(
        extensions.as_map()["org.other.unknown"],
        payload("org.other.unknown.v8", "keep")
    );
}
