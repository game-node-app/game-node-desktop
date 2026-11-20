use std::{path::PathBuf, process::Stdio, sync::Arc, time::Duration};

use gamenode_plugin_sdk::{
    Durability, Error, HostClient, Launch, ProviderPlugin, Session, Status, capability, proto::*,
    resources::Outcome,
};
use tokio::{
    process::{Child, Command},
    sync::Mutex,
    time::timeout,
};

const PREFERENCE_KEY: &str = "preferred-catalog-label";
struct ExamplePlugin {
    window: Arc<Mutex<Option<OpenWindow>>>,
    launcher: Arc<dyn WindowLauncher>,
}

impl Default for ExamplePlugin {
    fn default() -> Self {
        Self::with_launcher(Arc::new(SystemWindowLauncher))
    }
}

impl ExamplePlugin {
    fn with_launcher(launcher: Arc<dyn WindowLauncher>) -> Self {
        Self {
            window: Arc::new(Mutex::new(None)),
            launcher,
        }
    }
}

struct OpenWindow {
    id: String,
    process: Child,
}

#[async_trait::async_trait]
trait PreferenceStore: Send + Sync {
    async fn get(&self) -> Result<(Option<Vec<u8>>, u64), Status>;
    async fn put(&self, revision: u64, value: Vec<u8>) -> Result<bool, Status>;
}

struct HostPreferenceStore(HostClient);

#[async_trait::async_trait]
impl PreferenceStore for HostPreferenceStore {
    async fn get(&self) -> Result<(Option<Vec<u8>>, u64), Status> {
        let resources = self
            .0
            .resources()
            .ok_or_else(|| Status::failed_precondition("API 1.6 storage is unavailable"))?;
        let stored = resources.storage_get(PREFERENCE_KEY).await?;
        Ok((stored.found.then_some(stored.value), stored.revision))
    }

    async fn put(&self, revision: u64, value: Vec<u8>) -> Result<bool, Status> {
        let resources = self
            .0
            .resources()
            .ok_or_else(|| Status::failed_precondition("API 1.6 storage is unavailable"))?;
        let write = resources
            .storage_put(
                PREFERENCE_KEY,
                revision,
                &gamenode_plugin_sdk::idempotency_key(),
                value,
                Durability::Persistent,
                None,
            )
            .await?;
        Ok(write.outcome == Outcome::RevisionConflict)
    }
}

#[async_trait::async_trait]
trait WindowLauncher: Send + Sync {
    fn start(&self, preference: &str) -> Result<Child, String>;
    async fn focus(&self, pid: u32) -> bool;
}

struct SystemWindowLauncher;

#[async_trait::async_trait]
impl WindowLauncher for SystemWindowLauncher {
    fn start(&self, preference: &str) -> Result<Child, String> {
        start_window(preference)
    }

    async fn focus(&self, pid: u32) -> bool {
        focus(pid).await
    }
}

impl ExamplePlugin {
    async fn preference(store: &dyn PreferenceStore) -> Result<(String, u64), Status> {
        let (value, revision) = store.get().await?;
        Ok((
            value
                .map(|value| String::from_utf8_lossy(&value).into_owned())
                .unwrap_or_else(|| "Games".into()),
            revision,
        ))
    }

    async fn save_preference(store: &dyn PreferenceStore, value: Vec<u8>) -> Result<(), Status> {
        // Read immediately before the write so the host can reject a stale edit safely.
        let (_, mut revision) = store.get().await?;
        for _ in 0..2 {
            if !store.put(revision, value.clone()).await? {
                return Ok(());
            }
            (_, revision) = store.get().await?;
        }
        Err(Status::aborted("preference changed while saving"))
    }

    async fn show_or_focus(&self, command: ProviderCommand, host: HostClient) -> OperationResult {
        self.show_or_focus_with(command, Arc::new(HostPreferenceStore(host)))
            .await
    }

