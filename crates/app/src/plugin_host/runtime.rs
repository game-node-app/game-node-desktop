//! Generic application-facing provider runtime. Provider business logic lives
//! behind the public API; this layer owns process availability and initialization.

use super::{
    login::LoginBroker,
    managed_packages::{ManagedPackageLifecycle, PackageLifecycleError},
    process::{ProcessConfig, ProviderProcesses},
    profile::PluginStateLayout,
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
    lifecycle: Arc<ThirdPartyPluginLifecycle>,
}

/// Account-scoped enable/disable controls for already registered managed plugins. Registration
/// and package validation remain host-owned; this service only starts verified process slots and
/// persists the user's per-account choice.
pub struct ThirdPartyPluginLifecycle {
    api: ProviderApi,
    processes: ProviderProcesses,
    state: PluginStateLayout,
    packages: ManagedPackageLifecycle,
    mutation: tokio::sync::Mutex<()>,
}

impl ThirdPartyPluginLifecycle {
    pub fn new(
        api: ProviderApi,
        processes: ProviderProcesses,
        state: PluginStateLayout,
        packages: ManagedPackageLifecycle,
    ) -> Self {
        Self {
            api,
            processes,
            state,
            packages,
            mutation: tokio::sync::Mutex::new(()),
        }
    }

    pub async fn enabled(&self, plugin_id: String) -> Result<bool, Status> {
        let profile = self.api.profiles().require()?;
        let state = self.state.clone();
        tokio::task::spawn_blocking(move || {
            state
                .open_activation(&profile)
                .map(|activation| activation.is_enabled(&plugin_id))
                .map_err(|error| Status::internal(format!("plugin activation state: {error}")))
        })
        .await
        .map_err(|_| Status::internal("plugin activation worker failed"))?
    }

    pub async fn enable(&self, plugin_id: String) -> Result<(), Status> {
        self.set_enabled(plugin_id, true).await
    }

    pub async fn disable(&self, plugin_id: String) -> Result<(), Status> {
        self.set_enabled(plugin_id, false).await
    }

    /// Starts the plugins enabled in the active account after registration has been restored.
    pub async fn start_enabled_plugins(&self) -> Result<(), Status> {
        let profile = self.api.profiles().require()?;
        let profile_epoch = profile.epoch();
        let state = self.state.clone();
        let account_profile = profile.clone();
        let enabled = tokio::task::spawn_blocking(move || {
            state
                .open_activation(&account_profile)
                .map(|activation| activation.enabled().map(str::to_owned).collect::<Vec<_>>())
                .map_err(|error| Status::internal(format!("plugin activation state: {error}")))
        })
        .await
        .map_err(|_| Status::internal("plugin activation worker failed"))??;
        for plugin_id in enabled {
            if !self.api.profiles().is_current(profile_epoch) {
                return Err(Status::failed_precondition(
                    "plugin profile changed during lifecycle restore",
                ));
            }
            self.set_enabled_for_profile(plugin_id, true, Some(profile_epoch))
                .await?;
        }
        Ok(())
    }

    async fn set_enabled(&self, plugin_id: String, enabled: bool) -> Result<(), Status> {
        self.set_enabled_for_profile(plugin_id, enabled, None).await
    }

    async fn set_enabled_for_profile(
        &self,
        plugin_id: String,
        enabled: bool,
        expected_epoch: Option<u64>,
    ) -> Result<(), Status> {
        let _mutation = self.mutation.lock().await;
        let profile = self.api.profiles().require()?;
        if expected_epoch.is_some_and(|expected| expected != profile.epoch()) {
            return Err(Status::failed_precondition(
                "plugin profile changed during lifecycle restore",
            ));
        }
        let process_ids = self.process_ids(&plugin_id)?;
        let state = self.state.clone();
        let account = profile.clone();
        let plugin = plugin_id.clone();
        tokio::task::spawn_blocking(move || {
            let mut activation = state
                .open_activation(&account)
                .map_err(|error| Status::internal(format!("plugin activation state: {error}")))?;
            activation
                .set_enabled(&plugin, enabled)
                .map_err(|error| Status::internal(format!("plugin activation state: {error}")))
        })
        .await
        .map_err(|_| Status::internal("plugin activation worker failed"))??;

        if !enabled {
            for provider_id in process_ids {
                self.api.disconnect(&provider_id).await;
                self.processes.stop_provider(&provider_id).await;
            }
            return Ok(());
        }

        if !self.api.profiles().is_current(profile.epoch())
            || expected_epoch.is_some_and(|expected| expected != profile.epoch())
        {
            return Err(Status::failed_precondition(
                "plugin profile changed during lifecycle restore",
            ));
        }
        for provider_id in &process_ids {
            if let Err(error) = self.processes.ensure_started(provider_id).await {
                for provider_id in &process_ids {
                    self.api.disconnect(provider_id).await;
                    self.processes.stop_provider(provider_id).await;
                }
                let state = self.state.clone();
                let account = profile;
                let plugin = plugin_id;
                let _ = tokio::task::spawn_blocking(move || {
                    if let Ok(mut activation) = state.open_activation(&account) {
                        let _ = activation.set_enabled(&plugin, false);
                    }
                })
                .await;
                return Err(error);
            }
        }
        Ok(())
    }

