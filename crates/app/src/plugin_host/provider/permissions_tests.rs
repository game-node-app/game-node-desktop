//! Adversarial authorization cases exercise public RPC/event handlers. Adapter
//! entry counters ensure rejection happens before host I/O or publication.
use super::*;
use provider_host_server::ProviderHost;

struct CountingServices {
    inner: Arc<Services>,
    entered: Arc<AtomicUsize>,
}

#[tonic::async_trait]
impl HostServices for CountingServices {
    async fn context(&self, lease: Lease) -> Result<ProviderContext, Status> {
        self.entered.fetch_add(1, Ordering::SeqCst);
        self.inner.context(lease).await
    }
    async fn account(
        &self,
        lease: Lease,
        action: account_request::Action,
    ) -> Result<AccountReply, Status> {
        self.entered.fetch_add(1, Ordering::SeqCst);
        self.inner.account(lease, action).await
    }
    async fn secret(&self, lease: Lease, request: SecretRequest) -> Result<SecretReply, Status> {
        self.entered.fetch_add(1, Ordering::SeqCst);
        self.inner.secret(lease, request).await
    }
    async fn login(&self, lease: Lease, request: LoginRequest) -> Result<LoginReply, Status> {
        self.entered.fetch_add(1, Ordering::SeqCst);
        self.inner.login(lease, request).await
    }
    async fn actions(
        &self,
        lease: Lease,
        actions: ActionDeclaration,
    ) -> Result<ActionReply, Status> {
        self.entered.fetch_add(1, Ordering::SeqCst);
        self.inner.actions(lease, actions).await
    }
    fn publish_status(&self, status: models::AccountStatus) {
        self.entered.fetch_add(1, Ordering::SeqCst);
        self.inner.publish_status(status);
    }
}

impl Harness {
    /// Negotiate the exact granted descriptor instead of fabricating a session
    /// with permissions the handshake never approved.
    async fn with_grants(grants: &[&str]) -> (Self, Arc<AtomicUsize>) {
        let temp = tempfile::tempdir().unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let endpoint = temp.path().join("permissions.sock");
        let root = super::super::super::PluginApi::default();
        let api = root.providers.clone();
        let services = Arc::new(Services::default());
        let entered = Arc::new(AtomicUsize::new(0));
        let guarded = Arc::new(CountingServices {
            inner: services.clone(),
            entered: entered.clone(),
        });
        for (provider, plugin) in [("fixture", "fixture.plugin"), ("other", "other.plugin")] {
            api.register(Registration {
                provider_id: provider.into(),
                plugin_id: plugin.into(),
                capabilities: grants.iter().map(|v| (*v).into()).collect(),
                services: guarded.clone(),
            })
            .unwrap();
        }
        let launch = api.begin_launch("fixture").unwrap();
        let credential = (0..launch.credential.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&launch.credential[i..i + 2], 16).unwrap())
            .collect();
        let server = super::super::super::PluginApiServer::start(
            &tokio::runtime::Handle::current(),
            &endpoint,
            root,
        )
        .unwrap();
        let client = ClientSession::open(
            Launch {
                endpoint,
                credential,
                operation_id: "launch".into(),
            },
            "fixture.plugin",
            "fixture",
            "1.0",
            grants,
        )
        .await
        .unwrap();
        launch.ready.await.unwrap();
        (
            Self {
                api,
                client,
                services,
                _server: server,
                _temp: temp,
            },
            entered,
        )
    }
}

#[derive(Clone, Copy, Debug)]
enum Access {
    Context,
    Discovery,
    Catalog,
    AccountRead,
    AccountSave,
    AccountStatus,
    Login,
    Cookies,
    Page,
    SecretRead,
    SecretWrite,
    SecretDelete,
    Counter,
    Actions,
}

const CASES: &[Access] = &[
    Access::Context,
    Access::Discovery,
    Access::Catalog,
    Access::AccountRead,
    Access::AccountSave,
    Access::AccountStatus,
    Access::Login,
    Access::Cookies,
    Access::Page,
    Access::SecretRead,
    Access::SecretWrite,
    Access::SecretDelete,
    Access::Counter,
    Access::Actions,
];

