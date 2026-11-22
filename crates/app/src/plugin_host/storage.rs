//! Private key/value storage of third-party plugins, served through `ResourceHost.Storage*`.
//!
//! A plugin's storage is addressed only by the host-authenticated namespace
//! ([`ExternalNamespaceResolver::storage_owner`]): plugin id, pinned publisher fingerprint and
//! account profile. The request carries a key and nothing else that could name an owner, a path
//! or another namespace. Each namespace has two layers that share one key space:
//!
//! * **durable** (`PERSISTENT`) values live in the plugin's own SQLite file under the host's
//!   `plugin-data` folder ([`gamenode_platform::plugin_storage`]); they need `storage.persist`;
//! * **session-only** values stay in the host's memory and are discarded when the plugin's last
//!   open session ends or the profile changes; they need only `storage.write`.
//!
//! A key lives in exactly one layer: writing it with the other durability moves it, and a
//! denied durable write never falls back to a volatile one. Revisions come from one counter per
//! namespace, so a deleted and recreated key never repeats a revision. Every operation on a
//! namespace is serialized, and all the work runs on a blocking worker holding that
//! serialization, so cancelling an RPC cannot interleave or abandon a half-done write.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use gamenode_platform::plugin_storage::{
    self as durable, DurableStore, FileApproval, IntentApproval, PluginStorageError, ProfileSaved,
    PutResult, SaveProfileRow,
};
use tonic::Status;

use super::library::CommitGuard;
use super::namespace::ExternalNamespaceResolver;
use super::provider::ResourceCaller;

pub use durable::{
    ENTRY_OVERHEAD_BYTES, MAX_KEY_BYTES, MAX_TTL_SECONDS, MAX_VALUE_BYTES, QUOTA_BYTES,
};

type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

fn system_clock() -> Clock {
    Arc::new(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs() as i64)
    })
}

/// How long a stored value lasts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Durability {
    /// Until the plugin's last open session ends.
    SessionOnly,
    /// Until deleted or expired, across restarts.
    Persistent,
}

