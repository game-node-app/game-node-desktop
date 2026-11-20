//! Generic application-facing provider runtime. Provider business logic lives
//! behind the public API; this layer owns process availability and initialization.

use super::{
    grants::{PermissionRequest, PluginIdentity},
    login::LoginBroker,
    managed_packages::{ManagedPackageLifecycle, PackageLifecycleError, lock_publisher_identity},
    process::{
        ExtensionIntent, ExtensionLaunch, ExtensionTarget, PLUGIN_STOPPED, ProcessConfig,
        ProviderProcesses,
    },
    profile::{PluginProfiles, PluginStateLayout},
    provider::{Completion, ProviderApi, Registration},
    services::{ExternalAccountPersistence, ProviderServices},
    sharing_policy::{
        ActivationReadGrant, RecipientSharingSnapshot, SharedSharingPolicies, SharingPolicyError,
        SourceRecipientPolicy,
    },
};
use gamenode_platform::plugin_registry::{InstalledPluginVersion, PluginPackageRegistry};
use gamenode_plugin_protocol::{
    capability as cap, models,
    proto::{OperationDisposition, ProviderContext, ProviderOperation},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
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

#[tonic::async_trait]
pub trait RuntimeSessionHandler: Send + Sync {
    async fn report(
        &self,
        plugin_id: String,
        owner_key: String,
        report: gamenode_plugin_protocol::proto::RuntimeSessionReport,
    ) -> gamenode_plugin_protocol::proto::RuntimeSessionResult;

    async fn disconnected(&self, plugin_id: String);
}

#[async_trait::async_trait]
pub trait ProviderRuntime: Send + Sync {
    fn managed_plugin_inventory(&self) -> Vec<super::PluginInfo> {
        Vec::new()
    }
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
    fn set_runtime_session_handler(&self, _handler: Arc<dyn RuntimeSessionHandler>) {}
    async fn execute(
        &self,
        provider: &str,
        operation: ProviderOperation,
    ) -> Result<Completion, Status>;
    async fn start_extension(
        &self,
        _plugin_id: &str,
        _target: ExtensionTarget,
        _operation: super::intents::Operation,
    ) -> Result<(), Status> {
        Err(Status::unimplemented("extension actions are unavailable"))
    }
    async fn available_extensions(
        &self,
        _operation: super::intents::Operation,
    ) -> Result<Vec<super::PluginInfo>, Status> {
        Ok(Vec::new())
    }
    async fn execute_achievements(
        &self,
        _provider: &str,
        _request: models::AchievementsRequest,
    ) -> Result<Completion, Status> {
        Err(Status::unimplemented("achievement reads are unavailable"))
    }
    /// Stops the provider for this run, refusing later calls with the stable
    /// `plugin stopped` error. The default host has no supervised process to stop.
    async fn stop(&self, provider: &str) -> Result<(), Status> {
        Err(Status::unimplemented(format!(
            "the {provider} plugin cannot be stopped"
        )))
    }
    /// Starts a stopped provider with a fresh session, restoring its account.
    async fn start(&self, provider: &str) -> Result<(), Status> {
        Err(Status::unimplemented(format!(
            "the {provider} plugin cannot be started"
        )))
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
    let provider_id = match manifest.kind {
        super::package::PluginKind::Provider => manifest
            .provider_id
            .clone()
            .ok_or_else(|| Status::failed_precondition("provider package has no provider id"))?,
        super::package::PluginKind::Extension => {
            super::provider::extension_registration_id(&manifest.id)
        }
    };
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
            version: installed.version.clone(),
            publisher_key,
            emulation_policy: gamenode_platform::plugin_environment::EmulationPolicy::RequireNative,
        }),
    })
}

pub struct HostedProviders {
    api: ProviderApi,
    processes: ProviderProcesses,
    intents: Arc<super::intents::Intents>,
    endpoint_available: bool,
    entries: BTreeMap<String, Entry>,
    managed_plugins: Vec<super::PluginInfo>,
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
    #[cfg(test)]
    sharing_commit_hook: Mutex<Option<SharingCommitHook>>,
    #[cfg(test)]
    permission_commit_hook: Mutex<Option<PermissionCommitHook>>,
}

/// Host-owned permission state for one installed third-party provider in the active account.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginPermissionSnapshot {
    pub plugin_id: String,
    pub publisher_fingerprint: String,
    pub profile_id: String,
    pub grant_revision: u64,
    pub requested_permissions: Vec<super::grants::PermissionRequest>,
    pub effective_grants: Vec<super::grants::EffectiveGrant>,
}

/// Host-owned configuration availability and the active signed package revision.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginConfigurationState {
    pub plugin_id: String,
    pub available: bool,
    pub revision: Option<String>,
}

pub(crate) struct SharingIdentityObservation {
    pub identity: PluginIdentity,
    profile_epoch: u64,
}

#[cfg(test)]
#[derive(Clone, Copy)]
enum SharingCommitCheckpoint {
    BeforeWrite,
    AfterCommit,
}

#[cfg(test)]
type SharingCommitHook = Arc<dyn Fn(SharingCommitCheckpoint) + Send + Sync>;

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PermissionCommitCheckpoint {
    Prepared,
    BeforeWrite,
    AfterCommit,
}

