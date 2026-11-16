//! Generic application-facing provider runtime. Provider business logic lives
//! behind the public API; this layer owns process availability and initialization.

use super::{
    login::LoginBroker,
    managed_packages::{ManagedPackageLifecycle, PackageLifecycleError},
    process::{ProcessConfig, ProviderProcesses},
    profile::PluginStateLayout,
    provider::{Completion, ProviderApi, Registration},
    services::{ExternalAccountPersistence, ProviderServices},
};
use gamenode_plugin_protocol::{
    capability as cap, models,
    proto::{OperationDisposition, ProviderContext, ProviderOperation},
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tonic::Status;

struct UnavailableExternalAccounts;

impl ExternalAccountPersistence for UnavailableExternalAccounts {
    fn load(
        &self,
        _owner: &super::namespace::NamespaceOwner,
        _provider: &str,
    ) -> Result<Option<models::StoreAccount>, Status> {
        Ok(None)
    }

    fn save(
        &self,
        _owner: &super::namespace::NamespaceOwner,
        _account: models::StoreAccount,
        _clear_ownership: bool,
    ) -> Result<(), Status> {
        Err(Status::unavailable(
            "plugin account storage is not configured",
        ))
    }

    fn clear(
        &self,
        _owner: &super::namespace::NamespaceOwner,
        _provider: &str,
    ) -> Result<(), Status> {
        Err(Status::unavailable(
            "plugin account storage is not configured",
        ))
    }
}

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

pub(crate) fn managed_process_config(
    installed: &gamenode_platform::plugin_registry::InstalledPluginVersion,
    report: &super::managed_packages::PackageCandidateReport,
    publisher_key: [u8; 32],
) -> Result<ProcessConfig, Status> {
    let manifest = &report.manifest;
    if manifest.id != installed.plugin_id || manifest.version != installed.version {
        return Err(Status::failed_precondition(
            "managed package identity does not match its registry record",
        ));
    }
    if manifest.kind != super::package::PluginKind::Provider {
        return Err(Status::failed_precondition(
            "only provider packages have a supervised process runtime",
        ));
    }
    let provider_id = manifest
        .provider_id
        .clone()
        .ok_or_else(|| Status::failed_precondition("provider package has no provider id"))?;
    let target = report
        .selected_target()
        .map_err(|error| Status::failed_precondition(error.to_string()))?;
    let mut manifest_capabilities = vec![cap::CONTEXT.to_owned()];
    let mut required_capabilities = Vec::new();
    for permission in &manifest.permissions {
        if cap::KNOWN.contains(&permission.id.as_str()) {
            manifest_capabilities.push(permission.id.clone());
            if permission.required {
                required_capabilities.push(permission.id.clone());
            }
        } else if permission.required {
            return Err(Status::failed_precondition(
                "required provider permission is unsupported",
            ));
        }
    }
    manifest_capabilities.sort();
    manifest_capabilities.dedup();
    required_capabilities.sort();
    required_capabilities.dedup();
    Ok(ProcessConfig {
        provider_id,
        executable: installed.package_path.join(&target.target.entrypoint),
        arguments: vec![],
        environment: vec![],
        startup_timeout: Duration::from_secs(10),
        manifest_capabilities,
        required_capabilities,
        blocked_capabilities: vec![],
        integrity: Some(super::process::PackageIntegrity {
            package_root: installed.package_path.clone(),
            publisher_key,
            emulation_policy: gamenode_platform::plugin_environment::EmulationPolicy::RequireNative,
        }),
    })
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
    registry: gamenode_platform::plugin_registry::PluginPackageRegistry,
    packages: ManagedPackageLifecycle,
    mutation: tokio::sync::Mutex<()>,
}

impl ThirdPartyPluginLifecycle {
    pub fn new(
        api: ProviderApi,
        processes: ProviderProcesses,
        state: PluginStateLayout,
        registry: gamenode_platform::plugin_registry::PluginPackageRegistry,
        packages: ManagedPackageLifecycle,
    ) -> Self {
        Self {
            api,
            processes,
            state,
            registry,
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

    /// Atomically activates an already-installed candidate only after its supervised process
    /// completes the bounded Ready handshake with the account's current grants.
    pub(crate) async fn activate_update(
        &self,
        plugin_id: String,
        previous_version: String,
        installed: gamenode_platform::plugin_registry::InstalledPluginVersion,
        config: ProcessConfig,
    ) -> Result<(), Status> {
        let _mutation = self.mutation.lock().await;
        if installed.plugin_id != plugin_id || installed.version == previous_version {
            return Err(Status::invalid_argument("invalid staged plugin update"));
        }
        let provider_ids = self.api.third_party_provider_ids_for_plugin(&plugin_id);
        if provider_ids.len() != 1 || provider_ids[0] != config.provider_id {
            return Err(Status::failed_precondition(
                "managed plugin has no unique registered provider runtime",
            ));
        }
        let profile = self.api.profiles().require()?;
        let profile_epoch = profile.epoch();
        let state = self.state.clone();
        let account = profile.clone();
        let enabled = tokio::task::spawn_blocking(move || {
            state
                .open_activation(&account)
                .map(|activation| activation.is_enabled(&plugin_id))
                .map_err(|error| Status::internal(format!("plugin activation state: {error}")))
        })
        .await
        .map_err(|_| Status::internal("plugin activation worker failed"))??;
        let previous_config = self.processes.config(&config.provider_id)?;
        self.processes.stop_plugin(&installed.plugin_id).await;

        let registry = self.registry.clone();
        let candidate_id = installed.plugin_id.clone();
        let candidate_version = installed.version.clone();
        let expected_previous = previous_version.clone();
        let transaction = tokio::task::spawn_blocking(move || {
            registry.begin_activation(&candidate_id, &expected_previous, &candidate_version)
        })
        .await
        .map_err(|_| Status::internal("plugin activation worker failed"))?
        .map_err(|error| Status::failed_precondition(error.to_string()))?;

        let result = async {
            if !config.blocked_capabilities.is_empty() {
                let (plugin_id, publisher_fingerprint) = self
                    .api
                    .third_party_identity(&config.provider_id)
                    .ok_or_else(|| Status::failed_precondition("plugin identity is unavailable"))?;
                let blocked = config.blocked_capabilities.iter().cloned().collect();
                let state = self.state.clone();
                let account = profile.clone();
                tokio::task::spawn_blocking(move || {
                    let mut grants = state
                        .open_grants(&account)
                        .map_err(|error| Status::internal(format!("plugin grants: {error}")))?;
                    grants
                        .revoke_permissions(
                            &super::grants::PluginIdentity {
                                plugin_id,
                                publisher_fingerprint,
                                profile_id: account.profile_id().to_owned(),
                            },
                            &blocked,
                        )
                        .map_err(|error| {
                            Status::internal(format!("plugin grant revocation: {error}"))
                        })
                })
                .await
                .map_err(|_| Status::internal("plugin grant worker failed"))??;
            }
            self.processes
                .replace_config(&config.provider_id, config.clone())
                .await?;
            if !self.api.profiles().is_current(profile_epoch) {
                return Err(Status::failed_precondition(
                    "plugin profile changed during update activation",
                ));
            }
            self.processes
                .ensure_ready_for_update(&config.provider_id)
                .await?;
            if !self.api.profiles().is_current(profile_epoch) {
                return Err(Status::failed_precondition(
                    "plugin profile changed during update readiness check",
                ));
            }
            if !enabled {
                self.processes.stop_provider(&config.provider_id).await;
            }
            let registry = self.registry.clone();
            let transaction_to_commit = transaction.clone();
            let commit = tokio::task::spawn_blocking(move || {
                registry.commit_activation(transaction_to_commit)
            })
            .await;
            match commit {
                Ok(result) => result.map_err(|error| Status::internal(error.to_string()))?,
                Err(_) => {
                    // The blocking worker cannot be aborted after it starts. Retry by transaction
                    // idempotently in case it durably committed before its JoinError was observed.
                    let registry = self.registry.clone();
                    let retry = transaction.clone();
                    tokio::task::spawn_blocking(move || registry.commit_activation(retry))
                        .await
                        .map_err(|_| Status::internal("plugin activation worker failed"))?
                        .map_err(|error| Status::internal(error.to_string()))?;
                }
            }
            Ok(())
        }
        .await;

        if let Err(error) = result {
            self.processes.stop_provider(&config.provider_id).await;
            let registry = self.registry.clone();
            let transaction_to_rollback = transaction;
            let rollback = tokio::task::spawn_blocking(move || {
                registry.rollback_activation(transaction_to_rollback)
            })
            .await
            .map_err(|_| Status::internal("plugin rollback worker failed"))?;
            if let Err(rollback_error) = rollback {
                return Err(Status::internal(format!(
                    "candidate activation failed ({error}); rollback is pending recovery ({rollback_error})"
                )));
            }
            let previous_provider_id = previous_config.provider_id.clone();
            self.processes
                .replace_config(&previous_provider_id, previous_config)
                .await?;
            if enabled && self.api.profiles().is_current(profile_epoch) {
                if let Err(restart_error) = self.processes.ensure_started(&config.provider_id).await
                {
                    return Err(Status::unavailable(format!(
                        "candidate activation failed ({error}); previous version was restored but could not restart ({restart_error})"
                    )));
                }
            }
            return Err(error);
        }
        Ok(())
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
        let registry_paths = paths.clone();
        let registry = tokio::task::spawn_blocking(move || {
            gamenode_platform::plugin_registry::PluginPackageRegistry::open(&registry_paths)
        })
        .await
        .map_err(|_| Status::internal("plugin package registry worker failed"))?
        .map_err(|_| Status::internal("plugin package registry is unavailable"))?;
        let packages = ManagedPackageLifecycle::new(registry.clone());
        let trust_path = state.trust_dir();
        let trust = tokio::task::spawn_blocking(move || {
            super::publisher_trust::PublisherTrustStore::open(trust_path)
        })
        .await
        .map_err(|_| Status::internal("publisher trust worker failed"))?
        .map_err(|_| Status::internal("publisher trust store is unavailable"))?;
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
                manifest_capabilities: vec![],
                required_capabilities: vec![],
                blocked_capabilities: vec![],
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

        let active_packages = {
            let registry = registry.clone();
            tokio::task::spawn_blocking(move || registry.active_versions())
                .await
                .map_err(|_| Status::internal("plugin package registry worker failed"))?
                .map_err(|_| Status::internal("plugin package registry is unavailable"))?
        };
        for installed in active_packages {
            let package_root = installed.package_path.clone();
            let manifest = tokio::task::spawn_blocking(move || {
                let path = package_root.join("plugin.json");
                let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
                    Status::failed_precondition(format!("plugin manifest unavailable: {error}"))
                })?;
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || metadata.len() > 4 * 1024 * 1024
                {
                    return Err(Status::failed_precondition(
                        "plugin manifest is not a bounded regular file",
                    ));
                }
                let bytes = std::fs::read(path).map_err(|error| {
                    Status::failed_precondition(format!("plugin manifest unavailable: {error}"))
                })?;
                super::package::Manifest::parse_json(&bytes).map_err(|error| {
                    Status::failed_precondition(format!("plugin manifest is invalid: {error}"))
                })
            })
            .await
            .map_err(|_| Status::internal("plugin manifest worker failed"))?;
            let manifest = match manifest {
                Ok(manifest) => manifest,
                Err(error) => {
                    tracing::warn!(plugin_id = %installed.plugin_id, %error, "managed plugin is not eligible for runtime registration");
                    continue;
                }
            };
            let plugin_manifest = manifest.manifest();
            if plugin_manifest.id != installed.plugin_id
                || plugin_manifest.version != installed.version
            {
                tracing::warn!(plugin_id = %installed.plugin_id, "managed plugin registry identity does not match its manifest");
                continue;
            }
            let publisher_key = manifest.publisher_key();
            let publisher_fingerprint = match trust.inspect(&installed.plugin_id, &publisher_key) {
                Ok(super::publisher_trust::KeyStatus::Trusted { fingerprint }) => fingerprint,
                _ => {
                    tracing::warn!(plugin_id = %installed.plugin_id, "managed plugin publisher is not currently trusted");
                    continue;
                }
            };
            let report = match packages
                .inspect_candidate(
                    &installed.package_path,
                    publisher_key,
                    gamenode_platform::plugin_environment::EmulationPolicy::RequireNative,
                )
                .await
            {
                Ok(report) => report,
                Err(error) => {
                    tracing::warn!(plugin_id = %installed.plugin_id, %error, "managed plugin failed package validation");
                    continue;
                }
            };
            let config = match managed_process_config(&installed, &report, publisher_key) {
                Ok(config) => config,
                Err(error) => {
                    tracing::warn!(plugin_id = %installed.plugin_id, %error, "managed plugin has no usable provider runtime configuration");
                    continue;
                }
            };
            let provider_id = config.provider_id.clone();
            let services = Arc::new(ProviderServices::new_external(
                ProviderContext {
                    provider_id: provider_id.clone(),
                    ..Default::default()
                },
                paths.clone(),
                library.clone(),
                Arc::new(UnavailableExternalAccounts),
                login.clone(),
                None,
            )?);
            if let Err(error) = api.register(Registration {
                provider_id: provider_id.clone(),
                plugin_id: installed.plugin_id.clone(),
                kind: super::provider::RegistrationKind::Provider,
                origin: super::provider::RegistrationOrigin::ThirdParty {
                    publisher_fingerprint,
                },
                capabilities: config.manifest_capabilities.clone(),
                shared_catalog: false,
                services: services.clone(),
            }) {
                tracing::warn!(plugin_id = %installed.plugin_id, %error, "managed plugin registration was rejected");
                continue;
            }
            configs.push(config);
            entries.insert(
                provider_id,
                Entry {
                    services,
                    initialized: tokio::sync::Mutex::new(None),
                    packaged: true,
                    initialize: true,
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
        let lifecycle = Arc::new(ThirdPartyPluginLifecycle::new(
            api.clone(),
            processes.clone(),
            state,
            registry,
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

#[cfg(test)]
mod managed_runtime_tests {
    use super::*;

    #[test]
    fn managed_process_config_uses_the_selected_signed_provider_target() {
        let target = super::super::package::Target {
            os: "linux".into(),
            arch: "x86_64".into(),
            abi: "gnu".into(),
            entrypoint: "bin/provider".into(),
            requires: None,
            tested_on: vec![],
        };
        let manifest = super::super::package::Manifest {
            schema: 1,
            id: "org.example.community".into(),
            kind: super::super::package::PluginKind::Provider,
            provider_id: Some("community".into()),
            name: "Community provider".into(),
            version: "1.2.3".into(),
            api: super::super::package::ApiRange {
                min: "1.6.0".into(),
                max: "1.6.0".into(),
            },
            permissions: vec![super::super::package::PermissionRequest {
                id: cap::CONTEXT.into(),
                required: true,
                scope: "provider session".into(),
                reason: "Open a provider session".into(),
            }],
            native_access: vec![],
            targets: vec![target.clone()],
            publisher: super::super::package::Publisher {
                key_id: "publisher-key".into(),
                public_key: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
            },
            license: super::super::package::LicenseNotice {
                expression: "MIT".into(),
                files: vec!["LICENSE".into()],
                notices: vec![],
            },
            source_url: None,
            update_url: None,
            configuration: Default::default(),
            resources: BTreeMap::new(),
            schemas: vec![],
        };
        let package_path = PathBuf::from("managed-package");
        let installed = gamenode_platform::plugin_registry::InstalledPluginVersion {
            plugin_id: manifest.id.clone(),
            version: manifest.version.clone(),
            package_path: package_path.clone(),
        };
        let report = super::super::managed_packages::PackageCandidateReport {
            manifest,
            environment: gamenode_platform::plugin_environment::PluginEnvironment::default(),
            targets: vec![super::super::compatibility::TargetCompatibilityAssessment {
                target,
                verdict: super::super::compatibility::CompatibilityVerdict {
                    state: super::super::compatibility::CompatibilityState::Compatible,
                    reasons: vec![],
                    unknown_facts: vec![],
                },
            }],
        };

        let config = managed_process_config(&installed, &report, [7; 32]).unwrap();
        assert_eq!(config.provider_id, "community");
        assert_eq!(config.executable, package_path.join("bin/provider"));
        assert_eq!(config.manifest_capabilities, [cap::CONTEXT]);
        assert_eq!(config.required_capabilities, [cap::CONTEXT]);
        assert!(config.blocked_capabilities.is_empty());
        assert_eq!(
            config.integrity.unwrap().package_root,
            installed.package_path
        );
    }
}