/// A value as read back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub value: Vec<u8>,
    pub revision: u64,
    pub durability: Durability,
    /// Unix seconds, `None` when the value never expires.
    pub expires_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PutOutcome {
    Applied {
        revision: u64,
        expires_at: Option<i64>,
        /// Logical bytes the written layer holds now.
        usage: u64,
    },
    Conflict {
        current: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeleteOutcome {
    Deleted {
        revision: u64,
    },
    /// There was nothing under the key.
    Absent,
    Conflict {
        current: u64,
    },
}

struct Entry {
    value: Vec<u8>,
    revision: u64,
    expires_at: Option<i64>,
}

#[derive(Default)]
struct Inner {
    /// The highest revision handed out; seeded from the durable store when it is first opened.
    counter: u64,
    /// The open plugin sessions that used this namespace.
    sessions: BTreeSet<String>,
    session: BTreeMap<String, Entry>,
    durable: Option<DurableStore>,
}

struct Slot {
    epoch: u64,
    inner: Arc<tokio::sync::Mutex<Inner>>,
}

/// The host's private plugin storage: where durable data lives and which namespaces are loaded.
pub struct PluginStorage {
    /// `plugin-data`; `None` when the host has no data folder, which refuses durable writes.
    root: Option<PathBuf>,
    clock: Clock,
    namespaces: Mutex<HashMap<[u8; 32], Slot>>,
}

impl Default for PluginStorage {
    fn default() -> Self {
        Self {
            root: None,
            clock: system_clock(),
            namespaces: Mutex::default(),
        }
    }
}

impl PluginStorage {
    /// Synchronous access for an already-running blocking backup worker. Never call on an async
    /// runtime/UI thread. Only short approval/revision checks run while the namespace is locked.
    pub(crate) fn on_backup_worker<T>(
        &self,
        caller: &ResourceCaller,
        work: impl FnOnce(&mut Namespace<'_>) -> Result<T, Status>,
    ) -> Result<T, Status> {
        let resolver = caller
            .namespace
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("account namespace required"))?;
        let identity = resolver.identity();
        let dir = self.root.as_ref().map(|root| {
            durable::namespace_dir(
                root,
                &identity.publisher_fingerprint,
                &identity.plugin_id,
                &identity.profile_id,
            )
        });
        let inner = self.slot(*resolver.storage_owner().key(), caller.profile_epoch)?;
        let mut guard = inner
            .try_lock()
            .map_err(|_| Status::unavailable("backup scope is being updated"))?;
        work(&mut Namespace {
            inner: &mut guard,
            dir: dir.as_deref(),
            now: (self.clock)(),
        })
    }

    /// Durable data goes below `root` (the `plugin-data` folder).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: Some(root.into()),
            ..Self::default()
        }
    }

    /// Replaces the clock (Unix seconds), for tests of expiry.
    #[cfg(test)]
    pub(crate) fn with_clock(mut self, clock: impl Fn() -> i64 + Send + Sync + 'static) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    fn slot(&self, owner: [u8; 32], epoch: u64) -> Result<Arc<tokio::sync::Mutex<Inner>>, Status> {
        let mut namespaces = self.namespaces.lock().unwrap_or_else(|p| p.into_inner());
        // A new profile epoch drops every namespace of the previous one, session data included.
        namespaces.retain(|_, slot| slot.epoch >= epoch);
        let slot = namespaces.entry(owner).or_insert_with(|| Slot {
            epoch,
            inner: Arc::default(),
        });
        if slot.epoch != epoch {
            return Err(Status::failed_precondition("resource context is stale"));
        }
        Ok(slot.inner.clone())
    }

    /// Runs `work` on a blocking worker with exclusive use of a plugin's namespace on behalf of
    /// the host itself (the settings list of remembered confirmations), not of a plugin session:
    /// it does not count as a session using the namespace.
    pub(crate) async fn run_for_host<T: Send + 'static>(
        &self,
        resolver: &ExternalNamespaceResolver,
        profile_epoch: u64,
        work: impl FnOnce(&mut Namespace<'_>) -> Result<T, Status> + Send + 'static,
    ) -> Result<T, Status> {
        let owner = resolver.storage_owner();
        let identity = resolver.identity();
        let dir = self.root.as_ref().map(|root| {
            durable::namespace_dir(
                root,
                &identity.publisher_fingerprint,
                &identity.plugin_id,
                &identity.profile_id,
            )
        });
        let inner = self.slot(*owner.key(), profile_epoch)?;
        let mut guard = inner.lock_owned().await;
        let clock = self.clock.clone();
        tokio::task::spawn_blocking(move || {
            let mut namespace = Namespace {
                inner: &mut guard,
                dir: dir.as_deref(),
                now: clock(),
            };
            work(&mut namespace)
        })
        .await
        .map_err(|_| Status::unavailable("plugin storage is unavailable"))?
    }

    /// Host-only user protection for one durable private key. A plugin RPC has no field that can
    /// reach this decision, and it is never exposed through `ResourceHost`.
    pub async fn set_persistence_protected(
        &self,
        resolver: &ExternalNamespaceResolver,
        profile_epoch: u64,
        key: impl Into<String>,
        protected: bool,
    ) -> Result<bool, Status> {
        let key = key.into();
        if key.is_empty() || key.len() > MAX_KEY_BYTES || key.chars().any(char::is_control) {
            return Err(Status::invalid_argument("persistence key is invalid"));
        }
        self.run_for_host(resolver, profile_epoch, move |namespace| {
            namespace.set_persistence_protected(&key, protected)
        })
        .await
    }

    /// Host-only cleanup of one plugin's durable private store: expired and policy-exceeding
    /// records are removed, while user-protected records remain. Never called from plugin IPC.
    pub async fn cleanup_persistence(
        &self,
        resolver: &ExternalNamespaceResolver,
        profile_epoch: u64,
    ) -> Result<durable::CleanupOutcome, Status> {
        self.run_for_host(resolver, profile_epoch, |namespace| {
            namespace.cleanup_persistence().map(|(outcome, _)| outcome)
        })
        .await
    }

    /// The directory of one namespace below `plugin-data`, if this host has a data folder.
    fn namespace_dir(&self, resolver: &ExternalNamespaceResolver) -> Option<PathBuf> {
        let identity = resolver.identity();
        self.root.as_ref().map(|root| {
            durable::namespace_dir(
                root,
                &identity.publisher_fingerprint,
                &identity.plugin_id,
                &identity.profile_id,
            )
        })
    }

    /// Drops a cached namespace slot after its owner's data was archived or removed, so a later
    /// install cannot keep reading through an open connection.
    fn forget_slot(&self, owner: [u8; 32], epoch: u64, slot: &Arc<tokio::sync::Mutex<Inner>>) {
        let mut namespaces = self.namespaces.lock().unwrap_or_else(|p| p.into_inner());
        if namespaces.get(&owner).is_some_and(|candidate| {
            candidate.epoch == epoch && Arc::ptr_eq(&candidate.inner, slot)
        }) {
            namespaces.remove(&owner);
        }
    }

    /// Host-only: makes every live private entry of one plugin/profile namespace inert. The
    /// namespace is evicted afterwards, so a reinstall opens the archived rows and needs the
    /// separate reviewed reuse.
    pub async fn archive_persistence(
        &self,
        resolver: &ExternalNamespaceResolver,
        profile_epoch: u64,
    ) -> Result<durable::PrivateArchiveOutcome, Status> {
        let Some(dir) = self.namespace_dir(resolver) else {
            return Ok(durable::PrivateArchiveOutcome::default());
        };
        let owner = *resolver.storage_owner().key();
        let slot = self.slot(owner, profile_epoch)?;
        let mut guard = slot.clone().lock_owned().await;
        let now = (self.clock)();
        let outcome = tokio::task::spawn_blocking(move || {
            let mut namespace = Namespace {
                inner: &mut guard,
                dir: Some(&dir),
                now,
            };
            namespace.archive_persistence()
        })
        .await
        .map_err(|_| Status::unavailable("plugin storage is unavailable"))??;
        self.forget_slot(owner, profile_epoch, &slot);
        Ok(outcome)
    }

    /// Host-only: restores one namespace's archived private entries after an explicit review.
    /// The caller has already checked the confirmation and the active profile.
    pub async fn reuse_archived_persistence(
        &self,
        resolver: &ExternalNamespaceResolver,
        profile_epoch: u64,
    ) -> Result<durable::PrivateArchiveOutcome, Status> {
        self.run_for_host(resolver, profile_epoch, |namespace| {
            namespace.reuse_archived_persistence()
        })
        .await
    }

    /// Host-only: reclaims one namespace's archived, unprotected entries in a reviewed cleanup.
    pub async fn reclaim_archived_persistence(
        &self,
        resolver: &ExternalNamespaceResolver,
        profile_epoch: u64,
    ) -> Result<durable::PrivateArchiveOutcome, Status> {
        self.run_for_host(resolver, profile_epoch, |namespace| {
            namespace.reclaim_archived_persistence()
        })
        .await
    }

    /// Host-only preview of a reviewed publisher migration: what the old identity's private
    /// namespace could hand to its re-trusted successor. `None` means this host has no private
    /// data folder. Nothing is created or changed.
    pub async fn publisher_migration_preview(
        &self,
        source: &ExternalNamespaceResolver,
        target: &ExternalNamespaceResolver,
    ) -> Result<Option<durable::PrivateMigrationPreview>, Status> {
        let Some(root) = self.root.clone() else {
            return Ok(None);
        };
        let source = source.identity().clone();
        let target = target.identity().clone();
        let now = (self.clock)();
        tokio::task::spawn_blocking(move || {
            durable::migration_preview(
                &root,
                &source.publisher_fingerprint,
                &source.plugin_id,
                &source.profile_id,
                &target.publisher_fingerprint,
                &target.plugin_id,
                &target.profile_id,
                now,
            )
        })
        .await
        .map_err(|_| Status::unavailable("plugin storage is unavailable"))?
        .map(Some)
        .map_err(storage_status)
    }

    /// Host-only copy of one old publisher identity's private entries to its re-trusted successor.
    /// The source keeps every entry, conflicts and expired rows are reported instead of copied, and
    /// the target's cached namespace is dropped so a live session reopens the migrated file.
    pub async fn migrate_publisher_data(
        &self,
        source: &ExternalNamespaceResolver,
        target: &ExternalNamespaceResolver,
    ) -> Result<durable::PrivateMigrationOutcome, Status> {
        let Some(root) = self.root.clone() else {
            return Err(Status::failed_precondition(
                "private plugin data is unavailable on this host",
            ));
        };
        let source = source.identity().clone();
        let target_identity = target.identity().clone();
        let target_owner = *target.storage_owner().key();
        let now = (self.clock)();
        let outcome = tokio::task::spawn_blocking(move || {
            durable::migrate_namespace(
                &root,
                &source.publisher_fingerprint,
                &source.plugin_id,
                &source.profile_id,
                &target_identity.publisher_fingerprint,
                &target_identity.plugin_id,
                &target_identity.profile_id,
                now,
            )
        })
        .await
        .map_err(|_| Status::unavailable("plugin storage is unavailable"))?
        .map_err(storage_status)?;
        self.namespaces
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&target_owner);
        Ok(outcome)
    }

    /// Host-only rollback of one publisher migration's private copies. Only entries tagged with the
    /// old fingerprint are removed; the old identity's namespace and later plugin writes stay.
    pub async fn revert_publisher_migration(
        &self,
        target: &ExternalNamespaceResolver,
        source_publisher_fingerprint: String,
    ) -> Result<Option<durable::PrivateMigrationRevert>, Status> {
        let Some(root) = self.root.clone() else {
            return Ok(None);
        };
        let target_identity = target.identity().clone();
        let target_owner = *target.storage_owner().key();
        let outcome = tokio::task::spawn_blocking(move || {
            durable::revert_migrated_namespace(
                &root,
                &target_identity.publisher_fingerprint,
                &target_identity.plugin_id,
                &target_identity.profile_id,
                &source_publisher_fingerprint,
            )
        })
        .await
        .map_err(|_| Status::unavailable("plugin storage is unavailable"))?
        .map_err(storage_status)?;
        self.namespaces
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&target_owner);
        Ok(outcome)
    }

    /// Host-only: deletes one plugin/profile private namespace directory and reports the logical
    /// bytes it held. Another profile, plugin, publisher, game file or backup archive is never
    /// touched. The namespace slot is evicted and its cached connection closed first.
    pub async fn remove_persistence_namespace(
        &self,
        resolver: &ExternalNamespaceResolver,
        profile_epoch: u64,
    ) -> Result<durable::PrivateArchiveOutcome, Status> {
        let Some(dir) = self.namespace_dir(resolver) else {
            return Ok(durable::PrivateArchiveOutcome::default());
        };
        let owner = *resolver.storage_owner().key();
        let slot = self.slot(owner, profile_epoch)?;
        let mut guard = slot.clone().lock_owned().await;
        let now = (self.clock)();
        let outcome = tokio::task::spawn_blocking(
            move || -> Result<durable::PrivateArchiveOutcome, Status> {
                let mut namespace = Namespace {
                    inner: &mut guard,
                    dir: Some(&dir),
                    now,
                };
                let usage = namespace.store_usage()?;
                // Closing the connection before the unlink keeps no stale handle to the deleted
                // file.
                namespace.inner.durable = None;
                namespace.inner.session.clear();
                durable::remove_namespace_at(&dir).map_err(|error| {
                    tracing::warn!(%error, "private plugin namespace could not be removed");
                    Status::unavailable("private plugin data could not be removed")
                })?;
                Ok(durable::PrivateArchiveOutcome {
                    records: usage
                        .categories
                        .iter()
                        .map(|category| category.records)
                        .sum::<u64>()
                        .saturating_add(usage.archived_records),
                    logical_bytes: usage
                        .logical_bytes
                        .saturating_add(usage.archived_logical_bytes),
                })
            },
        )
        .await
        .map_err(|_| Status::unavailable("plugin storage is unavailable"))??;
        self.forget_slot(owner, profile_epoch, &slot);
        Ok(outcome)
    }

    /// Host-only: drops a cached namespace without touching its files. Used after a bundle
    /// uninstall archived or deleted the namespace directly, so no open connection outlives it.
    pub async fn forget_persistence_namespace(
        &self,
        resolver: &ExternalNamespaceResolver,
        profile_epoch: u64,
    ) -> Result<(), Status> {
        let owner = *resolver.storage_owner().key();
        // A namespace that was never opened has nothing to drop.
        let slot = {
            let namespaces = self.namespaces.lock().unwrap_or_else(|p| p.into_inner());
            match namespaces.get(&owner) {
                Some(slot) if slot.epoch == profile_epoch => slot.inner.clone(),
                Some(_) => return Err(Status::failed_precondition("resource context is stale")),
                None => return Ok(()),
            }
        };
        let mut guard = slot.clone().lock_owned().await;
        let dir = self.namespace_dir(resolver);
        tokio::task::spawn_blocking(move || {
            let namespace = Namespace {
                inner: &mut guard,
                dir: dir.as_deref(),
                now: 0,
            };
            namespace.inner.durable = None;
            namespace.inner.session.clear();
        })
        .await
        .map_err(|_| Status::unavailable("plugin storage is unavailable"))?;
        self.forget_slot(owner, profile_epoch, &slot);
        Ok(())
    }

    /// Host-only usage of one plugin/profile private namespace. `None` when the namespace never
    /// wrote anything durably. Never creates a store and never returns a value or a path.
    pub async fn private_usage(
        &self,
        resolver: &ExternalNamespaceResolver,
        profile_epoch: u64,
    ) -> Result<Option<durable::StoreUsage>, Status> {
        self.run_for_host(resolver, profile_epoch, |namespace| {
            namespace.private_usage()
        })
        .await
    }

    /// Runs `work` on a blocking worker with exclusive use of the caller's namespace.
    /// `session_open` says whether a plugin session id is still open.
    pub(crate) async fn run<T: Send + 'static>(
        &self,
        caller: &ResourceCaller,
        session_open: impl Fn(&str) -> bool + Send + 'static,
        work: impl FnOnce(&mut Namespace<'_>) -> Result<T, Status> + Send + 'static,
    ) -> Result<T, Status> {
        let resolver: &ExternalNamespaceResolver = caller.namespace.as_ref().ok_or_else(|| {
            Status::failed_precondition("official providers use trusted host adapters")
        })?;
        let owner = resolver.storage_owner();
        let identity = resolver.identity();
        let dir = self.root.as_ref().map(|root| {
            durable::namespace_dir(
                root,
                &identity.publisher_fingerprint,
                &identity.plugin_id,
                &identity.profile_id,
            )
        });
        let inner = self.slot(*owner.key(), caller.profile_epoch)?;
        let mut guard = inner.lock_owned().await;
        let clock = self.clock.clone();
        let session_id = caller.session_id.clone();
        tokio::task::spawn_blocking(move || {
            guard.sessions.retain(|id| session_open(id));
            if guard.sessions.is_empty() {
                guard.session.clear();
            }
            guard.sessions.insert(session_id);
            let mut namespace = Namespace {
                inner: &mut guard,
                dir: dir.as_deref(),
                now: clock(),
            };
            work(&mut namespace)
        })
        .await
        .map_err(|_| Status::unavailable("plugin storage is unavailable"))?
    }
}

