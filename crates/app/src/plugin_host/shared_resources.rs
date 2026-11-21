//! Shared emulator save resources: declarations, review scope and revision-bound consent.
//!
//! A save profile may declare that several library entries use one emulator backing folder (a
//! memory card, for example). The plugin proposes an alias and an approved folder handle; the host
//! resolves the canonical backing folder and the complete affected installation set. The host
//! refuses duplicate aliases, duplicate backing folders and nested or partially overlapping
//! resources, because separate lease keys would otherwise let one write bypass another.
//!
//! Consent lives beside the plugin's durable data, never in plugin RPC. It is bound to the exact
//! resolved declaration and the exact host-resolved affected set. Changing the backing folder,
//! runtime binding or affected set makes the stored decision stale; a use then asks for a fresh
//! review instead of widening itself.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use gamenode_platform::plugin_storage::{FileApproval, SaveProfileRow, operation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tonic::Status;

use super::files::FileAccess;
use super::library::{LibraryFeed, Scope};
use super::resource_leases::ResourceLeaseKey;
use super::saves;
use super::storage::{Namespace, PluginStorage};
use gamenode_platform::plugin_storage::SharedResourceConsentRow;

/// The most shared resources one profile may declare.
pub const MAX_SHARED_RESOURCES: usize = 8;
/// The longest resource alias, in bytes.
pub const MAX_RESOURCE_ID_BYTES: usize = 128;

/// One library entry in a shared-resource scope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AffectedEntry {
    pub entry_key: String,
    pub installation_id: String,
    pub game_id: String,
    pub name: String,
}

/// A host-resolved declaration stored on a save profile. `backing_path` is host-only: it never
/// reaches a plugin reply or a foreign profile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedResource {
    pub resource_id: String,
    pub approval_id: String,
    pub path: Vec<String>,
    pub display_name: String,
    pub backing_path: String,
    /// The approved runtime whose executable identity the host inspects for external use.
    pub runtime_id: String,
}

pub fn resources_json(resources: &[SharedResource]) -> String {
    serde_json::to_string(resources).unwrap_or_else(|_| "[]".into())
}

pub fn resources_from_json(text: &str) -> Vec<SharedResource> {
    serde_json::from_str(text).unwrap_or_default()
}

fn refuse(message: impl Into<String>) -> Status {
    Status::invalid_argument(message.into())
}

fn unavailable(message: impl Into<String>) -> Status {
    Status::unavailable(message.into())
}

fn check_resource_id(text: &str) -> Result<String, Status> {
    if text.is_empty()
        || text.len() > MAX_RESOURCE_ID_BYTES
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return Err(refuse(format!(
            "resource_id is 1 to {MAX_RESOURCE_ID_BYTES} bytes of letters, digits, '.', '_' and '-'"
        )));
    }
    Ok(text.to_owned())
}

fn hash_part(digest: &mut Sha256, part: &[u8]) {
    digest.update((part.len() as u64).to_be_bytes());
    digest.update(part);
}

/// The host-canonical declaration fingerprint. It deliberately excludes display names and
/// profile revision: a rename does not invalidate consent, but a changed handle, backing folder or
/// runtime binding does.
pub fn declaration_fingerprint(resource: &SharedResource) -> String {
    let mut digest = Sha256::new();
    digest.update(b"gamenode.shared-save-resource.declaration.v1\0");
    hash_part(&mut digest, resource.resource_id.as_bytes());
    hash_part(&mut digest, resource.approval_id.as_bytes());
    for component in &resource.path {
        hash_part(&mut digest, component.as_bytes());
    }
    digest.update([0]);
    hash_part(&mut digest, resource.backing_path.as_bytes());
    hash_part(&mut digest, resource.runtime_id.as_bytes());
    hex(&digest.finalize())
}

