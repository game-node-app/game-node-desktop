use super::*;
use crate::plugin_host::{
    PluginApi, PluginApiServer,
    provider::{ProviderApi, Registration},
};
use gamenode_platform::db::SqliteSettingsStore;
use gamenode_plugin_protocol::capability as cap;
use gamenode_plugin_sdk::{HostClient, Launch, Session};
use std::sync::atomic::{AtomicBool, Ordering};

struct FakeCredentials {
    value: Mutex<Option<SessionCredential>>,
    fail_read: AtomicBool,
    fail_write: AtomicBool,
    fail_delete: AtomicBool,
}
impl FakeCredentials {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            value: Mutex::new(SessionCredential::new("existing-cookie")),
            fail_read: false.into(),
            fail_write: false.into(),
            fail_delete: false.into(),
        })
    }
}
impl CredentialStore for FakeCredentials {
    fn method(&self) -> StorageMethod {
        StorageMethod::WindowsCredentialManager
    }
    fn load(&self) -> Result<Option<SessionCredential>, CredentialError> {
        if self.fail_read.load(Ordering::Relaxed) {
            Err(CredentialError::Locked)
        } else {
            Ok(lock(&self.value).clone())
        }
    }
    fn save(&self, value: &SessionCredential) -> Result<(), CredentialError> {
        if self.fail_write.load(Ordering::Relaxed) {
            Err(CredentialError::Failed)
        } else {
            *lock(&self.value) = Some(value.clone());
            Ok(())
        }
    }
    fn delete(&self) -> Result<(), CredentialError> {
        if self.fail_delete.load(Ordering::Relaxed) {
            Err(CredentialError::Failed)
        } else {
            *lock(&self.value) = None;
            Ok(())
        }
    }
}

struct Harness {
    api: ProviderApi,
    client: Session,
    services: Arc<ProviderServices>,
    library: Arc<dyn LibraryStore>,
    _server: PluginApiServer,
    _temp: tempfile::TempDir,
}

impl Harness {
    async fn new(credentials: Arc<dyn CredentialStore>) -> Self {
        let temp = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let library: Arc<dyn LibraryStore> = Arc::new(
            SqliteSettingsStore::open(&temp.path().join("library.db"))
                .unwrap()
                .library_store(),
        );
        let descriptor = crate::plugin_host::descriptor_for_provider("steam").unwrap();
        let services = Arc::new(
            ProviderServices::new(
                ProviderContext {
                    provider_id: "steam".into(),
                    platform: std::env::consts::OS.into(),
                    manifest_toml: descriptor.provider_manifest.into(),
                    granted_capabilities: cap::ALL.iter().map(|c| (*c).into()).collect(),
                    ..Default::default()
                },
                Some(credentials),
                library.clone(),
                None,
                descriptor.approved_login().unwrap(),
            )
            .unwrap(),
        );
        let root = PluginApi::default();
        let api = root.providers.clone();
        api.register(Registration {
            provider_id: "steam".into(),
            plugin_id: "fixture.steam".into(),
            capabilities: cap::ALL.iter().map(|c| (*c).into()).collect(),
            services: services.clone(),
        })
        .unwrap();
        let launch = api.begin_launch("steam").unwrap();
        let credential = launch
            .credential
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|s| u8::from_str_radix(std::str::from_utf8(s).unwrap(), 16).unwrap())
            .collect();
        #[cfg(target_os = "linux")]
        let server = PluginApiServer::start(
            &tokio::runtime::Handle::current(),
            temp.path().join("plugins.sock"),
            root,
        )
        .unwrap();
        #[cfg(windows)]
        let server =
            PluginApiServer::start_windows(&tokio::runtime::Handle::current(), root).unwrap();
        let client = Session::open(
            Launch {
                endpoint: server.endpoint().to_owned(),
                credential,
                operation_id: "launch".into(),
            },
            "fixture.steam",
            "steam",
            "1.0",
            cap::ALL,
        )
        .await
        .unwrap();
        launch.ready.await.unwrap();
        Self {
            api,
            client,
            services,
            library,
            _server: server,
            _temp: temp,
        }
    }

    async fn operation(
        &mut self,
        kind: ProviderOperation,
    ) -> (
        HostClient,
        tokio::task::JoinHandle<Result<super::super::provider::Completion, Status>>,
    ) {
        let api = self.api.clone();
        let pending = tokio::spawn(async move { api.execute("steam", kind).await });
        loop {
            let command = self.client.commands.message().await.unwrap().unwrap();
            if command.operation == ProviderOperation::Acknowledge as i32 {
                continue;
            }
            assert_eq!(command.operation, kind as i32);
            return (self.client.host.for_command(&command).unwrap(), pending);
        }
    }
}