fn unavailable(error: PluginStorageError) -> Status {
    // The error text never contains a path; the plugin gets even less.
    tracing::warn!(%error, "plugin storage failed");
    Status::unavailable("plugin storage is unavailable")
}

/// A plugin mutation that names user-protected data fails explicitly; no other storage detail
/// leaves the host.
fn storage_status(error: PluginStorageError) -> Status {
    match error {
        PluginStorageError::Protected => {
            Status::failed_precondition("the entry is protected by the user")
        }
        PluginStorageError::Archived => Status::failed_precondition(
            "archived plugin data requires a reviewed reuse before it can be read or replaced",
        ),
        PluginStorageError::InvalidMigration => {
            Status::failed_precondition("a publisher migration needs two different identities")
        }
        other => unavailable(other),
    }
}

fn session_bytes(entries: &BTreeMap<String, Entry>) -> u64 {
    entries
        .iter()
        .map(|(key, entry)| durable::charge(key, entry.value.len()))
        .sum()
}

fn exhausted(usage: u64) -> Status {
    Status::resource_exhausted(format!(
        "the plugin storage quota of {QUOTA_BYTES} bytes would be exceeded ({usage} in use); \
         nothing was changed"
    ))
}

/// One caller's namespace, exclusively held on a blocking worker.
pub(crate) struct Namespace<'a> {
    inner: &'a mut Inner,
    dir: Option<&'a Path>,
    now: i64,
}

