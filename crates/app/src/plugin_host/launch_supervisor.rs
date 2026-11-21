//! Host-owned lifetime management for reviewed custom launches.
//!
//! The platform tree is responsible for providing a fenced identity that cannot be confused
//! with a reused PID. An inspection failure is never interpreted as process exit.

use super::resource_leases::{
    ResourceLease, ResourceLeaseCoordinator, ResourceLeaseError, ResourceLeaseKey,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[cfg(target_os = "linux")]
mod linux_helper;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub(crate) fn spawn_native(command: &gamenode_core::drm_free::CustomLaunchCommand) -> SpawnOutcome {
    linux_helper::spawn(command)
}

/// Handles the private helper mode before the desktop, IPC, or a Tokio runtime is initialized.
#[cfg(target_os = "linux")]
pub fn run_linux_helper_if_requested() -> Option<Result<(), Box<dyn std::error::Error>>> {
    (std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new(linux_helper::HELPER_ARG)))
        .then(linux_helper::run)
}

#[cfg(windows)]
pub(crate) fn spawn_native(command: &gamenode_core::drm_free::CustomLaunchCommand) -> SpawnOutcome {
    windows::spawn(command)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TreeState {
    Running,
    Exited,
    Unknown,
}

pub(crate) trait LaunchTree: Send {
    fn state(&mut self) -> TreeState;
    /// Terminate and fence the entire tree; success requires a later confirmed `Exited` state.
    fn terminate(&mut self) -> Result<(), ()>;
}

/// A platform may return `NotStarted` only when it can prove no process was created. If a
/// process might have started, it must return its tree even when its state is indeterminate.
pub(crate) enum SpawnOutcome {
    Started(Box<dyn LaunchTree>),
    /// The spawn request may have reached the operating system but could not be confirmed.
    /// Keep the tree and its lease; report the failed confirmation to the caller.
    Uncertain(Box<dyn LaunchTree>),
    NotStarted,
}

struct Launch {
    tree: Box<dyn LaunchTree>,
    entry_key: Option<String>,
    // Retained through every indeterminate state and released only after confirmed exit.
    _lease: Option<Box<dyn ResourceLease>>,
}

#[derive(Default)]
pub(crate) struct LaunchSupervisor {
    launches: Mutex<BTreeMap<u64, Launch>>,
    next_id: std::sync::atomic::AtomicU64,
}

impl LaunchSupervisor {
    /// Creates a supervisor whose monitor runs on a dedicated worker thread. Inspection failures
    /// keep launches registered and their leases held; no UI or async executor thread waits.
    pub(crate) fn start_monitor() -> std::io::Result<Arc<Self>> {
        let supervisor = Arc::new(Self::default());
        let weak = Arc::downgrade(&supervisor);
        std::thread::Builder::new()
            .name("custom-launch-supervisor".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    let Some(supervisor) = weak.upgrade() else {
                        break;
                    };
                    let _ = supervisor.refresh_all();
                }
            })?;
        Ok(supervisor)
    }
    /// Acquires the complete lease set before invoking the platform spawn operation.
    #[cfg(test)]
    pub(crate) fn start(
        &self,
        coordinator: &dyn ResourceLeaseCoordinator,
        keys: &[ResourceLeaseKey],
        spawn: impl FnOnce() -> SpawnOutcome,
    ) -> Result<u64, ResourceLeaseError> {
        self.start_for_entry(coordinator, keys, None, spawn)
    }

    pub(crate) fn start_for_entry(
        &self,
        coordinator: &dyn ResourceLeaseCoordinator,
        keys: &[ResourceLeaseKey],
        entry_key: Option<String>,
        spawn: impl FnOnce() -> SpawnOutcome,
    ) -> Result<u64, ResourceLeaseError> {
        let mut launches = self
            .launches
            .lock()
            .map_err(|_| ResourceLeaseError::Unknown)?;
        if launches.len() >= 256 {
            return Err(ResourceLeaseError::Busy);
        }
        let id = self
            .next_id
            .try_update(
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
                |n| n.checked_add(1),
            )
            .map_err(|_| ResourceLeaseError::Unknown)?
            + 1;
        let lease = if keys.is_empty() {
            None
        } else {
            Some(coordinator.acquire_use(keys)?)
        };
        let (tree, confirmed) = match spawn() {
            SpawnOutcome::Started(tree) => (tree, true),
            SpawnOutcome::Uncertain(tree) => (tree, false),
            SpawnOutcome::NotStarted => return Err(ResourceLeaseError::Unknown),
        };
        launches.insert(
            id,
            Launch {
                tree,
                entry_key,
                _lease: lease,
            },
        );
        if confirmed {
            Ok(id)
        } else {
            Err(ResourceLeaseError::Unknown)
        }
    }

    #[cfg(test)]
    pub(crate) fn refresh(&self, id: u64) -> Result<TreeState, ResourceLeaseError> {
        let mut launches = self
            .launches
            .lock()
            .map_err(|_| ResourceLeaseError::Unknown)?;
        let launch = launches.get_mut(&id).ok_or(ResourceLeaseError::Unknown)?;
        let state = launch.tree.state();
        if state == TreeState::Exited {
            launches.remove(&id);
        }
        Ok(state)
    }

    pub(crate) fn stop(&self, id: u64) -> Result<TreeState, ResourceLeaseError> {
        let mut launches = self
            .launches
            .lock()
            .map_err(|_| ResourceLeaseError::Unknown)?;
        let launch = launches.get_mut(&id).ok_or(ResourceLeaseError::Unknown)?;
        launch
            .tree
            .terminate()
            .map_err(|_| ResourceLeaseError::Unknown)?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match launch.tree.state() {
                TreeState::Exited => {
                    launches.remove(&id);
                    return Ok(TreeState::Exited);
                }
                TreeState::Unknown => return Err(ResourceLeaseError::Unknown),
                TreeState::Running if std::time::Instant::now() >= deadline => {
                    return Err(ResourceLeaseError::Unknown);
                }
                TreeState::Running => std::thread::sleep(std::time::Duration::from_millis(25)),
            }
        }
    }

    /// Inspects launches off the UI thread and releases leases only for confirmed exits.
    pub(crate) fn refresh_all(&self) -> Result<(), ResourceLeaseError> {
        let mut launches = self
            .launches
            .lock()
            .map_err(|_| ResourceLeaseError::Unknown)?;
        launches.retain(|_, launch| launch.tree.state() != TreeState::Exited);
        Ok(())
    }

    /// Returns opaque identities for active or indeterminate launches.
    pub(crate) fn active_ids(&self) -> Result<Vec<u64>, ResourceLeaseError> {
        let launches = self
            .launches
            .lock()
            .map_err(|_| ResourceLeaseError::Unknown)?;
        Ok(launches.keys().copied().collect())
    }

    pub(crate) fn ids_for_entry(&self, entry_key: &str) -> Result<Vec<u64>, ResourceLeaseError> {
        let launches = self
            .launches
            .lock()
            .map_err(|_| ResourceLeaseError::Unknown)?;
        Ok(launches
            .iter()
            .filter_map(|(id, launch)| {
                (launch.entry_key.as_deref() == Some(entry_key)).then_some(*id)
            })
            .collect())
    }
}

