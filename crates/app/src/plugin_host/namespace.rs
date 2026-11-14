//! Host-owned identity and owner scopes for third-party private resources.
//!
//! A resolver is created from authenticated package/session state. Plugin request messages never
//! construct owners or supply filesystem paths. Persistent owner keys include stable identity and
//! profile values; the profile epoch fences work from a stale session without changing the
//! persistent namespace when the application restarts.

use crate::plugin_host::grants::{GrantError, PluginIdentity};
use sha2::{Digest, Sha256};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OwnerKind {
    Storage,
    Job,
    Account,
    Secret,
}

impl OwnerKind {
    fn provider_scoped(self) -> bool {
        matches!(self, Self::Account | Self::Secret)
    }
}

/// Opaque owner capability supplied by the authenticated host context to a service adapter.
/// It contains neither a path nor caller-controlled identity fields.
#[derive(Clone, PartialEq, Eq)]
pub struct NamespaceOwner {
    key: [u8; 32],
    profile_epoch: u64,
    kind: OwnerKind,
}

impl NamespaceOwner {
    /// Opaque deterministic owner key for host persistence adapters.
    pub(crate) fn key(&self) -> &[u8; 32] {
        &self.key
    }
}

impl fmt::Debug for NamespaceOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NamespaceOwner")
            .field("key", &"<redacted>")
            .field("profile_epoch", &self.profile_epoch)
            .field("kind", &self.kind)
            .finish()
    }
}

/// An opaque logical secret slot. This value cannot be used as an OS credential alias.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretSlot([u8; 32]);

impl SecretSlot {
    /// Opaque deterministic slot key for host credential adapters.
    pub(crate) fn key(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for SecretSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretSlot(<redacted>)")
    }
}

/// Resolves private-resource ownership using identity authenticated by the host.
#[derive(Clone, Debug)]
pub struct ExternalNamespaceResolver {
    identity: PluginIdentity,
    provider_id: Option<String>,
    profile_epoch: u64,
}

impl ExternalNamespaceResolver {
    /// `identity` and `provider_id` must come from the pinned package registry and active host
    /// session, never from a plugin RPC payload.
    pub(crate) fn new(
        identity: PluginIdentity,
        provider_id: Option<String>,
        profile_epoch: u64,
    ) -> Result<Self, GrantError> {
        identity.validate()?;
        if profile_epoch == 0
            || provider_id.as_ref().is_some_and(|id| {
                id.trim().is_empty() || id.len() > 256 || id.chars().any(char::is_control)
            })
        {
            return Err(GrantError::InvalidIdentity);
        }
        Ok(Self {
            identity,
            provider_id,
            profile_epoch,
        })
    }

    /// Owner for the plugin's private key/value storage, independent of provider sub-identity.
    pub fn storage_owner(&self) -> NamespaceOwner {
        self.owner(OwnerKind::Storage, false)
    }

    /// Owner for jobs submitted by this plugin in the current profile session.
    pub fn job_owner(&self) -> NamespaceOwner {
        self.owner(OwnerKind::Job, false)
    }

    /// Owner for provider-specific account identity and state.
    pub fn account_owner(&self) -> Result<NamespaceOwner, GrantError> {
        if self.provider_id.is_none() {
            return Err(GrantError::InvalidIdentity);
        }
        Ok(self.owner(OwnerKind::Account, true))
    }

    /// Owner for provider-specific secret material.
    pub fn secret_owner(&self) -> Result<NamespaceOwner, GrantError> {
        if self.provider_id.is_none() {
            return Err(GrantError::InvalidIdentity);
        }
        Ok(self.owner(OwnerKind::Secret, true))
    }

