use super::*;
use gamenode_plugin_protocol::proto::LoginCookie;
use std::sync::atomic::{AtomicBool, AtomicUsize};

#[derive(Default)]
struct FakeWindow {
    closed: AtomicBool,
    focuses: AtomicUsize,
    collecting: tokio::sync::Notify,
    release: tokio::sync::Notify,
    delay: AtomicBool,
}

#[async_trait::async_trait]
impl LoginWindow for FakeWindow {
    async fn focus(&self) -> Result<(), Status> {
        self.focuses.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    async fn close(&self) -> Result<(), Status> {
        self.closed.store(true, Ordering::Release);
        Ok(())
    }
    fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
    async fn collect(&self, policy: Arc<LoginPolicy>) -> Result<LoginReply, Status> {
        if self.delay.load(Ordering::Acquire) {
            self.collecting.notify_one();
            self.release.notified().await;
        }
        Ok(LoginReply {
            cookies: if policy.cookie_names.is_empty() {
                Vec::new()
            } else {
                vec![LoginCookie {
                    name: "steamLoginSecure".into(),
                    value: "private-cookie".into(),
                }]
            },
            page_result_json: Some(
                r#"{"steam_id":"76561198000000001","token":"temporary-token"}"#.into(),
            ),
            ..Default::default()
        })
    }
}

#[derive(Clone, Default)]
struct CapturedLogs(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for CapturedLogs {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CapturedLogs {
    type Writer = CapturedLogs;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

struct FakeWindows(Arc<FakeWindow>);
#[async_trait::async_trait]
impl LoginWindows for FakeWindows {
    async fn open(
        &self,
        _: String,
        _: PathBuf,
        _: Arc<LoginPolicy>,
    ) -> Result<Arc<dyn LoginWindow>, Status> {
        Ok(self.0.clone())
    }
}

fn policy() -> Arc<LoginPolicy> {
    Arc::new(toml::from_str(include_str!("../../../../plugin-steam/assets/login.toml")).unwrap())
}
fn cookie_free_policy() -> Arc<LoginPolicy> {
    Arc::new(LoginPolicy {
        start_url: "https://store.example.test/login".into(),
        cookie_urls: vec![],
        probe_origin: "https://store.example.test".into(),
        title: "Fixture".into(),
        allowed_hosts: vec!["example.test".into()],
        cookie_names: vec![],
        probe_script: "null".into(),
        width: 600,
        height: 760,
        timeout_seconds: 600,
    })
}
fn scope() -> Scope {
    Scope {
        session: "s1".into(),
        provider: "p1".into(),
        operation: "op1".into(),
        account: 1,
        profile: 1,
    }
}
fn valid() -> Valid {
    Arc::new(|| Ok(()))
}
fn request(operation: LoginOperation, handle: &str) -> LoginRequest {
    LoginRequest {
        context: None,
        operation: operation as i32,
        handle: handle.into(),
    }
}

async fn fixture() -> (tempfile::TempDir, Arc<LoginBroker>, Arc<FakeWindow>) {
    let dir = tempfile::TempDir::new().unwrap();
    let window = Arc::new(FakeWindow::default());
    let broker = LoginBroker::new(
        dir.path().join("private"),
        Arc::new(FakeWindows(window.clone())),
    )
    .await
    .unwrap();
    (dir, broker, window)
}

#[tokio::test]
async fn collection_keeps_secrets_off_debug_and_scopes_window_handles() {
    let (_dir, broker, _) = fixture().await;
    let opened = broker
        .call(
            scope(),
            valid(),
            request(LoginOperation::Open, ""),
            policy(),
        )
        .await
        .unwrap();
    let reply = broker
        .call(
            scope(),
            valid(),
            request(LoginOperation::Read, &opened.handle),
            policy(),
        )
        .await
        .unwrap();
    assert!(
        reply
            .page_result_json
            .as_ref()
            .unwrap()
            .contains("temporary-token")
    );
    assert_eq!(reply.cookies[0].value, "private-cookie");
    let debug = format!("{reply:?}");
    assert!(!debug.contains("temporary-token"));
    assert!(!debug.contains("private-cookie"));
    for other in [
        Scope {
            session: "other".into(),
            ..scope()
        },
        Scope {
            provider: "other".into(),
            ..scope()
        },
        Scope {
            profile: 2,
            ..scope()
        },
        Scope {
            account: 2,
            ..scope()
        },
        Scope {
            operation: "other".into(),
            ..scope()
        },
    ] {
        assert_eq!(
            broker
                .call(
                    other,
                    valid(),
                    request(LoginOperation::Read, &opened.handle),
                    policy()
                )
                .await
                .unwrap_err()
                .code(),
            tonic::Code::PermissionDenied
        );
    }
    assert_eq!(
        broker
            .call(
                scope(),
                valid(),
                request(LoginOperation::Read, &opened.handle),
                policy()
            )
            .await
            .unwrap_err()
            .code(),
        tonic::Code::ResourceExhausted
    );
    broker.session_closed("s1").await;
}

#[tokio::test]
async fn focus_and_cancel_bypass_a_pending_read_and_discard_its_late_secrets() {
    let (_dir, broker, window) = fixture().await;
    let opened = broker
        .call(
            scope(),
            valid(),
            request(LoginOperation::Open, ""),
            policy(),
        )
        .await
        .unwrap();
    window.delay.store(true, Ordering::Release);
    let read_broker = broker.clone();
    let handle = opened.handle.clone();
    let pending = tokio::spawn(async move {
        read_broker
            .call(
                scope(),
                valid(),
                request(LoginOperation::Read, &handle),
                policy(),
            )
            .await
    });
    window.collecting.notified().await;
    let control = Scope {
        operation: "focus".into(),
        ..scope()
    };
    tokio::time::timeout(
        Duration::from_millis(200),
        broker.call(
            control,
            valid(),
            request(LoginOperation::Focus, &opened.handle),
            policy(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(window.focuses.load(Ordering::Relaxed), 1);
    let control = Scope {
        account: 2,
        operation: "cancel".into(),
        ..scope()
    };
    let cancelled = tokio::time::timeout(
        Duration::from_millis(200),
        broker.call(
            control,
            valid(),
            request(LoginOperation::Cancel, &opened.handle),
            policy(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(cancelled.state, LoginState::Cancelled as i32);
    window.release.notify_one();
    let late = pending.await.unwrap().unwrap();
    assert!(late.cookies.is_empty());
    assert!(late.page_result_json.is_none());
    assert!(!broker.root.join("p1").exists());
}

#[tokio::test]
async fn invalidated_operation_and_process_exit_close_and_remove_private_profiles() {
    let (_dir, broker, window) = fixture().await;
    let alive = Arc::new(AtomicBool::new(true));
    let alive_check = alive.clone();
    let check: Valid = Arc::new(move || {
        if alive_check.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(Status::cancelled("stale login"))
        }
    });
    let _opened = broker
        .call(scope(), check, request(LoginOperation::Open, ""), policy())
        .await
        .unwrap();
    let profile = broker.root.join("p1");
    assert!(profile.is_dir());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&profile).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    alive.store(false, Ordering::Release);
    tokio::time::timeout(Duration::from_secs(2), async {
        while profile.exists() || !window.closed() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    broker.session_closed("s1").await;
    assert!(broker.attempts.lock().unwrap().is_empty());
}

#[tokio::test]
async fn repeated_login_is_rejected_and_old_handle_cannot_reach_the_next_attempt() {
    let (_dir, broker, _) = fixture().await;
    let first = broker
        .call(
            scope(),
            valid(),
            request(LoginOperation::Open, ""),
            policy(),
        )
        .await
        .unwrap();
    assert_eq!(
        broker
            .call(
                scope(),
                valid(),
                request(LoginOperation::Open, ""),
                policy()
            )
            .await
            .unwrap_err()
            .code(),
        tonic::Code::AlreadyExists
    );
    broker
        .call(
            scope(),
            valid(),
            request(LoginOperation::Close, &first.handle),
            policy(),
        )
        .await
        .unwrap();
    let second = broker
        .call(
            scope(),
            valid(),
            request(LoginOperation::Open, ""),
            policy(),
        )
        .await
        .unwrap();
    assert_ne!(first.handle, second.handle);
    assert_eq!(
        broker
            .call(
                scope(),
                valid(),
                request(LoginOperation::Cancel, &first.handle),
                policy()
            )
            .await
            .unwrap_err()
            .code(),
        tonic::Code::PermissionDenied
    );
    broker.session_closed("s1").await;
}

#[tokio::test]
async fn crash_recovery_removes_abandoned_profiles_without_following_links() {
    let dir = tempfile::TempDir::new().unwrap();
    let private = dir.path().join("private");
    std::fs::create_dir_all(private.join("login/abandoned")).unwrap();
    std::fs::write(private.join("login/abandoned/Cookies"), "not logged").unwrap();
    let outside = dir.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("keep"), "keep").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, private.join("login/linked")).unwrap();
    let broker = LoginBroker::new(
        private,
        Arc::new(FakeWindows(Arc::new(FakeWindow::default()))),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read_dir(&broker.root).unwrap().count(), 0);
    assert!(outside.join("keep").is_file());
}

#[tokio::test]
async fn upgrade_removes_only_approved_legacy_profiles_without_following_links() {
    let dir = tempfile::tempdir().unwrap();
    let private = dir.path().join("private");
    std::fs::create_dir_all(private.join("legacy-login")).unwrap();
    std::fs::write(private.join("legacy-login/Cookies"), "legacy-secret").unwrap();
    std::fs::write(private.join("session-token"), "keep-session").unwrap();
    let windows = Arc::new(FakeWindows(Arc::new(FakeWindow::default())));
    LoginBroker::recover(private.clone(), windows.clone(), vec!["../outside".into()])
        .await
        .err()
        .unwrap();
    LoginBroker::recover(private.clone(), windows.clone(), vec![LOGIN_DIR.into()])
        .await
        .err()
        .unwrap();
    assert!(private.join("legacy-login/Cookies").is_file());
    LoginBroker::recover(
        private.clone(),
        windows.clone(),
        vec!["legacy-login".into()],
    )
    .await
    .unwrap();
    assert!(!private.join("legacy-login").exists());
    assert_eq!(
        std::fs::read_to_string(private.join("session-token")).unwrap(),
        "keep-session"
    );
    #[cfg(unix)]
    {
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("keep"), "keep").unwrap();
        std::os::unix::fs::symlink(&outside, private.join("legacy-login")).unwrap();
        LoginBroker::recover(private.clone(), windows, vec!["legacy-login".into()])
            .await
            .unwrap();
        assert!(!private.join("legacy-login").exists());
        assert!(outside.join("keep").is_file());
    }
}

#[test]
fn collections_reject_unapproved_duplicate_oversized_and_malformed_data_safely() {
    let mut reply = LoginReply {
        cookies: vec![LoginCookie {
            name: "other-profile-cookie".into(),
            value: "do-not-leak".into(),
        }],
        ..Default::default()
    };
    let error = validate_collection(&reply, &policy()).unwrap_err();
    assert!(!format!("{error:?}").contains("do-not-leak"));
    reply.cookies[0].name = "steamLoginSecure".into();
    assert!(validate_collection(&reply, &policy()).is_ok());
    reply.cookies.push(reply.cookies[0].clone());
    assert!(validate_collection(&reply, &policy()).is_err());
    reply.cookies.clear();
    reply.page_result_json = Some("{".into());
    assert!(validate_collection(&reply, &policy()).is_err());
    reply.page_result_json = Some(format!("\"{}\"", "x".repeat(MAX_RESULT)));
    assert!(validate_collection(&reply, &policy()).is_err());
}

#[test]
fn the_steam_policy_collects_the_store_and_refresh_cookies_together() {
    let policy = policy();
    assert!(
        policy
            .cookie_urls
            .iter()
            .any(|url| url.contains("store.steampowered.com"))
    );
    assert!(
        policy
            .cookie_urls
            .iter()
            .any(|url| url.contains("login.steampowered.com"))
    );
    let reply = LoginReply {
        cookies: vec![
            LoginCookie {
                name: "steamLoginSecure".into(),
                value: "access".into(),
            },
            LoginCookie {
                name: "sessionid".into(),
                value: "session".into(),
            },
            LoginCookie {
                name: "steamRefresh_steam".into(),
                value: "refresh".into(),
            },
        ],
        ..Default::default()
    };
    assert!(validate_collection(&reply, &policy).is_ok());
}

#[test]
fn remote_login_pages_have_no_frontend_capabilities() {
    let capabilities: serde_json::Value =
        serde_json::from_str(include_str!("../../../capabilities/default.json")).unwrap();
    assert_eq!(capabilities["windows"], serde_json::json!(["main"]));
    assert!(capabilities.get("remote").is_none());
}

#[tokio::test]
async fn timeout_and_user_close_expire_the_attempt_and_remove_profiles() {
    for user_close in [false, true] {
        let (_dir, broker, window) = fixture().await;
        let opened = broker
            .call(
                scope(),
                valid(),
                request(LoginOperation::Open, ""),
                policy(),
            )
            .await
            .unwrap();
        let profile = broker.root.join("p1");
        if user_close {
            window.closed.store(true, Ordering::Release);
        } else {
            tokio::time::pause();
            tokio::time::advance(Duration::from_secs(601)).await;
            tokio::time::resume();
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            while profile.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let reply = broker
            .call(
                scope(),
                valid(),
                request(LoginOperation::Read, &opened.handle),
                policy(),
            )
            .await
            .unwrap();
        assert_eq!(
            reply.state,
            if user_close {
                LoginState::Closed
            } else {
                LoginState::TimedOut
            } as i32
        );
        assert!(reply.cookies.is_empty());
        broker.session_closed("s1").await;
    }
}

#[tokio::test]
async fn successful_and_cancelled_attempts_remove_their_profiles() {
    for (operation, read) in [
        (LoginOperation::Close, true),
        (LoginOperation::Cancel, false),
    ] {
        let (_dir, broker, _) = fixture().await;
        let opened = broker
            .call(
                scope(),
                valid(),
                request(LoginOperation::Open, ""),
                policy(),
            )
            .await
            .unwrap();
        let profile = broker.root.join("p1");
        assert!(profile.is_dir());
        if read {
            let reply = broker
                .call(
                    scope(),
                    valid(),
                    request(LoginOperation::Read, &opened.handle),
                    policy(),
                )
                .await
                .unwrap();
            assert_eq!(reply.cookies[0].value, "private-cookie");
        }
        let ended = broker
            .call(
                scope(),
                valid(),
                request(operation, &opened.handle),
                policy(),
            )
            .await
            .unwrap();
        assert_eq!(
            ended.state,
            if operation == LoginOperation::Close {
                LoginState::Closed
            } else {
                LoginState::Cancelled
            } as i32
        );
        assert!(!profile.exists());
        broker.session_closed("s1").await;
    }
}

#[tokio::test]
async fn two_providers_use_separate_login_profiles() {
    let (_dir, broker, _) = fixture().await;
    let first = Scope {
        provider: "provider-a".into(),
        ..scope()
    };
    let second = Scope {
        provider: "provider-b".into(),
        ..scope()
    };
    let first_attempt = broker
        .call(
            first.clone(),
            valid(),
            request(LoginOperation::Open, ""),
            policy(),
        )
        .await
        .unwrap();
    let second_attempt = broker
        .call(
            second.clone(),
            valid(),
            request(LoginOperation::Open, ""),
            policy(),
        )
        .await
        .unwrap();
    let first_profile = broker.root.join("provider-a");
    let second_profile = broker.root.join("provider-b");
    assert!(first_profile.is_dir());
    assert!(second_profile.is_dir());
    assert_ne!(first_profile, second_profile);

    broker
        .call(
            first,
            valid(),
            request(LoginOperation::Cancel, &first_attempt.handle),
            policy(),
        )
        .await
        .unwrap();
    broker
        .call(
            second,
            valid(),
            request(LoginOperation::Cancel, &second_attempt.handle),
            policy(),
        )
        .await
        .unwrap();
    assert!(!first_profile.exists());
    assert!(!second_profile.exists());
    broker.session_closed("s1").await;
}

#[tokio::test]
async fn cookie_free_reads_return_only_the_probe_result() {
    let (_dir, broker, _) = fixture().await;
    let opened = broker
        .call(
            scope(),
            valid(),
            request(LoginOperation::Open, ""),
            cookie_free_policy(),
        )
        .await
        .unwrap();
    let reply = broker
        .call(
            scope(),
            valid(),
            request(LoginOperation::Read, &opened.handle),
            cookie_free_policy(),
        )
        .await
        .unwrap();
    assert!(reply.cookies.is_empty());
    assert!(
        reply
            .page_result_json
            .as_deref()
            .is_some_and(|json| json.contains("temporary-token"))
    );
    broker
        .call(
            scope(),
            valid(),
            request(LoginOperation::Close, &opened.handle),
            cookie_free_policy(),
        )
        .await
        .unwrap();
    broker.session_closed("s1").await;
}

#[tokio::test]
async fn probe_results_and_cookies_never_reach_logs_or_snapshots() {
    let logs = CapturedLogs::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(logs.clone())
        .with_max_level(tracing::Level::TRACE)
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);

    let (_dir, broker, _) = fixture().await;
    let opened = broker
        .call(
            scope(),
            valid(),
            request(LoginOperation::Open, ""),
            policy(),
        )
        .await
        .unwrap();
    let reply = broker
        .call(
            scope(),
            valid(),
            request(LoginOperation::Read, &opened.handle),
            policy(),
        )
        .await
        .unwrap();
    let snapshot = format!("{reply:?}");
    assert!(!snapshot.contains("temporary-token"));
    assert!(!snapshot.contains("private-cookie"));
    assert!(!snapshot.contains("76561198000000001"));

    drop(guard);
    let output = String::from_utf8(
        logs.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone(),
    )
    .unwrap();
    assert!(!output.contains("temporary-token"));
    assert!(!output.contains("private-cookie"));
    assert!(!output.contains("76561198000000001"));
    broker.session_closed("s1").await;
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn public_sdk_reaches_broker_only_with_extraction_grants_and_session_exit_cleans_up() {
    use crate::plugin_host::{
        PluginApi, PluginApiServer,
        provider::{HostServices, Registration},
    };
    use gamenode_plugin_protocol::{capability as cap, proto::*};
    use gamenode_plugin_sdk::{Launch, Session};
    struct Services(Arc<LoginBroker>, Arc<LoginPolicy>);
    #[tonic::async_trait]
    impl HostServices for Services {
        async fn context(&self, _: Lease) -> Result<ProviderContext, Status> {
            Err(Status::unimplemented("fixture"))
        }
        async fn account(
            &self,
            _: Lease,
            _: account_request::Action,
        ) -> Result<AccountReply, Status> {
            Err(Status::unimplemented("fixture"))
        }
        async fn secret(&self, _: Lease, _: SecretRequest) -> Result<SecretReply, Status> {
            Err(Status::unimplemented("fixture"))
        }
        async fn actions(&self, _: Lease, _: ActionDeclaration) -> Result<ActionReply, Status> {
            Err(Status::unimplemented("fixture"))
        }
        async fn login(&self, lease: Lease, request: LoginRequest) -> Result<LoginReply, Status> {
            self.0.request(lease, request, self.1.clone()).await
        }
        async fn session_closed(&self, session: &str) {
            self.0.session_closed(session).await;
        }
        fn login_reads_cookies(&self) -> bool {
            !self.1.cookie_names.is_empty()
        }
    }

    #[derive(Clone, Copy)]
    enum Expected {
        Denied,
        Cookies,
        ProbeOnly,
    }

    for (cookie_free, capabilities, expected) in [
        (false, vec![cap::CONTEXT, cap::LOGIN], Expected::Denied),
        (
            false,
            vec![cap::CONTEXT, cap::LOGIN, cap::COOKIES, cap::PAGE],
            Expected::Cookies,
        ),
        (
            true,
            vec![cap::CONTEXT, cap::LOGIN, cap::PAGE],
            Expected::ProbeOnly,
        ),
    ] {
        let (dir, broker, window) = fixture().await;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let root = PluginApi::default();
        let api = root.providers.clone();
        let login_policy = if cookie_free {
            cookie_free_policy()
        } else {
            policy()
        };
        api.register(Registration {
            provider_id: "fixture".into(),
            plugin_id: "fixture.login".into(),
            capabilities: capabilities.iter().map(|c| (*c).to_owned()).collect(),
            services: Arc::new(Services(broker.clone(), login_policy)),
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
        let endpoint = dir.path().join("plugins.sock");
        let server =
            PluginApiServer::start(&tokio::runtime::Handle::current(), &endpoint, root).unwrap();
        let mut session = Session::open(
            Launch {
                endpoint,
                credential,
                operation_id: "launch".into(),
            },
            "fixture.login",
            "fixture",
            "1.0",
            &capabilities,
        )
        .await
        .unwrap();
        launch.ready.await.unwrap();
        let worker_api = api.clone();
        let pending = tokio::spawn(async move {
            worker_api
                .execute("fixture", ProviderOperation::SignIn)
                .await
        });
        let command = session.commands.message().await.unwrap().unwrap();
        let client = session.host.for_command(&command).unwrap();
        let opened = client.login(LoginOperation::Open, "").await.unwrap();
        let read = client.login(LoginOperation::Read, &opened.handle).await;
        match expected {
            Expected::Denied => {
                assert!(matches!(read, Err(gamenode_plugin_sdk::Error::Denied)));
            }
            Expected::Cookies => {
                assert_eq!(read.unwrap().cookies[0].value, "private-cookie");
            }
            Expected::ProbeOnly => {
                let read = read.unwrap();
                assert!(read.cookies.is_empty());
                assert!(read.page_result_json.is_some());
            }
        }
        api.disconnect("fixture").await;
        assert!(pending.await.unwrap().is_err());
        assert!(window.closed());
        assert!(!broker.root.join("fixture").exists());
        server.shutdown().await;
    }
}