#[cfg(test)]
type PermissionCommitHook = Arc<dyn Fn(PermissionCommitCheckpoint) + Send + Sync>;

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
            #[cfg(test)]
            sharing_commit_hook: Mutex::new(None),
            #[cfg(test)]
            permission_commit_hook: Mutex::new(None),
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

    pub fn require_active_profile(&self) -> Result<(), Status> {
        self.api.profiles().require().map(|_| ())
    }

    pub fn is_registered(&self, plugin_id: &str) -> bool {
        self.api
            .third_party_provider_ids_for_plugin(plugin_id)
            .len()
            == 1
    }

    pub(crate) fn extension_runtime_available(&self, plugin_id: &str) -> bool {
        let slot = super::provider::extension_registration_id(plugin_id);
        self.api.is_extension(&slot) && self.processes.extension_runtime_available(&slot)
    }

    pub async fn enable(&self, plugin_id: String) -> Result<(), Status> {
        self.set_enabled(plugin_id, true).await
    }

    pub async fn disable(&self, plugin_id: String) -> Result<(), Status> {
        self.set_enabled(plugin_id, false).await
    }

    /// Reads the persisted permission decisions for one registered provider in the active
    /// account. Disk access runs on a blocking worker, and a profile switch during that read
    /// invalidates the result rather than returning another account's grants.
    pub async fn permission_snapshot(
        &self,
        plugin_id: String,
    ) -> Result<PluginPermissionSnapshot, Status> {
        let profile = self.api.profiles().require()?;
        let provider_ids = self.api.third_party_provider_ids_for_plugin(&plugin_id);
        let [provider_id] = provider_ids.as_slice() else {
            return Err(Status::failed_precondition(
                "plugin has no unique registered third-party provider",
            ));
        };
        let (registered_plugin_id, publisher_fingerprint) = self
            .api
            .third_party_identity(provider_id)
            .ok_or_else(|| Status::failed_precondition("plugin identity is unavailable"))?;
        if registered_plugin_id != plugin_id {
            return Err(Status::failed_precondition(
                "registered plugin identity changed",
            ));
        }
        let requested_permissions = self
            .managed_permission_requests(&plugin_id, &publisher_fingerprint)
            .await?;
        let profile_epoch = profile.epoch();
        let identity = super::grants::PluginIdentity {
            plugin_id: registered_plugin_id.clone(),
            publisher_fingerprint: publisher_fingerprint.clone(),
            profile_id: profile.profile_id().to_owned(),
        };
        let state = self.state.clone();
        let account = profile.clone();
        let (grant_revision, effective_grants) = tokio::task::spawn_blocking(move || {
            let grants = state
                .open_grants(&account)
                .map_err(|error| Status::internal(format!("plugin grants unavailable: {error}")))?;
            Ok::<_, Status>((
                grants.revision(&identity),
                grants.effective_grants(&identity),
            ))
        })
        .await
        .map_err(|_| Status::internal("plugin permission worker failed"))??;
        if !self.api.profiles().is_current(profile_epoch) {
            return Err(Status::failed_precondition(
                "plugin profile changed while reading permissions",
            ));
        }
        Ok(PluginPermissionSnapshot {
            plugin_id: registered_plugin_id,
            publisher_fingerprint,
            profile_id: profile.profile_id().to_owned(),
            grant_revision,
            requested_permissions,
            effective_grants,
        })
    }

    /// Observe the registered, trusted publisher and the active profile together. Package
    /// signature verification uses the captured immutable installed version on a worker.
    pub(crate) async fn observe_sharing_identity(
        &self,
        plugin_id: String,
        expected_profile_id: &str,
    ) -> Result<SharingIdentityObservation, Status> {
        let api = self.api.clone();
        let profiles = api.profiles();
        let registry = self.registry.clone();
        let trust_dir = self.state.trust_dir();
        let expected_profile_id = expected_profile_id.to_owned();
        let (observation, installed, key) = tokio::task::spawn_blocking(move || {
            let _publisher = lock_publisher_identity();
            let profile = profiles.require()?;
            if profile.profile_id() != expected_profile_id {
                return Err(Status::failed_precondition("plugin profile changed"));
            }
            let fingerprint = registered_publisher(&api, &plugin_id)?;
            let (installed, key) =
                trusted_managed_package(&registry, &trust_dir, &plugin_id, &fingerprint)?;
            let observation = SharingIdentityObservation {
                identity: PluginIdentity {
                    plugin_id,
                    publisher_fingerprint: fingerprint,
                    profile_id: profile.profile_id().to_owned(),
                },
                profile_epoch: profile.epoch(),
            };
            Ok((observation, installed, key))
        })
        .await
        .map_err(|_| Status::internal("plugin identity worker failed"))??;
        self.verified_permission_requests(&installed, key).await?;
        Ok(observation)
    }

    pub(crate) async fn sharing_identity(
        &self,
        plugin_id: String,
        expected_profile_id: &str,
    ) -> Result<PluginIdentity, Status> {
        let observation = self
            .observe_sharing_identity(plugin_id, expected_profile_id)
            .await?;
        validate_sharing_profile(&self.api.profiles(), &observation)?;
        Ok(observation.identity)
    }

    pub(crate) async fn replace_sharing_policy(
        &self,
        observation: SharingIdentityObservation,
        policies: Arc<SharedSharingPolicies>,
        expected_revision: u64,
        grant: ActivationReadGrant,
        source_policies: Vec<SourceRecipientPolicy>,
        required_sources: Vec<PluginIdentity>,
    ) -> Result<RecipientSharingSnapshot, Status> {
        let api = self.api.clone();
        let registry = self.registry.clone();
        let trust_dir = self.state.trust_dir();
        #[cfg(test)]
        let commit_hook = lock(&self.sharing_commit_hook).clone();
        tokio::task::spawn_blocking(move || {
            // Fixed order: publisher identity, profile transition, then policy store.
            // Publisher changes are fenced from validation through the durable commit.
            let publisher = lock_publisher_identity();
            for identity in std::iter::once(&observation.identity).chain(&required_sources) {
                let current =
                    registered_publisher(&api, &identity.plugin_id).and_then(|registered| {
                        if registered != identity.publisher_fingerprint {
                            return Err(sharing_identity_conflict());
                        }
                        trusted_managed_package(
                            &registry,
                            &trust_dir,
                            &identity.plugin_id,
                            &registered,
                        )
                    });
                match current {
                    Ok(_) => {}
                    Err(error) if error.code() == tonic::Code::Internal => return Err(error),
                    Err(_) => return Err(sharing_identity_conflict()),
                }
            }
            let profiles = api.profiles();
            let transition = profiles.lock_transition();
            validate_sharing_profile(&profiles, &observation)?;
            #[cfg(test)]
            if let Some(hook) = &commit_hook {
                hook(SharingCommitCheckpoint::BeforeWrite);
            }
            let saved = policies
                .replace_recipient_policies_snapshot(
                    &observation.identity,
                    expected_revision,
                    grant,
                    source_policies,
                )
                .map_err(|error| match error {
                    SharingPolicyError::StaleRevision => Status::aborted(error.to_string()),
                    _ => Status::internal(error.to_string()),
                })?;
            drop(transition);
            drop(publisher);
            #[cfg(test)]
            if let Some(hook) = &commit_hook {
                hook(SharingCommitCheckpoint::AfterCommit);
            }
            Ok(saved)
        })
        .await
        .map_err(|_| Status::internal("shared-data permission worker failed"))?
    }

    /// Atomically replaces permissions only for the active account and the declarations in the
    /// currently trusted package. The reviewed publisher, profile and revision fence stale UI writes.
    pub(super) async fn replace_permissions(
        &self,
        activations: &tokio::sync::Mutex<()>,
        plugin_id: String,
        expected_profile_id: String,
        expected_publisher_fingerprint: String,
        expected_revision: u64,
        decisions: Vec<super::grants::PermissionDecision>,
    ) -> Result<PluginPermissionSnapshot, Status> {
        let profiles = self.api.profiles();
        let profile = profiles.require()?;
        if profile.profile_id() != expected_profile_id {
            return Err(Status::failed_precondition("plugin profile changed"));
        }
        // Capture the profile before waiting on update activation so even a round trip
        // to another account while queued invalidates this request.
        let _activation = activations.lock().await;
        let _mutation = self.mutation.lock().await;
        let (installed, key) = self
            .managed_package_for_permissions(&plugin_id, &expected_publisher_fingerprint)
            .await?;
        let requests = self.verified_permission_requests(&installed, key).await?;
        let identity = PluginIdentity {
            plugin_id,
            publisher_fingerprint: expected_publisher_fingerprint,
            profile_id: expected_profile_id,
        };
        // A permission change fences a running extension before the new grants are saved.
        let provider_ids = self
            .api
            .third_party_provider_ids_for_plugin(&identity.plugin_id);
        if let [provider_id] = provider_ids.as_slice()
            && self.api.is_extension(provider_id)
        {
            self.processes.terminate_provider(provider_id).await;
        }
        let state = self.state.clone();
        let registry = self.registry.clone();
        let api = self.api.clone();
        #[cfg(test)]
        let commit_hook = lock(&self.permission_commit_hook).clone();
        tokio::task::spawn_blocking(move || {
            #[cfg(test)]
            let checkpoint = |point| {
                if let Some(hook) = &commit_hook {
                    hook(point);
                }
            };
            #[cfg(test)]
            checkpoint(PermissionCommitCheckpoint::Prepared);

            // Match the shared-data commit's lock order. The package was verified outside these
            // locks. Recheck its immutable version and trusted publisher before saving.
            let publisher = lock_publisher_identity();
            if registered_publisher(&api, &identity.plugin_id)? != identity.publisher_fingerprint {
                return Err(Status::failed_precondition("plugin identity changed"));
            }
            let current = trusted_managed_package(
                &registry,
                &state.trust_dir(),
                &identity.plugin_id,
                &identity.publisher_fingerprint,
            )?;
            if current != (installed, key) {
                return Err(Status::failed_precondition(
                    "plugin package changed; reload and review",
                ));
            }

            let transition = profiles.lock_transition();
            if !profiles.current().is_some_and(|current| {
                current.epoch() == profile.epoch() && current.profile_id() == identity.profile_id
            }) {
                return Err(Status::failed_precondition(
                    "plugin profile changed while saving permissions",
                ));
            }
            let mut grants = state
                .open_grants(&profile)
                .map_err(|error| Status::internal(format!("plugin grants unavailable: {error}")))?;
            if grants.revision(&identity) != expected_revision {
                return Err(Status::aborted(
                    "plugin grants changed; refresh and try again",
                ));
            }
            #[cfg(test)]
            checkpoint(PermissionCommitCheckpoint::BeforeWrite);
            let grant_revision = grants
                .replace_grants(identity.clone(), &requests, &decisions)
                .map_err(|error| {
                    Status::invalid_argument(format!("plugin permission decision: {error}"))
                })?;
            profiles.notify_permissions_changed();
            let saved = PluginPermissionSnapshot {
                effective_grants: grants.effective_grants(&identity),
                plugin_id: identity.plugin_id,
                publisher_fingerprint: identity.publisher_fingerprint,
                profile_id: identity.profile_id,
                grant_revision,
                requested_permissions: requests,
            };
            drop(transition);
            drop(publisher);
            #[cfg(test)]
            checkpoint(PermissionCommitCheckpoint::AfterCommit);
            Ok(saved)
        })
        .await
        .map_err(|_| Status::internal("plugin permission worker failed"))?
    }

    async fn managed_permission_requests(
        &self,
        plugin_id: &str,
        registered_fingerprint: &str,
    ) -> Result<Vec<PermissionRequest>, Status> {
        let (installed, publisher_key) = self
            .managed_package_for_permissions(plugin_id, registered_fingerprint)
            .await?;
        self.verified_permission_requests(&installed, publisher_key)
            .await
    }

    async fn managed_package_for_permissions(
        &self,
        plugin_id: &str,
        fingerprint: &str,
    ) -> Result<(InstalledPluginVersion, [u8; 32]), Status> {
        let registry = self.registry.clone();
        let trust_dir = self.state.trust_dir();
        let plugin_id = plugin_id.to_owned();
        let fingerprint = fingerprint.to_owned();
        tokio::task::spawn_blocking(move || {
            trusted_managed_package(&registry, &trust_dir, &plugin_id, &fingerprint)
        })
        .await
        .map_err(|_| Status::internal("plugin publisher worker failed"))?
    }

    async fn verified_permission_requests(
        &self,
        installed: &InstalledPluginVersion,
        publisher_key: [u8; 32],
    ) -> Result<Vec<PermissionRequest>, Status> {
        let report = self
            .packages
            .inspect_candidate(
                &installed.package_path,
                publisher_key,
                gamenode_platform::plugin_environment::EmulationPolicy::RequireNative,
            )
            .await
            .map_err(|_| Status::failed_precondition("plugin package validation failed"))?;
        if report.manifest.id != installed.plugin_id || report.manifest.version != installed.version
        {
            return Err(Status::failed_precondition(
                "plugin manifest does not match the installed package",
            ));
        }
        super::grants::manifest_permission_requests(&report.manifest.permissions).map_err(|error| {
            Status::failed_precondition(format!(
                "plugin permission scopes are unsupported: {error}"
            ))
        })
    }

    /// Reports only whether the active signed manifest offers host-supported configuration
    /// modes and which package revision declared them. Plugin-owned preferences are never read.
    pub async fn configuration_state(
        &self,
        plugin_id: String,
    ) -> Result<PluginConfigurationState, Status> {
        let provider_ids = self.api.third_party_provider_ids_for_plugin(&plugin_id);
        let [provider_id] = provider_ids.as_slice() else {
            return Err(Status::failed_precondition(
                "plugin has no unique registered third-party provider",
            ));
        };
        let (registered_plugin_id, registered_fingerprint) = self
            .api
            .third_party_identity(provider_id)
            .ok_or_else(|| Status::failed_precondition("plugin identity is unavailable"))?;
        if registered_plugin_id != plugin_id {
            return Err(Status::failed_precondition(
                "registered plugin identity changed",
            ));
        }

        let registry = self.registry.clone();
        let lookup_id = plugin_id.clone();
        let installed = tokio::task::spawn_blocking(move || registry.active(&lookup_id))
            .await
            .map_err(|_| Status::internal("plugin package state worker failed"))?
            .map_err(|_| Status::internal("plugin package state is unavailable"))?;
        let Some(installed) = installed else {
            return Ok(PluginConfigurationState {
                plugin_id,
                available: false,
                revision: None,
            });
        };

        let package_path = installed.package_path.clone();
        let package_plugin_id = installed.plugin_id.clone();
        let trust_dir = self.state.trust_dir();
        let (publisher_key, trusted_fingerprint) = tokio::task::spawn_blocking(move || {
            let publisher_key = super::updates::package_key(&package_path)
                .map_err(|_| Status::failed_precondition("active plugin package is invalid"))?;
            let trust = super::publisher_trust::PublisherTrustStore::open(trust_dir)
                .map_err(|_| Status::internal("plugin publisher trust is unavailable"))?;
            if !trust
                .is_pinned(&package_plugin_id, &publisher_key)
                .map_err(|_| Status::internal("plugin publisher trust is invalid"))?
            {
                return Err(Status::permission_denied(
                    "active plugin publisher is not trusted",
                ));
            }
            use sha2::Digest as _;
            let fingerprint = format!("SHA256:{:x}", sha2::Sha256::digest(publisher_key));
            Ok((publisher_key, fingerprint))
        })
        .await
        .map_err(|_| Status::internal("plugin publisher trust worker failed"))??;
        if trusted_fingerprint != registered_fingerprint {
            return Err(Status::failed_precondition(
                "active plugin publisher does not match its registration",
            ));
        }
        let report = self
            .packages
            .inspect_candidate(
                &installed.package_path,
                publisher_key,
                gamenode_platform::plugin_environment::EmulationPolicy::RequireNative,
            )
            .await
            .map_err(|_| Status::failed_precondition("active plugin package is invalid"))?;
        if report.manifest.id != plugin_id || report.manifest.version != installed.version {
            return Err(Status::failed_precondition(
                "active plugin manifest does not match its package revision",
            ));
        }
        let modes = report.manifest.configuration;
        Ok(PluginConfigurationState {
            plugin_id,
            available: modes.external || modes.declarative,
            revision: Some(format!(
                "{}:external={}:declarative={}",
                installed.version, modes.external, modes.declarative
            )),
        })
    }

    /// Probes an inactive package in an Extension session before changing the active pointer.
    async fn activate_extension_update(
        &self,
        previous_version: String,
        installed: gamenode_platform::plugin_registry::InstalledPluginVersion,
        mut config: ProcessConfig,
        target: ExtensionTarget,
    ) -> Result<(), Status> {
        use super::{grants::PluginIdentity, publisher_trust};
        let profiles = self.api.profiles();
        let profile = profiles.require()?;
        let validate_target = move |target: &ExtensionTarget| {
            if !target.is_current()
                || profiles.current().is_none_or(|profile| {
                    !profile.verified()
                        || profile.epoch() != target.profile_epoch
                        || profile.profile_id() != target.context.profile_id
                })
            {
                return Err(Status::failed_precondition(
                    "selected extension target or profile changed",
                ));
            }
            Ok(())
        };
        validate_target(&target)?;
        let slot = config.provider_id.clone();
        if !self.processes.extension_runtime_available(&slot) {
            return Err(Status::unavailable("extension runtime is unavailable"));
        }
        let previous = self.processes.config(&slot)?;
        let key = config
            .integrity
            .as_ref()
            .filter(|integrity| {
                integrity.version == installed.version
                    && integrity.package_root == installed.package_path
            })
            .map(|integrity| integrity.publisher_key)
            .ok_or_else(|| Status::failed_precondition("candidate package identity changed"))?;
        let previous_root = previous
            .integrity
            .as_ref()
            .filter(|integrity| {
                integrity.version == previous_version && integrity.publisher_key == key
            })
            .map(|integrity| integrity.package_root.clone())
            .ok_or_else(|| {
                Status::failed_precondition("active extension package identity changed")
            })?;
        let identity = PluginIdentity {
            plugin_id: installed.plugin_id.clone(),
            publisher_fingerprint: publisher_trust::key_fingerprint(&key),
            profile_id: profile.profile_id().to_owned(),
        };
        let validate_package = {
            let state = self.state.clone();
            let registry = self.registry.clone();
            let api = self.api.clone();
            let slot = slot.clone();
            let installed = installed.clone();
            let identity = identity.clone();
            let previous_version = previous_version.clone();
            move || {
                let registered = api.third_party_identity(&slot);
                let trusted = publisher_trust::PublisherTrustStore::open(state.trust_dir())
                    .and_then(|trust| trust.is_pinned(&installed.plugin_id, &key))
                    .map_err(|error| Status::internal(error.to_string()))?;
                let active = registry
                    .active(&installed.plugin_id)
                    .map_err(|error| Status::internal(error.to_string()))?;
                if !trusted
                    || !api.is_extension(&slot)
                    || registered.as_ref()
                        != Some(&(
                            identity.plugin_id.clone(),
                            identity.publisher_fingerprint.clone(),
                        ))
                    || active.is_none_or(|active| {
                        active.version != previous_version
                            || active.package_path != previous_root
                            || super::updates::package_key(&active.package_path).ok() != Some(key)
                    })
                {
                    return Err(Status::failed_precondition(
                        "active extension identity changed",
                    ));
                }
                Ok(())
            }
        };
        // Fence the old process before changing grants. An old action is never restarted.
        self.processes
            .replace_config(&slot, previous.clone())
            .await?;
        let grant_revision = {
            let validate_package = validate_package.clone();
            let validate_target = validate_target.clone();
            let target = target.clone();
            let profiles = self.api.profiles();
            let state = self.state.clone();
            let profile = profile.clone();
            let identity = identity.clone();
            let blocked = config.blocked_capabilities.iter().cloned().collect();
            tokio::task::spawn_blocking(move || {
                let _publisher = lock_publisher_identity();
                let _profile = profiles.lock_transition();
                validate_package()?;
                validate_target(&target)?;
                let mut grants = state
                    .open_grants(&profile)
                    .map_err(|error| Status::internal(error.to_string()))?;
                grants
                    .revoke_permissions(&identity, &blocked)
                    .map_err(|error| Status::internal(error.to_string()))
            })
            .await
            .map_err(|_| Status::internal("extension grant worker failed"))??
        };
        let ready = self
            .processes
            .ensure_extension_ready_for_update(&slot, target.clone(), config.clone())
            .await;
        // Wait for the provisional tree and fence its session before committing.
        // The slot retains the previous configuration even if cleanup fails.
        self.processes.replace_config(&slot, previous).await?;
        ready?;
        let registry = self.registry.clone();
        let profiles = self.api.profiles();
        let state = self.state.clone();
        tokio::task::spawn_blocking(move || {
            let _publisher = lock_publisher_identity();
            let _profile = profiles.lock_transition();
            validate_package()?;
            let grants = state.open_grants(&profile)
                .map_err(|error| Status::internal(error.to_string()))?;
            if grants.revision(&identity) != grant_revision {
                return Err(Status::failed_precondition("extension grants changed during readiness"));
            }
            validate_target(&target)?;
            let outcome = registry.set_active(&installed.plugin_id, &installed.version)
                .map_err(|error| Status::internal(error.to_string()))
                // Also catch library changes while the registry write was in progress.
                .and_then(|()| validate_target(&target));
            if let Err(error) = outcome {
                // set_active can report a sync error after replacing the pointer.
                if registry.active(&installed.plugin_id).ok().flatten().as_ref() == Some(&installed) {
                    registry.set_active(&installed.plugin_id, &previous_version)
                        .map_err(|restore| Status::internal(format!(
                            "extension activation failed ({error}); previous version could not be restored ({restore})"
                        )))?;
                }
                return Err(error);
            }
            Ok(())
        })
        .await
        .map_err(|_| Status::internal("extension activation worker failed"))??;
        // Ordinary extension starts share the mutation lock and can only see this config
        // once the guarded registry commit above has completed.
        config.blocked_capabilities.clear();
        self.processes.replace_config(&slot, config).await
    }

    /// Atomically activates an already-installed candidate only after its supervised process
    /// completes the bounded Ready handshake with the account's current grants.
    pub(crate) async fn activate_update(
        &self,
        plugin_id: String,
        previous_version: String,
        installed: gamenode_platform::plugin_registry::InstalledPluginVersion,
        config: ProcessConfig,
        extension_target: Option<ExtensionTarget>,
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
        if self.api.is_extension(&config.provider_id) {
            let target = extension_target.ok_or_else(|| {
                Status::failed_precondition("select a game for the extension readiness check")
            })?;
            return self
                .activate_extension_update(previous_version, installed, config, target)
                .await;
        }
        if extension_target.is_some() {
            return Err(Status::invalid_argument(
                "provider updates take no extension target",
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
            let _publisher = lock_publisher_identity();
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
                self.processes.terminate_provider(&config.provider_id).await;
            }
            let registry = self.registry.clone();
            let transaction_to_commit = transaction.clone();
            let commit = tokio::task::spawn_blocking(move || {
                let _publisher = lock_publisher_identity();
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
                    tokio::task::spawn_blocking(move || {
                        let _publisher = lock_publisher_identity();
                        registry.commit_activation(retry)
                    })
                    .await
                    .map_err(|_| Status::internal("plugin activation worker failed"))?
                    .map_err(|error| Status::internal(error.to_string()))?;
                }
            }
            Ok(())
        }
        .await;

        if let Err(error) = result {
            self.processes.terminate_provider(&config.provider_id).await;
            let registry = self.registry.clone();
            let transaction_to_rollback = transaction;
            let rollback = tokio::task::spawn_blocking(move || {
                let _publisher = lock_publisher_identity();
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
            if enabled
                && !self.api.is_extension(&config.provider_id)
                && self.api.profiles().is_current(profile_epoch)
            {
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
        for id in process_ids.iter().filter(|id| self.api.is_extension(id)) {
            if enabled {
                if !self.processes.extension_runtime_available(id) {
                    return Err(Status::unavailable("extension runtime is unavailable"));
                }
            } else {
                self.processes.terminate_provider(id).await;
            }
        }
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
                if !self.api.is_extension(&provider_id) {
                    self.processes.terminate_provider(&provider_id).await;
                    self.api.disconnect(&provider_id).await;
                }
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
            if self.api.is_extension(provider_id) {
                continue;
            }
            if let Err(error) = self.processes.ensure_started(provider_id).await {
                for provider_id in &process_ids {
                    self.processes.terminate_provider(provider_id).await;
                    self.api.disconnect(provider_id).await;
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
        let profile = self.api.profiles().require()?;
        let registry = self.registry.clone();
        let lookup_id = plugin_id.clone();
        let installed = tokio::task::spawn_blocking(move || registry.active(&lookup_id))
            .await
            .map_err(|_| Status::internal("plugin package worker failed"))?
            .map_err(|_| Status::internal("plugin package state is unavailable"))?;
        if installed.is_none() {
            return Err(Status::not_found("third-party plugin is not installed"));
        }
        let provider_ids = self.api.third_party_provider_ids_for_plugin(&plugin_id);
        for id in provider_ids.iter().filter(|id| self.api.is_extension(id)) {
            self.processes.terminate_provider(id).await;
        }

        let state = self.state.clone();
        let account = profile.clone();
        let plugin = plugin_id.clone();
        tokio::task::spawn_blocking(move || {
            let mut activation = state
                .open_activation(&account)
                .map_err(|error| Status::internal(format!("plugin activation state: {error}")))?;
            activation
                .set_enabled(&plugin, false)
                .map_err(|error| Status::internal(format!("plugin activation state: {error}")))
        })
        .await
        .map_err(|_| Status::internal("plugin activation worker failed"))??;
        for provider_id in provider_ids.iter().filter(|id| !self.api.is_extension(id)) {
            self.processes.terminate_provider(provider_id).await;
            self.api.disconnect(provider_id).await;
        }
        if !provider_ids.is_empty() {
            self.api.unregister_third_party_plugin(&plugin_id).await?;
        }
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
                    if !lifecycle.api.is_extension(&provider_id) {
                        lifecycle.processes.terminate_provider(&provider_id).await;
                    }
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

fn sharing_identity_conflict() -> Status {
    Status::aborted(
        "plugin identity changed while saving shared-data permissions; reload and review",
    )
}

fn validate_sharing_profile(
    profiles: &PluginProfiles,
    observation: &SharingIdentityObservation,
) -> Result<(), Status> {
    if profiles.current().is_some_and(|current| {
        current.epoch() == observation.profile_epoch
            && current.profile_id() == observation.identity.profile_id
    }) {
        Ok(())
    } else {
        Err(sharing_identity_conflict())
    }
}

// Call while holding the publisher identity lock so registration stays stable between lookups.
fn registered_publisher(api: &ProviderApi, plugin_id: &str) -> Result<String, Status> {
    let provider_ids = api.third_party_provider_ids_for_plugin(plugin_id);
    let [provider_id] = provider_ids.as_slice() else {
        return Err(Status::failed_precondition(
            "plugin has no unique registered third-party provider",
        ));
    };
    api.third_party_identity(provider_id)
        .filter(|(registered_id, _)| registered_id == plugin_id)
        .map(|(_, fingerprint)| fingerprint)
        .ok_or_else(|| Status::failed_precondition("plugin identity is unavailable"))
}

fn trusted_managed_package(
    registry: &PluginPackageRegistry,
    trust_dir: &Path,
    plugin_id: &str,
    registered_fingerprint: &str,
) -> Result<(InstalledPluginVersion, [u8; 32]), Status> {
    let installed = registry
        .active(plugin_id)
        .map_err(|_| Status::internal("plugin package state is unavailable"))?
        .ok_or_else(|| Status::failed_precondition("plugin package is not installed"))?;
    let key = super::updates::package_key(&installed.package_path)
        .map_err(|_| Status::failed_precondition("plugin package manifest is invalid"))?;
    let trust = super::publisher_trust::PublisherTrustStore::open(trust_dir)
        .map_err(|_| Status::internal("plugin publisher trust is unavailable"))?;
    let status = trust
        .inspect(plugin_id, &key)
        .map_err(|_| Status::internal("plugin publisher trust is invalid"))?;
    let super::publisher_trust::KeyStatus::Trusted { fingerprint } = status else {
        return Err(Status::permission_denied("plugin publisher is not trusted"));
    };
    if fingerprint != registered_fingerprint {
        return Err(Status::failed_precondition(
            "plugin publisher does not match its registration",
        ));
    }
    Ok((installed, key))
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl HostedProviders {
    pub async fn bundled(
        api: ProviderApi,
        intents: Arc<super::intents::Intents>,
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
        let mut managed_plugins = vec![];
        let state = PluginStateLayout::managed(&paths);
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
                declared_developer: descriptor.name.clone(),
                package_digest: Vec::new(),
                capabilities: descriptor.capabilities.clone(),
                shared_catalog: descriptor.shared_catalog,
                playtime: Default::default(),
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
            let inventory_entry = super::PluginInfo {
                provider_id: report.manifest.provider_id.clone().unwrap_or_default(),
                plugin_id: installed.plugin_id.clone(),
                plugin_kind: match report.manifest.kind {
                    super::package::PluginKind::Provider => "provider".to_owned(),
                    super::package::PluginKind::Extension => "extension".to_owned(),
                },
                name: report.manifest.name.clone(),
                source: "community".to_owned(),
                status: "installed".to_owned(),
                // Manifest declarations are not grants. The permission viewer will expose
                // effective grants separately; this legacy field is only for bundled plugins.
                capabilities: vec![],
                session: "stopped".to_owned(),
                native_access: report.manifest.native_access.clone(),
                publisher_fingerprint: Some(publisher_fingerprint.clone()),
                version: installed.version.clone(),
                enabled: None,
                runtime_available: None,
                compatibility_details: Some(report.compatibility_review()),
            };
            let services = if report.manifest.kind == super::package::PluginKind::Provider {
                Some(Arc::new(ProviderServices::new_external(
                    ProviderContext {
                        provider_id: provider_id.clone(),
                        ..Default::default()
                    },
                    paths.clone(),
                    library.clone(),
                    Arc::new(UnavailableExternalAccounts),
                    login.clone(),
                    None,
                )?))
            } else {
                None
            };
            let host_services: Arc<dyn super::provider::HostServices> = match &services {
                Some(services) => services.clone(),
                None => Arc::new(super::provider::ExtensionServices),
            };
            let registration = Registration {
                provider_id: provider_id.clone(),
                plugin_id: installed.plugin_id.clone(),
                kind: match report.manifest.kind {
                    super::package::PluginKind::Provider => {
                        super::provider::RegistrationKind::Provider
                    }
                    super::package::PluginKind::Extension => {
                        super::provider::RegistrationKind::Extension
                    }
                },
                origin: super::provider::RegistrationOrigin::ThirdParty {
                    publisher_fingerprint: publisher_fingerprint.clone(),
                },
                declared_developer: report.manifest.name.clone(),
                package_digest: Vec::new(),
                playtime: report.manifest.playtime,
                capabilities: config.manifest_capabilities.clone(),
                shared_catalog: false,
                services: host_services,
            };
            let registration_api = api.clone();
            let registered =
                tokio::task::spawn_blocking(move || registration_api.register(registration))
                    .await
                    .map_err(|_| Status::internal("plugin registration worker failed"))?;
            if let Err(error) = registered {
                tracing::warn!(plugin_id = %installed.plugin_id, %error, "managed plugin registration was rejected");
                continue;
            }
            if let Err(error) = api.set_extension_schemas(
                &provider_id,
                &installed.plugin_id,
                &publisher_fingerprint,
                report.extension_schemas.clone(),
            ) {
                tracing::warn!(plugin_id = %installed.plugin_id, %error, "managed plugin schema catalog was rejected");
                continue;
            }
            managed_plugins.push(inventory_entry);
            configs.push(config);
            if let Some(services) = services {
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
            intents,
            endpoint_available,
            entries,
            managed_plugins,
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
        // A deliberate stop is a session state, not a fault: the plugin stays installed so Settings
        // can offer Start. Crashes and initialization failures still surface as `failed`.
        if self.processes.is_stopped(id) {
            return "installed";
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
            // Stopping is intentional. Calls refused with `plugin stopped` must not turn the
            // plugin into a failed one, otherwise Settings loses the Start action.
            Err(error) if self.processes.is_stopped(id) || error.message() == PLUGIN_STOPPED => {}
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
    fn managed_plugin_inventory(&self) -> Vec<super::PluginInfo> {
        self.managed_plugins
            .iter()
            .cloned()
            .map(|mut plugin| {
                let slot = if plugin.plugin_kind == "extension" {
                    plugin.status = if self
                        .lifecycle
                        .extension_runtime_available(&plugin.plugin_id)
                    {
                        "installed"
                    } else {
                        "unavailable"
                    }
                    .into();
                    super::provider::extension_registration_id(&plugin.plugin_id)
                } else {
                    plugin.status = self.availability(&plugin.provider_id).to_owned();
                    plugin.provider_id.clone()
                };
                plugin.session = if self.api.connected(&slot) {
                    "active"
                } else {
                    "stopped"
                }
                .into();
                plugin
            })
            .collect()
    }

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
        !self.processes.is_stopped(id) && matches!(self.availability(id), "installed" | "failed")
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
    fn set_runtime_session_handler(&self, handler: Arc<dyn RuntimeSessionHandler>) {
        self.api.set_runtime_session_handler(handler);
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

    async fn start_extension(
        &self,
        plugin_id: &str,
        target: ExtensionTarget,
        operation: super::intents::Operation,
    ) -> Result<(), Status> {
        let permission = match operation {
            super::intents::Operation::Launch => cap::GAME_LAUNCH,
            super::intents::Operation::Install => cap::GAMES_INSTALL,
            _ => return Err(Status::invalid_argument("unsupported extension action")),
        };
        let _mutation = self.lifecycle.mutation.lock().await;
        if !self.endpoint_available {
            return Err(Status::unavailable("extension runtime is unavailable"));
        }
        let provider_id = super::provider::extension_registration_id(plugin_id);
        if !self.api.is_extension(&provider_id) {
            return Err(Status::not_found("extension is not installed and trusted"));
        }
        let profile = self.api.profiles().require()?;
        if !profile.verified()
            || profile.epoch() != target.profile_epoch
            || profile.profile_id() != target.context.profile_id
            || !target.is_current()
        {
            return Err(Status::failed_precondition(
                "selected extension target is unavailable",
            ));
        }
        let (registered_plugin, publisher_fingerprint) = self
            .api
            .third_party_identity(&provider_id)
            .ok_or_else(|| Status::not_found("extension is not available"))?;
        if registered_plugin != plugin_id {
            return Err(Status::permission_denied("extension identity mismatch"));
        }
        let binding = super::intents::Binding {
            plugin_id: registered_plugin,
            publisher_fingerprint,
            profile_epoch: profile.epoch(),
            game_id: target.context.game_id.clone(),
            installation_id: target.context.installation_id.clone(),
            operation,
            target_revision: target
                .context
                .revision
                .parse()
                .map_err(|_| Status::invalid_argument("invalid target revision"))?,
            recipe_id: String::new(),
            recipe_revision: 0,
            plan_fingerprint: String::new(),
        };
        let intent = ExtensionIntent {
            id: self.intents.issue(binding)?,
            store: self.intents.clone(),
        };
        self.processes
            .ensure_extension_started(
                &provider_id,
                ExtensionLaunch {
                    target,
                    required_permission: permission,
                },
                intent,
            )
            .await
    }

    async fn available_extensions(
        &self,
        operation: super::intents::Operation,
    ) -> Result<Vec<super::PluginInfo>, Status> {
        let permission = match operation {
            super::intents::Operation::Launch => cap::GAME_LAUNCH,
            super::intents::Operation::Install => cap::GAMES_INSTALL,
            _ => return Err(Status::invalid_argument("unsupported extension action")),
        };
        if !self.endpoint_available {
            return Ok(Vec::new());
        }
        let profile = self.api.profiles().require()?;
        if !profile.verified() {
            return Ok(Vec::new());
        }
        let mut eligible = Vec::new();
        for plugin in self
            .managed_plugins
            .iter()
            .filter(|plugin| plugin.plugin_kind == "extension")
        {
            let slot = super::provider::extension_registration_id(&plugin.plugin_id);
            if !self
                .lifecycle
                .extension_runtime_available(&plugin.plugin_id)
            {
                continue;
            }
            let config = self.processes.config(&slot)?;
            if config
                .blocked_capabilities
                .iter()
                .any(|blocked| blocked == permission)
                || !config
                    .manifest_capabilities
                    .iter()
                    .any(|capability| capability == permission)
            {
                continue;
            }
            let Some((plugin_id, publisher_fingerprint)) = self.api.third_party_identity(&slot)
            else {
                continue;
            };
            let identity = super::grants::PluginIdentity {
                plugin_id: plugin_id.clone(),
                publisher_fingerprint,
                profile_id: profile.profile_id().to_owned(),
            };
            let state = self.lifecycle.state.clone();
            let account = profile.clone();
            let plugin_id_for_check = plugin_id.clone();
            let permission = permission.to_owned();
            let mut required = config.required_capabilities;
            required.push(permission);
            let can_start = tokio::task::spawn_blocking(move || -> Result<bool, Status> {
                let activation = state.open_activation(&account).map_err(|error| {
                    Status::internal(format!("plugin activation state: {error}"))
                })?;
                if !activation.is_enabled(&plugin_id_for_check) {
                    return Ok(false);
                }
                let grants = state
                    .open_grants(&account)
                    .map_err(|error| Status::internal(format!("plugin grants: {error}")))?;
                let granted = grants.effective_grants(&identity);
                Ok(required.iter().all(|permission| {
                    !config.blocked_capabilities.contains(permission)
                        && granted.iter().any(|grant| {
                            grant.permission_id == *permission
                                && grant.state == super::grants::PermissionState::Granted
                        })
                }))
            })
            .await
            .map_err(|_| Status::internal("plugin eligibility worker failed"))??;
            if can_start {
                let mut plugin = plugin.clone();
                plugin.runtime_available = Some(true);
                eligible.push(plugin);
            }
        }
        if !self.api.profiles().is_current(profile.epoch()) {
            return Ok(Vec::new());
        }
        Ok(eligible)
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
    async fn stop(&self, id: &str) -> Result<(), Status> {
        if !self.entries.contains_key(id) {
            return Err(Status::not_found("provider is not registered"));
        }
        // A pending sign-in owns a native window; ask the plugin to close it before the process
        // ends. The call is bounded so a wedged plugin cannot delay the stop indefinitely.
        if self.processes.process_id(id).await.is_some() {
            let _ = tokio::time::timeout(
                Duration::from_secs(5),
                self.processes.execute(id, ProviderOperation::CancelSignIn),
            )
            .await;
        }
        self.processes.stop_provider(id).await;
        self.notify();
        Ok(())
    }
    async fn start(&self, id: &str) -> Result<(), Status> {
        if !self.entries.contains_key(id) {
            return Err(Status::not_found("provider is not registered"));
        }
        self.processes.start_provider(id).await?;
        // The fresh process runs its initialization path, which restores the stored account.
        self.initialized(id).await?;
        // A successful fresh session clears a previous crash/failure marker.
        lock(&self.failures).remove(id);
        self.notify();
        Ok(())
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

    #[derive(Default)]
    struct PermissionSnapshotServices;

    #[tonic::async_trait]
    impl super::super::provider::HostServices for PermissionSnapshotServices {
        async fn context(
            &self,
            _lease: super::super::provider::Lease,
        ) -> Result<gamenode_plugin_protocol::proto::ProviderContext, Status> {
            Err(Status::unimplemented("fixture"))
        }

        async fn account(
            &self,
            _lease: super::super::provider::Lease,
            _action: gamenode_plugin_protocol::proto::account_request::Action,
        ) -> Result<gamenode_plugin_protocol::proto::AccountReply, Status> {
            Err(Status::unimplemented("fixture"))
        }

        async fn secret(
            &self,
            _lease: super::super::provider::Lease,
            _request: gamenode_plugin_protocol::proto::SecretRequest,
        ) -> Result<gamenode_plugin_protocol::proto::SecretReply, Status> {
            Err(Status::unimplemented("fixture"))
        }

        async fn login(
            &self,
            _lease: super::super::provider::Lease,
            _request: gamenode_plugin_protocol::proto::LoginRequest,
        ) -> Result<gamenode_plugin_protocol::proto::LoginReply, Status> {
            Err(Status::unimplemented("fixture"))
        }

        async fn actions(
            &self,
            _lease: super::super::provider::Lease,
            _actions: gamenode_plugin_protocol::proto::ActionDeclaration,
        ) -> Result<gamenode_plugin_protocol::proto::ActionReply, Status> {
            Err(Status::unimplemented("fixture"))
        }
    }

    fn permission_lifecycle(
        root: &std::path::Path,
        api: ProviderApi,
        state: PluginStateLayout,
    ) -> ThirdPartyPluginLifecycle {
        let registry =
            gamenode_platform::plugin_registry::PluginPackageRegistry::at(root.join("registry"))
                .unwrap();
        let processes = ProviderProcesses::new_for_lifecycle(
            api.clone(),
            root.join("provider.sock"),
            vec![],
            state.clone(),
        )
        .unwrap();
        ThirdPartyPluginLifecycle::new(
            api,
            processes,
            state,
            registry.clone(),
            ManagedPackageLifecycle::new(registry),
        )
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    mod sharing_policy_races {
        use super::*;
        use crate::plugin_host::{
            managed_packages::tests::write_signed_package,
            profile::profile_id_for_account,
            provider::{RegistrationKind, RegistrationOrigin},
            publisher_trust::{PublisherTrustStore, key_fingerprint},
            updates::{PluginUpdateService, tests::add_tree},
        };
        use gamenode_platform::{paths::DataPaths, plugin_environment::EmulationPolicy};
        use gamenode_plugin_protocol::proto::CommonDataCategory;
        use ring::signature::{Ed25519KeyPair, KeyPair};

        const PLUGIN_ID: &str = "org.example.lifecycle";
        const WAIT: Duration = Duration::from_secs(10);

        struct Fixture {
            root: tempfile::TempDir,
            lifecycle: Arc<ThirdPartyPluginLifecycle>,
            policies: Arc<SharedSharingPolicies>,
            original: RecipientSharingSnapshot,
        }

        impl Fixture {
            async fn new() -> (Self, SharingIdentityObservation) {
                let root = tempfile::tempdir().unwrap();
                let api = ProviderApi::default();
                api.profiles().update(Some("account-a"), true);
                let paths = DataPaths::at(root.path().join("app"));
                let state = PluginStateLayout::new(paths.plugin_data_dir().join("state"));
                let lifecycle = Arc::new(permission_lifecycle(root.path(), api.clone(), state));
                let key = install_package(&lifecycle, root.path(), "1.0.0", 42, true).await;
                let fingerprint = key_fingerprint(&key);
                let trust_dir = lifecycle.state.trust_dir();
                std::fs::create_dir_all(&trust_dir).unwrap();
                PublisherTrustStore::open(trust_dir)
                    .unwrap()
                    .confirm_first_key(PLUGIN_ID, &key, &fingerprint)
                    .unwrap();
                api.register(Registration {
                    provider_id: "lifecycle-fixture".into(),
                    plugin_id: PLUGIN_ID.into(),
                    kind: RegistrationKind::Provider,
                    origin: RegistrationOrigin::ThirdParty {
                        publisher_fingerprint: fingerprint,
                    },
                    declared_developer: "Fixture developer".into(),
                    package_digest: vec![],
                    capabilities: vec![cap::CONTEXT.into()],
                    shared_catalog: false,
                    playtime: Default::default(),
                    services: Arc::new(PermissionSnapshotServices),
                })
                .unwrap();
                let policies =
                    Arc::new(SharedSharingPolicies::open(root.path().join("policies")).unwrap());
                let observation = lifecycle
                    .observe_sharing_identity(
                        PLUGIN_ID.into(),
                        &profile_id_for_account("account-a"),
                    )
                    .await
                    .unwrap();
                policies
                    .replace_recipient_policies(
                        &observation.identity,
                        0,
                        grant(CommonDataCategory::GamePaths),
                        vec![],
                    )
                    .unwrap();
                let original = policies.recipient_snapshot(&observation.identity).unwrap();
                let fixture = Self {
                    root,
                    lifecycle,
                    policies,
                    original,
                };
                (fixture, observation)
            }

            async fn save(
                &self,
                observation: SharingIdentityObservation,
            ) -> Result<RecipientSharingSnapshot, Status> {
                self.lifecycle
                    .replace_sharing_policy(
                        observation,
                        self.policies.clone(),
                        self.original.revision,
                        grant(CommonDataCategory::RuntimeCatalog),
                        vec![],
                        vec![],
                    )
                    .await
            }

            fn on_disk(&self, identity: &PluginIdentity) -> RecipientSharingSnapshot {
                SharedSharingPolicies::open(self.root.path().join("policies"))
                    .unwrap()
                    .recipient_snapshot(identity)
                    .unwrap()
            }

            async fn assert_conflict(&self, observation: SharingIdentityObservation) {
                let old_identity = observation.identity.clone();
                let error = self.save(observation).await.unwrap_err();
                assert_eq!(error.code(), tonic::Code::Aborted);
                assert_eq!(self.on_disk(&old_identity), self.original);
            }
        }

        fn grant(category: CommonDataCategory) -> ActivationReadGrant {
            ActivationReadGrant::new([category], []).unwrap()
        }

        async fn install_package(
            lifecycle: &ThirdPartyPluginLifecycle,
            root: &Path,
            version: &str,
            seed: u8,
            activate: bool,
        ) -> [u8; 32] {
            let key = Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap();
            let public_key: [u8; 32] = key.public_key().as_ref().try_into().unwrap();
            let package = root.join(version);
            write_signed_package(&package, version, "linux", &key);
            lifecycle
                .packages
                .install_candidate(
                    &package,
                    public_key,
                    EmulationPolicy::RequireNative,
                    activate,
                )
                .await
                .unwrap();
            public_key
        }

        #[tokio::test]
        async fn profile_switch_after_review_rejects_the_durable_save() {
            let (fixture, observation) = Fixture::new().await;
            fixture
                .lifecycle
                .api
                .profiles()
                .update(Some("account-b"), true);
            fixture.assert_conflict(observation).await;
        }

        #[tokio::test]
        async fn profile_switch_back_after_review_still_rejects_the_durable_save() {
            let (fixture, observation) = Fixture::new().await;
            let profiles = fixture.lifecycle.api.profiles();
            profiles.update(Some("account-b"), true);
            profiles.update(Some("account-a"), true);
            assert_eq!(
                profiles.current().unwrap().profile_id(),
                observation.identity.profile_id
            );
            fixture.assert_conflict(observation).await;
        }

        #[tokio::test]
        async fn active_package_with_new_publisher_rejects_the_durable_save() {
            let (fixture, observation) = Fixture::new().await;
            let key =
                install_package(&fixture.lifecycle, fixture.root.path(), "2.0.0", 43, false).await;
            fixture
                .lifecycle
                .packages
                .activate_version(
                    PLUGIN_ID.into(),
                    "2.0.0".into(),
                    key,
                    EmulationPolicy::RequireNative,
                )
                .await
                .unwrap();
            assert_ne!(
                key_fingerprint(&key),
                observation.identity.publisher_fingerprint
            );
            // The existing registration is still for the original key.
            assert_eq!(
                registered_publisher(&fixture.lifecycle.api, PLUGIN_ID).unwrap(),
                observation.identity.publisher_fingerprint,
            );
            fixture.assert_conflict(observation).await;
        }

        #[tokio::test]
        async fn import_retrust_rejects_the_durable_save_before_install() {
            let (fixture, observation) = Fixture::new().await;
            let paths = DataPaths::at(fixture.root.path().join("app"));
            let updates = PluginUpdateService::open(&paths).unwrap();
            updates.set_lifecycle(fixture.lifecycle.clone());
            let key = Ed25519KeyPair::from_seed_unchecked(&[43; 32]).unwrap();
            let package = fixture.root.path().join("import");
            write_signed_package(&package, "2.0.0", "linux", &key);
            let archive = fixture.root.path().join("import.gnplugin");
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
            add_tree(&mut zip, &package, &package);
            zip.finish().unwrap();

            let review = updates.import_review(archive).await.unwrap();
            assert_eq!(review.trust, "changed");
            let trusted = updates
                .import_confirm_publisher(&review.token, &review.publisher_fingerprint)
                .await
                .unwrap();
            assert_eq!(trusted.trust, "trusted");
            assert_eq!(
                fixture
                    .lifecycle
                    .registry
                    .active(PLUGIN_ID)
                    .unwrap()
                    .unwrap()
                    .version,
                "1.0.0",
            );
            fixture.assert_conflict(observation).await;
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn committed_save_returns_saved_revision_after_profile_switch() {
            use std::sync::mpsc;

            let (fixture, observation) = Fixture::new().await;
            let fixture = Arc::new(fixture);
            let old_identity = observation.identity.clone();
            let profiles = fixture.lifecycle.api.profiles();
            let (checkpoint_tx, checkpoint_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let release_rx = Mutex::new(release_rx);
            *lock(&fixture.lifecycle.sharing_commit_hook) = Some(Arc::new(move |checkpoint| {
                checkpoint_tx.send(checkpoint).unwrap();
                lock(&release_rx).recv_timeout(WAIT).unwrap();
            }));
            let saver = fixture.clone();
            let save = tokio::spawn(async move { saver.save(observation).await });
            assert!(matches!(
                checkpoint_rx.recv_timeout(WAIT).unwrap(),
                SharingCommitCheckpoint::BeforeWrite,
            ));
            assert!(profiles.transition_is_locked());
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let switch = tokio::task::spawn_blocking(move || {
                started_tx.send(()).unwrap();
                profiles.update(Some("account-b"), true);
            });
            started_rx.await.unwrap();
            release_tx.send(()).unwrap();

            assert!(matches!(
                checkpoint_rx.recv_timeout(WAIT).unwrap(),
                SharingCommitCheckpoint::AfterCommit,
            ));
            switch.await.unwrap();
            let durable = fixture.on_disk(&old_identity);
            assert_eq!(durable.revision, fixture.original.revision + 1);
            assert_eq!(
                durable.categories,
                [CommonDataCategory::RuntimeCatalog].into()
            );
            let new_identity = PluginIdentity {
                profile_id: profile_id_for_account("account-b"),
                ..old_identity
            };
            assert_eq!(
                fixture.on_disk(&new_identity),
                RecipientSharingSnapshot::default()
            );
            release_tx.send(()).unwrap();
            assert_eq!(save.await.unwrap().unwrap(), durable);
        }
    }

    #[cfg(target_os = "linux")]
    mod network_origin_review {
        use super::*;
        use crate::plugin_host::{
            PluginApi,
            grants::{EffectiveGrant, GrantScope, PermissionDecision, PermissionState},
            profile::ProfileContext,
            provider::{RegistrationKind, RegistrationOrigin},
            publisher_trust::{PublisherTrustStore, key_fingerprint},
            updates::tests::signed_package_with_permissions,
        };
        use gamenode_platform::plugin_environment::EmulationPolicy;
        use ring::signature::{Ed25519KeyPair, KeyPair};

        const PLUGIN_ID: &str = "org.example.update";
        const WAIT: Duration = Duration::from_secs(10);

        struct Fixture {
            _temp: tempfile::TempDir,
            lifecycle: Arc<ThirdPartyPluginLifecycle>,
            identity: PluginIdentity,
            profile: ProfileContext,
            activations: tokio::sync::Mutex<()>,
        }

        impl Fixture {
            async fn new() -> Self {
                let temp = tempfile::tempdir().unwrap();
                let root = temp.path().join("package");
                let paths = gamenode_platform::paths::DataPaths::at(temp.path().join("app"));
                paths.ensure_data_dir().unwrap();
                let state = PluginStateLayout::prepare_managed(&paths).unwrap();
                let key = Ed25519KeyPair::from_seed_unchecked(&[52; 32]).unwrap();
                let public_key: [u8; 32] = key.public_key().as_ref().try_into().unwrap();
                let fingerprint = key_fingerprint(&public_key);
                signed_package_with_permissions(
                    &root,
                    "1.0.0",
                    &key,
                    vec![serde_json::json!({
                        "id": cap::NETWORK_FETCH, "required": false,
                        "scope": "https://API.example.com:443, https://cdn.example.com:8443",
                        "reason": "Fetch plugin data"
                    })],
                );
                PublisherTrustStore::open(state.trust_dir())
                    .unwrap()
                    .confirm_first_key(PLUGIN_ID, &public_key, &fingerprint)
                    .unwrap();
                let api = PluginApi::default();
                api.set_state_layout(state.clone());
                api.providers.profiles().update(Some("account-one"), true);
                api.providers
                    .register(Registration {
                        declared_developer: "Fixture developer".into(),
                        package_digest: Vec::new(),
                        provider_id: "update-fixture".into(),
                        plugin_id: PLUGIN_ID.into(),
                        kind: RegistrationKind::Provider,
                        origin: RegistrationOrigin::ThirdParty {
                            publisher_fingerprint: fingerprint.clone(),
                        },
                        capabilities: vec![cap::CONTEXT.into(), cap::NETWORK_FETCH.into()],
                        shared_catalog: false,
                        playtime: Default::default(),
                        services: Arc::new(PermissionSnapshotServices),
                    })
                    .unwrap();
                let profile = api.providers.profiles().require().unwrap();
                let lifecycle = Arc::new(permission_lifecycle(
                    temp.path(),
                    api.providers.clone(),
                    state,
                ));
                lifecycle
                    .packages
                    .install_candidate(&root, public_key, EmulationPolicy::RequireNative, true)
                    .await
                    .unwrap();
                let identity = PluginIdentity {
                    plugin_id: PLUGIN_ID.into(),
                    publisher_fingerprint: fingerprint,
                    profile_id: profile.profile_id().to_owned(),
                };
                Self {
                    _temp: temp,
                    lifecycle,
                    identity,
                    profile,
                    activations: tokio::sync::Mutex::new(()),
                }
            }

            fn on_disk(
                &self,
                profile: &ProfileContext,
                identity: &PluginIdentity,
            ) -> (u64, Vec<EffectiveGrant>) {
                let store = self.lifecycle.state.open_grants(profile).unwrap();
                (store.revision(identity), store.effective_grants(identity))
            }

            async fn save(
                &self,
                revision: u64,
                granted: bool,
            ) -> Result<PluginPermissionSnapshot, Status> {
                self.lifecycle
                    .replace_permissions(
                        &self.activations,
                        PLUGIN_ID.into(),
                        self.identity.profile_id.clone(),
                        self.identity.publisher_fingerprint.clone(),
                        revision,
                        vec![decision(granted)],
                    )
                    .await
            }
        }

        fn decision(granted: bool) -> PermissionDecision {
            PermissionDecision {
                permission_id: cap::NETWORK_FETCH.into(),
                scope: GrantScope::Declared {
                    value: "https://api.example.com".into(),
                },
                granted,
            }
        }

        #[tokio::test]
        async fn saves_only_explicit_choices_for_the_reviewed_publisher() {
            let fixture = Fixture::new().await;
            let snapshot = fixture
                .lifecycle
                .permission_snapshot(PLUGIN_ID.into())
                .await
                .unwrap();
            assert!(snapshot.effective_grants.is_empty());
            let [request] = snapshot.requested_permissions.as_slice() else {
                panic!("expected network permission");
            };
            assert_eq!(request.allowed_scopes.len(), 2);
            assert!(request.allowed_scopes.contains(&decision(true).scope));
            let error = fixture
                .lifecycle
                .replace_permissions(
                    &fixture.activations,
                    PLUGIN_ID.into(),
                    snapshot.profile_id.clone(),
                    "old publisher".into(),
                    snapshot.grant_revision,
                    vec![decision(true)],
                )
                .await
                .unwrap_err();
            assert_eq!(error.code(), tonic::Code::FailedPrecondition);
            let saved = fixture.save(snapshot.grant_revision, true).await.unwrap();
            let granted: Vec<_> = saved
                .effective_grants
                .iter()
                .filter(|grant| grant.state == PermissionState::Granted)
                .collect();
            assert_eq!(granted.len(), 1);
            assert_eq!(granted[0].scope, decision(true).scope);
            let stale = fixture
                .save(snapshot.grant_revision, false)
                .await
                .unwrap_err();
            assert_eq!(stale.code(), tonic::Code::Aborted);
            assert_eq!(
                fixture.on_disk(&fixture.profile, &fixture.identity),
                (saved.grant_revision, saved.effective_grants)
            );
            let revoked = fixture.save(saved.grant_revision, false).await.unwrap();
            assert!(
                revoked
                    .effective_grants
                    .iter()
                    .all(|grant| grant.state == PermissionState::Denied)
            );
        }

        async fn profile_transition_wins(switch_back: bool) {
            let fixture = Fixture::new().await;
            let original = fixture.save(0, true).await.unwrap();
            let profiles = fixture.lifecycle.api.profiles();
            let changes = profiles.subscribe_permissions();
            *lock(&fixture.lifecycle.permission_commit_hook) = Some(Arc::new(move |checkpoint| {
                assert_eq!(checkpoint, PermissionCommitCheckpoint::Prepared);
                profiles.update(Some("account-two"), true);
                if switch_back {
                    profiles.update(Some("account-one"), true);
                }
            }));
            let error = fixture
                .save(original.grant_revision, false)
                .await
                .unwrap_err();
            assert_eq!(error.code(), tonic::Code::FailedPrecondition);
            assert_eq!(
                fixture.on_disk(&fixture.profile, &fixture.identity),
                (original.grant_revision, original.effective_grants)
            );
            assert!(!changes.has_changed().unwrap());
        }

        #[tokio::test]
        async fn profile_switch_before_commit_preserves_saved_origins() {
            profile_transition_wins(false).await;
        }

        #[tokio::test]
        async fn profile_switch_back_before_commit_preserves_saved_origins() {
            profile_transition_wins(true).await;
        }

        #[tokio::test]
        async fn profile_round_trip_while_queued_for_activation_rejects_save() {
            let fixture = Fixture::new().await;
            let original = fixture.save(0, true).await.unwrap();
            let activation = fixture.activations.lock().await;
            let save = fixture.save(original.grant_revision, false);
            tokio::pin!(save);
            tokio::select! {
                biased;
                _ = &mut save => panic!("save must wait for update activation"),
                _ = std::future::ready(()) => {}
            }
            let profiles = fixture.lifecycle.api.profiles();
            profiles.update(Some("account-two"), true);
            profiles.update(Some("account-one"), true);
            drop(activation);

            assert_eq!(
                save.await.unwrap_err().code(),
                tonic::Code::FailedPrecondition
            );
            assert_eq!(
                fixture.on_disk(&fixture.profile, &fixture.identity),
                (original.grant_revision, original.effective_grants)
            );
        }

        #[tokio::test]
        async fn retrust_before_commit_preserves_saved_origins() {
            let fixture = Fixture::new().await;
            let original = fixture.save(0, true).await.unwrap();
            let trust_dir = fixture.lifecycle.state.trust_dir();
            *lock(&fixture.lifecycle.permission_commit_hook) = Some(Arc::new(move |checkpoint| {
                assert_eq!(checkpoint, PermissionCommitCheckpoint::Prepared);
                let _publisher = lock_publisher_identity();
                let key = Ed25519KeyPair::from_seed_unchecked(&[53; 32]).unwrap();
                let public_key: [u8; 32] = key.public_key().as_ref().try_into().unwrap();
                PublisherTrustStore::open(&trust_dir)
                    .unwrap()
                    .retrust_key(PLUGIN_ID, &public_key, &key_fingerprint(&public_key))
                    .unwrap();
            }));
            let error = fixture
                .save(original.grant_revision, false)
                .await
                .unwrap_err();
            assert_eq!(error.code(), tonic::Code::PermissionDenied);
            assert_eq!(
                fixture.on_disk(&fixture.profile, &fixture.identity),
                (original.grant_revision, original.effective_grants)
            );
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn committed_save_returns_original_identity_after_switch() {
            use std::sync::mpsc;

            let fixture = Arc::new(Fixture::new().await);
            let original = fixture.save(0, true).await.unwrap();
            let profiles = fixture.lifecycle.api.profiles();
            let changes = profiles.subscribe_permissions();
            let (checkpoint_tx, checkpoint_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let release_rx = Mutex::new(release_rx);
            *lock(&fixture.lifecycle.permission_commit_hook) = Some(Arc::new(move |checkpoint| {
                if checkpoint != PermissionCommitCheckpoint::Prepared {
                    checkpoint_tx.send(checkpoint).unwrap();
                    lock(&release_rx).recv_timeout(WAIT).unwrap();
                }
            }));
            let saver = fixture.clone();
            let revision = original.grant_revision;
            let save = tokio::spawn(async move { saver.save(revision, false).await });
            assert_eq!(
                checkpoint_rx.recv_timeout(WAIT).unwrap(),
                PermissionCommitCheckpoint::BeforeWrite
            );
            assert!(profiles.transition_is_locked());
            assert!(!changes.has_changed().unwrap());
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let switch = tokio::task::spawn_blocking(move || {
                started_tx.send(()).unwrap();
                profiles.update(Some("account-two"), true);
                profiles.require().unwrap()
            });
            started_rx.await.unwrap();
            release_tx.send(()).unwrap();
            assert_eq!(
                checkpoint_rx.recv_timeout(WAIT).unwrap(),
                PermissionCommitCheckpoint::AfterCommit
            );
            let next_profile = switch.await.unwrap();
            assert!(changes.has_changed().unwrap());

            let durable = fixture.on_disk(&fixture.profile, &fixture.identity);
            assert_eq!(durable.0, revision + 1);
            assert_eq!(durable.1.len(), 2);
            assert!(
                durable
                    .1
                    .iter()
                    .all(|grant| grant.state == PermissionState::Denied)
            );
            let next_identity = PluginIdentity {
                profile_id: next_profile.profile_id().to_owned(),
                ..fixture.identity.clone()
            };
            assert_eq!(fixture.on_disk(&next_profile, &next_identity), (0, vec![]));

            release_tx.send(()).unwrap();
            let saved = save.await.unwrap().unwrap();
            assert_eq!(saved.plugin_id, fixture.identity.plugin_id);
            assert_eq!(
                saved.publisher_fingerprint,
                fixture.identity.publisher_fingerprint
            );
            assert_eq!(saved.profile_id, fixture.identity.profile_id);
            assert_eq!((saved.grant_revision, saved.effective_grants), durable);
        }
    }

    #[tokio::test]
    async fn permission_snapshot_requires_the_installed_signed_package_for_scope_choices() {
        use super::super::provider::{RegistrationKind, RegistrationOrigin};

        let temp = tempfile::tempdir().unwrap();
        let api = ProviderApi::default();
        api.profiles().update(Some("account-one"), true);
        let plugin_id = "org.example.community";
        let fingerprint = "cd".repeat(32);
        api.register(Registration {
            declared_developer: "Fixture developer".into(),
            package_digest: Vec::new(),
            provider_id: "community".into(),
            plugin_id: plugin_id.into(),
            kind: RegistrationKind::Provider,
            origin: RegistrationOrigin::ThirdParty {
                publisher_fingerprint: fingerprint.clone(),
            },
            capabilities: vec![cap::CONTEXT.into()],
            shared_catalog: false,
            playtime: Default::default(),
            services: Arc::new(PermissionSnapshotServices),
        })
        .unwrap();
        let state = PluginStateLayout::new(temp.path().join("state"));
        let lifecycle = permission_lifecycle(temp.path(), api, state);

        let error = lifecycle
            .permission_snapshot(plugin_id.into())
            .await
            .unwrap_err();
        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
        assert_eq!(error.message(), "plugin package is not installed");
    }

    #[tokio::test]
    async fn permission_snapshot_requires_active_profile_and_unique_third_party_provider() {
        use super::super::provider::{RegistrationKind, RegistrationOrigin};

        let temp = tempfile::tempdir().unwrap();
        let api = ProviderApi::default();
        let state = PluginStateLayout::new(temp.path().join("state"));
        let lifecycle = permission_lifecycle(temp.path(), api.clone(), state.clone());
        assert_eq!(
            lifecycle
                .permission_snapshot("org.example.community".into())
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unavailable
        );

        api.profiles().update(Some("account-one"), true);
        api.register(Registration {
            declared_developer: "Fixture developer".into(),
            package_digest: Vec::new(),
            provider_id: "official".into(),
            plugin_id: "org.example.official".into(),
            kind: RegistrationKind::Provider,
            origin: RegistrationOrigin::Official,
            capabilities: vec![cap::CONTEXT.into()],
            shared_catalog: false,
            playtime: Default::default(),
            services: Arc::new(PermissionSnapshotServices),
        })
        .unwrap();
        assert_eq!(
            lifecycle
                .permission_snapshot("org.example.official".into())
                .await
                .unwrap_err()
                .code(),
            tonic::Code::FailedPrecondition
        );
    }

    #[tokio::test]
    async fn removal_requires_an_active_profile_but_can_remove_a_not_yet_registered_package() {
        let temp = tempfile::tempdir().unwrap();
        let api = ProviderApi::default();
        let state = PluginStateLayout::new(temp.path().join("state"));
        let package = temp.path().join("package");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(package.join("payload"), b"not executed").unwrap();
        let registry = gamenode_platform::plugin_registry::PluginPackageRegistry::at(
            temp.path().join("registry"),
        )
        .unwrap();
        registry
            .install_verified("org.example.imported", "1.0.0", &package, true)
            .unwrap();
        let lifecycle = permission_lifecycle(temp.path(), api.clone(), state);

        assert_eq!(
            lifecycle
                .remove("org.example.imported".into())
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unavailable
        );
        assert!(registry.active("org.example.imported").unwrap().is_some());

        api.profiles().update(Some("account-one"), true);
        lifecycle
            .remove("org.example.imported".into())
            .await
            .unwrap();
        assert!(registry.active("org.example.imported").unwrap().is_none());
    }

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
            playtime: Default::default(),
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
            extension_schemas: vec![],
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

#[cfg(all(test, target_os = "linux"))]
mod tests;
