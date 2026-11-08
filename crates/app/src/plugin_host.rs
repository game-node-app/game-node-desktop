//! Authenticated Plugin API hosted by the desktop process.

use std::{
    collections::{BTreeSet, HashMap},
    io,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use futures_util::StreamExt;
use gamenode_core::provider::Record;
use gamenode_plugin_protocol::{API_MAJOR, API_MINOR, proto::*};
use tokio::sync::oneshot;
use tonic::{Request, Response, Status, transport::Server};

/// Read-only inventory shown in the settings UI.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub provider_id: String,
    pub plugin_id: String,
    pub name: String,
    pub source: String,
    pub status: String,
    pub capabilities: Vec<String>,
}

/// Lists the plugins supported by this build and whether their host executable is packaged.
/// Call from a blocking worker because resolving the executable and checking files touch disk.
pub fn inventory() -> Vec<PluginInfo> {
    let executable_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(std::path::Path::to_path_buf));
    PLUGINS
        .iter()
        .filter(|plugin| cfg!(target_os = "linux") || plugin.provider_id == "steam")
        .map(|plugin| {
            let name = match plugin.provider_id {
                "steam" => "Steam game discovery",
                "packagekit" => "Linux package manager",
                _ => plugin.plugin_id,
            };
            let status = if cfg!(target_os = "linux") {
                executable_dir
                    .as_ref()
                    .is_some_and(|dir| dir.join(plugin.executable).is_file())
            } else {
                // On platforms without the external plugin host, Steam discovery is in-process.
                true
            };
            PluginInfo {
                provider_id: plugin.provider_id.to_owned(),
                plugin_id: plugin.plugin_id.to_owned(),
                name: name.to_owned(),
                source: "official".to_owned(),
                status: if status { "installed" } else { "missing" }.to_owned(),
                capabilities: vec![plugin.capability.to_owned()],
            }
        })
        .collect()
}

#[derive(Clone, Copy)]
struct PluginDescriptor {
    provider_id: &'static str,
    plugin_id: &'static str,
    capability: &'static str,
    executable: &'static str,
}

const PLUGINS: &[PluginDescriptor] = &[
    PluginDescriptor {
        provider_id: "steam",
        plugin_id: "org.gamenode.plugin.steam-discovery",
        capability: "library.discovery",
        executable: "gamenode-plugin-steam",
    },
    PluginDescriptor {
        provider_id: "packagekit",
        plugin_id: "org.gamenode.plugin.packagekit-discovery",
        capability: "library.discovery",
        executable: "gamenode-plugin-packagekit",
    },
];

fn descriptor_for_provider(provider_id: &str) -> Option<&'static PluginDescriptor> {
    PLUGINS
        .iter()
        .find(|plugin| plugin.provider_id == provider_id)
}

fn descriptor_for_plugin(plugin_id: &str) -> Option<&'static PluginDescriptor> {
    PLUGINS.iter().find(|plugin| plugin.plugin_id == plugin_id)
}
const SESSION_CREDENTIAL_BYTES: usize = 32;
const MAX_CONNECTIONS: usize = 16;
const MAX_PENDING_SESSIONS: usize = 16;
const MAX_ACTIVE_SESSIONS: usize = 16;
const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_DISCOVERY_RECORDS: usize = 50_000;

#[derive(Debug)]
pub struct PluginLaunch {
    pub operation_id: String,
    pub session_credential: String,
    pub session_opened: oneshot::Receiver<()>,
    pub result: oneshot::Receiver<Result<Vec<Record>, String>>,
}

#[derive(Clone, Default)]
pub struct PluginApi {
    sessions: Arc<Mutex<SessionState>>,
}

#[derive(Default)]
struct SessionState {
    pending: Vec<PendingSession>,
    active: HashMap<String, ActiveSession>,
}

struct PendingSession {
    credential: [u8; SESSION_CREDENTIAL_BYTES],
    operation_id: String,
    provider_id: String,
    plugin_id: String,
    capability: String,
    session_opened: Option<oneshot::Sender<()>>,
    completion: oneshot::Sender<Result<Vec<Record>, String>>,
}

