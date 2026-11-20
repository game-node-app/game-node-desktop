//! Binds third-party plugin profiles to the signed-in GameNode account.
//!
//! GameNode has no signed-out or local usage mode, so there is no anonymous plugin profile either.
//! The engine reports who is signed in: the verified account, or the cached last account while a
//! restored session awaits verification. Without an account the host issues no profile context,
//! starts no third-party process and answers profile-scoped services with UNAVAILABLE. Sign-out,
//! a rejected session and a verified account that differs from the cached one are profile changes:
//! they advance the epoch, which fences every context issued for the previous profile.
//!
//! Persistence is layered. Installed packages and publisher trust belong to the OS user;
//! activation, grants, configuration and private data belong to the account profile.

use crate::plugin_host::grants::{
    GrantError, PermissionGrantStore, PluginIdentity, atomic_replace, ensure_private_dir,
    ensure_private_file, sync_dir,
};
use crate::plugin_host::namespace::ExternalNamespaceResolver;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::watch;
use tonic::Status;

const ACTIVATION_FILE: &str = "plugin-activation.json";
const MAX_ACTIVATION_BYTES: u64 = 1024 * 1024;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

/// The plugin profile of one GameNode account, valid while its epoch is current.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileContext {
    profile_id: String,
    epoch: u64,
    verified: bool,
}

impl ProfileContext {
    /// Opaque profile id derived from the account id. It is safe as a directory name and does
    /// not reveal the account id in paths or namespace keys.
    pub fn profile_id(&self) -> &str {
        &self.profile_id
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// False while a restored session runs on the cached account before verification.
    pub fn verified(&self) -> bool {
        self.verified
    }
}

/// Derives the plugin profile id of a GameNode account.
pub fn profile_id_for_account(account_id: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"gamenode.plugin-profile.v1\0");
    hash.update(account_id.as_bytes());
    hash.finalize()[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// What a profile update did to previously issued contexts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileTransition {
    /// Same account (possibly now verified): issued contexts stay valid.
    Unchanged,
    /// The profile was replaced or removed: contexts of the previous profile are fenced.
    Fenced,
}

#[derive(Debug, Default)]
struct State {
    account_id: Option<String>,
    current: Option<ProfileContext>,
    epoch: u64,
}

/// Host-owned source of the active plugin profile.
pub struct PluginProfiles {
    transition: Mutex<()>,
    state: Mutex<State>,
    layout: Mutex<Option<PluginStateLayout>>,
    changes: watch::Sender<Option<ProfileContext>>,
    permission_changes: watch::Sender<()>,
}

impl Default for PluginProfiles {
    fn default() -> Self {
        Self {
            transition: Mutex::new(()),
            state: Mutex::new(State {
                epoch: 1,
                ..State::default()
            }),
            layout: Mutex::new(None),
            changes: watch::Sender::new(None),
            permission_changes: watch::Sender::new(()),
        }
    }
}

impl std::fmt::Debug for PluginProfiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginProfiles")
            .field("current", &self.current())
            .finish()
    }
}

pub(crate) struct LockedProfileTransition<'a> {
    profiles: &'a PluginProfiles,
    _guard: std::sync::MutexGuard<'a, ()>,
}

impl LockedProfileTransition<'_> {
    /// Applies who is signed in. `account_id` is the verified account, or the cached last account
    /// while a restored session is unverified; `None` after sign-out or a rejected session.
    pub fn update(&self, account_id: Option<&str>, verified: bool) -> ProfileTransition {
        let mut state = lock(&self.profiles.state);
        let account_id = account_id.filter(|id| !id.trim().is_empty());
        if state.account_id.as_deref() == account_id {
            if let Some(current) = state.current.as_mut()
                && current.verified != verified
            {
                // Verifying the cached account keeps the same profile and its contexts.
                current.verified = verified;
                let current = state.current.clone();
                self.profiles.changes.send_replace(current);
            }
            return ProfileTransition::Unchanged;
        }
        state.epoch += 1;
        state.account_id = account_id.map(str::to_owned);
        state.current = account_id.map(|id| ProfileContext {
            profile_id: profile_id_for_account(id),
            epoch: state.epoch,
            verified,
        });
        let current = state.current.clone();
        drop(state);
        self.profiles.changes.send_replace(current);
        ProfileTransition::Fenced
    }
}