fn completed() -> OperationResult {
    OperationResult {
        disposition: OperationDisposition::Completed as i32,
        ..Default::default()
    }
}
fn connected() -> models::AccountStatus {
    models::AccountStatus {
        state: models::AccountState::Connected,
        account_id: Some("123".into()),
        account_name: Some("player".into()),
        last_checked_at: Some(10),
        last_synced_at: None,
        storage: models::StorageMethod::PrivateFile,
        persist_failed: false,
        catalog_error: None,
        family_error: None,
    }
}

#[tokio::test]
async fn verified_identity_replacement_fences_received_and_queued_account_results() {
    let mut h = Harness::new(FakeCredentials::new()).await;
    let (old, old_operation) = h.operation(ProviderOperation::Check).await;
    let account = |id: &str| models::StoreAccount {
        provider: "steam".into(),
        account_id: id.into(),
        ..Default::default()
    };
    old.account_save(account("A")).await.unwrap();
    old.finish(completed()).await.unwrap();
    let received = old_operation.await.unwrap().unwrap();
    let (queued, queued_operation) = h.operation(ProviderOperation::Check).await;
    let (replacement, replacement_operation) = h.operation(ProviderOperation::SyncCatalog).await;
    replacement.account_save(account("B")).await.unwrap();
    assert!(matches!(
        queued.account_save(account("A")).await,
        Err(gamenode_plugin_sdk::Error::Stale)
    ));
    assert!(queued_operation.await.unwrap().is_err());
    assert_eq!(
        received
            .commit(|_, _| panic!("old identity cannot reach Core"))
            .await
            .unwrap_err()
            .code(),
        tonic::Code::FailedPrecondition
    );
    replacement.finish(completed()).await.unwrap();
    replacement_operation
        .await
        .unwrap()
        .unwrap()
        .commit(|_, _| ())
        .await
        .unwrap();
    assert_eq!(h.library.load_store_accounts().unwrap()[0].account_id, "B");
}

