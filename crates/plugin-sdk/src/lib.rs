//! Public provider client. This crate has no dependency on application internals.

use std::path::{Path, PathBuf};
use std::time::Duration;

pub use gamenode_plugin_protocol::{capability, models, proto, settings};
use proto::{plugin_host_client::PluginHostClient, provider_host_client::ProviderHostClient, *};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::{Channel, Endpoint};

pub const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
pub const EVENT_QUEUE: usize = 32;
const SERVICE_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("plugin launch configuration is invalid")]
    Configuration,
    #[error("plugin host is unavailable")]
    Unavailable,
    #[error("plugin host denied the operation")]
    Denied,
    #[error("plugin operation is stale")]
    Stale,
    #[error("plugin protocol response is invalid")]
    Protocol,
    #[error("plugin service is busy")]
    Busy,
}

impl From<tonic::Status> for Error {
    fn from(status: tonic::Status) -> Self {
        // Do not expose transport status text, which can contain request details.
        match status.code() {
            tonic::Code::PermissionDenied | tonic::Code::Unauthenticated => Self::Denied,
            tonic::Code::FailedPrecondition | tonic::Code::Cancelled => Self::Stale,
            tonic::Code::ResourceExhausted => Self::Busy,
            tonic::Code::Unavailable | tonic::Code::DeadlineExceeded => Self::Unavailable,
            _ => Self::Protocol,
        }
    }
}

impl From<gamenode_plugin_protocol::WireError> for Error {
    fn from(_: gamenode_plugin_protocol::WireError) -> Self {
        Self::Protocol
    }
}

/// Secret-bearing configuration is intentionally not Debug/Serialize.
pub struct Launch {
    pub endpoint: PathBuf,
    pub credential: Vec<u8>,
    pub operation_id: String,
}

impl Launch {
    pub fn from_env() -> Result<Self, Error> {
        let endpoint = std::env::var_os("GAMENODE_PLUGIN_ENDPOINT")
            .or_else(|| std::env::var_os("GAMENODE_PLUGIN_SOCKET"))
            .filter(|s| !s.is_empty())
            .ok_or(Error::Configuration)?;
        let encoded = std::env::var("GAMENODE_PLUGIN_SESSION_CREDENTIAL")
            .map_err(|_| Error::Configuration)?;
        if encoded.len() != 64 || !encoded.is_ascii() {
            return Err(Error::Configuration);
        }
        let credential = (0..encoded.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&encoded[i..i + 2], 16).map_err(|_| Error::Configuration))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            endpoint: endpoint.into(),
            credential,
            operation_id: std::env::var("GAMENODE_PLUGIN_OPERATION_ID").unwrap_or_default(),
        })
    }
}

/// Connect only to the local transport provided by the supervising host.
pub async fn connect(endpoint: &Path) -> Result<Channel, Error> {
    let path = endpoint.to_path_buf();
    Endpoint::from_static("http://[::]:50051")
        .connect_timeout(Duration::from_secs(3))
        .connect_with_connector(tower::service_fn(move |_| {
            let path = path.clone();
            async move {
                #[cfg(unix)]
                let stream = tokio::net::UnixStream::connect(path).await?;
                #[cfg(windows)]
                let stream = {
                    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
                    loop {
                        match tokio::net::windows::named_pipe::ClientOptions::new().open(&path) {
                            Ok(pipe) => break pipe,
                            Err(error)
                                if error.raw_os_error() == Some(231)
                                    && tokio::time::Instant::now() < deadline =>
                            {
                                tokio::time::sleep(Duration::from_millis(25)).await
                            }
                            Err(error) => return Err(error),
                        }
                    }
                };
                Ok::<_, std::io::Error>(hyper_util::rt::TokioIo::new(stream))
            }
        }))
        .await
        .map_err(|_| Error::Unavailable)
}

/// An immutable service client bound to one host-issued operation context.
#[derive(Clone)]
pub struct HostClient {
    client: ProviderHostClient<Channel>,
    context: ServiceContext,
    events: mpsc::Sender<ProviderEvent>,
    plugin_id: std::sync::Arc<str>,
    settings_supported: bool,
}

