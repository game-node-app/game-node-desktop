use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

use gamenode_plugin_protocol::prost::Message;
use gamenode_plugin_sdk::{Error, HostClient, Launch, Session as ClientSession};

#[path = "permissions_tests.rs"]
mod permissions_tests;

#[derive(Default)]
struct Services {
    reads: AtomicUsize,
    publications: AtomicUsize,
    status: Mutex<Option<models::AccountStatus>>,
}

#[tonic::async_trait]
impl HostServices for Services {
    async fn context(&self, lease: Lease) -> Result<ProviderContext, Status> {
        lease.check()?;
        self.reads.fetch_add(1, Ordering::Relaxed);
        Ok(ProviderContext {
            provider_id: "fixture".into(),
            ..Default::default()
        })
    }
    async fn account(&self, _: Lease, _: account_request::Action) -> Result<AccountReply, Status> {
        Err(Status::unimplemented("fixture"))
    }
    async fn secret(&self, _: Lease, _: SecretRequest) -> Result<SecretReply, Status> {
        panic!("unauthorized service must not run")
    }
    async fn login(&self, _: Lease, _: LoginRequest) -> Result<LoginReply, Status> {
        Err(Status::unimplemented("fixture"))
    }
    async fn actions(&self, _: Lease, _: ActionDeclaration) -> Result<ActionReply, Status> {
        Err(Status::unimplemented("fixture"))
    }
    fn publish_status(&self, status: models::AccountStatus) {
        self.publications.fetch_add(1, Ordering::Relaxed);
        *lock(&self.status) = Some(status);
    }
}

struct Harness {
    api: ProviderApi,
    client: ClientSession,
    services: Arc<Services>,
    _server: super::super::PluginApiServer,
    _temp: tempfile::TempDir,
}

impl Harness {
    async fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let endpoint = temp.path().join("plugins.sock");
        let root = super::super::PluginApi::default();
        let api = root.providers.clone();
        let services = Arc::new(Services::default());
        let capabilities = [
            cap::CONTEXT,
            cap::DISCOVERY,
            cap::CATALOG,
            cap::ACCOUNT_PUBLISH,
            cap::LOGIN,
            cap::SETTINGS,
        ];
        api.register(Registration {
            provider_id: "fixture".into(),
            plugin_id: "fixture.plugin".into(),
            capabilities: capabilities.iter().map(|s| (*s).into()).collect(),
            services: services.clone(),
        })
        .unwrap();
        let launch = api.begin_launch("fixture").unwrap();
        let credential = launch
            .credential
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|s| u8::from_str_radix(std::str::from_utf8(s).unwrap(), 16).unwrap())
            .collect();
        let server = super::super::PluginApiServer::start(
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
            &capabilities,
        )
        .await
        .unwrap();
        launch.ready.await.unwrap();
        Self {
            api,
            client,
            services,
            _server: server,
            _temp: temp,
        }
    }

    fn execute(
        &self,
        kind: ProviderOperation,
    ) -> tokio::task::JoinHandle<Result<Completion, Status>> {
        let api = self.api.clone();
        tokio::spawn(async move { api.execute("fixture", kind).await })
    }

    async fn next(&mut self, expected: ProviderOperation) -> HostClient {
        loop {
            let command =
                tokio::time::timeout(Duration::from_secs(2), self.client.commands.message())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
            if command.operation == ProviderOperation::Acknowledge as i32 {
                continue;
            }
            assert_eq!(command.operation, expected as i32);
            return self.client.host.for_command(&command).unwrap();
        }
    }
}

fn completed() -> OperationResult {
    OperationResult {
        disposition: OperationDisposition::Completed as i32,
        observed_at: 100,
        ..Default::default()
    }
}

#[tokio::test]
async fn repeated_sign_in_does_not_invalidate_the_pending_attempt() {
    let mut h = Harness::new().await;
    let first = h.execute(ProviderOperation::SignIn);
    let client = h.next(ProviderOperation::SignIn).await;
    let repeated = h
        .api
        .execute("fixture", ProviderOperation::SignIn)
        .await
        .unwrap();
    assert_eq!(
        repeated.result.disposition,
        OperationDisposition::Superseded as i32
    );
    client.provider_context().await.unwrap();
    client.finish(completed()).await.unwrap();
    first.await.unwrap().unwrap();
}

