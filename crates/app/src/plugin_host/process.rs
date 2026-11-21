//! Supervision for provider and scoped extension processes. Executable configurations are
//! supplied only by trusted desktop composition, never by plugin RPCs.

use std::collections::{BTreeSet, HashMap};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gamenode_plugin_protocol::proto::ProviderOperation;
use gamenode_plugin_protocol::proto::TargetGameContext;
use tokio::sync::{Mutex, Notify, oneshot, watch};
use tonic::Status;

use super::provider::{Completion, ProviderApi, SupervisedExtension};

/// Stable error returned when a provider was deliberately stopped for this run.
pub(crate) const PLUGIN_STOPPED: &str = "plugin stopped";

#[derive(Clone)]
pub struct ProcessConfig {
    pub provider_id: String,
    pub executable: PathBuf,
    pub arguments: Vec<OsString>,
    pub environment: Vec<(OsString, OsString)>,
    pub startup_timeout: Duration,
    /// Signed manifest permissions which the process is allowed to request.
    pub manifest_capabilities: Vec<String>,
    /// Required manifest permissions gate startup until the user has granted them.
    pub required_capabilities: Vec<String>,
    /// Update declarations whose previous grants have been revoked and need a fresh review.
    pub blocked_capabilities: Vec<String>,
    /// Present for managed packages; publisher key comes from host trust state.
    pub integrity: Option<PackageIntegrity>,
}

#[derive(Clone)]
pub struct PackageIntegrity {
    pub package_root: PathBuf,
    pub version: String,
    pub publisher_key: [u8; 32],
    pub emulation_policy: gamenode_platform::plugin_environment::EmulationPolicy,
}

async fn verify_before_launch(
    config: &ProcessConfig,
    extension_trust_dir: Option<PathBuf>,
) -> Result<Option<super::package::PlaytimeCapabilities>, Status> {
    let Some(integrity) = config.integrity.clone() else {
        return Ok(None);
    };
    let executable = config.executable.clone();
    let config_id = config.provider_id.clone();
    let environment = gamenode_platform::plugin_environment::detect()
        .await
        .map_err(|_| Status::unavailable("plugin execution environment could not be detected"))?;
    tokio::task::spawn_blocking(move || {
        let integrity_manifest = gamenode_platform::plugin_packages::verify_package_directory(
            &integrity.package_root,
            &integrity.publisher_key,
        )
        .map_err(|_| Status::failed_precondition("plugin package integrity check failed"))?;
        let manifest_path = integrity.package_root.join("plugin.json");
        let metadata = std::fs::metadata(&manifest_path)
            .map_err(|_| Status::failed_precondition("plugin manifest is unavailable"))?;
        if metadata.len() > 4 * 1024 * 1024 {
            return Err(Status::failed_precondition("plugin manifest is invalid"));
        }
        let manifest = std::fs::read(&manifest_path)
            .map_err(|_| Status::failed_precondition("plugin manifest is unavailable"))?;
        use sha2::Digest;
        let manifest_hash = format!("{:x}", sha2::Sha256::digest(&manifest));
        if integrity_manifest.files.0.get("plugin.json") != Some(&manifest_hash) {
            return Err(Status::failed_precondition(
                "plugin manifest changed during compatibility validation",
            ));
        }
        let root = std::fs::canonicalize(&integrity.package_root)
            .map_err(|_| Status::failed_precondition("plugin package is unavailable"))?;
        let target = std::fs::canonicalize(executable)
            .map_err(|_| Status::failed_precondition("plugin executable is unavailable"))?;
        if !target.starts_with(&root) || !target.is_file() {
            return Err(Status::failed_precondition(
                "plugin executable is outside its verified package",
            ));
        }
        let verdict = super::compatibility::assess_package(
            &root,
            &target,
            &manifest,
            &environment,
            integrity.emulation_policy,
        )
        .map_err(|_| Status::failed_precondition("plugin target compatibility is invalid"))?;
        if verdict.state != super::compatibility::CompatibilityState::Compatible {
            return Err(Status::failed_precondition(format!(
                "plugin target compatibility is {:?}: {:?}",
                verdict.state, verdict.reasons
            )));
        }
        let playtime = if let Some(plugin_id) = config_id.strip_prefix("extension:") {
            let trust = super::publisher_trust::PublisherTrustStore::open(
                extension_trust_dir
                    .ok_or_else(|| Status::failed_precondition("extension trust is unavailable"))?,
            )
            .map_err(|_| Status::failed_precondition("extension publisher trust is unavailable"))?;
            if !trust
                .is_pinned(plugin_id, &integrity.publisher_key)
                .unwrap_or(false)
            {
                return Err(Status::permission_denied(
                    "extension publisher is no longer trusted",
                ));
            }
            let manifest = super::package::Manifest::parse_json(&manifest).map_err(|_| {
                Status::failed_precondition("verified extension manifest is invalid")
            })?;
            let manifest = manifest.manifest();
            if manifest.id != plugin_id
                || manifest.kind != super::package::PluginKind::Extension
                || manifest.version != integrity.version
            {
                return Err(Status::failed_precondition(
                    "verified extension identity changed",
                ));
            }
            #[cfg(target_os = "linux")]
            {
                // The package registry copies verified bytes into a private directory.
                // Only the signed, selected entrypoint needs owner execute permission.
                use std::os::unix::fs::PermissionsExt;
                let file = std::fs::File::open(&target).map_err(|_| {
                    Status::failed_precondition("extension executable is unavailable")
                })?;
                let metadata = file.metadata().map_err(|_| {
                    Status::failed_precondition("extension executable is unavailable")
                })?;
                if metadata.permissions().mode() & 0o7777 != 0o700 {
                    file.set_permissions(std::fs::Permissions::from_mode(0o700))
                        .map_err(|_| {
                            Status::failed_precondition(
                                "extension executable permissions could not be set",
                            )
                        })?;
                }
            }
            Some(manifest.playtime)
        } else {
            None
        };
        Ok(playtime)
    })
    .await
    .map_err(|_| Status::unavailable("plugin integrity worker failed"))?
}