fn request<T>(message: T) -> tonic::Request<T> {
    let mut request = tonic::Request::new(message);
    request.set_timeout(SERVICE_TIMEOUT);
    request
}

impl HostClient {
    pub fn context(&self) -> &ServiceContext {
        &self.context
    }

    pub fn for_command(&self, command: &ProviderCommand) -> Result<Self, Error> {
        let context = command.context.as_ref().ok_or(Error::Protocol)?;
        if context.session_id != self.context.session_id
            || context.provider_id != self.context.provider_id
            || context.session_credential != self.context.session_credential
            || context.operation_id.is_empty()
        {
            return Err(Error::Protocol);
        }
        Ok(Self {
            client: self.client.clone(),
            context: context.clone(),
            events: self.events.clone(),
            plugin_id: self.plugin_id.clone(),
            settings_supported: self.settings_supported,
        })
    }

    /// Validate an optional settings command before handing it to plugin code.
    /// This does not grant access to account, secret or arbitrary host services.
    pub fn settings_request(&self, command: &ProviderCommand) -> Result<settings::Request, Error> {
        if !self.settings_supported {
            return Err(Error::Denied);
        }
        let request: settings::Request = command
            .settings
            .clone()
            .ok_or(Error::Protocol)?
            .try_into()?;
        if request.operation() as i32 != command.operation
            || command.context.as_ref() != Some(&self.context)
            || !request.matches_service(&self.context, &self.plugin_id)
        {
            return Err(Error::Protocol);
        }
        Ok(request)
    }

    pub async fn provider_context(&self) -> Result<ProviderContext, Error> {
        Ok(self
            .client
            .clone()
            .get_context(request(self.context.clone()))
            .await?
            .into_inner())
    }

    pub async fn account_load(&self) -> Result<Option<models::StoreAccount>, Error> {
        let response = self
            .client
            .clone()
            .account(request(AccountRequest {
                context: Some(self.context.clone()),
                action: Some(account_request::Action::Read(true)),
            }))
            .await?
            .into_inner();
        if !response.accepted {
            return Err(Error::Protocol);
        }
        response
            .account
            .map(TryInto::try_into)
            .transpose()
            .map_err(Into::into)
    }

    pub async fn account_save(&self, account: models::StoreAccount) -> Result<(), Error> {
        let reply = self
            .client
            .clone()
            .account(request(AccountRequest {
                context: Some(self.context.clone()),
                action: Some(account_request::Action::Save(account.into())),
            }))
            .await?
            .into_inner();
        if reply.accepted {
            Ok(())
        } else {
            Err(Error::Protocol)
        }
    }

    pub async fn account_clear(&self) -> Result<(), Error> {
        let reply = self
            .client
            .clone()
            .account(request(AccountRequest {
                context: Some(self.context.clone()),
                action: Some(account_request::Action::Clear(true)),
            }))
            .await?
            .into_inner();
        if reply.accepted {
            Ok(())
        } else {
            Err(Error::Protocol)
        }
    }

    pub async fn secret(
        &self,
        operation: SecretOperation,
        key: &str,
        value: Option<&models::SessionCredential>,
    ) -> Result<SecretReply, Error> {
        Ok(self
            .client
            .clone()
            .secret(request(SecretRequest {
                context: Some(self.context.clone()),
                key: key.into(),
                operation: operation as i32,
                value: value.map(|s| s.expose().as_bytes().to_vec()),
            }))
            .await?
            .into_inner())
    }

    pub async fn login(
        &self,
        operation: LoginOperation,
        handle: &str,
    ) -> Result<LoginReply, Error> {
        Ok(self
            .client
            .clone()
            .login(request(LoginRequest {
                context: Some(self.context.clone()),
                operation: operation as i32,
                handle: handle.into(),
            }))
            .await?
            .into_inner())
    }

    pub async fn declare_actions(&self, actions: ActionDeclaration) -> Result<(), Error> {
        let reply = self
            .client
            .clone()
            .declare_actions(request(ActionRequest {
                context: Some(self.context.clone()),
                actions: Some(actions),
            }))
            .await?
            .into_inner();
        if reply.accepted {
            Ok(())
        } else {
            Err(Error::Protocol)
        }
    }

