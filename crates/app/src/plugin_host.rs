//! Authenticated provider API hosted by the desktop process.

use std::{path::PathBuf, sync::Arc};

#[cfg(target_os = "linux")]
use futures_util::StreamExt;
use gamenode_plugin_protocol::{capability, proto::*};
#[cfg(target_os = "linux")]
use std::io;
use tokio::sync::oneshot;
use tonic::{Request, Response, Status, transport::Server};

pub mod login;
pub mod process;
pub mod provider;
pub mod runtime;
pub mod services;
mod transport;

/// Read-only inventory of the bundled provider executables.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub provider_id: String,
    pub plugin_id: String,
    pub name: String,
    pub source: String,
    pub status: String,
    pub capabilities: Vec<String>,
    pub session: String,
    pub native_access: Vec<String>,
}

/// Resolving the current executable and checking sibling files touches disk;
/// callers should run this on a blocking worker.
pub fn inventory(health: impl Fn(&str) -> (&'static str, bool)) -> Vec<PluginInfo> {
    let executable_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(std::path::Path::to_path_buf));
    inventory_at(executable_dir.as_deref(), health)
}

fn inventory_at(
    executable_dir: Option<&std::path::Path>,
    health: impl Fn(&str) -> (&'static str, bool),
) -> Vec<PluginInfo> {
    PLUGINS
        .iter()
        .filter(|plugin| {
            gamenode_core::provider::Platform::current()
                .is_some_and(|platform| plugin.platforms.contains(&platform))
        })
        .map(|plugin| {
            let installed =
                executable_dir.is_some_and(|dir| dir.join(plugin.executable_name()).is_file());
            let (runtime_status, connected) = health(plugin.provider_id);
            PluginInfo {
                provider_id: plugin.provider_id.to_owned(),
                plugin_id: plugin.plugin_id.to_owned(),
                name: plugin.name.to_owned(),
                source: "official".to_owned(),
                status: if !installed {
                    "missing"
                } else {
                    runtime_status
                }
                .to_owned(),
                capabilities: plugin
                    .capabilities
                    .iter()
                    .map(|capability| (*capability).to_owned())
                    .collect(),
                session: if connected { "active" } else { "stopped" }.into(),
                native_access: plugin
                    .native_access
                    .iter()
                    .map(|s| (*s).to_owned())
                    .collect(),
            }
        })
        .collect()
}

#[derive(Clone, Copy)]
pub(crate) struct PluginDescriptor {
    pub provider_id: &'static str,
    pub plugin_id: &'static str,
    pub name: &'static str,
    pub platforms: &'static [gamenode_core::provider::Platform],
    pub capabilities: &'static [&'static str],
    pub executable: &'static str,
    pub provider_manifest: &'static str,
    pub login_policy: Option<&'static str>,
    /// Existing physical Steam credential slot, selected by trusted host data.
    pub credential_alias: Option<gamenode_platform::credentials::CredentialSlot>,
    pub native_access: &'static [&'static str],
    pub legacy_login_profiles: &'static [&'static str],
}

impl PluginDescriptor {
    pub fn executable_name(&self) -> String {
        if cfg!(windows) {
            format!("{}.exe", self.executable)
        } else {
            self.executable.to_owned()
        }
    }

    pub fn approved_login(
        &self,
    ) -> Result<Option<gamenode_plugin_protocol::login::LoginPolicy>, Status> {
        self.login_policy
            .map(|text| {
                let policy: gamenode_plugin_protocol::login::LoginPolicy = toml::from_str(text)
                    .map_err(|_| Status::invalid_argument("invalid packaged login policy"))?;
                policy
                    .validate()
                    .map_err(|_| Status::invalid_argument("invalid packaged login policy"))?;
                Ok(policy)
            })
            .transpose()
    }
}

const PLUGINS: &[PluginDescriptor] = &[
    PluginDescriptor {
        provider_id: "steam",
        plugin_id: "org.gamenode.plugin.steam-discovery",
        name: "Steam",
        platforms: &[
            gamenode_core::provider::Platform::Linux,
            gamenode_core::provider::Platform::Windows,
        ],
        capabilities: capability::ALL,
        executable: "gamenode-plugin-steam",
        provider_manifest: include_str!("../../../providers/steam.toml"),
        login_policy: Some(include_str!("../../plugin-steam/assets/login.toml")),
        credential_alias: Some(gamenode_platform::credentials::CredentialSlot::Steam),
        native_access: &["files", "network", "registry"],
        legacy_login_profiles: &["steam-login"],
    },
    PluginDescriptor {
        provider_id: "packagekit",
        plugin_id: "org.gamenode.plugin.packagekit-discovery",
        name: "Gerenciador de pacotes Linux",
        platforms: &[gamenode_core::provider::Platform::Linux],
        capabilities: &[capability::CONTEXT, capability::DISCOVERY],
        executable: "gamenode-plugin-packagekit",
        provider_manifest: include_str!("../../../providers/packagekit.toml"),
        login_policy: None,
        credential_alias: None,
        native_access: &["files", "process", "dbus"],
        legacy_login_profiles: &[],
    },
];

