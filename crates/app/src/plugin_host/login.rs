//! Provider-neutral isolated login windows. Only trusted distribution metadata
//! supplies URLs, cookie names and a probe; runtime requests supply handles only.

use super::provider::Lease;
use gamenode_platform::paths::LOGIN_DIR;
use gamenode_plugin_protocol::{
    login::LoginPolicy,
    proto::{LoginOperation, LoginReply, LoginRequest, LoginState, ServiceContext},
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicI32, Ordering},
    },
    time::{Duration, Instant},
};
use tonic::Status;

mod native;
pub use native::TauriLoginWindows;

const MAX_RESULT: usize = 64 * 1024;
const MAX_WINDOWS: usize = 16;
const READ_INTERVAL: Duration = Duration::from_millis(500);

#[async_trait::async_trait]
pub trait LoginWindow: Send + Sync {
    async fn focus(&self) -> Result<(), Status>;
    async fn close(&self) -> Result<(), Status>;
    fn closed(&self) -> bool;
    async fn collect(&self, policy: Arc<LoginPolicy>) -> Result<LoginReply, Status>;
}

#[async_trait::async_trait]
pub trait LoginWindows: Send + Sync {
    async fn open(
        &self,
        label: String,
        profile: PathBuf,
        policy: Arc<LoginPolicy>,
    ) -> Result<Arc<dyn LoginWindow>, Status>;
}

#[derive(Clone, PartialEq, Eq)]
struct Scope {
    session: String,
    provider: String,
    operation: String,
    account: u64,
    profile: u64,
}

impl From<&ServiceContext> for Scope {
    fn from(c: &ServiceContext) -> Self {
        Self {
            session: c.session_id.clone(),
            provider: c.provider_id.clone(),
            operation: c.operation_id.clone(),
            account: c.account_epoch,
            profile: c.profile_epoch,
        }
    }
}

type Valid = Arc<dyn Fn() -> Result<(), Status> + Send + Sync>;
struct Attempt {
    scope: Scope,
    handle: String,
    profile: PathBuf,
    policy: Arc<LoginPolicy>,
    valid: Valid,
    state: AtomicI32,
    window: tokio::sync::Mutex<Option<Arc<dyn LoginWindow>>>,
    read: tokio::sync::Mutex<Option<Instant>>,
}

impl Attempt {
    fn state(&self) -> LoginState {
        LoginState::try_from(self.state.load(Ordering::Acquire)).unwrap_or(LoginState::Failed)
    }

