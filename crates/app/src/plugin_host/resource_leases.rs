//! Host-owned coordination for shared emulator resources.
//!
//! A runtime launch holds a use lease for every shared save resource it may touch. Backup and
//! restore hold an exclusive lease for the complete operation. Acquiring a set is atomic: either
//! every key is reserved or none is. The plugin protocol never receives these keys or leases.

use super::grants::PluginIdentity;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

const MAX_LEASE_KEYS: usize = 64;

/// An opaque host-derived identity for one reviewed shared save resource.
///
/// The key identifies the host-resolved backing resource. Authorization remains bound to the
/// plugin, profile, epoch and reviewed revision, but those values must not split the lock for
/// the same backing resource. It is never accepted from plugin input or exposed over IPC.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceLeaseKey([u8; 32]);

impl ResourceLeaseKey {
    pub(crate) fn derive(
        identity: &PluginIdentity,
        profile_epoch: u64,
        resource_id: &str,
        resource_revision: u64,
        approved_handle_identity: &[u8],
    ) -> Option<Self> {
        if profile_epoch == 0
            || resource_id.is_empty()
            || resource_id.len() > 256
            || resource_revision == 0
            || approved_handle_identity.is_empty()
        {
            return None;
        }
        let mut digest = Sha256::new();
        digest.update(b"gamenode.shared-save-resource.lease.v1\0");
        // These fields establish that the request was reviewed; they are deliberately not part
        // of the lock identity. All aliases and revisions of a backing resource must serialize.
        let _ = identity;
        hash_part(&mut digest, approved_handle_identity);
        Some(Self(digest.finalize().into()))
    }
}

fn hash_part(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_be_bytes());
    digest.update(bytes);
}

/// A conflict or inability to determine the resource state. Both deny the requested operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ResourceLeaseError {
    #[error("the shared resource is in use")]
    Busy,
    #[error("the shared resource state cannot be determined")]
    Unknown,
}

/// Marker for an RAII lease. Dropping it releases every key in its atomic set.
pub(crate) trait ResourceLease: Send + Sync {}

/// Serializes supervised runtime use against backup and restore.
pub(crate) trait ResourceLeaseCoordinator: Send + Sync {
    /// Atomically reserve all keys for supervised runtime use.
    fn acquire_use(
        &self,
        keys: &[ResourceLeaseKey],
    ) -> Result<Box<dyn ResourceLease>, ResourceLeaseError>;

    /// Atomically reserve all keys exclusively for backup or restore.
    fn acquire_exclusive(
        &self,
        keys: &[ResourceLeaseKey],
    ) -> Result<Box<dyn ResourceLease>, ResourceLeaseError>;
}

#[derive(Clone, Copy, Debug, Default)]
struct State {
    uses: usize,
    exclusive: bool,
}

/// In-process coordinator shared by the launch supervisor and backup host.
#[derive(Default)]
pub(crate) struct ResourceLeases {
    state: Arc<Mutex<BTreeMap<ResourceLeaseKey, State>>>,
}

struct LeaseGuard {
    state: Arc<Mutex<BTreeMap<ResourceLeaseKey, State>>>,
    keys: Vec<ResourceLeaseKey>,
    exclusive: bool,
}

impl ResourceLease for LeaseGuard {}

impl Drop for LeaseGuard {
    fn drop(&mut self) {
        let Ok(mut state) = self.state.lock() else {
            // A poisoned coordinator cannot safely infer that a resource is idle. Keep the
            // reservations in place; subsequent acquisitions also fail closed.
            return;
        };
        for key in &self.keys {
            let remove = if let Some(entry) = state.get_mut(key) {
                if self.exclusive {
                    entry.exclusive = false;
                } else {
                    entry.uses = entry.uses.saturating_sub(1);
                }
                entry.uses == 0 && !entry.exclusive
            } else {
                false
            };
            if remove {
                state.remove(key);
            }
        }
    }
}

