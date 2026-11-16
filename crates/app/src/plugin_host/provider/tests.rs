use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

use gamenode_plugin_protocol::prost::Message;
use gamenode_plugin_sdk::{Error, HostClient, Launch, Session as ClientSession, SessionOptions};

#[path = "permissions_tests.rs"]
mod permissions_tests;
#[path = "profile_tests.rs"]
mod profile_tests;

#[derive(Default)]
struct Services {
    reads: AtomicUsize,
    publications: AtomicUsize,
    status: Mutex<Option<models::AccountStatus>>,
}

#[tokio::test]
async fn configuration_modes_separate_declarations_from_active_authorization() {
    let harness = Harness::third_party_config("account").await;
    let available = harness.api.configuration_modes("fixture").unwrap();
    assert!(available.external_declared && available.external_authorized);
    assert!(available.declarative_declared && available.declarative_authorized);

    harness
        .api
        .revoke_session_permission("fixture", cap::SETTINGS)
        .await;
    let revoked = harness.api.configuration_modes("fixture").unwrap();
    assert!(revoked.declarative_declared);
    assert!(!revoked.declarative_authorized);
    assert!(revoked.external_authorized);
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
        Self::build(&[], false).await
    }

    /// A harness whose trusted descriptor declares shared catalog access.
    async fn with_shared_catalog() -> Self {
        Self::build(&[], true).await
    }

    /// A harness whose descriptor also grants the counter capability.
    async fn with_counter() -> Self {
        Self::with_extra_capabilities(&[cap::COUNTER]).await
    }

    async fn with_extra_capabilities(extra: &[&'static str]) -> Self {
        Self::build(extra, false).await
    }

    async fn build(extra: &[&'static str], shared_catalog: bool) -> Self {
        Self::build_with(extra, shared_catalog, RegistrationOrigin::Official, None).await
    }

    /// A harness for a third-party provider running inside the profile of `account`.
    async fn third_party(account: &str) -> Self {
        Self::build_with(
            &[],
            false,
            RegistrationOrigin::ThirdParty {
                publisher_fingerprint: "ab".repeat(32),
            },
            Some(account),
        )
        .await
    }

    async fn third_party_config(account: &str) -> Self {
        Self::build_with(
            &[cap::CONFIGURE, cap::LIBRARY_READ],
            false,
            RegistrationOrigin::ThirdParty {
                publisher_fingerprint: "ab".repeat(32),
            },
            Some(account),
        )
        .await
    }

    async fn build_with(
        extra: &[&'static str],
        shared_catalog: bool,
        origin: RegistrationOrigin,
        account: Option<&str>,
    ) -> Self {
        let temp = tempfile::tempdir().unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let endpoint = temp.path().join("plugins.sock");
        let root = super::super::PluginApi::default();
        let api = root.providers.clone();
        api.profiles().update(account, true);
        let services = Arc::new(Services::default());
        let mut capabilities = vec![
            cap::CONTEXT,
            cap::DISCOVERY,
            cap::CATALOG,
            cap::ACCOUNT_PUBLISH,
            cap::LOGIN,
            cap::SETTINGS,
            cap::ACHIEVEMENTS,
        ];
        capabilities.extend_from_slice(extra);
        api.register(Registration {
            provider_id: "fixture".into(),
            plugin_id: "fixture.plugin".into(),
            kind: RegistrationKind::Provider,
            origin,
            capabilities: capabilities.iter().map(|s| (*s).into()).collect(),
            shared_catalog,
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
        let plugin_launch = Launch {
            endpoint,
            credential,
            operation_id: "launch".into(),
        };
        let client = if capabilities.contains(&cap::CONFIGURE) {
            ClientSession::open_with_options(
                plugin_launch,
                "fixture.plugin",
                "fixture",
                "1.0",
                &capabilities,
                SessionOptions {
                    min_api_major: 1,
                    min_api_minor: gamenode_plugin_protocol::API_MINOR,
                    max_api_major: 1,
                    max_api_minor: gamenode_plugin_protocol::API_MINOR,
                    ..Default::default()
                },
            )
            .await
            .unwrap()
        } else {
            ClientSession::open(
                plugin_launch,
                "fixture.plugin",
                "fixture",
                "1.0",
                &capabilities,
            )
            .await
            .unwrap()
        };
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
        let command = self.next_command(expected).await;
        self.client.host.for_command(&command).unwrap()
    }

    async fn next_command(&mut self, expected: ProviderOperation) -> ProviderCommand {
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
            return command;
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

fn configuration_result(command: &ProviderCommand) -> OperationResult {
    OperationResult {
        disposition: OperationDisposition::Completed as i32,
        configuration: Some(ConfigurationResult {
            session_id: command.context.as_ref().unwrap().session_id.clone(),
            profile_id: super::super::profile::profile_id_for_account("account"),
            window_id: command.target_operation_id.clone(),
            accepted: true,
            failure: String::new(),
        }),
        ..Default::default()
    }
}

async fn open_configuration_window(h: &mut Harness) -> (String, HostClient) {
    let api = h.api.clone();
    let opening = tokio::spawn(async move {
        api.configure("fixture", ProviderOperation::ConfigurationOpen)
            .await
    });
    let command = h.next_command(ProviderOperation::ConfigurationOpen).await;
    let window_id = command.target_operation_id.clone();
    let host = h.client.host.for_command(&command).unwrap();
    host.finish(configuration_result(&command)).await.unwrap();
    opening.await.unwrap().unwrap();
    (window_id, host)
}

#[tokio::test]
async fn revoking_configure_requests_close_and_removes_its_grant() {
    let mut h = Harness::third_party_config("account").await;
    let (window_id, _) = open_configuration_window(&mut h).await;

    let api = h.api.clone();
    let revoking = tokio::spawn(async move {
        api.revoke_session_permission("fixture", cap::CONFIGURE)
            .await;
    });
    let close = h.next_command(ProviderOperation::ConfigurationClose).await;
    assert_eq!(close.target_operation_id, window_id);
    h.client
        .host
        .for_command(&close)
        .unwrap()
        .finish(configuration_result(&close))
        .await
        .unwrap();
    revoking.await.unwrap();

    let permissions = h.client.host.get_permissions().await.unwrap();
    assert!(
        !permissions
            .grants
            .iter()
            .any(|grant| grant.permission_id == cap::CONFIGURE && grant.state == "granted")
    );
    assert_eq!(
        h.api
            .configure("fixture", ProviderOperation::ConfigurationOpen)
            .await
            .unwrap_err()
            .code(),
        tonic::Code::PermissionDenied
    );
}

#[tokio::test]
async fn switching_verified_accounts_closes_the_old_window_and_fences_its_context() {
    let mut h = Harness::third_party_config("account").await;
    let (window_id, old_host) = open_configuration_window(&mut h).await;
    let old_resources = old_host.resources().unwrap();
    assert!(old_resources.library_snapshot("").await.is_ok());

    h.api.profiles().update(Some("next-account"), true);
    assert_eq!(
        old_resources.library_snapshot("").await.unwrap_err().code(),
        tonic::Code::FailedPrecondition
    );
    let api = h.api.clone();
    let fencing = tokio::spawn(async move { api.fence_third_party_sessions().await });
    let close = h.next_command(ProviderOperation::ConfigurationClose).await;
    assert_eq!(close.target_operation_id, window_id);
    h.client
        .host
        .for_command(&close)
        .unwrap()
        .finish(configuration_result(&close))
        .await
        .unwrap();
    fencing.await.unwrap();

    assert!(matches!(
        old_host.get_permissions().await,
        Err(gamenode_plugin_sdk::DetailedError::Unauthenticated)
    ));
}

#[tokio::test]
async fn signing_out_closes_the_window_before_ending_the_plugin_session() {
    let mut h = Harness::third_party_config("account").await;
    let (window_id, _) = open_configuration_window(&mut h).await;

    h.api.profiles().update(None, false);
    let api = h.api.clone();
    let fencing = tokio::spawn(async move { api.fence_third_party_sessions().await });
    let close = h.next_command(ProviderOperation::ConfigurationClose).await;
    assert_eq!(close.target_operation_id, window_id);
    h.client
        .host
        .for_command(&close)
        .unwrap()
        .finish(configuration_result(&close))
        .await
        .unwrap();
    fencing.await.unwrap();
    assert!(!h.api.connected("fixture"));
}

async fn assert_managed_stop_closes_window(stop_plugin: bool) {
    let mut h = Harness::third_party_config("account").await;
    let (window_id, old_host) = open_configuration_window(&mut h).await;
    let processes = crate::plugin_host::process::ProviderProcesses::new(
        h.api.clone(),
        h._temp.path().join("unused-plugin-endpoint"),
        vec![],
    )
    .unwrap();
    let stop = tokio::spawn(async move {
        if stop_plugin {
            processes.stop_plugin("fixture.plugin").await;
        } else {
            processes.stop_provider("fixture").await;
        }
    });

    let close = h.next_command(ProviderOperation::ConfigurationClose).await;
    assert_eq!(close.target_operation_id, window_id);
    h.client
        .host
        .for_command(&close)
        .unwrap()
        .finish(configuration_result(&close))
        .await
        .unwrap();
    stop.await.unwrap();

    h.api.disconnect("fixture").await;
    assert!(matches!(
        old_host.get_permissions().await,
        Err(gamenode_plugin_sdk::DetailedError::Unauthenticated)
    ));
}

#[tokio::test]
async fn disabling_or_removing_through_provider_stop_closes_configuration_first() {
    // Disable and removal use stop_provider; the persisted lifecycle then disconnects the
    // session, so a stale plugin-side configuration context cannot keep using its grants.
    assert_managed_stop_closes_window(false).await;
}

#[tokio::test]
async fn update_through_plugin_stop_closes_configuration_first() {
    // Staged update uses stop_plugin, which routes each registered provider through the same
    // close-before-stop path before replacing the process configuration.
    assert_managed_stop_closes_window(true).await;
}

#[tokio::test]
async fn configure_reuses_one_host_handle_for_open_focus_and_close() {
    let mut h = Harness::third_party_config("account").await;

    let api = h.api.clone();
    let opening = tokio::spawn(async move {
        api.configure("fixture", ProviderOperation::ConfigurationOpen)
            .await
    });
    let open_command = h.next_command(ProviderOperation::ConfigurationOpen).await;
    let handle = open_command.target_operation_id.clone();
    assert!(!handle.is_empty());
    h.client
        .host
        .for_command(&open_command)
        .unwrap()
        .finish(configuration_result(&open_command))
        .await
        .unwrap();
    opening.await.unwrap().unwrap();

    let api = h.api.clone();
    let focusing = tokio::spawn(async move {
        api.configure("fixture", ProviderOperation::ConfigurationOpen)
            .await
    });
    let focus_command = h.next_command(ProviderOperation::ConfigurationFocus).await;
    assert_eq!(focus_command.target_operation_id, handle);
    h.client
        .host
        .for_command(&focus_command)
        .unwrap()
        .finish(configuration_result(&focus_command))
        .await
        .unwrap();
    focusing.await.unwrap().unwrap();

    let api = h.api.clone();
    let closing = tokio::spawn(async move {
        api.configure("fixture", ProviderOperation::ConfigurationClose)
            .await
    });
    let close_command = h.next_command(ProviderOperation::ConfigurationClose).await;
    assert_eq!(close_command.target_operation_id, handle);
    h.client
        .host
        .for_command(&close_command)
        .unwrap()
        .finish(configuration_result(&close_command))
        .await
        .unwrap();
    closing.await.unwrap().unwrap();
}

#[tokio::test]
async fn rejected_focus_clears_stale_handle_for_the_next_configure_open() {
    let mut h = Harness::third_party_config("account").await;
    let (old_handle, _) = open_configuration_window(&mut h).await;

    let api = h.api.clone();
    let focusing = tokio::spawn(async move {
        api.configure("fixture", ProviderOperation::ConfigurationFocus)
            .await
    });
    let focus_command = h.next_command(ProviderOperation::ConfigurationFocus).await;
    h.client
        .host
        .for_command(&focus_command)
        .unwrap()
        .finish(OperationResult {
            disposition: OperationDisposition::Failed as i32,
            configuration: Some(ConfigurationResult {
                session_id: focus_command.context.as_ref().unwrap().session_id.clone(),
                profile_id: super::super::profile::profile_id_for_account("account"),
                window_id: old_handle.clone(),
                accepted: false,
                failure: "window-closed".into(),
            }),
            ..Default::default()
        })
        .await
        .unwrap();
    focusing.await.unwrap().unwrap();

    let api = h.api.clone();
    let opening = tokio::spawn(async move {
        api.configure("fixture", ProviderOperation::ConfigurationOpen)
            .await
    });
    let open_command = h.next_command(ProviderOperation::ConfigurationOpen).await;
    assert_ne!(open_command.target_operation_id, old_handle);
    h.client
        .host
        .for_command(&open_command)
        .unwrap()
        .finish(configuration_result(&open_command))
        .await
        .unwrap();
    opening.await.unwrap().unwrap();
}

#[tokio::test]
async fn configuration_ack_timeout_does_not_block_provider_operations() {
    let mut h = Harness::third_party_config("account").await;
    tokio::time::pause();
    let api = h.api.clone();
    let opening = tokio::spawn(async move {
        api.configure("fixture", ProviderOperation::ConfigurationOpen)
            .await
    });
    let _open_command = h.next_command(ProviderOperation::ConfigurationOpen).await;

    let discovery = h.execute(ProviderOperation::Discover);
    let discovery_host = h.next(ProviderOperation::Discover).await;
    discovery_host.finish(completed()).await.unwrap();
    discovery.await.unwrap().unwrap();

    tokio::time::advance(Duration::from_secs(5)).await;
    let error = opening.await.unwrap().unwrap_err();
    assert_eq!(error.code(), tonic::Code::DeadlineExceeded);
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
async fn ungranted_operation_is_denied_before_dispatch() {
    let h = Harness::new().await;
    let error = h
        .api
        .execute("fixture", ProviderOperation::ReadCounter)
        .await
        .unwrap_err();
    // Grant enforcement runs first: an operation outside the grant is denied, with details.
    assert_eq!(error.code(), tonic::Code::PermissionDenied);
    let detail = DenialDetail::decode(error.details()).unwrap();
    assert_eq!(detail.reason, DenialReason::NotGranted as i32);
    assert_eq!(detail.permission_id, cap::COUNTER);
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
                fields: Default::default(),
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
async fn catalog_chunks_keep_bounded_fields_for_core() {
    let mut h = Harness::new().await;
    let operation = h.execute(ProviderOperation::SyncCatalog);
    let client = h.next(ProviderOperation::SyncCatalog).await;
    let record = models::CatalogRecord::new("1", "A game")
        .with_field("namespace", "catalog-namespace")
        .unwrap()
        .with_field("catalog_item_id", "item-1")
        .unwrap();
    let completion = client
        .catalog_chunks(CatalogCategory::Owned, vec![record.clone()])
        .await
        .unwrap();
    let mut result = completed();
    result.catalogs = vec![CatalogCompletion {
        outcome: Some(catalog_completion::Outcome::CompletedRecords(1)),
        ..completion
    }];
    client.finish(result).await.unwrap();

    let completion = operation.await.unwrap().unwrap();
    let owned = completion.catalog.owned.unwrap().unwrap();
    assert_eq!(owned, vec![record]);
}

#[tokio::test]
async fn counter_dates_accept_absence_and_reject_negative_values() {
    let mut h = Harness::with_counter().await;
    let operation = h.execute(ProviderOperation::ReadCounter);
    let client = h.next(ProviderOperation::ReadCounter).await;
    let mut result = completed();
    result.counters = vec![CounterObservation {
        id: "42".into(),
        seconds: 60,
        last_played: None,
    }];
    client.finish(result).await.unwrap();
    let completion = operation.await.unwrap().unwrap();
    assert_eq!(completion.result.counters[0].last_played, None);

    let operation = h.execute(ProviderOperation::ReadCounter);
    let client = h.next(ProviderOperation::ReadCounter).await;
    let mut result = completed();
    result.counters = vec![CounterObservation {
        id: "42".into(),
        seconds: 60,
        last_played: Some(-1),
    }];
    client.finish(result).await.unwrap();
    assert_eq!(
        operation.await.unwrap().unwrap_err().code(),
        tonic::Code::InvalidArgument
    );
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

#[cfg(unix)]
#[tokio::test]
async fn provider_free_extension_gets_only_its_host_selected_launch_context() {
    use gamenode_plugin_protocol::proto::TargetGameContext;
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let endpoint = temp.path().join("extension.sock");
    let root = super::super::PluginApi::default();
    let api = root.providers.clone();
    api.profiles().update(Some("account-a"), true);
    let plugin_id = "org.example.launch-helper";
    let internal_id = super::extension_registration_id(plugin_id);
    api.register(Registration {
        provider_id: internal_id.clone(),
        plugin_id: plugin_id.into(),
        kind: RegistrationKind::Extension,
        origin: RegistrationOrigin::ThirdParty {
            publisher_fingerprint: "ef".repeat(32),
        },
        capabilities: vec![cap::GAME_CUSTOM_LAUNCH.into()],
        shared_catalog: false,
        services: Arc::new(Services::default()),
    })
    .unwrap();

    let launch = api.begin_launch(&internal_id).unwrap();
    let credential = (0..launch.credential.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&launch.credential[index..index + 2], 16).unwrap())
        .collect();
    let server =
        super::super::PluginApiServer::start(&tokio::runtime::Handle::current(), &endpoint, root)
            .unwrap();
    let session = gamenode_plugin_sdk::Session::open_extension(
        gamenode_plugin_sdk::Launch {
            endpoint: endpoint.clone(),
            credential,
            operation_id: "extension-launch".into(),
        },
        plugin_id,
        "1.0.0",
        TargetGameContext {
            game_id: "42".into(),
            installation_id: "steam:123".into(),
            profile_id: "account-a".into(),
            revision: "target-rev-1".into(),
        },
        &[cap::GAME_CUSTOM_LAUNCH],
    )
    .await
    .unwrap();
    launch.ready.await.unwrap();
    assert!(session.host.context().provider_id.is_empty());
    let resource_context = session.host.resource_context().unwrap();
    assert!(resource_context.provider_id.is_empty());
    assert_eq!(resource_context.target_game.as_ref().unwrap().game_id, "42");

    drop(session);
    api.disconnect(&internal_id).await;
    let launch = api.begin_launch(&internal_id).unwrap();
    let credential = (0..launch.credential.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&launch.credential[index..index + 2], 16).unwrap())
        .collect();
    let provider_session = gamenode_plugin_sdk::Session::open(
        gamenode_plugin_sdk::Launch {
            endpoint,
            credential,
            operation_id: "provider-impersonation".into(),
        },
        plugin_id,
        "steam",
        "1.0.0",
        &[cap::GAME_CUSTOM_LAUNCH],
    )
    .await;
    assert!(provider_session.is_err());
    drop(server);
}
