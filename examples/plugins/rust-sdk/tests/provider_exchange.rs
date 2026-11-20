use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
};

use gamenode_plugin_mvp_example::{CAPABILITIES, MvpProvider, PLUGIN_ID, PROVIDER_ID};
use gamenode_plugin_sdk::{
    Launch, Session, SessionOptions,
    models::{SessionCredential, StoreAccount},
    proto::*,
};
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::{ReceiverStream, UnixListenerStream};
use tonic::{Request, Response, Status};

const AUTH_CAPABILITIES: &[&str] = &[
    "provider.context.read",
    "account.state.read",
    "account.state.publish",
    "secrets.read",
    "secrets.write",
    "auth.webview",
];
const READ_CAPABILITIES: &[&str] = &[
    "provider.context.read",
    "account.state.read",
    "secrets.read",
];

#[derive(Default)]
struct FixtureState {
    open: Option<OpenSessionRequest>,
    context_reads: usize,
    catalog_chunks: Vec<CatalogChunk>,
    results: BTreeMap<String, OperationResult>,
    accounts: HashMap<String, AccountIdentity>,
    secrets: HashMap<(String, String), Vec<u8>>,
    grants: HashMap<String, Vec<String>>,
}

#[derive(Clone, Default)]
struct FixtureHost(Arc<Mutex<FixtureState>>);

fn validate_context(context: &ServiceContext) -> Result<(), Status> {
    if !matches!(
        context.session_id.as_str(),
        "fixture-session" | "fixture-profile-b" | "fixture-denied"
    ) || (context.session_id == "fixture-session" && context.session_credential != [7; 32])
        || (context.session_id == "fixture-profile-b" && context.session_credential != [8; 32])
        || (context.session_id == "fixture-denied" && context.session_credential != [9; 32])
        || context.provider_id != PROVIDER_ID
        || context.session_kind != SessionKind::Provider as i32
    {
        return Err(Status::permission_denied("invalid fixture context"));
    }
    Ok(())
}

#[tonic::async_trait]
impl plugin_host_server::PluginHost for FixtureHost {
    type WatchPermissionChangesStream =
        tokio_stream::Iter<std::vec::IntoIter<Result<PermissionChange, Status>>>;

    async fn open_session(
        &self,
        request: Request<OpenSessionRequest>,
    ) -> Result<Response<OpenSessionReply>, Status> {
        let hello = request.into_inner();
        if hello.plugin_id != PLUGIN_ID
            || hello.plugin_version != "0.1.0"
            || (hello.session_credential != [7; 32]
                && hello.session_credential != [8; 32]
                && hello.session_credential != [9; 32])
            || hello.min_api_major != 1
            || hello.min_api_minor != 6
            || hello.max_api_major != 1
            || hello.max_api_minor != 6
            || hello.session_kind != SessionKind::Provider as i32
            || hello.target_game.is_some()
            || (hello.requested_capabilities.as_slice() != CAPABILITIES
                && hello.requested_capabilities.as_slice() != AUTH_CAPABILITIES
                && hello.requested_capabilities.as_slice() != READ_CAPABILITIES
                && hello.requested_capabilities.as_slice() != ["provider.context.read"])
        {
            return Err(Status::permission_denied("invalid provider negotiation"));
        }
        self.0.lock().unwrap().open = Some(hello.clone());
        let session_id = match hello.session_credential[0] {
            8 => "fixture-profile-b",
            9 => "fixture-denied",
            _ => "fixture-session",
        };
        self.0
            .lock()
            .unwrap()
            .grants
            .insert(session_id.into(), hello.requested_capabilities.clone());
        Ok(Response::new(OpenSessionReply {
            session_id: session_id.into(),
            granted_capabilities: hello.requested_capabilities,
            api_major: 1,
            api_minor: 6,
            ..Default::default()
        }))
    }