/// The stable identity of an affected set: installation and GameNode game, never a display name.
pub fn scope_fingerprint(scope: &[AffectedEntry]) -> String {
    let mut entries: Vec<(&str, &str)> = scope
        .iter()
        .map(|entry| (entry.installation_id.as_str(), entry.game_id.as_str()))
        .collect();
    entries.sort();
    let mut digest = Sha256::new();
    digest.update(b"gamenode.shared-save-resource.scope.v1\0");
    for (installation, game) in entries {
        hash_part(&mut digest, installation.as_bytes());
        hash_part(&mut digest, game.as_bytes());
    }
    hex(&digest.finalize())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The complete current scope of a standing approval. Missing durable identities fail closed: a
/// review must name every affected game and installation.
pub fn scope_of(feed: &LibraryFeed, approval: &FileApproval) -> Result<Vec<AffectedEntry>, Status> {
    if approval.affected.is_empty() {
        return Err(Status::failed_precondition(
            "the shared resource has no resolved affected games",
        ));
    }
    let mut scope = Vec::with_capacity(approval.affected.len());
    for key in &approval.affected {
        let (_, entry) = feed.projected_entry(key).ok_or_else(|| {
            unavailable("the affected game set is not fully resolved; try again later")
        })?;
        let name = entry
            .display_name
            .clone()
            .unwrap_or_else(|| entry.name.clone());
        scope.push(AffectedEntry {
            entry_key: key.clone(),
            installation_id: entry.installation_id,
            game_id: entry.game_id.map(|id| id.to_string()).unwrap_or_default(),
            name,
        });
    }
    scope.sort_by(|a, b| a.entry_key.cmp(&b.entry_key));
    Ok(scope)
}

/// The canonical absolute folder below an approval. Symlinks are resolved once; a missing folder
/// fails closed.
pub fn backing_path(approval: &FileApproval, path: &[String]) -> Result<String, Status> {
    let root = std::fs::canonicalize(&approval.root)
        .map_err(|_| Status::failed_precondition("the approved backing folder changed"))?;
    let mut target = root.clone();
    for component in path {
        if !gamenode_platform::plugin_files::valid_component(component) {
            return Err(Status::permission_denied(
                "the shared resource path is not a plain relative path",
            ));
        }
        target.push(component);
    }
    let canonical = std::fs::canonicalize(&target)
        .map_err(|_| Status::failed_precondition("the approved backing folder is unavailable"))?;
    if !canonical.starts_with(&root) || !canonical.is_dir() {
        return Err(Status::permission_denied(
            "the shared resource is outside its approved folder",
        ));
    }
    canonical
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| Status::failed_precondition("the approved path is not valid Unicode"))
}

fn overlaps(left: &str, right: &str) -> bool {
    let (left, right) = (Path::new(left), Path::new(right));
    left.starts_with(right) || right.starts_with(left)
}

