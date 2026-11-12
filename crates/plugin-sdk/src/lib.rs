//! Public provider client. This crate has no dependency on application internals.

use std::path::{Path, PathBuf};
use std::time::Duration;

pub use gamenode_plugin_protocol::{capability, extensions, models, prost_types, proto, settings};
use proto::{plugin_host_client::PluginHostClient, provider_host_client::ProviderHostClient, *};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::{Channel, Endpoint};

pub const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
pub const EVENT_QUEUE: usize = 32;
const SERVICE_TIMEOUT: Duration = Duration::from_secs(15);

/// Build an explicit, typed per-query category selector. An empty iterator is
/// valid and means private/owner-visible records only. The host still checks
/// the matching activation grant and its scope for every query.
pub fn common_data_selector(
    categories: impl IntoIterator<Item = CommonDataCategory>,
) -> Result<CommonDataSelector, gamenode_plugin_protocol::common_data::SelectorError> {
    let selector = CommonDataSelector {
        categories: categories
            .into_iter()
            .map(|category| category as i32)
            .collect(),
        ..Default::default()
    };
    gamenode_plugin_protocol::common_data::categories(Some(&selector))?;
    Ok(selector)
}

/// Build a selector for explicitly granted published records without asking
/// for category-wide access. The host must authorize every id independently.
pub fn selected_record_selector(
    record_ids: impl IntoIterator<Item = impl Into<String>>,
) -> Result<CommonDataSelector, gamenode_plugin_protocol::common_data::SelectorError> {
    let selector = CommonDataSelector {
        selected_record_ids: record_ids.into_iter().map(Into::into).collect(),
        ..Default::default()
    };
    gamenode_plugin_protocol::common_data::selected_record_ids(Some(&selector))?;
    Ok(selector)
}

/// Build a selector combining category-wide and selected-record grants.
pub fn common_data_selector_with_records(
    categories: impl IntoIterator<Item = CommonDataCategory>,
    record_ids: impl IntoIterator<Item = impl Into<String>>,
) -> Result<CommonDataSelector, gamenode_plugin_protocol::common_data::SelectorError> {
    let mut selector = common_data_selector(categories)?;
    selector.selected_record_ids = record_ids.into_iter().map(Into::into).collect();
    gamenode_plugin_protocol::common_data::selected_record_ids(Some(&selector))?;
    Ok(selector)
}

#[cfg(test)]
mod common_data_selector_tests {
    use super::*;

    #[test]
    fn selector_is_explicit_and_empty_means_private_only() {
        let private = common_data_selector([]).unwrap();
        assert!(private.categories.is_empty());
        assert!(private.selected_record_ids.is_empty());

        let selected = common_data_selector([
            CommonDataCategory::GamePaths,
            CommonDataCategory::GamePerformanceHistory,
        ])
        .unwrap();
        assert_eq!(
            selected.categories,
            vec![
                CommonDataCategory::GamePaths as i32,
                CommonDataCategory::GamePerformanceHistory as i32,
            ]
        );

        let selected_records = selected_record_selector(["runtime:one", "save:two"]).unwrap();
        assert!(selected_records.categories.is_empty());
        assert_eq!(selected_records.selected_record_ids, ["runtime:one", "save:two"]);
        assert!(
            common_data_selector([CommonDataCategory::GamePaths, CommonDataCategory::GamePaths,])
                .is_err()
        );
    }
}

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

/// Opt-in detailed status classification. The legacy [`Error`] conversion is
/// unchanged so existing official plugins retain their matching behavior.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DetailedError {
    Denied(Option<DenialDetail>),
    Unauthenticated,
    Stale,
    Unsupported,
    Invalid,
    Unavailable,
    Protocol,
}

impl From<&tonic::Status> for DetailedError {
    fn from(status: &tonic::Status) -> Self {
        use gamenode_plugin_protocol::StatusKind as K;
        match gamenode_plugin_protocol::classify_status(status.code()) {
            K::Denied => Self::Denied(status.details().get(..).and_then(|bytes| {
                // The status detail payload uses a length-delimited protobuf
                // DenialDetail. Malformed/absent details remain a plain denial.
                use gamenode_plugin_protocol::prost::Message;
                DenialDetail::decode(bytes).ok()
            })),
            K::Unauthenticated => Self::Unauthenticated,
            K::Stale => Self::Stale,
            K::Unsupported => Self::Unsupported,
            K::Invalid => Self::Invalid,
            K::Unavailable => Self::Unavailable,
            K::Protocol => Self::Protocol,
        }
    }
}