    async fn submit_discovery(
        &self,
        _: Request<SubmitDiscoveryRequest>,
    ) -> Result<Response<SubmitDiscoveryReply>, Status> {
        Err(Status::permission_denied("not used by API 1.6 providers"))
    }

    async fn describe_api(
        &self,
        request: Request<DescribeApiRequest>,
    ) -> Result<Response<DescribeApiReply>, Status> {
        let request = request.get_ref();
        if !((request.session_id == "fixture-session" && request.session_credential == [7; 32])
            || (request.session_id == "fixture-profile-b" && request.session_credential == [8; 32])
            || (request.session_id == "fixture-denied" && request.session_credential == [9; 32]))
        {
            return Err(Status::unauthenticated("invalid session"));
        }
        Ok(Response::new(DescribeApiReply {
            api_major: 1,
            api_minor: 6,
            ..Default::default()
        }))
    }

    async fn get_permissions(
        &self,
        request: Request<GetPermissionsRequest>,
    ) -> Result<Response<GetPermissionsReply>, Status> {
        let request = request.get_ref();
        if !((request.session_id == "fixture-session" && request.session_credential == [7; 32])
            || (request.session_id == "fixture-profile-b" && request.session_credential == [8; 32])
            || (request.session_id == "fixture-denied" && request.session_credential == [9; 32]))
        {
            return Err(Status::unauthenticated("invalid session"));
        }
        Ok(Response::new(GetPermissionsReply::default()))
    }

    async fn watch_permission_changes(
        &self,
        _: Request<WatchPermissionChangesRequest>,
    ) -> Result<Response<Self::WatchPermissionChangesStream>, Status> {
        Ok(Response::new(tokio_stream::iter(Vec::new())))
    }
}

#[tonic::async_trait]
impl provider_host_server::ProviderHost for FixtureHost {
    type ExchangeStream = ReceiverStream<Result<ProviderCommand, Status>>;

