//! Host-side consent for cross-plugin data.
//!
//! Activation grants only authorize a later, explicit per-query selection. They do not
//! schedule or perform reads. Category and record grants are kept in separate sets so that a
//! selected record never expands into category-wide access. Sensitive and custom datasets use
//! a separate source-to-recipient rule with exact field names and independent history/live
//! switches.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    sync::{Arc, RwLock},
};

use gamenode_plugin_protocol::{
    common_data::{self, AuthorizedSelection, SelectorError},
    proto::{CommonDataCategory, CommonDataSelector},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::grants::{
    PluginIdentity, atomic_replace, ensure_private_dir, ensure_private_file, sync_dir,
};

const STORE_FILE: &str = "plugin-sharing-policies.json";
const STORE_SCHEMA: u32 = 1;
const MAX_STORE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_RECORD_ID_BYTES: usize = 256;
const MAX_DATASET_ID_BYTES: usize = 256;
const MAX_FIELD_BYTES: usize = 256;
const MAX_FIELDS_PER_POLICY: usize = 256;
const MAX_SELECTED_RECORDS_PER_RECIPIENT: usize = 1000;
const MAX_SOURCE_POLICIES_PER_RECIPIENT: usize = 1024;
static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(1);

/// An activation-time selection for one authenticated recipient.
///
/// This object stores consent only. Call [`SharingPolicies::authorize_common_query`] for a
/// particular RPC, then fetch records using the returned selection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActivationReadGrant {
    categories: BTreeSet<CommonDataCategory>,
    selected_record_ids: BTreeSet<String>,
}

impl ActivationReadGrant {
    /// Construct the optional common-data grants the user selected while activating a plugin.
    /// Empty sets are a valid default-deny grant.
    pub fn new(
        categories: impl IntoIterator<Item = CommonDataCategory>,
        selected_record_ids: impl IntoIterator<Item = String>,
    ) -> Result<Self, SharingPolicyError> {
        let categories = categories.into_iter().collect::<BTreeSet<_>>();
        if categories.contains(&CommonDataCategory::Unspecified) {
            return Err(SharingPolicyError::InvalidCategory);
        }
        let selected_record_ids = selected_record_ids.into_iter().collect::<BTreeSet<_>>();
        if selected_record_ids.len() > MAX_SELECTED_RECORDS_PER_RECIPIENT
            || selected_record_ids
                .iter()
                .any(|id| !valid_plain_identifier(id, MAX_RECORD_ID_BYTES))
        {
            return Err(SharingPolicyError::InvalidRecordId);
        }
        Ok(Self {
            categories,
            selected_record_ids,
        })
    }

    pub fn categories(&self) -> &BTreeSet<CommonDataCategory> {
        &self.categories
    }

    pub fn selected_record_ids(&self) -> &BTreeSet<String> {
        &self.selected_record_ids
    }
}

/// Data classes used to select a common-data or source-recipient policy path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharingClass {
    Common,
    Sensitive,
    Custom,
}

/// Which delivery path is being authorized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// A direct query for the current value.
    Query,
    /// Reading retained historical values.
    History,
    /// Receiving future updates through a live subscription.
    Live,
}

/// An explicitly reviewed source-to-recipient rule for one dataset.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRecipientPolicy {
    source: PluginIdentity,
    recipient: PluginIdentity,
    class: SharingClass,
    dataset_id: String,
    fields: BTreeSet<String>,
    history: bool,
    live: bool,
}

/// Host-owned view of all data-sharing choices for one authenticated recipient.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecipientSharingSnapshot {
    pub revision: u64,
    pub categories: BTreeSet<CommonDataCategory>,
    pub selected_record_ids: BTreeSet<String>,
    pub source_recipient_policies: Vec<SourceRecipientPolicy>,
}

impl SourceRecipientPolicy {
    pub fn new(
        source: PluginIdentity,
        recipient: PluginIdentity,
        class: SharingClass,
        dataset_id: String,
        fields: impl IntoIterator<Item = String>,
        history: bool,
        live: bool,
    ) -> Result<Self, SharingPolicyError> {
        validate_identity(&source)?;
        validate_identity(&recipient)?;
        if class == SharingClass::Common {
            return Err(SharingPolicyError::InvalidPolicyClass);
        }
        if !valid_plain_identifier(&dataset_id, MAX_DATASET_ID_BYTES) {
            return Err(SharingPolicyError::InvalidDatasetId);
        }
        let fields = fields.into_iter().collect::<BTreeSet<_>>();
        if fields.is_empty() || fields.len() > MAX_FIELDS_PER_POLICY {
            return Err(SharingPolicyError::InvalidFields);
        }
        if fields.iter().any(|field| !valid_field_name(field)) {
            return Err(SharingPolicyError::InvalidFields);
        }
        Ok(Self {
            source,
            recipient,
            class,
            dataset_id,
            fields,
            history,
            live,
        })
    }

    pub fn source(&self) -> &PluginIdentity {
        &self.source
    }

    pub fn recipient(&self) -> &PluginIdentity {
        &self.recipient
    }

    pub fn class(&self) -> SharingClass {
        self.class
    }

    pub fn dataset_id(&self) -> &str {
        &self.dataset_id
    }

    pub fn fields(&self) -> &BTreeSet<String> {
        &self.fields
    }

    pub fn allows_history(&self) -> bool {
        self.history
    }

