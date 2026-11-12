//! No application/Core/platform dependencies: the fixture owns configuration,
//! semantic validation and acknowledgement over the public settings contract.
#![cfg(unix)]

use gamenode_plugin_sdk::{
    Error, HostClient, Launch, ProviderPlugin, Session, capability, proto::*, settings::*,
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::{Semaphore, mpsc, oneshot};
use tokio_stream::wrappers::{ReceiverStream, UnixListenerStream};
use tonic::{Request as RpcRequest, Response, Status};

type Commands = mpsc::Receiver<Result<ProviderCommand, Status>>;

#[derive(Clone)]
struct ReferenceHost {
    minor: u32,
    grants: Vec<String>,
    commands: Arc<Mutex<Option<Commands>>>,
    events: mpsc::Sender<ProviderEvent>,
}

#[tonic::async_trait]
impl plugin_host_server::PluginHost for ReferenceHost {
    type WatchPermissionChangesStream = tokio_stream::Empty<Result<PermissionChange, Status>>;
    async fn open_session(
        &self,
        request: RpcRequest<OpenSessionRequest>,
    ) -> Result<Response<OpenSessionReply>, Status> {
        let request = request.into_inner();
        if request.plugin_id != "fixture.settings"
            || request.session_credential != [9; 32]
            || request.requested_capabilities != self.grants
        {
            return Err(Status::permission_denied("not granted"));
        }
        Ok(Response::new(OpenSessionReply {
            session_id: "session".into(),
            granted_capabilities: self.grants.clone(),
            api_major: 1,
            api_minor: self.minor,
            features: vec![],
            support_window: None,
        }))
    }
    async fn submit_discovery(
        &self,
        _: RpcRequest<SubmitDiscoveryRequest>,
    ) -> Result<Response<SubmitDiscoveryReply>, Status> {
        Err(Status::permission_denied("not granted"))
    }
    async fn describe_api(
        &self,
        _: RpcRequest<DescribeApiRequest>,
    ) -> Result<Response<DescribeApiReply>, Status> {
        Err(Status::unimplemented("fixture"))
    }
    async fn get_permissions(
        &self,
        _: RpcRequest<GetPermissionsRequest>,
    ) -> Result<Response<GetPermissionsReply>, Status> {
        Err(Status::unimplemented("fixture"))
    }
    async fn watch_permission_changes(
        &self,
        _: RpcRequest<WatchPermissionChangesRequest>,
    ) -> Result<Response<Self::WatchPermissionChangesStream>, Status> {
        Err(Status::unimplemented("fixture"))
    }
}

#[tonic::async_trait]
impl provider_host_server::ProviderHost for ReferenceHost {
    type ExchangeStream = ReceiverStream<Result<ProviderCommand, Status>>;
    async fn exchange(
        &self,
        request: RpcRequest<tonic::Streaming<ProviderEvent>>,
    ) -> Result<Response<Self::ExchangeStream>, Status> {
        let mut stream = request.into_inner();
        let ready = stream
            .message()
            .await?
            .ok_or_else(|| Status::unauthenticated("no ready event"))?;
        if !matches!(ready.event, Some(provider_event::Event::Ready(true))) {
            return Err(Status::permission_denied("not ready"));
        }
        let tx = self.events.clone();
        tokio::spawn(async move {
            while let Ok(Some(event)) = stream.message().await {
                if tx.send(event).await.is_err() {
                    break;
                }
            }
        });
        Ok(Response::new(ReceiverStream::new(
            self.commands.lock().unwrap().take().unwrap(),
        )))
    }
    async fn get_context(
        &self,
        _: RpcRequest<ServiceContext>,
    ) -> Result<Response<ProviderContext>, Status> {
        Err(Status::permission_denied("not granted"))
    }
    async fn account(
        &self,
        _: RpcRequest<AccountRequest>,
    ) -> Result<Response<AccountReply>, Status> {
        Err(Status::permission_denied("not granted"))
    }
    async fn secret(&self, _: RpcRequest<SecretRequest>) -> Result<Response<SecretReply>, Status> {
        Err(Status::permission_denied("not granted"))
    }
    async fn login(&self, _: RpcRequest<LoginRequest>) -> Result<Response<LoginReply>, Status> {
        Err(Status::permission_denied("not granted"))
    }
    async fn declare_actions(
        &self,
        _: RpcRequest<ActionRequest>,
    ) -> Result<Response<ActionReply>, Status> {
        Err(Status::permission_denied("not granted"))
    }
}

struct Harness {
    commands: mpsc::Sender<Result<ProviderCommand, Status>>,
    events: mpsc::Receiver<ProviderEvent>,
    stopped: Option<oneshot::Sender<()>>,
    server: tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
    _temp: tempfile::TempDir,
}

impl Harness {
    async fn open(minor: u32, grants: &[&str]) -> (Self, Result<Session, Error>) {
        let temp = tempfile::tempdir().unwrap();
        let endpoint = temp.path().join("settings.sock");
        let listener = tokio::net::UnixListener::bind(&endpoint).unwrap();
        let (commands, rx) = mpsc::channel(8);
        let (tx, events) = mpsc::channel(8);
        let host = ReferenceHost {
            minor,
            grants: grants.iter().map(|s| (*s).into()).collect(),
            commands: Arc::new(Mutex::new(Some(rx))),
            events: tx,
        };
        let (shutdown, stopped) = oneshot::channel();
        let server = tokio::spawn(
            tonic::transport::Server::builder()
                .add_service(plugin_host_server::PluginHostServer::new(host.clone()))
                .add_service(provider_host_server::ProviderHostServer::new(host))
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
            "fixture.settings",
            "fixture",
            "1.0",
            grants,
        )
        .await;
        (
            Self {
                commands,
                events,
                stopped: Some(shutdown),
                server,
                _temp: temp,
            },
            session,
        )
    }

    async fn send(&self, id: &str, action: Action) -> Request {
        let settings = gamenode_plugin_sdk::settings::Request {
            context: Context {
                plugin_id: "fixture.settings".into(),
                provider_id: "fixture".into(),
                session_id: "session".into(),
                profile_epoch: 5,
                form_handle: "form-1".into(),
            },
            action,
        };
        self.commands
            .send(Ok(ProviderCommand {
                context: Some(ServiceContext {
                    session_id: "session".into(),
                    session_credential: vec![9; 32],
                    provider_id: "fixture".into(),
                    operation_id: id.into(),
                    account_epoch: 8,
                    profile_epoch: 5,
                    session_kind: SessionKind::Unspecified as i32,
                    target_game: None,
                }),
                operation: settings.operation() as i32,
                target_operation_id: String::new(),
                settings: Some(settings.clone().into()),
                runtime_session_result: None,
            }))
            .await
            .unwrap();
        settings
    }

    async fn result(&mut self, id: &str) -> OperationResult {
        let event = tokio::time::timeout(std::time::Duration::from_secs(3), self.events.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(event.context.unwrap().operation_id, id);
        let Some(provider_event::Event::Result(result)) = event.event else {
            panic!("terminal reply expected");
        };
        result
    }

    async fn collect(&mut self, count: usize) -> BTreeMap<String, OperationResult> {
        let mut results = BTreeMap::new();
        for _ in 0..count {
            let event = tokio::time::timeout(std::time::Duration::from_secs(3), self.events.recv())
                .await
                .unwrap()
                .unwrap();
            let id = event.context.unwrap().operation_id;
            let Some(provider_event::Event::Result(result)) = event.event else {
                panic!("terminal reply expected");
            };
            results.insert(id, result);
        }
        results
    }

    async fn reply(&mut self, id: &str) -> Reply {
        let result = self.result(id).await;
        assert_eq!(result.disposition, OperationDisposition::Completed as i32);
        assert!(result.failure.is_none());
        result.settings.unwrap().try_into().unwrap()
    }

    async fn stop(mut self, plugin: tokio::task::JoinHandle<Result<(), Error>>) {
        drop(self.commands);
        tokio::time::timeout(std::time::Duration::from_secs(3), plugin)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let _ = self.stopped.take().unwrap().send(());
        self.server.await.unwrap().unwrap();
    }
}

struct Config {
    revision: u64,
    prefix: String,
}
struct Fixture {
    config: Mutex<Config>,
    fail_persistence: AtomicBool,
    saving: Semaphore,
    cancel: Semaphore,
}
impl Fixture {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            config: Mutex::new(Config {
                revision: 1,
                prefix: "Games".into(),
            }),
            fail_persistence: false.into(),
            saving: Semaphore::new(0),
            cancel: Semaphore::new(0),
        })
    }
    fn form(&self) -> Form {
        Form {
            schema_version: SCHEMA_VERSION,
            form_id: "preferences".into(),
            schema_revision: "schema-1".into(),
            configuration_revision: self.config.lock().unwrap().revision.to_string(),
            title: Text::plain("Preferences"),
            fields: vec![Field {
                id: "prefix".into(),
                label: Text::plain("Prefix"),
                help: None,
                kind: Kind::Text {
                    min_length: 1,
                    max_length: 20,
                },
                default: Value::Text("Games".into()),
            }],
        }
    }
}