struct ChildProcess {
    pid: u32,
    running: Arc<AtomicBool>,
    stopped: Arc<Notify>,
    stop: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
    is_extension: bool,
}

/// A selection made by the host against the active library and account.
#[derive(Clone)]
pub struct ExtensionTarget {
    pub(crate) context: TargetGameContext,
    pub(crate) profile_epoch: u64,
    pub(crate) still_selected: Arc<dyn Fn() -> bool + Send + Sync>,
}

impl ExtensionTarget {
    pub(crate) fn is_current(&self) -> bool {
        (self.still_selected)()
    }
}

#[derive(Clone)]
pub(crate) struct ExtensionLaunch {
    pub target: ExtensionTarget,
    pub required_permission: &'static str,
}

pub(crate) struct ExtensionIntent {
    pub id: String,
    pub store: Arc<super::intents::Intents>,
}

impl Drop for ExtensionIntent {
    fn drop(&mut self) {
        self.store.revoke_issued(&self.id);
    }
}

async fn wait_for_invalid_target(api: &ProviderApi, target: Option<&ExtensionTarget>) {
    let Some(target) = target else {
        return std::future::pending().await;
    };
    let profiles = api.profiles();
    let mut changes = profiles.subscribe();
    let mut interval = tokio::time::interval(Duration::from_millis(250));
    loop {
        if !target.is_current()
            || profiles.current().is_none_or(|profile| {
                !profile.verified() || profile.epoch() != target.profile_epoch
            })
        {
            return;
        }
        tokio::select! {
            _ = changes.changed() => {},
            _ = interval.tick() => {},
        }
    }
}

/// Owns the OS-level process group assigned to a supervised plugin. Dropping this guard kills
/// remaining descendants even when the plugin's main process exits first.
#[cfg(target_os = "linux")]
struct ChildTree {
    process_group: rustix::process::Pid,
}

#[cfg(target_os = "linux")]
impl ChildTree {
    fn attach(pid: u32) -> Result<Self, Status> {
        rustix::process::Pid::from_raw(pid as i32)
            .map(|process_group| Self { process_group })
            .ok_or_else(|| Status::unavailable("plugin process group is unavailable"))
    }

    fn terminate(&self) {
        let _ =
            rustix::process::kill_process_group(self.process_group, rustix::process::Signal::TERM);
    }