impl PluginProfiles {
    /// Supplies the host-owned root for profile-scoped plugin grants and activation.
    pub fn set_state_layout(&self, layout: PluginStateLayout) {
        *lock(&self.layout) = Some(layout);
    }

    /// Serializes account changes with durable plugin permission and shared-data commits.
    /// Profile reads use a separate state lock so they can still run while the commit is syncing to disk.
    pub(crate) fn lock_transition(&self) -> LockedProfileTransition<'_> {
        LockedProfileTransition {
            profiles: self,
            _guard: lock(&self.transition),
        }
    }

    #[cfg(test)]
    pub(crate) fn transition_is_locked(&self) -> bool {
        matches!(
            self.transition.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        )
    }

    pub fn update(&self, account_id: Option<&str>, verified: bool) -> ProfileTransition {
        self.lock_transition().update(account_id, verified)
    }

    pub fn current(&self) -> Option<ProfileContext> {
        lock(&self.state).current.clone()
    }

    /// The current profile epoch. It advances on every profile change, including sign-out, so a
    /// context captured earlier can never match a later profile.
    pub fn epoch(&self) -> u64 {
        lock(&self.state).epoch
    }

    pub fn is_current(&self, epoch: u64) -> bool {
        lock(&self.state)
            .current
            .as_ref()
            .is_some_and(|current| current.epoch == epoch)
    }

    /// The active profile, or UNAVAILABLE when no account session exists. This is distinct from
    /// a permission denial: the plugin did nothing wrong, there is just no profile to serve.
    pub fn require(&self) -> Result<ProfileContext, Status> {
        self.current().ok_or_else(no_profile)
    }

    /// Third-party processes may run only while an account profile is active.
    pub fn may_start_third_party(&self) -> bool {
        lock(&self.state).current.is_some()
    }

    /// Namespace resolver for an authenticated plugin in the active profile.
    pub fn resolver(
        &self,
        plugin_id: &str,
        publisher_fingerprint: &str,
        provider_id: Option<&str>,
    ) -> Result<ExternalNamespaceResolver, Status> {
        let profile = self.require()?;
        ExternalNamespaceResolver::new(
            PluginIdentity {
                plugin_id: plugin_id.into(),
                publisher_fingerprint: publisher_fingerprint.into(),
                profile_id: profile.profile_id.clone(),
            },
            provider_id.map(str::to_owned),
            profile.epoch,
        )
        .map_err(|_| Status::invalid_argument("invalid plugin identity"))
    }

    /// Authenticated plugin identities known in the active profile's grant store.
    pub fn identities(&self) -> Result<Vec<PluginIdentity>, Status> {
        let profile = self.require()?;
        let layout = lock(&self.layout)
            .clone()
            .ok_or_else(|| Status::unavailable("plugin state layout is unavailable"))?;
        let grants = layout
            .open_grants(&profile)
            .map_err(|_| Status::unavailable("plugin grants are unavailable"))?;
        Ok(grants.identities())
    }

    /// Whether an authenticated plugin identity is enabled in the active account profile.
    pub fn is_enabled(&self, plugin_id: &str) -> Result<bool, Status> {
        let profile = self.require()?;
        let layout = lock(&self.layout)
            .clone()
            .ok_or_else(|| Status::unavailable("plugin state layout is unavailable"))?;
        let activation = layout
            .open_activation(&profile)
            .map_err(|_| Status::unavailable("plugin activation state is unavailable"))?;
        Ok(activation.is_enabled(plugin_id))
    }

    /// Whether this profile allows one signed plugin to contribute measured playtime.
    /// The preference is independent of discovery, launch grants and plugin activation.
    pub fn is_playtime_accounting_enabled(&self, plugin_id: &str) -> Result<bool, Status> {
        let profile = self.require()?;
        let layout = lock(&self.layout)
            .clone()
            .ok_or_else(|| Status::unavailable("plugin state layout is unavailable"))?;
        let activation = layout
            .open_activation(&profile)
            .map_err(|_| Status::unavailable("plugin accounting preference is unavailable"))?;
        Ok(activation.is_playtime_accounting_enabled(plugin_id))
    }

    /// Persist the accounting choice before it is applied to subsequent runtime reports.
    pub fn set_playtime_accounting_enabled(
        &self,
        plugin_id: &str,
        enabled: bool,
    ) -> Result<(), Status> {
        let profile = self.require()?;
        let layout = lock(&self.layout)
            .clone()
            .ok_or_else(|| Status::unavailable("plugin state layout is unavailable"))?;
        let mut activation = layout
            .open_activation(&profile)
            .map_err(|_| Status::unavailable("plugin accounting preference is unavailable"))?;
        activation
            .set_playtime_accounting_enabled(plugin_id, enabled)
            .map_err(|_| Status::unavailable("plugin accounting preference could not be saved"))
    }

    /// Current host-consented permissions for a package identity in the active profile.
    /// Used only for host actions explicitly initiated by the user, never to construct a plugin
    /// RPC context or elevate a plugin request.
    pub fn granted_permissions(
        &self,
        plugin_id: &str,
        publisher_fingerprint: &str,
    ) -> Result<(u64, std::collections::BTreeSet<String>), Status> {
        use crate::plugin_host::grants::PermissionState;

        let profile = self.require()?;
        let layout = lock(&self.layout)
            .clone()
            .ok_or_else(|| Status::unavailable("plugin state layout is unavailable"))?;
        let grants = layout
            .open_grants(&profile)
            .map_err(|_| Status::unavailable("plugin grants are unavailable"))?;
        let identity = crate::plugin_host::grants::PluginIdentity {
            plugin_id: plugin_id.to_owned(),
            publisher_fingerprint: publisher_fingerprint.to_owned(),
            profile_id: profile.profile_id,
        };
        let revision = grants.revision(&identity);
        let permissions = grants
            .effective_grants(&identity)
            .into_iter()
            .filter(|grant| grant.state == PermissionState::Granted)
            .map(|grant| grant.permission_id)
            .collect();
        Ok((revision, permissions))
    }

    /// Notified with the new profile (or `None`) after every change.
    pub fn subscribe(&self) -> watch::Receiver<Option<ProfileContext>> {
        self.changes.subscribe()
    }

    pub(crate) fn subscribe_permissions(&self) -> watch::Receiver<()> {
        self.permission_changes.subscribe()
    }

    /// Wake in-flight work after a permission decision has been persisted.
    pub(crate) fn notify_permissions_changed(&self) {
        self.permission_changes.send_replace(());
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) fn no_profile() -> Status {
    Status::unavailable("no GameNode account profile is active")
}

