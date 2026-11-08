//! The actual Steam provider runs against a standalone public reference host.
//! No app, Core, platform, database, real browser or real Steam account is used.
use super::*;
use gamenode_plugin_protocol::{API_MAJOR, API_MINOR, capability as cap};
use gamenode_plugin_sdk::{Launch, Session};
use std::{collections::BTreeMap, sync::atomic::AtomicUsize};
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::{ReceiverStream, UnixListenerStream};
use tonic::{Request, Response, Status};

type Commands = mpsc::Receiver<Result<ProviderCommand, Status>>;

#[derive(Clone)]
struct Host {
    commands: Arc<tokio::sync::Mutex<Option<Commands>>>,
    events: mpsc::Sender<ProviderEvent>,
    credential: Arc<Mutex<Option<Vec<u8>>>>,
    account: Arc<Mutex<Option<AccountIdentity>>>,
    fail_delete: Arc<AtomicBool>,
    login: Arc<LoginControl>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Observation {
    Verified,
    CookieOnly,
    PageOnly,
    Closed,
    TimedOut,
}

struct LoginControl {
    page: Mutex<Observation>,
    delay: AtomicBool,
    reading: tokio::sync::Semaphore,
    release: tokio::sync::Semaphore,
    handle: Mutex<Option<String>>,
    opens: AtomicUsize,
    focuses: AtomicUsize,
    cancels: AtomicUsize,
    closes: AtomicUsize,
    writes: AtomicUsize,
    pages: Mutex<Vec<Observation>>,
}

impl Default for LoginControl {
    fn default() -> Self {
        Self {
            page: Mutex::new(Observation::Verified),
            delay: false.into(),
            reading: tokio::sync::Semaphore::new(0),
            release: tokio::sync::Semaphore::new(0),
            handle: Mutex::new(None),
            opens: 0.into(),
            focuses: 0.into(),
            cancels: 0.into(),
            closes: 0.into(),
            writes: 0.into(),
            pages: Mutex::new(vec![]),
        }
    }
}

impl LoginControl {
    async fn read_started(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(5), self.reading.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
    }
    async fn page_read(&self, expected: Observation) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if lock(&self.pages).contains(&expected) {
                    return;
                }
                self.reading.acquire().await.unwrap().forget();
            }
        })
        .await
        .unwrap();
    }
}

fn authorize(context: Option<&ServiceContext>) -> Result<(), Status> {
    let context = context.ok_or_else(|| Status::unauthenticated("context required"))?;
    if context.provider_id != "steam"
        || context.session_id != "reference"
        || context.session_credential != [9; 32]
    {
        return Err(Status::permission_denied("invalid reference context"));
    }
    Ok(())
}

#[tonic::async_trait]
impl plugin_host_server::PluginHost for Host {
    async fn open_session(
        &self,
        request: Request<OpenSessionRequest>,
    ) -> Result<Response<OpenSessionReply>, Status> {
        let hello = request.into_inner();
        if hello.plugin_id != "org.gamenode.plugin.steam-discovery"
            || hello.session_credential != [9; 32]
            || hello.requested_capabilities
                != cap::ALL.iter().map(|p| (*p).to_owned()).collect::<Vec<_>>()
        {
            return Err(Status::permission_denied("invalid launch"));
        }
        Ok(Response::new(OpenSessionReply {
            session_id: "reference".into(),
            api_major: API_MAJOR,
            api_minor: API_MINOR,
            granted_capabilities: hello.requested_capabilities,
        }))
    }
    async fn submit_discovery(
        &self,
        _: Request<SubmitDiscoveryRequest>,
    ) -> Result<Response<SubmitDiscoveryReply>, Status> {
        Err(Status::unimplemented("stream only"))
    }
}