pub fn legacy_login_profiles() -> Vec<String> {
    PLUGINS
        .iter()
        .flat_map(|plugin| plugin.legacy_login_profiles)
        .map(|profile| (*profile).to_owned())
        .collect()
}

pub(crate) fn descriptor_for_provider(provider_id: &str) -> Option<&'static PluginDescriptor> {
    PLUGINS
        .iter()
        .find(|plugin| plugin.provider_id == provider_id)
}

pub fn backends(
    platform: gamenode_core::provider::Platform,
) -> Vec<gamenode_core::provider::PluginBackend> {
    PLUGINS
        .iter()
        .filter(|plugin| {
            if !plugin.platforms.contains(&platform) {
                return false;
            }
            let manifest =
                toml::from_str::<gamenode_core::provider::Manifest>(plugin.provider_manifest);
            let login = plugin.approved_login();
            let valid = manifest.is_ok_and(|manifest| {
                manifest.id == plugin.provider_id
                    && manifest
                        .platforms
                        .iter()
                        .all(|platform| plugin.platforms.contains(platform))
            }) && login.is_ok_and(|login| {
                login.is_some() == plugin.capabilities.contains(&capability::LOGIN)
            });
            if !valid {
                tracing::warn!(
                    provider = plugin.provider_id,
                    "invalid bundled plugin descriptor"
                );
            }
            valid
        })
        .map(|plugin| gamenode_core::provider::PluginBackend {
            plugin_id: plugin.plugin_id.into(),
            provider_id: plugin.provider_id.into(),
            discovery: plugin.capabilities.contains(&capability::DISCOVERY),
            catalog: plugin.capabilities.contains(&capability::CATALOG),
            counter: plugin.capabilities.contains(&capability::COUNTER),
            actions: plugin.capabilities.contains(&capability::ACTIONS),
        })
        .collect()
}

#[derive(Clone, Default)]
pub struct PluginApi {
    pub providers: provider::ProviderApi,
}

const MAX_CONNECTIONS: usize = 16;
const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_DISCOVERY_RECORDS: usize = 50_000;

#[tonic::async_trait]
impl plugin_host_server::PluginHost for PluginApi {
    async fn open_session(
        &self,
        request: Request<OpenSessionRequest>,
    ) -> Result<Response<OpenSessionReply>, Status> {
        self.providers
            .open_session(&request.into_inner())
            .map(Response::new)
    }

