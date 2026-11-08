//! The SDK and a fixture plugin exercise the public wire contract without Core,
//! the real app, its database or its platform adapters.
#![cfg(unix)]

use std::sync::Arc;

use gamenode_plugin_sdk::{
    Error, HostClient, Launch, ProviderPlugin, Session, capability, models, proto::*,
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
        || context.provider_id != "fixture"
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
    async fn open_session(
        &self,
        request: Request<OpenSessionRequest>,
    ) -> Result<Response<OpenSessionReply>, Status> {
        let hello = request.into_inner();
        if hello.plugin_id != "fixture.plugin"
            || hello.session_credential != [9; 32]
            || hello.requested_capabilities != [capability::CONTEXT]
        {
            return Err(Status::permission_denied("invalid launch"));
        }
        Ok(Response::new(OpenSessionReply {
            session_id: "session".into(),
            granted_capabilities: hello.requested_capabilities,
            api_major: 1,
            api_minor: 2,
        }))
    }

    async fn submit_discovery(
        &self,
        _: Request<SubmitDiscoveryRequest>,
    ) -> Result<Response<SubmitDiscoveryReply>, Status> {
        Err(Status::permission_denied("not granted"))
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
                target_operation_id: String::new(),
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
                    if count == 9 {
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
    let session = Session::open(
        Launch {
            endpoint,
            credential: vec![9; 32],
            operation_id: "launch".into(),
        },
        "fixture.plugin",
        "fixture",
        "1.0",
        &[capability::CONTEXT],
    )
    .await
    .unwrap();
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
    for _ in 0..9 {
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
    let _ = shutdown.send(());
    server.await.unwrap().unwrap();
}

#[test]
fn public_models_do_not_expose_session_secrets_in_diagnostics() {
    let secret = models::SessionCredential::new("fixture-secret").unwrap();
    assert!(!format!("{secret:?}").contains("fixture-secret"));
}