#[tonic::async_trait]
impl provider_host_server::ProviderHost for Host {
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
        authorize(ready.context.as_ref())?;
        assert!(matches!(
            ready.event,
            Some(provider_event::Event::Ready(true))
        ));
        let receiver = self
            .commands
            .lock()
            .await
            .take()
            .ok_or_else(|| Status::already_exists("stream already connected"))?;
        let events = self.events.clone();
        tokio::spawn(async move {
            while let Ok(Some(event)) = input.message().await {
                authorize(event.context.as_ref()).unwrap();
                if events.send(event).await.is_err() {
                    break;
                }
            }
        });
        Ok(Response::new(ReceiverStream::new(receiver)))
    }
    async fn get_context(
        &self,
        request: Request<ServiceContext>,
    ) -> Result<Response<ProviderContext>, Status> {
        authorize(Some(request.get_ref()))?;
        Ok(Response::new(ProviderContext {
            provider_id: "steam".into(),
            platform: "linux".into(),
            manifest_toml: include_str!("../../../../providers/steam.toml").into(),
            secret_storage: SecretStorage::PrivateFile as i32,
            ..Default::default()
        }))
    }
    async fn account(
        &self,
        request: Request<AccountRequest>,
    ) -> Result<Response<AccountReply>, Status> {
        let request = request.into_inner();
        authorize(request.context.as_ref())?;
        let mut account = lock(&self.account);
        match request.action.unwrap() {
            account_request::Action::Read(true) => {}
            account_request::Action::Save(value) => {
                assert_eq!(value.provider_id, "steam");
                *account = Some(value);
            }
            account_request::Action::Clear(true) => *account = None,
            _ => return Err(Status::invalid_argument("invalid action")),
        }
        Ok(Response::new(AccountReply {
            account: account.clone(),
            accepted: true,
        }))
    }
    async fn secret(
        &self,
        request: Request<SecretRequest>,
    ) -> Result<Response<SecretReply>, Status> {
        use secret_reply::Outcome;
        let request = request.into_inner();
        authorize(request.context.as_ref())?;
        assert_eq!(request.key, "session");
        let mut credential = lock(&self.credential);
        let outcome = match SecretOperation::try_from(request.operation).unwrap() {
            SecretOperation::Read => credential
                .clone()
                .map_or(Outcome::Absent(true), Outcome::Value),
            SecretOperation::Write => {
                self.login.writes.fetch_add(1, Ordering::AcqRel);
                *credential = request.value;
                Outcome::Accepted(true)
            }
            SecretOperation::Delete if self.fail_delete.load(Ordering::Acquire) => {
                Outcome::Failed(ServiceFailure::Failed as i32)
            }
            SecretOperation::Delete => {
                *credential = None;
                Outcome::Accepted(true)
            }
            _ => return Err(Status::invalid_argument("invalid action")),
        };
        Ok(Response::new(SecretReply {
            storage: SecretStorage::PrivateFile as i32,
            outcome: Some(outcome),
        }))
    }
    async fn login(&self, request: Request<LoginRequest>) -> Result<Response<LoginReply>, Status> {
        let request = request.into_inner();
        authorize(request.context.as_ref())?;
        let reply = match LoginOperation::try_from(request.operation).unwrap() {
            LoginOperation::Open => {
                let handle = format!(
                    "attempt-{}",
                    self.login.opens.fetch_add(1, Ordering::AcqRel)
                );
                *lock(&self.login.handle) = Some(handle.clone());
                LoginReply {
                    handle,
                    state: LoginState::Open as i32,
                    ..Default::default()
                }
            }
            LoginOperation::Read => {
                // Capture a page before pausing the RPC. Tests can deliver this
                // authenticated observation after cancellation/sign-out.
                let page = *lock(&self.login.page);
                lock(&self.login.pages).push(page);
                self.login.reading.add_permits(1);
                if self.login.delay.load(Ordering::Acquire) {
                    self.login.release.acquire().await.unwrap().forget();
                }
                LoginReply {
                    handle: request.handle,
                    state: match page { Observation::Closed => LoginState::Closed, Observation::TimedOut => LoginState::TimedOut, _ => LoginState::Open } as i32,
                    cookies: if matches!(page, Observation::Verified | Observation::CookieOnly) { vec![LoginCookie { name: "steamLoginSecure".into(), value: "cookie-secret".into() }] } else { vec![] },
                    page_result_json: matches!(page, Observation::Verified | Observation::PageOnly).then(|| r#"{"steam_id":"76561198000000001","account_name":"player","token":"temporary-token-secret"}"#.into()),
                }
            }
            LoginOperation::Focus => {
                self.login.focuses.fetch_add(1, Ordering::AcqRel);
                LoginReply {
                    state: if lock(&self.login.handle).as_deref() == Some(&request.handle) {
                        LoginState::Open
                    } else {
                        LoginState::Closed
                    } as i32,
                    ..Default::default()
                }
            }
            LoginOperation::Cancel | LoginOperation::Close => {
                if request.operation == LoginOperation::Cancel as i32 {
                    self.login.cancels.fetch_add(1, Ordering::AcqRel);
                } else {
                    self.login.closes.fetch_add(1, Ordering::AcqRel);
                }
                let mut handle = lock(&self.login.handle);
                if handle.as_deref() == Some(&request.handle) {
                    *handle = None;
                }
                LoginReply {
                    state: LoginState::Closed as i32,
                    ..Default::default()
                }
            }
            _ => return Err(Status::invalid_argument("invalid action")),
        };
        Ok(Response::new(reply))
    }
    async fn declare_actions(
        &self,
        request: Request<ActionRequest>,
    ) -> Result<Response<ActionReply>, Status> {
        authorize(request.get_ref().context.as_ref())?;
        assert_eq!(
            request
                .get_ref()
                .actions
                .as_ref()
                .unwrap()
                .play_uri
                .as_deref(),
            Some("steam://rungameid/{id}")
        );
        Ok(Response::new(ActionReply { accepted: true }))
    }
}