    /// Resolve a logical secret key to a dedicated opaque slot. No caller-provided string is
    /// interpreted as a keyring alias or filesystem path.
    pub fn secret_slot(&self, logical_key: &str) -> Result<SecretSlot, GrantError> {
        let owner = self.secret_owner()?;
        if logical_key.is_empty()
            || logical_key.len() > 128
            || !logical_key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            return Err(GrantError::InvalidIdentity);
        }
        let mut hash = Sha256::new();
        hash.update(b"gamenode.third-party.secret-slot.v1\0");
        hash.update(owner.key);
        hash.update((logical_key.len() as u64).to_be_bytes());
        hash.update(logical_key.as_bytes());
        Ok(SecretSlot(hash.finalize().into()))
    }

    /// A service adapter checks the supplied owner before accessing its backing store.
    pub(crate) fn owns(&self, owner: &NamespaceOwner, kind: OwnerKind) -> bool {
        (!kind.provider_scoped() || self.provider_id.is_some())
            && owner.kind == kind
            && owner.profile_epoch == self.profile_epoch
            && owner.key() == self.owner(kind, kind.provider_scoped()).key()
    }

    /// Returns false when a queued operation captured a previous profile epoch.
    pub fn is_current_profile_epoch(&self, captured_epoch: u64) -> bool {
        captured_epoch == self.profile_epoch
    }

    fn owner(&self, kind: OwnerKind, provider_scoped: bool) -> NamespaceOwner {
        let mut hash = Sha256::new();
        hash.update(b"gamenode.third-party.namespace-owner.v1\0");
        hash_part(&mut hash, self.identity.plugin_id.as_bytes());
        hash_part(&mut hash, self.identity.publisher_fingerprint.as_bytes());
        hash_part(&mut hash, self.identity.profile_id.as_bytes());
        hash_part(
            &mut hash,
            match kind {
                OwnerKind::Storage => b"storage",
                OwnerKind::Job => b"job",
                OwnerKind::Account => b"account",
                OwnerKind::Secret => b"secret",
            },
        );
        if provider_scoped {
            if let Some(provider_id) = &self.provider_id {
                hash_part(&mut hash, provider_id.as_bytes());
            }
        }
        NamespaceOwner {
            key: hash.finalize().into(),
            profile_epoch: self.profile_epoch,
            kind,
        }
    }
}

fn hash_part(hash: &mut Sha256, part: &[u8]) {
    hash.update((part.len() as u64).to_be_bytes());
    hash.update(part);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(plugin: &str, publisher: &str, profile: &str) -> PluginIdentity {
        PluginIdentity {
            plugin_id: plugin.into(),
            publisher_fingerprint: publisher.into(),
            profile_id: profile.into(),
        }
    }

    fn resolver(
        plugin: &str,
        publisher: &str,
        profile: &str,
        provider: &str,
        epoch: u64,
    ) -> ExternalNamespaceResolver {
        ExternalNamespaceResolver::new(
            identity(plugin, publisher, profile),
            Some(provider.into()),
            epoch,
        )
        .unwrap()
    }

    #[test]
    fn private_owners_are_separated_by_plugin_publisher_and_profile() {
        let base = resolver("org.example.a", "publisher-a", "default", "provider", 1);
        let other_plugin = resolver("org.example.b", "publisher-a", "default", "provider", 1);
        let other_publisher = resolver("org.example.a", "publisher-b", "default", "provider", 1);
        let other_profile = resolver("org.example.a", "publisher-a", "child", "provider", 1);

        let owner = base.storage_owner();
        assert!(base.owns(&owner, OwnerKind::Storage));
        assert!(!other_plugin.owns(&owner, OwnerKind::Storage));
        assert!(!other_publisher.owns(&owner, OwnerKind::Storage));
        assert!(!other_profile.owns(&owner, OwnerKind::Storage));
        assert!(!base.owns(&base.job_owner(), OwnerKind::Storage));
    }

    #[test]
    fn secret_slots_are_provider_scoped_and_never_official_aliases() {
        let a = resolver("org.example.a", "publisher-a", "default", "provider-a", 1);
        let b = resolver("org.example.a", "publisher-a", "default", "provider-b", 1);
        let slot_a = a.secret_slot("session").unwrap();
        let slot_b = b.secret_slot("session").unwrap();

        assert_ne!(slot_a, slot_b);
        assert_ne!(format!("{slot_a:?}"), "steam");
        assert_ne!(slot_a.key().as_slice(), b"steam");
        assert!(a.secret_slot("../../steam").is_err());
        assert!(
            ExternalNamespaceResolver::new(
                identity("org.example.x", "publisher-x", "default"),
                None,
                1
            )
            .unwrap()
            .secret_slot("session")
            .is_err()
        );
    }

    #[test]
    fn account_identity_is_provider_and_profile_scoped() {
        let a = resolver("org.example.a", "publisher-a", "default", "provider-a", 1);
        let b = resolver("org.example.a", "publisher-a", "default", "provider-b", 1);
        let account = a.account_owner().unwrap();

        assert!(a.owns(&account, OwnerKind::Account));
        assert!(!b.owns(&account, OwnerKind::Account));
        assert!(
            !resolver("org.example.a", "publisher-a", "child", "provider-a", 1)
                .owns(&account, OwnerKind::Account)
        );
    }

    #[test]
    fn profile_epoch_fences_queued_operations_without_changing_durable_owner() {
        let current = resolver("org.example.a", "publisher-a", "default", "provider", 9);
        let stale = resolver("org.example.a", "publisher-a", "default", "provider", 8);
        let captured = current.storage_owner();

        assert!(current.is_current_profile_epoch(9));
        assert!(!current.is_current_profile_epoch(8));
        assert!(!stale.owns(&captured, OwnerKind::Storage));
        assert_eq!(stale.storage_owner().key, captured.key);
    }
}