impl Namespace<'_> {
    /// Drops the session-only values that expired.
    fn sweep(&mut self) {
        let now = self.now;
        self.inner
            .session
            .retain(|_, entry| entry.expires_at.is_none_or(|at| at > now));
    }

    /// The durable store: opened if it exists, or created when `create` is set. `None` when the
    /// namespace never stored anything durably.
    fn durable(&mut self, create: bool) -> Result<Option<&mut DurableStore>, Status> {
        if self.inner.durable.is_none() {
            let Some(dir) = self.dir else {
                return if create {
                    Err(Status::unavailable(
                        "durable plugin storage is not available",
                    ))
                } else {
                    Ok(None)
                };
            };
            let opened = if create {
                Some(DurableStore::open(dir).map_err(unavailable)?)
            } else {
                DurableStore::open_existing(dir).map_err(unavailable)?
            };
            if let Some(store) = &opened {
                let high_water = store.high_water().map_err(unavailable)?;
                self.inner.counter = self.inner.counter.max(high_water);
            }
            self.inner.durable = opened;
        }
        Ok(self.inner.durable.as_mut())
    }

    /// The value under `key`, in whichever layer holds it.
    pub fn get(&mut self, key: &str) -> Result<Option<Found>, Status> {
        self.sweep();
        if let Some(entry) = self.inner.session.get(key) {
            return Ok(Some(Found {
                value: entry.value.clone(),
                revision: entry.revision,
                durability: Durability::SessionOnly,
                expires_at: entry.expires_at,
            }));
        }
        let now = self.now;
        let Some(store) = self.durable(false)? else {
            return Ok(None);
        };
        Ok(store
            .get(key, now)
            .map_err(unavailable)?
            .map(|stored| Found {
                value: stored.value,
                revision: stored.revision,
                durability: Durability::Persistent,
                expires_at: stored.expires_at,
            }))
    }

    /// The logical bytes held in one layer.
    pub fn usage(&mut self, durability: Durability) -> Result<u64, Status> {
        match durability {
            Durability::SessionOnly => {
                self.sweep();
                Ok(session_bytes(&self.inner.session))
            }
            Durability::Persistent => {
                let now = self.now;
                match self.durable(false)? {
                    Some(store) => store.usage(now).map_err(unavailable),
                    None => Ok(0),
                }
            }
        }
    }

    /// Durable logical entries exposed to Persistence administration. Values and paths never
    /// leave the store; session-only values are deliberately excluded.
    pub fn persistence_entries(&mut self) -> Result<Vec<durable::PersistenceEntry>, Status> {
        let now = self.now;
        match self.durable(false)? {
            Some(store) => store.persistence_entries(now).map_err(unavailable),
            None => Ok(Vec::new()),
        }
    }

    /// Per-category logical accounting and the namespace-wide physical estimate.
    pub fn store_usage(&mut self) -> Result<durable::StoreUsage, Status> {
        let now = self.now;
        match self.durable(false)? {
            Some(store) => store.store_usage(now).map_err(unavailable),
            None => Ok(durable::StoreUsage {
                categories: vec![durable::CategoryUsage {
                    category: "private".to_owned(),
                    records: 0,
                    logical_bytes: 0,
                    protected_records: 0,
                }],
                logical_bytes: 0,
                physical_bytes_estimate: 0,
                archived_records: 0,
                archived_logical_bytes: 0,
                oldest_used_at: None,
                retention: None,
            }),
        }
    }

    /// Host-only protection decision for one durable private key. No plugin RPC reaches it.
    pub fn set_persistence_protected(
        &mut self,
        key: &str,
        protected: bool,
    ) -> Result<bool, Status> {
        match self.durable(false)? {
            Some(store) => store
                .set_user_protected(key, protected)
                .map_err(unavailable),
            None => Ok(false),
        }
    }

    /// Host-only cleanup that keeps user-protected records and advances the namespace revision so
    /// stale cursors cannot resume across the change.
    pub fn cleanup_persistence(&mut self) -> Result<(durable::CleanupOutcome, u64), Status> {
        if self.durable(false)?.is_none() {
            return Ok((durable::CleanupOutcome::default(), self.inner.counter));
        }
        let revision = self.inner.counter.saturating_add(1);
        let now = self.now;
        let store = self
            .durable(false)?
            .ok_or_else(|| Status::unavailable("plugin storage is unavailable"))?;
        let outcome = store.cleanup(now).map_err(unavailable)?;
        self.inner.counter = revision;
        Ok((outcome, revision))
    }

    /// Host-only uninstall Keep for this namespace. Advances the revision so cursors are fenced.
    pub fn archive_persistence(&mut self) -> Result<durable::PrivateArchiveOutcome, Status> {
        if self.durable(false)?.is_none() {
            return Ok(durable::PrivateArchiveOutcome::default());
        }
        let revision = self.inner.counter.saturating_add(1);
        let store = self
            .durable(false)?
            .ok_or_else(|| Status::unavailable("plugin storage is unavailable"))?;
        let outcome = store.archive_entries(revision).map_err(unavailable)?;
        self.inner.counter = revision;
        Ok(outcome)
    }

    /// Host-only reuse of this namespace's archived entries. Advances the revision.
    pub fn reuse_archived_persistence(&mut self) -> Result<durable::PrivateArchiveOutcome, Status> {
        if self.durable(false)?.is_none() {
            return Ok(durable::PrivateArchiveOutcome::default());
        }
        let revision = self.inner.counter.saturating_add(1);
        let now = self.now;
        let store = self
            .durable(false)?
            .ok_or_else(|| Status::unavailable("plugin storage is unavailable"))?;
        let outcome = store
            .reuse_archived_entries(now, revision)
            .map_err(unavailable)?;
        self.inner.counter = revision;
        Ok(outcome)
    }

    /// Host-only usage of this namespace, including archived rows. Never creates a store.
    pub fn private_usage(&mut self) -> Result<Option<durable::StoreUsage>, Status> {
        let now = self.now;
        match self.durable(false)? {
            Some(store) => store.store_usage(now).map(Some).map_err(unavailable),
            None => Ok(None),
        }
    }

    /// Host-only reviewed reclaim of archived, unprotected entries. Advances the revision.
    pub fn reclaim_archived_persistence(
        &mut self,
    ) -> Result<durable::PrivateArchiveOutcome, Status> {
        if self.durable(false)?.is_none() {
            return Ok(durable::PrivateArchiveOutcome::default());
        }
        let revision = self.inner.counter.saturating_add(1);
        let store = self
            .durable(false)?
            .ok_or_else(|| Status::unavailable("plugin storage is unavailable"))?;
        let outcome = store.reclaim_archived(revision).map_err(unavailable)?;
        self.inner.counter = revision;
        Ok(outcome)
    }

    /// Deletes an atomic set of durable keys and advances the namespace revision once.
    pub fn delete_persistence_entries(
        &mut self,
        keys: &[String],
        guard: &CommitGuard,
    ) -> Result<(u64, u64), Status> {
        let entries = self.persistence_entries()?;
        let wanted: std::collections::HashSet<&str> = keys.iter().map(String::as_str).collect();
        if !wanted
            .iter()
            .all(|key| entries.iter().any(|entry| entry.key == **key))
        {
            return Err(Status::not_found("persistence entry not found"));
        }
        if wanted.is_empty() {
            return Ok((0, self.inner.counter));
        }
        guard()?;
        let revision = self.inner.counter.saturating_add(1);
        let now = self.now;
        let store = self
            .durable(false)?
            .ok_or_else(|| Status::not_found("persistence entry not found"))?;
        let reclaimed = store
            .delete_persistence_entries(keys, now, revision)
            .map_err(storage_status)?;
        self.inner.counter = revision;
        Ok((reclaimed, revision))
    }

    pub fn set_retention_policy(
        &mut self,
        category: &str,
        policy: durable::RetentionPolicy,
        guard: &CommitGuard,
    ) -> Result<(u64, u64), Status> {
        guard()?;
        let revision = self.inner.counter.saturating_add(1);
        let now = self.now;
        let store = self
            .durable(true)?
            .ok_or_else(|| Status::unavailable("plugin storage unavailable"))?;
        let reclaimed = store
            .set_retention_policy(category, policy, now, revision)
            .map_err(unavailable)?;
        self.inner.counter = revision;
        Ok((reclaimed, revision))
    }

    /// Stores `value` under `key` when `expected` is the key's current revision (0 for a key
    /// that does not exist). `guard` is checked right before the first change. Over the quota
    /// is `RESOURCE_EXHAUSTED` and changes nothing.
    pub fn put(
        &mut self,
        key: &str,
        value: Vec<u8>,
        durability: Durability,
        ttl_seconds: u64,
        expected: u64,
        guard: &CommitGuard,
    ) -> Result<PutOutcome, Status> {
        let current = self.get(key)?;
        let current_revision = current.as_ref().map_or(0, |found| found.revision);
        if current_revision != expected {
            return Ok(PutOutcome::Conflict {
                current: current_revision,
            });
        }
        let revision = self.inner.counter + 1;
        let expires_at = (ttl_seconds != 0).then(|| self.now.saturating_add(ttl_seconds as i64));
        let now = self.now;
        let usage = match durability {
            Durability::Persistent => {
                guard()?;
                let store = self
                    .durable(true)?
                    .ok_or_else(|| Status::unavailable("plugin storage is unavailable"))?;
                match store
                    .put(key, &value, revision, expires_at, now, QUOTA_BYTES)
                    .map_err(storage_status)?
                {
                    PutResult::OverQuota { usage } => return Err(exhausted(usage)),
                    PutResult::Stored { usage } => {
                        // Now durable: the volatile copy, if any, is the one to go.
                        self.inner.session.remove(key);
                        usage
                    }
                }
            }
            Durability::SessionOnly => {
                let others = session_bytes(&self.inner.session)
                    - self
                        .inner
                        .session
                        .get(key)
                        .map_or(0, |entry| durable::charge(key, entry.value.len()));
                let usage = others + durable::charge(key, value.len());
                if usage > QUOTA_BYTES {
                    return Err(exhausted(others));
                }
                guard()?;
                if current
                    .as_ref()
                    .is_some_and(|found| found.durability == Durability::Persistent)
                    && let Some(store) = self.durable(false)?
                {
                    store.remove(key).map_err(storage_status)?;
                }
                self.inner.session.insert(
                    key.to_owned(),
                    Entry {
                        value,
                        revision,
                        expires_at,
                    },
                );
                usage
            }
        };
        self.inner.counter = revision;
        Ok(PutOutcome::Applied {
            revision,
            expires_at,
            usage,
        })
    }

    /// Deletes `key` when `expected` is its current revision. A key that is not there is
    /// `Absent`, whatever `expected` is: the desired state already holds.
    pub fn delete(
        &mut self,
        key: &str,
        expected: u64,
        guard: &CommitGuard,
    ) -> Result<DeleteOutcome, Status> {
        let Some(current) = self.get(key)? else {
            return Ok(DeleteOutcome::Absent);
        };
        if current.revision != expected {
            return Ok(DeleteOutcome::Conflict {
                current: current.revision,
            });
        }
        guard()?;
        match current.durability {
            Durability::SessionOnly => {
                self.inner.session.remove(key);
            }
            Durability::Persistent => {
                if let Some(store) = self.durable(false)? {
                    store.remove(key).map_err(storage_status)?;
                }
            }
        }
        Ok(DeleteOutcome::Deleted {
            revision: current.revision,
        })
    }
}