/// Resolves a profile's proposed declarations against approvals in this plugin/profile namespace.
/// Runs on the namespace worker.
pub(crate) fn resolve_declarations(
    files: &FileAccess,
    feed: &LibraryFeed,
    namespace: &mut Namespace<'_>,
    session: &str,
    profile_id: Option<&str>,
    installation_id: &str,
    runtime_id: &str,
    declarations: &[gamenode_plugin_protocol::proto::SharedResourceDeclaration],
    existing: &[SaveProfileRow],
) -> Result<Vec<SharedResource>, Status> {
    if declarations.is_empty() {
        return Ok(Vec::new());
    }
    if declarations.len() > MAX_SHARED_RESOURCES {
        return Err(refuse(format!(
            "a profile declares at most {MAX_SHARED_RESOURCES} shared resources"
        )));
    }
    if runtime_id.is_empty() {
        return Err(Status::failed_precondition(
            "a shared resource requires the profile's approved runtime binding",
        ));
    }
    let (entry_key, _) = feed
        .target(
            &Scope {
                own_provider: String::new(),
                read_all: true,
                provider_filter: None,
            },
            installation_id,
        )
        .ok_or_else(|| Status::failed_precondition("the profile installation is unavailable"))?;

    let mut resolved: Vec<SharedResource> = Vec::with_capacity(declarations.len());
    for declaration in declarations {
        let resource_id = check_resource_id(&declaration.resource_id)?;
        if declaration.approved_resource_handle.is_empty() {
            return Err(refuse(
                "a shared resource requires an approved resource handle",
            ));
        }
        let display_name = saves::check_display_name(&declaration.display_name)?;
        let opened = files
            .open_handle(session, &declaration.approved_resource_handle)
            .ok_or_else(|| Status::not_found("file handle not found"))?;
        // Re-derive the standing approval, including its complete affected set and volume.
        let opened = super::resources::files::open_approval(
            files,
            feed,
            namespace,
            &opened.0,
            opened.1,
            operation::READ,
        )?;
        if opened.approval.kind == "library" {
            return Err(Status::permission_denied(
                "a shared resource needs an approved folder, not a library root",
            ));
        }
        if !opened.approval.affected.iter().any(|key| key == &entry_key) {
            return Err(Status::permission_denied(
                "the approved folder does not affect this profile's installation",
            ));
        }
        let entry = opened
            .confined
            .stat(&opened.path)
            .map_err(super::resources::files::file_status)?;
        if !entry.directory {
            return Err(refuse("a shared resource must be an approved folder"));
        }
        let backing_path = backing_path(&opened.approval, &opened.path)?;
        let resource = SharedResource {
            resource_id,
            approval_id: opened.approval.id,
            path: opened.path,
            display_name,
            backing_path,
            runtime_id: runtime_id.to_owned(),
        };
        if resolved.iter().any(|other| {
            other.resource_id == resource.resource_id
                || other.backing_path == resource.backing_path
                || overlaps(&other.backing_path, &resource.backing_path)
        }) {
            return Err(refuse(
                "shared resources must have distinct aliases and non-overlapping backing folders",
            ));
        }
        resolved.push(resource);
    }

    for row in existing
        .iter()
        .filter(|row| Some(row.id.as_str()) != profile_id)
    {
        for other in resources_from_json(&row.shared_resources) {
            for resource in &resolved {
                if other.resource_id == resource.resource_id
                    && other.backing_path != resource.backing_path
                {
                    return Err(refuse(format!(
                        "resource_id {:?} already names another backing folder",
                        resource.resource_id
                    )));
                }
                if other.backing_path == resource.backing_path
                    && other.resource_id != resource.resource_id
                {
                    return Err(refuse(
                        "a backing folder must keep the same resource_id in every profile",
                    ));
                }
                if overlaps(&other.backing_path, &resource.backing_path) {
                    return Err(refuse(
                        "shared resources must not overlap another profile's backing folder",
                    ));
                }
            }
        }
    }
    Ok(resolved)
}

/// Why a declaration's current state needs review.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsentState {
    Pending,
    Current,
    Stale,
}

impl ConsentState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Current => "current",
            Self::Stale => "stale",
        }
    }

    pub fn proto(self) -> i32 {
        match self {
            Self::Pending => gamenode_plugin_protocol::proto::SharedResourceConsent::Pending as i32,
            Self::Current => gamenode_plugin_protocol::proto::SharedResourceConsent::Current as i32,
            Self::Stale => gamenode_plugin_protocol::proto::SharedResourceConsent::Stale as i32,
        }
    }
}

/// A declaration together with the host-resolved state needed to review or use it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedSharedResource {
    pub declaration: SharedResource,
    pub scope: Vec<AffectedEntry>,
    pub consent: ConsentState,
    pub lease_key: ResourceLeaseKey,
}