    fn end(&self, state: LoginState) {
        let _ = self.state.compare_exchange(
            LoginState::Open as i32,
            state as i32,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    fn reply(&self) -> LoginReply {
        LoginReply {
            handle: self.handle.clone(),
            state: self.state() as i32,
            cookies: vec![],
            page_result_json: None,
        }
    }

    async fn cleanup(&self) {
        let mut slot = self.window.lock().await;
        if let Some(window) = slot.as_ref() {
            // Retain the window until destruction is confirmed. Its profile must
            // never be reused while WebView2 still owns it.
            if window.close().await.is_err() {
                tracing::warn!("plugin login window cleanup failed");
            }
        }
        *slot = None;
        let profile = self.profile.clone();
        let handle = self.handle.clone();
        let removed =
            tokio::task::spawn_blocking(move || remove_owned_profile(&profile, &handle)).await;
        if !matches!(removed, Ok(Ok(_))) {
            tracing::warn!("plugin login profile cleanup failed");
        }
    }
}

/// One active attempt per provider; no profile path or page material is exposed
/// through snapshots. Construct once after acquiring profile ownership.
pub struct LoginBroker {
    root: PathBuf,
    windows: Arc<dyn LoginWindows>,
    attempts: Mutex<HashMap<(String, String), Arc<Attempt>>>,
}

impl LoginBroker {
    pub async fn new(
        private_dir: PathBuf,
        windows: Arc<dyn LoginWindows>,
    ) -> Result<Arc<Self>, Status> {
        Self::recover(private_dir, windows, vec![]).await
    }

    /// Upgrade-only profile aliases come from trusted distribution metadata.
    /// They are removed, never reused by a new plugin login attempt.
    pub async fn recover(
        private_dir: PathBuf,
        windows: Arc<dyn LoginWindows>,
        legacy_profiles: Vec<String>,
    ) -> Result<Arc<Self>, Status> {
        if legacy_profiles.iter().any(|name| {
            name.is_empty()
                || name == "."
                || name == ".."
                || name.contains(['/', '\\'])
                || name == LOGIN_DIR
        }) {
            return Err(Status::invalid_argument("invalid legacy profile alias"));
        }
        let root = private_dir.join(LOGIN_DIR);
        let recovery = root.clone();
        tokio::task::spawn_blocking(move || {
            private_directory(&private_dir)?;
            for alias in legacy_profiles {
                remove_path(&private_dir.join(alias))?;
            }
            // Never traverse a link left in the private profile namespace.
            remove_path(&recovery)?;
            private_directory(&recovery)
        })
        .await
        .map_err(|_| Status::unavailable("login profile unavailable"))?
        .map_err(|_| Status::unavailable("login profile unavailable"))?;
        Ok(Arc::new(Self {
            root,
            windows,
            attempts: Mutex::new(HashMap::new()),
        }))
    }

    pub async fn request(
        self: &Arc<Self>,
        lease: Lease,
        request: LoginRequest,
        policy: Arc<LoginPolicy>,
    ) -> Result<LoginReply, Status> {
        lease.check()?;
        let scope = Scope::from(&lease.context);
        let extracting = request.operation == LoginOperation::Read as i32;
        let needs_cookies = !policy.cookie_names.is_empty();
        let valid: Valid = Arc::new(move || {
            lease.check()?;
            if extracting {
                if needs_cookies {
                    lease.require(gamenode_plugin_protocol::capability::COOKIES)?;
                }
                lease.require(gamenode_plugin_protocol::capability::PAGE)?;
            }
            Ok(())
        });
        self.call(scope, valid, request, policy).await
    }

    async fn call(
        self: &Arc<Self>,
        scope: Scope,
        valid: Valid,
        request: LoginRequest,
        policy: Arc<LoginPolicy>,
    ) -> Result<LoginReply, Status> {
        valid()?;
        let operation = LoginOperation::try_from(request.operation)
            .map_err(|_| Status::invalid_argument("invalid login operation"))?;
        if operation == LoginOperation::Open {
            if !request.handle.is_empty() {
                return Err(Status::invalid_argument("unexpected login handle"));
            }
            let provider = profile_component(&scope.provider)?;
            policy
                .validate()
                .map_err(|_| Status::failed_precondition("login policy unavailable"))?;
            let handle = super::random_session_id()?;
            let attempt = Arc::new(Attempt {
                scope: scope.clone(),
                profile: self.root.join(provider),
                handle,
                policy,
                valid,
                state: AtomicI32::new(LoginState::Open as i32),
                window: tokio::sync::Mutex::new(None),
                read: tokio::sync::Mutex::new(None),
            });
            {
                let mut attempts = self
                    .attempts
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let key = (scope.session.clone(), scope.provider.clone());
                if attempts.values().any(|attempt| {
                    attempt.state() == LoginState::Open && attempt.scope.provider == scope.provider
                }) {
                    return Err(Status::already_exists("login already open"));
                }
                if !attempts.contains_key(&key) && attempts.len() >= MAX_WINDOWS {
                    return Err(Status::resource_exhausted("login limit reached"));
                }
                attempts.insert(key, attempt.clone());
            }
            let windows = self.windows.clone();
            let opened = attempt.clone();
            // A cancelled RPC must not abandon a native window halfway through
            // creation. This owner finishes initialization and always cleans up.
            let (sender, receiver) = tokio::sync::oneshot::channel();
            tokio::spawn(async move {
                let deadline = tokio::time::Instant::now()
                    + Duration::from_secs(opened.policy.timeout_seconds);
                let result = async {
                    let mut window = opened.window.lock().await;
                    (opened.valid)()?;
                    if opened.state() != LoginState::Open {
                        return Err(Status::cancelled("login cancelled"));
                    }
                    let path = opened.profile.clone();
                    let handle = opened.handle.clone();
                    tokio::task::spawn_blocking(move || prepare_profile(&path, &handle))
                        .await
                        .map_err(|_| Status::unavailable("login profile unavailable"))?
                        .map_err(|_| Status::unavailable("login profile unavailable"))?;
                    *window = Some(
                        windows
                            .open(
                                format!("login-{}", opened.handle),
                                opened.profile.clone(),
                                opened.policy.clone(),
                            )
                            .await?,
                    );
                    (opened.valid)()?;
                    if opened.state() != LoginState::Open {
                        return Err(Status::cancelled("login cancelled"));
                    }
                    Ok(opened.reply())
                }
                .await;
                if result.is_err() {
                    opened.end(LoginState::Failed);
                }
                if sender.send(result).is_err() {
                    opened.end(LoginState::Cancelled);
                }
                while opened.state() == LoginState::Open {
                    if (opened.valid)().is_err() {
                        opened.end(LoginState::Cancelled);
                    } else if tokio::time::Instant::now() >= deadline {
                        opened.end(LoginState::TimedOut);
                    } else if opened
                        .window
                        .lock()
                        .await
                        .as_ref()
                        .is_some_and(|w| w.closed())
                    {
                        opened.end(LoginState::Closed);
                    }
                    if opened.state() == LoginState::Open {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
                opened.cleanup().await;
            });
            return receiver
                .await
                .map_err(|_| Status::unavailable("login unavailable"))?;
        }
        if operation == LoginOperation::Unspecified {
            return Err(Status::invalid_argument("missing login operation"));
        }
        let attempt = self
            .attempts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&(scope.session.clone(), scope.provider.clone()))
            .cloned()
            .filter(|a| a.handle == request.handle && a.scope.profile == scope.profile)
            .ok_or_else(|| Status::permission_denied("invalid login handle"))?;
        // Control requests have their own operation ID. Only close/cancel may
        // clean up the previous account generation after invalidation.
        if !matches!(operation, LoginOperation::Cancel | LoginOperation::Close)
            && (attempt.scope.account != scope.account
                || (operation == LoginOperation::Read
                    && attempt.scope.operation != scope.operation))
        {
            return Err(Status::permission_denied("stale login handle"));
        }
        if matches!(operation, LoginOperation::Cancel | LoginOperation::Close) {
            attempt.end(if operation == LoginOperation::Cancel {
                LoginState::Cancelled
            } else {
                LoginState::Closed
            });
            attempt.cleanup().await;
            valid()?;
            return Ok(attempt.reply());
        }
        if attempt.state() != LoginState::Open {
            return Ok(attempt.reply());
        }
        (attempt.valid)()?;
        let window = attempt
            .window
            .lock()
            .await
            .clone()
            .ok_or_else(|| Status::unavailable("login opening"))?;
        if operation == LoginOperation::Focus {
            window.focus().await?;
            valid()?;
            return Ok(attempt.reply());
        }
        let mut read = attempt
            .read
            .try_lock()
            .map_err(|_| Status::resource_exhausted("login read in progress"))?;
        if read.is_some_and(|last| last.elapsed() < READ_INTERVAL) {
            return Err(Status::resource_exhausted("login read too frequent"));
        }
        *read = Some(Instant::now());
        let mut reply = tokio::time::timeout(
            Duration::from_secs(3),
            window.collect(attempt.policy.clone()),
        )
        .await
        .map_err(|_| Status::deadline_exceeded("login collection timed out"))??;
        valid()?;
        (attempt.valid)()?;
        if attempt.state() != LoginState::Open {
            return Ok(attempt.reply());
        }
        validate_collection(&reply, &attempt.policy)?;
        reply.handle = attempt.handle.clone();
        reply.state = LoginState::Open as i32;
        Ok(reply)
    }

    pub async fn session_closed(&self, session: &str) {
        let removed: Vec<_> = {
            let mut attempts = self
                .attempts
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let keys: Vec<_> = attempts
                .keys()
                .filter(|(s, _)| s == session)
                .cloned()
                .collect();
            keys.into_iter()
                .filter_map(|k| attempts.remove(&k))
                .collect()
        };
        for attempt in removed {
            attempt.end(LoginState::Cancelled);
            attempt.cleanup().await;
        }
    }
}

fn validate_collection(reply: &LoginReply, policy: &LoginPolicy) -> Result<(), Status> {
    let size = reply.page_result_json.as_ref().map_or(0, String::len)
        + reply
            .cookies
            .iter()
            .map(|c| c.name.len() + c.value.len())
            .sum::<usize>();
    if size > MAX_RESULT
        || reply.cookies.len() > policy.cookie_names.len()
        || reply.cookies.iter().enumerate().any(|(i, c)| {
            !policy.cookie_names.contains(&c.name)
                || c.value.chars().any(char::is_control)
                || reply.cookies[..i]
                    .iter()
                    .any(|previous| previous.name == c.name)
        })
        || reply
            .page_result_json
            .as_ref()
            .is_some_and(|json| serde_json::from_str::<serde_json::Value>(json).is_err())
    {
        return Err(Status::data_loss("invalid login collection"));
    }
    Ok(())
}

fn profile_component(provider: &str) -> Result<&str, Status> {
    if provider.is_empty() || provider == "." || provider == ".." || provider.contains(['/', '\\'])
    {
        return Err(Status::permission_denied("invalid login provider"));
    }
    Ok(provider)
}

const PROFILE_OWNER: &str = ".gamenode-owner";

/// Prepares the provider's ephemeral profile. A stale directory from an interrupted attempt is
/// removed first, and the owner marker lets a late cleanup avoid deleting a newer attempt.
fn prepare_profile(path: &Path, handle: &str) -> std::io::Result<()> {
    remove_path(path)?;
    private_directory(path)?;
    std::fs::write(path.join(PROFILE_OWNER), handle)
}

fn remove_owned_profile(path: &Path, handle: &str) -> std::io::Result<bool> {
    match std::fs::read_to_string(path.join(PROFILE_OWNER)) {
        Ok(owner) if owner.trim() == handle => {
            remove_with_retry(path)?;
            Ok(true)
        }
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            remove_with_retry(path)?;
            Ok(true)
        }
        Err(error) => Err(error),
    }
}

fn private_directory(path: &Path) -> std::io::Result<()> {
    if let Ok(metadata) = std::fs::symlink_metadata(path)
        && (!metadata.is_dir() || metadata.file_type().is_symlink())
    {
        return Err(std::io::Error::other("invalid private directory"));
    }
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn remove_path(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {
            std::fs::remove_dir_all(path)
        }
        Ok(_) => std::fs::remove_file(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

fn remove_with_retry(path: &Path) -> std::io::Result<()> {
    for _ in 0..9 {
        if remove_path(path).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    remove_path(path)
}

#[cfg(test)]
mod tests;