#[tonic::async_trait]
impl ProviderPlugin for Fixture {
    async fn handle(&self, _: ProviderCommand, _: HostClient) -> Result<OperationResult, Error> {
        Err(Error::Denied)
    }
    async fn settings(
        &self,
        request: gamenode_plugin_sdk::settings::Request,
        host: HostClient,
    ) -> Result<Reply, Error> {
        // A settings-only grant cannot acquire account/context/secret authority.
        assert!(matches!(host.provider_context().await, Err(Error::Denied)));
        let outcome = match request.action {
            Action::DescribeRead => Outcome::Form {
                form: self.form(),
                values: BTreeMap::from([(
                    "prefix".into(),
                    Value::Text(self.config.lock().unwrap().prefix.clone()),
                )]),
            },
            Action::Submit {
                schema_revision,
                configuration_revision,
                values,
            } => {
                let form = self.form();
                form.validate_values(&values)?;
                if schema_revision != form.schema_revision
                    || configuration_revision != form.configuration_revision
                {
                    Outcome::RevisionConflict {
                        configuration_revision: form.configuration_revision,
                    }
                } else if values.get("prefix") == Some(&Value::Text("reserved".into())) {
                    Outcome::FieldErrors {
                        errors: BTreeMap::from([(
                            "prefix".into(),
                            Text::plain("This prefix is reserved"),
                        )]),
                    }
                } else if values.get("prefix") == Some(&Value::Text("wait".into())) {
                    self.saving.add_permits(1);
                    self.cancel.acquire().await.unwrap().forget();
                    Outcome::Failed {
                        failure: Failure::Cancelled,
                    }
                } else if self.fail_persistence.load(Ordering::Relaxed) {
                    Outcome::Failed {
                        failure: Failure::PersistenceFailed,
                    }
                } else {
                    let Value::Text(prefix) = values.get("prefix").unwrap() else {
                        return Err(Error::Protocol);
                    };
                    let mut config = self.config.lock().unwrap();
                    config.prefix = prefix.clone();
                    config.revision += 1;
                    Outcome::Applied {
                        configuration_revision: config.revision.to_string(),
                        persistence: Persistence::Persisted,
                    }
                }
            }
            Action::Cancel => {
                self.cancel.add_permits(1);
                Outcome::Failed {
                    failure: Failure::Cancelled,
                }
            }
        };
        Ok(Reply {
            context: request.context,
            outcome,
        })
    }
}