    async fn show_or_focus_with(
        &self,
        command: ProviderCommand,
        store: Arc<dyn PreferenceStore>,
    ) -> OperationResult {
        let target_id = command.target_operation_id.clone();
        if target_id.is_empty() {
            return configuration_result(command, false, "missing-window-id");
        }
        let mut window = self.window.lock().await;
        if let Some(current) = window.as_mut() {
            if current.process.try_wait().ok().flatten().is_none() {
                if current.id == target_id
                    && timeout(
                        Duration::from_secs(2),
                        self.launcher
                            .focus(current.process.id().unwrap_or_default()),
                    )
                    .await
                    .unwrap_or(false)
                {
                    return configuration_result(command, true, "");
                }
                return configuration_result(command, false, "window-already-open");
            }
            if let Some(old) = window.take() {
                if let Ok(output) = old.process.wait_with_output().await
                    && output.status.success()
                    && let Err(error) = timeout(
                        Duration::from_secs(2),
                        Self::save_preference(store.as_ref(), output.stdout),
                    )
                    .await
                    .unwrap_or_else(|_| Err(Status::deadline_exceeded("preference save timed out")))
                {
                    eprintln!("could not save plugin preference: {error}");
                }
            }
        }
        let (preference, _) =
            match timeout(Duration::from_secs(2), Self::preference(store.as_ref())).await {
                Ok(Ok(value)) => value,
                Ok(Err(error)) => {
                    return configuration_result(command, false, &error.code().to_string());
                }
                Err(_) => return configuration_result(command, false, "preference-read-timeout"),
            };
        match self.launcher.start(&preference) {
            Ok(process) => {
                *window = Some(OpenWindow {
                    id: target_id,
                    process,
                });
                let shared_window = self.window.clone();
                let saved_store = store.clone();
                let target_id = command.target_operation_id.clone();
                tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(Duration::from_millis(200)).await;
                        let finished = {
                            let mut current = shared_window.lock().await;
                            let Some(open) = current.as_mut().filter(|open| open.id == target_id)
                            else {
                                return;
                            };
                            if open.process.try_wait().ok().flatten().is_some() {
                                current.take()
                            } else {
                                None
                            }
                        };
                        if let Some(open) = finished {
                            if let Ok(output) = open.process.wait_with_output().await
                                && output.status.success()
                                && let Err(error) =
                                    Self::save_preference(saved_store.as_ref(), output.stdout).await
                            {
                                eprintln!("could not save plugin preference: {error}");
                            }
                            return;
                        }
                    }
                });
                configuration_result(command, true, "")
            }
            Err(error) => configuration_result(command, false, &error),
        }
    }

    async fn close(&self, command: ProviderCommand, host: HostClient) -> OperationResult {
        self.close_with(command, Arc::new(HostPreferenceStore(host)))
            .await
    }

    async fn close_with(
        &self,
        command: ProviderCommand,
        store: Arc<dyn PreferenceStore>,
    ) -> OperationResult {
        let mut window = self.window.lock().await;
        let Some(mut current) = window.take() else {
            return configuration_result(command, true, "");
        };
        if command.target_operation_id != current.id {
            *window = Some(current);
            return configuration_result(command, false, "unknown-window-id");
        }
        if current.process.try_wait().ok().flatten().is_none() {
            let _ = current.process.start_kill();
        }
        if let Ok(output) = timeout(Duration::from_secs(1), current.process.wait_with_output())
            .await
            .unwrap_or_else(|_| Err(std::io::Error::other("window close timed out")))
            && output.status.success()
            && let Err(error) = timeout(
                Duration::from_secs(2),
                Self::save_preference(store.as_ref(), output.stdout),
            )
            .await
            .unwrap_or_else(|_| Err(Status::deadline_exceeded("preference save timed out")))
        {
            eprintln!("could not save plugin preference: {error}");
        }
        configuration_result(command, true, "")
    }
}

#[async_trait::async_trait]
impl ProviderPlugin for ExamplePlugin {
    async fn handle(
        &self,
        command: ProviderCommand,
        _host: HostClient,
    ) -> Result<OperationResult, Error> {
        let operation =
            ProviderOperation::try_from(command.operation).map_err(|_| Error::Protocol)?;
        let result = match operation {
            ProviderOperation::Initialize | ProviderOperation::Check => OperationResult {
                disposition: OperationDisposition::Completed as i32,
                ..Default::default()
            },
            ProviderOperation::Discover => OperationResult {
                disposition: OperationDisposition::Completed as i32,
                discovery: Some(DiscoveryResult {
                    records: vec![],
                    ..Default::default()
                }),
                ..Default::default()
            },
            ProviderOperation::SyncCatalog => OperationResult {
                disposition: OperationDisposition::Completed as i32,
                catalogs: vec![CatalogCompletion {
                    category: 0,
                    outcome: Some(catalog_completion::Outcome::CompletedRecords(0)),
                }],
                ..Default::default()
            },
            ProviderOperation::ReadCounter => OperationResult {
                disposition: OperationDisposition::Completed as i32,
                counters: vec![],
                ..Default::default()
            },
            ProviderOperation::Shutdown => OperationResult {
                disposition: OperationDisposition::Completed as i32,
                ..Default::default()
            },
            _ => return Err(Error::Protocol),
        };
        Ok(result)
    }

