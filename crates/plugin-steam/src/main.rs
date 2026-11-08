#[cfg(target_os = "linux")]
mod linux {
    use serde::Deserialize;
    use std::{collections::BTreeSet, env, fs, path::PathBuf, time::Duration};

    use gamenode_plugin_protocol::{
        API_MAJOR, API_MINOR,
        proto::{
            DiscoveryRecord, DiscoveryResult, OpenSessionRequest, SubmitDiscoveryRequest,
            plugin_host_client::PluginHostClient, submit_discovery_request::Outcome,
        },
    };
    use gamenode_scanner::{Files, ScanInput, steam};
    use tonic::transport::{Channel, Endpoint};

    const PLUGIN_ID: &str = "org.gamenode.plugin.steam-discovery";
    const CAPABILITY: &str = "library.discovery";

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
                id: record.id.unwrap_or_default(),
                name: record.name.unwrap_or_default(),
                install_dir: record.install_dir.unwrap_or_default(),
                fields: record.fields.into_iter().collect(),
            })
            .collect();

        client
            .submit_discovery(SubmitDiscoveryRequest {
                session_id: session.session_id,
                session_credential: credential,
                operation_id,
                outcome: Some(Outcome::Completed(DiscoveryResult {
                    records,
                    provider_id: "steam".to_owned(),
                })),
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

    fn scan_steam() -> Result<Vec<gamenode_scanner::RawRecord>, ()> {
        let home = env::var_os("HOME").map(PathBuf::from).ok_or(())?;
        let roots = [
            home.join(".steam/steam"),
            home.join(".local/share/Steam"),
            home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
        ]
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
        let mut records = steam::scan(&ScanInput {
            files: &StdFiles,
            roots: &roots,
        })
        .map_err(|_| ())?;
        #[derive(Deserialize)]
        struct SteamManifest {
            #[serde(default)]
            exclude_ids: Vec<String>,
        }
        let manifest: SteamManifest =
            toml::from_str(include_str!("../../../providers/steam.toml")).map_err(|_| ())?;
        let excluded = manifest.exclude_ids.into_iter().collect::<BTreeSet<_>>();
        let mut seen = BTreeSet::new();
        records.retain(|record| {
            let Some(id) = record.id.as_deref() else {
                return false;
            };
            let id = id.trim();
            !id.is_empty() && !excluded.contains(id) && seen.insert(id.to_owned())
        });
        Ok(records)
    }

    struct StdFiles;

    impl Files for StdFiles {
        fn canonicalize(&self, path: &str) -> Result<String, String> {
            fs::canonicalize(path)
                .map(|path| path.to_string_lossy().into_owned())
                .map_err(|error| error.to_string())
        }

        fn read_dir(&self, path: &str) -> Result<Vec<String>, String> {
            fs::read_dir(path)
                .map_err(|error| error.to_string())?
                .map(|entry| {
                    entry
                        .map_err(|error| error.to_string())?
                        .file_name()
                        .into_string()
                        .map_err(|_| "filesystem entry name is not UTF-8".to_owned())
                })
                .collect()
        }

        fn read_to_string(&self, path: &str) -> Result<String, String> {
            fs::read_to_string(path).map_err(|error| error.to_string())
        }

        fn is_dir(&self, path: &str) -> bool {
            fs::metadata(path).is_ok_and(|metadata| metadata.is_dir())
        }
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