fn submit(revision: &str, prefix: &str) -> Action {
    Action::Submit {
        schema_revision: "schema-1".into(),
        configuration_revision: revision.into(),
        values: BTreeMap::from([("prefix".into(), Value::Text(prefix.into()))]),
    }
}

/// Mirrors the desktop host's acceptance check: a settings result must carry a
/// correlated, valid payload. A bare failure is denied and tears down the
/// whole provider session, which the permissive reference host never did.
fn host_accepts(request: &Request, result: &OperationResult) -> bool {
    result.settings.as_ref().is_some_and(|reply| {
        Reply::try_from(reply.clone()).is_ok_and(|reply| reply.validate_for(request).is_ok())
    })
}

struct FailingSettings;

#[tonic::async_trait]
impl ProviderPlugin for FailingSettings {
    async fn handle(&self, _: ProviderCommand, _: HostClient) -> Result<OperationResult, Error> {
        Err(Error::Denied)
    }
    async fn settings(&self, request: Request, _: HostClient) -> Result<Reply, Error> {
        match request.action {
            Action::DescribeRead => Err(Error::Denied),
            Action::Submit { .. } => {
                let mut context = request.context;
                context.form_handle = "other-form".into();
                Ok(Reply {
                    context,
                    outcome: Outcome::Failed {
                        failure: Failure::Unavailable,
                    },
                })
            }
            Action::Cancel => Err(Error::Stale),
        }
    }
}