struct ActiveSession {
    credential: [u8; SESSION_CREDENTIAL_BYTES],
    operation_id: String,
    provider_id: String,
    plugin_id: String,
    capabilities: BTreeSet<String>,
    completion: oneshot::Sender<Result<Vec<Record>, String>>,
}

impl PluginApi {
    pub fn begin_discovery(
        &self,
        provider_id: &str,
        operation_id: impl Into<String>,
    ) -> Result<PluginLaunch, String> {
        let descriptor = descriptor_for_provider(provider_id).ok_or_else(|| {
            format!("no discovery plugin is registered for provider `{provider_id}`")
        })?;
        let operation_id = operation_id.into();
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if sessions.pending.len() >= MAX_PENDING_SESSIONS {
            return Err("plugin session capacity is temporarily full".to_owned());
        }
        let mut credential = [0; SESSION_CREDENTIAL_BYTES];
        getrandom::fill(&mut credential)
            .map_err(|_| "could not create plugin session credential".to_owned())?;
        let (completion_tx, result) = oneshot::channel();
        let (session_opened_tx, session_opened) = oneshot::channel();
        sessions.pending.push(PendingSession {
            credential,
            operation_id: operation_id.clone(),
            provider_id: descriptor.provider_id.to_owned(),
            plugin_id: descriptor.plugin_id.to_owned(),
            capability: descriptor.capability.to_owned(),
            session_opened: Some(session_opened_tx),
            completion: completion_tx,
        });
        Ok(PluginLaunch {
            operation_id,
            session_credential: hex(&credential),
            session_opened,
            result,
        })
    }

    pub fn cancel_operation(&self, operation_id: &str, provider_id: &str) {
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sessions.pending.retain(|session| {
            session.operation_id != operation_id || session.provider_id != provider_id
        });
        sessions.active.retain(|_, session| {
            session.operation_id != operation_id || session.provider_id != provider_id
        });
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone)]
pub struct PluginManager {
    api: PluginApi,
    socket_path: PathBuf,
    plugin_directory: PathBuf,
    shutdown: tokio::sync::watch::Sender<bool>,
    active: Arc<std::sync::atomic::AtomicUsize>,
    idle: Arc<tokio::sync::Notify>,
}

#[cfg(target_os = "linux")]
impl PluginManager {
    pub fn bundled(api: PluginApi, socket_path: PathBuf) -> Result<Self, String> {
        let plugin_directory = std::env::current_exe()
            .map_err(|error| format!("could not locate desktop executable: {error}"))?
            .parent()
            .ok_or_else(|| "desktop executable has no parent directory".to_owned())?
            .to_path_buf();
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Ok(Self {
            api,
            socket_path,
            plugin_directory,
            shutdown,
            active: Arc::default(),
            idle: Arc::default(),
        })
    }

