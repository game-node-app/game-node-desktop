//! Reviewed discovery. Native directory events request bounded reconciliation; no executable runs.
use gamenode_core::drm_free::*;
use std::sync::atomic::AtomicBool;
#[cfg(windows)]
use std::sync::atomic::Ordering;
#[derive(Default)]
pub struct WindowsDiscovery;
impl DiscoveryHost for WindowsDiscovery {
    fn discover(
        &self,
        watch: &WatchRoot,
        cancelled: &AtomicBool,
    ) -> Result<DiscoveryReport, LocalError> {
        #[cfg(not(windows))]
        {
            let _ = (watch, cancelled);
            Err(LocalError::Unsupported)
        }
        #[cfg(windows)]
        {
            use std::collections::BTreeSet;
            let root = std::fs::canonicalize(&watch.path)
                .map_err(|e| LocalError::Failed(e.to_string()))?;
            let data = crate::paths::DataPaths::resolve().ok_or(LocalError::InvalidTarget)?;
            let manifest =
                crate::manifest_cache::ManifestCache::bundled(data.data_dir())?.load()?;
            let mut results = Vec::new();
            let mut scan_incomplete = false;
            let host = crate::installations::WindowsInstallations;
            let mut visited = BTreeSet::new();
            for child in std::fs::read_dir(&root).map_err(|e| LocalError::Failed(e.to_string()))? {
                if cancelled.load(Ordering::Relaxed) {
                    return Err(LocalError::Cancelled);
                }
                let child = child.map_err(|e| LocalError::Failed(e.to_string()))?;
                if !child
                    .file_type()
                    .map_err(|e| LocalError::Failed(e.to_string()))?
                    .is_dir()
                {
                    continue;
                }
                let install = child.path();
                let mut queue = vec![(install.clone(), 0usize)];
                let mut count = 0;
                let mut targets = Vec::new();
                let mut evidence = Vec::new();
                let mut title = None;
                let mut incomplete = false;
                let mut identities = BTreeSet::new();
                while let Some((directory, depth)) = queue.pop() {
                    if cancelled.load(Ordering::Relaxed) {
                        return Err(LocalError::Cancelled);
                    }
                    let Ok(canonical) = std::fs::canonicalize(&directory) else {
                        incomplete = true;
                        continue;
                    };
                    if !canonical.starts_with(&root)
                        || !visited.insert(canonical.to_string_lossy().to_lowercase())
                    {
                        continue;
                    }
                    let Ok(entries) = std::fs::read_dir(canonical) else {
                        incomplete = true;
                        continue;
                    };
                    for entry in entries {
                        count += 1;
                        if count > 10000 {
                            incomplete = true;
                            break;
                        }
                        let Ok(entry) = entry else {
                            incomplete = true;
                            continue;
                        };
                        let Ok(kind) = entry.file_type() else {
                            continue;
                        };
                        if kind.is_symlink() {
                            continue;
                        }
                        if kind.is_dir() {
                            if depth < 8 {
                                queue.push((entry.path(), depth + 1));
                            } else {
                                incomplete = true;
                            }
                            continue;
                        }
                        let path = entry.path();
                        if !path
                            .extension()
                            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
                        {
                            continue;
                        }
                        if identity::maintenance(&entry.file_name().to_string_lossy()) {
                            continue;
                        }
                        let Ok(mut registration) = host.inspect(&path.to_string_lossy()) else {
                            continue;
                        };
                        registration.root = install.to_string_lossy().into_owned();
                        let Ok(local) = host.evidence(&registration) else {
                            continue;
                        };
                        let relative = path
                            .strip_prefix(&install)
                            .unwrap_or(&path)
                            .to_string_lossy();
                        let local = manifest.index.corroborate(
                            &local,
                            &child.file_name().to_string_lossy(),
                            &relative,
                        );
                        let Some(inferred) = identity::infer(&local) else {
                            continue;
                        };
                        if !inferred.strong
                            || inferred.conflict
                            || manifest.index.exact(&inferred.title).len() != 1
                        {
                            continue;
                        }
                        let metadata = std::fs::metadata(&path)
                            .map_err(|e| LocalError::Failed(e.to_string()))?;
                        let signature = format!(
                            "{}:{}:{:?}",
                            registration.file_identity,
                            metadata.len(),
                            metadata.modified().ok()
                        );
                        if !identities.insert(signature) {
                            continue;
                        }
                        title.get_or_insert(inferred.title);
                        targets.push(registration.executable);
                        evidence.extend(local);
                    }
                    if count > 10000 {
                        break;
                    }
                }
                scan_incomplete |= incomplete;
                if let Some(title) = title {
                    use sha2::{Digest, Sha256};
                    let fingerprint = format!(
                        "{:x}",
                        Sha256::digest(
                            identities
                                .into_iter()
                                .collect::<Vec<_>>()
                                .join("\n")
                                .as_bytes()
                        )
                    );
                    let selected_target = if targets.len() == 1 {
                        Some(targets[0].clone())
                    } else {
                        None
                    };
                    results.push(Candidate {
                        fingerprint,
                        watch: watch.id.clone(),
                        root: install.to_string_lossy().into_owned(),
                        title,
                        targets,
                        selected_target,
                        evidence,
                        ignored: false,
                        incomplete,
                    });
                }
            }
            Ok(DiscoveryReport {
                candidates: results,
                incomplete: scan_incomplete,
            })
        }
    }
}
#[cfg(windows)]
pub struct FolderNotifications {
    _watcher: notify::RecommendedWatcher,
    events: std::sync::mpsc::Receiver<()>,
    overflow: std::sync::Arc<AtomicBool>,
}
#[cfg(windows)]
impl FolderNotifications {
    pub fn new(path: &str) -> Result<Self, LocalError> {
        use notify::Watcher;
        let (tx, events) = std::sync::mpsc::sync_channel(64);
        let overflow = std::sync::Arc::new(AtomicBool::new(false));
        let flag = overflow.clone();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                // Reading executables during discovery can itself produce access events.
                if event
                    .as_ref()
                    .is_ok_and(|event| matches!(event.kind, notify::EventKind::Access(_)))
                {
                    return;
                }
                if tx.try_send(()).is_err() {
                    flag.store(true, Ordering::Relaxed);
                }
            })
            .map_err(|e| LocalError::Failed(e.to_string()))?;
        watcher
            .watch(std::path::Path::new(path), notify::RecursiveMode::Recursive)
            .map_err(|e| LocalError::Failed(e.to_string()))?;
        Ok(Self {
            _watcher: watcher,
            events,
            overflow,
        })
    }
    pub fn changed(&self) -> bool {
        self.events.try_iter().count() > 0 || self.overflow.swap(false, Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(windows))]
    fn linux_returns_unsupported() {
        let watch = WatchRoot {
            id: "w1".into(),
            path: "/games".into(),
            enabled: true,
        };
        let cancelled = AtomicBool::new(false);
        assert_eq!(
            WindowsDiscovery.discover(&watch, &cancelled),
            Err(LocalError::Unsupported)
        );
    }

    #[test]
    #[cfg(windows)]
    fn windows_discovery_cancellation_returns_cancelled() {
        let watch = WatchRoot {
            id: "w1".into(),
            path: "C:\\".into(),
            enabled: true,
        };
        let cancelled = AtomicBool::new(true);
        assert_eq!(
            WindowsDiscovery.discover(&watch, &cancelled),
            Err(LocalError::Cancelled)
        );
    }
}