/// How writing or removing a save profile ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileWrite {
    /// Written or removed; `revision` is the revision it now has (or the removal's).
    Applied { revision: u64 },
    /// The profile is not at the revision the caller expected.
    Conflict { current: u64 },
    /// Another profile of the installation already has this name.
    NameTaken,
    /// There is no profile with this id.
    NotFound,
}

/// Save profiles live in the same per-namespace store, with revisions from the same counter.
impl Namespace<'_> {
    /// Every profile of this plugin, ordered by installation and id.
    pub fn save_profiles(&mut self) -> Result<Vec<SaveProfileRow>, Status> {
        match self.durable(false)? {
            Some(store) => store.save_profile_rows().map_err(unavailable),
            None => Ok(Vec::new()),
        }
    }

    /// The profile with this id.
    pub fn save_profile(&mut self, id: &str) -> Result<Option<SaveProfileRow>, Status> {
        match self.durable(false)? {
            Some(store) => store.save_profile_row(id).map_err(unavailable),
            None => Ok(None),
        }
    }

    /// The highest revision this namespace ever handed out.
    pub fn revision(&mut self) -> Result<u64, Status> {
        self.durable(false)?;
        Ok(self.inner.counter)
    }

    /// Creates (`expected_revision` 0, `row.id` new) or replaces (`expected_revision` is the
    /// profile's current revision) a profile, giving it the namespace's next revision. `guard` is
    /// checked right before the change.
    pub fn put_save_profile(
        &mut self,
        mut row: SaveProfileRow,
        expected_revision: u64,
        guard: &CommitGuard,
    ) -> Result<ProfileWrite, Status> {
        guard()?;
        // Opening the store first seeds the counter from the highest revision it ever recorded.
        self.durable(true)?;
        let revision = self.inner.counter + 1;
        let store = self
            .durable(true)?
            .ok_or_else(|| Status::unavailable("plugin storage is unavailable"))?;
        match (
            store.save_profile_row(&row.id).map_err(unavailable)?,
            expected_revision,
        ) {
            (None, 0) => {}
            (None, _) => return Ok(ProfileWrite::NotFound),
            (Some(existing), expected) if existing.revision != expected => {
                return Ok(ProfileWrite::Conflict {
                    current: existing.revision,
                });
            }
            (Some(existing), _) => row.created_at = existing.created_at,
        }
        row.revision = revision;
        match store.save_profile(&row).map_err(unavailable)? {
            ProfileSaved::NameTaken => Ok(ProfileWrite::NameTaken),
            ProfileSaved::Saved => {
                self.inner.counter = revision;
                Ok(ProfileWrite::Applied { revision })
            }
        }
    }

    /// Removes a profile if it is at `expected_revision`. The removal takes a revision of its
    /// own, so a profile created afterwards never repeats one.
    pub fn remove_save_profile(
        &mut self,
        id: &str,
        expected_revision: u64,
        guard: &CommitGuard,
    ) -> Result<ProfileWrite, Status> {
        guard()?;
        self.durable(false)?;
        let revision = self.inner.counter + 1;
        let Some(store) = self.durable(false)? else {
            return Ok(ProfileWrite::NotFound);
        };
        let Some(existing) = store.save_profile_row(id).map_err(unavailable)? else {
            return Ok(ProfileWrite::NotFound);
        };
        if existing.revision != expected_revision {
            return Ok(ProfileWrite::Conflict {
                current: existing.revision,
            });
        }
        store
            .delete_save_profile(id, revision)
            .map_err(unavailable)?;
        self.inner.counter = revision;
        Ok(ProfileWrite::Applied { revision })
    }

    /// The persisted user decision for one shared-resource declaration, if any.
    pub fn shared_resource_consent(
        &mut self,
        profile_id: &str,
        resource_id: &str,
    ) -> Result<Option<durable::SharedResourceConsentRow>, Status> {
        match self.durable(false)? {
            Some(store) => store
                .shared_resource_consent(profile_id, resource_id)
                .map_err(unavailable),
            None => Ok(None),
        }
    }

    /// Records the user's exact decision. The caller has already re-resolved the declaration and
    /// its current affected set on this namespace worker.
    pub fn put_shared_resource_consent(
        &mut self,
        row: &durable::SharedResourceConsentRow,
    ) -> Result<(), Status> {
        let store = self
            .durable(true)?
            .ok_or_else(|| Status::unavailable("plugin storage is unavailable"))?;
        store.save_shared_resource_consent(row).map_err(unavailable)
    }

    /// Forgets the decision so the next use requires a fresh review.
    pub fn revoke_shared_resource_consent(
        &mut self,
        profile_id: &str,
        resource_id: &str,
    ) -> Result<bool, Status> {
        let Some(store) = self.durable(false)? else {
            return Ok(false);
        };
        store
            .revoke_shared_resource_consent(profile_id, resource_id)
            .map_err(unavailable)
    }
}