/// Re-resolves one profile's stored declarations, requires a current consent for each and returns
/// the host-owned lease keys. This is the only path that may authorize use.
pub(crate) fn prepare_for_use(
    files: &FileAccess,
    feed: &LibraryFeed,
    namespace: &mut Namespace<'_>,
    profile: &SaveProfileRow,
) -> Result<Vec<PreparedSharedResource>, Status> {
    let declarations = resources_from_json(&profile.shared_resources);
    if declarations.is_empty() {
        return Ok(Vec::new());
    }
    if profile.runtime_id.is_empty() {
        return Err(Status::failed_precondition(
            "the shared resource has no approved runtime binding",
        ));
    }
    let mut prepared = Vec::with_capacity(declarations.len());
    for declaration in declarations {
        if declaration.runtime_id != profile.runtime_id {
            return Err(Status::failed_precondition(
                "the shared resource runtime binding changed; ask the plugin to declare it again",
            ));
        }
        let opened = super::resources::files::open_approval_inspect(
            files,
            feed,
            namespace,
            &declaration.approval_id,
            declaration.path.clone(),
        )?
        .map_err(|_| Status::failed_precondition("the approved shared folder is unavailable"))?;
        let current_backing = backing_path(&opened.approval, &opened.path)?;
        if current_backing != declaration.backing_path {
            return Err(Status::failed_precondition(
                "the shared resource backing folder changed; review it again",
            ));
        }
        let scope = scope_of(feed, &opened.approval)?;
        let consent = namespace
            .shared_resource_consent(&profile.id, &declaration.resource_id)?
            .filter(|record| {
                record.declaration == declaration_fingerprint(&declaration)
                    && record.scope == scope_fingerprint(&scope)
            })
            .map(|_| ConsentState::Current)
            .unwrap_or(ConsentState::Stale);
        if consent != ConsentState::Current {
            return Err(Status::failed_precondition(format!(
                "shared resource {:?} needs the user's review",
                declaration.resource_id
            )));
        }
        let lease_key =
            ResourceLeaseKey::from_backing_identity(declaration.backing_path.as_bytes())
                .ok_or_else(|| unavailable("the shared resource identity cannot be determined"))?;
        prepared.push(PreparedSharedResource {
            declaration,
            scope,
            consent,
            lease_key,
        });
    }
    Ok(prepared)
}

/// A review item for the settings surface. It never contains a filesystem path.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSharedResourceReview {
    pub plugin_id: String,
    pub publisher_fingerprint: String,
    pub profile_id: String,
    pub profile_name: String,
    pub game_id: String,
    pub installation_id: String,
    pub runtime_id: String,
    pub resource_id: String,
    pub display_name: String,
    pub affected_games: Vec<PluginSharedResourceGame>,
    /// `pending`, `current` or `stale`.
    pub consent: String,
    /// True when the current approval or affected set can no longer be resolved.
    pub unavailable: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSharedResourceGame {
    pub installation_id: String,
    pub game_id: String,
    pub name: String,
}

/// Host-side review and consent for one authenticated plugin in the active account profile.
pub struct PluginSharedResources {
    storage: Arc<PluginStorage>,
    profiles: Arc<super::profile::PluginProfiles>,
    files: Arc<FileAccess>,
    feed: Arc<LibraryFeed>,
}

impl PluginSharedResources {
    pub fn new(
        storage: Arc<PluginStorage>,
        profiles: Arc<super::profile::PluginProfiles>,
        files: Arc<FileAccess>,
        feed: Arc<LibraryFeed>,
    ) -> Self {
        Self {
            storage,
            profiles,
            files,
            feed,
        }
    }

    fn resolver(
        &self,
        plugin_id: &str,
        publisher: &str,
    ) -> Result<super::namespace::ExternalNamespaceResolver, String> {
        self.profiles
            .resolver(plugin_id, publisher, None)
            .map_err(|error| error.message().to_owned())
    }

