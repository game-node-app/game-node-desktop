//! Supervision for persistent provider processes. Executable configurations are
//! supplied only by trusted desktop composition, never by plugin RPCs.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gamenode_plugin_protocol::proto::ProviderOperation;
use tokio::sync::{Mutex, Notify, oneshot, watch};
use tonic::Status;

use super::provider::{Completion, ProviderApi};

pub struct ProcessConfig {
    pub provider_id: String,
    pub executable: PathBuf,
    pub arguments: Vec<OsString>,
    pub environment: Vec<(OsString, OsString)>,
    pub startup_timeout: Duration,
    /// Present for managed packages; publisher key comes from host trust state.
    pub integrity: Option<PackageIntegrity>,
}

#[derive(Clone)]
pub struct PackageIntegrity {
    pub package_root: PathBuf,
    pub publisher_key: [u8; 32],
    pub emulation_policy: gamenode_platform::plugin_environment::EmulationPolicy,
}

async fn verify_before_launch(config: &ProcessConfig) -> Result<(), Status> {
    let Some(integrity) = config.integrity.clone() else {
        return Ok(());
    };
    let executable = config.executable.clone();
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
        Ok(())
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
}

impl Drop for ChildProcess {
    fn drop(&mut self) {
        self.task.abort(); // dropping the supervised Child also kills it
    }
}

struct Slot {
    config: ProcessConfig,
    child: Mutex<Option<ChildProcess>>,
}

struct Inner {
    api: ProviderApi,
    endpoint: PathBuf,
    slots: HashMap<String, Slot>,
    shutdown: watch::Sender<bool>,
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
                    config,
                    child: Mutex::new(None),
                },
            );
        }
        let (shutdown, _) = watch::channel(false);
        Ok(Self {
            inner: Arc::new(Inner {
                api,
                endpoint,
                slots,
                shutdown,
            }),
        })
    }

    pub fn has_provider(&self, id: &str) -> bool {
        self.inner.slots.contains_key(id)
    }

    pub async fn ensure_started(&self, provider_id: &str) -> Result<(), Status> {
        if *self.inner.shutdown.borrow() {
            return Err(Status::unavailable("desktop is shutting down"));
        }
        let slot = self
            .inner
            .slots
            .get(provider_id)
            .ok_or_else(|| Status::not_found("provider process is not registered"))?;
        // A per-provider async lock joins concurrent startup requests without
        // blocking another provider or a native UI callback.
        let mut stored = slot.child.lock().await;
        if let Some(child) = stored.as_mut() {
            if child.running.load(Ordering::Acquire) && self.inner.api.connected(provider_id) {
                return Ok(());
            }
            if child.running.load(Ordering::Acquire) {
                if let Some(stop) = child.stop.take() {
                    let _ = stop.send(());
                }
                let stopped = child.stopped.notified();
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
        verify_before_launch(&slot.config).await?;
        let launch = self.inner.api.begin_launch(provider_id)?;
        let mut command = tokio::process::Command::new(&slot.config.executable);
        command
            .args(&slot.config.arguments)
            .envs(slot.config.environment.iter().cloned())
            .env("GAMENODE_PLUGIN_ENDPOINT", &self.inner.endpoint)
            .env("GAMENODE_PLUGIN_OPERATION_ID", "initialization")
            .env("GAMENODE_PLUGIN_SESSION_CREDENTIAL", &launch.credential)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
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
                startup_timeout: slot.config.startup_timeout,
                startup,
                stop: stop_receiver,
                shutdown: self.inner.shutdown.subscribe(),
                running: running.clone(),
                stopped: stopped.clone(),
            },
        ));
        *stored = Some(ChildProcess {
            pid,
            running,
            stopped,
            stop: Some(stop),
            task,
        });
        started
            .await
            .map_err(|_| Status::unavailable("provider startup failed"))?
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
            self.stop_provider(provider_id).await;
        }
        result
    }

    pub async fn stop_provider(&self, provider_id: &str) {
        if let Some(slot) = self.inner.slots.get(provider_id) {
            let mut child = slot.child.lock().await;
            if let Some(mut child) = child.take() {
                if let Some(stop) = child.stop.take() {
                    let _ = stop.send(());
                }
                let stopped = child.stopped.notified();
                if child.running.load(Ordering::Acquire) {
                    let _ = tokio::time::timeout(Duration::from_secs(4), stopped).await;
                }
            }
        }
    }

    pub async fn shutdown(&self) {
        self.inner.shutdown.send_replace(true);
        let _ = tokio::time::timeout(Duration::from_secs(4), async {
            for id in self.inner.slots.keys() {
                self.stop_provider(id).await;
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
    startup_timeout: Duration,
    startup: oneshot::Sender<Result<(), Status>>,
    stop: oneshot::Receiver<()>,
    shutdown: watch::Receiver<bool>,
    running: Arc<AtomicBool>,
    stopped: Arc<Notify>,
}

async fn supervise(mut child: tokio::process::Child, supervision: Supervision) {
    let Supervision {
        api,
        provider_id,
        mut ready,
        startup_timeout,
        startup,
        mut stop,
        mut shutdown,
        running,
        stopped,
    } = supervision;
    let opened = tokio::select! {
        ready = tokio::time::timeout(startup_timeout, &mut ready) => ready.is_ok_and(|r| r.is_ok()),
        _ = child.wait() => false,
        _ = &mut stop => false,
        _ = shutdown.changed() => false,
    };
    let _ = startup.send(if opened {
        Ok(())
    } else {
        Err(Status::unavailable(
            "provider did not establish its command session",
        ))
    });
    if opened {
        tokio::select! {
            _ = child.wait() => {},
            _ = &mut stop => {
                api.request_shutdown(&provider_id);
                let _ = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
            },
            _ = shutdown.changed() => {
                api.request_shutdown(&provider_id);
                let _ = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
            },
        }
    }
    if !matches!(child.try_wait(), Ok(Some(_))) {
        let _ = child.kill().await;
    }
    api.disconnect(&provider_id).await;
    running.store(false, Ordering::Release);
    stopped.notify_waiters();
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