    pub async fn shutdown(&self) {
        self.shutdown.send_replace(true);
        let active = self.active.clone();
        let idle = self.idle.clone();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), async move {
            loop {
                let notified = idle.notified();
                if active.load(std::sync::atomic::Ordering::Acquire) == 0 {
                    return;
                }
                notified.await;
            }
        })
        .await;
    }

    pub fn has_provider(&self, provider_id: &str) -> bool {
        descriptor_for_provider(provider_id).is_some()
    }

    pub async fn run_discovery(
        &self,
        provider_id: &str,
        operation_id: impl Into<String>,
    ) -> Result<Vec<Record>, String> {
        const STARTUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
        const OPERATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);
        const EXIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

        if *self.shutdown.borrow() {
            return Err("desktop host is shutting down".to_owned());
        }
        self.active
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        if *self.shutdown.borrow() {
            self.operation_finished();
            return Err("desktop host is shutting down".to_owned());
        }
        let _operation = PluginOperationGuard {
            active: self.active.clone(),
            idle: self.idle.clone(),
        };
        let mut shutdown = self.shutdown.subscribe();

        let descriptor = descriptor_for_provider(provider_id).ok_or_else(|| {
            format!("no discovery plugin is registered for provider `{provider_id}`")
        })?;
        let executable = self.plugin_directory.join(descriptor.executable);
        let launch = self.api.begin_discovery(provider_id, operation_id)?;
        let operation_id = launch.operation_id.clone();
        let mut child = match tokio::process::Command::new(&executable)
            .env("GAMENODE_PLUGIN_SOCKET", &self.socket_path)
            .env("GAMENODE_PLUGIN_OPERATION_ID", &operation_id)
            .env(
                "GAMENODE_PLUGIN_SESSION_CREDENTIAL",
                &launch.session_credential,
            )
            .kill_on_drop(true)
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                self.api
                    .cancel_operation(&operation_id, descriptor.provider_id);
                return Err(format!(
                    "{} discovery plugin could not start: {error}",
                    descriptor.provider_id
                ));
            }
        };

        let mut session_opened = launch.session_opened;
        tokio::select! {
            opened = tokio::time::timeout(STARTUP_TIMEOUT, &mut session_opened) => {
                if opened.is_err() || opened.is_ok_and(|result| result.is_err()) {
                    self.api
                        .cancel_operation(&operation_id, descriptor.provider_id);
                    let _ = child.kill().await;
                    return Err(format!("{} discovery plugin did not complete its startup handshake", descriptor.provider_id));
                }
            }
            status = child.wait() => {
                self.api
                    .cancel_operation(&operation_id, descriptor.provider_id);
                return Err(match status {
                    Ok(status) => format!("{} discovery plugin exited before handshake ({status})", descriptor.provider_id),
                    Err(error) => format!("{} discovery plugin exit could not be read: {error}", descriptor.provider_id),
                });
            }
            _ = shutdown.changed() => {
                self.api
                    .cancel_operation(&operation_id, descriptor.provider_id);
                let _ = child.kill().await;
                return Err("desktop host is shutting down".to_owned());
            }
        }

        let mut result = launch.result;
        let outcome = tokio::select! {
            biased;
            outcome = tokio::time::timeout(OPERATION_TIMEOUT, &mut result) => {
                match outcome {
                    Ok(Ok(outcome)) => outcome,
                    Ok(Err(_)) => Err(format!("{} discovery plugin closed its session without a result", descriptor.provider_id)),
                    Err(_) => Err(format!("{} discovery plugin exceeded its operation deadline", descriptor.provider_id)),
                }
            }
            status = child.wait() => {
                self.api
                    .cancel_operation(&operation_id, descriptor.provider_id);
                Err(match status {
                    Ok(status) => format!("{} discovery plugin exited without a result ({status})", descriptor.provider_id),
                    Err(error) => format!("{} discovery plugin exit could not be read: {error}", descriptor.provider_id),
                })
            }
            _ = shutdown.changed() => {
                self.api
                    .cancel_operation(&operation_id, descriptor.provider_id);
                let _ = child.kill().await;
                Err("desktop host is shutting down".to_owned())
            }
        };

        if tokio::time::timeout(EXIT_TIMEOUT, child.wait())
            .await
            .is_err()
        {
            let _ = child.kill().await;
        }
        self.api
            .cancel_operation(&operation_id, descriptor.provider_id);
        outcome
    }

    fn operation_finished(&self) {
        if self
            .active
            .fetch_sub(1, std::sync::atomic::Ordering::AcqRel)
            == 1
        {
            self.idle.notify_one();
        }
    }
}

#[cfg(target_os = "linux")]
struct PluginOperationGuard {
    active: Arc<std::sync::atomic::AtomicUsize>,
    idle: Arc<tokio::sync::Notify>,
}

#[cfg(target_os = "linux")]
impl Drop for PluginOperationGuard {
    fn drop(&mut self) {
        if self
            .active
            .fetch_sub(1, std::sync::atomic::Ordering::AcqRel)
            == 1
        {
            self.idle.notify_one();
        }
    }
}