    fn kill(&self) {
        let _ =
            rustix::process::kill_process_group(self.process_group, rustix::process::Signal::KILL);
    }
}

#[cfg(windows)]
struct ChildTree {
    job: isize,
}

#[cfg(windows)]
impl ChildTree {
    fn attach(pid: u32) -> Result<Self, Status> {
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
        };

        // Fail closed rather than run a plugin process the host cannot supervise as a tree.
        let process = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid) };
        if process.is_null() {
            return Err(Status::unavailable(
                "plugin process could not be supervised",
            ));
        }
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            unsafe { CloseHandle(process) };
            return Err(Status::unavailable(
                "plugin process job could not be created",
            ));
        }
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION as *const std::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        let assigned =
            configured != 0 && unsafe { AssignProcessToJobObject(job, process as HANDLE) != 0 };
        unsafe { CloseHandle(process) };
        if !assigned {
            unsafe { CloseHandle(job) };
            return Err(Status::unavailable(
                "plugin process tree could not be supervised",
            ));
        }
        Ok(Self { job: job as isize })
    }

    fn terminate(&self) {
        // The protocol shutdown request is the graceful path on Windows; the job is killed after
        // the bounded grace period when the plugin does not exit.
    }

    fn kill(&self) {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        unsafe {
            let _ = TerminateJobObject(self.job as windows_sys::Win32::Foundation::HANDLE, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for ChildTree {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        self.kill();
        unsafe { CloseHandle(self.job as HANDLE) };
    }
}

#[cfg(target_os = "linux")]
impl Drop for ChildTree {
    fn drop(&mut self) {
        self.kill();
    }
}

impl Drop for ChildProcess {
    fn drop(&mut self) {
        self.task.abort(); // dropping the supervised Child also kills it
    }
}

struct Slot {
    config: std::sync::Mutex<ProcessConfig>,
    child: Mutex<Option<ChildProcess>>,
}

struct Inner {
    api: ProviderApi,
    endpoint: PathBuf,
    slots: HashMap<String, Slot>,
    shutdown: watch::Sender<bool>,
    activation: std::sync::RwLock<Option<super::profile::PluginStateLayout>>,
    /// Providers the user stopped for this run. Work is refused with a stable error until an
    /// explicit start clears the marker.
    stopped: std::sync::Mutex<std::collections::BTreeSet<String>>,
}

#[derive(Clone)]
pub struct ProviderProcesses {
    inner: Arc<Inner>,
}

impl ProviderProcesses {
    pub fn new(
        api: ProviderApi,
        endpoint: PathBuf,
        configs: Vec<ProcessConfig>,
    ) -> Result<Self, Status> {
        Self::build(api, endpoint, configs, None, true)
    }

    pub(crate) fn new_for_lifecycle(
        api: ProviderApi,
        endpoint: PathBuf,
        configs: Vec<ProcessConfig>,
        activation: super::profile::PluginStateLayout,
    ) -> Result<Self, Status> {
        Self::build(api, endpoint, configs, Some(activation), false)
    }

    fn build(
        api: ProviderApi,
        endpoint: PathBuf,
        configs: Vec<ProcessConfig>,
        activation: Option<super::profile::PluginStateLayout>,
        watch_profile: bool,
    ) -> Result<Self, Status> {
        let mut slots = HashMap::new();
        for config in configs {
            if !api.registered(&config.provider_id)
                || config.startup_timeout.is_zero()
                || slots.contains_key(&config.provider_id)
            {
                return Err(Status::invalid_argument(
                    "invalid provider process configuration",
                ));
            }
            slots.insert(
                config.provider_id.clone(),
                Slot {
                    config: std::sync::Mutex::new(config),
                    child: Mutex::new(None),
                },
            );
        }
        let (shutdown, _) = watch::channel(false);
        let manager = Self {
            inner: Arc::new(Inner {
                api,
                endpoint,
                slots,
                shutdown,
                activation: std::sync::RwLock::new(activation),
                stopped: std::sync::Mutex::new(std::collections::BTreeSet::new()),
            }),
        };
        if watch_profile {
            manager.spawn_profile_shutdown()?;
        }
        Ok(manager)
    }

    /// Reap every third-party process when its account profile is fenced. The receiver does not
    /// keep the process manager alive, so dropping the host also ends this task.
    fn spawn_profile_shutdown(&self) -> Result<(), Status> {
        let provider_ids = self.inner.api.third_party_provider_ids();
        if provider_ids.is_empty() {
            return Ok(());
        }
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| {
            Status::failed_precondition("plugin process host requires an async runtime")
        })?;
        let api = self.inner.api.clone();
        let mut changes = api.profiles().subscribe();
        let manager = Arc::downgrade(&self.inner);
        runtime.spawn(async move {
            let mut epoch = api.profiles().epoch();
            while changes.changed().await.is_ok() {
                let current = api.profiles().epoch();
                if current == epoch {
                    continue;
                }
                epoch = current;
                let Some(inner) = manager.upgrade() else {
                    break;
                };
                let processes = ProviderProcesses { inner };
                for provider_id in &provider_ids {
                    processes.terminate_provider(provider_id).await;
                }
            }
        });
        Ok(())
    }

    pub fn has_provider(&self, id: &str) -> bool {
        self.inner.slots.contains_key(id)
    }

    pub(crate) fn extension_runtime_available(&self, slot_id: &str) -> bool {
        !self.inner.endpoint.as_os_str().is_empty()
            && !*self.inner.shutdown.borrow()
            && self.has_provider(slot_id)
    }

    pub async fn ensure_started(&self, provider_id: &str) -> Result<(), Status> {
        self.ensure_started_inner(provider_id, false, None, None, None)
            .await
    }

    /// Starts an extension only for a host-selected game and passes its one-use user intent to
    /// that child. Extension launches never join a previously running process session.
    pub(crate) async fn ensure_extension_started(
        &self,
        slot_id: &str,
        launch: ExtensionLaunch,
        intent: ExtensionIntent,
    ) -> Result<(), Status> {
        if intent.id.is_empty() || intent.id.len() > 128 {
            return Err(Status::invalid_argument("invalid user intent"));
        }
        self.ensure_started_inner(
            slot_id,
            false,
            Some(launch.target),
            Some((intent, launch.required_permission)),
            None,
        )
        .await
    }

    /// Negotiate an Extension session for readiness without issuing a user intent.
    pub(crate) async fn ensure_extension_ready_for_update(
        &self,
        slot_id: &str,
        target: ExtensionTarget,
        candidate: ProcessConfig,
    ) -> Result<(), Status> {
        if candidate.provider_id != slot_id || candidate.startup_timeout.is_zero() {
            return Err(Status::invalid_argument(
                "invalid extension readiness configuration",
            ));
        }
        self.ensure_started_inner(slot_id, true, Some(target), None, Some(candidate))
            .await
    }

    /// One-shot readiness check used only after explicit update approval. It may start a plugin
    /// whose account activation is disabled, but still requires an active profile and current
    /// grants. Callers must stop it again after readiness when the user had disabled it.
    pub(crate) async fn ensure_ready_for_update(&self, provider_id: &str) -> Result<(), Status> {
        self.ensure_started_inner(provider_id, true, None, None, None)
            .await
    }

    async fn ensure_started_inner(
        &self,
        provider_id: &str,
        allow_disabled: bool,
        extension: Option<ExtensionTarget>,
        action: Option<(ExtensionIntent, &'static str)>,
        candidate: Option<ProcessConfig>,
    ) -> Result<(), Status> {
        if *self.inner.shutdown.borrow() {
            return Err(Status::unavailable("desktop is shutting down"));
        }
        if self.is_stopped(provider_id) {
            return Err(Status::unavailable(PLUGIN_STOPPED));
        }
        let slot = self
            .inner
            .slots
            .get(provider_id)
            .ok_or_else(|| Status::not_found("provider process is not registered"))?;
        if extension.is_some() != self.inner.api.is_extension(provider_id) {
            return Err(Status::failed_precondition(
                "extension startup requires a selected game",
            ));
        }
        if extension.is_some() && self.inner.endpoint.as_os_str().is_empty() {
            return Err(Status::unavailable("extension runtime is unavailable"));
        }
        let activation_layout = self
            .inner
            .activation
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let third_party_identity = self.inner.api.third_party_identity(provider_id);
        let mut active_profile = None;
        let mut active_profile_epoch = None;
        if let Some((plugin_id, _)) = third_party_identity.as_ref() {
            let profile = self.inner.api.profiles().require()?;
            if let Some(target) = &extension
                && (!profile.verified()
                    || profile.epoch() != target.profile_epoch
                    || profile.profile_id() != target.context.profile_id
                    || !target.is_current()
                    || activation_layout.is_none())
            {
                return Err(Status::failed_precondition(
                    "selected extension target is unavailable",
                ));
            }
            active_profile_epoch = Some(profile.epoch());
            if !allow_disabled && let Some(layout) = activation_layout.as_ref() {
                let layout = layout.clone();
                let plugin = plugin_id.clone();
                let account = profile.clone();
                let enabled = tokio::task::spawn_blocking(move || {
                    layout
                        .open_activation(&account)
                        .map(|state| state.is_enabled(&plugin))
                        .map_err(|error| {
                            Status::internal(format!("plugin activation state: {error}"))
                        })
                })
                .await
                .map_err(|_| Status::internal("plugin activation worker failed"))??;
                if !enabled {
                    return Err(Status::permission_denied("third-party plugin is disabled"));
                }
            }
            active_profile = Some(profile);
        }
        // A per-provider async lock joins concurrent startup requests without
        // blocking another provider or a native UI callback.
        let mut stored = slot.child.lock().await;
        if let Some(child) = stored.as_mut() {
            if extension.is_none()
                && child.running.load(Ordering::Acquire)
                && self.inner.api.connected(provider_id)
            {
                return Ok(());
            }
            if child.running.load(Ordering::Acquire) {
                if let Some(stop) = child.stop.take() {
                    let _ = stop.send(());
                }
                let mut stopped = std::pin::pin!(child.stopped.notified());
                stopped.as_mut().enable();
                if child.running.load(Ordering::Acquire)
                    && tokio::time::timeout(Duration::from_secs(4), stopped)
                        .await
                        .is_err()
                {
                    return Err(Status::unavailable("provider cleanup is still in progress"));
                }
            }
        }
        stored.take();
        let config = candidate.unwrap_or_else(|| {
            slot.config
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        });
        if let (Some((plugin_id, publisher_fingerprint)), Some(profile), Some(layout)) = (
            third_party_identity,
            active_profile,
            activation_layout.as_ref(),
        ) {
            let grants_layout = layout.clone();
            let grants_profile = profile.clone();
            let identity = super::grants::PluginIdentity {
                plugin_id,
                publisher_fingerprint,
                profile_id: profile.profile_id().to_owned(),
            };
            let declared = config.manifest_capabilities.clone();
            let required = config.required_capabilities.clone();
            let blocked = config.blocked_capabilities.clone();
            let effective = tokio::task::spawn_blocking(move || {
                let grants = grants_layout
                    .open_grants(&grants_profile)
                    .map_err(|error| {
                        Status::internal(format!("plugin grants unavailable: {error}"))
                    })?;
                let effective = grants
                    .effective_grants(&identity)
                    .into_iter()
                    .filter(|grant| grant.state == super::grants::PermissionState::Granted)
                    .map(|grant| grant.permission_id)
                    .filter(|permission| !blocked.contains(permission))
                    .collect::<BTreeSet<_>>();
                let missing_required = required
                    .iter()
                    .find(|permission| !effective.contains(*permission));
                if missing_required.is_some() {
                    return Err(Status::permission_denied(
                        "required plugin permissions are not granted",
                    ));
                }
                let mut allowed = vec![gamenode_plugin_protocol::capability::CONTEXT.to_owned()];
                allowed.extend(
                    declared
                        .into_iter()
                        .filter(|permission| effective.contains(permission)),
                );
                allowed.sort();
                allowed.dedup();
                Ok(allowed)
            })
            .await
            .map_err(|_| Status::internal("plugin permission worker failed"))??;
            if action.as_ref().is_some_and(|(_, permission)| {
                !effective.iter().any(|granted| granted == *permission)
            }) {
                return Err(Status::permission_denied(
                    "extension action permission is not granted",
                ));
            }
            if !self.inner.api.profiles().is_current(profile.epoch()) {
                return Err(Status::failed_precondition(
                    "plugin profile changed during startup",
                ));
            }
            self.inner.api.set_third_party_capabilities(
                provider_id,
                &config.manifest_capabilities,
                &effective,
            )?;
        }
        let trust_dir = activation_layout.as_ref().map(|layout| layout.trust_dir());
        let playtime = verify_before_launch(&config, trust_dir).await?;
        if active_profile_epoch.is_some_and(|epoch| !self.inner.api.profiles().is_current(epoch)) {
            return Err(Status::failed_precondition(
                "plugin profile changed during startup",
            ));
        }
        let (intent, required_permission) = match action {
            Some((intent, permission)) => (Some(intent), Some(permission)),
            None => (None, None),
        };
        let intent_id = intent.as_ref().map(|intent| intent.id.clone());
        let launch = if let Some(target) = extension.as_ref() {
            self.inner.api.begin_supervised_extension(
                provider_id,
                SupervisedExtension {
                    target: target.clone(),
                    required_permission,
                    expected_version: config
                        .integrity
                        .as_ref()
                        .map(|integrity| integrity.version.clone()),
                    intent,
                    playtime,
                },
            )?
        } else {
            self.inner
                .api
                .begin_launch_for_profile(provider_id, active_profile_epoch)?
        };
        let mut command = tokio::process::Command::new(&config.executable);
        command
            .args(&config.arguments)
            .envs(config.environment.iter().cloned())
            .env("GAMENODE_PLUGIN_ENDPOINT", &self.inner.endpoint)
            .env("GAMENODE_PLUGIN_OPERATION_ID", "initialization")
            .env("GAMENODE_PLUGIN_SESSION_CREDENTIAL", &launch.credential)
            .env_remove("GAMENODE_PLUGIN_USER_INTENT_ID")
            .env_remove("GAMENODE_PLUGIN_TARGET_GAME_ID")
            .env_remove("GAMENODE_PLUGIN_TARGET_INSTALLATION_ID")
            .env_remove("GAMENODE_PLUGIN_TARGET_PROFILE_ID")
            .env_remove("GAMENODE_PLUGIN_TARGET_REVISION")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        if let Some(target) = extension.as_ref() {
            if let Some(intent) = intent_id {
                command.env("GAMENODE_PLUGIN_USER_INTENT_ID", intent);
            }
            command
                .env("GAMENODE_PLUGIN_TARGET_GAME_ID", &target.context.game_id)
                .env(
                    "GAMENODE_PLUGIN_TARGET_INSTALLATION_ID",
                    &target.context.installation_id,
                )
                .env(
                    "GAMENODE_PLUGIN_TARGET_PROFILE_ID",
                    &target.context.profile_id,
                )
                .env("GAMENODE_PLUGIN_TARGET_REVISION", &target.context.revision);
        }
        #[cfg(target_os = "linux")]
        command.process_group(0);
        #[cfg(windows)]
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
        let child = match command.spawn() {
            Ok(child) => child,
            Err(_) => {
                self.inner.api.disconnect(provider_id).await;
                return Err(Status::unavailable(
                    "packaged provider executable could not start",
                ));
            }
        };
        let pid = child
            .id()
            .ok_or_else(|| Status::unavailable("provider process exited"))?;
        let tree = match ChildTree::attach(pid) {
            Ok(tree) => tree,
            Err(error) => {
                let mut child = child;
                let _ = child.kill().await;
                self.inner.api.disconnect(provider_id).await;
                return Err(error);
            }
        };
        let running = Arc::new(AtomicBool::new(true));
        let stopped = Arc::new(Notify::new());
        let (stop, stop_receiver) = oneshot::channel();
        let (startup, started) = oneshot::channel();
        let task = tokio::spawn(supervise(
            child,
            Supervision {
                api: self.inner.api.clone(),
                provider_id: provider_id.into(),
                ready: launch.ready,
                closed: launch.closed,
                target: extension.clone(),
                startup_timeout: config.startup_timeout,
                startup,
                stop: stop_receiver,
                shutdown: self.inner.shutdown.subscribe(),
                running: running.clone(),
                stopped: stopped.clone(),
                tree,
            },
        ));
        *stored = Some(ChildProcess {
            pid,
            running,
            stopped,
            stop: Some(stop),
            task,
            is_extension: extension.is_some(),
        });
        started
            .await
            .map_err(|_| Status::unavailable("plugin startup failed"))?
    }

    pub async fn execute(
        &self,
        provider_id: &str,
        operation: ProviderOperation,
    ) -> Result<Completion, Status> {
        self.ensure_started(provider_id).await?;
        let result = self.inner.api.execute(provider_id, operation).await;
        if result
            .as_ref()
            .is_err_and(|e| e.code() == tonic::Code::DeadlineExceeded)
        {
            // Terminate a timed-out provider; its outstanding authorization and
            // login handles cannot survive into a later process session.
            self.terminate_provider(provider_id).await;
        }
        result
    }

    pub async fn execute_achievements(
        &self,
        provider_id: &str,
        request: gamenode_plugin_protocol::models::AchievementsRequest,
    ) -> Result<Completion, Status> {
        self.ensure_started(provider_id).await?;
        let result = self
            .inner
            .api
            .execute_achievements(provider_id, request)
            .await;
        if result
            .as_ref()
            .is_err_and(|e| e.code() == tonic::Code::DeadlineExceeded)
        {
            self.terminate_provider(provider_id).await;
        }
        result
    }

    /// Whether the user stopped this provider for the current run.
    pub fn is_stopped(&self, provider_id: &str) -> bool {
        self.inner
            .stopped
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(provider_id)
    }

    /// Stops the provider for this run: pending calls are refused with `plugin stopped` and a
    /// later `execute` cannot silently relaunch the process.
    pub async fn stop_provider(&self, provider_id: &str) {
        self.inner
            .stopped
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(provider_id.to_owned());
        self.terminate_provider(provider_id).await;
    }

    /// Starts a stopped provider again with a fresh process session. Its account is restored by
    /// the normal initialization path.
    pub async fn start_provider(&self, provider_id: &str) -> Result<(), Status> {
        self.inner
            .stopped
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(provider_id);
        self.ensure_started(provider_id).await
    }

    /// Ends the current process and waits for the supervised shutdown, without changing whether
    /// the provider is user-stopped.
    pub async fn terminate_provider(&self, provider_id: &str) {
        if !self.inner.api.is_extension(provider_id) {
            let _ = self
                .inner
                .api
                .close_configuration_before_stop(provider_id)
                .await;
        }
        if let Some(slot) = self.inner.slots.get(provider_id) {
            let mut child = slot.child.lock().await;
            if let Some(mut child) = child.take() {
                if child.is_extension {
                    self.inner.api.request_shutdown(provider_id);
                    self.inner.api.disconnect(provider_id).await;
                }
                if let Some(stop) = child.stop.take() {
                    let _ = stop.send(());
                }
                let mut stopped = std::pin::pin!(child.stopped.notified());
                stopped.as_mut().enable();
                if child.running.load(Ordering::Acquire) {
                    let _ = tokio::time::timeout(Duration::from_secs(4), stopped).await;
                }
            }
        }
    }

    /// Stop a package process before replacing its immutable verified executable configuration.
    /// The per-provider child lock fences an overlapping startup until the new config is visible.
    pub(crate) async fn replace_config(
        &self,
        provider_id: &str,
        config: ProcessConfig,
    ) -> Result<(), Status> {
        if config.provider_id != provider_id || config.startup_timeout.is_zero() {
            return Err(Status::invalid_argument(
                "invalid provider process configuration",
            ));
        }
        let slot = self
            .inner
            .slots
            .get(provider_id)
            .ok_or_else(|| Status::not_found("provider process is not registered"))?;
        let mut child = slot.child.lock().await;
        if let Some(child) = child.as_mut() {
            if child.is_extension {
                self.inner.api.request_shutdown(provider_id);
                self.inner.api.disconnect(provider_id).await;
            }
            if let Some(stop) = child.stop.take() {
                let _ = stop.send(());
            }
            let mut stopped = std::pin::pin!(child.stopped.notified());
            stopped.as_mut().enable();
            if child.running.load(Ordering::Acquire)
                && tokio::time::timeout(Duration::from_secs(4), stopped)
                    .await
                    .is_err()
            {
                return Err(Status::unavailable("provider cleanup is still in progress"));
            }
        }
        child.take();
        *slot
            .config
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = config;
        Ok(())
    }

    pub(crate) fn config(&self, provider_id: &str) -> Result<ProcessConfig, Status> {
        let slot = self
            .inner
            .slots
            .get(provider_id)
            .ok_or_else(|| Status::not_found("provider process is not registered"))?;
        Ok(slot
            .config
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone())
    }

    pub async fn stop_plugin(&self, plugin_id: &str) {
        for provider_id in self
            .inner
            .api
            .third_party_provider_ids_for_plugin(plugin_id)
        {
            self.terminate_provider(&provider_id).await;
        }
    }

    pub async fn shutdown(&self) {
        // Send correlated Close acknowledgements while provider command sessions are still
        // serviceable. Broadcasting shutdown first would let supervisors tear down those sessions
        // before the configuration windows could be fenced.
        let provider_ids: Vec<_> = self.inner.slots.keys().cloned().collect();
        for provider_id in &provider_ids {
            let _ = self
                .inner
                .api
                .close_configuration_before_stop(provider_id)
                .await;
        }
        self.inner.shutdown.send_replace(true);
        let _ = tokio::time::timeout(Duration::from_secs(4), async {
            for id in &provider_ids {
                self.terminate_provider(id).await;
            }
        })
        .await;
    }

    pub async fn process_id(&self, provider_id: &str) -> Option<u32> {
        self.inner
            .slots
            .get(provider_id)?
            .child
            .lock()
            .await
            .as_ref()
            .filter(|p| p.running.load(Ordering::Acquire))
            .map(|p| p.pid)
    }
}

struct Supervision {
    api: ProviderApi,
    provider_id: String,
    ready: oneshot::Receiver<()>,
    closed: oneshot::Receiver<()>,
    target: Option<ExtensionTarget>,
    startup_timeout: Duration,
    startup: oneshot::Sender<Result<(), Status>>,
    stop: oneshot::Receiver<()>,
    shutdown: watch::Receiver<bool>,
    running: Arc<AtomicBool>,
    stopped: Arc<Notify>,
    tree: ChildTree,
}

async fn supervise(mut child: tokio::process::Child, supervision: Supervision) {
    let Supervision {
        api,
        provider_id,
        mut ready,
        mut closed,
        target,
        startup_timeout,
        startup,
        mut stop,
        mut shutdown,
        running,
        stopped,
        tree,
    } = supervision;
    let invalid_target = wait_for_invalid_target(&api, target.as_ref());
    tokio::pin!(invalid_target);
    let opened = tokio::select! {
        biased;
        _ = &mut invalid_target => false,
        _ = &mut closed, if target.is_some() => false,
        _ = child.wait() => false,
        _ = &mut stop => false,
        _ = shutdown.changed() => false,
        ready = tokio::time::timeout(startup_timeout, &mut ready) => ready.is_ok_and(|r| r.is_ok()),
    };
    let _ = startup.send(if opened {
        Ok(())
    } else {
        Err(Status::unavailable(
            "plugin did not establish its command session",
        ))
    });
    if opened {
        let exited = tokio::select! {
            _ = child.wait() => true,
            _ = &mut closed, if target.is_some() => false,
            _ = &mut invalid_target => false,
            _ = tokio::time::sleep(Duration::from_secs(5 * 60)), if target.is_some() => false,
            _ = &mut stop => false,
            _ = shutdown.changed() => false,
        };
        if !exited {
            api.request_shutdown(&provider_id);
        }
        if target.is_some() {
            api.disconnect(&provider_id).await;
        }
        if !exited
            && tokio::time::timeout(Duration::from_secs(1), child.wait())
                .await
                .is_err()
        {
            tree.terminate();
            if tokio::time::timeout(Duration::from_secs(2), child.wait())
                .await
                .is_err()
            {
                tree.kill();
                let _ = child.wait().await;
            }
        }
    }
    // A cleanly exiting plugin may have left descendants behind. The process group/job is private
    // to this launch and cannot affect another plugin or the desktop process.
    tree.kill();
    if !matches!(child.try_wait(), Ok(Some(_))) {
        let _ = child.kill().await;
    }
    api.disconnect(&provider_id).await;
    running.store(false, Ordering::Release);
    stopped.notify_waiters();
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