/// Where third-party plugin state lives for one OS user.
///
/// ```text
/// <root>/packages/                     installed packages (OS user)
/// <root>/trust/                        publisher-key trust (OS user)
/// <root>/accounts/<profile_id>/        activation, grants, configuration, data (account)
/// ```
#[derive(Clone, Debug)]
pub struct PluginStateLayout {
    root: PathBuf,
}

impl PluginStateLayout {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Refuse the development-only plugin state layouts instead of silently ignoring them or
    /// importing user trust/grants without review. This check is read-only and must run before
    /// opening any current plugin stores.
    pub(crate) fn refuse_legacy_layouts(
        paths: &gamenode_platform::paths::DataPaths,
    ) -> io::Result<()> {
        for legacy in [
            paths.private_dir().join("publisher-keys.json"),
            paths.data_dir().join("plugin-state"),
        ] {
            match fs::symlink_metadata(&legacy) {
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "unsupported legacy third-party plugin state at {}; it was left untouched",
                            legacy.display()
                        ),
                    ));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    /// The desktop runtime's existing state directory for managed plugins.
    pub fn managed(paths: &gamenode_platform::paths::DataPaths) -> Self {
        Self::new(paths.plugin_data_dir().join("state"))
    }

    pub(super) fn prepare_managed(paths: &gamenode_platform::paths::DataPaths) -> io::Result<Self> {
        Self::refuse_legacy_layouts(paths)?;
        let layout = Self::managed(paths);
        ensure_private_dir(&paths.plugin_data_dir())?;
        ensure_private_dir(&layout.root)?;
        Ok(layout)
    }

