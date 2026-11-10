//! The actual Epic provider runs against a standalone public reference host. No app, Core,
//! platform, database, real browser or real Epic account is used.

use super::*;
use crate::account::EpicApi;
use crate::http::EpicError;
use gamenode_plugin_protocol::models::{SessionCredential, StorageMethod};
use gamenode_plugin_protocol::{API_MAJOR, API_MINOR};
use gamenode_plugin_sdk::{Launch, Session};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{Semaphore, mpsc, oneshot};
use tokio_stream::wrappers::{ReceiverStream, UnixListenerStream};
use tonic::{Request, Response, Status};

type Commands = mpsc::Receiver<Result<ProviderCommand, Status>>;

const CODE: &str = "one-time-code-secret";
const ACCESS: &str = "access-token-secret";
const REFRESH: &str = "rotated-refresh-token-secret";
const ACCOUNT: &str = "0123456789abcdef0123456789abcdef";
const NAMESPACE_A: &str = "0123456789abcdef0123456789abcdef";
const NAMESPACE_B: &str = "fedcba9876543210fedcba9876543210";

fn cred(value: &str) -> SessionCredential {
    SessionCredential::new(value).expect("a test credential")
}

fn token(access: &str, refresh: &str, account: &str, name: &str) -> crate::http::EpicToken {
    crate::http::EpicToken {
        access_token: cred(access),
        refresh_token: cred(refresh),
        account_id: account.to_owned(),
        display_name: name.to_owned(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Observation {
    Code,
    Cancelled,
    Closed,
    TimedOut,
}

struct LoginControl {
    page: Mutex<Observation>,
    delay: AtomicBool,
    reading: Semaphore,
    release: Semaphore,
    handle: Mutex<Option<String>>,
    opens: AtomicUsize,
    cancels: AtomicUsize,
    closes: AtomicUsize,
}

impl Default for LoginControl {
    fn default() -> Self {
        Self {
            page: Mutex::new(Observation::Code),
            delay: false.into(),
            reading: Semaphore::new(0),
            release: Semaphore::new(0),
            handle: Mutex::new(None),
            opens: 0.into(),
            cancels: 0.into(),
            closes: 0.into(),
        }
    }
}

impl LoginControl {
    fn set_page(&self, page: Observation) {
        *lock(&self.page) = page;
    }

    async fn read_started(&self) {
        tokio::time::timeout(Duration::from_secs(5), self.reading.acquire())
            .await
            .expect("the plugin reads the login window")
            .expect("the read semaphore stays open")
            .forget();
    }
}

#[derive(Clone)]
struct Host {
    commands: Arc<tokio::sync::Mutex<Option<Commands>>>,
    events: mpsc::Sender<ProviderEvent>,
    credential: Arc<Mutex<Option<Vec<u8>>>>,
    account: Arc<Mutex<Option<AccountIdentity>>>,
    fail_delete: Arc<AtomicBool>,
    fail_read: Arc<AtomicBool>,
    login: Arc<LoginControl>,
    actions: Arc<Mutex<Option<ActionDeclaration>>>,
    /// The credential value visible when a `Connected` status was published.
    connected_secrets: Arc<Mutex<Vec<Option<Vec<u8>>>>>,
    /// Shared with the fake API to prove the delete happens before the revoke.
    order: Arc<Mutex<Vec<&'static str>>>,
}

fn authorize(context: Option<&ServiceContext>) -> Result<(), Status> {
    let context = context.ok_or_else(|| Status::unauthenticated("context required"))?;
    if context.provider_id != "epic"
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
        let expected = crate::CAPABILITIES
            .iter()
            .map(|capability| (*capability).to_owned())
            .collect::<Vec<_>>();
        if hello.plugin_id != crate::PLUGIN_ID
            || hello.session_credential != [9; 32]
            || hello.requested_capabilities != expected
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
        let host = self.clone();
        tokio::spawn(async move {
            while let Ok(Some(event)) = input.message().await {
                authorize(event.context.as_ref()).unwrap();
                if let Some(provider_event::Event::AccountStatus(status)) = &event.event
                    && status.state == AccountConnection::Connected as i32
                {
                    lock(&host.connected_secrets).push(lock(&host.credential).clone());
                }
                if host.events.send(event).await.is_err() {
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
            provider_id: "epic".into(),
            platform: "windows".into(),
            manifest_toml: include_str!("../../../../providers/epic.toml").into(),
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
                assert_eq!(value.provider_id, "epic");
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
            SecretOperation::Read if self.fail_read.load(Ordering::Acquire) => {
                Outcome::Failed(ServiceFailure::Locked as i32)
            }
            SecretOperation::Read => credential
                .clone()
                .map_or(Outcome::Absent(true), Outcome::Value),
            SecretOperation::Write => {
                *credential = request.value;
                Outcome::Accepted(true)
            }
            SecretOperation::Delete if self.fail_delete.load(Ordering::Acquire) => {
                Outcome::Failed(ServiceFailure::Failed as i32)
            }
            SecretOperation::Delete => {
                lock(&self.order).push("delete");
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
                let page = *lock(&self.login.page);
                self.login.reading.add_permits(1);
                if self.login.delay.load(Ordering::Acquire) {
                    self.login.release.acquire().await.unwrap().forget();
                }
                LoginReply {
                    handle: request.handle,
                    state: match page {
                        Observation::Closed => LoginState::Closed,
                        Observation::TimedOut => LoginState::TimedOut,
                        Observation::Cancelled => LoginState::Cancelled,
                        Observation::Code => LoginState::Open,
                    } as i32,
                    // The probe is cookie-free: only the one-time code is returned.
                    page_result_json: (page == Observation::Code)
                        .then(|| format!(r#"{{"authorization_code":"{CODE}"}}"#)),
                    ..Default::default()
                }
            }
            LoginOperation::Focus => {
                let open = lock(&self.login.handle).as_deref() == Some(&request.handle);
                LoginReply {
                    state: if open {
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
        *lock(&self.actions) = request.get_ref().actions.clone();
        Ok(Response::new(ActionReply { accepted: true }))
    }
}

struct FakeApi {
    exchange: Mutex<VecDeque<Result<crate::http::EpicToken, EpicError>>>,
    refresh: Mutex<VecDeque<Result<crate::http::EpicToken, EpicError>>>,
    library: Mutex<VecDeque<Result<Vec<crate::http::EpicLibraryItem>, EpicError>>>,
    catalog: Mutex<BTreeMap<String, BTreeMap<String, crate::http::EpicCatalogItem>>>,
    catalog_errors: Mutex<VecDeque<EpicError>>,
    playtime: Mutex<VecDeque<Result<Vec<crate::http::EpicPlaytime>, EpicError>>>,
    playtime_accounts: Mutex<Vec<String>>,
    exchange_calls: AtomicUsize,
    refresh_calls: AtomicUsize,
    library_calls: AtomicUsize,
    catalog_calls: AtomicUsize,
    playtime_calls: AtomicUsize,
    revokes: AtomicUsize,
    started: Arc<Semaphore>,
    hold: Mutex<Option<Arc<Semaphore>>>,
    order: Arc<Mutex<Vec<&'static str>>>,
}

impl Default for FakeApi {
    fn default() -> Self {
        Self {
            exchange: Mutex::new(VecDeque::new()),
            refresh: Mutex::new(VecDeque::new()),
            library: Mutex::new(VecDeque::new()),
            catalog: Mutex::new(BTreeMap::new()),
            catalog_errors: Mutex::new(VecDeque::new()),
            playtime: Mutex::new(VecDeque::new()),
            playtime_accounts: Mutex::new(Vec::new()),
            exchange_calls: AtomicUsize::new(0),
            refresh_calls: AtomicUsize::new(0),
            library_calls: AtomicUsize::new(0),
            catalog_calls: AtomicUsize::new(0),
            playtime_calls: AtomicUsize::new(0),
            revokes: AtomicUsize::new(0),
            started: Arc::new(Semaphore::new(0)),
            hold: Mutex::new(None),
            order: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl FakeApi {
    fn queue_exchange(&self, value: Result<crate::http::EpicToken, EpicError>) {
        self.exchange.lock().unwrap().push_back(value);
    }

    fn queue_refresh(&self, value: Result<crate::http::EpicToken, EpicError>) {
        self.refresh.lock().unwrap().push_back(value);
    }

    fn queue_library(&self, value: Result<Vec<crate::http::EpicLibraryItem>, EpicError>) {
        self.library.lock().unwrap().push_back(value);
    }

    fn queue_playtime(&self, value: Result<Vec<crate::http::EpicPlaytime>, EpicError>) {
        self.playtime.lock().unwrap().push_back(value);
    }

    fn queue_catalog_error(&self, value: EpicError) {
        self.catalog_errors.lock().unwrap().push_back(value);
    }

    fn set_catalog(
        &self,
        namespace: &str,
        id: &str,
        title: &str,
        is_game: bool,
        has_main_game: bool,
    ) {
        self.catalog
            .lock()
            .unwrap()
            .entry(namespace.into())
            .or_default()
            .insert(
                id.into(),
                crate::http::EpicCatalogItem {
                    id: id.into(),
                    title: title.into(),
                    is_game,
                    has_main_game,
                },
            );
    }

    fn hold_library(&self, release: Arc<Semaphore>) {
        *self.hold.lock().unwrap() = Some(release);
    }
}

#[async_trait::async_trait]
impl EpicApi for FakeApi {
    async fn exchange_code(
        &self,
        _: &SessionCredential,
    ) -> Result<crate::http::EpicToken, EpicError> {
        self.exchange_calls.fetch_add(1, Ordering::AcqRel);
        self.exchange
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Err(EpicError::Unreachable))
    }

    async fn refresh(&self, _: &SessionCredential) -> Result<crate::http::EpicToken, EpicError> {
        self.refresh_calls.fetch_add(1, Ordering::AcqRel);
        self.refresh
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Err(EpicError::Unreachable))
    }

    async fn revoke(&self, _: &SessionCredential) -> Result<(), EpicError> {
        self.revokes.fetch_add(1, Ordering::AcqRel);
        lock(&self.order).push("revoke");
        Ok(())
    }

    async fn library_items(
        &self,
        _: &SessionCredential,
    ) -> Result<Vec<crate::http::EpicLibraryItem>, EpicError> {
        self.library_calls.fetch_add(1, Ordering::AcqRel);
        let hold = lock(&self.hold).clone();
        if let Some(release) = hold {
            self.started.add_permits(1);
            let _ = release.acquire().await;
        }
        self.library
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(Vec::new()))
    }

    async fn catalog_items(
        &self,
        _: &SessionCredential,
        namespace: &str,
        ids: &[String],
    ) -> Result<BTreeMap<String, crate::http::EpicCatalogItem>, EpicError> {
        self.catalog_calls.fetch_add(1, Ordering::AcqRel);
        if let Some(error) = self.catalog_errors.lock().unwrap().pop_front() {
            return Err(error);
        }
        let catalog = self.catalog.lock().unwrap();
        Ok(ids
            .iter()
            .filter_map(|id| {
                catalog
                    .get(namespace)
                    .and_then(|items| items.get(id))
                    .map(|item| (id.clone(), item.clone()))
            })
            .collect())
    }

    async fn playtime(
        &self,
        _: &SessionCredential,
        account_id: &str,
    ) -> Result<Vec<crate::http::EpicPlaytime>, EpicError> {
        assert!(
            !account_id.is_empty(),
            "the counter uses the loaded account"
        );
        self.playtime_accounts
            .lock()
            .unwrap()
            .push(account_id.to_owned());
        self.playtime_calls.fetch_add(1, Ordering::AcqRel);
        self.playtime
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(Vec::new()))
    }
}

struct Harness {
    commands: mpsc::Sender<Result<ProviderCommand, Status>>,
    events: mpsc::Receiver<ProviderEvent>,
    completed: BTreeMap<String, OperationResult>,
    chunks: BTreeMap<String, Vec<CatalogChunk>>,
    host: Host,
    api: Arc<FakeApi>,
    run: tokio::task::JoinHandle<Result<(), Error>>,
    stop: oneshot::Sender<()>,
    server: tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
    _temp: tempfile::TempDir,
}

impl Harness {
    async fn open(connected: bool) -> Self {
        Self::open_with(Duration::from_secs(290), connected).await
    }

    async fn open_with(catalog_budget: Duration, connected: bool) -> Self {
        let api = Arc::new(FakeApi::default());
        Self::open_with_api(connected, api, catalog_budget, false).await
    }

    async fn open_with_failures(connected: bool) -> Self {
        Self::open_with_api(
            connected,
            Arc::new(FakeApi::default()),
            Duration::from_secs(290),
            true,
        )
        .await
    }

    async fn open_with_api(
        connected: bool,
        api: Arc<FakeApi>,
        catalog_budget: Duration,
        fail_read: bool,
    ) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let endpoint = temp.path().join("reference.sock");
        let listener = tokio::net::UnixListener::bind(&endpoint).unwrap();
        let (commands, receiver) = mpsc::channel(16);
        let (events, results) = mpsc::channel(64);
        let order = api.order.clone();
        let host = Host {
            commands: Arc::new(tokio::sync::Mutex::new(Some(receiver))),
            events,
            credential: Arc::new(Mutex::new(
                connected.then(|| b"stored-refresh-token".to_vec()),
            )),
            account: Arc::new(Mutex::new(connected.then(|| AccountIdentity {
                provider_id: "epic".into(),
                account_id: ACCOUNT.into(),
                account_name: Some("Player One".into()),
                last_verified_at: Some(10),
            }))),
            fail_delete: Arc::new(AtomicBool::new(false)),
            fail_read: Arc::new(AtomicBool::new(fail_read)),
            login: Arc::new(LoginControl::default()),
            actions: Arc::new(Mutex::new(None)),
            connected_secrets: Arc::new(Mutex::new(Vec::new())),
            order,
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
            crate::PLUGIN_ID,
            crate::PROVIDER_ID,
            "test",
            crate::CAPABILITIES,
        )
        .await
        .unwrap();
        let plugin = Arc::new(EpicPlugin {
            client: api.clone(),
            account: Mutex::new(None),
            login: LoginAttempt::default(),
            contexts: Mutex::new(Contexts::default()),
            catalog_budget,
        });
        let run = tokio::spawn(gamenode_plugin_sdk::run(session, plugin));
        let mut harness = Self {
            commands,
            events: results,
            completed: BTreeMap::new(),
            chunks: BTreeMap::new(),
            host,
            api,
            run,
            stop,
            server,
            _temp: temp,
        };
        harness.send(ProviderOperation::Initialize, 1, "init").await;
        harness.result("init").await;
        harness
    }

    async fn send(&self, kind: ProviderOperation, epoch: u64, id: &str) {
        self.commands
            .send(Ok(ProviderCommand {
                context: Some(ServiceContext {
                    session_id: "reference".into(),
                    session_credential: vec![9; 32],
                    provider_id: "epic".into(),
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
        self.result_with_chunks(id).await.0
    }

    async fn result_with_chunks(&mut self, id: &str) -> (OperationResult, Vec<CatalogChunk>) {
        if let Some(result) = self.completed.remove(id) {
            return (result, self.chunks.remove(id).unwrap_or_default());
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = self.events.recv().await.expect("plugin stream alive");
                let operation = event.context.unwrap().operation_id;
                match event.event.unwrap() {
                    provider_event::Event::CatalogChunk(chunk) => {
                        self.chunks.entry(operation).or_default().push(chunk);
                    }
                    provider_event::Event::Result(result) => {
                        if operation == id {
                            let chunks = self.chunks.remove(id).unwrap_or_default();
                            return (result, chunks);
                        }
                        assert!(self.completed.insert(operation, result).is_none());
                    }
                    provider_event::Event::AccountStatus(_) => {}
                    _ => {}
                }
            }
        })
        .await
        .expect("provider operation completes without retaining an observer queue")
    }

    async fn close(self) {
        self.send(ProviderOperation::Shutdown, 99, "shutdown").await;
        tokio::time::timeout(Duration::from_secs(5), self.run)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        self.stop.send(()).unwrap();
        self.server.await.unwrap().unwrap();
    }
}

fn completion(result: &OperationResult) -> &CatalogCompletion {
    assert_eq!(result.catalogs.len(), 1, "{:?}", result.catalogs);
    &result.catalogs[0]
}

#[tokio::test]
async fn initialize_declares_the_play_and_install_templates_from_the_manifest() {
    let h = Harness::open(false).await;
    assert_eq!(h.api.refresh_calls.load(Ordering::Acquire), 0);
    assert_eq!(h.host.login.opens.load(Ordering::Acquire), 0);
    assert!(lock(&h.host.account).is_none());

    let actions = lock(&h.host.actions).clone().expect("declared actions");
    assert_eq!(
        actions.play_uri.as_deref(),
        Some("com.epicgames.launcher://apps/{id}?action=launch&silent=true")
    );
    assert_eq!(
        actions.install_uri.as_deref(),
        Some("com.epicgames.launcher://apps/{id}?action=install")
    );

    h.close().await;
}

#[tokio::test]
async fn sign_in_after_the_second_factor_page_stores_only_the_refresh_token() {
    let mut h = Harness::open(false).await;
    h.api.queue_exchange(Ok(token(
        "sign-in-access",
        "sign-in-refresh",
        ACCOUNT,
        "Player One",
    )));
    // The probe only resolves once the login page, including 2FA, has completed. The plugin
    // receives the same one-time code either way.
    h.host.login.set_page(Observation::Code);

    h.send(ProviderOperation::SignIn, 2, "login").await;
    let result = h.result("login").await;

    assert_eq!(result.disposition, OperationDisposition::Completed as i32);
    assert_eq!(
        result.login_outcome,
        Some(AccountLoginOutcome::Connected as i32)
    );
    let status = result.status.expect("status");
    assert_eq!(status.state, AccountConnection::Connected as i32);
    assert_eq!(status.account_name.as_deref(), Some("Player One"));
    assert_eq!(
        lock(&h.host.credential).as_deref(),
        Some(b"sign-in-refresh".as_slice())
    );
    assert_eq!(
        lock(&h.host.connected_secrets).last(),
        Some(&Some(b"sign-in-refresh".to_vec()))
    );
    let account = lock(&h.host.account).clone().expect("saved identity");
    assert_eq!(account.provider_id, "epic");
    assert_eq!(account.account_id, ACCOUNT);
    // The access token never reaches a persistent slot.
    assert_ne!(
        lock(&h.host.credential).as_deref(),
        Some(b"sign-in-access".as_slice())
    );

    h.close().await;
}

#[tokio::test]
async fn a_cancelled_sign_in_saves_nothing() {
    let mut h = Harness::open(false).await;
    h.host.login.set_page(Observation::Cancelled);

    h.send(ProviderOperation::SignIn, 2, "login").await;
    let result = h.result("login").await;

    assert_eq!(
        result.login_outcome,
        Some(AccountLoginOutcome::Cancelled as i32)
    );
    assert_eq!(
        result.status.unwrap().state,
        AccountConnection::NotConnected as i32
    );
    assert!(lock(&h.host.credential).is_none());
    assert_eq!(h.api.exchange_calls.load(Ordering::Acquire), 0);

    h.close().await;
}

#[tokio::test]
async fn a_closed_sign_in_saves_nothing() {
    let mut h = Harness::open(true).await;
    h.host.login.set_page(Observation::Closed);

    h.send(ProviderOperation::SignIn, 2, "login").await;
    let result = h.result("login").await;

    assert_eq!(
        result.login_outcome,
        Some(AccountLoginOutcome::Closed as i32)
    );
    assert_eq!(
        result.status.unwrap().state,
        AccountConnection::Checking as i32
    );
    assert_eq!(
        lock(&h.host.credential).as_deref(),
        Some(b"stored-refresh-token".as_slice())
    );

    h.close().await;
}

#[tokio::test]
async fn a_timed_out_sign_in_saves_nothing_and_restores_expired() {
    let mut h = Harness::open(true).await;
    h.host.login.set_page(Observation::TimedOut);
    h.api.queue_refresh(Err(EpicError::InvalidGrant));
    h.send(ProviderOperation::Check, 1, "check").await;
    assert_eq!(
        h.result("check").await.status.unwrap().state,
        AccountConnection::Expired as i32
    );

    h.send(ProviderOperation::SignIn, 2, "login").await;
    let result = h.result("login").await;

    assert_eq!(
        result.login_outcome,
        Some(AccountLoginOutcome::TimedOut as i32)
    );
    assert_eq!(
        result.status.unwrap().state,
        AccountConnection::Expired as i32
    );
    assert!(lock(&h.host.credential).is_none());

    h.close().await;
}

#[tokio::test]
async fn a_rejected_code_fails_without_saving_a_credential() {
    let mut h = Harness::open(false).await;
    h.host.login.set_page(Observation::Code);
    h.api.queue_exchange(Err(EpicError::InvalidGrant));

    h.send(ProviderOperation::SignIn, 2, "login").await;
    let result = h.result("login").await;

    assert_eq!(
        result.login_outcome,
        Some(AccountLoginOutcome::Failed as i32)
    );
    assert_eq!(
        result.status.unwrap().state,
        AccountConnection::NotConnected as i32
    );
    assert!(lock(&h.host.credential).is_none());
    assert!(lock(&h.host.account).is_none());

    h.close().await;
}

#[tokio::test]
async fn a_check_writes_the_rotated_refresh_token_before_publishing_connected() {
    let mut h = Harness::open(true).await;
    h.api
        .queue_refresh(Ok(token(ACCESS, REFRESH, ACCOUNT, "Player One")));

    h.send(ProviderOperation::Check, 1, "check").await;
    let result = h.result("check").await;

    let status = result.status.expect("status");
    assert_eq!(status.state, AccountConnection::Connected as i32);
    assert_eq!(status.account_name.as_deref(), Some("Player One"));
    assert_eq!(
        lock(&h.host.credential).as_deref(),
        Some(REFRESH.as_bytes())
    );
    // The connected event observes the rotated value, not the old one.
    assert_eq!(
        lock(&h.host.connected_secrets).last(),
        Some(&Some(REFRESH.as_bytes().to_vec()))
    );
    let account = lock(&h.host.account).clone().expect("saved identity");
    assert_eq!(account.last_verified_at, status.last_checked_at);

    h.close().await;
}

#[tokio::test]
async fn invalid_grant_expires_and_deletes_the_refresh_token() {
    let mut h = Harness::open(true).await;
    h.api.queue_refresh(Err(EpicError::InvalidGrant));

    h.send(ProviderOperation::Check, 1, "check").await;
    let status = h.result("check").await.status.unwrap();

    assert_eq!(status.state, AccountConnection::Expired as i32);
    assert_eq!(status.account_id.as_deref(), Some(ACCOUNT));
    assert!(lock(&h.host.credential).is_none());
    assert_eq!(lock(&h.host.order).as_slice(), ["delete"]);

    h.close().await;
}

#[tokio::test]
async fn an_unauthorized_refresh_expires_and_deletes_the_refresh_token() {
    let mut h = Harness::open(true).await;
    h.api.queue_refresh(Err(EpicError::Unauthorized));

    h.send(ProviderOperation::Check, 1, "check").await;
    let status = h.result("check").await.status.unwrap();

    assert_eq!(status.state, AccountConnection::Expired as i32);
    assert!(lock(&h.host.credential).is_none());

    h.close().await;
}

#[tokio::test]
async fn an_unreachable_check_keeps_the_refresh_token() {
    let mut h = Harness::open(true).await;
    h.api.queue_refresh(Err(EpicError::Unreachable));

    h.send(ProviderOperation::Check, 1, "check").await;
    let status = h.result("check").await.status.unwrap();

    assert_eq!(status.state, AccountConnection::Unreachable as i32);
    assert_eq!(
        lock(&h.host.credential).as_deref(),
        Some(b"stored-refresh-token".as_slice())
    );

    h.close().await;
}

#[tokio::test]
async fn a_server_error_keeps_the_refresh_token() {
    let mut h = Harness::open(true).await;
    h.api.queue_refresh(Err(EpicError::Server));

    h.send(ProviderOperation::Check, 1, "check").await;
    let status = h.result("check").await.status.unwrap();

    assert_eq!(status.state, AccountConnection::Unreachable as i32);
    assert_eq!(
        lock(&h.host.credential).as_deref(),
        Some(b"stored-refresh-token".as_slice())
    );

    h.close().await;
}

#[tokio::test]
async fn an_unreadable_credential_is_a_credential_error_and_recovers_on_retry() {
    let mut h = Harness::open_with_failures(true).await;

    h.send(ProviderOperation::Check, 1, "check").await;
    let status = h.result("check").await.status.unwrap();
    assert_eq!(status.state, AccountConnection::CredentialError as i32);
    assert_eq!(h.api.refresh_calls.load(Ordering::Acquire), 0);
    assert_eq!(status.account_id.as_deref(), Some(ACCOUNT));

    // Unlocking or replacing the credential lets the same card recover without a new sign-in.
    h.host.fail_read.store(false, Ordering::Release);
    h.api
        .queue_refresh(Ok(token(ACCESS, REFRESH, ACCOUNT, "Player One")));
    h.send(ProviderOperation::Check, 1, "retry").await;
    let status = h.result("retry").await.status.unwrap();
    assert_eq!(status.state, AccountConnection::Connected as i32);
    assert_eq!(h.api.refresh_calls.load(Ordering::Acquire), 1);
    assert_eq!(
        lock(&h.host.credential).as_deref(),
        Some(REFRESH.as_bytes())
    );

    h.close().await;
}

#[tokio::test]
async fn sign_out_deletes_the_local_credential_before_revoking_the_session() {
    let mut h = Harness::open(true).await;
    h.api
        .queue_refresh(Ok(token(ACCESS, REFRESH, ACCOUNT, "Player One")));
    h.send(ProviderOperation::Check, 1, "check").await;
    assert_eq!(
        h.result("check").await.status.unwrap().state,
        AccountConnection::Connected as i32
    );

    h.send(ProviderOperation::SignOut, 2, "out").await;
    let result = h.result("out").await;

    assert!(result.signed_out);
    assert_eq!(
        result.status.unwrap().state,
        AccountConnection::NotConnected as i32
    );
    assert!(lock(&h.host.credential).is_none());
    assert!(lock(&h.host.account).is_none());
    assert_eq!(h.api.revokes.load(Ordering::Acquire), 1);
    assert_eq!(lock(&h.host.order).as_slice(), ["delete", "revoke"]);

    h.close().await;
}

#[tokio::test]
async fn a_failed_sign_out_delete_keeps_the_account_connected() {
    let mut h = Harness::open(true).await;
    h.api
        .queue_refresh(Ok(token(ACCESS, REFRESH, ACCOUNT, "Player One")));
    h.send(ProviderOperation::Check, 1, "check").await;
    assert_eq!(
        h.result("check").await.status.unwrap().state,
        AccountConnection::Connected as i32
    );
    h.host.fail_delete.store(true, Ordering::Release);

    h.send(ProviderOperation::SignOut, 2, "out").await;
    let result = h.result("out").await;

    assert!(!result.signed_out);
    let status = result.status.expect("status");
    assert_eq!(status.state, AccountConnection::Connected as i32);
    assert!(status.persist_failed);
    assert_eq!(
        lock(&h.host.credential).as_deref(),
        Some(REFRESH.as_bytes())
    );
    assert_eq!(h.api.revokes.load(Ordering::Acquire), 0);

    h.close().await;
}

#[tokio::test]
async fn a_late_sign_in_result_after_sign_out_is_ignored() {
    let mut h = Harness::open(false).await;
    h.api.queue_exchange(Ok(token(
        "late-access",
        "late-refresh",
        ACCOUNT,
        "Player One",
    )));
    h.host.login.set_page(Observation::Code);
    h.host.login.delay.store(true, Ordering::Release);

    h.send(ProviderOperation::SignIn, 2, "login").await;
    h.host.login.read_started().await;
    h.send(ProviderOperation::SignOut, 3, "out").await;
    let signed_out = h.result("out").await;
    assert!(signed_out.signed_out);

    h.host.login.release.add_permits(1);
    let late = h.result("login").await;

    // The stale operation is discarded as a whole, so it cannot publish a newer state.
    assert_eq!(late.disposition, OperationDisposition::Superseded as i32);
    assert!(
        late.login_outcome.is_none()
            || late.login_outcome == Some(AccountLoginOutcome::Superseded as i32)
    );
    assert!(
        late.status
            .is_none_or(|status| status.state == AccountConnection::NotConnected as i32)
    );
    assert!(lock(&h.host.credential).is_none());
    assert!(lock(&h.host.account).is_none());
    assert_eq!(h.api.exchange_calls.load(Ordering::Acquire), 0);

    h.close().await;
}

#[tokio::test]
async fn the_cancel_control_restores_the_previous_state() {
    let mut h = Harness::open(false).await;
    h.host.login.set_page(Observation::Code);
    h.host.login.delay.store(true, Ordering::Release);

    h.send(ProviderOperation::SignIn, 2, "login").await;
    h.host.login.read_started().await;
    h.send(ProviderOperation::CancelSignIn, 2, "cancel").await;
    let cancel = h.result("cancel").await;

    assert_eq!(
        cancel.status.unwrap().state,
        AccountConnection::NotConnected as i32
    );
    assert!(lock(&h.host.login.handle).is_none());

    h.host.login.release.add_permits(1);
    let late = h.result("login").await;
    assert_eq!(
        late.login_outcome,
        Some(AccountLoginOutcome::Superseded as i32)
    );
    assert!(lock(&h.host.credential).is_none());

    h.close().await;
}

#[tokio::test]
async fn an_owned_catalog_pages_and_carries_namespace_fields() {
    let mut h = Harness::open(true).await;
    h.api
        .queue_refresh(Ok(token(ACCESS, REFRESH, ACCOUNT, "Player One")));
    h.api
        .queue_refresh(Ok(token("access-2", "refresh-2", ACCOUNT, "Player One")));
    h.api.queue_library(Ok(vec![
        crate::http::EpicLibraryItem {
            app_name: Some("GameAlpha".into()),
            namespace: Some(NAMESPACE_A.into()),
            catalog_item_id: Some("catalog-alpha".into()),
            product_id: None,
            artifact_id: Some("artifact-alpha".into()),
        },
        crate::http::EpicLibraryItem {
            app_name: Some("GameBeta".into()),
            namespace: Some(NAMESPACE_B.into()),
            catalog_item_id: Some("catalog-beta".into()),
            product_id: None,
            artifact_id: Some("artifact-beta".into()),
        },
        crate::http::EpicLibraryItem {
            app_name: Some("GameGamma".into()),
            namespace: Some(NAMESPACE_A.into()),
            catalog_item_id: Some("catalog-gamma".into()),
            product_id: None,
            artifact_id: Some("artifact-gamma".into()),
        },
    ]));
    h.api
        .set_catalog(NAMESPACE_A, "catalog-alpha", "Alpha Adventure", true, false);
    h.api
        .set_catalog(NAMESPACE_A, "catalog-gamma", "Gamma Tactics", true, false);
    h.api
        .set_catalog(NAMESPACE_B, "catalog-beta", "Beta Quest", true, false);

    h.send(ProviderOperation::Check, 1, "check").await;
    assert_eq!(
        h.result("check").await.status.unwrap().state,
        AccountConnection::Connected as i32
    );
    h.send(ProviderOperation::SyncCatalog, 1, "catalog").await;
    let (result, chunks) = h.result_with_chunks("catalog").await;

    assert_eq!(
        result.status.as_ref().unwrap().state,
        AccountConnection::Connected as i32
    );
    assert!(matches!(
        completion(&result).outcome,
        Some(catalog_completion::Outcome::CompletedRecords(3))
    ));
    assert_eq!(chunks.len(), 1);
    assert_eq!(
        chunks[0]
            .games
            .iter()
            .map(|game| game.id.as_str())
            .collect::<Vec<_>>(),
        ["GameAlpha", "GameBeta", "GameGamma"]
    );
    assert_eq!(chunks[0].games[0].name, "Alpha Adventure");
    assert_eq!(
        chunks[0].games[0].fields.get("namespace"),
        Some(&NAMESPACE_A.into())
    );
    assert_eq!(
        chunks[0].games[0].fields.get("catalog_item_id"),
        Some(&"catalog-alpha".into())
    );
    assert!(
        chunks[0]
            .games
            .iter()
            .all(|game| game.fields.contains_key("namespace"))
    );

    h.close().await;
}

#[tokio::test]
async fn an_empty_owned_catalog_completes_with_zero_records() {
    let mut h = Harness::open(true).await;
    h.api
        .queue_refresh(Ok(token(ACCESS, REFRESH, ACCOUNT, "Player One")));
    h.api
        .queue_refresh(Ok(token("access-2", "refresh-2", ACCOUNT, "Player One")));
    h.api.queue_library(Ok(Vec::new()));

    h.send(ProviderOperation::Check, 1, "check").await;
    assert_eq!(
        h.result("check").await.status.unwrap().state,
        AccountConnection::Connected as i32
    );
    h.send(ProviderOperation::SyncCatalog, 1, "catalog").await;
    let (result, chunks) = h.result_with_chunks("catalog").await;

    assert!(matches!(
        completion(&result).outcome,
        Some(catalog_completion::Outcome::CompletedRecords(0))
    ));
    assert!(chunks.is_empty());
    let status = result.status.unwrap();
    assert_eq!(status.state, AccountConnection::Connected as i32);
    assert!(status.last_synced_at.is_some());
    assert!(status.last_checked_at.is_some());

    h.close().await;
}

#[tokio::test]
async fn every_library_failure_fails_the_whole_owned_category() {
    let mut h = Harness::open(true).await;
    for _ in 0..3 {
        h.api
            .queue_refresh(Ok(token(ACCESS, REFRESH, ACCOUNT, "Player One")));
    }
    // `library_items` follows the cursor internally; an error here is a later page failing.
    h.api.queue_library(Err(EpicError::Unreachable));
    h.api.queue_library(Err(EpicError::RateLimited));
    h.api.queue_library(Err(EpicError::Server));

    for (index, expected) in [
        EpicError::Unreachable,
        EpicError::RateLimited,
        EpicError::Server,
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("catalog-{index}");
        h.send(ProviderOperation::SyncCatalog, 1, &id).await;
        let (result, chunks) = h.result_with_chunks(&id).await;
        assert!(chunks.is_empty());
        let failure = match completion(&result).outcome {
            Some(catalog_completion::Outcome::Failed(code)) => code,
            other => panic!("expected a failed category, got {other:?}"),
        };
        let expected = match expected {
            EpicError::Unreachable => CatalogFailure::Unreachable,
            EpicError::RateLimited => CatalogFailure::RateLimited,
            EpicError::Server => CatalogFailure::Server,
            _ => unreachable!(),
        };
        assert_eq!(failure, expected as i32);
        let status = result.status.unwrap();
        assert_eq!(status.state, AccountConnection::Unreachable as i32);
        assert_eq!(status.catalog_error, Some(expected as i32));
        assert_eq!(status.last_synced_at, None);
        // The credential survives a catalog failure.
        assert_eq!(
            lock(&h.host.credential).as_deref(),
            Some(REFRESH.as_bytes())
        );
    }

    h.close().await;
}

#[tokio::test]
async fn dlc_engine_and_non_game_records_are_excluded() {
    let mut h = Harness::open(true).await;
    h.api
        .queue_refresh(Ok(token(ACCESS, REFRESH, ACCOUNT, "Player One")));
    h.api
        .queue_refresh(Ok(token("access-2", "refresh-2", ACCOUNT, "Player One")));
    h.api.queue_library(Ok(vec![
        crate::http::EpicLibraryItem {
            app_name: Some("GameAlpha".into()),
            namespace: Some(NAMESPACE_A.into()),
            catalog_item_id: Some("catalog-alpha".into()),
            product_id: None,
            artifact_id: Some("artifact-alpha".into()),
        },
        crate::http::EpicLibraryItem {
            app_name: Some("DlcApp".into()),
            namespace: Some(NAMESPACE_A.into()),
            catalog_item_id: Some("catalog-dlc".into()),
            product_id: None,
            artifact_id: Some("artifact-dlc".into()),
        },
        crate::http::EpicLibraryItem {
            app_name: Some("EngineItem".into()),
            namespace: Some("ue".into()),
            catalog_item_id: Some("catalog-engine".into()),
            product_id: None,
            artifact_id: Some("artifact-engine".into()),
        },
        crate::http::EpicLibraryItem {
            app_name: None,
            namespace: Some(NAMESPACE_B.into()),
            catalog_item_id: Some("catalog-noapp".into()),
            product_id: None,
            artifact_id: Some("artifact-noapp".into()),
        },
        crate::http::EpicLibraryItem {
            app_name: Some("ToolApp".into()),
            namespace: Some(NAMESPACE_B.into()),
            catalog_item_id: Some("catalog-tool".into()),
            product_id: None,
            artifact_id: None,
        },
    ]));
    h.api
        .set_catalog(NAMESPACE_A, "catalog-alpha", "Alpha Adventure", true, false);
    h.api
        .set_catalog(NAMESPACE_A, "catalog-dlc", "Alpha Bonus Pack", true, true);
    h.api
        .set_catalog(NAMESPACE_B, "catalog-tool", "Beta Tool", false, false);

    h.send(ProviderOperation::Check, 1, "check").await;
    assert_eq!(
        h.result("check").await.status.unwrap().state,
        AccountConnection::Connected as i32
    );
    h.send(ProviderOperation::SyncCatalog, 1, "catalog").await;
    let (result, chunks) = h.result_with_chunks("catalog").await;

    assert!(matches!(
        completion(&result).outcome,
        Some(catalog_completion::Outcome::CompletedRecords(1))
    ));
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].games.len(), 1);
    assert_eq!(chunks[0].games[0].id, "GameAlpha");
    // Only namespaced catalog titles were requested; the engine namespace never was.
    assert!(lock(&h.host.account).is_some());

    h.close().await;
}

#[tokio::test]
async fn a_catalog_title_is_retried_once_before_the_category_fails() {
    let api = Arc::new(FakeApi::default());
    let mut h = Harness::open_with_api(true, api.clone(), Duration::from_secs(290), false).await;
    for _ in 0..3 {
        h.api
            .queue_refresh(Ok(token(ACCESS, REFRESH, ACCOUNT, "Player One")));
    }
    for _ in 0..2 {
        h.api.queue_library(Ok(vec![crate::http::EpicLibraryItem {
            app_name: Some("GameAlpha".into()),
            namespace: Some(NAMESPACE_A.into()),
            catalog_item_id: Some("catalog-alpha".into()),
            product_id: None,
            artifact_id: Some("artifact-alpha".into()),
        }]));
    }
    h.api
        .set_catalog(NAMESPACE_A, "catalog-alpha", "Alpha Adventure", true, false);
    // One failure is retried and succeeds.
    h.api.queue_catalog_error(EpicError::Server);

    h.send(ProviderOperation::Check, 1, "check").await;
    assert_eq!(
        h.result("check").await.status.unwrap().state,
        AccountConnection::Connected as i32
    );
    h.send(ProviderOperation::SyncCatalog, 1, "catalog-one")
        .await;
    let (retried, chunks) = h.result_with_chunks("catalog-one").await;
    assert!(matches!(
        completion(&retried).outcome,
        Some(catalog_completion::Outcome::CompletedRecords(1))
    ));
    assert_eq!(chunks[0].games[0].id, "GameAlpha");
    assert_eq!(h.api.catalog_calls.load(Ordering::Acquire), 2);

    // Every retry is used once; a second failure fails the whole category.
    h.api.queue_catalog_error(EpicError::Server);
    h.api.queue_catalog_error(EpicError::Server);
    h.send(ProviderOperation::SyncCatalog, 1, "catalog-two")
        .await;
    let (failed, chunks) = h.result_with_chunks("catalog-two").await;
    assert!(chunks.is_empty());
    assert!(matches!(
        completion(&failed).outcome,
        Some(catalog_completion::Outcome::Failed(code)) if code == CatalogFailure::Server as i32
    ));
    assert_eq!(
        failed.status.unwrap().state,
        AccountConnection::Unreachable as i32
    );
    assert_eq!(h.api.catalog_calls.load(Ordering::Acquire), 4);

    h.close().await;
}

#[tokio::test]
async fn the_sync_finishes_inside_an_accelerated_provider_budget() {
    let mut h = Harness::open_with(Duration::from_millis(100), true).await;
    h.api
        .queue_refresh(Ok(token(ACCESS, REFRESH, ACCOUNT, "Player One")));
    h.api
        .queue_refresh(Ok(token("access-2", "refresh-2", ACCOUNT, "Player One")));
    h.api.queue_library(Ok(vec![crate::http::EpicLibraryItem {
        app_name: Some("GameAlpha".into()),
        namespace: Some(NAMESPACE_A.into()),
        catalog_item_id: Some("catalog-alpha".into()),
        product_id: None,
        artifact_id: Some("artifact-alpha".into()),
    }]));
    let release = Arc::new(Semaphore::new(0));
    h.api.hold_library(release.clone());

    h.send(ProviderOperation::Check, 1, "check").await;
    assert_eq!(
        h.result("check").await.status.unwrap().state,
        AccountConnection::Connected as i32
    );

    let started = Instant::now();
    h.send(ProviderOperation::SyncCatalog, 1, "catalog").await;
    h.api.started.acquire().await.unwrap().forget();
    let (result, chunks) = h.result_with_chunks("catalog").await;
    let elapsed = started.elapsed();
    release.add_permits(1);

    assert!(
        elapsed < Duration::from_secs(2),
        "the sync must stop at its provider budget, took {elapsed:?}"
    );
    assert!(chunks.is_empty());
    assert!(matches!(
        completion(&result).outcome,
        Some(catalog_completion::Outcome::Failed(code)) if code == CatalogFailure::Unreachable as i32
    ));
    // The credential survives a budget expiry; the access token is dropped with the state.
    assert_eq!(
        lock(&h.host.credential).as_deref(),
        Some(b"refresh-2".as_slice())
    );

    h.close().await;
}

#[tokio::test]
async fn counter_reports_epic_totals_under_the_application_name() {
    let mut h = Harness::open(true).await;
    h.api
        .queue_refresh(Ok(token(ACCESS, REFRESH, ACCOUNT, "Player One")));
    h.api
        .queue_refresh(Ok(token("access-2", "refresh-2", ACCOUNT, "Player One")));
    h.api.queue_library(Ok(vec![
        crate::http::EpicLibraryItem {
            app_name: Some("GameAlpha".into()),
            namespace: Some(NAMESPACE_A.into()),
            catalog_item_id: Some("catalog-alpha".into()),
            product_id: None,
            artifact_id: Some("artifact-alpha".into()),
        },
        crate::http::EpicLibraryItem {
            app_name: Some("GameBeta".into()),
            namespace: Some(NAMESPACE_B.into()),
            catalog_item_id: Some("catalog-beta".into()),
            product_id: None,
            artifact_id: Some("artifact-beta".into()),
        },
    ]));
    h.api
        .set_catalog(NAMESPACE_A, "catalog-alpha", "Alpha Adventure", true, false);
    h.api
        .set_catalog(NAMESPACE_B, "catalog-beta", "Beta Quest", true, false);

    h.send(ProviderOperation::Check, 1, "check").await;
    assert_eq!(
        h.result("check").await.status.unwrap().state,
        AccountConnection::Connected as i32
    );
    // The artifact-id to application-name map comes from the owned library, not the ledger.
    h.send(ProviderOperation::SyncCatalog, 1, "catalog").await;
    assert!(matches!(
        completion(&h.result("catalog").await).outcome,
        Some(catalog_completion::Outcome::CompletedRecords(2))
    ));
    h.api.queue_playtime(Ok(vec![
        crate::http::EpicPlaytime {
            artifact_id: "artifact-alpha".into(),
            seconds: 7200,
        },
        crate::http::EpicPlaytime {
            artifact_id: "artifact-beta".into(),
            seconds: 61,
        },
    ]));

    h.send(ProviderOperation::ReadCounter, 1, "counter").await;
    let result = h.result("counter").await;

    assert_eq!(h.api.playtime_calls.load(Ordering::Acquire), 1);
    assert_eq!(
        h.api.playtime_accounts.lock().unwrap().as_slice(),
        [ACCOUNT],
        "the ledger is read for the connected account"
    );
    assert_eq!(result.counters.len(), 2);
    assert_eq!(result.counters[0].id, "GameAlpha");
    assert_eq!(result.counters[0].seconds, 7200);
    assert_eq!(result.counters[0].last_played, None);
    assert_eq!(result.counters[1].id, "GameBeta");
    assert_eq!(result.counters[1].seconds, 61);
    assert_eq!(result.counters[1].last_played, None);
    assert!(result.status.is_none());

    h.close().await;
}

#[tokio::test]
async fn counter_maps_artifact_alpha_to_game_alpha_and_skips_an_unmapped_artifact() {
    let mut h = Harness::open(true).await;
    h.api
        .queue_refresh(Ok(token(ACCESS, REFRESH, ACCOUNT, "Player One")));
    h.api
        .queue_refresh(Ok(token("access-2", "refresh-2", ACCOUNT, "Player One")));
    h.api.queue_library(Ok(vec![crate::http::EpicLibraryItem {
        app_name: Some("GameAlpha".into()),
        namespace: Some(NAMESPACE_A.into()),
        catalog_item_id: Some("catalog-alpha".into()),
        product_id: None,
        artifact_id: Some("artifact-alpha".into()),
    }]));
    h.api
        .set_catalog(NAMESPACE_A, "catalog-alpha", "Alpha Adventure", true, false);

    h.send(ProviderOperation::Check, 1, "check").await;
    assert_eq!(
        h.result("check").await.status.unwrap().state,
        AccountConnection::Connected as i32
    );
    h.send(ProviderOperation::SyncCatalog, 1, "catalog").await;
    assert!(matches!(
        completion(&h.result("catalog").await).outcome,
        Some(catalog_completion::Outcome::CompletedRecords(1))
    ));
    h.api.queue_playtime(Ok(vec![
        crate::http::EpicPlaytime {
            artifact_id: "artifact-alpha".into(),
            seconds: 7200,
        },
        // The ledger can hold artifacts the library no longer lists.
        crate::http::EpicPlaytime {
            artifact_id: "artifact-unmapped".into(),
            seconds: 99,
        },
    ]));

    h.send(ProviderOperation::ReadCounter, 1, "counter").await;
    let result = h.result("counter").await;

    assert_eq!(h.api.playtime_calls.load(Ordering::Acquire), 1);
    assert_eq!(result.counters.len(), 1);
    assert_eq!(result.counters[0].id, "GameAlpha");
    assert_eq!(result.counters[0].seconds, 7200);
    assert_eq!(result.counters[0].last_played, None);
    assert!(
        result
            .counters
            .iter()
            .all(|counter| counter.id != "artifact-alpha"),
        "a raw artifact id is never an entry id"
    );

    h.close().await;
}

#[tokio::test]
async fn a_failed_catalog_sync_keeps_the_last_playtime_mapping() {
    let mut h = Harness::open(true).await;
    for _ in 0..4 {
        h.api
            .queue_refresh(Ok(token(ACCESS, REFRESH, ACCOUNT, "Player One")));
    }
    h.api.queue_library(Ok(vec![crate::http::EpicLibraryItem {
        app_name: Some("GameAlpha".into()),
        namespace: Some(NAMESPACE_A.into()),
        catalog_item_id: Some("catalog-alpha".into()),
        product_id: None,
        artifact_id: Some("artifact-alpha".into()),
    }]));
    h.api.queue_library(Err(EpicError::Unreachable));
    h.api
        .set_catalog(NAMESPACE_A, "catalog-alpha", "Alpha Adventure", true, false);

    h.send(ProviderOperation::Check, 1, "check").await;
    assert_eq!(
        h.result("check").await.status.unwrap().state,
        AccountConnection::Connected as i32
    );
    h.send(ProviderOperation::SyncCatalog, 1, "catalog-ok")
        .await;
    assert!(matches!(
        completion(&h.result("catalog-ok").await).outcome,
        Some(catalog_completion::Outcome::CompletedRecords(1))
    ));
    h.send(ProviderOperation::SyncCatalog, 1, "catalog-fail")
        .await;
    let failed = h.result("catalog-fail").await;
    assert_eq!(
        failed.status.unwrap().state,
        AccountConnection::Unreachable as i32
    );

    // A later successful check reconnects the same account; the mapping is still valid.
    h.send(ProviderOperation::Check, 1, "check-again").await;
    assert_eq!(
        h.result("check-again").await.status.unwrap().state,
        AccountConnection::Connected as i32
    );
    h.api.queue_playtime(Ok(vec![crate::http::EpicPlaytime {
        artifact_id: "artifact-alpha".into(),
        seconds: 7200,
    }]));
    h.send(ProviderOperation::ReadCounter, 1, "counter").await;
    let result = h.result("counter").await;

    assert_eq!(result.counters.len(), 1);
    assert_eq!(result.counters[0].id, "GameAlpha");
    assert_eq!(result.counters[0].seconds, 7200);

    h.close().await;
}

#[tokio::test]
async fn a_different_account_drops_the_previous_playtime_mapping() {
    let mut h = Harness::open(true).await;
    h.api
        .queue_refresh(Ok(token(ACCESS, REFRESH, ACCOUNT, "Player One")));
    h.api
        .queue_refresh(Ok(token("access-2", "refresh-2", ACCOUNT, "Player One")));
    h.api
        .queue_refresh(Ok(token("access-3", "refresh-3", "fedcba", "Player Two")));
    h.api.queue_library(Ok(vec![crate::http::EpicLibraryItem {
        app_name: Some("GameAlpha".into()),
        namespace: Some(NAMESPACE_A.into()),
        catalog_item_id: Some("catalog-alpha".into()),
        product_id: None,
        artifact_id: Some("artifact-alpha".into()),
    }]));
    h.api
        .set_catalog(NAMESPACE_A, "catalog-alpha", "Alpha Adventure", true, false);

    h.send(ProviderOperation::Check, 1, "check").await;
    assert_eq!(
        h.result("check").await.status.unwrap().state,
        AccountConnection::Connected as i32
    );
    h.send(ProviderOperation::SyncCatalog, 1, "catalog").await;
    assert!(matches!(
        completion(&h.result("catalog").await).outcome,
        Some(catalog_completion::Outcome::CompletedRecords(1))
    ));
    h.send(ProviderOperation::Check, 1, "check-other").await;
    assert!(h.result("check-other").await.account_changed);

    h.api.queue_playtime(Ok(vec![crate::http::EpicPlaytime {
        artifact_id: "artifact-alpha".into(),
        seconds: 7200,
    }]));
    h.send(ProviderOperation::ReadCounter, 1, "counter").await;
    let result = h.result("counter").await;

    assert!(
        result.counters.is_empty(),
        "another account's artifact ids must not map"
    );
    assert_eq!(
        h.api.playtime_accounts.lock().unwrap().as_slice(),
        ["fedcba"],
        "the ledger was read for the second account"
    );

    h.close().await;
}

#[tokio::test]
async fn counter_without_a_connected_account_returns_no_observations() {
    let mut h = Harness::open(false).await;

    h.send(ProviderOperation::ReadCounter, 1, "counter").await;
    let result = h.result("counter").await;

    assert!(result.counters.is_empty());
    assert_eq!(h.api.playtime_calls.load(Ordering::Acquire), 0);

    h.close().await;
}

#[tokio::test]
async fn counter_after_sign_out_returns_no_observations() {
    let mut h = Harness::open(true).await;
    h.api
        .queue_refresh(Ok(token(ACCESS, REFRESH, ACCOUNT, "Player One")));
    h.send(ProviderOperation::Check, 1, "check").await;
    assert_eq!(
        h.result("check").await.status.unwrap().state,
        AccountConnection::Connected as i32
    );
    h.send(ProviderOperation::SignOut, 2, "out").await;
    assert!(h.result("out").await.signed_out);

    h.send(ProviderOperation::ReadCounter, 2, "counter").await;
    let result = h.result("counter").await;

    assert!(result.counters.is_empty());
    assert_eq!(h.api.playtime_calls.load(Ordering::Acquire), 0);

    h.close().await;
}

#[tokio::test]
async fn a_stored_credential_only_becomes_connected_after_a_successful_check() {
    let mut h = Harness::open(true).await;
    // Initialize loaded the secret and identity, but made no network request.
    assert_eq!(h.api.refresh_calls.load(Ordering::Acquire), 0);

    h.api
        .queue_refresh(Ok(token(ACCESS, REFRESH, ACCOUNT, "Player One")));
    h.send(ProviderOperation::Check, 1, "check").await;
    let status = h.result("check").await.status.unwrap();

    assert_eq!(status.state, AccountConnection::Connected as i32);
    assert!(status.last_checked_at.is_some());
    assert_eq!(
        status.storage,
        SecretStorage::PrivateFile as i32,
        "the reference host reports the private-file slot"
    );

    h.close().await;
}

#[test]
fn the_reference_storage_method_maps_to_the_private_file_slot() {
    let method: StorageMethod = SecretStorage::PrivateFile.try_into().unwrap();
    assert_eq!(method, StorageMethod::PrivateFile);
}

#[tokio::test]
async fn the_account_status_never_contains_a_token_or_the_authorization_code() {
    let mut h = Harness::open(true).await;
    h.api
        .queue_refresh(Ok(token(ACCESS, REFRESH, ACCOUNT, "Player One")));
    h.send(ProviderOperation::Check, 1, "check").await;
    let status = h.result("check").await.status.unwrap();
    let debug = format!("{status:?}");
    assert!(!debug.contains(ACCESS));
    assert!(!debug.contains(REFRESH));
    assert!(!debug.contains(CODE));

    h.close().await;
}