#[tonic::async_trait]
impl plugin_host_server::PluginHost for PluginApi {
    async fn open_session(
        &self,
        request: Request<OpenSessionRequest>,
    ) -> Result<Response<OpenSessionReply>, Status> {
        let request = request.into_inner();
        if request.api_major != API_MAJOR || request.api_minor < API_MINOR {
            return Err(Status::failed_precondition(format!(
                "incompatible Plugin API {}.{}; host requires {}.{} or a compatible newer minor version",
                request.api_major, request.api_minor, API_MAJOR, API_MINOR
            )));
        }
        let Some(descriptor) = descriptor_for_plugin(&request.plugin_id) else {
            return Err(Status::permission_denied(
                "plugin identity is not authorized",
            ));
        };
        if request.plugin_version.is_empty() || request.plugin_version.len() > 128 {
            return Err(Status::invalid_argument("plugin version is invalid"));
        }
        if request.requested_capabilities.len() != 1
            || request.requested_capabilities[0] != descriptor.capability
        {
            return Err(Status::permission_denied(
                "plugin capability is not authorized",
            ));
        }
        let requested: BTreeSet<_> = request.requested_capabilities.into_iter().collect();
        if requested != BTreeSet::from([descriptor.capability.to_owned()]) {
            return Err(Status::permission_denied(
                "plugin capability is not authorized",
            ));
        }
        let credential: [u8; SESSION_CREDENTIAL_BYTES] = request
            .session_credential
            .try_into()
            .map_err(|_| Status::unauthenticated("invalid plugin session credential"))?;

        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if sessions.active.len() >= MAX_ACTIVE_SESSIONS {
            return Err(Status::resource_exhausted(
                "Plugin API session capacity is temporarily full",
            ));
        }
        let Some(index) = sessions
            .pending
            .iter()
            .position(|pending| constant_time_eq(&pending.credential, &credential))
        else {
            return Err(Status::unauthenticated("invalid plugin session credential"));
        };
        let pending_matches = sessions.pending[index].plugin_id == request.plugin_id
            && sessions.pending[index].capability == descriptor.capability;
        if !pending_matches {
            return Err(Status::permission_denied(
                "plugin identity does not match the launched operation",
            ));
        }
        let mut pending = sessions.pending.remove(index);
        let session_id = random_session_id()?;
        if let Some(opened) = pending.session_opened.take() {
            let _ = opened.send(());
        }
        sessions.active.insert(
            session_id.clone(),
            ActiveSession {
                credential,
                operation_id: pending.operation_id,
                provider_id: pending.provider_id,
                plugin_id: request.plugin_id,
                capabilities: requested.clone(),
                completion: pending.completion,
            },
        );
        Ok(Response::new(OpenSessionReply {
            session_id,
            granted_capabilities: requested.into_iter().collect(),
        }))
    }