    pub async fn remove(&self, plugin_id: String) -> Result<(), Status> {
        let _mutation = self.mutation.lock().await;
        let provider_ids = self.api.third_party_provider_ids_for_plugin(&plugin_id);
        if provider_ids.is_empty() {
            return Err(Status::not_found("third-party plugin is not registered"));
        }
        if let Ok(profile) = self.api.profiles().require() {
            let state = self.state.clone();
            let plugin = plugin_id.clone();
            tokio::task::spawn_blocking(move || {
                let mut activation = state.open_activation(&profile).map_err(|error| {
                    Status::internal(format!("plugin activation state: {error}"))
                })?;
                activation
                    .set_enabled(&plugin, false)
                    .map_err(|error| Status::internal(format!("plugin activation state: {error}")))
            })
            .await
            .map_err(|_| Status::internal("plugin activation worker failed"))??;
        }
        for provider_id in &provider_ids {
            self.api.disconnect(provider_id).await;
            self.processes.stop_provider(provider_id).await;
        }
        self.api.unregister_third_party_plugin(&plugin_id).await?;
        self.packages
            .remove_plugin(plugin_id.clone())
            .await
            .map_err(package_status)?;
        let state = self.state.clone();
        tokio::task::spawn_blocking(move || state.remove_plugin_account_state(&plugin_id))
            .await
            .map_err(|_| Status::internal("plugin state removal worker failed"))?
            .map_err(|error| Status::internal(format!("plugin account state: {error}")))
    }

    fn process_ids(&self, plugin_id: &str) -> Result<Vec<String>, Status> {
        let registered = self.api.third_party_provider_ids_for_plugin(plugin_id);
        if registered.is_empty() {
            return Err(Status::not_found("third-party plugin is not registered"));
        }
        let process_ids = registered
            .into_iter()
            .filter(|provider_id| self.processes.has_provider(provider_id))
            .collect::<Vec<_>>();
        if process_ids.is_empty() {
            return Err(Status::failed_precondition(
                "third-party plugin has no verified process configuration",
            ));
        }
        Ok(process_ids)
    }

    pub(crate) fn spawn_profile_lifecycle(self: &Arc<Self>, runtime: &tokio::runtime::Handle) {
        let lifecycle = self.clone();
        let mut changes = self.api.profiles().subscribe();
        runtime.spawn(async move {
            let mut epoch = lifecycle.api.profiles().epoch();
            if lifecycle.api.profiles().current().is_some()
                && let Err(error) = lifecycle.start_enabled_plugins().await
            {
                tracing::warn!(%error, "could not restore enabled third-party plugins");
            }
            while changes.changed().await.is_ok() {
                let current_epoch = lifecycle.api.profiles().epoch();
                if current_epoch == epoch {
                    continue;
                }
                epoch = current_epoch;
                lifecycle.api.fence_third_party_sessions().await;
                for provider_id in lifecycle.api.third_party_provider_ids() {
                    lifecycle.processes.stop_provider(&provider_id).await;
                }
                if lifecycle.api.profiles().current().is_some()
                    && let Err(error) = lifecycle.start_enabled_plugins().await
                {
                    tracing::warn!(%error, "could not restore enabled third-party plugins");
                }
            }
        });
    }
}

fn package_status(error: PackageLifecycleError) -> Status {
    Status::internal(format!("plugin package lifecycle failed: {error}"))
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
        let state = PluginStateLayout::new(paths.plugin_data_dir().join("state"));
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
                kind: super::provider::RegistrationKind::Provider,
                origin: crate::plugin_host::provider::RegistrationOrigin::Official,
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
                integrity: None,
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
        let processes = ProviderProcesses::new_for_lifecycle(
            api.clone(),
            endpoint.unwrap_or_default(),
            configs,
            state.clone(),
        )?;
        let registry_paths = paths.clone();
        let registry = tokio::task::spawn_blocking(move || {
            gamenode_platform::plugin_registry::PluginPackageRegistry::open(&registry_paths)
        })
        .await
        .map_err(|_| Status::internal("plugin package registry worker failed"))?
        .map_err(|_| Status::internal("plugin package registry is unavailable"))?;
        let packages = ManagedPackageLifecycle::new(registry);
        let lifecycle = Arc::new(ThirdPartyPluginLifecycle::new(
            api.clone(),
            processes.clone(),
            state,
            packages,
        ));
        lifecycle.spawn_profile_lifecycle(&tokio::runtime::Handle::try_current().map_err(
            |_| Status::failed_precondition("plugin lifecycle requires an async runtime"),
        )?);
        Ok(Arc::new(Self {
            api,
            processes,
            endpoint_available,
            entries,
            failures: Mutex::new(BTreeMap::new()),
            observer: Mutex::new(None),
            lifecycle,
        }))
    }

    pub fn lifecycle(&self) -> Arc<ThirdPartyPluginLifecycle> {
        self.lifecycle.clone()
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