#[tokio::test]
async fn cancel_cannot_interrupt_an_accepted_verified_login_commit() {
    let credentials = FakeCredentials::new();
    let mut h = Harness::new(credentials.clone()).await;
    let (login, pending) = h.operation(ProviderOperation::SignIn).await;
    login
        .secret(
            SecretOperation::Write,
            "session",
            Some(&models::SessionCredential::new("verified-cookie").unwrap()),
        )
        .await
        .unwrap();
    let cancelled = h
        .api
        .execute("steam", ProviderOperation::CancelSignIn)
        .await
        .unwrap();
    assert_eq!(
        cancelled.result.disposition,
        OperationDisposition::Superseded as i32
    );
    login
        .account_save(models::StoreAccount {
            provider: "steam".into(),
            account_id: "verified".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    login.finish(completed()).await.unwrap();
    pending
        .await
        .unwrap()
        .unwrap()
        .commit(|_, _| ())
        .await
        .unwrap();
    assert_eq!(
        credentials.load().unwrap().unwrap().expose(),
        "verified-cookie"
    );
    assert_eq!(
        h.library.load_store_accounts().unwrap()[0].account_id,
        "verified"
    );
}

#[tokio::test]
async fn failed_credential_deletion_cannot_clear_identity_or_claim_sign_out() {
    let credentials = FakeCredentials::new();
    let mut h = Harness::new(credentials.clone()).await;
    h.services.publish_status(connected());
    let original = gamenode_core::library::StoreAccount {
        provider: "steam".into(),
        account_id: "123".into(),
        account_name: None,
        last_verified_at: None,
    };
    h.library.save_store_account(&original).unwrap();
    credentials.fail_delete.store(true, Ordering::Relaxed);
    let (client, pending) = h.operation(ProviderOperation::SignOut).await;
    let reply = client
        .secret(SecretOperation::Delete, "session", None)
        .await
        .unwrap();
    assert!(matches!(
        reply.outcome,
        Some(secret_reply::Outcome::Failed(_))
    ));
    assert!(matches!(
        client.account_clear().await,
        Err(gamenode_plugin_sdk::Error::Denied)
    ));
    client
        .finish(OperationResult {
            signed_out: true,
            ..completed()
        })
        .await
        .unwrap();
    assert!(pending.await.unwrap().is_err());
    assert_eq!(
        h.services.status().unwrap().state,
        models::AccountState::Connected
    );
    assert_eq!(h.library.load_store_accounts().unwrap(), vec![original]);
    assert!(credentials.load().unwrap().is_some());
}

#[tokio::test]
async fn real_account_storage_is_provider_scoped_and_actions_cannot_expand_templates() {
    let mut h = Harness::new(FakeCredentials::new()).await;
    let other = gamenode_core::library::StoreAccount {
        provider: "other".into(),
        account_id: "unrelated".into(),
        account_name: None,
        last_verified_at: None,
    };
    h.library.save_store_account(&other).unwrap();
    let (client, operation) = h.operation(ProviderOperation::Initialize).await;
    assert!(client.account_load().await.unwrap().is_none());
    let own = models::StoreAccount {
        provider: "steam".into(),
        account_id: "123".into(),
        account_name: Some("player".into()),
        last_verified_at: Some(10),
    };
    client.account_save(own.clone()).await.unwrap();
    assert_eq!(client.account_load().await.unwrap(), Some(own));
    assert!(matches!(
        client
            .account_save(models::StoreAccount {
                provider: "other".into(),
                account_id: "valid-but-unrelated".into(),
                ..Default::default()
            })
            .await,
        Err(gamenode_plugin_sdk::Error::Denied | gamenode_plugin_sdk::Error::Protocol)
    ));
    client
        .declare_actions(ActionDeclaration {
            play_uri: Some("steam://rungameid/{id}".into()),
            install_uri: None,
        })
        .await
        .unwrap();
    assert!(matches!(
        client
            .declare_actions(ActionDeclaration {
                play_uri: Some("file:///tmp/plugin-script".into()),
                install_uri: None
            })
            .await,
        Err(gamenode_plugin_sdk::Error::Denied)
    ));
    assert!(matches!(
        client.account_clear().await,
        Err(gamenode_plugin_sdk::Error::Denied)
    ));
    client.finish(completed()).await.unwrap();
    operation.await.unwrap().unwrap();
    let (client, operation) = h.operation(ProviderOperation::SignOut).await;
    assert!(matches!(
        client.account_clear().await,
        Err(gamenode_plugin_sdk::Error::Denied)
    ));
    client
        .secret(SecretOperation::Delete, "session", None)
        .await
        .unwrap();
    client.account_clear().await.unwrap();
    assert_eq!(h.library.load_store_accounts().unwrap(), vec![other]);
    client
        .finish(OperationResult {
            signed_out: true,
            ..completed()
        })
        .await
        .unwrap();
    operation.await.unwrap().unwrap();
}

fn stored(id: &str) -> gamenode_core::library::StoreAccount {
    gamenode_core::library::StoreAccount {
        provider: "steam".into(),
        account_id: id.into(),
        account_name: Some("player".into()),
        last_verified_at: Some(10),
    }
}

fn plugin_account(id: &str) -> models::StoreAccount {
    models::StoreAccount {
        provider: "steam".into(),
        account_id: id.into(),
        account_name: Some("player".into()),
        last_verified_at: Some(10),
    }
}

#[tokio::test]
async fn saving_a_different_account_with_an_empty_identity_cache_reports_a_change() {
    let mut h = Harness::new(FakeCredentials::new()).await;
    // The cache is empty (no prior read) but the library already holds a
    // different account from the previous run.
    h.library.save_store_account(&stored("previous")).unwrap();
    let (_pending, pending_operation) = h.operation(ProviderOperation::Check).await;
    let (replacement, replacement_operation) = h.operation(ProviderOperation::SyncCatalog).await;
    replacement
        .account_save(plugin_account("replacement"))
        .await
        .unwrap();
    // An identity change fences the account-scoped operation that was already
    // in flight; without the persisted-account lookup it would stay valid.
    assert!(pending_operation.await.unwrap().is_err());
    replacement.finish(completed()).await.unwrap();
    replacement_operation.await.unwrap().unwrap();
    assert_eq!(
        h.library.load_store_accounts().unwrap()[0].account_id,
        "replacement"
    );
}

#[tokio::test]
async fn saving_the_persisted_account_with_an_empty_identity_cache_reports_no_change() {
    let mut h = Harness::new(FakeCredentials::new()).await;
    h.library.save_store_account(&stored("same")).unwrap();
    let (pending, pending_operation) = h.operation(ProviderOperation::Check).await;
    let (same, same_operation) = h.operation(ProviderOperation::SyncCatalog).await;
    same.account_save(plugin_account("same")).await.unwrap();
    // The in-flight operation stays valid when the persisted identity did not
    // change, so it can complete normally.
    pending.finish(completed()).await.unwrap();
    pending_operation.await.unwrap().unwrap();
    same.finish(completed()).await.unwrap();
    same_operation.await.unwrap().unwrap();
    assert_eq!(
        h.library.load_store_accounts().unwrap()[0].account_id,
        "same"
    );
}

struct UnavailableAccounts;

impl AccountPersistence for UnavailableAccounts {
    fn load(&self, _: &str) -> Result<Option<models::StoreAccount>, Status> {
        Err(Status::unavailable("account storage unavailable"))
    }
    fn save(&self, _: models::StoreAccount, _: bool) -> Result<(), Status> {
        panic!("the previous identity must be read before saving")
    }
    fn clear(&self, _: &str) -> Result<(), Status> {
        panic!("the previous identity must be read before clearing")
    }
}

#[tokio::test]
async fn a_failed_previous_identity_read_is_reported_as_a_persistence_failure() {
    let mut h = Harness::new(FakeCredentials::new()).await;
    h.services
        .set_account_persistence(Arc::new(UnavailableAccounts));
    let (client, pending) = h.operation(ProviderOperation::Check).await;
    assert!(
        client
            .account_save(plugin_account("account"))
            .await
            .is_err()
    );
    h.services.publish_status(connected());
    assert!(h.services.status().unwrap().persist_failed);
    client.finish(completed()).await.unwrap();
    pending.await.unwrap().unwrap();
}

#[tokio::test]
async fn storage_outcomes_override_false_persistence_and_plaintext_claims() {
    let credentials = FakeCredentials::new();
    let mut h = Harness::new(credentials.clone()).await;
    let (client, operation) = h.operation(ProviderOperation::Check).await;
    credentials.fail_read.store(true, Ordering::Relaxed);
    let reply = client
        .secret(SecretOperation::Read, "session", None)
        .await
        .unwrap();
    assert!(
        matches!(reply.outcome, Some(secret_reply::Outcome::Failed(code)) if code == ServiceFailure::Locked as i32)
    );
    credentials.fail_read.store(false, Ordering::Relaxed);
    credentials.fail_write.store(true, Ordering::Relaxed);
    assert!(matches!(
        client
            .secret(
                SecretOperation::Write,
                "session",
                Some(&models::SessionCredential::new("new-cookie").unwrap())
            )
            .await
            .unwrap()
            .outcome,
        Some(secret_reply::Outcome::Failed(_))
    ));
    client
        .secret(SecretOperation::Read, "session", None)
        .await
        .unwrap();
    h.services.publish_status(connected());
    let status = h.services.status().unwrap();
    assert!(
        status.persist_failed,
        "reading an old cookie cannot erase a failed save"
    );
    assert_eq!(
        status.storage,
        models::StorageMethod::WindowsCredentialManager
    );
    assert!(!format!("{status:?}").contains("existing-cookie"));
    assert!(!format!("{status:?}").contains("new-cookie"));
    credentials.fail_write.store(false, Ordering::Relaxed);
    client
        .secret(
            SecretOperation::Write,
            "session",
            Some(&models::SessionCredential::new("rotated-cookie").unwrap()),
        )
        .await
        .unwrap();
    h.services.publish_status(connected());
    assert!(!h.services.status().unwrap().persist_failed);
    credentials.fail_delete.store(true, Ordering::Relaxed);
    assert!(matches!(
        client
            .secret(SecretOperation::Delete, "session", None)
            .await
            .unwrap()
            .outcome,
        Some(secret_reply::Outcome::Failed(_))
    ));
    assert_eq!(
        credentials.load().unwrap().unwrap().expose(),
        "rotated-cookie"
    );
    h.services.publish_status(connected());
    assert!(h.services.status().unwrap().persist_failed);
    assert_eq!(
        h.services.status().unwrap().state,
        models::AccountState::Connected
    );
    assert!(matches!(
        client.secret(SecretOperation::Read, "gamenode", None).await,
        Err(gamenode_plugin_sdk::Error::Denied | gamenode_plugin_sdk::Error::Protocol)
    ));
    client.finish(completed()).await.unwrap();
    operation.await.unwrap().unwrap();
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn legacy_private_credential_alias_is_used_without_copy_and_deleted_authoritatively() {
    use gamenode_platform::credentials::FileCredentialStore;
    use gamenode_platform::paths::DataPaths;
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path().to_owned());
    let gamenode = FileCredentialStore::new(paths.private_dir(), paths.credential_file());
    gamenode
        .save(&SessionCredential::new("GameNode-only").unwrap())
        .unwrap();
    let steam = Arc::new(FileCredentialStore::new(
        paths.private_dir(),
        paths.steam_credential_file(),
    ));
    steam
        .save(&SessionCredential::new("legacy-steam-cookie").unwrap())
        .unwrap();
    let mut h = Harness::new(steam).await;
    let (client, operation) = h.operation(ProviderOperation::Check).await;
    assert_eq!(
        client
            .secret(SecretOperation::Read, "session", None)
            .await
            .unwrap()
            .outcome,
        Some(secret_reply::Outcome::Value(
            b"legacy-steam-cookie".to_vec()
        ))
    );
    client
        .secret(
            SecretOperation::Write,
            "session",
            Some(&models::SessionCredential::new("rotated-cookie").unwrap()),
        )
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(paths.steam_credential_file()).unwrap(),
        "rotated-cookie"
    );
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(paths.steam_credential_file())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    client
        .secret(SecretOperation::Delete, "session", None)
        .await
        .unwrap();
    assert!(!paths.steam_credential_file().exists());
    assert!(matches!(
        client
            .secret(SecretOperation::Read, "session", None)
            .await
            .unwrap()
            .outcome,
        Some(secret_reply::Outcome::Absent(true))
    ));
    assert_eq!(gamenode.load().unwrap().unwrap().expose(), "GameNode-only");
    client.finish(completed()).await.unwrap();
    operation.await.unwrap().unwrap();
}
