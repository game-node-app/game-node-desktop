//! Generic application-facing provider runtime. Provider business logic lives
//! behind the public API; this layer owns process availability and initialization.

use super::{
    login::LoginBroker,
    process::{ProcessConfig, ProviderProcesses},
    provider::{Completion, ProviderApi, Registration},
    services::ProviderServices,
};
use gamenode_plugin_protocol::{
    capability as cap, models,
    proto::{OperationDisposition, ProviderOperation},
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tonic::Status;

pub type Observer = Arc<dyn Fn() + Send + Sync>;

#[async_trait::async_trait]
pub trait ProviderRuntime: Send + Sync {
    fn accounts(&self) -> Vec<String>;
    fn has_provider(&self, provider: &str) -> bool;
    fn status(&self, provider: &str) -> Option<models::AccountStatus>;
    fn available(&self, provider: &str) -> bool;
    fn availability(&self, provider: &str) -> &'static str {
        if self.available(provider) {
            "installed"
        } else {
            "unavailable"
        }
    }
    fn connected(&self, _provider: &str) -> bool {
        false
    }
    fn set_observer(&self, observer: Observer);
    fn set_account_persistence(&self, _accounts: Arc<dyn super::services::AccountPersistence>) {}
    async fn execute(
        &self,
        provider: &str,
        operation: ProviderOperation,
    ) -> Result<Completion, Status>;
    async fn execute_achievements(
        &self,
        _provider: &str,
        _request: models::AchievementsRequest,
    ) -> Result<Completion, Status> {
        Err(Status::unimplemented("achievement reads are unavailable"))
    }
    async fn shutdown(&self);
}

struct Entry {
    services: Arc<ProviderServices>,
    initialized: tokio::sync::Mutex<Option<u32>>,
    packaged: bool,
    initialize: bool,
}

pub struct HostedProviders {
    api: ProviderApi,
    processes: ProviderProcesses,
    endpoint_available: bool,
    entries: BTreeMap<String, Entry>,
    failures: Mutex<BTreeMap<String, String>>,
    observer: Mutex<Option<Observer>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl HostedProviders {
    pub async fn bundled(
        api: ProviderApi,
        endpoint: Option<PathBuf>,
        paths: gamenode_platform::paths::DataPaths,
        library: Arc<dyn gamenode_core::library::LibraryStore>,
        login: Option<Arc<LoginBroker>>,
    ) -> Result<Arc<Self>, Status> {
        let executable_dir = tokio::task::spawn_blocking(|| {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(std::path::Path::to_owned))
        })
        .await
        .map_err(|_| Status::unavailable("provider directory unavailable"))?
        .ok_or_else(|| Status::unavailable("provider directory unavailable"))?;
        let mut entries = BTreeMap::new();
        let mut configs = vec![];
        for descriptor in super::descriptors().iter().filter(|p| p.has(cap::CONTEXT)) {
            if gamenode_core::provider::Platform::current()
                .is_none_or(|platform| !descriptor.supports(platform))
            {
                continue;
            }
            let services = Arc::new(
                ProviderServices::bundled(
                    &descriptor.provider_id,
                    paths.clone(),
                    library.clone(),
                    login.clone(),
                )
                .await?,
            );
            api.register(Registration {
                provider_id: descriptor.provider_id.clone(),
                plugin_id: descriptor.plugin_id.clone(),
                capabilities: descriptor.capabilities.clone(),
                shared_catalog: descriptor.shared_catalog,
                services: services.clone(),
            })?;
            let executable = executable_dir.join(descriptor.executable_name());
            let checked = executable.clone();
            let packaged = tokio::task::spawn_blocking(move || checked.is_file())
                .await
                .unwrap_or(false);
            configs.push(ProcessConfig {
                provider_id: descriptor.provider_id.clone(),
                executable,
                arguments: vec![],
                environment: vec![],
                startup_timeout: Duration::from_secs(10),
            });
            entries.insert(
                descriptor.provider_id.clone(),
                Entry {
                    services,
                    initialized: tokio::sync::Mutex::new(None),
                    packaged,
                    initialize: descriptor.has(cap::CONTEXT),
                },
            );
        }
        let endpoint_available = endpoint.is_some();
        let processes = ProviderProcesses::new(api.clone(), endpoint.unwrap_or_default(), configs)?;
        Ok(Arc::new(Self {
            api,
            processes,
            endpoint_available,
            entries,
            failures: Mutex::new(BTreeMap::new()),
            observer: Mutex::new(None),
        }))
    }