impl Access {
    fn capability(self) -> &'static str {
        match self {
            Self::Context => cap::CONTEXT,
            Self::Discovery => cap::DISCOVERY,
            Self::Catalog => cap::CATALOG,
            Self::AccountRead => cap::ACCOUNT_READ,
            Self::AccountSave | Self::AccountStatus => cap::ACCOUNT_PUBLISH,
            Self::Login => cap::LOGIN,
            Self::Cookies => cap::COOKIES,
            Self::Page => cap::PAGE,
            Self::SecretRead => cap::SECRET_READ,
            Self::SecretWrite => cap::SECRET_WRITE,
            Self::SecretDelete => cap::SECRET_DELETE,
            Self::Counter => cap::COUNTER,
            Self::Actions => cap::ACTIONS,
        }
    }
    fn operation(self) -> ProviderOperation {
        match self {
            Self::Discovery => ProviderOperation::Discover,
            Self::Catalog => ProviderOperation::SyncCatalog,
            Self::Login | Self::Cookies | Self::Page => ProviderOperation::SignIn,
            Self::Counter => ProviderOperation::ReadCounter,
            Self::Actions => ProviderOperation::Initialize,
            _ => ProviderOperation::Check,
        }
    }
    fn required_to_dispatch(self) -> bool {
        matches!(
            self,
            Self::Discovery | Self::Catalog | Self::Login | Self::Counter
        )
    }

    async fn attempt(self, api: &ProviderApi, context: ServiceContext) -> Result<(), Status> {
        let event = |event| ProviderEvent {
            context: Some(context.clone()),
            event: Some(event),
        };
        match self {
            Self::Context => ProviderHost::get_context(api, Request::new(context))
                .await
                .map(|_| ()),
            Self::AccountRead | Self::AccountSave => {
                let action = if matches!(self, Self::AccountRead) {
                    account_request::Action::Read(true)
                } else {
                    account_request::Action::Save(AccountIdentity {
                        provider_id: "fixture".into(),
                        account_id: "account-1".into(),
                        ..Default::default()
                    })
                };
                ProviderHost::account(
                    api,
                    Request::new(AccountRequest {
                        context: Some(context),
                        action: Some(action),
                    }),
                )
                .await
                .map(|_| ())
            }
            Self::SecretRead | Self::SecretWrite | Self::SecretDelete => {
                let operation = match self {
                    Self::SecretRead => SecretOperation::Read,
                    Self::SecretWrite => SecretOperation::Write,
                    _ => SecretOperation::Delete,
                };
                ProviderHost::secret(
                    api,
                    Request::new(SecretRequest {
                        context: Some(context),
                        key: "session".into(),
                        operation: operation as i32,
                        value: (operation == SecretOperation::Write)
                            .then(|| b"opaque-secret".to_vec()),
                    }),
                )
                .await
                .map(|_| ())
            }
            Self::Login | Self::Cookies | Self::Page => ProviderHost::login(
                api,
                Request::new(LoginRequest {
                    context: Some(context),
                    handle: "approved-attempt".into(),
                    operation: if matches!(self, Self::Login) {
                        LoginOperation::Open
                    } else {
                        LoginOperation::Read
                    } as i32,
                }),
            )
            .await
            .map(|_| ()),
            Self::Actions => ProviderHost::declare_actions(
                api,
                Request::new(ActionRequest {
                    context: Some(context),
                    actions: Some(ActionDeclaration {
                        play_uri: Some("store://play/{id}".into()),
                        install_uri: None,
                    }),
                }),
            )
            .await
            .map(|_| ()),
            Self::AccountStatus => {
                api.event(event(provider_event::Event::AccountStatus(AccountStatus {
                    state: AccountConnection::Connected as i32,
                    account_id: Some("account-1".into()),
                    storage: SecretStorage::PrivateFile as i32,
                    ..Default::default()
                })))
                .await
            }
            Self::Catalog => {
                api.event(event(provider_event::Event::CatalogChunk(CatalogChunk {
                    category: CatalogCategory::Owned as i32,
                    index: 0,
                    games: vec![CatalogGame {
                        id: "42".into(),
                        name: "A game".into(),
                    }],
                })))
                .await
            }
            Self::Discovery | Self::Counter => {
                let mut result = completed();
                if matches!(self, Self::Discovery) {
                    result.discovery = Some(DiscoveryResult {
                        provider_id: "fixture".into(),
                        records: vec![],
                    });
                } else {
                    result.counters = vec![CounterObservation {
                        id: "42".into(),
                        seconds: 60,
                        last_played: 10,
                    }];
                }
                api.event(event(provider_event::Event::Result(result)))
                    .await
            }
        }
    }
}