    async fn submit_discovery(
        &self,
        _request: Request<SubmitDiscoveryRequest>,
    ) -> Result<Response<SubmitDiscoveryReply>, Status> {
        Err(Status::unimplemented(
            "discovery results use the provider operation stream",
        ))
    }
}

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
        let parent = path
            .parent()
            .ok_or("plugin socket must be placed in a private directory")?;
        let metadata = std::fs::symlink_metadata(parent)?;
        if !metadata.is_dir()
            || metadata.permissions().mode() & 0o077 != 0
            || metadata.uid() != rustix::process::getuid().as_raw()
        {
            return Err("plugin socket directory must be private to the current user".into());
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
        let permits = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
        let incoming = tokio_stream::wrappers::UnixListenerStream::new(listener)
            .map(move |accepted| {
                let permits = permits.clone();
                async move {
                    match accepted {
                        Ok(stream)
                            if stream
                                .peer_cred()
                                .is_ok_and(|credentials| credentials.uid() == owner_uid) =>
                        {
                            permits.try_acquire_owned().ok().map(|permit| {
                                Ok(transport::Connection {
                                    inner: stream,
                                    permit,
                                })
                            })
                        }
                        Ok(_) => None,
                        Err(error) => Some(Err(error)),
                    }
                }
            })
            .buffer_unordered(MAX_CONNECTIONS)
            .filter_map(|item| async move { item });

        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let task = runtime.spawn(async move {
            let provider = provider_host_server::ProviderHostServer::new(api.providers.clone())
                .max_decoding_message_size(MAX_MESSAGE_BYTES)
                .max_encoding_message_size(MAX_MESSAGE_BYTES);
            let host = plugin_host_server::PluginHostServer::new(api)
                .max_decoding_message_size(MAX_MESSAGE_BYTES)
                .max_encoding_message_size(MAX_MESSAGE_BYTES);
            if let Err(error) = Server::builder()
                .concurrency_limit_per_connection(8)
                .max_concurrent_streams(8)
                .add_service(host)
                .add_service(provider)
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
}

#[cfg(windows)]
impl PluginApiServer {
    pub fn start_windows(
        runtime: &tokio::runtime::Handle,
        api: PluginApi,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let _guard = runtime.enter();
        let listener = gamenode_platform::ipc::PrivatePipeListener::bind(&format!(
            "{}-{}",
            std::process::id(),
            random_session_id()?
        ))?;
        let path = PathBuf::from(listener.pipe_path());
        let permits = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
        let incoming = futures_util::stream::try_unfold(
            (listener, permits),
            |(mut listener, permits)| async move {
                let mut reported_error = false;
                loop {
                    let pipe = match listener.accept().await {
                        Ok(pipe) => pipe,
                        Err(error) => {
                            if !reported_error {
                                tracing::warn!(%error, "plugin pipe accept failed; retrying");
                                reported_error = true;
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                            continue;
                        }
                    };
                    if let Ok(permit) = permits.clone().try_acquire_owned() {
                        return Ok::<_, std::io::Error>(Some((
                            transport::Connection {
                                inner: pipe,
                                permit,
                            },
                            (listener, permits),
                        )));
                    }
                }
            },
        );
        let (shutdown, stopped) = oneshot::channel();
        let task = runtime.spawn(async move {
            let provider = provider_host_server::ProviderHostServer::new(api.providers.clone())
                .max_decoding_message_size(MAX_MESSAGE_BYTES)
                .max_encoding_message_size(MAX_MESSAGE_BYTES);
            let host = plugin_host_server::PluginHostServer::new(api)
                .max_decoding_message_size(MAX_MESSAGE_BYTES)
                .max_encoding_message_size(MAX_MESSAGE_BYTES);
            if let Err(error) = Server::builder()
                .concurrency_limit_per_connection(8)
                .max_concurrent_streams(8)
                .add_service(host)
                .add_service(provider)
                .serve_with_incoming_shutdown(incoming, async {
                    let _ = stopped.await;
                })
                .await
            {
                tracing::error!(%error, "Plugin API server stopped unexpectedly");
            }
        });
        Ok(Self {
            path,
            shutdown: Some(shutdown),
            task,
        })
    }
}

impl PluginApiServer {
    pub fn endpoint(&self) -> &std::path::Path {
        &self.path
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

impl Drop for PluginApiServer {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.task.abort();
        #[cfg(target_os = "linux")]
        let _ = std::fs::remove_file(&self.path);
    }
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

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn dropping_plugin_api_server_removes_its_socket() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = directory.path().join("plugins.sock");
        let server = PluginApiServer::start(
            &tokio::runtime::Handle::current(),
            &socket,
            PluginApi::default(),
        )
        .unwrap();
        assert!(socket.exists());
        let directory_meta = std::fs::metadata(directory.path()).unwrap();
        let socket_meta = std::fs::symlink_metadata(&socket).unwrap();
        assert_eq!(directory_meta.uid(), rustix::process::getuid().as_raw());
        assert_eq!(directory_meta.permissions().mode() & 0o777, 0o700);
        assert_eq!(socket_meta.uid(), rustix::process::getuid().as_raw());
        assert_eq!(socket_meta.permissions().mode() & 0o777, 0o600);
        drop(server);
        assert!(!socket.exists());
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn plugin_api_refuses_a_shared_socket_directory() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        let result = PluginApiServer::start(
            &tokio::runtime::Handle::current(),
            directory.path().join("plugins.sock"),
            PluginApi::default(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn only_trusted_plugins_are_registered_with_narrow_packagekit_grants() {
        assert_eq!(PLUGINS.len(), 2);
        assert_eq!(
            descriptor_for_provider("steam").unwrap().capabilities,
            capability::ALL
        );
        let packagekit = descriptor_for_provider("packagekit").unwrap();
        assert_eq!(packagekit.name, "Gerenciador de pacotes Linux");
        assert_eq!(
            packagekit.platforms,
            &[gamenode_core::provider::Platform::Linux]
        );
        assert_eq!(
            packagekit.capabilities,
            &[capability::CONTEXT, capability::DISCOVERY]
        );
        assert_eq!(packagekit.native_access, &["files", "process", "dbus"]);
        assert!(packagekit.credential_alias.is_none());
        assert!(packagekit.approved_login().unwrap().is_none());
    }
}

#[cfg(all(test, target_os = "linux"))]
mod inventory_tests {
    use super::*;

    #[test]
    fn inventory_uses_each_providers_own_health_and_session() {
        let dir = tempfile::tempdir().unwrap();
        for plugin in PLUGINS {
            std::fs::write(dir.path().join(plugin.executable_name()), b"fixture").unwrap();
        }
        let plugins = inventory_at(Some(dir.path()), |id| {
            if id == "steam" {
                ("failed", false)
            } else {
                ("installed", true)
            }
        });
        let steam = plugins.iter().find(|p| p.provider_id == "steam").unwrap();
        let packages = plugins
            .iter()
            .find(|p| p.provider_id == "packagekit")
            .unwrap();
        assert_eq!((&*steam.status, &*steam.session), ("failed", "stopped"));
        assert_eq!(
            (&*packages.status, &*packages.session),
            ("installed", "active")
        );
    }
}