    /// Installed packages, shared by every account of this OS user.
    pub fn packages_dir(&self) -> PathBuf {
        self.root.join("packages")
    }

    /// Publisher-key trust, shared by every account of this OS user.
    pub fn trust_dir(&self) -> PathBuf {
        self.root.join("trust")
    }

    /// Private state of one account profile. Never shared with another account.
    pub fn account_dir(&self, profile: &ProfileContext) -> io::Result<PathBuf> {
        ensure_private_dir(&self.root)?;
        let accounts = self.root.join("accounts");
        ensure_private_dir(&accounts)?;
        let dir = accounts.join(&profile.profile_id);
        ensure_private_dir(&dir)?;
        Ok(dir)
    }

    /// The permission grants of one account profile.
    pub fn open_grants(
        &self,
        profile: &ProfileContext,
    ) -> Result<PermissionGrantStore, GrantError> {
        PermissionGrantStore::open(self.account_dir(profile)?)
    }

    /// The activation (enabled) state of one account profile.
    pub fn open_activation(
        &self,
        profile: &ProfileContext,
    ) -> Result<AccountActivationStore, GrantError> {
        AccountActivationStore::open(self.account_dir(profile)?)
    }

    /// Removes a plugin's activation and grants from every account without touching its private
    /// configuration, storage, or secrets. Existing account directories are never followed when
    /// they are symlinks.
    pub fn remove_plugin_account_state(&self, plugin_id: &str) -> Result<(), GrantError> {
        if plugin_id.trim().is_empty()
            || plugin_id.len() > 256
            || plugin_id.chars().any(char::is_control)
        {
            return Err(GrantError::InvalidIdentity);
        }
        for account_dir in self.account_dirs()? {
            let mut activation = AccountActivationStore::open(account_dir.clone())?;
            if activation.is_enabled(plugin_id) {
                activation.set_enabled(plugin_id, false)?;
            }
            let mut grants = PermissionGrantStore::open(&account_dir)?;
            grants.remove_plugin(plugin_id)?;
        }
        Ok(())
    }

    /// Invalidates changed network declarations even in profiles that were inactive at update.
    pub(super) fn revoke_network_for_package(
        &self,
        plugin_id: &str,
        publisher_fingerprint: &str,
    ) -> Result<(), GrantError> {
        let network =
            BTreeSet::from([gamenode_plugin_protocol::capability::NETWORK_FETCH.to_owned()]);
        for account_dir in self.account_dirs()? {
            let mut grants = PermissionGrantStore::open(&account_dir)?;
            for identity in grants.identities().into_iter().filter(|identity| {
                identity.plugin_id == plugin_id
                    && identity.publisher_fingerprint == publisher_fingerprint
            }) {
                grants.revoke_permissions(&identity, &network)?;
            }
        }
        Ok(())
    }