struct Client;
#[async_trait::async_trait]
impl crate::steam::SteamWebApi for Client {
    async fn session(
        &self,
        cookie: &models::SessionCredential,
    ) -> Result<crate::steam::SteamSession, crate::steam::SteamError> {
        Ok(crate::steam::SteamSession {
            steam_id: "76561198000000001".into(),
            account_name: Some("player".into()),
            token: models::SessionCredential::new("temporary-token-secret").unwrap(),
            cookie: cookie.clone(),
        })
    }
    async fn owned_games(
        &self,
        _: &crate::steam::SteamSession,
    ) -> Result<Vec<crate::steam::SteamOwnedGame>, crate::steam::SteamError> {
        Ok(vec![
            crate::steam::SteamOwnedGame {
                app_id: "620".into(),
                name: "Portal 2".into(),
            },
            crate::steam::SteamOwnedGame {
                app_id: "228980".into(),
                name: "excluded runtime".into(),
            },
        ])
    }
    async fn family_games(
        &self,
        _: &crate::steam::SteamSession,
    ) -> Result<crate::steam::SteamFamily, crate::steam::SteamError> {
        Err(crate::steam::SteamError::Server)
    }
}

async fn execute(
    commands: &mpsc::Sender<Result<ProviderCommand, Status>>,
    events: &mut mpsc::Receiver<ProviderEvent>,
    kind: ProviderOperation,
    epoch: u64,
    id: &str,
) -> (OperationResult, Vec<CatalogChunk>) {
    commands
        .send(Ok(ProviderCommand {
            context: Some(ServiceContext {
                session_id: "reference".into(),
                session_credential: vec![9; 32],
                provider_id: "steam".into(),
                operation_id: id.into(),
                account_epoch: epoch,
                profile_epoch: 1,
            }),
            operation: kind as i32,
            ..Default::default()
        }))
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut chunks = vec![];
        loop {
            let event = events.recv().await.expect("plugin stream alive");
            assert_eq!(event.context.as_ref().unwrap().operation_id, id);
            match event.event.unwrap() {
                provider_event::Event::CatalogChunk(chunk) => chunks.push(chunk),
                provider_event::Event::Result(result) => return (result, chunks),
                _ => {}
            }
        }
    })
    .await
    .expect("provider operation completes without retaining an observer queue")
}