    /// Every declared shared resource in the plugin's profiles, with its current review state.
    pub async fn list(
        &self,
        plugin_id: &str,
        publisher: &str,
    ) -> Result<Vec<PluginSharedResourceReview>, String> {
        let resolver = self.resolver(plugin_id, publisher)?;
        let epoch = self.profiles.epoch();
        let files = self.files.clone();
        let feed = self.feed.clone();
        let owner = plugin_id.to_owned();
        let publisher_owned = publisher.to_owned();
        let mut reviews = self
            .storage
            .run_for_host(&resolver, epoch, move |namespace| {
                let mut reviews = Vec::new();
                for profile in namespace.save_profiles()? {
                    let declarations = resources_from_json(&profile.shared_resources);
                    if declarations.is_empty() {
                        continue;
                    }
                    for declaration in declarations {
                        let record = namespace
                            .shared_resource_consent(&profile.id, &declaration.resource_id)?;
                        let opened = super::resources::files::open_approval_inspect(
                            &files,
                            &feed,
                            namespace,
                            &declaration.approval_id,
                            declaration.path.clone(),
                        );
                        let (mut scope, unavailable) = match opened {
                            Ok(Ok(opened)) => match scope_of(&feed, &opened.approval) {
                                Ok(scope) => (scope, false),
                                Err(_) => (Vec::new(), true),
                            },
                            _ => (Vec::new(), true),
                        };
                        let consent = record
                            .filter(|record| {
                                record.declaration == declaration_fingerprint(&declaration)
                            })
                            .map(|record| {
                                if unavailable || record.scope != scope_fingerprint(&scope) {
                                    ConsentState::Stale
                                } else {
                                    ConsentState::Current
                                }
                            })
                            .unwrap_or(ConsentState::Pending);
                        scope.sort_by(|a, b| a.name.cmp(&b.name));
                        reviews.push(PluginSharedResourceReview {
                            plugin_id: owner.clone(),
                            publisher_fingerprint: publisher_owned.clone(),
                            profile_id: profile.id.clone(),
                            profile_name: profile.name.clone(),
                            game_id: profile.game_id.clone(),
                            installation_id: profile.installation_id.clone(),
                            runtime_id: profile.runtime_id.clone(),
                            resource_id: declaration.resource_id.clone(),
                            display_name: declaration.display_name.clone(),
                            affected_games: scope
                                .into_iter()
                                .map(|entry| PluginSharedResourceGame {
                                    installation_id: entry.installation_id,
                                    game_id: entry.game_id,
                                    name: entry.name,
                                })
                                .collect(),
                            consent: consent.as_str().to_owned(),
                            unavailable,
                        });
                    }
                }
                Ok(reviews)
            })
            .await
            .map_err(|error| error.message().to_owned())?;
        reviews
            .sort_by(|a, b| (&a.profile_id, &a.resource_id).cmp(&(&b.profile_id, &b.resource_id)));
        Ok(reviews)
    }

    /// Accepts the currently resolved declaration and scope. Any drift from the stored declaration
    /// is refused: the plugin must save the declaration again before the user can review it.
    pub async fn accept(
        &self,
        plugin_id: &str,
        publisher: &str,
        profile_id: &str,
        resource_id: &str,
    ) -> Result<(), String> {
        let resolver = self.resolver(plugin_id, publisher)?;
        let epoch = self.profiles.epoch();
        let files = self.files.clone();
        let feed = self.feed.clone();
        let profile_id = profile_id.to_owned();
        let resource_id = resource_id.to_owned();
        self.storage
            .run_for_host(&resolver, epoch, move |namespace| {
                let profile = namespace
                    .save_profile(&profile_id)?
                    .ok_or_else(|| Status::not_found("save profile not found"))?;
                if profile.runtime_id.is_empty() {
                    return Err(Status::failed_precondition(
                        "a shared resource requires the profile's approved runtime binding",
                    ));
                }
                let declaration = resources_from_json(&profile.shared_resources)
                    .into_iter()
                    .find(|resource| resource.resource_id == resource_id)
                    .ok_or_else(|| Status::not_found("shared resource declaration not found"))?;
                if declaration.runtime_id != profile.runtime_id {
                    return Err(Status::failed_precondition(
                        "the shared resource runtime binding changed",
                    ));
                }
                let opened = super::resources::files::open_approval_inspect(
                    &files,
                    &feed,
                    namespace,
                    &declaration.approval_id,
                    declaration.path.clone(),
                )?
                .map_err(|_| Status::failed_precondition("the approved shared folder is unavailable"))?;
                let backing = backing_path(&opened.approval, &opened.path)?;
                if backing != declaration.backing_path {
                    return Err(Status::failed_precondition(
                        "the shared resource backing folder changed; ask the plugin to declare it again",
                    ));
                }
                let scope = scope_of(&feed, &opened.approval)?;
                namespace.put_shared_resource_consent(&SharedResourceConsentRow {
                    profile_id: profile.id.clone(),
                    resource_id: declaration.resource_id.clone(),
                    declaration: declaration_fingerprint(&declaration),
                    scope: scope_fingerprint(&scope),
                    profile_revision: profile.revision,
                    approved_at: namespace.now(),
                })
            })
            .await
            .map_err(|error| error.message().to_owned())
    }