    pub fn allows_live(&self) -> bool {
        self.live
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct IdentityKey {
    plugin_id: String,
    publisher_fingerprint: String,
    profile_id: String,
}

impl From<&PluginIdentity> for IdentityKey {
    fn from(identity: &PluginIdentity) -> Self {
        Self {
            plugin_id: identity.plugin_id.clone(),
            publisher_fingerprint: identity.publisher_fingerprint.clone(),
            profile_id: identity.profile_id.clone(),
        }
    }
}

fn identity_from_key(key: &IdentityKey) -> PluginIdentity {
    PluginIdentity {
        plugin_id: key.plugin_id.clone(),
        publisher_fingerprint: key.publisher_fingerprint.clone(),
        profile_id: key.profile_id.clone(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PolicyKey {
    source: IdentityKey,
    recipient: IdentityKey,
    class: SharingClass,
    dataset_id: String,
}

/// Host-owned policy index. Its optional path is always under a private directory; policy
/// changes are written atomically before becoming visible. Authorization has no query or
/// subscription side effects.
#[derive(Clone, Debug, Default)]
pub struct SharingPolicies {
    activation: BTreeMap<IdentityKey, ActivationReadGrant>,
    source_recipient: BTreeMap<PolicyKey, SourceRecipientPolicy>,
    revisions: BTreeMap<IdentityKey, u64>,
    path: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistentStore {
    schema: u32,
    activation: Vec<PersistedActivationGrant>,
    source_recipient: Vec<SourceRecipientPolicy>,
    revisions: Vec<PersistedRevision>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedActivationGrant {
    recipient: PluginIdentity,
    categories: Vec<i32>,
    selected_record_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedRevision {
    recipient: PluginIdentity,
    revision: u64,
}

/// The exact scope that an authenticated Data/domain query selected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccessScope {
    /// Owner-private data; the broker must still enforce owner-specific API permissions.
    Owner,
    Category(CommonDataCategory),
    SelectedRecord(String),
    /// Explicit source dataset; valid only for Sensitive/Custom source-recipient policies.
    Dataset(String),
}

/// One candidate record's access check. `fields` contains field names, never values.
#[derive(Clone)]
pub struct AccessRequest<'a> {
    pub recipient: &'a PluginIdentity,
    pub source: &'a PluginIdentity,
    pub record_id: &'a str,
    pub dataset_id: &'a str,
    pub category: Option<CommonDataCategory>,
    pub scope: AccessScope,
    pub class: SharingClass,
    pub fields: &'a BTreeSet<String>,
    pub delivery: Delivery,
}

/// The record projection that remains after current host policy has been evaluated.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccessDecision {
    pub allowed: bool,
    pub fields: BTreeSet<String>,
    pub history: bool,
    pub live: bool,
    pub revision: u64,
}

/// Minimal policy interface used by the shared-data broker.
pub trait Authorizer: Send + Sync {
    fn revision(&self, recipient: &PluginIdentity) -> u64;
    fn decide(&self, request: &AccessRequest<'_>) -> AccessDecision;
}

/// Thread-safe handle for host services that need to share policy reads and serialize durable
/// policy mutations. The handle is cheap to clone and can be stored as `Arc<dyn Authorizer>`.
#[derive(Clone, Debug)]
pub struct SharedSharingPolicies {
    inner: Arc<RwLock<SharingPolicies>>,
}

impl Default for SharedSharingPolicies {
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::new(SharingPolicies::default())),
        }
    }
}

impl SharedSharingPolicies {
    /// Open the durable store and wrap it for sharing across host services.
    pub fn open(private_dir: impl AsRef<Path>) -> Result<Self, SharingPolicyError> {
        Ok(Self {
            inner: Arc::new(RwLock::new(SharingPolicies::open(private_dir)?)),
        })
    }

    pub fn set_activation_grants(
        &self,
        recipient: &PluginIdentity,
        grant: ActivationReadGrant,
    ) -> Result<(), SharingPolicyError> {
        self.write()?.set_activation_grants(recipient, grant)
    }

    pub fn clear_activation_grants(
        &self,
        recipient: &PluginIdentity,
    ) -> Result<(), SharingPolicyError> {
        self.write()?.clear_activation_grants(recipient)
    }

    pub fn set_source_recipient_policy(
        &self,
        policy: SourceRecipientPolicy,
    ) -> Result<(), SharingPolicyError> {
        self.write()?.set_source_recipient_policy(policy)
    }

    pub fn revoke_source_recipient_policy(
        &self,
        source: &PluginIdentity,
        recipient: &PluginIdentity,
        class: SharingClass,
        dataset_id: &str,
    ) -> Result<bool, SharingPolicyError> {
        self.write()?
            .revoke_source_recipient_policy(source, recipient, class, dataset_id)
    }

    pub fn recipient_snapshot(
        &self,
        recipient: &PluginIdentity,
    ) -> Result<RecipientSharingSnapshot, SharingPolicyError> {
        self.read()?.recipient_snapshot(recipient)
    }

    /// Atomically replace one recipient's complete data-sharing policy, fenced by the revision
    /// shown in its current settings snapshot.
    pub fn replace_recipient_policies(
        &self,
        recipient: &PluginIdentity,
        expected_revision: u64,
        grant: ActivationReadGrant,
        source_policies: Vec<SourceRecipientPolicy>,
    ) -> Result<u64, SharingPolicyError> {
        self.write()?.replace_recipient_policies(
            recipient,
            expected_revision,
            grant,
            source_policies,
        )
    }

    pub(crate) fn replace_recipient_policies_snapshot(
        &self,
        recipient: &PluginIdentity,
        expected_revision: u64,
        grant: ActivationReadGrant,
        source_policies: Vec<SourceRecipientPolicy>,
    ) -> Result<RecipientSharingSnapshot, SharingPolicyError> {
        let mut policies = self.write()?;
        policies.replace_recipient_policies(
            recipient,
            expected_revision,
            grant,
            source_policies,
        )?;
        policies.recipient_snapshot(recipient)
    }

    fn read(&self) -> Result<std::sync::RwLockReadGuard<'_, SharingPolicies>, SharingPolicyError> {
        self.inner
            .read()
            .map_err(|_| SharingPolicyError::LockPoisoned)
    }

    fn write(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, SharingPolicies>, SharingPolicyError> {
        self.inner
            .write()
            .map_err(|_| SharingPolicyError::LockPoisoned)
    }
}

impl Authorizer for SharedSharingPolicies {
    fn revision(&self, recipient: &PluginIdentity) -> u64 {
        self.read()
            .map(|policies| policies.revision(recipient))
            // A maximal revision fences existing cursors if the policy lock is poisoned.
            .unwrap_or(u64::MAX)
    }

    fn decide(&self, request: &AccessRequest<'_>) -> AccessDecision {
        self.read()
            .map(|policies| policies.decide(request))
            .unwrap_or_else(|_| AccessDecision {
                revision: u64::MAX,
                ..AccessDecision::default()
            })
    }
}

impl SharingPolicies {
    /// Open the host-owned policy database under a private directory.
    pub fn open(private_dir: impl AsRef<Path>) -> Result<Self, SharingPolicyError> {
        let directory = private_dir.as_ref();
        ensure_private_dir(directory)?;
        let path = directory.join(STORE_FILE);
        let mut policies = match fs::symlink_metadata(&path) {
            Ok(metadata)
                if metadata.file_type().is_file() && !metadata.file_type().is_symlink() =>
            {
                if metadata.len() > MAX_STORE_BYTES {
                    return Err(SharingPolicyError::InvalidStore(
                        "file exceeds size limit".into(),
                    ));
                }
                ensure_private_file(&path)?;
                let mut bytes = Vec::with_capacity(metadata.len() as usize);
                File::open(&path)?
                    .take(MAX_STORE_BYTES + 1)
                    .read_to_end(&mut bytes)?;
                if bytes.len() as u64 > MAX_STORE_BYTES {
                    return Err(SharingPolicyError::InvalidStore(
                        "file exceeds size limit".into(),
                    ));
                }
                let store: PersistentStore = serde_json::from_slice(&bytes)
                    .map_err(|error| SharingPolicyError::InvalidStore(error.to_string()))?;
                Self::from_persistent(store)?
            }
            Ok(_) => {
                return Err(SharingPolicyError::InvalidStore(
                    "unsafe policy file".into(),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(error) => return Err(error.into()),
        };
        policies.path = Some(path);
        Ok(policies)
    }

    fn from_persistent(store: PersistentStore) -> Result<Self, SharingPolicyError> {
        if store.schema != STORE_SCHEMA {
            return Err(SharingPolicyError::InvalidStore(
                "unsupported policy schema".into(),
            ));
        }
        let mut policies = Self::default();
        for persisted in store.activation {
            validate_identity(&persisted.recipient)?;
            let categories = persisted
                .categories
                .into_iter()
                .map(|raw| {
                    CommonDataCategory::try_from(raw)
                        .map_err(|_| SharingPolicyError::InvalidCategory)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let grant = ActivationReadGrant::new(categories, persisted.selected_record_ids)?;
            let key = IdentityKey::from(&persisted.recipient);
            if policies.activation.insert(key, grant).is_some() {
                return Err(SharingPolicyError::InvalidStore(
                    "duplicate activation identity".into(),
                ));
            }
        }
        for persisted in store.source_recipient {
            let policy = SourceRecipientPolicy::new(
                persisted.source,
                persisted.recipient,
                persisted.class,
                persisted.dataset_id,
                persisted.fields,
                persisted.history,
                persisted.live,
            )?;
            if policies
                .source_recipient
                .insert(policy_key(&policy), policy)
                .is_some()
            {
                return Err(SharingPolicyError::InvalidStore(
                    "duplicate source-recipient policy".into(),
                ));
            }
        }
        for persisted in store.revisions {
            validate_identity(&persisted.recipient)?;
            if persisted.revision == 0 {
                return Err(SharingPolicyError::InvalidStore(
                    "zero policy revision".into(),
                ));
            }
            if policies
                .revisions
                .insert(IdentityKey::from(&persisted.recipient), persisted.revision)
                .is_some()
            {
                return Err(SharingPolicyError::InvalidStore(
                    "duplicate policy revision".into(),
                ));
            }
        }
        for recipient in policies
            .activation
            .keys()
            .chain(policies.source_recipient.keys().map(|key| &key.recipient))
        {
            if !policies.revisions.contains_key(recipient) {
                return Err(SharingPolicyError::InvalidStore(
                    "policy is missing its revision".into(),
                ));
            }
        }
        Ok(policies)
    }

    fn to_persistent(&self) -> PersistentStore {
        let activation = self
            .activation
            .iter()
            .map(|(identity, grant)| PersistedActivationGrant {
                recipient: identity_from_key(identity),
                categories: grant
                    .categories
                    .iter()
                    .map(|category| *category as i32)
                    .collect(),
                selected_record_ids: grant.selected_record_ids.iter().cloned().collect(),
            })
            .collect();
        let revisions = self
            .revisions
            .iter()
            .map(|(identity, revision)| PersistedRevision {
                recipient: identity_from_key(identity),
                revision: *revision,
            })
            .collect();
        PersistentStore {
            schema: STORE_SCHEMA,
            activation,
            source_recipient: self.source_recipient.values().cloned().collect(),
            revisions,
        }
    }

    fn persist(&self) -> Result<(), SharingPolicyError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let parent = path
            .parent()
            .ok_or_else(|| SharingPolicyError::InvalidStore("policy path has no parent".into()))?;
        ensure_private_dir(parent)?;
        let bytes = serde_json::to_vec(&self.to_persistent())
            .map_err(|error| SharingPolicyError::InvalidStore(error.to_string()))?;
        if bytes.len() as u64 > MAX_STORE_BYTES {
            return Err(SharingPolicyError::InvalidStore(
                "file exceeds size limit".into(),
            ));
        }
        let sequence = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(
            ".{STORE_FILE}.{}.{}.tmp",
            std::process::id(),
            sequence
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result = (|| -> Result<(), SharingPolicyError> {
            let mut file = options.open(&temp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            atomic_replace(&temp, path)?;
            ensure_private_file(path)?;
            sync_dir(parent)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }

    fn commit_candidate(&mut self, candidate: Self) -> Result<(), SharingPolicyError> {
        candidate.persist()?;
        *self = candidate;
        Ok(())
    }

    /// Replace the activation-selected common-data grants for an authenticated plugin.
    pub fn set_activation_grants(
        &mut self,
        recipient: &PluginIdentity,
        grant: ActivationReadGrant,
    ) -> Result<(), SharingPolicyError> {
        validate_identity(recipient)?;
        let mut candidate = self.clone();
        candidate.bump_revision(recipient)?;
        candidate.activation.insert(recipient.into(), grant);
        self.commit_candidate(candidate)
    }

    /// Remove all common-data grants associated with this exact plugin/publisher/profile.
    pub fn clear_activation_grants(
        &mut self,
        recipient: &PluginIdentity,
    ) -> Result<(), SharingPolicyError> {
        validate_identity(recipient)?;
        let key: IdentityKey = recipient.into();
        if self.activation.contains_key(&key) {
            let mut candidate = self.clone();
            candidate.bump_revision(recipient)?;
            candidate.activation.remove(&key);
            self.commit_candidate(candidate)?;
        }
        Ok(())
    }

    /// Current host policy revision for cursor and subscription fencing.
    pub fn revision(&self, recipient: &PluginIdentity) -> u64 {
        self.revisions
            .get(&IdentityKey::from(recipient))
            .copied()
            .unwrap_or_default()
    }

    fn recipient_snapshot(
        &self,
        recipient: &PluginIdentity,
    ) -> Result<RecipientSharingSnapshot, SharingPolicyError> {
        validate_identity(recipient)?;
        let key = IdentityKey::from(recipient);
        let grant = self.activation.get(&key).cloned().unwrap_or_default();
        let mut source_recipient_policies = self
            .source_recipient
            .iter()
            .filter(|(policy_key, _)| policy_key.recipient == key)
            .map(|(_, policy)| policy.clone())
            .collect::<Vec<_>>();
        source_recipient_policies.sort_by(|left, right| {
            (&left.source.plugin_id, &left.dataset_id, left.class).cmp(&(
                &right.source.plugin_id,
                &right.dataset_id,
                right.class,
            ))
        });
        Ok(RecipientSharingSnapshot {
            revision: self.revision(recipient),
            categories: grant.categories,
            selected_record_ids: grant.selected_record_ids,
            source_recipient_policies,
        })
    }

    fn replace_recipient_policies(
        &mut self,
        recipient: &PluginIdentity,
        expected_revision: u64,
        grant: ActivationReadGrant,
        source_policies: Vec<SourceRecipientPolicy>,
    ) -> Result<u64, SharingPolicyError> {
        validate_identity(recipient)?;
        if self.revision(recipient) != expected_revision {
            return Err(SharingPolicyError::StaleRevision);
        }
        if source_policies.len() > MAX_SOURCE_POLICIES_PER_RECIPIENT {
            return Err(SharingPolicyError::TooManyPolicies);
        }
        let recipient_key = IdentityKey::from(recipient);
        let mut policy_keys = BTreeSet::new();
        for policy in &source_policies {
            validate_identity(&policy.source)?;
            validate_identity(&policy.recipient)?;
            if policy.recipient != *recipient
                || policy.source == policy.recipient
                || policy.class == SharingClass::Common
                || !valid_plain_identifier(&policy.dataset_id, MAX_DATASET_ID_BYTES)
                || policy.fields.is_empty()
                || policy.fields.len() > MAX_FIELDS_PER_POLICY
                || policy.fields.iter().any(|field| !valid_field_name(field))
                || !policy_keys.insert(policy_key(policy))
            {
                return Err(SharingPolicyError::InvalidFields);
            }
        }

        let mut candidate = self.clone();
        let revision = candidate.bump_revision(recipient)?;
        candidate.activation.insert(recipient_key.clone(), grant);
        candidate
            .source_recipient
            .retain(|key, _| key.recipient != recipient_key);
        for policy in source_policies {
            candidate
                .source_recipient
                .insert(policy_key(&policy), policy);
        }
        self.commit_candidate(candidate)?;
        Ok(revision)
    }

    fn bump_revision(&mut self, recipient: &PluginIdentity) -> Result<u64, SharingPolicyError> {
        let key = IdentityKey::from(recipient);
        let next = self
            .revisions
            .get(&key)
            .copied()
            .unwrap_or_default()
            .checked_add(1)
            .ok_or(SharingPolicyError::RevisionExhausted)?;
        self.revisions.insert(key, next);
        Ok(next)
    }

    /// Resolve a request's explicit selector against the recipient's current activation grants.
    ///
    /// The result is a filter for a subsequent host query; no data access occurs here. Missing
    /// activation grants, a missing selector, and an empty selector all authorize no shared data.
    pub fn authorize_common_query(
        &self,
        recipient: &PluginIdentity,
        selector: Option<&CommonDataSelector>,
    ) -> Result<AuthorizedSelection, SharingPolicyError> {
        validate_identity(recipient)?;
        let grant = self.activation.get(&recipient.into());
        let no_categories = BTreeSet::new();
        let no_records = BTreeSet::new();
        let categories = grant.map_or(&no_categories, |grant| &grant.categories);
        let records = grant.map_or(&no_records, |grant| &grant.selected_record_ids);
        Ok(common_data::authorize_selector(
            selector, categories, records,
        )?)
    }

    /// Test whether a particular record is selected by the authorized per-query filter.
    pub fn common_record_in_query(
        selection: &AuthorizedSelection,
        category: CommonDataCategory,
        record_id: &str,
    ) -> bool {
        selection.includes_category(category) || selection.includes_record(record_id)
    }

    /// Replace one exact source-to-recipient dataset policy.
    pub fn set_source_recipient_policy(
        &mut self,
        policy: SourceRecipientPolicy,
    ) -> Result<(), SharingPolicyError> {
        // Revalidate at the mutation boundary in case a policy was deserialized or otherwise
        // constructed outside this module in the future.
        validate_identity(&policy.source)?;
        validate_identity(&policy.recipient)?;
        if policy.class == SharingClass::Common
            || !valid_plain_identifier(&policy.dataset_id, MAX_DATASET_ID_BYTES)
            || policy.fields.is_empty()
            || policy.fields.len() > MAX_FIELDS_PER_POLICY
            || policy.fields.iter().any(|field| !valid_field_name(field))
        {
            return Err(SharingPolicyError::InvalidFields);
        }
        let mut candidate = self.clone();
        candidate.bump_revision(&policy.recipient)?;
        candidate
            .source_recipient
            .insert(policy_key(&policy), policy);
        self.commit_candidate(candidate)
    }

    pub fn revoke_source_recipient_policy(
        &mut self,
        source: &PluginIdentity,
        recipient: &PluginIdentity,
        class: SharingClass,
        dataset_id: &str,
    ) -> Result<bool, SharingPolicyError> {
        validate_identity(source)?;
        validate_identity(recipient)?;
        if !valid_plain_identifier(dataset_id, MAX_DATASET_ID_BYTES) {
            return Err(SharingPolicyError::InvalidDatasetId);
        }
        let key = PolicyKey {
            source: source.into(),
            recipient: recipient.into(),
            class,
            dataset_id: dataset_id.to_owned(),
        };
        if self.source_recipient.contains_key(&key) {
            let mut candidate = self.clone();
            candidate.bump_revision(recipient)?;
            candidate.source_recipient.remove(&key);
            self.commit_candidate(candidate)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Authorize one field and delivery path using the exact source, recipient, class and dataset.
    pub fn allows_source_delivery(
        &self,
        source: &PluginIdentity,
        recipient: &PluginIdentity,
        class: SharingClass,
        dataset_id: &str,
        field: &str,
        delivery: Delivery,
    ) -> Result<bool, SharingPolicyError> {
        validate_identity(source)?;
        validate_identity(recipient)?;
        if !valid_plain_identifier(dataset_id, MAX_DATASET_ID_BYTES) {
            return Err(SharingPolicyError::InvalidDatasetId);
        }
        if !valid_field_name(field) {
            return Err(SharingPolicyError::InvalidFields);
        }
        let Some(policy) = self.source_recipient.get(&PolicyKey {
            source: source.into(),
            recipient: recipient.into(),
            class,
            dataset_id: dataset_id.to_owned(),
        }) else {
            return Ok(false);
        };
        let delivery_allowed = match delivery {
            Delivery::Query => true,
            Delivery::History => policy.history,
            Delivery::Live => policy.live,
        };
        Ok(delivery_allowed && policy.fields.contains(field))
    }

    fn decide_access(&self, request: &AccessRequest<'_>) -> AccessDecision {
        let revision = self.revision(request.recipient);
        let denied = AccessDecision {
            revision,
            ..AccessDecision::default()
        };
        if validate_identity(request.recipient).is_err()
            || validate_identity(request.source).is_err()
            || request.source.profile_id != request.recipient.profile_id
            || !valid_plain_identifier(request.record_id, MAX_RECORD_ID_BYTES)
            || !valid_plain_identifier(request.dataset_id, MAX_DATASET_ID_BYTES)
        {
            return denied;
        }

        // The broker owns the private-owner permission check and can bypass sharing review for
        // that path. Returning deny here makes accidentally routing owner data through the
        // cross-plugin broker fail closed.
        if request.scope == AccessScope::Owner {
            return denied;
        }

        let requested_scope_is_explicit = match &request.scope {
            AccessScope::Owner => false,
            AccessScope::Category(category) => {
                request.class == SharingClass::Common && request.category == Some(*category)
            }
            AccessScope::SelectedRecord(record_id) => {
                request.class == SharingClass::Common && record_id == request.record_id
            }
            AccessScope::Dataset(dataset_id) => {
                request.class != SharingClass::Common && dataset_id == request.dataset_id
            }
        };
        if !requested_scope_is_explicit {
            return denied;
        }

        if request.fields.is_empty() {
            return denied;
        }

        if request.class == SharingClass::Common {
            let Some(grant) = self.activation.get(&IdentityKey::from(request.recipient)) else {
                return denied;
            };
            let scope_granted = match &request.scope {
                AccessScope::Owner => false,
                AccessScope::Category(category) => grant.categories.contains(category),
                AccessScope::SelectedRecord(record_id) => {
                    grant.selected_record_ids.contains(record_id)
                }
                AccessScope::Dataset(_) => false,
            };
            if !scope_granted {
                return denied;
            }
            let fields = request
                .fields
                .iter()
                .filter(|field| valid_field_name(field))
                .cloned()
                .collect::<BTreeSet<_>>();
            let allowed = !fields.is_empty();
            return AccessDecision {
                allowed,
                fields,
                history: allowed && request.delivery == Delivery::History,
                live: allowed && request.delivery == Delivery::Live,
                revision,
            };
        }

        let Some(policy) = self.source_recipient.get(&PolicyKey {
            source: request.source.into(),
            recipient: request.recipient.into(),
            class: request.class,
            dataset_id: request.dataset_id.to_owned(),
        }) else {
            return denied;
        };
        let delivery_allowed = match request.delivery {
            Delivery::Query => true,
            Delivery::History => policy.history,
            Delivery::Live => policy.live,
        };
        let fields = request
            .fields
            .iter()
            .filter(|field| valid_field_name(field) && policy.fields.contains(*field))
            .cloned()
            .collect::<BTreeSet<_>>();
        let allowed = delivery_allowed && !fields.is_empty();
        AccessDecision {
            allowed,
            fields,
            history: policy.history,
            live: policy.live,
            revision,
        }
    }
}

impl Authorizer for SharingPolicies {
    fn revision(&self, recipient: &PluginIdentity) -> u64 {
        SharingPolicies::revision(self, recipient)
    }

    fn decide(&self, request: &AccessRequest<'_>) -> AccessDecision {
        self.decide_access(request)
    }
}

fn policy_key(policy: &SourceRecipientPolicy) -> PolicyKey {
    PolicyKey {
        source: (&policy.source).into(),
        recipient: (&policy.recipient).into(),
        class: policy.class,
        dataset_id: policy.dataset_id.clone(),
    }
}

fn validate_identity(identity: &PluginIdentity) -> Result<(), SharingPolicyError> {
    identity
        .validate()
        .map_err(|_| SharingPolicyError::InvalidIdentity)
}

fn valid_plain_identifier(value: &str, max_bytes: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max_bytes && !value.chars().any(char::is_control)
}

fn valid_field_name(value: &str) -> bool {
    if value.is_empty()
        || value.len() > MAX_FIELD_BYTES
        || value.split('.').any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
    {
        return false;
    }

    // Reject common secret-bearing names after normalizing punctuation and case, so variations
    // such as `refresh_token`, `api-key`, and `Private.Key` cannot bypass the denylist.
    let normalized = value
        .bytes()
        .filter(u8::is_ascii_alphanumeric)
        .map(|byte| byte.to_ascii_lowercase())
        .map(char::from)
        .collect::<String>();
    ![
        "password",
        "passwd",
        "secret",
        "token",
        "credential",
        "privatekey",
        "apikey",
        "authcookie",
        "authorization",
    ]
    .iter()
    .any(|secret| normalized.contains(secret))
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SharingPolicyError {
    #[error("authenticated plugin identity is invalid")]
    InvalidIdentity,
    #[error("common-data category is unspecified")]
    InvalidCategory,
    #[error("selected record id is invalid")]
    InvalidRecordId,
    #[error("dataset id is invalid")]
    InvalidDatasetId,
    #[error("field list is invalid or contains a secret-bearing field")]
    InvalidFields,
    #[error("common data cannot use a source-recipient policy")]
    InvalidPolicyClass,
    #[error("sharing policy changed; reload the settings before saving")]
    StaleRevision,
    #[error("recipient has too many source-sharing policies")]
    TooManyPolicies,
    #[error("policy revision exhausted")]
    RevisionExhausted,
    #[error("sharing policy lock is poisoned")]
    LockPoisoned,
    #[error("policy store is invalid: {0}")]
    InvalidStore(String),
    #[error("policy store I/O failed: {0}")]
    Io(String),
    #[error(transparent)]
    Selector(#[from] SelectorError),
}

impl From<io::Error> for SharingPolicyError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(plugin_id: &str, publisher: &str, profile: &str) -> PluginIdentity {
        PluginIdentity {
            plugin_id: plugin_id.into(),
            publisher_fingerprint: publisher.into(),
            profile_id: profile.into(),
        }
    }

    fn selector(categories: &[CommonDataCategory], record_ids: &[&str]) -> CommonDataSelector {
        CommonDataSelector {
            categories: categories.iter().map(|category| *category as i32).collect(),
            selected_record_ids: record_ids.iter().map(|id| (*id).to_owned()).collect(),
        }
    }

    #[test]
    fn common_data_is_default_deny_and_requires_per_query_selection() {
        let mut policies = SharingPolicies::default();
        let recipient = identity("org.consumer", "SHA256:consumer", "profile-a");
        let requested = selector(&[CommonDataCategory::GamePaths], &["record-allowed"]);

        let no_activation = policies
            .authorize_common_query(&recipient, Some(&requested))
            .unwrap();
        assert!(!no_activation.includes_category(CommonDataCategory::GamePaths));
        assert!(!no_activation.includes_record("record-allowed"));

        policies
            .set_activation_grants(
                &recipient,
                ActivationReadGrant::new(
                    [CommonDataCategory::GamePaths],
                    ["record-allowed".to_owned()],
                )
                .unwrap(),
            )
            .unwrap();
        let omitted = policies.authorize_common_query(&recipient, None).unwrap();
        assert!(!omitted.includes_category(CommonDataCategory::GamePaths));
        assert!(!omitted.includes_record("record-allowed"));

        let selected = policies
            .authorize_common_query(&recipient, Some(&requested))
            .unwrap();
        assert!(selected.includes_category(CommonDataCategory::GamePaths));
        assert!(selected.includes_record("record-allowed"));
        assert!(!selected.includes_record("record-other"));
    }

    #[test]
    fn category_and_selected_record_grants_are_independent() {
        let mut policies = SharingPolicies::default();
        let recipient = identity("org.consumer", "SHA256:consumer", "profile-a");
        policies
            .set_activation_grants(
                &recipient,
                ActivationReadGrant::new([], ["record-only".to_owned()]).unwrap(),
            )
            .unwrap();
        let request = selector(
            &[CommonDataCategory::GamePerformanceHistory],
            &["record-only", "category-ungranted"],
        );
        let selected = policies
            .authorize_common_query(&recipient, Some(&request))
            .unwrap();
        assert!(!selected.includes_category(CommonDataCategory::GamePerformanceHistory));
        assert!(SharingPolicies::common_record_in_query(
            &selected,
            CommonDataCategory::GamePaths,
            "record-only"
        ));
        assert!(!SharingPolicies::common_record_in_query(
            &selected,
            CommonDataCategory::GamePaths,
            "category-ungranted"
        ));
    }

    #[test]
    fn authorizer_requires_exact_explicit_common_scope_and_projects_safe_fields() {
        let recipient = identity("org.consumer", "SHA256:consumer", "profile-a");
        let source = identity("org.source", "SHA256:source", "profile-a");
        let mut policies = SharingPolicies::default();
        policies
            .set_activation_grants(
                &recipient,
                ActivationReadGrant::new(
                    [CommonDataCategory::GamePaths],
                    ["record-selected".into()],
                )
                .unwrap(),
            )
            .unwrap();

        let available = [
            "game.path".to_owned(),
            "game.token".to_owned(),
            "metadata.note".to_owned(),
        ]
        .into_iter()
        .collect();
        let request = AccessRequest {
            recipient: &recipient,
            source: &source,
            record_id: "record-selected",
            dataset_id: "games",
            category: Some(CommonDataCategory::GamePaths),
            scope: AccessScope::SelectedRecord("record-selected".into()),
            class: SharingClass::Common,
            fields: &available,
            delivery: Delivery::Query,
        };
        let selected = policies.decide(&request);
        assert!(selected.allowed);
        assert_eq!(
            selected.fields,
            ["game.path", "metadata.note"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        );
        assert_eq!(selected.revision, policies.revision(&recipient));

        let category_query = AccessRequest {
            record_id: "another-published-record",
            scope: AccessScope::Category(CommonDataCategory::GamePaths),
            ..request.clone()
        };
        assert!(policies.decide(&category_query).allowed);

        let category_without_grant = AccessRequest {
            scope: AccessScope::Category(CommonDataCategory::GamePerformanceHistory),
            category: Some(CommonDataCategory::GamePerformanceHistory),
            ..request
        };
        assert!(!policies.decide(&category_without_grant).allowed);
    }

    #[test]
    fn authorizer_applies_per_field_history_and_live_grants() {
        let source = identity("org.source", "SHA256:source", "profile-a");
        let recipient = identity("org.consumer", "SHA256:consumer", "profile-a");
        let mut policies = SharingPolicies::default();
        policies
            .set_source_recipient_policy(
                SourceRecipientPolicy::new(
                    source.clone(),
                    recipient.clone(),
                    SharingClass::Sensitive,
                    "hardware".into(),
                    ["device.model".into()],
                    true,
                    false,
                )
                .unwrap(),
            )
            .unwrap();
        let available = ["device.model".to_owned(), "device.serial".to_owned()]
            .into_iter()
            .collect();
        let request = AccessRequest {
            recipient: &recipient,
            source: &source,
            record_id: "hardware-record",
            dataset_id: "hardware",
            category: None,
            scope: AccessScope::Dataset("hardware".into()),
            class: SharingClass::Sensitive,
            fields: &available,
            delivery: Delivery::History,
        };

        let history = policies.decide(&request);
        assert!(history.allowed);
        assert_eq!(
            history.fields,
            ["device.model".to_owned()].into_iter().collect()
        );
        assert!(history.history);
        assert!(!history.live);

        let live_request = AccessRequest {
            delivery: Delivery::Live,
            ..request.clone()
        };
        assert!(!policies.decide(&live_request).allowed);

        let foreign_source = identity("org.source", "SHA256:rotated", "profile-a");
        let foreign_request = AccessRequest {
            source: &foreign_source,
            ..request
        };
        assert!(!policies.decide(&foreign_request).allowed);
    }

    #[test]
    fn sensitive_and_custom_dataset_scope_requires_exact_dataset_and_delivery_grants() {
        let source = identity("org.source", "SHA256:source", "profile-a");
        let recipient = identity("org.consumer", "SHA256:consumer", "profile-a");
        let mut policies = SharingPolicies::default();
        policies
            .set_source_recipient_policy(
                SourceRecipientPolicy::new(
                    source.clone(),
                    recipient.clone(),
                    SharingClass::Custom,
                    "org.source.metrics".into(),
                    ["fps.average".into()],
                    true,
                    true,
                )
                .unwrap(),
            )
            .unwrap();
        let fields = ["fps.average".to_owned()].into_iter().collect();
        let request = AccessRequest {
            recipient: &recipient,
            source: &source,
            record_id: "record-9",
            dataset_id: "org.source.metrics",
            category: None,
            scope: AccessScope::Dataset("org.source.metrics".into()),
            class: SharingClass::Custom,
            fields: &fields,
            delivery: Delivery::Query,
        };

        for delivery in [Delivery::Query, Delivery::History, Delivery::Live] {
            assert!(
                policies
                    .decide(&AccessRequest {
                        delivery,
                        ..request.clone()
                    })
                    .allowed
            );
        }

        let wrong_dataset = AccessRequest {
            scope: AccessScope::Dataset("org.source.other".into()),
            ..request.clone()
        };
        assert!(!policies.decide(&wrong_dataset).allowed);

        let category_scope = AccessRequest {
            category: Some(CommonDataCategory::GamePaths),
            scope: AccessScope::Category(CommonDataCategory::GamePaths),
            ..request.clone()
        };
        assert!(!policies.decide(&category_scope).allowed);
        let selected_record_scope = AccessRequest {
            scope: AccessScope::SelectedRecord("record-9".into()),
            ..request.clone()
        };
        assert!(!policies.decide(&selected_record_scope).allowed);

        let common_dataset_scope = AccessRequest {
            class: SharingClass::Common,
            ..request
        };
        assert!(!policies.decide(&common_dataset_scope).allowed);
    }

    #[test]
    fn malformed_selector_categories_and_records_are_rejected() {
        let policies = SharingPolicies::default();
        let recipient = identity("org.consumer", "SHA256:consumer", "profile-a");
        let unspecified = CommonDataSelector {
            categories: vec![CommonDataCategory::Unspecified as i32],
            selected_record_ids: Vec::new(),
        };
        assert_eq!(
            policies.authorize_common_query(&recipient, Some(&unspecified)),
            Err(SharingPolicyError::Selector(SelectorError::Unspecified))
        );
        let duplicate_record = CommonDataSelector {
            categories: Vec::new(),
            selected_record_ids: vec!["same".into(), "same".into()],
        };
        assert_eq!(
            policies.authorize_common_query(&recipient, Some(&duplicate_record)),
            Err(SharingPolicyError::Selector(
                SelectorError::DuplicateRecordId
            ))
        );
        assert!(
            ActivationReadGrant::new([CommonDataCategory::Unspecified], std::iter::empty())
                .is_err()
        );
    }

    #[test]
    fn exact_plugin_publisher_and_profile_identity_is_required() {
        let mut policies = SharingPolicies::default();
        let recipient = identity("org.consumer", "SHA256:consumer", "profile-a");
        policies
            .set_activation_grants(
                &recipient,
                ActivationReadGrant::new([CommonDataCategory::RuntimeCatalog], std::iter::empty())
                    .unwrap(),
            )
            .unwrap();
        let changed_publisher = identity("org.consumer", "SHA256:other", "profile-a");
        let changed_profile = identity("org.consumer", "SHA256:consumer", "profile-b");
        let query = selector(&[CommonDataCategory::RuntimeCatalog], &[]);
        for foreign in [&changed_publisher, &changed_profile] {
            let selection = policies
                .authorize_common_query(foreign, Some(&query))
                .unwrap();
            assert!(!selection.includes_category(CommonDataCategory::RuntimeCatalog));
        }
    }

    #[test]
    fn policy_revision_increases_when_grants_change_and_fences_revocation() {
        let source = identity("org.source", "SHA256:source", "profile-a");
        let recipient = identity("org.consumer", "SHA256:consumer", "profile-a");
        let mut policies = SharingPolicies::default();
        assert_eq!(policies.revision(&recipient), 0);
        policies
            .set_activation_grants(
                &recipient,
                ActivationReadGrant::new([CommonDataCategory::GamePaths], std::iter::empty())
                    .unwrap(),
            )
            .unwrap();
        let activation_revision = policies.revision(&recipient);
        assert_eq!(activation_revision, 1);

        policies
            .set_source_recipient_policy(
                SourceRecipientPolicy::new(
                    source.clone(),
                    recipient.clone(),
                    SharingClass::Sensitive,
                    "org.source.device".into(),
                    ["device.model".into()],
                    false,
                    false,
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(policies.revision(&recipient), activation_revision + 1);
        assert!(
            policies
                .revoke_source_recipient_policy(
                    &source,
                    &recipient,
                    SharingClass::Sensitive,
                    "org.source.device",
                )
                .unwrap()
        );
        assert_eq!(policies.revision(&recipient), activation_revision + 2);
    }

    #[test]
    fn sensitive_and_custom_fields_are_scoped_and_history_live_are_separate() {
        let source = identity("org.source", "SHA256:source", "profile-a");
        let recipient = identity("org.consumer", "SHA256:consumer", "profile-a");
        let mut policies = SharingPolicies::default();
        policies
            .set_source_recipient_policy(
                SourceRecipientPolicy::new(
                    source.clone(),
                    recipient.clone(),
                    SharingClass::Custom,
                    "org.source.dataset".into(),
                    ["summary.fps".into(), "summary.platform".into()],
                    false,
                    true,
                )
                .unwrap(),
            )
            .unwrap();

        assert!(
            policies
                .allows_source_delivery(
                    &source,
                    &recipient,
                    SharingClass::Custom,
                    "org.source.dataset",
                    "summary.fps",
                    Delivery::Query,
                )
                .unwrap()
        );
        assert!(
            !policies
                .allows_source_delivery(
                    &source,
                    &recipient,
                    SharingClass::Custom,
                    "org.source.dataset",
                    "summary.fps",
                    Delivery::History,
                )
                .unwrap()
        );
        assert!(
            policies
                .allows_source_delivery(
                    &source,
                    &recipient,
                    SharingClass::Custom,
                    "org.source.dataset",
                    "summary.fps",
                    Delivery::Live,
                )
                .unwrap()
        );
        assert!(
            !policies
                .allows_source_delivery(
                    &source,
                    &recipient,
                    SharingClass::Custom,
                    "org.source.dataset",
                    "summary.platform.extra",
                    Delivery::Query,
                )
                .unwrap()
        );
        assert!(
            !policies
                .allows_source_delivery(
                    &source,
                    &recipient,
                    SharingClass::Sensitive,
                    "org.source.dataset",
                    "summary.fps",
                    Delivery::Query,
                )
                .unwrap()
        );
    }

    #[test]
    fn secret_and_malformed_fields_are_rejected() {
        let source = identity("org.source", "SHA256:source", "profile-a");
        let recipient = identity("org.consumer", "SHA256:consumer", "profile-a");
        for secret_field in [
            "api_key",
            "refresh.token",
            "nested.passwordHash",
            "auth-cookie",
        ] {
            assert_eq!(
                SourceRecipientPolicy::new(
                    source.clone(),
                    recipient.clone(),
                    SharingClass::Sensitive,
                    "org.source.dataset".into(),
                    [secret_field.into()],
                    true,
                    true,
                )
                .unwrap_err(),
                SharingPolicyError::InvalidFields
            );
        }
        assert!(
            SourceRecipientPolicy::new(
                source,
                recipient,
                SharingClass::Sensitive,
                "org.source.dataset".into(),
                ["field..name".into()],
                true,
                false,
            )
            .is_err()
        );
    }

    #[test]
    fn common_and_source_recipient_grants_survive_reopen_and_revocation() {
        let directory = tempfile::tempdir().unwrap();
        let source = identity("org.source", "SHA256:source", "profile-a");
        let recipient = identity("org.consumer", "SHA256:consumer", "profile-a");
        let selected = selector(&[CommonDataCategory::GamePerformanceHistory], &["record-1"]);
        let mut policies = SharingPolicies::open(directory.path()).unwrap();
        policies
            .set_activation_grants(
                &recipient,
                ActivationReadGrant::new(
                    [CommonDataCategory::GamePerformanceHistory],
                    ["record-1".into()],
                )
                .unwrap(),
            )
            .unwrap();
        policies
            .set_source_recipient_policy(
                SourceRecipientPolicy::new(
                    source.clone(),
                    recipient.clone(),
                    SharingClass::Sensitive,
                    "hardware".into(),
                    ["device.model".into()],
                    true,
                    false,
                )
                .unwrap(),
            )
            .unwrap();
        let persisted_revision = policies.revision(&recipient);
        drop(policies);

        let mut reopened = SharingPolicies::open(directory.path()).unwrap();
        assert_eq!(reopened.revision(&recipient), persisted_revision);
        let common = reopened
            .authorize_common_query(&recipient, Some(&selected))
            .unwrap();
        assert!(common.includes_category(CommonDataCategory::GamePerformanceHistory));
        assert!(common.includes_record("record-1"));
        assert!(
            reopened
                .allows_source_delivery(
                    &source,
                    &recipient,
                    SharingClass::Sensitive,
                    "hardware",
                    "device.model",
                    Delivery::History,
                )
                .unwrap()
        );

        assert!(
            reopened
                .revoke_source_recipient_policy(
                    &source,
                    &recipient,
                    SharingClass::Sensitive,
                    "hardware",
                )
                .unwrap()
        );
        assert_eq!(reopened.revision(&recipient), persisted_revision + 1);
        drop(reopened);

        let reopened = SharingPolicies::open(directory.path()).unwrap();
        assert_eq!(reopened.revision(&recipient), persisted_revision + 1);
        assert!(
            !reopened
                .allows_source_delivery(
                    &source,
                    &recipient,
                    SharingClass::Sensitive,
                    "hardware",
                    "device.model",
                    Delivery::History,
                )
                .unwrap()
        );
        let common = reopened
            .authorize_common_query(&recipient, Some(&selected))
            .unwrap();
        assert!(common.includes_category(CommonDataCategory::GamePerformanceHistory));
    }

    #[test]
    fn recipient_policy_replacement_is_atomic_persisted_and_revision_fenced() {
        let directory = tempfile::tempdir().unwrap();
        let shared = SharedSharingPolicies::open(directory.path()).unwrap();
        let source = identity("org.source", "SHA256:source", "profile-a");
        let recipient = identity("org.consumer", "SHA256:consumer", "profile-a");
        let policy = SourceRecipientPolicy::new(
            source.clone(),
            recipient.clone(),
            SharingClass::Custom,
            "org.source.metrics".into(),
            ["fps".into()],
            true,
            false,
        )
        .unwrap();
        let expected_revision = shared.recipient_snapshot(&recipient).unwrap().revision;
        let next_revision = shared
            .replace_recipient_policies(
                &recipient,
                expected_revision,
                ActivationReadGrant::new(
                    [CommonDataCategory::GamePerformanceHistory],
                    ["record-1".into()],
                )
                .unwrap(),
                vec![policy],
            )
            .unwrap();
        assert_eq!(next_revision, expected_revision + 1);
        assert!(matches!(
            shared.replace_recipient_policies(
                &recipient,
                expected_revision,
                ActivationReadGrant::default(),
                Vec::new(),
            ),
            Err(SharingPolicyError::StaleRevision)
        ));

        let snapshot = shared.recipient_snapshot(&recipient).unwrap();
        assert!(
            snapshot
                .categories
                .contains(&CommonDataCategory::GamePerformanceHistory)
        );
        assert!(snapshot.selected_record_ids.contains("record-1"));
        assert_eq!(snapshot.source_recipient_policies.len(), 1);
        assert!(
            snapshot.source_recipient_policies[0]
                .fields()
                .contains("fps")
        );

        let reopened = SharedSharingPolicies::open(directory.path()).unwrap();
        let reopened_snapshot = reopened.recipient_snapshot(&recipient).unwrap();
        assert_eq!(reopened_snapshot, snapshot);
        assert!(
            reopened
                .revoke_source_recipient_policy(
                    &source,
                    &recipient,
                    SharingClass::Custom,
                    "org.source.metrics",
                )
                .unwrap()
        );
        assert_eq!(
            reopened
                .recipient_snapshot(&recipient)
                .unwrap()
                .source_recipient_policies
                .len(),
            0
        );
    }

    #[test]
    fn recipient_policy_replacement_rejects_foreign_recipients_and_duplicates() {
        let recipient = identity("org.consumer", "SHA256:consumer", "profile-a");
        let other = identity("org.other", "SHA256:other", "profile-a");
        let source = identity("org.source", "SHA256:source", "profile-a");
        let policy = SourceRecipientPolicy::new(
            source,
            other.clone(),
            SharingClass::Custom,
            "metrics".into(),
            ["fps".into()],
            false,
            true,
        )
        .unwrap();
        let mut policies = SharingPolicies::default();
        assert!(matches!(
            policies.replace_recipient_policies(
                &recipient,
                0,
                ActivationReadGrant::default(),
                vec![policy.clone()],
            ),
            Err(SharingPolicyError::InvalidFields)
        ));
        assert!(matches!(
            policies.replace_recipient_policies(
                &other,
                0,
                ActivationReadGrant::default(),
                vec![policy.clone(), policy],
            ),
            Err(SharingPolicyError::InvalidFields)
        ));
    }

    #[test]
    fn persistence_rejects_unknown_fields_unsupported_schema_and_oversized_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(STORE_FILE);
        fs::write(
            &path,
            br#"{"schema":1,"activation":[],"source_recipient":[],"revisions":[],"extra":true}"#,
        )
        .unwrap();
        assert!(matches!(
            SharingPolicies::open(directory.path()),
            Err(SharingPolicyError::InvalidStore(_))
        ));

        fs::write(
            &path,
            br#"{"schema":2,"activation":[],"source_recipient":[],"revisions":[]}"#,
        )
        .unwrap();
        assert!(matches!(
            SharingPolicies::open(directory.path()),
            Err(SharingPolicyError::InvalidStore(message)) if message == "unsupported policy schema"
        ));

        fs::write(&path, vec![b'x'; MAX_STORE_BYTES as usize + 1]).unwrap();
        assert!(matches!(
            SharingPolicies::open(directory.path()),
            Err(SharingPolicyError::InvalidStore(message)) if message == "file exceeds size limit"
        ));
    }

    #[test]
    fn shared_handle_serializes_mutations_and_implements_authorizer() {
        let directory = tempfile::tempdir().unwrap();
        let shared = SharedSharingPolicies::open(directory.path()).unwrap();
        let recipient = identity("org.consumer", "SHA256:consumer", "profile-a");
        let source = identity("org.source", "SHA256:source", "profile-a");
        let available = ["runtime.name".to_owned()].into_iter().collect();
        let request = AccessRequest {
            recipient: &recipient,
            source: &source,
            record_id: "runtime-1",
            dataset_id: "runtime-catalog",
            category: Some(CommonDataCategory::RuntimeCatalog),
            scope: AccessScope::Category(CommonDataCategory::RuntimeCatalog),
            class: SharingClass::Common,
            fields: &available,
            delivery: Delivery::Query,
        };
        let authorizer: Arc<dyn Authorizer> = Arc::new(shared.clone());
        let before = authorizer.revision(&recipient);
        assert!(!authorizer.decide(&request).allowed);

        shared
            .set_activation_grants(
                &recipient,
                ActivationReadGrant::new([CommonDataCategory::RuntimeCatalog], std::iter::empty())
                    .unwrap(),
            )
            .unwrap();
        let granted = authorizer.decide(&request);
        assert!(granted.allowed);
        assert!(granted.revision > before);

        shared.clear_activation_grants(&recipient).unwrap();
        let revoked = authorizer.decide(&request);
        assert!(!revoked.allowed);
        assert!(revoked.revision > granted.revision);
    }
}