/// File-root approvals live beside the key/value data in the same per-namespace store.
impl Namespace<'_> {
    /// The current time, Unix seconds, as this operation sees it.
    pub fn now(&self) -> i64 {
        self.now
    }

    /// The approval of `installation_id` for `kind`, revoked or not.
    pub fn approval_for(
        &mut self,
        installation_id: &str,
        kind: &str,
    ) -> Result<Option<FileApproval>, Status> {
        match self.durable(false)? {
            Some(store) => store
                .approval_for(installation_id, kind)
                .map_err(unavailable),
            None => Ok(None),
        }
    }

    /// The approval with this id, revoked or not.
    pub fn approval(&mut self, id: &str) -> Result<Option<FileApproval>, Status> {
        match self.durable(false)? {
            Some(store) => store.approval(id).map_err(unavailable),
            None => Ok(None),
        }
    }

    /// Every persistent folder approval in this plugin/profile namespace, revoked or active.
    pub fn file_approvals(&mut self) -> Result<Vec<FileApproval>, Status> {
        match self.durable(false)? {
            Some(store) => store.approvals().map_err(unavailable),
            None => Ok(Vec::new()),
        }
    }

    /// Records an approval, replacing the earlier one of the same installation and kind.
    /// `guard` is checked right before the change.
    pub fn save_approval(
        &mut self,
        approval: &FileApproval,
        guard: &CommitGuard,
    ) -> Result<(), Status> {
        guard()?;
        let store = self
            .durable(true)?
            .ok_or_else(|| Status::unavailable("plugin storage is unavailable"))?;
        store.save_approval(approval).map_err(unavailable)
    }

    /// The remembered confirmation of `installation_id` for `operation`, if any.
    pub fn intent_approval(
        &mut self,
        installation_id: &str,
        operation: &str,
    ) -> Result<Option<IntentApproval>, Status> {
        match self.durable(false)? {
            Some(store) => store
                .intent_approval(installation_id, operation)
                .map_err(unavailable),
            None => Ok(None),
        }
    }

    /// Remembers a confirmation, replacing the earlier one for the same installation and
    /// operation. `guard` is checked right before the change.
    pub fn save_intent_approval(
        &mut self,
        approval: &IntentApproval,
        guard: &CommitGuard,
    ) -> Result<(), Status> {
        guard()?;
        let store = self
            .durable(true)?
            .ok_or_else(|| Status::unavailable("plugin storage is unavailable"))?;
        store.save_intent_approval(approval).map_err(unavailable)
    }

    /// Every remembered confirmation of this plugin.
    pub fn intent_approvals(&mut self) -> Result<Vec<IntentApproval>, Status> {
        match self.durable(false)? {
            Some(store) => store.intent_approvals().map_err(unavailable),
            None => Ok(Vec::new()),
        }
    }

    /// Forgets a remembered confirmation. Returns whether it existed.
    pub fn revoke_intent_approval(&mut self, id: &str) -> Result<bool, Status> {
        match self.durable(false)? {
            Some(store) => store.revoke_intent_approval(id).map_err(unavailable),
            None => Ok(false),
        }
    }

    /// Revokes an approval; its handles stop working at once. Returns whether it was active.
    pub fn revoke_approval(&mut self, id: &str) -> Result<bool, Status> {
        let now = self.now;
        match self.durable(false)? {
            Some(store) => store.revoke_approval(id, now).map_err(unavailable),
            None => Ok(false),
        }
    }
}