#[tokio::test]
async fn full_provider_restores_logs_in_syncs_and_handles_failed_then_successful_sign_out() {
    let temp = tempfile::tempdir().unwrap();
    let endpoint = temp.path().join("reference.sock");
    let listener = tokio::net::UnixListener::bind(&endpoint).unwrap();
    let (commands, receiver) = mpsc::channel(16);
    let (events, mut results) = mpsc::channel(64);
    let host = Host {
        commands: Arc::new(tokio::sync::Mutex::new(Some(receiver))),
        events,
        credential: Arc::new(Mutex::new(Some(b"saved-cookie-secret".to_vec()))),
        account: Arc::new(Mutex::new(Some(AccountIdentity {
            provider_id: "steam".into(),
            account_id: "76561198000000001".into(),
            account_name: Some("player".into()),
            last_verified_at: Some(10),
        }))),
        fail_delete: Arc::new(AtomicBool::new(false)),
        login: Arc::new(LoginControl::default()),
    };
    let (stop, stopped) = oneshot::channel();
    let server = tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(plugin_host_server::PluginHostServer::new(host.clone()))
            .add_service(provider_host_server::ProviderHostServer::new(host.clone()))
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
        "org.gamenode.plugin.steam-discovery",
        "steam",
        "test",
        cap::ALL,
    )
    .await
    .unwrap();
    let plugin = Arc::new(SteamPlugin {
        client: Arc::new(Client),
        account: Mutex::new(None),
        login: LoginAttempt::default(),
        contexts: Mutex::new(Contexts::default()),
    });
    let run = tokio::spawn(gamenode_plugin_sdk::run(session, plugin));
    let (init, _) = execute(
        &commands,
        &mut results,
        ProviderOperation::Initialize,
        1,
        "init",
    )
    .await;
    assert_eq!(
        init.status.unwrap().state,
        AccountConnection::Checking as i32
    );
    let (restored, _) = execute(
        &commands,
        &mut results,
        ProviderOperation::Check,
        1,
        "restore",
    )
    .await;
    assert_eq!(
        restored.status.unwrap().state,
        AccountConnection::Connected as i32
    );
    assert_eq!(
        lock(&host.credential).as_deref(),
        Some(b"saved-cookie-secret".as_slice())
    );
    let (signed_in, _) = execute(
        &commands,
        &mut results,
        ProviderOperation::SignIn,
        2,
        "signin",
    )
    .await;
    assert_eq!(
        signed_in.login_outcome,
        Some(AccountLoginOutcome::Connected as i32)
    );
    assert_eq!(
        signed_in.status.unwrap().state,
        AccountConnection::Connected as i32
    );
    assert_eq!(
        lock(&host.credential).as_deref(),
        Some(b"steamLoginSecure=cookie-secret".as_slice())
    );
    let (catalog, chunks) = execute(
        &commands,
        &mut results,
        ProviderOperation::SyncCatalog,
        2,
        "catalog",
    )
    .await;
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].games.len(), 1);
    assert_eq!(chunks[0].games[0].id, "620");
    assert!(
        matches!(catalog.catalogs[1].outcome, Some(catalog_completion::Outcome::Failed(code)) if code == CatalogFailure::FamilyUnavailable as i32)
    );
    host.fail_delete.store(true, Ordering::Release);
    let (failed, _) = execute(
        &commands,
        &mut results,
        ProviderOperation::SignOut,
        3,
        "failed-out",
    )
    .await;
    assert!(!failed.signed_out);
    let status = failed.status.unwrap();
    assert_eq!(status.state, AccountConnection::Connected as i32);
    assert!(status.persist_failed);
    assert!(lock(&host.credential).is_some());
    host.fail_delete.store(false, Ordering::Release);
    let (success, _) = execute(
        &commands,
        &mut results,
        ProviderOperation::SignOut,
        4,
        "out",
    )
    .await;
    assert!(success.signed_out);
    assert!(lock(&host.credential).is_none());
    assert!(lock(&host.account).is_none());
    commands
        .send(Ok(ProviderCommand {
            context: Some(ServiceContext {
                session_id: "reference".into(),
                session_credential: vec![9; 32],
                provider_id: "steam".into(),
                operation_id: "shutdown".into(),
                ..Default::default()
            }),
            operation: ProviderOperation::Shutdown as i32,
            ..Default::default()
        }))
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), run)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    stop.send(()).unwrap();
    server.await.unwrap().unwrap();
}

struct Harness {
    commands: mpsc::Sender<Result<ProviderCommand, Status>>,
    events: mpsc::Receiver<ProviderEvent>,
    completed: BTreeMap<String, OperationResult>,
    host: Host,
    run: tokio::task::JoinHandle<Result<(), Error>>,
    stop: oneshot::Sender<()>,
    server: tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
    _temp: tempfile::TempDir,
}