struct HeldSettings {
    release: Semaphore,
}

#[tonic::async_trait]
impl ProviderPlugin for HeldSettings {
    async fn handle(&self, _: ProviderCommand, _: HostClient) -> Result<OperationResult, Error> {
        Err(Error::Denied)
    }
    async fn settings(&self, request: Request, _: HostClient) -> Result<Reply, Error> {
        self.release.acquire().await.unwrap().forget();
        Ok(Reply {
            context: request.context,
            outcome: Outcome::Failed {
                failure: Failure::Cancelled,
            },
        })
    }
}

#[tokio::test]
async fn independent_settings_plugin_acknowledges_save_reopen_conflicts_and_persistence_failure() {
    let (mut h, session) = Harness::open(3, &[capability::SETTINGS]).await;
    let fixture = Fixture::new();
    let plugin = tokio::spawn(gamenode_plugin_sdk::run(session.unwrap(), fixture.clone()));
    h.send("read", Action::DescribeRead).await;
    let reply = h.reply("read").await;
    assert!(
        matches!(reply.outcome, Outcome::Form { ref form, .. } if form.configuration_revision == "1")
    );
    h.send("invalid", submit("1", "reserved")).await;
    assert!(matches!(
        h.reply("invalid").await.outcome,
        Outcome::FieldErrors { .. }
    ));
    assert_eq!(fixture.config.lock().unwrap().revision, 1);
    fixture.fail_persistence.store(true, Ordering::Relaxed);
    h.send("failed", submit("1", "Arcade")).await;
    assert!(matches!(
        h.reply("failed").await.outcome,
        Outcome::Failed {
            failure: Failure::PersistenceFailed
        }
    ));
    assert_eq!(fixture.config.lock().unwrap().prefix, "Games");
    fixture.fail_persistence.store(false, Ordering::Relaxed);
    h.send("save", submit("1", "Arcade")).await;
    assert!(
        matches!(h.reply("save").await.outcome, Outcome::Applied { ref configuration_revision, persistence: Persistence::Persisted } if configuration_revision == "2")
    );
    h.send("conflict", submit("1", "Overwrite")).await;
    assert!(
        matches!(h.reply("conflict").await.outcome, Outcome::RevisionConflict { ref configuration_revision } if configuration_revision == "2")
    );
    h.send("reopen", Action::DescribeRead).await;
    assert!(
        matches!(h.reply("reopen").await.outcome, Outcome::Form { ref values, .. } if values.get("prefix") == Some(&Value::Text("Arcade".into())))
    );
    h.stop(plugin).await;
}

