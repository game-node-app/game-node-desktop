use std::collections::HashMap;

use gamenode_plugin_sdk::{
    Error, HostClient, ProviderPlugin, capability,
    models::CatalogRecord,
    proto::{
        CatalogCategory, CatalogCompletion, CounterObservation, DiscoveryRecord, DiscoveryResult,
        OperationDisposition, OperationResult, ProviderCommand, ProviderContext, ProviderOperation,
    },
};

pub const PLUGIN_ID: &str = "org.example.gamenode.rust-mvp";
pub const PROVIDER_ID: &str = "example-rust";
pub const PLUGIN_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const CAPABILITIES: &[&str] = &[
    capability::CONTEXT,
    capability::DISCOVERY,
    capability::CATALOG,
    capability::COUNTER,
];

#[derive(Default)]
pub struct MvpProvider;

impl MvpProvider {
    fn discovery() -> DiscoveryResult {
        DiscoveryResult {
            provider_id: PROVIDER_ID.into(),
            records: vec![DiscoveryRecord {
                id: "demo-installed-game".into(),
                name: "Example Adventure".into(),
                install_dir: "/fictional/games/example-adventure".into(),
                fields: HashMap::from([("source".into(), "rust-sdk-example".into())]),
                target: None,
            }],
            ..Default::default()
        }
    }

    fn catalog() -> Vec<CatalogRecord> {
        vec![
            CatalogRecord::new("demo-catalog-game", "Example Adventure")
                .with_field("edition", "standard")
                .expect("static catalog field is valid"),
        ]
    }

    fn counter() -> CounterObservation {
        CounterObservation {
            id: "demo-catalog-game".into(),
            seconds: 7_200,
            last_played: Some(1_700_000_000),
        }
    }

    async fn check_context(host: &HostClient) -> Result<ProviderContext, Error> {
        let context = host.provider_context().await?;
        if context.provider_id != PROVIDER_ID {
            return Err(Error::Protocol);
        }
        Ok(context)
    }
}

#[async_trait::async_trait]
impl ProviderPlugin for MvpProvider {
    async fn handle(
        &self,
        command: ProviderCommand,
        host: HostClient,
    ) -> Result<OperationResult, Error> {
        Self::check_context(&host).await?;
        let disposition = OperationDisposition::Completed as i32;
        let mut result = OperationResult {
            disposition,
            ..Default::default()
        };

        match ProviderOperation::try_from(command.operation).map_err(|_| Error::Protocol)? {
            ProviderOperation::Initialize | ProviderOperation::Check => {}
            ProviderOperation::Discover => result.discovery = Some(Self::discovery()),
            ProviderOperation::SyncCatalog => {
                let completion = host
                    .catalog_chunks(CatalogCategory::Owned, Self::catalog())
                    .await?;
                result.catalogs.push(CatalogCompletion {
                    category: completion.category,
                    outcome: completion.outcome,
                });
            }
            ProviderOperation::ReadCounter => result.counters.push(Self::counter()),
            _ => return Err(Error::Protocol),
        }
        Ok(result)
    }
}
