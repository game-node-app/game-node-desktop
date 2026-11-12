//! Persistent third-party permission grants and domain-resource authorization.
//!
//! This module models consent separately from a live RPC session. A session receives only the
//! current effective grant revision; a grant update is persisted before it can become effective.

use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use thiserror::Error;

const STORE_FILE: &str = "plugin-permissions.json";
const MAX_STORE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_AUDIT_EVENTS: usize = 20_000;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DomainCategory {
    Runtime,
    LibraryRoot,
    SaveProfile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Operation {
    Read,
    Create,
    Update,
    Delete,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "kebab-case", deny_unknown_fields)]
pub enum GrantScope {
    /// The caller may act on records whose immutable creator is the caller.
    OwnCreator { category: DomainCategory },
    /// The caller may act on published records in this category.
    PublicCategory { category: DomainCategory },
    /// The caller may act only on the listed published record ids.
    SelectedRecords {
        category: DomainCategory,
        record_ids: BTreeSet<String>,
    },
    /// Exact host-approved scope for non-domain permissions, such as a game handle or origin.
    Declared { value: String },
    /// Reserved for a caller's own private durable namespace.
    OwnPrivateNamespace,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginIdentity {
    pub plugin_id: String,
    pub publisher_fingerprint: String,
    pub profile_id: String,
}

impl PluginIdentity {
    fn validate(&self) -> Result<(), GrantError> {
        if !valid_identity_part(&self.plugin_id, 256)
            || !valid_identity_part(&self.profile_id, 256)
            || self.publisher_fingerprint.len() > 128
            || self.publisher_fingerprint.trim().is_empty()
            || self.publisher_fingerprint.chars().any(char::is_control)
        {
            return Err(GrantError::InvalidIdentity);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionRequest {
    pub permission_id: String,
    pub required: bool,
    pub allowed_scopes: BTreeSet<GrantScope>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionDecision {
    pub permission_id: String,
    pub scope: GrantScope,
    pub granted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionState {
    Granted,
    Denied,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveGrant {
    pub permission_id: String,
    pub required: bool,
    pub scope: GrantScope,
    pub state: PermissionState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginGrantRecord {
    identity: PluginIdentity,
    revision: u64,
    grants: Vec<EffectiveGrant>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoreFile {
    schema: u32,
    plugins: BTreeMap<String, PluginGrantRecord>,
    audit: Vec<AuditEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditOperation {
    Create,
    Update,
    Delete,
    PrivateCleanup,
}

/// An audit event is produced from the authenticated caller identity, never request provenance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditEvent {
    pub actor_plugin_id: String,
    pub target_record_id: String,
    pub operation: AuditOperation,
    pub previous_revision: Option<u64>,
    pub resulting_revision: Option<u64>,
    pub grant_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainRecord {
    pub id: String,
    pub category: DomainCategory,
    /// Immutable host-owned creator attribution.
    creator_plugin_id: String,
    /// Whether the producer requested publication and the user authorized it.
    pub published: bool,
    pub revision: u64,
}

#[derive(Debug, Error)]
pub enum GrantError {
    #[error("plugin identity is invalid")]
    InvalidIdentity,
    #[error("permission request is malformed")]
    InvalidRequest,
    #[error("required permission is not supported by this host")]
    RequiredPermissionUnsupported,
    #[error("permission decision is outside the manifest request")]
    DecisionNotRequested,
    #[error("permission scope is not valid for this operation")]
    InvalidScope,
    #[error("permission is not granted for this operation and resource")]
    Denied,
    #[error("resource revision is stale")]
    StaleRevision,
    #[error("the requested namespace is not owned by the authenticated plugin")]
    ForeignPrivateNamespace,
    #[error("grant store is invalid: {0}")]
    InvalidStore(String),
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl PartialEq for GrantError {
    fn eq(&self, other: &Self) -> bool {
        use GrantError::*;
        match (self, other) {
            (InvalidIdentity, InvalidIdentity)
            | (InvalidRequest, InvalidRequest)
            | (RequiredPermissionUnsupported, RequiredPermissionUnsupported)
            | (DecisionNotRequested, DecisionNotRequested)
            | (InvalidScope, InvalidScope)
            | (Denied, Denied)
            | (StaleRevision, StaleRevision)
            | (ForeignPrivateNamespace, ForeignPrivateNamespace) => true,
            (InvalidStore(a), InvalidStore(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for GrantError {}

impl DomainRecord {
    /// Host code assigns creator attribution from authenticated publisher identity.
    pub fn new(
        id: String,
        category: DomainCategory,
        creator: &PluginIdentity,
        published: bool,
        revision: u64,
    ) -> Result<Self, GrantError> {
        creator.validate()?;
        Ok(Self {
            id,
            category,
            creator_plugin_id: creator.plugin_id.clone(),
            published,
            revision,
        })
    }

    pub fn creator_plugin_id(&self) -> &str {
        &self.creator_plugin_id
    }
}

pub struct PermissionGrantStore {
    path: PathBuf,
    state: StoreFile,
}

impl PermissionGrantStore {
    /// Opens `<private_dir>/plugin-permissions.json`; absent grants are default-deny.
    pub fn open(private_dir: impl AsRef<Path>) -> Result<Self, GrantError> {
        let directory = private_dir.as_ref();
        ensure_private_dir(directory)?;
        let path = directory.join(STORE_FILE);
        let state = match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_file() && !meta.file_type().is_symlink() => {
                if meta.len() > MAX_STORE_BYTES {
                    return Err(GrantError::InvalidStore("file exceeds size limit".into()));
                }
                ensure_private_file(&path)?;
                let mut bytes = Vec::with_capacity(meta.len() as usize);
                File::open(&path)?
                    .take(MAX_STORE_BYTES + 1)
                    .read_to_end(&mut bytes)?;
                if bytes.len() as u64 > MAX_STORE_BYTES {
                    return Err(GrantError::InvalidStore("file exceeds size limit".into()));
                }
                let state: StoreFile = serde_json::from_slice(&bytes)
                    .map_err(|error| GrantError::InvalidStore(error.to_string()))?;
                if state.schema != 1 {
                    return Err(GrantError::InvalidStore("unsupported schema".into()));
                }
                for record in state.plugins.values() {
                    record.identity.validate()?;
                }
                state
            }
            Ok(_) => return Err(GrantError::InvalidStore("unsafe grant file".into())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => StoreFile {
                schema: 1,
                ..StoreFile::default()
            },
            Err(error) => return Err(error.into()),
        };
        Ok(Self { path, state })
    }

    pub fn revision(&self, identity: &PluginIdentity) -> u64 {
        self.state
            .plugins
            .get(&identity_key(identity))
            .filter(|entry| entry.identity == *identity)
            .map_or(0, |entry| entry.revision)
    }

    pub fn effective_grants(&self, identity: &PluginIdentity) -> Vec<EffectiveGrant> {
        self.state
            .plugins
            .get(&identity_key(identity))
            .filter(|entry| entry.identity == *identity)
            .map(|entry| entry.grants.clone())
            .unwrap_or_default()
    }

    /// Required requests gate activation when denied or unsupported.
    pub fn required_permissions_granted(&self, identity: &PluginIdentity) -> bool {
        let grants = self.effective_grants(identity);
        let required: BTreeSet<_> = grants
            .iter()
            .filter(|grant| grant.required)
            .map(|grant| grant.permission_id.as_str())
            .collect();
        required.into_iter().all(|permission_id| {
            grants.iter().any(|grant| {
                grant.permission_id == permission_id && grant.state == PermissionState::Granted
            })
        })
    }

    /// Replaces a plugin's grant set atomically. Request limits come from its validated manifest;
    /// decisions are host/UI inputs. Unknown optional permissions are retained as unsupported,
    /// never granted. The old in-memory state survives any persistence failure.
    pub fn replace_grants(
        &mut self,
        identity: PluginIdentity,
        requests: &[PermissionRequest],
        decisions: &[PermissionDecision],
    ) -> Result<u64, GrantError> {
        identity.validate()?;
        let mut request_map = BTreeMap::new();
        for request in requests {
            validate_permission_request(request)?;
            if request_map
                .insert(request.permission_id.clone(), request)
                .is_some()
            {
                return Err(GrantError::InvalidRequest);
            }
            if request.required && !is_known_permission(&request.permission_id) {
                return Err(GrantError::RequiredPermissionUnsupported);
            }
        }
        let mut decisions_by_permission: BTreeMap<&str, Vec<&PermissionDecision>> = BTreeMap::new();
        let mut seen_decisions = BTreeSet::new();
        for decision in decisions {
            let request = request_map
                .get(&decision.permission_id)
                .ok_or(GrantError::DecisionNotRequested)?;
            if !request.allowed_scopes.contains(&decision.scope) {
                return Err(GrantError::DecisionNotRequested);
            }
            if !seen_decisions.insert((decision.permission_id.as_str(), &decision.scope)) {
                return Err(GrantError::InvalidRequest);
            }
            if !is_known_permission(&decision.permission_id) {
                return Err(GrantError::DecisionNotRequested);
            }
            validate_scope_for_permission(&decision.permission_id, &decision.scope)?;
            decisions_by_permission
                .entry(&decision.permission_id)
                .or_default()
                .push(decision);
        }

        let mut grants = Vec::new();
        for request in requests {
            let supported = is_known_permission(&request.permission_id);
            if !supported {
                if request.allowed_scopes.is_empty() {
                    grants.push(EffectiveGrant {
                        permission_id: request.permission_id.clone(),
                        required: request.required,
                        scope: GrantScope::Declared {
                            value: "unsupported".into(),
                        },
                        state: PermissionState::Unsupported,
                    });
                } else {
                    grants.extend(request.allowed_scopes.iter().cloned().map(|scope| {
                        EffectiveGrant {
                            permission_id: request.permission_id.clone(),
                            required: request.required,
                            scope,
                            state: PermissionState::Unsupported,
                        }
                    }));
                }
                continue;
            }
            let scopes = decisions_by_permission
                .get(request.permission_id.as_str())
                .into_iter()
                .flatten();
            let any_granted = scopes.clone().any(|decision| decision.granted);
            if request.permission_id == gamenode_plugin_protocol::capability::STORAGE_PERSIST {
                grants.push(EffectiveGrant {
                    permission_id: request.permission_id.clone(),
                    required: request.required,
                    scope: GrantScope::OwnPrivateNamespace,
                    state: if any_granted {
                        PermissionState::Granted
                    } else {
                        PermissionState::Denied
                    },
                });
            } else {
                for scope in &request.allowed_scopes {
                    let granted = scopes
                        .clone()
                        .any(|decision| decision.scope == *scope && decision.granted);
                    grants.push(EffectiveGrant {
                        permission_id: request.permission_id.clone(),
                        required: request.required,
                        scope: scope.clone(),
                        state: if granted {
                            PermissionState::Granted
                        } else {
                            PermissionState::Denied
                        },
                    });
                }
            }
        }
        grants.sort_by(|a, b| (&a.permission_id, &a.scope).cmp(&(&b.permission_id, &b.scope)));
        let key = identity_key(&identity);
        let previous = self.state.plugins.get(&key);
        if previous.is_some_and(|entry| entry.identity != identity) {
            return Err(GrantError::InvalidIdentity);
        }
        let revision = previous.map_or(1, |entry| entry.revision.saturating_add(1));
        let mut next = self.state.clone();
        next.schema = 1;
        next.plugins.insert(
            key,
            PluginGrantRecord {
                identity,
                revision,
                grants,
            },
        );
        self.persist(&next)?;
        self.state = next;
        Ok(revision)
    }

    pub fn authorize_domain(
        &self,
        identity: &PluginIdentity,
        record: &DomainRecord,
        operation: Operation,
    ) -> Result<u64, GrantError> {
        identity.validate()?;
        if !valid_identity_part(&record.id, 256)
            || !valid_identity_part(&record.creator_plugin_id, 256)
        {
            return Err(GrantError::InvalidRequest);
        }
        let permission_id = permission_id(&record.category, operation);
        let grants = self.effective_grants(identity);
        let allowed = grants.iter().any(|grant| {
            if grant.permission_id != permission_id || grant.state != PermissionState::Granted {
                return false;
            }
            match &grant.scope {
                GrantScope::OwnCreator { category } => {
                    category == &record.category && record.creator_plugin_id == identity.plugin_id
                }
                GrantScope::PublicCategory { category } => {
                    category == &record.category
                        && if operation == Operation::Create {
                            record.creator_plugin_id == identity.plugin_id && record.published
                        } else {
                            record.creator_plugin_id != identity.plugin_id && record.published
                        }
                }
                GrantScope::SelectedRecords {
                    category,
                    record_ids,
                } => {
                    category == &record.category
                        && record.creator_plugin_id != identity.plugin_id
                        && record.published
                        && record_ids.contains(&record.id)
                }
                GrantScope::Declared { .. } => false,
                GrantScope::OwnPrivateNamespace => false,
            }
        });
        if allowed {
            Ok(self.revision(identity))
        } else {
            Err(GrantError::Denied)
        }
    }

    /// Rechecks the grant revision and expected record revision at commit, then records the
    /// authenticated actor. Callers must persist the record effect transactionally with this audit.
    pub fn audit_domain_commit(
        &mut self,
        identity: &PluginIdentity,
        record: &DomainRecord,
        operation: Operation,
        expected_record_revision: Option<u64>,
        expected_grant_revision: u64,
    ) -> Result<AuditEvent, GrantError> {
        let current_grant_revision = self.authorize_domain(identity, record, operation)?;
        if current_grant_revision != expected_grant_revision {
            return Err(GrantError::Denied);
        }
        if operation != Operation::Create && expected_record_revision != Some(record.revision) {
            return Err(GrantError::StaleRevision);
        }
        if operation == Operation::Create && expected_record_revision.is_some() {
            return Err(GrantError::InvalidRequest);
        }
        let event = AuditEvent {
            actor_plugin_id: identity.plugin_id.clone(),
            target_record_id: record.id.clone(),
            operation: match operation {
                Operation::Create => AuditOperation::Create,
                Operation::Update => AuditOperation::Update,
                Operation::Delete => AuditOperation::Delete,
                Operation::Read => return Err(GrantError::InvalidRequest),
            },
            previous_revision: (operation != Operation::Create).then_some(record.revision),
            resulting_revision: (operation != Operation::Delete)
                .then_some(record.revision.saturating_add(1)),
            grant_revision: current_grant_revision,
        };
        let mut next = self.state.clone();
        next.audit.push(event.clone());
        if next.audit.len() > MAX_AUDIT_EVENTS {
            next.audit.drain(..next.audit.len() - MAX_AUDIT_EVENTS);
        }
        self.persist(&next)?;
        self.state = next;
        Ok(event)
    }

    pub fn audit_events(&self) -> &[AuditEvent] {
        &self.state.audit
    }

    /// storage.persist grants cleanup only for the authenticated caller's private namespace;
    /// it does not grant Storage.Delete or any foreign namespace selection.
    pub fn authorize_private_cleanup(
        &self,
        identity: &PluginIdentity,
        requested_owner_plugin_id: &str,
    ) -> Result<u64, GrantError> {
        if requested_owner_plugin_id != identity.plugin_id {
            return Err(GrantError::ForeignPrivateNamespace);
        }
        let permitted = self.effective_grants(identity).iter().any(|grant| {
            grant.permission_id == gamenode_plugin_protocol::capability::STORAGE_PERSIST
                && grant.scope == GrantScope::OwnPrivateNamespace
                && grant.state == PermissionState::Granted
        });
        if permitted {
            Ok(self.revision(identity))
        } else {
            Err(GrantError::Denied)
        }
    }

    fn persist(&self, state: &StoreFile) -> Result<(), GrantError> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| GrantError::InvalidStore("store has no parent".into()))?;
        ensure_private_dir(parent)?;
        let bytes = serde_json::to_vec(state)
            .map_err(|error| GrantError::InvalidStore(error.to_string()))?;
        if bytes.len() as u64 > MAX_STORE_BYTES {
            return Err(GrantError::InvalidStore(
                "serialized file exceeds size limit".into(),
            ));
        }
        let temp = parent.join(format!(
            ".plugin-permissions-{}-{}",
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
            atomic_replace(&temp, &self.path)?;
            sync_dir(parent)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result?;
        ensure_private_file(&self.path)?;
        Ok(())
    }
}

pub fn permission_id(category: &DomainCategory, operation: Operation) -> String {
    let stem = match category {
        DomainCategory::Runtime => "game.runtime",
        DomainCategory::LibraryRoot => "library.roots",
        DomainCategory::SaveProfile => "game.saves",
    };
    format!(
        "{stem}.{}",
        match operation {
            Operation::Read => "read",
            Operation::Create => "create",
            Operation::Update => "update",
            Operation::Delete => "delete",
        }
    )
}

fn permission_category(permission: &str) -> Option<(DomainCategory, Operation)> {
    [
        DomainCategory::Runtime,
        DomainCategory::LibraryRoot,
        DomainCategory::SaveProfile,
    ]
    .into_iter()
    .flat_map(|category| {
        [
            Operation::Read,
            Operation::Create,
            Operation::Update,
            Operation::Delete,
        ]
        .into_iter()
        .map(move |operation| (category.clone(), operation))
    })
    .find(|(category, operation)| self::permission_id(category, *operation) == permission)
}

fn is_known_permission(permission: &str) -> bool {
    gamenode_plugin_protocol::capability::KNOWN.contains(&permission)
}

fn validate_permission_request(request: &PermissionRequest) -> Result<(), GrantError> {
    if request.permission_id.trim().is_empty()
        || request.permission_id.len() > 128
        || request.permission_id.chars().any(char::is_control)
    {
        return Err(GrantError::InvalidRequest);
    }
    if request.permission_id == gamenode_plugin_protocol::capability::STORAGE_PERSIST {
        if request
            .allowed_scopes
            .iter()
            .any(|scope| *scope != GrantScope::OwnPrivateNamespace)
        {
            return Err(GrantError::InvalidScope);
        }
        return Ok(());
    }
    if is_known_permission(&request.permission_id) && request.allowed_scopes.is_empty() {
        return Err(GrantError::InvalidRequest);
    }
    if permission_category(&request.permission_id).is_none() {
        if !is_known_permission(&request.permission_id) {
            return Ok(());
        }
        if is_known_permission(&request.permission_id)
            && request.allowed_scopes.iter().all(|scope| {
                matches!(scope, GrantScope::Declared { value } if valid_identity_part(value, 512))
            })
        {
            return Ok(());
        }
        return Err(GrantError::InvalidScope);
    }
    for scope in &request.allowed_scopes {
        validate_scope_for_permission(&request.permission_id, scope)?;
    }
    Ok(())
}

fn validate_scope_for_permission(
    permission_id_value: &str,
    scope: &GrantScope,
) -> Result<(), GrantError> {
    if permission_id_value == gamenode_plugin_protocol::capability::STORAGE_PERSIST {
        return if scope == &GrantScope::OwnPrivateNamespace {
            Ok(())
        } else {
            Err(GrantError::InvalidScope)
        };
    }
    let Some((category, operation)) = permission_category(permission_id_value) else {
        return if is_known_permission(permission_id_value)
            && matches!(scope, GrantScope::Declared { value } if valid_identity_part(value, 512))
        {
            Ok(())
        } else {
            Err(GrantError::InvalidScope)
        };
    };
    let matching_category = match scope {
        GrantScope::OwnCreator {
            category: scope_category,
        }
        | GrantScope::PublicCategory {
            category: scope_category,
        }
        | GrantScope::SelectedRecords {
            category: scope_category,
            ..
        } => scope_category == &category,
        GrantScope::Declared { .. } => false,
        GrantScope::OwnPrivateNamespace => false,
    };
    if !matching_category {
        return Err(GrantError::InvalidScope);
    }
    if operation == Operation::Create && matches!(scope, GrantScope::SelectedRecords { .. }) {
        return Err(GrantError::InvalidScope);
    }
    if let GrantScope::SelectedRecords { record_ids, .. } = scope {
        if record_ids.is_empty()
            || record_ids.len() > 1000
            || record_ids.iter().any(|id| !valid_identity_part(id, 256))
        {
            return Err(GrantError::InvalidScope);
        }
    }
    Ok(())
}

fn identity_key(identity: &PluginIdentity) -> String {
    format!(
        "{}\0{}\0{}",
        identity.plugin_id, identity.publisher_fingerprint, identity.profile_id
    )
}

fn valid_identity_part(value: &str, max_len: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max_len && !value.chars().any(char::is_control)
}

#[cfg(unix)]
fn ensure_private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsafe grant directory",
        )),
        Ok(meta) => {
            if meta.permissions().mode() & 0o077 != 0 {
                fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::DirBuilder::new().mode(0o700).create(path)
        }
        Err(error) => Err(error),
    }
}

#[cfg(not(unix))]
fn ensure_private_dir(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsafe grant directory",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir(path),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn ensure_private_file(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsafe grant file",
        ));
    }
    if meta.permissions().mode() & 0o077 != 0 {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_private_file(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsafe grant file",
        ));
    }
    Ok(())
}

fn sync_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn atomic_replace(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };
        let from = from
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let to = to
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        // SAFETY: both paths are NUL-terminated UTF-16 buffers valid for this call.
        let result = unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if result == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
    #[cfg(not(windows))]
    {
        fs::rename(from, to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn identity() -> PluginIdentity {
        PluginIdentity {
            plugin_id: "org.example.consumer".into(),
            publisher_fingerprint: "SHA256:consumer".into(),
            profile_id: "default".into(),
        }
    }

    fn scopes() -> BTreeSet<GrantScope> {
        BTreeSet::from([
            GrantScope::OwnCreator {
                category: DomainCategory::Runtime,
            },
            GrantScope::PublicCategory {
                category: DomainCategory::Runtime,
            },
            GrantScope::SelectedRecords {
                category: DomainCategory::Runtime,
                record_ids: BTreeSet::from(["runtime-a".into()]),
            },
        ])
    }

    fn request(permission_id: &str, allowed_scopes: BTreeSet<GrantScope>) -> PermissionRequest {
        PermissionRequest {
            permission_id: permission_id.into(),
            required: false,
            allowed_scopes,
        }
    }

    #[test]
    fn grants_are_default_deny_persisted_and_operation_scoped() {
        let dir = tempdir().unwrap();
        let mut store = PermissionGrantStore::open(dir.path()).unwrap();
        let actor = identity();
        let record = DomainRecord {
            id: "runtime-a".into(),
            category: DomainCategory::Runtime,
            creator_plugin_id: "org.example.producer".into(),
            published: true,
            revision: 7,
        };
        assert_eq!(
            store.authorize_domain(&actor, &record, Operation::Read),
            Err(GrantError::Denied)
        );
        let read_id = permission_id(&DomainCategory::Runtime, Operation::Read);
        let update_id = permission_id(&DomainCategory::Runtime, Operation::Update);
        let delete_id = permission_id(&DomainCategory::Runtime, Operation::Delete);
        let reqs = vec![
            request(&read_id, scopes()),
            request(&update_id, scopes()),
            request(&delete_id, scopes()),
        ];
        let dec = vec![
            PermissionDecision {
                permission_id: read_id.clone(),
                scope: GrantScope::SelectedRecords {
                    category: DomainCategory::Runtime,
                    record_ids: BTreeSet::from(["runtime-a".into()]),
                },
                granted: true,
            },
            PermissionDecision {
                permission_id: update_id.clone(),
                scope: GrantScope::SelectedRecords {
                    category: DomainCategory::Runtime,
                    record_ids: BTreeSet::from(["runtime-a".into()]),
                },
                granted: true,
            },
        ];
        assert_eq!(store.replace_grants(actor.clone(), &reqs, &dec).unwrap(), 1);
        assert_eq!(
            store.authorize_domain(&actor, &record, Operation::Read),
            Ok(1)
        );
        assert_eq!(
            store.authorize_domain(&actor, &record, Operation::Update),
            Ok(1)
        );
        assert_eq!(
            store.authorize_domain(&actor, &record, Operation::Delete),
            Err(GrantError::Denied)
        );
        let restarted = PermissionGrantStore::open(dir.path()).unwrap();
        assert_eq!(
            restarted.authorize_domain(&actor, &record, Operation::Read),
            Ok(1)
        );
        assert_eq!(
            restarted.authorize_domain(&actor, &record, Operation::Delete),
            Err(GrantError::Denied)
        );
    }

    #[test]
    fn selected_read_does_not_require_category_grant_and_revocation_changes_revision() {
        let dir = tempdir().unwrap();
        let mut store = PermissionGrantStore::open(dir.path()).unwrap();
        let actor = identity();
        let permission = permission_id(&DomainCategory::Runtime, Operation::Read);
        let selected = GrantScope::SelectedRecords {
            category: DomainCategory::Runtime,
            record_ids: BTreeSet::from(["runtime-a".into()]),
        };
        let reqs = vec![request(
            &permission,
            BTreeSet::from([
                selected.clone(),
                GrantScope::PublicCategory {
                    category: DomainCategory::Runtime,
                },
            ]),
        )];
        store
            .replace_grants(
                actor.clone(),
                &reqs,
                &[PermissionDecision {
                    permission_id: permission.clone(),
                    scope: selected.clone(),
                    granted: true,
                }],
            )
            .unwrap();
        let record = DomainRecord {
            id: "runtime-a".into(),
            category: DomainCategory::Runtime,
            creator_plugin_id: "org.example.producer".into(),
            published: true,
            revision: 1,
        };
        assert!(
            store
                .authorize_domain(&actor, &record, Operation::Read)
                .is_ok()
        );
        let revision = store.replace_grants(actor.clone(), &reqs, &[]).unwrap();
        assert_eq!(revision, 2);
        assert_eq!(
            store.authorize_domain(&actor, &record, Operation::Read),
            Err(GrantError::Denied)
        );
    }

    #[test]
    fn create_is_category_scoped_and_creator_scope_requires_user_consent() {
        let id = permission_id(&DomainCategory::Runtime, Operation::Create);
        let selected = GrantScope::SelectedRecords {
            category: DomainCategory::Runtime,
            record_ids: BTreeSet::from(["new".into()]),
        };
        assert_eq!(
            validate_scope_for_permission(&id, &selected),
            Err(GrantError::InvalidScope)
        );
        let dir = tempdir().unwrap();
        let mut store = PermissionGrantStore::open(dir.path()).unwrap();
        let actor = identity();
        let own = GrantScope::OwnCreator {
            category: DomainCategory::Runtime,
        };
        store
            .replace_grants(
                actor.clone(),
                &[request(&id, BTreeSet::from([own.clone()]))],
                &[PermissionDecision {
                    permission_id: id.clone(),
                    scope: own,
                    granted: true,
                }],
            )
            .unwrap();
        let record = DomainRecord {
            id: "created".into(),
            category: DomainCategory::Runtime,
            creator_plugin_id: actor.plugin_id.clone(),
            published: false,
            revision: 0,
        };
        assert!(
            store
                .authorize_domain(&actor, &record, Operation::Create)
                .is_ok()
        );
    }

    #[test]
    fn private_cleanup_is_own_namespace_only_and_does_not_grant_public_delete() {
        let dir = tempdir().unwrap();
        let mut store = PermissionGrantStore::open(dir.path()).unwrap();
        let actor = identity();
        let persist = gamenode_plugin_protocol::capability::STORAGE_PERSIST;
        store
            .replace_grants(
                actor.clone(),
                &[request(
                    persist,
                    BTreeSet::from([GrantScope::OwnPrivateNamespace]),
                )],
                &[PermissionDecision {
                    permission_id: persist.into(),
                    scope: GrantScope::OwnPrivateNamespace,
                    granted: true,
                }],
            )
            .unwrap();
        assert!(
            store
                .authorize_private_cleanup(&actor, &actor.plugin_id)
                .is_ok()
        );
        assert_eq!(
            store.authorize_private_cleanup(&actor, "org.example.other"),
            Err(GrantError::ForeignPrivateNamespace)
        );
        let public = DomainRecord {
            id: "r".into(),
            category: DomainCategory::Runtime,
            creator_plugin_id: "org.example.producer".into(),
            published: true,
            revision: 1,
        };
        assert_eq!(
            store.authorize_domain(&actor, &public, Operation::Delete),
            Err(GrantError::Denied)
        );
    }

    #[test]
    fn commit_audit_uses_authenticated_actor_preserves_record_creator_and_checks_revision() {
        let dir = tempdir().unwrap();
        let mut store = PermissionGrantStore::open(dir.path()).unwrap();
        let actor = identity();
        let permission = permission_id(&DomainCategory::Runtime, Operation::Update);
        let scope = GrantScope::SelectedRecords {
            category: DomainCategory::Runtime,
            record_ids: BTreeSet::from(["runtime-a".into()]),
        };
        let reqs = vec![request(&permission, BTreeSet::from([scope.clone()]))];
        store
            .replace_grants(
                actor.clone(),
                &reqs,
                &[PermissionDecision {
                    permission_id: permission,
                    scope,
                    granted: true,
                }],
            )
            .unwrap();
        let record = DomainRecord {
            id: "runtime-a".into(),
            category: DomainCategory::Runtime,
            creator_plugin_id: "org.example.producer".into(),
            published: true,
            revision: 5,
        };
        assert_eq!(
            store.audit_domain_commit(&actor, &record, Operation::Update, Some(4), 1),
            Err(GrantError::StaleRevision)
        );
        let event = store
            .audit_domain_commit(&actor, &record, Operation::Update, Some(5), 1)
            .unwrap();
        assert_eq!(event.actor_plugin_id, actor.plugin_id);
        assert_eq!(record.creator_plugin_id, "org.example.producer");
        assert_eq!(event.previous_revision, Some(5));
        assert_eq!(event.resulting_revision, Some(6));
        assert_eq!(store.audit_events(), &[event]);
    }

    #[test]
    fn failed_persistence_does_not_change_effective_in_memory_grants() {
        let dir = tempdir().unwrap();
        let mut store = PermissionGrantStore::open(dir.path()).unwrap();
        let actor = identity();
        let permission = permission_id(&DomainCategory::Runtime, Operation::Read);
        let own = GrantScope::OwnCreator {
            category: DomainCategory::Runtime,
        };
        let reqs = vec![request(&permission, BTreeSet::from([own.clone()]))];
        store
            .replace_grants(
                actor.clone(),
                &reqs,
                &[PermissionDecision {
                    permission_id: permission.clone(),
                    scope: own.clone(),
                    granted: true,
                }],
            )
            .unwrap();
        let target_path = store.path.clone();
        fs::remove_file(&target_path).unwrap();
        fs::create_dir(&target_path).unwrap();
        let err = store.replace_grants(actor.clone(), &reqs, &[]).unwrap_err();
        assert!(matches!(err, GrantError::Io(_)));
        assert!(
            store
                .effective_grants(&actor)
                .iter()
                .any(|grant| grant.permission_id == permission
                    && grant.state == PermissionState::Granted)
        );
    }

    #[test]
    fn unknown_optional_permissions_remain_unsupported_and_unknown_required_are_blocked() {
        let dir = tempdir().unwrap();
        let mut store = PermissionGrantStore::open(dir.path()).unwrap();
        let actor = identity();
        let unknown = request(
            "future.capability",
            BTreeSet::from([GrantScope::Declared {
                value: "game:one".into(),
            }]),
        );
        store
            .replace_grants(actor.clone(), &[unknown.clone()], &[])
            .unwrap();
        assert_eq!(
            store.effective_grants(&actor)[0].state,
            PermissionState::Unsupported
        );
        let required = PermissionRequest {
            required: true,
            ..unknown
        };
        assert!(matches!(
            store.replace_grants(actor, &[required], &[]),
            Err(GrantError::RequiredPermissionUnsupported)
        ));
    }

    #[test]
    fn domain_catalog_maps_each_operation_to_a_distinct_known_permission() {
        for category in [
            DomainCategory::Runtime,
            DomainCategory::LibraryRoot,
            DomainCategory::SaveProfile,
        ] {
            let ids: BTreeSet<_> = [
                Operation::Read,
                Operation::Create,
                Operation::Update,
                Operation::Delete,
            ]
            .into_iter()
            .map(|operation| permission_id(&category, operation))
            .collect();
            assert_eq!(ids.len(), 4);
            assert!(ids.iter().all(|permission| {
                gamenode_plugin_protocol::capability::KNOWN.contains(&permission.as_str())
            }));
        }
        assert!(!gamenode_plugin_protocol::capability::KNOWN.contains(&"library.roots.write"));
        assert!(!gamenode_plugin_protocol::capability::KNOWN.contains(&"game.runtime.write"));
    }
}