impl Harness {
    async fn open(connected: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let endpoint = temp.path().join("reference.sock");
        let listener = tokio::net::UnixListener::bind(&endpoint).unwrap();
        let (commands, receiver) = mpsc::channel(16);
        let (events, results) = mpsc::channel(64);
        let host = Host {
            commands: Arc::new(tokio::sync::Mutex::new(Some(receiver))),
            events,
            credential: Arc::new(Mutex::new(
                connected.then(|| b"saved-cookie-secret".to_vec()),
            )),
            account: Arc::new(Mutex::new(connected.then(|| AccountIdentity {
                provider_id: "steam".into(),
                account_id: "76561198000000001".into(),
                account_name: Some("player".into()),
                last_verified_at: Some(10),
            }))),
            fail_delete: Arc::new(AtomicBool::new(false)),
            login: Arc::new(LoginControl::default()),
        };
        let (stop, stopped) = oneshot::channel();
        let server = tokio::spawn(
            tonic::transport::Server::builder()
                .add_service(plugin_host_server::PluginHostServer::new(host.clone()))
                .add_service(provider_host_server::ProviderHostServer::new(host.clone()))
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
            "org.gamenode.plugin.steam-discovery",
            "steam",
            "test",
            cap::ALL,
        )
        .await
        .unwrap();
        let plugin = Arc::new(SteamPlugin {
            client: Arc::new(Client),
            account: Mutex::new(None),
            login: LoginAttempt::default(),
            contexts: Mutex::new(Contexts::default()),
        });
        let run = tokio::spawn(gamenode_plugin_sdk::run(session, plugin));
        let mut harness = Self {
            commands,
            events: results,
            completed: BTreeMap::new(),
            host,
            run,
            stop,
            server,
            _temp: temp,
        };
        harness.send(ProviderOperation::Initialize, 1, "init").await;
        harness.result("init").await;
        if connected {
            harness.send(ProviderOperation::Check, 1, "restore").await;
            assert_eq!(
                harness.result("restore").await.status.unwrap().state,
                AccountConnection::Connected as i32
            );
        }
        harness
    }

    async fn send(&self, kind: ProviderOperation, epoch: u64, id: &str) {
        self.commands
            .send(Ok(ProviderCommand {
                context: Some(ServiceContext {
                    session_id: "reference".into(),
                    session_credential: vec![9; 32],
                    provider_id: "steam".into(),
                    operation_id: id.into(),
                    account_epoch: epoch,
                    profile_epoch: 1,
                }),
                operation: kind as i32,
                ..Default::default()
            }))
            .await
            .unwrap();
    }