    pub fn availability(&self, id: &str) -> &'static str {
        let Some(entry) = self.entries.get(id) else {
            return "unavailable";
        };
        if !entry.packaged {
            return "missing";
        }
        if !self.endpoint_available {
            return "unavailable";
        }
        if let Some(reason) = lock(&self.failures).get(id) {
            return if reason == "incompatible" {
                "incompatible"
            } else {
                "failed"
            };
        }
        "installed"
    }

    pub fn services(&self, provider: &str) -> Option<Arc<ProviderServices>> {
        self.entries.get(provider).map(|e| e.services.clone())
    }

    async fn initialized(&self, id: &str) -> Result<(), Status> {
        if !self.endpoint_available {
            return Err(Status::unavailable("provider endpoint unavailable"));
        }
        let entry = self
            .entries
            .get(id)
            .ok_or_else(|| Status::unavailable("provider is not registered"))?;
        self.processes.ensure_started(id).await?;
        let mut initialized = entry.initialized.lock().await;
        let pid = self
            .processes
            .process_id(id)
            .await
            .ok_or_else(|| Status::unavailable("provider process unavailable"))?;
        if *initialized == Some(pid) {
            return Ok(());
        }
        if !entry.initialize {
            *initialized = Some(pid);
            return Ok(());
        }
        let result = self
            .processes
            .execute(id, ProviderOperation::Initialize)
            .await?;
        if result.result.disposition != OperationDisposition::Completed as i32 {
            return Err(Status::unavailable("provider initialization failed"));
        }
        *initialized = Some(pid);
        Ok(())
    }

    fn record_result(&self, id: &str, result: &Result<Completion, Status>) {
        match result {
            Ok(_) => {
                lock(&self.failures).remove(id);
            }
            Err(error)
                if !matches!(
                    error.code(),
                    tonic::Code::FailedPrecondition | tonic::Code::Cancelled
                ) =>
            {
                lock(&self.failures).insert(
                    id.into(),
                    if error.code() == tonic::Code::Unimplemented {
                        "incompatible"
                    } else {
                        "provider_unavailable"
                    }
                    .into(),
                );
            }
            Err(_) => {}
        }
        self.notify();
    }

    fn notify(&self) {
        if let Some(observer) = lock(&self.observer).clone() {
            observer();
        }
    }
}

#[async_trait::async_trait]
impl ProviderRuntime for HostedProviders {
    fn accounts(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|(_, e)| e.services.initial_status().is_some())
            .map(|(id, _)| id.clone())
            .collect()
    }
    fn has_provider(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }
    fn status(&self, id: &str) -> Option<models::AccountStatus> {
        self.entries
            .get(id)
            .and_then(|e| e.services.status().or_else(|| e.services.initial_status()))
    }
    fn available(&self, id: &str) -> bool {
        matches!(self.availability(id), "installed" | "failed")
    }
    fn availability(&self, id: &str) -> &'static str {
        HostedProviders::availability(self, id)
    }
    fn connected(&self, id: &str) -> bool {
        self.api.connected(id)
    }
    fn set_observer(&self, observer: Observer) {
        for entry in self.entries.values() {
            entry.services.set_observer(observer.clone());
        }
        *lock(&self.observer) = Some(observer);
    }
    fn set_account_persistence(&self, accounts: Arc<dyn super::services::AccountPersistence>) {
        for entry in self.entries.values() {
            entry.services.set_account_persistence(accounts.clone());
        }
    }
    async fn execute(&self, id: &str, operation: ProviderOperation) -> Result<Completion, Status> {
        let result = async {
            self.initialized(id).await?;
            self.processes.execute(id, operation).await
        }
        .await;
        self.record_result(id, &result);
        result
    }
    async fn execute_achievements(
        &self,
        id: &str,
        request: models::AchievementsRequest,
    ) -> Result<Completion, Status> {
        let result = async {
            self.initialized(id).await?;
            self.processes.execute_achievements(id, request).await
        }
        .await;
        self.record_result(id, &result);
        result
    }
    async fn shutdown(&self) {
        self.processes.shutdown().await;
        for id in self.entries.keys() {
            let _ = tokio::time::timeout(Duration::from_secs(3), self.api.disconnect(id)).await;
        }
    }
}