    async fn configure(
        &self,
        command: ProviderCommand,
        host: HostClient,
    ) -> Result<OperationResult, Error> {
        Ok(
            match ProviderOperation::try_from(command.operation).map_err(|_| Error::Protocol)? {
                ProviderOperation::ConfigurationOpen => self.show_or_focus(command, host).await,
                ProviderOperation::ConfigurationFocus => {
                    let mut window = self.window.lock().await;
                    let Some(current) = window.as_mut() else {
                        return Ok(configuration_result(command, false, "window-closed"));
                    };
                    if current.id != command.target_operation_id
                        || current.process.try_wait().ok().flatten().is_some()
                    {
                        return Ok(configuration_result(command, false, "window-closed"));
                    }
                    let accepted = timeout(
                        Duration::from_secs(2),
                        self.launcher
                            .focus(current.process.id().unwrap_or_default()),
                    )
                    .await
                    .unwrap_or(false);
                    configuration_result(
                        command,
                        accepted,
                        if accepted { "" } else { "focus-failed" },
                    )
                }
                ProviderOperation::ConfigurationClose => self.close(command, host).await,
                _ => return Err(Error::Protocol),
            },
        )
    }

    async fn shutdown(&self) {
        if let Some(mut current) = self.window.lock().await.take() {
            let _ = current.process.start_kill();
            let _ = current.process.wait().await;
        }
    }
}

fn configuration_result(
    command: ProviderCommand,
    accepted: bool,
    failure: &str,
) -> OperationResult {
    let context = command.context.unwrap_or_default();
    OperationResult {
        disposition: (if accepted {
            OperationDisposition::Completed
        } else {
            OperationDisposition::Failed
        }) as i32,
        configuration: Some(ConfigurationResult {
            session_id: context.session_id,
            profile_id: String::new(),
            window_id: command.target_operation_id,
            accepted,
            failure: failure.into(),
        }),
        ..Default::default()
    }
}