impl DetailedError {
    pub fn from_status(status: &tonic::Status) -> Self {
        status.into()
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

#[derive(Clone, Debug)]
pub struct SessionOptions {
    pub min_api_major: u32,
    pub min_api_minor: u32,
    pub max_api_major: u32,
    pub max_api_minor: u32,
    pub features: Vec<(String, bool)>,
    pub session_kind: SessionKind,
    pub target_game: Option<TargetGameContext>,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            min_api_major: 0,
            min_api_minor: 0,
            max_api_major: 0,
            max_api_minor: 0,
            features: Vec::new(),
            session_kind: SessionKind::Unspecified,
            target_game: None,
        }
    }
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
    channel: Channel,
    context: ServiceContext,
    events: mpsc::Sender<ProviderEvent>,
    plugin_id: std::sync::Arc<str>,
    settings_supported: bool,
    configure_supported: bool,
}

fn request<T>(message: T) -> tonic::Request<T> {
    let mut request = tonic::Request::new(message);
    request.set_timeout(SERVICE_TIMEOUT);
    request
}

impl HostClient {
    /// Raw generated client for the API 1.4 resource surface. It remains bound
    /// to the same local authenticated channel; callers provide scoped request
    /// contexts explicitly so the SDK adds no authority implicitly.
    pub fn resource_client(&self) -> proto::resource_host_client::ResourceHostClient<Channel> {
        proto::resource_host_client::ResourceHostClient::new(self.channel.clone())
            .max_decoding_message_size(MAX_MESSAGE_BYTES)
            .max_encoding_message_size(MAX_MESSAGE_BYTES)
    }

    pub fn context(&self) -> &ServiceContext {
        &self.context
    }

    pub async fn describe_api(&self) -> Result<DescribeApiReply, DetailedError> {
        PluginHostClient::new(self.channel.clone())
            .describe_api(request(DescribeApiRequest {
                session_id: self.context.session_id.clone(),
                session_credential: self.context.session_credential.clone(),
            }))
            .await
            .map(|reply| reply.into_inner())
            .map_err(|status| DetailedError::from_status(&status))
    }

    pub async fn get_permissions(&self) -> Result<GetPermissionsReply, DetailedError> {
        PluginHostClient::new(self.channel.clone())
            .get_permissions(request(GetPermissionsRequest {
                session_id: self.context.session_id.clone(),
                session_credential: self.context.session_credential.clone(),
            }))
            .await
            .map(|reply| reply.into_inner())
            .map_err(|status| DetailedError::from_status(&status))
    }

    pub async fn watch_permission_changes(
        &self,
        after_revision: u64,
    ) -> Result<tonic::Streaming<PermissionChange>, DetailedError> {
        PluginHostClient::new(self.channel.clone())
            .watch_permission_changes(request(WatchPermissionChangesRequest {
                session_id: self.context.session_id.clone(),
                session_credential: self.context.session_credential.clone(),
                after_revision,
            }))
            .await
            .map(|reply| reply.into_inner())
            .map_err(|status| DetailedError::from_status(&status))
    }