fn untouched(h: &Harness, entered: &AtomicUsize, case: Access) {
    assert_eq!(
        entered.load(Ordering::SeqCst),
        0,
        "{case:?} reached a host adapter"
    );
    assert_eq!(
        h.services.reads.load(Ordering::SeqCst),
        0,
        "{case:?} read context"
    );
    assert_eq!(
        h.services.publications.load(Ordering::SeqCst),
        0,
        "{case:?} published state"
    );
}

#[tokio::test]
async fn every_missing_host_grant_is_rejected_before_adapter_or_domain_effects() {
    for &case in CASES {
        let mut grants: Vec<_> = cap::ALL
            .iter()
            .copied()
            .filter(|v| *v != case.capability())
            .collect();
        // Settings-only descriptors can legitimately omit context. Include its
        // independent feature solely to permit negotiating that negative case.
        if matches!(case, Access::Context) {
            grants.push(cap::SETTINGS);
        }
        let (mut h, entered) = Harness::with_grants(&grants).await;
        if case.required_to_dispatch() {
            let error = h
                .api
                .execute("fixture", case.operation())
                .await
                .unwrap_err();
            assert_eq!(error.code(), tonic::Code::Unimplemented, "{case:?}");
            assert!(
                tokio::time::timeout(Duration::from_millis(10), h.client.commands.message())
                    .await
                    .is_err(),
                "{case:?} dispatched without its grant"
            );
        } else {
            let operation = if matches!(case, Access::AccountSave | Access::AccountStatus) {
                ProviderOperation::Initialize
            } else {
                case.operation()
            };
            let pending = h.execute(operation);
            let context = h.next(operation).await.context().clone();
            assert_eq!(
                case.attempt(&h.api, context).await.unwrap_err().code(),
                tonic::Code::PermissionDenied,
                "{case:?}"
            );
            h.api.disconnect("fixture").await;
            assert!(
                pending.await.unwrap().is_err(),
                "{case:?} yielded a domain completion"
            );
        }
        untouched(&h, &entered, case);
    }
}

#[tokio::test]
async fn every_service_rejects_cross_provider_credentials_and_obsolete_sessions() {
    for &case in CASES {
        let (mut h, entered) = Harness::with_grants(cap::ALL).await;
        let pending = h.execute(case.operation());
        let context = h.next(case.operation()).await.context().clone();
        let mut cross_provider = context.clone();
        cross_provider.provider_id = "other".into();
        assert_eq!(
            case.attempt(&h.api, cross_provider)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::PermissionDenied,
            "{case:?}: provider"
        );
        let other = h.api.begin_launch("other").unwrap();
        let mut cross_plugin = context.clone();
        cross_plugin.session_credential = (0..other.credential.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&other.credential[i..i + 2], 16).unwrap())
            .collect();
        assert_eq!(
            case.attempt(&h.api, cross_plugin).await.unwrap_err().code(),
            tonic::Code::PermissionDenied,
            "{case:?}: plugin credential"
        );
        let mut wrong_operation = context.clone();
        wrong_operation.operation_id = "other-operation".into();
        assert_eq!(
            case.attempt(&h.api, wrong_operation)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::FailedPrecondition,
            "{case:?}: operation"
        );
        h.api.disconnect("fixture").await;
        assert!(
            pending.await.unwrap().is_err(),
            "{case:?} yielded a domain completion"
        );
        assert_eq!(
            case.attempt(&h.api, context).await.unwrap_err().code(),
            tonic::Code::Unauthenticated,
            "{case:?}: obsolete session"
        );
        untouched(&h, &entered, case);
        h.api.disconnect("other").await;
    }
}

#[tokio::test]
async fn a_granted_permission_cannot_be_borrowed_by_an_unrelated_operation() {
    for &case in CASES.iter().filter(|case| !matches!(case, Access::Context)) {
        let (mut h, entered) = Harness::with_grants(cap::ALL).await;
        let operation = if matches!(case, Access::Discovery) {
            ProviderOperation::Check
        } else {
            ProviderOperation::Discover
        };
        let pending = h.execute(operation);
        let context = h.next(operation).await.context().clone();
        assert_eq!(
            case.attempt(&h.api, context).await.unwrap_err().code(),
            tonic::Code::PermissionDenied,
            "{case:?}"
        );
        h.api.disconnect("fixture").await;
        assert!(
            pending.await.unwrap().is_err(),
            "{case:?} yielded a domain completion"
        );
        untouched(&h, &entered, case);
    }
}