    pub async fn publish_status(&self, status: models::AccountStatus) -> Result<(), Error> {
        self.send(provider_event::Event::AccountStatus(status.into()))
            .await
    }

    pub async fn catalog_chunks(
        &self,
        category: CatalogCategory,
        records: Vec<models::CatalogRecord>,
    ) -> Result<CatalogCompletion, Error> {
        let count = u32::try_from(records.len()).map_err(|_| Error::Protocol)?;
        for (index, chunk) in records.chunks(256).enumerate() {
            self.send(provider_event::Event::CatalogChunk(CatalogChunk {
                category: category as i32,
                index: index as u32,
                games: chunk.iter().cloned().map(Into::into).collect(),
            }))
            .await?;
        }
        Ok(CatalogCompletion {
            category: category as i32,
            outcome: Some(catalog_completion::Outcome::CompletedRecords(count)),
        })
    }

    pub async fn finish(&self, result: OperationResult) -> Result<(), Error> {
        self.send(provider_event::Event::Result(result)).await
    }

    async fn send(&self, event: provider_event::Event) -> Result<(), Error> {
        self.events
            .send(ProviderEvent {
                context: Some(self.context.clone()),
                event: Some(event),
            })
            .await
            .map_err(|_| Error::Unavailable)
    }
}

pub struct Session {
    pub host: HostClient,
    pub commands: tonic::Streaming<ProviderCommand>,
}

impl Session {
    pub async fn open(
        launch: Launch,
        plugin_id: &str,
        provider_id: &str,
        plugin_version: &str,
        capabilities: &[&str],
    ) -> Result<Self, Error> {
        let channel = connect(&launch.endpoint).await?;
        Self::from_channel(
            channel,
            launch,
            plugin_id,
            provider_id,
            plugin_version,
            capabilities,
        )
        .await
    }

    /// Also allows independent reference hosts to exercise the public protocol.
    pub async fn from_channel(
        channel: Channel,
        launch: Launch,
        plugin_id: &str,
        provider_id: &str,
        plugin_version: &str,
        capabilities: &[&str],
    ) -> Result<Self, Error> {
        if launch.credential.len() != 32 {
            return Err(Error::Configuration);
        }
        let reply = PluginHostClient::new(channel.clone())
            .open_session(request(OpenSessionRequest {
                plugin_id: plugin_id.into(),
                plugin_version: plugin_version.into(),
                api_major: gamenode_plugin_protocol::API_MAJOR,
                api_minor: gamenode_plugin_protocol::API_MINOR,
                session_credential: launch.credential.clone(),
                requested_capabilities: capabilities.iter().map(|s| (*s).into()).collect(),
            }))
            .await?
            .into_inner();
        if reply.api_major != gamenode_plugin_protocol::API_MAJOR
            || reply.api_minor < 2
            || reply.api_minor > gamenode_plugin_protocol::API_MINOR
            || reply.granted_capabilities.len() != capabilities.len()
            || capability::negotiate(
                reply.api_major,
                reply.api_minor,
                &reply.granted_capabilities,
                capabilities,
            )
            .is_err()
            || capabilities
                .iter()
                .any(|c| !reply.granted_capabilities.iter().any(|s| s == c))
        {
            return Err(Error::Protocol);
        }
        let settings_supported = capabilities.contains(&capability::SETTINGS);
        if settings_supported && reply.api_minor < capability::SETTINGS_API_MINOR {
            return Err(Error::Protocol);
        }
        let context = ServiceContext {
            session_id: reply.session_id,
            session_credential: launch.credential,
            provider_id: provider_id.into(),
            operation_id: launch.operation_id,
            account_epoch: 0,
            profile_epoch: 0,
        };
        let (events, receive) = mpsc::channel(EVENT_QUEUE);
        let mut client = ProviderHostClient::new(channel)
            .max_decoding_message_size(MAX_MESSAGE_BYTES)
            .max_encoding_message_size(MAX_MESSAGE_BYTES);
        events
            .send(ProviderEvent {
                context: Some(context.clone()),
                event: Some(provider_event::Event::Ready(true)),
            })
            .await
            .map_err(|_| Error::Unavailable)?;
        // A persistent stream has no per-RPC deadline; individual commands and
        // host services are bounded by the supervising runtime.
        let commands = client
            .exchange(ReceiverStream::new(receive))
            .await?
            .into_inner();
        Ok(Self {
            host: HostClient {
                client,
                context,
                events,
                plugin_id: plugin_id.into(),
                settings_supported,
            },
            commands,
        })
    }
}