#[tokio::test]
async fn unsupported_operation_is_rejected_before_dispatch() {
    let h = Harness::new().await;
    let error = h
        .api
        .execute("fixture", ProviderOperation::ReadCounter)
        .await
        .unwrap_err();
    assert_eq!(error.code(), tonic::Code::Unimplemented);
    assert_eq!(h.services.reads.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn real_host_fences_old_account_work_and_keeps_discovery_independent() {
    let mut h = Harness::new().await;
    let scan = h.execute(ProviderOperation::Discover);
    let scan_client = h.next(ProviderOperation::Discover).await;
    let login = h.execute(ProviderOperation::SignIn);
    let login_client = h.next(ProviderOperation::SignIn).await;
    login_client.provider_context().await.unwrap();
    assert_eq!(
        login_client
            .secret(SecretOperation::Read, "session", None)
            .await,
        Err(Error::Denied)
    );
    let cancel = h.execute(ProviderOperation::CancelSignIn);
    let cancel_client = h.next(ProviderOperation::CancelSignIn).await;
    assert_eq!(login_client.provider_context().await, Err(Error::Stale));
    assert_eq!(
        login.await.unwrap().unwrap_err().code(),
        tonic::Code::FailedPrecondition
    );
    cancel_client.finish(completed()).await.unwrap();
    cancel.await.unwrap().unwrap();
    let mut result = completed();
    result.discovery = Some(DiscoveryResult {
        provider_id: "fixture".into(),
        records: vec![],
    });
    scan_client.finish(result).await.unwrap();
    assert!(
        scan.await
            .unwrap()
            .unwrap()
            .result
            .discovery
            .unwrap()
            .records
            .is_empty()
    );
    assert_eq!(h.services.reads.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn incomplete_catalog_cannot_complete_successfully() {
    let mut h = Harness::new().await;
    let operation = h.execute(ProviderOperation::SyncCatalog);
    let client = h.next(ProviderOperation::SyncCatalog).await;
    let completion = client
        .catalog_chunks(
            CatalogCategory::Owned,
            vec![models::CatalogRecord {
                id: "1".into(),
                name: "A game".into(),
            }],
        )
        .await
        .unwrap();
    let mut result = completed();
    result.catalogs = vec![CatalogCompletion {
        outcome: Some(catalog_completion::Outcome::CompletedRecords(2)),
        ..completion
    }];
    client.finish(result).await.unwrap();
    assert_eq!(
        operation.await.unwrap().unwrap_err().code(),
        tonic::Code::InvalidArgument
    );
    assert_eq!(h.services.publications.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn duplicate_completion_is_applied_once_and_does_not_close_the_stream() {
    let mut h = Harness::new().await;
    let operation = h.execute(ProviderOperation::Check);
    let client = h.next(ProviderOperation::Check).await;
    let mut result = completed();
    result.status = Some(AccountStatus {
        state: AccountConnection::Connected as i32,
        account_id: Some("account-1".into()),
        storage: SecretStorage::PrivateFile as i32,
        ..Default::default()
    });
    client.finish(result.clone()).await.unwrap();
    operation.await.unwrap().unwrap();
    client.finish(result).await.unwrap();
    let next = h.execute(ProviderOperation::Check);
    let next_client = h.next(ProviderOperation::Check).await;
    next_client.finish(completed()).await.unwrap();
    next.await.unwrap().unwrap();
    assert_eq!(h.services.publications.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn mismatched_provider_result_is_rejected_without_publishing_state() {
    let mut h = Harness::new().await;
    let operation = h.execute(ProviderOperation::Discover);
    let client = h.next(ProviderOperation::Discover).await;
    let mut result = completed();
    result.discovery = Some(DiscoveryResult {
        provider_id: "other-provider".into(),
        records: vec![],
    });
    client.finish(result).await.unwrap();
    let error = operation.await.unwrap().unwrap_err();
    assert_eq!(error.code(), tonic::Code::PermissionDenied);
    let denial = DenialDetail::decode(error.details()).unwrap();
    assert_eq!(denial.reason, DenialReason::ScopeDenied as i32);
    assert_eq!(denial.permission_id, cap::DISCOVERY);
    assert_eq!(h.services.publications.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn rejected_submission_preserves_previous_successful_account_facts() {
    let mut h = Harness::new().await;
    let first = h.execute(ProviderOperation::Check);
    let client = h.next(ProviderOperation::Check).await;
    let mut accepted = completed();
    accepted.status = Some(AccountStatus {
        state: AccountConnection::Connected as i32,
        account_id: Some("account-previous".into()),
        storage: SecretStorage::PrivateFile as i32,
        ..Default::default()
    });
    client.finish(accepted).await.unwrap();
    first.await.unwrap().unwrap();

    let second = h.execute(ProviderOperation::Check);
    let client = h.next(ProviderOperation::Check).await;
    let mut rejected = completed();
    rejected.discovery = Some(DiscoveryResult {
        provider_id: "foreign-provider".into(),
        records: vec![],
    });
    client.finish(rejected).await.unwrap();
    let error = second.await.unwrap().unwrap_err();
    assert_eq!(error.code(), tonic::Code::PermissionDenied);
    let denial = DenialDetail::decode(error.details()).unwrap();
    assert_eq!(denial.reason, DenialReason::NotGranted as i32);
    assert_eq!(denial.permission_id, cap::DISCOVERY);

    let status = lock(&h.services.status).clone().unwrap();
    assert_eq!(status.state, models::AccountState::Connected);
    assert_eq!(status.account_id.as_deref(), Some("account-previous"));
    assert_eq!(h.services.publications.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn sign_out_wins_over_a_queued_old_storage_write() {
    let mut h = Harness::new().await;
    let operation = h.execute(ProviderOperation::Check);
    let client = h.next(ProviderOperation::Check).await;
    let (lease, _) = h
        .api
        .lease(Some(client.context().clone()), cap::CONTEXT)
        .unwrap();
    let provider = lock(&h.api.state).providers["fixture"].clone();
    let guard = provider.commits.clone().lock_owned().await;
    let api = h.api.clone();
    let mut sign_out =
        Box::pin(async move { api.execute("fixture", ProviderOperation::SignOut).await });
    // Poll explicitly: invalidation is queued before the stale write, without
    // relying on scheduler delays or sleep-based race timing.
    assert!(futures_util::poll!(sign_out.as_mut()).is_pending());
    let mutations = Arc::new(AtomicUsize::new(0));
    let changed = mutations.clone();
    let mut queued = Box::pin(lease.storage(move || {
        changed.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }));
    assert!(futures_util::poll!(queued.as_mut()).is_pending());
    drop(guard);
    let sign_out = tokio::spawn(sign_out);
    let current = h.next(ProviderOperation::SignOut).await;
    assert_eq!(
        queued.await.unwrap_err().code(),
        tonic::Code::FailedPrecondition
    );
    assert_eq!(mutations.load(Ordering::Relaxed), 0);
    assert!(operation.await.unwrap().is_err());
    current.finish(completed()).await.unwrap();
    sign_out.await.unwrap().unwrap();
}

#[tokio::test]
async fn cancelled_rpc_keeps_the_storage_guard_until_its_worker_finishes() {
    let mut h = Harness::new().await;
    let operation = h.execute(ProviderOperation::Check);
    let client = h.next(ProviderOperation::Check).await;
    let (lease, _) = h
        .api
        .lease(Some(client.context().clone()), cap::CONTEXT)
        .unwrap();
    let (started, ready) = oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let work = tokio::spawn(lease.storage(move || {
        let _ = started.send(());
        wait.recv().unwrap();
        Ok(())
    }));
    ready.await.unwrap();
    work.abort();
    assert!(work.await.unwrap_err().is_cancelled());
    let provider = lock(&h.api.state).providers["fixture"].clone();
    assert!(provider.commits.try_lock().is_err());
    release.send(()).unwrap();
    let _finished = provider.commits.lock().await;
    client.finish(completed()).await.unwrap();
    drop(_finished);
    operation.await.unwrap().unwrap();
}

#[tokio::test]
async fn profile_replacement_revokes_the_session_and_queued_profile_writes() {
    let mut h = Harness::new().await;
    let operation = h.execute(ProviderOperation::Check);
    let client = h.next(ProviderOperation::Check).await;
    let (lease, _) = h
        .api
        .lease(Some(client.context().clone()), cap::CONTEXT)
        .unwrap();
    let provider = lock(&h.api.state).providers["fixture"].clone();
    let guard = provider.commits.clone().lock_owned().await;
    let api = h.api.clone();
    let mut invalidation = Box::pin(async move { api.invalidate_profile().await });
    assert!(futures_util::poll!(invalidation.as_mut()).is_pending());
    let mut queued = Box::pin(
        lease.storage(|| -> Result<(), Status> { panic!("obsolete profile must not be written") }),
    );
    assert!(futures_util::poll!(queued.as_mut()).is_pending());
    drop(guard);
    let invalidation = tokio::spawn(invalidation);
    assert!(queued.await.is_err());
    invalidation.await.unwrap();
    assert!(!h.api.connected("fixture"));
    assert!(operation.await.unwrap().is_err());
}

#[tokio::test]
async fn received_results_are_fenced_at_core_commit_and_discovery_survives_sign_out() {
    let mut h = Harness::new().await;
    let old = h.execute(ProviderOperation::Check);
    h.next(ProviderOperation::Check)
        .await
        .finish(completed())
        .await
        .unwrap();
    let old = old.await.unwrap().unwrap();
    let scan = h.execute(ProviderOperation::Discover);
    h.next(ProviderOperation::Discover)
        .await
        .finish(completed())
        .await
        .unwrap();
    let scan = scan.await.unwrap().unwrap();
    let out = h.execute(ProviderOperation::SignOut);
    h.next(ProviderOperation::SignOut)
        .await
        .finish(completed())
        .await
        .unwrap();
    out.await.unwrap().unwrap();
    assert!(
        old.commit(|_, _| panic!("old account completion applied"))
            .await
            .is_err()
    );
    scan.commit(|_, _| ()).await.unwrap();
    let scan = h.execute(ProviderOperation::Discover);
    h.next(ProviderOperation::Discover)
        .await
        .finish(completed())
        .await
        .unwrap();
    let scan = scan.await.unwrap().unwrap();
    h.api.invalidate_profile().await;
    assert!(
        scan.commit(|_, _| panic!("old profile completion applied"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn catalog_deadline_allows_three_network_stages_but_still_expires() {
    let mut h = Harness::new().await;
    let sync = h.execute(ProviderOperation::SyncCatalog);
    let _client = h.next(ProviderOperation::SyncCatalog).await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(271)).await;
    assert!(
        !sync.is_finished(),
        "three bounded network stages fit the host budget"
    );
    tokio::time::advance(Duration::from_secs(30)).await;
    assert_eq!(
        sync.await.unwrap().unwrap_err().code(),
        tonic::Code::DeadlineExceeded
    );
}

#[tokio::test]
async fn typed_settings_failure_is_accepted_and_keeps_the_session_alive() {
    let mut h = Harness::new().await;
    let context = h.api.settings_scope("fixture", "form-1".into()).unwrap();
    let request = settings::Request {
        context: context.clone(),
        action: settings::Action::DescribeRead,
    };
    let api = h.api.clone();
    let operation = tokio::spawn(async move { api.execute_settings(request).await });
    let client = h.next(ProviderOperation::SettingsDescribeRead).await;
    client
        .finish(OperationResult {
            disposition: OperationDisposition::Failed as i32,
            settings: Some(
                settings::Reply {
                    context,
                    outcome: settings::Outcome::Failed {
                        failure: settings::Failure::Invalid,
                    },
                }
                .into(),
            ),
            ..Default::default()
        })
        .await
        .unwrap();
    let completion = operation.await.unwrap().unwrap();
    let reply: settings::Reply = completion.result.settings.unwrap().try_into().unwrap();
    assert_eq!(
        reply.outcome,
        settings::Outcome::Failed {
            failure: settings::Failure::Invalid
        }
    );
    // A typed failure is a normal terminal result: the same provider stream
    // must still serve the next operation.
    let check = h.execute(ProviderOperation::Check);
    let client = h.next(ProviderOperation::Check).await;
    client.finish(completed()).await.unwrap();
    check.await.unwrap().unwrap();
}