    async fn exchange(
        &self,
        request: Request<tonic::Streaming<ProviderEvent>>,
    ) -> Result<Response<Self::ExchangeStream>, Status> {
        let mut input = request.into_inner();
        let ready = input
            .message()
            .await?
            .ok_or_else(|| Status::unauthenticated("missing Ready"))?;
        let context = ready
            .context
            .ok_or_else(|| Status::unauthenticated("missing Ready context"))?;
        validate_context(&context)?;
        if !matches!(ready.event, Some(provider_event::Event::Ready(true))) {
            return Err(Status::permission_denied("provider did not send Ready"));
        }

        let (tx, rx) = mpsc::channel(8);
        let state = self.0.clone();
        tokio::spawn(async move {
            let commands = [
                (ProviderOperation::Initialize, "initialize"),
                (ProviderOperation::Discover, "discover"),
                (ProviderOperation::SyncCatalog, "catalog"),
                (ProviderOperation::ReadCounter, "counter"),
            ];
            for (operation, id) in commands {
                let command_context = ServiceContext {
                    operation_id: id.into(),
                    ..context.clone()
                };
                tx.send(Ok(ProviderCommand {
                    context: Some(command_context.clone()),
                    operation: operation as i32,
                    ..Default::default()
                }))
                .await
                .map_err(|_| ())?;

                loop {
                    let Some(event) = input.message().await.map_err(|_| ())? else {
                        return Err(());
                    };
                    if event.context.as_ref() != Some(&command_context) {
                        return Err(());
                    }
                    match event.event {
                        Some(provider_event::Event::CatalogChunk(chunk)) => {
                            state.lock().unwrap().catalog_chunks.push(chunk);
                        }
                        Some(provider_event::Event::Result(result)) => {
                            state.lock().unwrap().results.insert(id.into(), result);
                            break;
                        }
                        _ => return Err(()),
                    }
                }
            }

            let _ = tx
                .send(Ok(ProviderCommand {
                    context: Some(ServiceContext {
                        operation_id: "shutdown".into(),
                        ..context
                    }),
                    operation: ProviderOperation::Shutdown as i32,
                    ..Default::default()
                }))
                .await;
            Ok::<(), ()>(())
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn get_context(
        &self,
        request: Request<ServiceContext>,
    ) -> Result<Response<ProviderContext>, Status> {
        validate_context(request.get_ref())?;
        self.0.lock().unwrap().context_reads += 1;
        Ok(Response::new(ProviderContext {
            provider_id: PROVIDER_ID.into(),
            platform: "fixture".into(),
            max_records: 100,
            max_message_bytes: 1024 * 1024,
            catalog_chunk_records: 256,
            ..Default::default()
        }))
    }

    async fn account(
        &self,
        request: Request<AccountRequest>,
    ) -> Result<Response<AccountReply>, Status> {
        let request = request.into_inner();
        let context = request
            .context
            .as_ref()
            .ok_or_else(|| Status::unauthenticated("missing context"))?;
        validate_context(context)?;
        let mut state = self.0.lock().unwrap();
        match request
            .action
            .ok_or_else(|| Status::invalid_argument("missing account action"))?
        {
            account_request::Action::Read(true)
                if state.grants[&context.session_id]
                    .iter()
                    .any(|g| g == "account.state.read") =>
            {
                Ok(Response::new(AccountReply {
                    account: state.accounts.get(&context.session_id).cloned(),
                    accepted: true,
                }))
            }
            account_request::Action::Save(account)
                if account.provider_id == PROVIDER_ID
                    && state.grants[&context.session_id]
                        .iter()
                        .any(|g| g == "account.state.publish") =>
            {
                state.accounts.insert(context.session_id.clone(), account);
                Ok(Response::new(AccountReply {
                    accepted: true,
                    ..Default::default()
                }))
            }
            account_request::Action::Clear(true) => {
                if !state.grants[&context.session_id]
                    .iter()
                    .any(|grant| grant == "account.state.publish")
                {
                    return Err(Status::permission_denied(
                        "account publish grant not issued",
                    ));
                }
                state.accounts.remove(&context.session_id);
                Ok(Response::new(AccountReply {
                    accepted: true,
                    ..Default::default()
                }))
            }
            _ => Err(Status::permission_denied("account grant not issued")),
        }
    }

    async fn secret(
        &self,
        request: Request<SecretRequest>,
    ) -> Result<Response<SecretReply>, Status> {
        let request = request.into_inner();
        let context = request
            .context
            .as_ref()
            .ok_or_else(|| Status::unauthenticated("missing context"))?;
        validate_context(context)?;
        let key = (context.session_id.clone(), request.key);
        let mut state = self.0.lock().unwrap();
        let operation = SecretOperation::try_from(request.operation)
            .map_err(|_| Status::invalid_argument("invalid operation"))?;
        let permission = match operation {
            SecretOperation::Read => "secrets.read",
            SecretOperation::Write => "secrets.write",
            SecretOperation::Delete => "secrets.delete",
            _ => "",
        };
        if !state.grants[&context.session_id]
            .iter()
            .any(|grant| grant == permission)
        {
            return Err(Status::permission_denied("secret grant not issued"));
        }
        match operation {
            SecretOperation::Write if request.value.as_deref() == Some(b"fixture-token") => {
                state.secrets.insert(key, request.value.unwrap());
                Ok(Response::new(SecretReply {
                    outcome: Some(secret_reply::Outcome::Accepted(true)),
                    ..Default::default()
                }))
            }
            SecretOperation::Read => Ok(Response::new(match state.secrets.get(&key) {
                Some(value) => SecretReply {
                    outcome: Some(secret_reply::Outcome::Value(value.clone())),
                    ..Default::default()
                },
                None => SecretReply {
                    outcome: Some(secret_reply::Outcome::Absent(true)),
                    ..Default::default()
                },
            })),
            SecretOperation::Delete => {
                state.secrets.remove(&key);
                Ok(Response::new(SecretReply {
                    outcome: Some(secret_reply::Outcome::Accepted(true)),
                    ..Default::default()
                }))
            }
            _ => Err(Status::permission_denied(
                "secret grant not issued or invalid fixture value",
            )),
        }
    }

    async fn login(&self, request: Request<LoginRequest>) -> Result<Response<LoginReply>, Status> {
        let request = request.into_inner();
        let context = request
            .context
            .as_ref()
            .ok_or_else(|| Status::unauthenticated("missing context"))?;
        validate_context(context)?;
        if !self.0.lock().unwrap().grants[&context.session_id]
            .iter()
            .any(|grant| grant == "auth.webview")
        {
            return Err(Status::permission_denied("login grant not issued"));
        }
        if request.handle != "host-defined-fixture-login" {
            return Err(Status::permission_denied(
                "login handle is not in host fixture policy",
            ));
        }
        Ok(Response::new(LoginReply {
            handle: request.handle,
            state: LoginState::Open as i32,
            cookies: vec![],
            page_result_json: Some(r#"{"authenticated":true}"#.into()),
        }))
    }

    async fn declare_actions(
        &self,
        _: Request<ActionRequest>,
    ) -> Result<Response<ActionReply>, Status> {
        Err(Status::permission_denied("not granted"))
    }
}

#[tokio::test]
async fn negotiates_authenticated_session_and_exchanges_mvp_operations() {
    let temp = tempfile::tempdir().unwrap();
    let socket = temp.path().join("provider.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let host = FixtureHost::default();
    let state = host.0.clone();
    let (shutdown, stopped) = oneshot::channel();
    let server = tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(plugin_host_server::PluginHostServer::new(host.clone()))
            .add_service(provider_host_server::ProviderHostServer::new(host))
            .serve_with_incoming_shutdown(UnixListenerStream::new(listener), async {
                let _ = stopped.await;
            }),
    );

    let session = Session::open_with_options(
        Launch {
            endpoint: socket,
            credential: vec![7; 32],
            operation_id: "fixture-launch".into(),
        },
        PLUGIN_ID,
        PROVIDER_ID,
        "0.1.0",
        CAPABILITIES,
        SessionOptions {
            min_api_major: 1,
            min_api_minor: 6,
            max_api_major: 1,
            max_api_minor: 6,
            features: vec![],
            session_kind: SessionKind::Provider,
            target_game: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(session.host.context().session_id, "fixture-session");
    assert_eq!(session.host.context().session_credential, [7; 32]);
    assert_eq!(session.host.describe_api().await.unwrap().api_minor, 6);
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        gamenode_plugin_sdk::run(session, Arc::new(MvpProvider)),
    )
    .await
    .unwrap()
    .unwrap();

    let state = state.lock().unwrap();
    let hello = state.open.as_ref().unwrap();
    assert_eq!(hello.requested_capabilities, CAPABILITIES);
    assert_eq!(state.context_reads, 4);

    let discovery = state.results["discover"].discovery.as_ref().unwrap();
    assert_eq!(discovery.provider_id, PROVIDER_ID);
    assert_eq!(discovery.records.len(), 1);
    assert_eq!(discovery.records[0].id, "demo-installed-game");
    assert!(discovery.records[0].target.is_none());

    assert_eq!(state.catalog_chunks.len(), 1);
    let record = &state.catalog_chunks[0].games[0];
    assert_eq!(record.id, "demo-catalog-game");
    assert_eq!(record.fields["edition"], "standard");
    assert_eq!(
        state.results["catalog"].catalogs[0].outcome,
        Some(catalog_completion::Outcome::CompletedRecords(1))
    );

    let counter = &state.results["counter"].counters[0];
    assert_eq!(counter.id, "demo-catalog-game");
    assert_eq!(counter.seconds, 7_200);
    assert_eq!(counter.last_played, Some(1_700_000_000));

    let _ = shutdown.send(());
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn account_secret_and_host_defined_login_are_profile_scoped_and_grant_checked() {
    let temp = tempfile::tempdir().unwrap();
    let socket = temp.path().join("auth-fixture.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let host = FixtureHost::default();
    let (shutdown, stopped) = oneshot::channel();
    let server = tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(plugin_host_server::PluginHostServer::new(host.clone()))
            .add_service(provider_host_server::ProviderHostServer::new(host))
            .serve_with_incoming_shutdown(UnixListenerStream::new(listener), async {
                let _ = stopped.await;
            }),
    );

    async fn open(socket: std::path::PathBuf, credential: u8, capabilities: &[&str]) -> Session {
        Session::open_with_options(
            Launch {
                endpoint: socket,
                credential: vec![credential; 32],
                operation_id: "fixture-launch".into(),
            },
            PLUGIN_ID,
            PROVIDER_ID,
            "0.1.0",
            capabilities,
            SessionOptions {
                min_api_major: 1,
                min_api_minor: 6,
                max_api_major: 1,
                max_api_minor: 6,
                features: vec![],
                session_kind: SessionKind::Provider,
                target_game: None,
            },
        )
        .await
        .unwrap()
    }

    let profile_a = open(socket.clone(), 7, AUTH_CAPABILITIES).await;
    profile_a
        .host
        .account_save(StoreAccount {
            provider: PROVIDER_ID.into(),
            account_id: "fixture-account".into(),
            account_name: Some("Fake User".into()),
            last_verified_at: Some(1_700_000_000),
        })
        .await
        .unwrap();
    assert_eq!(
        profile_a
            .host
            .account_load()
            .await
            .unwrap()
            .unwrap()
            .account_id,
        "fixture-account"
    );

    let token = SessionCredential::new("fixture-token").unwrap();
    profile_a
        .host
        .secret(SecretOperation::Write, "session", Some(&token))
        .await
        .unwrap();
    let secret = profile_a
        .host
        .secret(SecretOperation::Read, "session", None)
        .await
        .unwrap();
    assert!(
        matches!(secret.outcome, Some(secret_reply::Outcome::Value(value)) if value == b"fixture-token")
    );

    let login = profile_a
        .host
        .login(LoginOperation::Open, "host-defined-fixture-login")
        .await
        .unwrap();
    assert_eq!(login.state, LoginState::Open as i32);
    assert!(login.cookies.is_empty(), "cookie access was not granted");
    assert_eq!(
        login.page_result_json.as_deref(),
        Some(r#"{"authenticated":true}"#)
    );
    assert!(
        profile_a
            .host
            .login(LoginOperation::Open, "plugin-supplied-script")
            .await
            .is_err()
    );

    let profile_b = open(socket.clone(), 8, READ_CAPABILITIES).await;
    assert!(
        profile_b.host.account_load().await.unwrap().is_none(),
        "account state leaked across profiles"
    );
    assert!(matches!(
        profile_b
            .host
            .secret(SecretOperation::Read, "session", None)
            .await
            .unwrap()
            .outcome,
        Some(secret_reply::Outcome::Absent(true))
    ));
    assert!(
        profile_b
            .host
            .login(LoginOperation::Open, "host-defined-fixture-login")
            .await
            .is_err(),
        "login bypassed its grant in a profile without login consent"
    );

    let denied = open(socket, 9, &["provider.context.read"]).await;
    assert!(
        denied.host.account_load().await.is_err(),
        "account read bypassed its grant"
    );
    assert!(
        denied
            .host
            .secret(SecretOperation::Read, "session", None)
            .await
            .is_err(),
        "secret read bypassed its grant"
    );
    assert!(
        denied
            .host
            .login(LoginOperation::Open, "host-defined-fixture-login")
            .await
            .is_err(),
        "login bypassed its grant"
    );

    drop((profile_a, profile_b, denied));
    let _ = shutdown.send(());
    server.await.unwrap().unwrap();
}