    pub fn for_command(&self, command: &ProviderCommand) -> Result<Self, Error> {
        let context = command.context.as_ref().ok_or(Error::Protocol)?;
        if context.session_id != self.context.session_id
            || context.provider_id != self.context.provider_id
            || context.session_credential != self.context.session_credential
            || context.session_kind != self.context.session_kind
            || context.target_game != self.context.target_game
            || context.operation_id.is_empty()
        {
            return Err(Error::Protocol);
        }
        Ok(Self {
            client: self.client.clone(),
            channel: self.channel.clone(),
            context: context.clone(),
            events: self.events.clone(),
            plugin_id: self.plugin_id.clone(),
            settings_supported: self.settings_supported,
            configure_supported: self.configure_supported,
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

    pub async fn open_with_options(
        launch: Launch,
        plugin_id: &str,
        provider_id: &str,
        plugin_version: &str,
        capabilities: &[&str],
        options: SessionOptions,
    ) -> Result<Self, Error> {
        let channel = connect(&launch.endpoint).await?;
        Self::from_channel_with_options(
            channel,
            launch,
            plugin_id,
            provider_id,
            plugin_version,
            capabilities,
            options,
        )
        .await
    }

    /// Opens an extension session scoped to exactly one target game. The
    /// extension has no provider identity of its own and must use the resource
    /// client instead of provider-owned account/catalog operations.
    pub async fn open_extension(
        launch: Launch,
        plugin_id: &str,
        plugin_version: &str,
        target: TargetGameContext,
        capabilities: &[&str],
    ) -> Result<Self, Error> {
        let channel = connect(&launch.endpoint).await?;
        Self::from_channel_with_options(
            channel,
            launch,
            plugin_id,
            "",
            plugin_version,
            capabilities,
            SessionOptions {
                min_api_major: 1,
                min_api_minor: gamenode_plugin_protocol::API_MINOR,
                max_api_major: 1,
                max_api_minor: gamenode_plugin_protocol::API_MINOR,
                features: vec![(capability::FEATURE_EXTENSION_SESSION.into(), true)],
                session_kind: SessionKind::Extension,
                target_game: Some(target),
            },
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
        Self::from_channel_with_options(
            channel,
            launch,
            plugin_id,
            provider_id,
            plugin_version,
            capabilities,
            SessionOptions::default(),
        )
        .await
    }

    /// API 1.4+ handshake with an explicit compatibility range and optional
    /// feature requests. The legacy `from_channel` method keeps its API 1.1–1.3
    /// request shape for existing official plugins.
    pub async fn from_channel_with_options(
        channel: Channel,
        launch: Launch,
        plugin_id: &str,
        provider_id: &str,
        plugin_version: &str,
        capabilities: &[&str],
        options: SessionOptions,
    ) -> Result<Self, Error> {
        if launch.credential.len() != 32 {
            return Err(Error::Configuration);
        }
        capability::validate_session_scope(
            options.session_kind as u32,
            options.target_game.is_some(),
        )
        .map_err(|_| Error::Configuration)?;
        if let Some(target) = options.target_game.as_ref() {
            capability::validate_target(target).map_err(|_| Error::Configuration)?;
        }
        let reply = PluginHostClient::new(channel.clone())
            .open_session(request(OpenSessionRequest {
                plugin_id: plugin_id.into(),
                plugin_version: plugin_version.into(),
                api_major: gamenode_plugin_protocol::API_MAJOR,
                api_minor: if options.min_api_major == 0
                    && options.min_api_minor == 0
                    && options.max_api_major == 0
                    && options.max_api_minor == 0
                {
                    gamenode_plugin_protocol::LEGACY_API_MINOR
                } else {
                    gamenode_plugin_protocol::API_MINOR
                },
                session_credential: launch.credential.clone(),
                requested_capabilities: capabilities.iter().map(|s| (*s).into()).collect(),
                min_api_major: options.min_api_major,
                min_api_minor: options.min_api_minor,
                max_api_major: options.max_api_major,
                max_api_minor: options.max_api_minor,
                features: options
                    .features
                    .iter()
                    .map(|(id, required)| FeatureRequest {
                        id: id.clone(),
                        required: *required,
                    })
                    .collect(),
                session_kind: options.session_kind as i32,
                target_game: options.target_game.clone(),
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
        for (id, required) in &options.features {
            let Some(status) = reply.features.iter().find(|status| &status.id == id) else {
                if *required {
                    return Err(Error::Protocol);
                }
                continue;
            };
            if *required && status.availability != FeatureAvailability::Supported as i32 {
                return Err(Error::Protocol);
            }
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
            session_kind: options.session_kind as i32,
            target_game: options.target_game.clone(),
        };
        let (events, receive) = mpsc::channel(EVENT_QUEUE);
        let mut client = ProviderHostClient::new(channel.clone())
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
                channel: channel.clone(),
                context,
                events,
                plugin_id: plugin_id.into(),
                settings_supported,
                configure_supported: capabilities.contains(&capability::CONFIGURE),
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
    /// Optional external configuration lifecycle. The default explicitly
    /// declines; it does not turn an SDK plugin into an external UI provider.
    async fn configure(
        &self,
        command: ProviderCommand,
        _host: HostClient,
    ) -> Result<OperationResult, Error> {
        let context = command.context.ok_or(Error::Protocol)?;
        Ok(OperationResult {
            disposition: OperationDisposition::Failed as i32,
            configuration: Some(ConfigurationResult {
                session_id: context.session_id,
                profile_id: String::new(),
                window_id: command.target_operation_id,
                accepted: false,
                failure: "unsupported".into(),
            }),
            ..Default::default()
        })
    }
    async fn shutdown(&self) {}
}

#[async_trait::async_trait]
pub trait ExtensionPlugin: Send + Sync + 'static {
    async fn handle(
        &self,
        command: ProviderCommand,
        host: HostClient,
    ) -> Result<OperationResult, Error>;

    async fn configure(
        &self,
        command: ProviderCommand,
        _host: HostClient,
    ) -> Result<OperationResult, Error> {
        let context = command.context.ok_or(Error::Protocol)?;
        Ok(OperationResult {
            disposition: OperationDisposition::Failed as i32,
            configuration: Some(ConfigurationResult {
                session_id: context.session_id,
                profile_id: String::new(),
                window_id: command.target_operation_id,
                accepted: false,
                failure: "unsupported".into(),
            }),
            ..Default::default()
        })
    }

    async fn shutdown(&self) {}
}

struct ExtensionAdapter<P>(std::sync::Arc<P>);

#[async_trait::async_trait]
impl<P: ExtensionPlugin> ProviderPlugin for ExtensionAdapter<P> {
    async fn handle(
        &self,
        command: ProviderCommand,
        host: HostClient,
    ) -> Result<OperationResult, Error> {
        self.0.handle(command, host).await
    }
    async fn configure(
        &self,
        command: ProviderCommand,
        host: HostClient,
    ) -> Result<OperationResult, Error> {
        self.0.configure(command, host).await
    }
    async fn shutdown(&self) {
        self.0.shutdown().await;
    }
}

/// Dispatches the shared authenticated command stream to an extension after
/// checking the negotiated session is target-scoped.
pub async fn run_extension<P: ExtensionPlugin>(
    session: Session,
    plugin: std::sync::Arc<P>,
) -> Result<(), Error> {
    if session.host.context.session_kind != SessionKind::Extension as i32
        || session.host.context.target_game.is_none()
    {
        return Err(Error::Configuration);
    }
    run(session, std::sync::Arc::new(ExtensionAdapter(plugin))).await
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
                let control = matches!(operation, ProviderOperation::FocusSignIn | ProviderOperation::CancelSignIn | ProviderOperation::Cancel | ProviderOperation::SignOut | ProviderOperation::SettingsCancel | ProviderOperation::ConfigurationFocus | ProviderOperation::ConfigurationClose);
                let permit = if control { controls.clone() } else { work.clone() }.try_acquire_owned();
                let Ok(permit) = permit else {
                    let busy = if is_settings(operation) {
                        host.settings_request(&command).map_or_else(
                            // A command with an unparseable settings scope has
                            // no trustworthy context to echo; the host rejects
                            // it as a protocol error.
                            |_| failure(Error::Busy),
                            |request| settings_failure(&request, Error::Busy),
                        )
                    } else {
                        failure(Error::Busy)
                    };
                    host.finish(busy).await?;
                    continue;
                };
                let plugin = plugin.clone();
                tasks.spawn(async move {
                    let _permit = permit;
                    let result = if is_settings(operation) {
                        dispatch_settings(plugin.as_ref(), command, host.clone()).await
                    } else if matches!(operation, ProviderOperation::ConfigurationOpen | ProviderOperation::ConfigurationFocus | ProviderOperation::ConfigurationClose) {
                        if !host.configure_supported {
                            Ok(failure(Error::Denied))
                        } else {
                            let expected_session = host.context.session_id.clone();
                            let expected_window = command.target_operation_id.clone();
                            let result = plugin.configure(command, host.clone()).await?;
                            if result.configuration.as_ref().is_none_or(|reply| reply.session_id != expected_session || reply.window_id != expected_window) {
                                Ok(failure(Error::Protocol))
                            } else { Ok(result) }
                        }
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

fn is_settings(operation: ProviderOperation) -> bool {
    matches!(
        operation,
        ProviderOperation::SettingsDescribeRead
            | ProviderOperation::SettingsSubmit
            | ProviderOperation::SettingsCancel
    )
}

async fn dispatch_settings<P: ProviderPlugin>(
    plugin: &P,
    command: ProviderCommand,
    host: HostClient,
) -> Result<OperationResult, Error> {
    let request = match host.settings_request(&command) {
        Ok(request) => request,
        // The host-issued scope is invalid or unavailable, so no context can
        // be echoed faithfully. The host treats this malformed command as a
        // protocol error instead of a settings outcome.
        Err(error) => return Ok(failure(error)),
    };
    let reply = match plugin.settings(request.clone(), host).await {
        Ok(reply) => match reply.validate_for(&request) {
            Ok(()) => reply,
            Err(_) => return Ok(settings_failure(&request, Error::Protocol)),
        },
        Err(error) => return Ok(settings_failure(&request, error)),
    };
    Ok(OperationResult {
        disposition: OperationDisposition::Completed as i32,
        settings: Some(reply.into()),
        ..Default::default()
    })
}

/// Errors after request parsing still need a typed settings reply: the host
/// rejects a bare failure because it cannot correlate it to the settings
/// operation, and then drops the whole provider session.
fn settings_failure(request: &settings::Request, error: Error) -> OperationResult {
    let failure = match error {
        Error::Denied => settings::Failure::Denied,
        Error::Unavailable | Error::Busy => settings::Failure::Unavailable,
        Error::Stale => settings::Failure::Stale,
        // Configuration and protocol problems make this request/reply invalid;
        // there is no more specific settings failure variant.
        Error::Configuration | Error::Protocol => settings::Failure::Invalid,
    };
    OperationResult {
        disposition: OperationDisposition::Failed as i32,
        settings: Some(
            settings::Reply {
                context: request.context.clone(),
                outcome: settings::Outcome::Failed { failure },
            }
            .into(),
        ),
        ..Default::default()
    }
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