impl Drop for LaunchSupervisor {
    fn drop(&mut self) {
        // The owner disappearing is not evidence that its launches exited. Preserve leases for
        // the life of the coordinator rather than reporting a potentially active resource idle.
        let launches = self
            .launches
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (_, launch) in std::mem::take(launches) {
            std::mem::forget(launch);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_host::grants::PluginIdentity;
    use crate::plugin_host::resource_leases::ResourceLeases;
    use std::sync::atomic::{AtomicU8, Ordering};

    struct Fixture(Arc<AtomicU8>);
    impl LaunchTree for Fixture {
        fn state(&mut self) -> TreeState {
            match self.0.load(Ordering::SeqCst) {
                0 => TreeState::Running,
                1 => TreeState::Exited,
                _ => TreeState::Unknown,
            }
        }
        fn terminate(&mut self) -> Result<(), ()> {
            self.0.store(1, Ordering::SeqCst);
            Ok(())
        }
    }
    fn key() -> ResourceLeaseKey {
        ResourceLeaseKey::derive(
            &PluginIdentity {
                plugin_id: "example".into(),
                publisher_fingerprint: "publisher".into(),
                profile_id: "profile".into(),
            },
            1,
            "save",
            1,
            b"handle",
        )
        .unwrap()
    }
    #[test]
    fn lease_is_acquired_before_spawn_and_held_until_confirmed_exit() {
        let leases = ResourceLeases::default();
        let supervisor = LaunchSupervisor::default();
        let state = Arc::new(AtomicU8::new(0));
        let id = supervisor
            .start(&leases, &[key()], || {
                assert!(matches!(
                    leases.acquire_exclusive(&[key()]),
                    Err(ResourceLeaseError::Busy)
                ));
                SpawnOutcome::Started(Box::new(Fixture(state.clone())))
            })
            .unwrap();
        state.store(2, Ordering::SeqCst);
        assert_eq!(supervisor.refresh(id).unwrap(), TreeState::Unknown);
        assert!(matches!(
            leases.acquire_exclusive(&[key()]),
            Err(ResourceLeaseError::Busy)
        ));
        state.store(1, Ordering::SeqCst);
        assert_eq!(supervisor.refresh(id).unwrap(), TreeState::Exited);
        assert!(leases.acquire_exclusive(&[key()]).is_ok());
    }
    #[test]
    fn exclusive_lease_blocks_spawn_and_stop_fences_before_release() {
        let leases = ResourceLeases::default();
        let supervisor = LaunchSupervisor::default();
        let exclusive = leases.acquire_exclusive(&[key()]).unwrap();
        assert!(matches!(
            supervisor.start(&leases, &[key()], || panic!("spawn must not run")),
            Err(ResourceLeaseError::Busy)
        ));
        drop(exclusive);
        let id = supervisor
            .start(&leases, &[key()], || {
                SpawnOutcome::Started(Box::new(Fixture(Arc::new(AtomicU8::new(0)))))
            })
            .unwrap();
        assert_eq!(supervisor.stop(id).unwrap(), TreeState::Exited);
        assert!(leases.acquire_exclusive(&[key()]).is_ok());
    }

    #[test]
    fn failed_spawn_releases_the_reservation() {
        let leases = ResourceLeases::default();
        let supervisor = LaunchSupervisor::default();
        assert!(matches!(
            supervisor.start(&leases, &[key()], || SpawnOutcome::NotStarted),
            Err(ResourceLeaseError::Unknown)
        ));
        assert!(leases.acquire_exclusive(&[key()]).is_ok());
    }

    #[test]
    fn uncertain_spawn_reports_failure_but_keeps_reservation() {
        let leases = ResourceLeases::default();
        let supervisor = LaunchSupervisor::default();
        assert!(matches!(
            supervisor.start(&leases, &[key()], || {
                SpawnOutcome::Uncertain(Box::new(Fixture(Arc::new(AtomicU8::new(2)))))
            }),
            Err(ResourceLeaseError::Unknown)
        ));
        assert!(matches!(
            leases.acquire_exclusive(&[key()]),
            Err(ResourceLeaseError::Busy)
        ));
    }

    #[test]
    fn dropping_supervisor_does_not_claim_live_tree_has_exited() {
        let leases = ResourceLeases::default();
        {
            let supervisor = LaunchSupervisor::default();
            supervisor
                .start(&leases, &[key()], || {
                    SpawnOutcome::Started(Box::new(Fixture(Arc::new(AtomicU8::new(0)))))
                })
                .unwrap();
        }
        assert!(matches!(
            leases.acquire_exclusive(&[key()]),
            Err(ResourceLeaseError::Busy)
        ));
    }
}