    async fn submit_discovery(
        &self,
        request: Request<SubmitDiscoveryRequest>,
    ) -> Result<Response<SubmitDiscoveryReply>, Status> {
        let request = request.into_inner();
        let credential: [u8; SESSION_CREDENTIAL_BYTES] = request
            .session_credential
            .try_into()
            .map_err(|_| Status::unauthenticated("invalid plugin session credential"))?;
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(session) = sessions.active.get(&request.session_id) else {
            return Err(Status::unauthenticated("plugin session is not active"));
        };
        if !constant_time_eq(&session.credential, &credential)
            || session.operation_id != request.operation_id
            || descriptor_for_plugin(&session.plugin_id)
                .is_none_or(|descriptor| !session.capabilities.contains(descriptor.capability))
        {
            return Err(Status::permission_denied(
                "plugin request does not match the active session",
            ));
        }
        let session = sessions
            .active
            .remove(&request.session_id)
            .expect("active session checked above");
        let outcome = match request.outcome {
            Some(submit_discovery_request::Outcome::Completed(result)) => {
                if result.provider_id != session.provider_id {
                    return Err(Status::permission_denied(
                        "plugin discovery result does not match its registered provider",
                    ));
                }
                if result.records.len() > MAX_DISCOVERY_RECORDS {
                    return Err(Status::resource_exhausted(
                        "plugin discovery result contains too many records",
                    ));
                }
                let records = result
                    .records
                    .into_iter()
                    .map(|record| {
                        if record.id.len() > 256
                            || record.name.len() > 4096
                            || record.install_dir.len() > 16_384
                            || record.id.trim().is_empty()
                            || record.id.chars().any(char::is_control)
                            || record.name.chars().any(char::is_control)
                            || record.install_dir.trim().is_empty()
                            || record.install_dir.chars().any(char::is_control)
                            || record.fields.len() > 64
                            || record
                                .fields
                                .iter()
                                .any(|(key, value)| key.len() > 128 || value.len() > 4096)
                        {
                            return Err(Status::invalid_argument(
                                "plugin discovery record exceeds field limits",
                            ));
                        }
                        Ok(Record {
                            id: record.id,
                            name: (!record.name.is_empty()).then_some(record.name),
                            install_dir: record.install_dir,
                            fields: record.fields.into_iter().collect(),
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(records)
            }
            Some(submit_discovery_request::Outcome::Failed(failure)) => {
                let code = match failure.code.as_str() {
                    "scan_failed" | "steam_unavailable" | "unsupported_platform" => failure.code,
                    _ => "plugin_failed".to_owned(),
                };
                Err(format!("{} plugin reported {code}", session.provider_id))
            }
            None => return Err(Status::invalid_argument("plugin result is missing")),
        };
        let _ = session.completion.send(outcome);
        Ok(Response::new(SubmitDiscoveryReply { accepted: true }))
    }
}

#[cfg(target_os = "linux")]
pub struct PluginApiServer {
    path: PathBuf,
    shutdown: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

#[cfg(target_os = "linux")]
impl PluginApiServer {
    pub fn start(
        runtime: &tokio::runtime::Handle,
        path: impl Into<PathBuf>,
        api: PluginApi,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};

        let path = path.into();
        if let Some(parent) = path.parent() {
            let metadata = std::fs::symlink_metadata(parent)?;
            if !metadata.is_dir()
                || metadata.permissions().mode() & 0o077 != 0
                || metadata.uid() != rustix::process::getuid().as_raw()
            {
                return Err("plugin socket directory must be private to the current user".into());
            }
        }
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_socket() => std::fs::remove_file(&path)?,
            Ok(_) => return Err("refusing to replace a non-socket plugin endpoint".into()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let listener = std::os::unix::net::UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let _runtime_guard = runtime.enter();
        let listener = tokio::net::UnixListener::from_std(listener)?;

        let owner_uid = rustix::process::getuid().as_raw();
        let incoming = tokio_stream::wrappers::UnixListenerStream::new(listener)
            .map(move |accepted| async move {
                match accepted {
                    Ok(stream) => {
                        if stream
                            .peer_cred()
                            .is_ok_and(|credentials| credentials.uid() == owner_uid)
                        {
                            Some(Ok(stream))
                        } else {
                            None
                        }
                    }
                    Err(error) => Some(Err(error)),
                }
            })
            .buffer_unordered(MAX_CONNECTIONS)
            .filter_map(|item| async move { item });

        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let task = runtime.spawn(async move {
            let service = plugin_host_server::PluginHostServer::new(api)
                .max_decoding_message_size(MAX_MESSAGE_BYTES)
                .max_encoding_message_size(MAX_MESSAGE_BYTES);
            if let Err(error) = Server::builder()
                .concurrency_limit_per_connection(8)
                .max_concurrent_streams(8)
                .add_service(service)
                .serve_with_incoming_shutdown(incoming, async move {
                    let _ = shutdown_rx.await;
                })
                .await
            {
                tracing::error!(%error, "Plugin API server stopped unexpectedly");
            }
        });
        Ok(Self {
            path,
            shutdown: Some(shutdown_tx),
            task,
        })
    }

    pub async fn shutdown(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if tokio::time::timeout(std::time::Duration::from_secs(2), &mut self.task)
            .await
            .is_err()
        {
            self.task.abort();
            let _ = (&mut self.task).await;
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for PluginApiServer {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.task.abort();
        let _ = std::fs::remove_file(&self.path);
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

fn random_session_id() -> Result<String, Status> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes)
        .map_err(|_| Status::internal("could not create plugin session id"))?;
    Ok(hex(&bytes))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(DIGITS[(byte >> 4) as usize] as char);
        result.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token_bytes(token: &str) -> Vec<u8> {
        token
            .as_bytes()
            .chunks_exact(2)
            .map(|digits| {
                let digit = |value: u8| match value {
                    b'0'..=b'9' => value - b'0',
                    b'a'..=b'f' => value - b'a' + 10,
                    _ => panic!("generated token must be lowercase hex"),
                };
                (digit(digits[0]) << 4) | digit(digits[1])
            })
            .collect()
    }

    fn open_request_for(provider_id: &str, credential: Vec<u8>) -> OpenSessionRequest {
        let descriptor = descriptor_for_provider(provider_id).unwrap();
        OpenSessionRequest {
            plugin_id: descriptor.plugin_id.to_owned(),
            plugin_version: "0.1.0".to_owned(),
            api_major: API_MAJOR,
            api_minor: API_MINOR,
            session_credential: credential,
            requested_capabilities: vec![descriptor.capability.to_owned()],
        }
    }

    fn open_request(credential: Vec<u8>) -> OpenSessionRequest {
        open_request_for("steam", credential)
    }

    #[tokio::test]
    async fn compatible_authorized_plugin_opens_a_scoped_session() {
        let api = PluginApi::default();
        let launch = api.begin_discovery("steam", "scan-1").unwrap();
        let request = open_request(token_bytes(&launch.session_credential));

        let session = plugin_host_server::PluginHost::open_session(&api, Request::new(request))
            .await
            .unwrap()
            .into_inner();

        assert!(!session.session_id.is_empty());
        assert_eq!(session.granted_capabilities, ["library.discovery"]);
        assert!(launch.session_opened.await.is_ok());
    }

    #[tokio::test]
    async fn handshake_rejects_wrong_identity_capability_version_and_credential() {
        let cases = [
            ("identity", tonic::Code::PermissionDenied),
            ("capability", tonic::Code::PermissionDenied),
            ("version", tonic::Code::FailedPrecondition),
            ("credential", tonic::Code::Unauthenticated),
        ];

        for (case, expected_code) in cases {
            let api = PluginApi::default();
            let launch = api.begin_discovery("steam", case).unwrap();
            let mut request = open_request(token_bytes(&launch.session_credential));
            match case {
                "identity" => request.plugin_id = "org.example.unknown".to_owned(),
                "capability" => request.requested_capabilities = vec!["library.write".to_owned()],
                "version" => request.api_major = API_MAJOR + 1,
                "credential" => request.session_credential = vec![0; SESSION_CREDENTIAL_BYTES],
                _ => unreachable!(),
            }

            let status = plugin_host_server::PluginHost::open_session(&api, Request::new(request))
                .await
                .unwrap_err();
            assert_eq!(status.code(), expected_code, "case: {case}");
            api.cancel_operation(case, "steam");
        }
    }

    #[tokio::test]
    async fn cancelling_one_provider_keeps_other_provider_session_in_same_scan() {
        let api = PluginApi::default();
        let steam = api.begin_discovery("steam", "scan-1").unwrap();
        let packages = api.begin_discovery("packagekit", "scan-1").unwrap();
        let steam_credential = token_bytes(&steam.session_credential);
        let packages_credential = token_bytes(&packages.session_credential);
        let steam_session = plugin_host_server::PluginHost::open_session(
            &api,
            Request::new(open_request_for("steam", steam_credential.clone())),
        )
        .await
        .unwrap()
        .into_inner();
        let packages_session = plugin_host_server::PluginHost::open_session(
            &api,
            Request::new(open_request_for("packagekit", packages_credential.clone())),
        )
        .await
        .unwrap()
        .into_inner();

        assert!(steam.session_opened.await.is_ok());
        assert!(packages.session_opened.await.is_ok());
        api.cancel_operation("scan-1", "steam");

        let request = SubmitDiscoveryRequest {
            session_id: packages_session.session_id,
            session_credential: packages_credential,
            operation_id: "scan-1".to_owned(),
            outcome: Some(submit_discovery_request::Outcome::Completed(
                DiscoveryResult {
                    provider_id: "packagekit".to_owned(),
                    records: Vec::new(),
                },
            )),
        };
        plugin_host_server::PluginHost::submit_discovery(&api, Request::new(request))
            .await
            .unwrap();
        assert!(packages.result.await.unwrap().unwrap().is_empty());
        assert!(steam.result.await.is_err());

        // The other plugin's result belongs to the same scan but remains independently valid.
        assert!(!steam_session.session_id.is_empty());
    }
}