fn helper(name: &str) -> PathBuf {
    if let Ok(executable) = std::env::current_exe() {
        for parent in executable.ancestors() {
            let packaged = parent.join("plugins/shared").join(name);
            if packaged.is_file() {
                return packaged;
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../shared")
        .join(name)
}

fn start_window(preference: &str) -> Result<Child, String> {
    #[cfg(target_os = "linux")]
    let child = Command::new("sh")
        .arg(helper("config-window.sh"))
        .arg(preference)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    #[cfg(target_os = "windows")]
    let child = Command::new("powershell.exe")
        .args(["-NoProfile", "-STA", "-File"])
        .arg(helper("config-window.ps1"))
        .arg(preference)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    let child: Result<Child, std::io::Error> = Err(std::io::Error::other("unsupported OS"));
    child.map_err(|error| format!("could-not-open-window:{error}"))
}

async fn focus(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    let status = Command::new("powershell.exe")
        .args(["-NoProfile", "-File"])
        .arg(helper("focus-window.ps1"))
        .arg(pid.to_string())
        .status()
        .await;
    #[cfg(target_os = "linux")]
    let status = async {
        let output = Command::new("wmctrl").arg("-lp").output().await.ok()?;
        let windows = String::from_utf8_lossy(&output.stdout);
        let id = windows.lines().find_map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            (fields.get(2).and_then(|field| field.parse::<u32>().ok()) == Some(pid))
                .then(|| fields.first().copied())
                .flatten()
        })?;
        Command::new("wmctrl").args(["-ia", id]).status().await.ok()
    }
    .await;
    #[cfg(target_os = "windows")]
    {
        status.is_ok_and(|status| status.success())
    }
    #[cfg(target_os = "linux")]
    {
        status.is_some_and(|status| status.success())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let session = Session::open_with_options(
        Launch::from_env()?,
        "org.example.gamenode.configuration",
        "configuration-example",
        "0.1.0",
        &[
            capability::CONFIGURE,
            capability::STORAGE_READ,
            capability::STORAGE_WRITE,
            capability::STORAGE_PERSIST,
        ],
        gamenode_plugin_sdk::SessionOptions {
            min_api_major: 1,
            min_api_minor: 6,
            max_api_major: 1,
            max_api_minor: 6,
            session_kind: SessionKind::Provider,
            ..Default::default()
        },
    )
    .await?;
    if session.host.resources().is_none() {
        return Err("API 1.6 storage requires a resource context".into());
    }
    gamenode_plugin_sdk::run(session, Arc::new(ExamplePlugin::default())).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Mutex as StdMutex,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Default)]
    struct FixturePreferences(StdMutex<(Option<Vec<u8>>, u64)>);

    #[async_trait::async_trait]
    impl PreferenceStore for FixturePreferences {
        async fn get(&self) -> Result<(Option<Vec<u8>>, u64), Status> {
            let state = self.0.lock().unwrap();
            Ok((state.0.clone(), state.1))
        }

        async fn put(&self, revision: u64, value: Vec<u8>) -> Result<bool, Status> {
            let mut state = self.0.lock().unwrap();
            if state.1 != revision {
                return Ok(true);
            }
            state.0 = Some(value);
            state.1 += 1;
            Ok(false)
        }
    }

    struct FixtureWindows {
        starts: AtomicUsize,
        focuses: AtomicUsize,
        preferences: StdMutex<Vec<String>>,
        delay_seconds: u64,
    }

    #[async_trait::async_trait]
    impl WindowLauncher for FixtureWindows {
        fn start(&self, preference: &str) -> Result<Child, String> {
            self.starts.fetch_add(1, Ordering::Relaxed);
            self.preferences.lock().unwrap().push(preference.to_owned());
            #[cfg(target_os = "linux")]
            let child = Command::new("sh")
                .args([
                    "-c",
                    &format!("sleep {}; printf Library", self.delay_seconds),
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn();
            #[cfg(target_os = "windows")]
            let child = Command::new("powershell.exe")
                .args([
                    "-NoProfile",
                    "-Command",
                    &format!(
                        "Start-Sleep -Seconds {}; [Console]::Out.Write('Library')",
                        self.delay_seconds
                    ),
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn();
            #[cfg(not(any(target_os = "linux", target_os = "windows")))]
            let child: Result<Child, std::io::Error> = Err(std::io::Error::other(
                "fixture process unsupported on this OS",
            ));
            child.map_err(|error| error.to_string())
        }

        async fn focus(&self, _: u32) -> bool {
            self.focuses.fetch_add(1, Ordering::Relaxed);
            true
        }
    }

    fn configuration_command(operation: ProviderOperation, window_id: &str) -> ProviderCommand {
        ProviderCommand {
            operation: operation as i32,
            context: Some(ServiceContext {
                session_id: "session-fixture".into(),
                ..Default::default()
            }),
            target_operation_id: window_id.into(),
            ..Default::default()
        }
    }

    #[test]
    fn configuration_replies_keep_host_session_and_window_correlation() {
        let command = configuration_command(
            ProviderOperation::ConfigurationOpen,
            "opaque-window-fixture",
        );
        let result = configuration_result(command, true, "");
        let reply = result.configuration.unwrap();
        assert!(reply.accepted);
        assert_eq!(reply.session_id, "session-fixture");
        assert_eq!(reply.window_id, "opaque-window-fixture");
    }

    #[tokio::test]
    async fn fixture_configure_focus_close_and_persist_preference_lifecycle() {
        let windows = Arc::new(FixtureWindows {
            starts: AtomicUsize::new(0),
            focuses: AtomicUsize::new(0),
            preferences: StdMutex::new(Vec::new()),
            delay_seconds: 1,
        });
        let plugin = ExamplePlugin::with_launcher(windows.clone());
        let preferences = Arc::new(FixturePreferences::default());
        let store: Arc<dyn PreferenceStore> = preferences.clone();

        let opened = plugin
            .show_or_focus_with(
                configuration_command(ProviderOperation::ConfigurationOpen, "window-fixture"),
                store.clone(),
            )
            .await;
        assert!(opened.configuration.unwrap().accepted);
        let focused = plugin
            .show_or_focus_with(
                configuration_command(ProviderOperation::ConfigurationOpen, "window-fixture"),
                store.clone(),
            )
            .await;
        assert!(focused.configuration.unwrap().accepted);
        assert_eq!(windows.starts.load(Ordering::Relaxed), 1);
        assert_eq!(windows.focuses.load(Ordering::Relaxed), 1);

        tokio::time::timeout(Duration::from_secs(4), async {
            loop {
                let (value, revision) = preferences.get().await.unwrap();
                if value.as_deref() == Some(b"Library") && revision == 1 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("closed fixture window should persist its preference");

        let reopened = plugin
            .show_or_focus_with(
                configuration_command(ProviderOperation::ConfigurationOpen, "window-fixture-2"),
                store.clone(),
            )
            .await;
        assert!(reopened.configuration.unwrap().accepted);
        assert_eq!(*windows.preferences.lock().unwrap(), ["Games", "Library"]);
        let closed = plugin
            .close_with(
                configuration_command(ProviderOperation::ConfigurationClose, "window-fixture-2"),
                store,
            )
            .await;
        assert!(closed.configuration.unwrap().accepted);
    }
}