#[tokio::test]
async fn settings_cancel_runs_while_submit_is_pending_and_does_not_apply_draft() {
    let (mut h, session) = Harness::open(3, &[capability::SETTINGS]).await;
    let fixture = Fixture::new();
    let plugin = tokio::spawn(gamenode_plugin_sdk::run(session.unwrap(), fixture.clone()));
    h.send("save", submit("1", "wait")).await;
    tokio::time::timeout(std::time::Duration::from_secs(3), fixture.saving.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    h.send("cancel", Action::Cancel).await;
    // Both operations finish; scheduling may deliver either terminal event first.
    for _ in 0..2 {
        let event = tokio::time::timeout(std::time::Duration::from_secs(3), h.events.recv())
            .await
            .unwrap()
            .unwrap();
        let Some(provider_event::Event::Result(result)) = event.event else {
            panic!("terminal expected");
        };
        let reply: Reply = result.settings.unwrap().try_into().unwrap();
        assert!(matches!(
            reply.outcome,
            Outcome::Failed {
                failure: Failure::Cancelled
            }
        ));
    }
    assert_eq!(fixture.config.lock().unwrap().prefix, "Games");
    assert_eq!(fixture.config.lock().unwrap().revision, 1);
    h.stop(plugin).await;
}

#[tokio::test]
async fn settings_require_feature_version_and_grant_before_dispatch() {
    let (mut h, session) = Harness::open(2, &[capability::SETTINGS]).await;
    assert!(matches!(session, Err(Error::Protocol)));
    let _ = h.stopped.take().unwrap().send(());
    h.server.await.unwrap().unwrap();
    let (mut h, session) = Harness::open(3, &[capability::CONTEXT]).await;
    let fixture = Fixture::new();
    let plugin = tokio::spawn(gamenode_plugin_sdk::run(session.unwrap(), fixture.clone()));
    h.send("forged", Action::DescribeRead).await;
    let event = tokio::time::timeout(std::time::Duration::from_secs(3), h.events.recv())
        .await
        .unwrap()
        .unwrap();
    let Some(provider_event::Event::Result(result)) = event.event else {
        panic!("terminal expected");
    };
    assert_eq!(result.failure.unwrap().code, "denied");
    assert!(result.settings.is_none());
    assert_eq!(fixture.config.lock().unwrap().revision, 1);
    h.stop(plugin).await;
}

#[tokio::test]
async fn plugin_settings_errors_become_typed_failures_the_host_can_accept() {
    let (mut h, session) = Harness::open(3, &[capability::SETTINGS]).await;
    let plugin = tokio::spawn(gamenode_plugin_sdk::run(
        session.unwrap(),
        Arc::new(FailingSettings),
    ));

    let request = h.send("denied", Action::DescribeRead).await;
    let result = h.result("denied").await;
    assert_eq!(result.disposition, OperationDisposition::Failed as i32);
    let reply: Reply = result.settings.clone().unwrap().try_into().unwrap();
    assert_eq!(
        reply.outcome,
        Outcome::Failed {
            failure: Failure::Denied
        }
    );
    assert_eq!(reply.context, request.context);
    assert!(
        host_accepts(&request, &result),
        "a bare or uncorrelated failure would be rejected by the desktop host"
    );

    // A reply that fails correlation is replaced by a typed Invalid failure
    // instead of escaping as a bare protocol error.
    let request = h.send("uncorrelated", submit("1", "Arcade")).await;
    let result = h.result("uncorrelated").await;
    let reply: Reply = result.settings.clone().unwrap().try_into().unwrap();
    assert_eq!(
        reply.outcome,
        Outcome::Failed {
            failure: Failure::Invalid
        }
    );
    assert_eq!(reply.context, request.context);
    assert!(host_accepts(&request, &result));
    h.stop(plugin).await;
}

#[tokio::test]
async fn settings_busy_replies_keep_their_typed_context() {
    let (mut h, session) = Harness::open(3, &[capability::SETTINGS]).await;
    let plugin = Arc::new(HeldSettings {
        release: Semaphore::new(0),
    });
    let task = tokio::spawn(gamenode_plugin_sdk::run(session.unwrap(), plugin.clone()));
    let mut requests = BTreeMap::new();
    for index in 0..9 {
        let id = format!("hold-{index}");
        let request = h.send(&id, Action::DescribeRead).await;
        requests.insert(id, request);
    }
    // The ninth concurrent settings operation exhausts the work permits, but
    // the Busy path runs before dispatch and must still echo its context.
    let busy = h.collect(1).await;
    let result = &busy["hold-8"];
    assert_eq!(result.disposition, OperationDisposition::Failed as i32);
    let reply: Reply = result.settings.clone().unwrap().try_into().unwrap();
    assert_eq!(
        reply.outcome,
        Outcome::Failed {
            failure: Failure::Unavailable
        }
    );
    assert!(host_accepts(&requests["hold-8"], result));
    plugin.release.add_permits(8);
    let held = h.collect(8).await;
    assert_eq!(held.len(), 8);
    for (id, result) in &held {
        assert!(host_accepts(&requests[id], result));
    }
    h.stop(task).await;
}