    async fn result(&mut self, id: &str) -> OperationResult {
        if let Some(result) = self.completed.remove(id) {
            return result;
        }
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let event = self.events.recv().await.expect("plugin stream alive");
                let operation = event.context.unwrap().operation_id;
                if let Some(provider_event::Event::Result(result)) = event.event {
                    assert!(result.failure.is_none());
                    if operation == id {
                        return result;
                    }
                    assert!(self.completed.insert(operation, result).is_none());
                }
            }
        })
        .await
        .expect("control/operation remains responsive while another login waits")
    }

    async fn close(self) {
        self.send(ProviderOperation::Shutdown, 99, "shutdown").await;
        tokio::time::timeout(std::time::Duration::from_secs(5), self.run)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        self.stop.send(()).unwrap();
        self.server.await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn real_plugin_keeps_focus_and_repeated_login_responsive_then_rejects_late_verified_page() {
    let mut h = Harness::open(true).await;
    h.host.login.delay.store(true, Ordering::Release);
    let old_account = lock(&h.host.account).clone();
    h.send(ProviderOperation::SignIn, 2, "login").await;
    h.host.login.read_started().await;
    h.send(ProviderOperation::FocusSignIn, 2, "focus").await;
    let focus = h.result("focus").await;
    assert_eq!(focus.focused, Some(true));
    assert_eq!(
        focus.status.unwrap().state,
        AccountConnection::SigningIn as i32
    );
    assert_eq!(h.host.login.focuses.load(Ordering::Acquire), 1);
    h.send(ProviderOperation::SignIn, 2, "repeat").await;
    assert_eq!(
        h.result("repeat").await.login_outcome,
        Some(AccountLoginOutcome::Superseded as i32)
    );
    assert_eq!(h.host.login.opens.load(Ordering::Acquire), 1);
    h.send(ProviderOperation::CancelSignIn, 3, "cancel").await;
    assert_eq!(
        h.result("cancel").await.status.unwrap().state,
        AccountConnection::Connected as i32
    );
    assert_eq!(h.host.login.cancels.load(Ordering::Acquire), 1);
    assert_eq!(
        lock(&h.host.credential).as_deref(),
        Some(b"saved-cookie-secret".as_slice())
    );
    h.host.login.release.add_permits(1);
    let late = h.result("login").await;
    assert_eq!(late.disposition, OperationDisposition::Superseded as i32);
    assert_eq!(h.host.login.writes.load(Ordering::Acquire), 0);
    assert_eq!(*lock(&h.host.account), old_account);
    assert!(lock(&h.host.login.handle).is_none());
    assert_eq!(h.host.login.closes.load(Ordering::Acquire), 1);
    h.close().await;
}

#[tokio::test]
async fn real_plugin_requires_both_cookie_and_confirmed_page_before_saving() {
    let mut h = Harness::open(false).await;
    *lock(&h.host.login.page) = Observation::CookieOnly;
    h.send(ProviderOperation::SignIn, 2, "login").await;
    h.host.login.read_started().await;
    h.send(ProviderOperation::FocusSignIn, 2, "cookie-only")
        .await;
    assert_eq!(
        h.result("cookie-only").await.status.unwrap().state,
        AccountConnection::SigningIn as i32
    );
    assert_eq!(h.host.login.writes.load(Ordering::Acquire), 0);
    assert!(lock(&h.host.credential).is_none());
    *lock(&h.host.login.page) = Observation::PageOnly;
    h.host.login.page_read(Observation::PageOnly).await;
    h.send(ProviderOperation::FocusSignIn, 2, "page-only").await;
    assert_eq!(
        h.result("page-only").await.status.unwrap().state,
        AccountConnection::SigningIn as i32
    );
    assert_eq!(h.host.login.writes.load(Ordering::Acquire), 0);
    *lock(&h.host.login.page) = Observation::Verified;
    let logged_in = h.result("login").await;
    assert_eq!(
        logged_in.login_outcome,
        Some(AccountLoginOutcome::Connected as i32)
    );
    assert_eq!(
        logged_in.status.unwrap().state,
        AccountConnection::Connected as i32
    );
    assert_eq!(h.host.login.writes.load(Ordering::Acquire), 1);
    assert_eq!(
        lock(&h.host.credential).as_deref(),
        Some(b"steamLoginSecure=cookie-secret".as_slice())
    );
    assert!(
        !String::from_utf8(lock(&h.host.credential).clone().unwrap())
            .unwrap()
            .contains("temporary-token")
    );
    assert!(lock(&h.host.login.handle).is_none());
    h.close().await;
}

#[tokio::test]
async fn real_plugin_restores_connected_and_disconnected_states_after_close_or_timeout() {
    let mut h = Harness::open(true).await;
    for (epoch, id, page, outcome) in [
        (
            2,
            "closed",
            Observation::Closed,
            AccountLoginOutcome::Closed,
        ),
        (
            3,
            "timeout",
            Observation::TimedOut,
            AccountLoginOutcome::TimedOut,
        ),
    ] {
        *lock(&h.host.login.page) = page;
        h.send(ProviderOperation::SignIn, epoch, id).await;
        let result = h.result(id).await;
        assert_eq!(result.login_outcome, Some(outcome as i32));
        assert_eq!(
            result.status.unwrap().state,
            AccountConnection::Connected as i32
        );
        assert_eq!(h.host.login.writes.load(Ordering::Acquire), 0);
        assert!(lock(&h.host.login.handle).is_none());
    }
    h.send(ProviderOperation::SignOut, 4, "out").await;
    assert!(h.result("out").await.signed_out);
    *lock(&h.host.login.page) = Observation::Closed;
    h.send(ProviderOperation::SignIn, 5, "closed-disconnected")
        .await;
    let result = h.result("closed-disconnected").await;
    assert_eq!(
        result.login_outcome,
        Some(AccountLoginOutcome::Closed as i32)
    );
    assert_eq!(
        result.status.unwrap().state,
        AccountConnection::NotConnected as i32
    );
    assert!(lock(&h.host.account).is_none());
    assert!(lock(&h.host.credential).is_none());
    assert_eq!(h.host.login.opens.load(Ordering::Acquire), 3);
    assert_eq!(h.host.login.closes.load(Ordering::Acquire), 3);
    h.close().await;
}

#[tokio::test]
async fn real_plugin_does_not_resurrect_a_signed_out_account_from_a_late_login_page() {
    let mut h = Harness::open(true).await;
    h.host.login.delay.store(true, Ordering::Release);
    h.send(ProviderOperation::SignIn, 2, "login").await;
    h.host.login.read_started().await;
    h.send(ProviderOperation::SignOut, 3, "out").await;
    let signed_out = h.result("out").await;
    assert!(signed_out.signed_out);
    assert_eq!(
        signed_out.status.unwrap().state,
        AccountConnection::NotConnected as i32
    );
    h.host.login.release.add_permits(1);
    assert_eq!(
        h.result("login").await.disposition,
        OperationDisposition::Superseded as i32
    );
    assert_eq!(h.host.login.writes.load(Ordering::Acquire), 0);
    assert!(lock(&h.host.credential).is_none());
    assert!(lock(&h.host.account).is_none());
    assert!(lock(&h.host.login.handle).is_none());
    h.close().await;
}
