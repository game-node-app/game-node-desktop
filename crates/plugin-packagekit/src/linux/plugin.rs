//! The `gamenode-plugin-sdk::ProviderPlugin` implementation.
//!
//! Discovery is the only supported operation. The plugin obtains the trusted provider context
//! from the host, checks that it really is the Linux `packagekit` provider, runs one bounded scan
//! and returns normalized records in `OperationResult.discovery`. Every other operation is
//! declined so the plugin keeps no account, catalog, action, settings or launch authority.

use std::{env, sync::Arc, time::Duration};

use async_trait::async_trait;
use gamenode_plugin_protocol::{capability, proto::*};
use gamenode_plugin_sdk::{Error, HostClient, Launch, ProviderPlugin, Session};

use super::scan::{self, MAX_RECORDS, PackageCandidate, ScanFailure};

pub(super) const PLUGIN_ID: &str = "org.gamenode.plugin.packagekit-discovery";
pub(super) const PROVIDER_ID: &str = "packagekit";
pub(super) const PLATFORM: &str = "linux";
const SCAN_TIMEOUT: Duration = Duration::from_secs(60);

/// A source of already-normalized package candidates.
///
/// The production source reads the system and queries the selected backend; fixtures substitute
/// a static source so the SDK contract can be exercised without host package metadata.
#[async_trait]
pub(super) trait DiscoverySource: Send + Sync {
    async fn discover(&self) -> Result<Vec<PackageCandidate>, ScanFailure>;
}

struct SystemDiscovery;

#[async_trait]
impl DiscoverySource for SystemDiscovery {
    async fn discover(&self) -> Result<Vec<PackageCandidate>, ScanFailure> {
        scan::discover().await
    }
}

pub(super) struct PackageManagerPlugin {
    source: Arc<dyn DiscoverySource>,
    timeout: Duration,
}

impl PackageManagerPlugin {
    fn system() -> Self {
        Self {
            source: Arc::new(SystemDiscovery),
            timeout: SCAN_TIMEOUT,
        }
    }

    #[cfg(test)]
    pub(super) fn with_source(source: Arc<dyn DiscoverySource>, timeout: Duration) -> Self {
        Self { source, timeout }
    }
}

#[async_trait]
impl ProviderPlugin for PackageManagerPlugin {
    async fn handle(
        &self,
        command: ProviderCommand,
        host: HostClient,
    ) -> Result<OperationResult, Error> {
        let operation =
            ProviderOperation::try_from(command.operation).map_err(|_| Error::Protocol)?;
        match operation {
            ProviderOperation::Initialize => Ok(OperationResult {
                disposition: OperationDisposition::Completed as i32,
                ..Default::default()
            }),
            ProviderOperation::Discover => {
                let context = host.provider_context().await?;
                if context.provider_id != PROVIDER_ID || context.platform != PLATFORM {
                    return Err(Error::Denied);
                }
                let candidates = tokio::time::timeout(self.timeout, self.source.discover())
                    .await
                    .map_err(|_| {
                        tracing::warn!(code = "scan_timeout", "Linux package discovery timed out");
                        Error::Unavailable
                    })?
                    .map_err(|failure| {
                        tracing::warn!(code = failure.code(), "Linux package discovery failed");
                        Error::Unavailable
                    })?;
                Ok(OperationResult {
                    disposition: OperationDisposition::Completed as i32,
                    discovery: Some(DiscoveryResult {
                        provider_id: context.provider_id,
                        records: discovery_records(candidates),
                        // Packages have no store install location or library folders.
                        ..Default::default()
                    }),
                    ..Default::default()
                })
            }
            _ => Err(Error::Denied),
        }
    }
}

/// Normalizes and bounds the provider-local records sent to the host.
fn discovery_records(candidates: Vec<PackageCandidate>) -> Vec<DiscoveryRecord> {
    candidates
        .into_iter()
        .take(MAX_RECORDS)
        .map(|record| DiscoveryRecord {
            id: record.id,
            name: record.name.unwrap_or_default(),
            install_dir: record.install_dir,
            fields: record.fields.into_iter().collect(),
            target: None,
        })
        .collect()
}

pub(crate) async fn run() -> Result<(), &'static str> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("warn")
        .with_writer(std::io::stderr)
        .try_init();
    let launch = Launch::from_env().map_err(|_| "invalid_launch_configuration")?;
    let session = Session::open(
        launch,
        PLUGIN_ID,
        PROVIDER_ID,
        env!("CARGO_PKG_VERSION"),
        &[capability::CONTEXT, capability::DISCOVERY],
    )
    .await
    .map_err(|_| "provider_session_unavailable")?;
    gamenode_plugin_sdk::run(session, Arc::new(PackageManagerPlugin::system()))
        .await
        .map_err(|_| "provider_session_failed")
}