    fn account_dirs(&self) -> Result<Vec<PathBuf>, GrantError> {
        match fs::symlink_metadata(&self.root) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => return Err(GrantError::InvalidStore("unsafe plugin state root".into())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        }
        let accounts = self.root.join("accounts");
        match fs::symlink_metadata(&accounts) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => return Err(GrantError::InvalidStore("unsafe accounts directory".into())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        }
        let entries = match fs::read_dir(&accounts) {
            Ok(entries) => entries,
            Err(error) => return Err(error.into()),
        };
        let mut directories = Vec::new();
        for entry in entries {
            let entry = entry?;
            let account_dir = entry.path();
            let metadata = fs::symlink_metadata(&account_dir)?;
            if metadata.file_type().is_symlink() {
                return Err(GrantError::InvalidStore("unsafe account directory".into()));
            }
            if metadata.is_dir() {
                directories.push(account_dir);
            }
        }
        Ok(directories)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivationFile {
    schema: u32,
    enabled: BTreeSet<String>,
    #[serde(default)]
    accounting_disabled: BTreeSet<String>,
}

/// Which installed plugins one account has enabled. Absent plugins are disabled.
#[derive(Debug)]
pub struct AccountActivationStore {
    path: PathBuf,
    state: ActivationFile,
}

impl AccountActivationStore {
    fn open(account_dir: PathBuf) -> Result<Self, GrantError> {
        let path = account_dir.join(ACTIVATION_FILE);
        let state = match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_file() && !meta.file_type().is_symlink() => {
                if meta.len() > MAX_ACTIVATION_BYTES {
                    return Err(GrantError::InvalidStore("file exceeds size limit".into()));
                }
                ensure_private_file(&path)?;
                let mut bytes = Vec::with_capacity(meta.len() as usize);
                File::open(&path)?
                    .take(MAX_ACTIVATION_BYTES + 1)
                    .read_to_end(&mut bytes)?;
                let state: ActivationFile = serde_json::from_slice(&bytes)
                    .map_err(|error| GrantError::InvalidStore(error.to_string()))?;
                if state.schema != 1 {
                    return Err(GrantError::InvalidStore("unsupported schema".into()));
                }
                state
            }
            Ok(_) => return Err(GrantError::InvalidStore("unsafe activation file".into())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => ActivationFile {
                schema: 1,
                ..ActivationFile::default()
            },
            Err(error) => return Err(error.into()),
        };
        Ok(Self { path, state })
    }

    pub fn is_enabled(&self, plugin_id: &str) -> bool {
        self.state.enabled.contains(plugin_id)
    }

    pub fn enabled(&self) -> impl Iterator<Item = &str> {
        self.state.enabled.iter().map(String::as_str)
    }

    pub fn is_playtime_accounting_enabled(&self, plugin_id: &str) -> bool {
        !self.state.accounting_disabled.contains(plugin_id)
    }

    pub fn set_playtime_accounting_enabled(
        &mut self,
        plugin_id: &str,
        enabled: bool,
    ) -> Result<(), GrantError> {
        if plugin_id.trim().is_empty()
            || plugin_id.len() > 256
            || plugin_id.chars().any(char::is_control)
        {
            return Err(GrantError::InvalidIdentity);
        }
        let mut next = self.state.clone();
        if enabled {
            next.accounting_disabled.remove(plugin_id);
        } else {
            next.accounting_disabled.insert(plugin_id.to_owned());
        }
        persist(&self.path, &next)?;
        self.state = next;
        Ok(())
    }

    /// Persists the change before it takes effect; a failed write leaves the state unchanged.
    pub fn set_enabled(&mut self, plugin_id: &str, enabled: bool) -> Result<(), GrantError> {
        if plugin_id.trim().is_empty()
            || plugin_id.len() > 256
            || plugin_id.chars().any(char::is_control)
        {
            return Err(GrantError::InvalidIdentity);
        }
        let mut next = self.state.clone();
        if enabled {
            next.enabled.insert(plugin_id.to_owned());
        } else {
            next.enabled.remove(plugin_id);
        }
        persist(&self.path, &next)?;
        self.state = next;
        Ok(())
    }

    pub fn remove_plugin(&mut self, plugin_id: &str) -> Result<(), GrantError> {
        if plugin_id.trim().is_empty()
            || plugin_id.len() > 256
            || plugin_id.chars().any(char::is_control)
        {
            return Err(GrantError::InvalidIdentity);
        }
        let mut next = self.state.clone();
        if next.enabled.remove(plugin_id) {
            persist(&self.path, &next)?;
            self.state = next;
        }
        Ok(())
    }
}

fn persist(path: &Path, state: &ActivationFile) -> Result<(), GrantError> {
    let parent = path
        .parent()
        .ok_or_else(|| GrantError::InvalidStore("store has no parent".into()))?;
    ensure_private_dir(parent)?;
    let bytes =
        serde_json::to_vec(state).map_err(|error| GrantError::InvalidStore(error.to_string()))?;
    let temp = parent.join(format!(
        ".plugin-activation-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    let result = (|| -> io::Result<()> {
        file.write_all(&bytes)?;
        file.sync_all()?;
        atomic_replace(&temp, path)?;
        sync_dir(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(Into::into)
}

#[cfg(test)]
mod tests;