    /// Forgets a decision; the next use asks again.
    pub async fn revoke(
        &self,
        plugin_id: &str,
        publisher: &str,
        profile_id: &str,
        resource_id: &str,
    ) -> Result<bool, String> {
        let resolver = self.resolver(plugin_id, publisher)?;
        let epoch = self.profiles.epoch();
        let profile_id = profile_id.to_owned();
        let resource_id = resource_id.to_owned();
        self.storage
            .run_for_host(&resolver, epoch, move |namespace| {
                namespace.revoke_shared_resource_consent(&profile_id, &resource_id)
            })
            .await
            .map_err(|error| error.message().to_owned())
    }

    /// Test seam for the host-owned use path: revalidates every declaration and returns the
    /// prepared resources or the same refusal a launch or backup would receive.
    #[cfg(test)]
    pub async fn prepared(
        &self,
        plugin_id: &str,
        publisher: &str,
        profile_id: &str,
    ) -> Result<Vec<PreparedSharedResource>, String> {
        let resolver = self.resolver(plugin_id, publisher)?;
        let epoch = self.profiles.epoch();
        let files = self.files.clone();
        let feed = self.feed.clone();
        let profile_id = profile_id.to_owned();
        self.storage
            .run_for_host(&resolver, epoch, move |namespace| {
                let profile = namespace
                    .save_profile(&profile_id)?
                    .ok_or_else(|| Status::not_found("save profile not found"))?;
                prepare_for_use(&files, &feed, namespace, &profile)
            })
            .await
            .map_err(|error| error.message().to_owned())
    }
}

/// The state a save-profile reply can show without re-opening the approval. It still catches a
/// changed declaration; `prepare_for_use` additionally checks the current affected set.
pub fn reply_state(
    record: Option<&SharedResourceConsentRow>,
    declaration: &SharedResource,
) -> ConsentState {
    match record {
        None => ConsentState::Pending,
        Some(record) if record.declaration == declaration_fingerprint(declaration) => {
            ConsentState::Current
        }
        Some(_) => ConsentState::Stale,
    }
}

/// A reply state that also compares the host-resolved affected set with the stored decision.
pub fn reply_state_with_scope(
    record: Option<&SharedResourceConsentRow>,
    declaration: &SharedResource,
    scope: &[AffectedEntry],
) -> ConsentState {
    match record {
        None => ConsentState::Pending,
        Some(record)
            if record.declaration == declaration_fingerprint(declaration)
                && record.scope == scope_fingerprint(scope) =>
        {
            ConsentState::Current
        }
        Some(_) => ConsentState::Stale,
    }
}

/// The result of inspecting externally started processes for an approved runtime executable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalUse {
    Idle,
    Active,
    /// The platform cannot list processes, or the listing failed. Callers fail closed.
    Unknown,
}

/// Host-side external-process check against one approved runtime executable identity.
#[derive(Clone)]
pub struct RuntimeUseCheck {
    pub executable: String,
    pub processes: Option<Arc<dyn gamenode_core::ports::Processes>>,
}

impl RuntimeUseCheck {
    pub fn inspect(&self) -> ExternalUse {
        let Some(processes) = &self.processes else {
            return ExternalUse::Unknown;
        };
        let Ok(list) = processes.list() else {
            return ExternalUse::Unknown;
        };
        let approved = Path::new(&self.executable);
        let active = list.iter().any(|process| {
            let running = Path::new(&process.exe_path);
            #[cfg(windows)]
            {
                // Windows paths compare case-insensitively.
                running
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&self.executable)
            }
            #[cfg(not(windows))]
            {
                running == approved
            }
        });
        if active {
            ExternalUse::Active
        } else {
            ExternalUse::Idle
        }
    }
}

/// The host-owned lease key for a canonical backing folder.
pub fn lease_key_for(backing: &str) -> Result<ResourceLeaseKey, Status> {
    ResourceLeaseKey::from_backing_identity(backing.as_bytes())
        .ok_or_else(|| unavailable("the shared resource identity cannot be determined"))
}