#[async_trait::async_trait]
pub trait ProviderPlugin: Send + Sync + 'static {
    async fn handle(
        &self,
        command: ProviderCommand,
        host: HostClient,
    ) -> Result<OperationResult, Error>;
    /// Plugin-owned semantic validation and persistence. Check the immutable
    /// request's profile/session scope again immediately before applying work.
    /// The default keeps settings unavailable for existing provider plugins.
    async fn settings(
        &self,
        request: settings::Request,
        _host: HostClient,
    ) -> Result<settings::Reply, Error> {
        Ok(settings::Reply {
            context: request.context,
            outcome: settings::Outcome::Failed {
                failure: settings::Failure::Unavailable,
            },
        })
    }
    async fn shutdown(&self) {}
}

/// Dispatch bounded operations concurrently so control requests can reach a
/// provider whose login or network request is still pending.
pub async fn run<P: ProviderPlugin>(
    mut session: Session,
    plugin: std::sync::Arc<P>,
) -> Result<(), Error> {
    let work = std::sync::Arc::new(tokio::sync::Semaphore::new(8));
    let controls = std::sync::Arc::new(tokio::sync::Semaphore::new(4));
    let mut tasks = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            command = session.commands.message() => {
                let Some(command) = command? else { break; };
                let host = session.host.for_command(&command)?;
                let operation = ProviderOperation::try_from(command.operation).map_err(|_| Error::Protocol)?;
                if operation == ProviderOperation::Acknowledge { continue; }
                if operation == ProviderOperation::Shutdown { break; }
                let control = matches!(operation, ProviderOperation::FocusSignIn | ProviderOperation::CancelSignIn | ProviderOperation::Cancel | ProviderOperation::SignOut | ProviderOperation::SettingsCancel);
                let permit = if control { controls.clone() } else { work.clone() }.try_acquire_owned();
                let Ok(permit) = permit else {
                    host.finish(failure(Error::Busy)).await?;
                    continue;
                };
                let plugin = plugin.clone();
                tasks.spawn(async move {
                    let _permit = permit;
                    let result = if matches!(operation, ProviderOperation::SettingsDescribeRead | ProviderOperation::SettingsSubmit | ProviderOperation::SettingsCancel) {
                        dispatch_settings(plugin.as_ref(), command, host.clone()).await
                    } else {
                        plugin.handle(command, host.clone()).await
                    }.unwrap_or_else(failure);
                    host.finish(result).await
                });
            }
            result = tasks.join_next(), if !tasks.is_empty() => {
                match result {
                    Some(Ok(Ok(()))) => {},
                    _ => { tasks.abort_all(); plugin.shutdown().await; return Err(Error::Unavailable); }
                }
            }
        }
    }
    tasks.abort_all();
    let _ = tokio::time::timeout(Duration::from_secs(3), plugin.shutdown()).await;
    Ok(())
}

async fn dispatch_settings<P: ProviderPlugin>(
    plugin: &P,
    command: ProviderCommand,
    host: HostClient,
) -> Result<OperationResult, Error> {
    let request = host.settings_request(&command)?;
    let reply = plugin.settings(request.clone(), host).await?;
    reply.validate_for(&request)?;
    Ok(OperationResult {
        disposition: OperationDisposition::Completed as i32,
        settings: Some(reply.into()),
        ..Default::default()
    })
}

fn failure(error: Error) -> OperationResult {
    OperationResult {
        disposition: OperationDisposition::Failed as i32,
        failure: Some(PluginFailure {
            code: match error {
                Error::Configuration => "configuration",
                Error::Unavailable => "unavailable",
                Error::Denied => "denied",
                Error::Stale => "stale",
                Error::Protocol => "protocol",
                Error::Busy => "busy",
            }
            .into(),
            message: error.to_string(),
        }),
        ..Default::default()
    }
}