#[cfg(all(test, unix))]
mod tests;

impl Namespace<'_> {
    pub fn plugin_libraries(
        &mut self,
    ) -> Result<Vec<gamenode_platform::plugin_storage::PluginLibraryRow>, Status> {
        match self.durable(false)? {
            Some(store) => store.plugin_libraries().map_err(unavailable),
            None => Ok(Vec::new()),
        }
    }

    pub fn put_plugin_library(
        &mut self,
        mut row: gamenode_platform::plugin_storage::PluginLibraryRow,
        expected: u64,
        guard: &CommitGuard,
    ) -> Result<u64, Status> {
        guard()?;
        self.durable(true)?;
        let rows = self.plugin_libraries()?;
        let existing = rows.iter().find(|r| r.id == row.id);
        if existing.is_none()
            && rows.len() >= gamenode_platform::plugin_storage::MAX_PLUGIN_LIBRARIES
        {
            return Err(Status::resource_exhausted("library record limit reached"));
        }
        let root_count = rows
            .iter()
            .filter(|r| r.id != row.id)
            .map(|r| r.roots.len())
            .sum::<usize>()
            + row.roots.len();
        if root_count > gamenode_platform::plugin_storage::MAX_PLUGIN_LIBRARY_ROOTS {
            return Err(Status::resource_exhausted("root limit reached"));
        }
        if existing.as_ref().map_or(0, |r| r.revision) != expected {
            return Err(Status::aborted("library revision changed; get it again"));
        }
        let revision = self
            .inner
            .counter
            .checked_add(1)
            .ok_or_else(|| Status::resource_exhausted("revision limit reached"))?;
        row.revision = revision;
        guard()?;
        self.durable(true)?
            .ok_or_else(|| Status::unavailable("library storage unavailable"))?
            .put_plugin_library(&row)
            .map_err(unavailable)?;
        self.inner.counter = revision;
        Ok(revision)
    }
}

impl Namespace<'_> {
    pub fn backup_sources(&mut self, profile: &str) -> Result<Vec<String>, Status> {
        match self.durable(false)? {
            Some(store) => store.backup_sources(profile).map_err(unavailable),
            None => Ok(vec![]),
        }
    }
    pub fn remember_backup_source(
        &mut self,
        profile: &str,
        destination: &str,
        guard: &CommitGuard,
    ) -> Result<(), Status> {
        guard()?;
        let store = self
            .durable(true)?
            .ok_or_else(|| Status::unavailable("backup storage unavailable"))?;
        let sources = store.backup_sources(profile).map_err(unavailable)?;
        if sources.len() >= 32 && !sources.iter().any(|source| source == destination) {
            return Err(Status::resource_exhausted(
                "backup destination limit reached",
            ));
        }
        store
            .remember_backup_source(profile, destination)
            .map_err(unavailable)
    }
}