/// Whether a set of prepared resources contains a duplicate lease key. Defensive: declaration
/// validation already rejects aliases and overlaps.
pub fn distinct_keys(
    resources: &[PreparedSharedResource],
) -> Result<Vec<ResourceLeaseKey>, Status> {
    let mut keys = BTreeSet::new();
    for resource in resources {
        if !keys.insert(resource.lease_key) {
            return Err(Status::failed_precondition(
                "two shared resources resolve to one backing folder",
            ));
        }
    }
    Ok(keys.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(installation: &str, game: &str, name: &str) -> AffectedEntry {
        AffectedEntry {
            entry_key: format!("key-{installation}"),
            installation_id: installation.into(),
            game_id: game.into(),
            name: name.into(),
        }
    }

    fn declaration(backing: &str, runtime: &str) -> SharedResource {
        SharedResource {
            resource_id: "memory-card".into(),
            approval_id: "approval-a".into(),
            path: vec![],
            display_name: "Memory card".into(),
            backing_path: backing.into(),
            runtime_id: runtime.into(),
        }
    }

    #[test]
    fn scope_identity_ignores_order_names_and_display_only_changes() {
        let one = entry("id-a", "1", "Card A");
        let two = entry("id-b", "2", "Card B");
        assert_eq!(
            scope_fingerprint(&[one.clone(), two.clone()]),
            scope_fingerprint(&[two, one])
        );
        let renamed = entry("id-a", "1", "Renamed");
        assert_eq!(
            scope_fingerprint(&[renamed]),
            scope_fingerprint(&[entry("id-a", "1", "Card A")])
        );
        assert_ne!(
            scope_fingerprint(&[entry("id-a", "1", "Card A")]),
            scope_fingerprint(&[entry("id-a", "9", "Card A")])
        );
    }

    #[test]
    fn declaration_identity_changes_only_for_authority_changes() {
        let original = declaration("/games/shared", "runtime-a");
        let renamed = SharedResource {
            display_name: "Other name".into(),
            ..original.clone()
        };
        assert_eq!(
            declaration_fingerprint(&original),
            declaration_fingerprint(&renamed)
        );
        for changed in [
            SharedResource {
                backing_path: "/games/other".into(),
                ..original.clone()
            },
            SharedResource {
                runtime_id: "runtime-b".into(),
                ..original.clone()
            },
            SharedResource {
                approval_id: "approval-b".into(),
                ..original.clone()
            },
        ] {
            assert_ne!(
                declaration_fingerprint(&original),
                declaration_fingerprint(&changed)
            );
        }
    }

    #[test]
    fn nested_folders_overlap_and_siblings_do_not() {
        assert!(overlaps("/games/shared", "/games/shared/card"));
        assert!(overlaps("/games/shared/card", "/games/shared"));
        assert!(!overlaps("/games/shared/one", "/games/shared/two"));
        assert!(!overlaps("/games/one", "/games/two"));
    }

    struct FakeProcesses(Vec<gamenode_core::ports::ProcessInfo>);

    impl gamenode_core::ports::Processes for FakeProcesses {
        fn list(
            &self,
        ) -> Result<Vec<gamenode_core::ports::ProcessInfo>, gamenode_core::ports::ProcessError>
        {
            Ok(self.0.clone())
        }

        fn watch(
            &self,
            _: &gamenode_core::ports::ProcessInfo,
            _: Box<dyn FnOnce(std::time::SystemTime) + Send>,
        ) -> Result<(), gamenode_core::ports::ProcessError> {
            Err(gamenode_core::ports::ProcessError::Unsupported)
        }
    }

    fn process(path: &str) -> gamenode_core::ports::ProcessInfo {
        gamenode_core::ports::ProcessInfo {
            pid: 7,
            created_at: std::time::UNIX_EPOCH,
            exe_path: path.into(),
            parent: None,
        }
    }

    #[test]
    fn external_use_fails_closed_without_inspection_and_matches_the_exact_identity() {
        let idle = RuntimeUseCheck {
            executable: "/emulators/dolphin".into(),
            processes: Some(Arc::new(FakeProcesses(vec![process("/emulators/other")]))),
        };
        assert_eq!(idle.inspect(), ExternalUse::Idle);
        let active = RuntimeUseCheck {
            executable: "/emulators/dolphin".into(),
            processes: Some(Arc::new(FakeProcesses(vec![process("/emulators/dolphin")]))),
        };
        assert_eq!(active.inspect(), ExternalUse::Active);
        let unknown = RuntimeUseCheck {
            executable: "/emulators/dolphin".into(),
            processes: None,
        };
        assert_eq!(unknown.inspect(), ExternalUse::Unknown);
    }
}
