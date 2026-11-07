#[cfg(target_os = "linux")]
mod linux {
    use std::{env, path::PathBuf, time::Duration};

    use gamenode_core::provider::{Platform, load};
    use gamenode_platform::host::{StdHostFiles, SystemRoots};
    use gamenode_plugin_protocol::{
        API_MAJOR, API_MINOR,
        proto::{
            DiscoveryRecord, DiscoveryResult, OpenSessionRequest, SubmitDiscoveryRequest,
            plugin_host_client::PluginHostClient, submit_discovery_request::Outcome,
        },
    };
    use tonic::transport::{Channel, Endpoint};

    const PLUGIN_ID: &str = "org.gamenode.plugin.steam-discovery";
    const CAPABILITY: &str = "library.discovery.steam";

    pub(super) async fn run() -> Result<(), &'static str> {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("warn")
            .with_writer(std::io::stderr)
            .try_init();

        let socket = required_path("GAMENODE_PLUGIN_SOCKET")?;
        let operation_id = required("GAMENODE_PLUGIN_OPERATION_ID")?;
        let credential = decode_hex(required("GAMENODE_PLUGIN_SESSION_CREDENTIAL")?)?;
        if credential.len() != 32 {
            return Err("invalid_session_credential");
        }

        let channel = connect(socket).await?;
        let mut client = PluginHostClient::new(channel);
        let session = client
            .open_session(OpenSessionRequest {
                plugin_id: PLUGIN_ID.to_owned(),
                plugin_version: env!("CARGO_PKG_VERSION").to_owned(),
                api_major: API_MAJOR,
                api_minor: API_MINOR,
                session_credential: credential.clone(),
                requested_capabilities: vec![CAPABILITY.to_owned()],
            })
            .await
            .map_err(|_| "session_rejected")?
            .into_inner();

        let records = tokio::task::spawn_blocking(scan_steam)
            .await
            .map_err(|_| "scan_failed")?
            .map_err(|_| "steam_unavailable")?;
        let records = records
            .into_iter()
            .map(|record| DiscoveryRecord {
                id: record.id,
                name: record.name.unwrap_or_default(),
                install_dir: record.install_dir,
                fields: record.fields.into_iter().collect(),
            })
            .collect();

        client
            .submit_discovery(SubmitDiscoveryRequest {
                session_id: session.session_id,
                session_credential: credential,
                operation_id,
                outcome: Some(Outcome::Completed(DiscoveryResult { records })),
            })
            .await
            .map_err(|_| "result_rejected")?;
        Ok(())
    }

    async fn connect(socket: PathBuf) -> Result<Channel, &'static str> {
        let endpoint =
            Endpoint::from_static("http://[::]:50051").connect_timeout(Duration::from_secs(3));
        endpoint
            .connect_with_connector(tower::service_fn(move |_| {
                let socket = socket.clone();
                async move {
                    tokio::net::UnixStream::connect(socket)
                        .await
                        .map(hyper_util::rt::TokioIo::new)
                }
            }))
            .await
            .map_err(|_| "host_unavailable")
    }

    fn scan_steam() -> Result<Vec<gamenode_core::provider::Record>, ()> {
        let loaded = load(
            &[("steam.toml", include_str!("../../../providers/steam.toml"))],
            Platform::Linux,
        );
        let provider = loaded.get("steam").ok_or(())?;
        provider
            .scan(Platform::Linux, &StdHostFiles, &SystemRoots::new())
            .map_err(|_| ())
    }

    fn required(name: &'static str) -> Result<String, &'static str> {
        env::var(name).map_err(|_| "missing_launch_configuration")
    }

    fn required_path(name: &'static str) -> Result<PathBuf, &'static str> {
        required(name).map(PathBuf::from)
    }

    fn decode_hex(value: String) -> Result<Vec<u8>, &'static str> {
        if value.len() % 2 != 0 || !value.is_ascii() {
            return Err("invalid_session_credential");
        }
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|digits| {
                let high = hex_digit(digits[0]).ok_or("invalid_session_credential")?;
                let low = hex_digit(digits[1]).ok_or("invalid_session_credential")?;
                Ok((high << 4) | low)
            })
            .collect()
    }

    fn hex_digit(digit: u8) -> Option<u8> {
        match digit {
            b'0'..=b'9' => Some(digit - b'0'),
            b'a'..=b'f' => Some(digit - b'a' + 10),
            _ => None,
        }
    }
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() {
    if let Err(code) = linux::run().await {
        tracing::warn!(failure = code, "Steam discovery plugin failed");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {}
