use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

use gamenode_plugin_sdk::{Error, HostClient, Launch, Session as ClientSession};

#[path = "permissions_tests.rs"]
mod permissions_tests;

#[derive(Default)]
struct Services {
    reads: AtomicUsize,
    publications: AtomicUsize,
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
    fn publish_status(&self, _: models::AccountStatus) {
        self.publications.fetch_add(1, Ordering::Relaxed);
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
            cap::ACHIEVEMENTS,
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
        if kind == ProviderOperation::ReadAchievements {
            return tokio::spawn(async move {
                api.execute_achievements(
                    "fixture",
                    models::AchievementsRequest {
                        game_ids: vec!["42".into()],
                        language: "en".into(),
                    },
                )
                .await
            });
        }
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
    assert!(operation.await.unwrap().is_err());
    assert_eq!(h.services.publications.load(Ordering::Relaxed), 0);
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

#[tokio::test]
async fn achievement_batches_and_strings_are_bounded_before_dispatch() {
    let mut h = Harness::new().await;
    let too_many = models::AchievementsRequest {
        game_ids: (0..=gamenode_plugin_protocol::achievements::MAX_GAMES)
            .map(|index| index.to_string())
            .collect(),
        language: "en".into(),
    };
    assert_eq!(
        h.api
            .execute_achievements("fixture", too_many)
            .await
            .unwrap_err()
            .code(),
        tonic::Code::InvalidArgument
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(20), h.client.commands.message())
            .await
            .is_err(),
        "an oversized batch never reaches the plugin"
    );

    let pending = h.execute(ProviderOperation::ReadAchievements);
    let host = h.next(ProviderOperation::ReadAchievements).await;
    let mut result = completed();
    result.achievements = vec![AchievementOutcome {
        game_id: "42".into(),
        outcome: Some(achievement_outcome::Outcome::Ready(AchievementGame {
            achievements: vec![Achievement {
                id: "a".into(),
                name: "n".repeat(gamenode_plugin_protocol::achievements::MAX_NAME + 1),
                ..Default::default()
            }],
        })),
    }];
    let _ = host.finish(result).await;
    assert_eq!(
        pending.await.unwrap().unwrap_err().code(),
        tonic::Code::InvalidArgument
    );
}

#[tokio::test]
async fn achievement_results_for_unrequested_ids_are_rejected() {
    let mut h = Harness::new().await;
    let pending = h.execute(ProviderOperation::ReadAchievements);
    let host = h.next(ProviderOperation::ReadAchievements).await;
    let mut result = completed();
    result.achievements = vec![AchievementOutcome {
        game_id: "99".into(),
        outcome: Some(achievement_outcome::Outcome::None(true)),
    }];
    let _ = host.finish(result).await;
    assert_eq!(
        pending.await.unwrap().unwrap_err().code(),
        tonic::Code::InvalidArgument
    );

    // A completed result must also cover every requested id, so a silent partial batch cannot
    // look like success.
    let mut h = Harness::new().await;
    let pending = h.execute(ProviderOperation::ReadAchievements);
    let host = h.next(ProviderOperation::ReadAchievements).await;
    let mut result = completed();
    result.achievements = Vec::new();
    let _ = host.finish(result).await;
    assert_eq!(
        pending.await.unwrap().unwrap_err().code(),
        tonic::Code::InvalidArgument
    );
}

#[tokio::test]
async fn achievement_results_from_a_previous_account_are_fenced() {
    let mut h = Harness::new().await;
    let pending = h.execute(ProviderOperation::ReadAchievements);
    let host = h.next(ProviderOperation::ReadAchievements).await;

    // A sign-in increments the account epoch and invalidates account-scoped work.
    let api = h.api.clone();
    tokio::spawn(async move {
        let _ = api.execute("fixture", ProviderOperation::SignIn).await;
    });
    for _ in 0..50 {
        if tokio::time::timeout(Duration::from_millis(5), h.client.commands.message())
            .await
            .is_ok()
        {
            break;
        }
        tokio::task::yield_now().await;
    }
    let mut result = completed();
    result.achievements = vec![AchievementOutcome {
        game_id: "42".into(),
        outcome: Some(achievement_outcome::Outcome::None(true)),
    }];
    let _ = host.finish(result).await;
    assert!(
        pending.await.unwrap().is_err(),
        "an achievement result from the previous account is discarded"
    );
    h.api.disconnect("fixture").await;
}

#[tokio::test]
async fn achievement_text_with_raw_control_characters_is_still_rejected_by_the_host() {
    let mut h = Harness::new().await;
    let pending = h.execute(ProviderOperation::ReadAchievements);
    let host = h.next(ProviderOperation::ReadAchievements).await;
    let mut result = completed();
    result.achievements = vec![AchievementOutcome {
        game_id: "42".into(),
        outcome: Some(achievement_outcome::Outcome::Ready(AchievementGame {
            achievements: vec![Achievement {
                id: "a".into(),
                name: "Tab\there\u{85}".into(),
                ..Default::default()
            }],
        })),
    }];
    let _ = host.finish(result).await;
    assert_eq!(
        pending.await.unwrap().unwrap_err().code(),
        tonic::Code::InvalidArgument,
        "the host keeps its control-character rejection as defense"
    );
}