impl ResourceLeases {
    fn reserve(
        &self,
        keys: &[ResourceLeaseKey],
        exclusive: bool,
    ) -> Result<Box<dyn ResourceLease>, ResourceLeaseError> {
        if keys.is_empty() || keys.len() > MAX_LEASE_KEYS {
            return Err(ResourceLeaseError::Unknown);
        }
        let unique = keys.iter().copied().collect::<BTreeSet<_>>();
        if unique.len() != keys.len() {
            return Err(ResourceLeaseError::Unknown);
        }
        let mut state = self.state.lock().map_err(|_| ResourceLeaseError::Unknown)?;
        if unique.iter().any(|key| {
            state
                .get(key)
                .is_some_and(|entry| entry.exclusive || (exclusive && entry.uses > 0))
        }) {
            return Err(ResourceLeaseError::Busy);
        }
        for key in &unique {
            let entry = state.entry(*key).or_default();
            if exclusive {
                entry.exclusive = true;
            } else {
                entry.uses += 1;
            }
        }
        Ok(Box::new(LeaseGuard {
            state: Arc::clone(&self.state),
            keys: unique.into_iter().collect(),
            exclusive,
        }))
    }
}

impl ResourceLeaseCoordinator for ResourceLeases {
    fn acquire_use(
        &self,
        keys: &[ResourceLeaseKey],
    ) -> Result<Box<dyn ResourceLease>, ResourceLeaseError> {
        self.reserve(keys, false)
    }

    fn acquire_exclusive(
        &self,
        keys: &[ResourceLeaseKey],
    ) -> Result<Box<dyn ResourceLease>, ResourceLeaseError> {
        self.reserve(keys, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(value: u8) -> ResourceLeaseKey {
        ResourceLeaseKey([value; 32])
    }

    #[test]
    fn use_and_exclusive_leases_exclude_each_other_and_release_on_drop() {
        let leases = ResourceLeases::default();
        let use_lease = leases.acquire_use(&[key(1)]).unwrap();
        assert!(matches!(
            leases.acquire_exclusive(&[key(1)]),
            Err(ResourceLeaseError::Busy)
        ));
        drop(use_lease);

        let exclusive = leases.acquire_exclusive(&[key(1)]).unwrap();
        assert!(matches!(
            leases.acquire_use(&[key(1)]),
            Err(ResourceLeaseError::Busy)
        ));
        drop(exclusive);
        assert!(leases.acquire_use(&[key(1)]).is_ok());
    }

    #[test]
    fn multi_key_acquisition_is_atomic() {
        let leases = ResourceLeases::default();
        let held = leases.acquire_use(&[key(2)]).unwrap();
        assert!(matches!(
            leases.acquire_exclusive(&[key(1), key(2)]),
            Err(ResourceLeaseError::Busy)
        ));
        assert!(leases.acquire_use(&[key(1)]).is_ok());
        drop(held);
    }

    #[test]
    fn concurrent_exclusive_acquisitions_have_one_winner() {
        let leases = Arc::new(ResourceLeases::default());
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let workers = (0..2)
            .map(|_| {
                let leases = Arc::clone(&leases);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let result = leases.acquire_exclusive(&[key(3), key(4)]);
                    barrier.wait();
                    result.is_ok()
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let winners = workers
            .into_iter()
            .map(|worker| worker.join().expect("lease worker did not panic"))
            .filter(|won| *won)
            .count();
        assert_eq!(winners, 1);
    }

    #[test]
    fn resource_identity_remains_stable_across_authorization_changes() {
        let identity = PluginIdentity {
            plugin_id: "org.example.emulator".into(),
            publisher_fingerprint: "ab".repeat(32),
            profile_id: "account-a".into(),
        };
        let derive = |epoch, id, revision, handle| {
            ResourceLeaseKey::derive(&identity, epoch, id, revision, handle).unwrap()
        };
        let baseline = derive(1, "card-1", 4, b"approved-folder-a");
        assert_eq!(baseline, derive(2, "card-1", 4, b"approved-folder-a"));
        assert_eq!(baseline, derive(1, "card-2", 4, b"approved-folder-a"));
        assert_eq!(baseline, derive(1, "card-1", 5, b"approved-folder-a"));
        assert_ne!(baseline, derive(1, "card-1", 4, b"approved-folder-b"));
        let mut other_identity = identity;
        other_identity.profile_id = "account-b".into();
        assert_eq!(
            baseline,
            ResourceLeaseKey::derive(&other_identity, 1, "card-1", 4, b"approved-folder-a")
                .unwrap()
        );
        let leases = ResourceLeases::default();
        let _use_lease = leases.acquire_use(&[baseline]).unwrap();
        let after_review =
            ResourceLeaseKey::derive(&other_identity, 2, "renamed", 7, b"approved-folder-a")
                .unwrap();
        assert!(matches!(
            leases.acquire_exclusive(&[after_review]),
            Err(ResourceLeaseError::Busy)
        ));
    }
}
