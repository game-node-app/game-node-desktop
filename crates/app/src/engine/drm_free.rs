//! Short UI commands; filesystem and persistence work run on blocking workers.
mod suggestions;
use super::*;
use gamenode_core::drm_free::{
    BackupReason, BackupVersion, BackupVersionMeta, Candidate, ClaimRegistration, ClaimStoreEntry,
    LocalError, LocalGameStore, LocalView, PendingClaim, Registration,
};
impl Engine {
    pub fn refresh_local_games(&self) {
        let engine = self.clone();
        self.shared.runtime.spawn_blocking(move || {
            let _views = engine.lock_local_views();
            let (account, generation) = {
                let state = engine.lock();
                (
                    state.library.account().map(|a| a.id.clone()),
                    state.local_generation,
                )
            };
            let mut view = engine.load_local_view();
            let inferences = account
                .as_ref()
                .and_then(|account| {
                    engine
                        .shared
                        .ports
                        .local_games
                        .as_ref()?
                        .inferences(account)
                        .ok()
                })
                .unwrap_or_default();
            let mut state = engine.lock();
            if state.local_generation == generation {
                for inference in &inferences {
                    if view.registrations.iter().any(|r| {
                        r.id == inference.installation
                            && r.revision == inference.revision
                            && !r.removed
                    }) {
                        let effects = state.library.set_local_inference(
                            format!("manual:{}", inference.installation),
                            inference.game_id,
                            None,
                        );
                        for effect in effects {
                            engine.library_effect(&mut state, effect);
                        }
                    }
                }
            }
            if state.local_generation != generation {
                return;
            }
            view.inferences = inferences;
            view.busy = state.local.busy;
            view.versions = state.local.versions.clone();
            view.history_bytes = state.local.history_bytes;
            view.history_count = state.local.history_count;
            view.restore = state.local.restore.clone();
            view.review_selected = state.local.review_selected.clone();
            view.recently_added = state.local.recently_added.clone();
            view.discovery_incomplete = state.local.discovery_incomplete.clone();
            view.discovery_scanning = state.local.discovery_scanning;
            view.discovery_completed = state.local.discovery_completed;
            view.discovery_error = state.local.discovery_error.clone();
            view.discovery_last_checked_at = state
                .local
                .discovery_last_checked_at
                .or(view.discovery_last_checked_at);
            engine.apply_review_defaults(&mut state, &mut view);
            state.library.replace_local_covers(local_covers(&view));
            state.local = view;
            engine.publish(&mut state);
            drop(state);
            engine.request_discovery_suggestions();
        });
    }
    pub fn add_local_executable(&self, path: Option<String>) {
        let Some(path) = path else {
            return;
        };
        self.local_work(move |engine| {
            let (store, host) = engine
                .shared
                .ports
                .local_games
                .as_ref()
                .zip(engine.shared.ports.installations.as_ref())
                .ok_or(LocalError::Unsupported)?;
            let mut registration = host.inspect(&path)?;
            let registrations = store.registrations().map_err(local_storage)?;
            let existing = registrations
                .iter()
                .find(|r| !r.removed && r.file_identity == registration.file_identity)
                .cloned();
            if existing.is_none()
                && let Some(old) = registrations.into_iter().find(|r| {
                    r.removed
                        && (r.file_identity == registration.file_identity
                            || r.root.eq_ignore_ascii_case(&registration.root)
                            || gamenode_core::provider::path::normalize(&r.executable)
                                .eq_ignore_ascii_case(&gamenode_core::provider::path::normalize(
                                    &registration.executable,
                                )))
                })
            {
                registration.id = old.id;
                registration.revision = old.revision + 1;
                registration.custom_cover = old.custom_cover;
            }
            let key = existing.as_ref().unwrap_or(&registration).key();
            if existing.is_none() {
                let claim = {
                    let state = engine.lock();
                    let exe = gamenode_core::provider::path::normalize(&registration.executable)
                        .to_lowercase();
                    state
                        .library
                        .installed_entries()
                        .filter(|e| e.provider != "manual")
                        .find(|entry| {
                            let root = gamenode_core::provider::path::normalize(&entry.install_dir)
                                .to_lowercase();
                            let sep = if gamenode_core::provider::path::is_windows_style(&root) {
                                '\\'
                            } else {
                                '/'
                            };
                            exe.starts_with(&format!("{}{sep}", root.trim_end_matches(['/', '\\'])))
                        })
                        .map(|entry| {
                            let store_key = entry.key.clone();
                            let store_title =
                                entry.name.clone().unwrap_or_else(|| entry.store_id.clone());
                            let store_cover = state.library.cover_for(&store_key);
                            let playtime_seconds = state
                                .library
                                .playtime_for(&store_key)
                                .and_then(|playtime| playtime.seconds);
                            // A custom cover wins over the matched GameNode cover (design D7/H).
                            let registration_cover = registration
                                .custom_cover
                                .clone()
                                .or_else(|| state.library.cover_for(&key));
                            (
                                PendingClaim {
                                    store_key,
                                    store_entry: ClaimStoreEntry {
                                        title: store_title,
                                        provider: entry.provider.clone(),
                                        cover_file: store_cover,
                                        playtime_seconds,
                                    },
                                    registration: ClaimRegistration {
                                        title: registration.title.clone(),
                                        root: registration.root.clone(),
                                        cover_file: registration_cover,
                                    },
                                },
                                registration.clone(),
                            )
                        })
                };
                if let Some((claim, draft)) = claim {
                    let mut state = engine.lock();
                    state.pending_claim = Some(claim);
                    state.pending_claim_registration = Some(draft);
                    return Ok(());
                }
                store
                    .save_registration(&registration)
                    .map_err(local_storage)?;
                // A port whose registration was removed keeps its provenance; adding the same
                // executable again links the existing install instead of creating a second one
                // (design D10).
                engine.relink_port_registration(&registration);
            }
            engine.lock().local_selection = Some(key);
            Ok(())
        });
    }
    pub fn remove_local_game(&self, id: String, allow_rediscovery: bool) {
        self.local_work(move |engine| {
            let store = engine
                .shared
                .ports
                .local_games
                .as_ref()
                .ok_or(LocalError::Unsupported)?;
            let registrations = store.registrations().map_err(local_storage)?;
            if let Some(mut registration) = registrations.iter().find(|r| r.id == id).cloned() {
                registration.removed = true;
                registration.rediscovery_allowed = allow_rediscovery;
                registration.revision += 1;
                store
                    .save_registration(&registration)
                    .map_err(local_storage)?;
                for mut other in registrations.into_iter().filter(|r| {
                    r.id != registration.id
                        && r.removed
                        && (r.file_identity == registration.file_identity
                            || r.root.eq_ignore_ascii_case(&registration.root))
                }) {
                    if other.rediscovery_allowed != allow_rediscovery {
                        other.rediscovery_allowed = allow_rediscovery;
                        store.save_registration(&other).map_err(local_storage)?;
                    }
                }
                let should_ignore = !allow_rediscovery;
                for mut candidate in store.candidates().map_err(local_storage)? {
                    if candidate_matches_registration(&candidate, &registration)
                        && candidate.ignored != should_ignore
                    {
                        candidate.ignored = should_ignore;
                        store.save_candidate(&candidate).map_err(local_storage)?;
                    }
                }
            }
            Ok(())
        });
    }
    pub fn edit_local_game(&self, value: Registration) {
        self.local_work(move |engine| {
            let (store, host) = engine
                .shared
                .ports
                .local_games
                .as_ref()
                .zip(engine.shared.ports.installations.as_ref())
                .ok_or(LocalError::Unsupported)?;
            let custom_cover = match &value.custom_cover {
                Some(name) => {
                    if !gamenode_platform::covers::is_local_cover_name(&value.id, name) {
                        return Err(LocalError::InvalidTarget);
                    }
                    Some(name.clone())
                }
                None => None,
            };
            let mut inspected = host.inspect(&value.executable)?;
            inspected.id = value.id;
            inspected.title = value.title;
            inspected.root = value.root;
            inspected.working_directory = value.working_directory;
            inspected.arguments = value.arguments;
            inspected.gameplay = value.gameplay;
            inspected.launchers = value.launchers;
            inspected.custom_cover = custom_cover.clone();
            let current = store
                .registrations()
                .map_err(local_storage)?
                .into_iter()
                .find(|r| r.id == inspected.id && !r.removed)
                .ok_or(LocalError::InvalidTarget)?;
            if current.revision != value.revision {
                return Err(LocalError::Failed(
                    "game settings changed; reopen the editor".into(),
                ));
            }
            inspected.revision = current.revision + 1;
            let inspected = host.validate_registration(&inspected)?;
            store.save_registration(&inspected).map_err(local_storage)?;

            // The editor is closing with this save: a pick still in flight for the same
            // registration must not leave a second orphan behind.
            let (pending, other_pending) = {
                let mut state = engine.lock();
                *state
                    .local_cover_epoch
                    .entry(inspected.id.clone())
                    .or_default() += 1;
                let pending = state.local_cover_pending.remove(&inspected.id);
                (pending, state.local_cover_pending.clone())
            };
            let registrations = store.registrations().map_err(local_storage)?;
            if let Some(previous) = current.custom_cover.as_deref()
                && Some(previous) != custom_cover.as_deref()
            {
                remove_cover_if_unreferenced(engine, &registrations, &other_pending, previous);
            }
            if let Some(pending) = pending
                && Some(pending.as_str()) != custom_cover.as_deref()
            {
                remove_cover_if_unreferenced(engine, &registrations, &other_pending, &pending);
            }
            Ok(())
        });
    }
    pub fn repair_local_game(&self, id: String, path: Option<String>) {
        let Some(path) = path else {
            return;
        };
        self.local_work(move |engine| {
            let (store, host) = engine
                .shared
                .ports
                .local_games
                .as_ref()
                .zip(engine.shared.ports.installations.as_ref())
                .ok_or(LocalError::Unsupported)?;
            let mut replacement = host.inspect(&path)?;
            let old = store
                .registrations()
                .map_err(local_storage)?
                .into_iter()
                .find(|r| r.id == id)
                .ok_or(LocalError::InvalidTarget)?;
            replacement.id = old.id;
            replacement.title = old.title;
            replacement.revision = old.revision + 1;
            replacement.arguments = old.arguments;
            replacement.custom_cover = old.custom_cover;
            store
                .save_registration(&replacement)
                .map_err(local_storage)?;
            Ok(())
        });
    }
    /// Marks one local operation busy and clears its previous error. Returns `Busy` instead of
    /// silently dropping a second request.
    fn start_local_work(&self) -> Result<(), LocalError> {
        let mut state = self.lock();
        if state.local.busy {
            return Err(LocalError::Busy);
        }
        state.local_generation += 1;
        state.local.busy = true;
        state.local.error = None;
        self.publish(&mut state);
        Ok(())
    }
    /// Rebuilds the local view after a worker finished and publishes the result. Returns the
    /// operation's own result so an awaiting command can report it.
    fn finish_local_work(&self, result: Result<(), LocalError>) -> Result<(), LocalError> {
        let _views = self.lock_local_views();
        let view = self.load_local_view();
        {
            let mut state = self.lock();
            let mut view = view;
            view.inferences = state.local.inferences.clone();
            view.versions = state.local.versions.clone();
            view.history_bytes = state.local.history_bytes;
            view.history_count = state.local.history_count;
            view.restore = state.local.restore.clone();
            view.recently_added = state.local.recently_added.clone();
            view.discovery_incomplete = state.local.discovery_incomplete.clone();
            view.discovery_scanning = state.local.discovery_scanning;
            view.discovery_completed = state.local.discovery_completed;
            view.discovery_error = state.local.discovery_error.clone();
            view.discovery_last_checked_at = state
                .local
                .discovery_last_checked_at
                .or(view.discovery_last_checked_at);
            self.apply_review_defaults(&mut state, &mut view);
            state.library.replace_local_covers(local_covers(&view));
            state.local = view;
            state.local.error = result.as_ref().err().map(|e| e.to_string());
            self.publish(&mut state);
        }
        self.rescan();
        self.enrich_local_games();
        self.request_discovery_suggestions();
        result
    }
    fn local_work(&self, work: impl FnOnce(&Engine) -> Result<(), LocalError> + Send + 'static) {
        if self.start_local_work().is_err() {
            return;
        }
        let engine = self.clone();
        self.shared.runtime.spawn_blocking(move || {
            let _ = engine.finish_local_work(work(&engine));
        });
    }
    /// Saves the three backup settings as one atomic operation and waits for the actual blocking
    /// validation and write. A busy worker is reported instead of dropping the request.
    pub async fn set_local_backup_settings(
        &self,
        automatic: bool,
        destination: String,
        retention: u8,
    ) -> Result<(), LocalError> {
        use gamenode_core::drm_free::valid_retention;
        if !valid_retention(retention) {
            return Err(LocalError::InvalidTarget);
        }
        self.start_local_work()?;
        let (done, finished) = tokio::sync::oneshot::channel();
        let engine = self.clone();
        self.shared.runtime.spawn_blocking(move || {
            let result = (|| -> Result<(), LocalError> {
                let store = engine
                    .shared
                    .ports
                    .local_games
                    .as_ref()
                    .ok_or(LocalError::Unsupported)?;
                let registrations = store.registrations().map_err(local_storage)?;
                let profiles = store.profiles().map_err(local_storage)?;
                let protected: Vec<_> = registrations
                    .iter()
                    .filter(|r| !r.removed)
                    .map(|r| std::path::PathBuf::from(&r.root))
                    .chain(
                        profiles
                            .iter()
                            .flat_map(|p| p.files.iter().map(std::path::PathBuf::from)),
                    )
                    .chain(
                        store
                            .watches()
                            .map_err(local_storage)?
                            .iter()
                            .map(|w| std::path::PathBuf::from(&w.path)),
                    )
                    .collect();
                gamenode_platform::backups::validate_destination(
                    std::path::Path::new(&destination),
                    &protected,
                )?;
                let mut settings = store.backup_settings().map_err(local_storage)?;
                settings.automatic = automatic;
                settings.destination = destination;
                settings.retention = retention;
                store
                    .save_backup_settings(&settings)
                    .map_err(local_storage)?;
                if !automatic {
                    for mut job in store
                        .jobs()
                        .map_err(local_storage)?
                        .into_iter()
                        .filter(|j| {
                            j.play_period.is_some()
                                && j.state == gamenode_core::drm_free::JobState::Queued
                        })
                    {
                        job.state = gamenode_core::drm_free::JobState::Cancelled;
                        store.save_job(&job).map_err(local_storage)?;
                    }
                }
                Ok(())
            })();
            let result = engine.finish_local_work(result);
            let _ = done.send(result);
        });
        finished.await.map_err(|_| LocalError::Cancelled)?
    }
    fn load_local_view(&self) -> LocalView {
        let Some(store) = &self.shared.ports.local_games else {
            return LocalView::default();
        };
        let result = (|| {
            let mut roots = store.watches()?;
            let registrations = store.registrations()?;
            // Availability is a fact of this moment: the first time a folder is missing keeps
            // the time it became unavailable, and reconnecting clears it.
            let now = unix_now();
            for root in &mut roots {
                let available = !root.path.is_empty() && std::path::Path::new(&root.path).is_dir();
                let changed = if available {
                    root.unavailable_since.take().is_some()
                } else if root.unavailable_since.is_none() {
                    root.unavailable_since = Some(now);
                    true
                } else {
                    false
                };
                if changed {
                    store.save_watch(root)?;
                }
            }
            let checking = self
                .shared
                .local_checking
                .lock()
                .map(|ids| ids.clone())
                .unwrap_or_default();
            let watches: Vec<gamenode_core::drm_free::WatchView> = roots
                .into_iter()
                .map(|root| watch_view(root, &registrations, &checking))
                .collect();
            let candidates = store
                .candidates()?
                .into_iter()
                .filter(|candidate| {
                    watches
                        .iter()
                        .any(|watch| watch.enabled && watch.id == candidate.watch)
                        && !registrations.iter().any(|r| {
                            candidate_matches_registration(candidate, r)
                                && r.removed
                                && !r.rediscovery_allowed
                        })
                })
                .map(|mut candidate| {
                    candidate.likely_non_game =
                        gamenode_core::drm_free::identity::likely_non_game(&candidate);
                    candidate
                })
                .collect();
            let discovery_last_checked_at = self
                .shared
                .ports
                .settings
                .load_discovery_last_checked_at()
                .ok()
                .flatten();
            Ok::<_, StorageError>(LocalView {
                watches,
                registrations,
                candidates,
                discovery_last_checked_at,
                profiles: store.profiles()?,
                jobs: store.jobs()?,
                settings: {
                    let mut settings = store.backup_settings()?;
                    if settings.destination.is_empty() {
                        settings.destination =
                            default_backup_destination(&self.shared.ports.covers_dir);
                        store.save_backup_settings(&settings)?;
                    }
                    settings
                },
                ..LocalView::default()
            })
        })();
        result.unwrap_or_else(|e| LocalView {
            error: Some(e.to_string()),
            ..LocalView::default()
        })
    }

    /// Applies the review defaults: a candidate that first appears starts selected unless it is a
    /// likely non-game. A fingerprint already seen keeps whatever the user chose in an earlier
    /// snapshot, so a refresh never re-selects a manual uncheck.
    fn apply_review_defaults(
        &self,
        state: &mut State,
        view: &mut gamenode_core::drm_free::LocalView,
    ) {
        let active: std::collections::BTreeSet<&str> = view
            .candidates
            .iter()
            .filter(|candidate| !candidate.ignored)
            .map(|candidate| candidate.fingerprint.as_str())
            .collect();
        state
            .local_review_seen
            .retain(|fingerprint| active.contains(fingerprint.as_str()));
        let mut selected: Vec<String> = state
            .local
            .review_selected
            .iter()
            .filter(|fingerprint| active.contains(fingerprint.as_str()))
            .cloned()
            .collect();
        for candidate in view
            .candidates
            .iter()
            .filter(|candidate| !candidate.ignored)
        {
            if state
                .local_review_seen
                .insert(candidate.fingerprint.clone())
                && !candidate.likely_non_game
                && !selected.contains(&candidate.fingerprint)
            {
                selected.push(candidate.fingerprint.clone());
            }
        }
        view.review_selected = selected;
        state.local.review_selected = view.review_selected.clone();
    }
}
fn local_storage(error: StorageError) -> LocalError {
    LocalError::Failed(error.to_string())
}

/// Records a review suggestion as the entry's automatic match, never as a user choice, so a later
/// lookup may still improve it. The inference keeps it across restarts.
fn apply_candidate_suggestion(
    engine: &Engine,
    store: &dyn gamenode_core::drm_free::LocalGameStore,
    registration: &Registration,
    candidate: &Candidate,
    suggestion: gamenode_core::library::SuggestionView,
) -> Result<(), LocalError> {
    let account = engine
        .lock()
        .library
        .account()
        .map(|account| account.id.clone());
    let Some(account) = account else {
        return Ok(());
    };
    let game = gamenode_core::library::GameRecord {
        game_id: suggestion.game_id,
        name: suggestion.title,
        release_year: suggestion.year,
        cover_url: suggestion.cover_url,
        cover_file: None,
        cover_failed_at: None,
        platforms: Vec::new(),
    };
    let inference = gamenode_core::drm_free::Inference {
        installation: registration.id.clone(),
        revision: registration.revision,
        title: candidate.title.clone(),
        evidence: candidate.evidence.clone(),
        game_id: Some(game.game_id),
        account,
    };
    store.save_inference(&inference).map_err(local_storage)?;
    if let Err(error) = engine
        .shared
        .ports
        .library
        .save_games(std::slice::from_ref(&game))
    {
        tracing::warn!(%error, "could not save the suggested game");
    }
    let mut state = engine.lock();
    let effects =
        state
            .library
            .set_local_inference(registration.key(), Some(game.game_id), Some(game));
    for effect in effects {
        engine.library_effect(&mut state, effect);
    }
    engine.publish(&mut state);
    Ok(())
}

/// One watched root as Settings shows it, with the entries whose installation is inside it.
fn watch_view(
    root: gamenode_core::drm_free::WatchRoot,
    registrations: &[gamenode_core::drm_free::Registration],
    checking: &std::collections::BTreeSet<String>,
) -> gamenode_core::drm_free::WatchView {
    use gamenode_core::drm_free::{WatchState, WatchView};
    let entry_count = registrations
        .iter()
        .filter(|registration| !registration.removed)
        .filter(|registration| path_inside(&root.path, &registration.root))
        .count();
    let state = if root.unavailable_since.is_some() {
        WatchState::Unavailable
    } else if checking.contains(&root.id) {
        WatchState::Checking
    } else {
        WatchState::Idle
    };
    WatchView {
        id: root.id,
        path: root.path,
        enabled: root.enabled,
        entry_count: u32::try_from(entry_count).unwrap_or(u32::MAX),
        last_checked_at: root.last_checked_at,
        state,
        unavailable_since: root.unavailable_since,
    }
}

/// Whether `path` is `root` or one of its descendants, on either path style.
fn path_inside(root: &str, path: &str) -> bool {
    if root.is_empty() || path.is_empty() {
        return false;
    }
    let root = gamenode_core::provider::path::normalize(root).to_lowercase();
    let path = gamenode_core::provider::path::normalize(path).to_lowercase();
    let separator = if gamenode_core::provider::path::is_windows_style(&root) {
        '\\'
    } else {
        '/'
    };
    let trimmed = root.trim_end_matches(['/', '\\']);
    path == trimmed || path.starts_with(&format!("{trimmed}{separator}"))
}

/// The destination the app uses when none was chosen.
fn default_backup_destination(covers_dir: &std::path::Path) -> String {
    covers_dir
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("backups")
        .to_string_lossy()
        .into_owned()
}

/// Space and version count of every game under `<destination>/manual`, including recovery
/// profiles and games that no longer exist in the library.
fn backup_usage_at(
    destination: &str,
    registrations: &[gamenode_core::drm_free::Registration],
    profiles: &[gamenode_core::drm_free::SaveProfile],
) -> gamenode_core::drm_free::BackupUsage {
    use gamenode_core::drm_free::{BackupUsage, BackupUsageGame};
    let mut usage = BackupUsage::default();
    let base = std::path::Path::new(destination).join("manual");
    let Ok(entries) = std::fs::read_dir(&base) else {
        return usage;
    };
    for entry in entries.flatten() {
        if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        let (bytes, versions) = backup_directory_totals(&entry.path());
        if bytes == 0 && versions == 0 {
            continue;
        }
        let title = registrations
            .iter()
            .find(|registration| registration.id == id)
            .map(|registration| registration.title.clone())
            .or_else(|| {
                profiles
                    .iter()
                    .find(|profile| profile.installation == id)
                    .map(|profile| profile.title.clone())
            })
            .unwrap_or_else(|| id.clone());
        usage.games.push(BackupUsageGame {
            entry_key: format!("manual:{id}"),
            title,
            cover_file: None,
            bytes,
            versions,
        });
    }
    usage.games.sort_by(|left, right| {
        right
            .bytes
            .cmp(&left.bytes)
            .then_with(|| left.title.cmp(&right.title))
    });
    usage
}

/// Bytes under one installation and its version count. Each profile directory keeps one
/// subdirectory per version, so recovery profiles count like ordinary ones.
fn backup_directory_totals(installation: &std::path::Path) -> (u64, u32) {
    let mut versions = 0u32;
    let Ok(profiles) = std::fs::read_dir(installation) else {
        return (0, 0);
    };
    for profile in profiles.flatten() {
        if !profile
            .file_type()
            .map(|kind| kind.is_dir())
            .unwrap_or(false)
        {
            continue;
        }
        if let Ok(backups) = std::fs::read_dir(profile.path()) {
            for backup in backups.flatten() {
                if backup
                    .file_type()
                    .map(|kind| kind.is_dir())
                    .unwrap_or(false)
                    && !backup.file_name().to_string_lossy().starts_with('.')
                {
                    versions = versions.saturating_add(1);
                }
            }
        }
    }
    (files_size(installation), versions)
}

/// Sums regular files under `directory`, without following symbolic links.
fn files_size(directory: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return 0;
    };
    let mut total = 0u64;
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            total = total.saturating_add(files_size(&entry.path()));
        } else if kind.is_file() {
            total = total.saturating_add(entry.metadata().map(|meta| meta.len()).unwrap_or(0));
        }
    }
    total
}

/// The custom covers the library view must apply, by entry key.
fn local_covers(view: &LocalView) -> std::collections::BTreeMap<String, String> {
    view.registrations
        .iter()
        .filter(|registration| !registration.removed)
        .filter_map(|registration| {
            registration
                .custom_cover
                .as_ref()
                .map(|file| (registration.key(), file.clone()))
        })
        .collect()
}

/// Deletes a custom-cover file only when no registration or pending pick still points at it.
fn remove_cover_if_unreferenced(
    engine: &Engine,
    registrations: &[Registration],
    other_pending: &std::collections::BTreeMap<String, String>,
    file: &str,
) {
    if !gamenode_platform::covers::is_local_cover_file(file) {
        return;
    }
    if registrations
        .iter()
        .any(|registration| registration.custom_cover.as_deref() == Some(file))
        || other_pending.values().any(|pending| pending == file)
    {
        return;
    }
    let _ = std::fs::remove_file(engine.shared.ports.covers_dir.join(file));
}

/// Resolves a `manual:<id>` or bare `<id>` key to the registration id.
fn registration_id(entry_key: &str) -> String {
    entry_key
        .strip_prefix("manual:")
        .unwrap_or(entry_key)
        .to_owned()
}

/// Parses the Ludusavi archive time (`YYYY-MM-DD HH:MM:SS`, treated as UTC) into Unix seconds.
pub(super) fn backup_archive_time(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    if bytes[4] != b'-'
        || bytes[7] != b'-'
        || !matches!(bytes[10], b' ' | b'T')
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let number = |range: std::ops::Range<usize>| value.get(range)?.parse::<i64>().ok();
    let year = number(0..4)?;
    let month = number(5..7)?;
    let day = number(8..10)?;
    let hour = number(11..13)?;
    let minute = number(14..16)?;
    let second = number(17..19)?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=59).contains(&second)
    {
        return None;
    }
    Some(
        days_from_civil(year, month, day)
            .saturating_mul(86_400)
            .saturating_add(hour * 3_600 + minute * 60 + second),
    )
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let shifted_month = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn history_totals(versions: &[BackupVersion]) -> (u64, u32) {
    (
        versions.iter().map(|version| version.bytes).sum(),
        u32::try_from(versions.len()).unwrap_or(u32::MAX),
    )
}

fn history_version(
    installation: &str,
    profile: &str,
    destination: &str,
    item: &serde_json::Value,
    bytes: u64,
    meta: Option<&BackupVersionMeta>,
) -> Option<BackupVersion> {
    let name = item["name"].as_str()?;
    let label = item["when"].as_str().unwrap_or(name).to_owned();
    Some(BackupVersion {
        installation: installation.to_owned(),
        profile: profile.to_owned(),
        destination: destination.to_owned(),
        id: name.to_owned(),
        created_at: backup_archive_time(&label).or_else(|| meta.map(|m| m.created_at)),
        reason: meta.map(|m| m.reason),
        play_seconds: meta.and_then(|m| m.play_seconds),
        label,
        bytes,
    })
}

fn collect_history(
    backup: &dyn gamenode_core::drm_free::BackupEngine,
    cancel: &std::sync::atomic::AtomicBool,
    request: &gamenode_core::drm_free::BackupRequest,
    installation: &str,
    profile_id: &str,
    meta: &[BackupVersionMeta],
    versions: &mut Vec<BackupVersion>,
) -> Result<(), LocalError> {
    use gamenode_core::drm_free::BackupOperation;
    let report = backup.execute(request, cancel)?;
    let json: serde_json::Value =
        serde_json::from_str(&report.json).map_err(|e| LocalError::Failed(e.to_string()))?;
    let Some(backups) = json["games"][&request.profile.title]["backups"].as_array() else {
        return Ok(());
    };
    for item in backups {
        let Some(name) = item["name"].as_str() else {
            continue;
        };
        let preview = backup.execute(
            &gamenode_core::drm_free::BackupRequest {
                operation: BackupOperation::RestorePreview {
                    backup: name.to_owned(),
                },
                ..request.clone()
            },
            cancel,
        )?;
        let size: serde_json::Value =
            serde_json::from_str(&preview.json).map_err(|e| LocalError::Failed(e.to_string()))?;
        let bytes = size["overall"]["totalBytes"].as_u64().unwrap_or(0);
        let meta = meta
            .iter()
            .find(|m| m.profile == profile_id && m.backup_id == name);
        if let Some(version) = history_version(
            installation,
            profile_id,
            &request.destination,
            item,
            bytes,
            meta,
        ) {
            versions.push(version);
        }
    }
    Ok(())
}

fn record_restore_recovery_meta(
    store: &dyn LocalGameStore,
    backup: &dyn gamenode_core::drm_free::BackupEngine,
    request: &gamenode_core::drm_free::BackupRequest,
    cancelled: &std::sync::atomic::AtomicBool,
    now: i64,
) -> Result<(), LocalError> {
    use gamenode_core::drm_free::{BackupOperation, BackupRequest, JobState};
    let marker_path = std::path::Path::new(&request.destination)
        .join("manual")
        .join(&request.installation.id)
        .join(&request.profile.id)
        .join(".gamenode-restore-recovery.json");
    let marker: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&marker_path).map_err(|e| LocalError::Failed(e.to_string()))?,
    )
    .map_err(|e| LocalError::Failed(e.to_string()))?;
    let state: JobState =
        serde_json::from_value(marker["state"].clone()).unwrap_or(JobState::Unchanged);
    if state != JobState::Successful {
        return Ok(());
    }
    let recovery = std::path::PathBuf::from(
        marker["recovery"]
            .as_str()
            .ok_or(LocalError::InvalidTarget)?,
    );
    let recovery_id = recovery
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or(LocalError::InvalidTarget)?;
    let mut profile = request.profile.clone();
    profile.id = recovery_id.clone();
    let report = backup.execute(
        &BackupRequest {
            operation: BackupOperation::History,
            ..request.clone()
        },
        cancelled,
    )?;
    let history: serde_json::Value =
        serde_json::from_str(&report.json).map_err(|e| LocalError::Failed(e.to_string()))?;
    let backup_id = history["games"][&profile.title]["backups"]
        .as_array()
        .into_iter()
        .flatten()
        .max_by_key(|item| item["when"].as_str())
        .and_then(|item| item["name"].as_str())
        .ok_or(LocalError::InvalidTarget)?;
    store
        .save_backup_version_meta(&BackupVersionMeta {
            installation: request.installation.id.clone(),
            profile: recovery_id,
            backup_id: backup_id.to_owned(),
            reason: BackupReason::Recovery,
            play_seconds: None,
            created_at: now,
        })
        .map_err(local_storage)
}

fn record_backup_meta(
    store: &dyn LocalGameStore,
    installation: &str,
    profile: &str,
    backup_id: &str,
    reason: BackupReason,
    play_seconds: Option<u32>,
    now: i64,
) -> Result<(), LocalError> {
    store
        .save_backup_version_meta(&BackupVersionMeta {
            installation: installation.to_owned(),
            profile: profile.to_owned(),
            backup_id: backup_id.to_owned(),
            reason,
            play_seconds,
            created_at: now,
        })
        .map_err(local_storage)
}

pub(super) fn candidate_matches_registration(
    candidate: &gamenode_core::drm_free::Candidate,
    registration: &Registration,
) -> bool {
    candidate.root.eq_ignore_ascii_case(&registration.root)
        || candidate.targets.iter().any(|target| {
            gamenode_core::provider::path::normalize(target).eq_ignore_ascii_case(
                &gamenode_core::provider::path::normalize(&registration.executable),
            )
        })
}

impl Engine {
    pub fn pick_local_executable(&self, repair: Option<String>) {
        let engine = self.clone();
        self.shared.runtime.spawn_blocking(move || {
            match gamenode_platform::installations::pick_executable() {
                Ok(path) => {
                    if let Some(id) = repair {
                        engine.repair_local_game(id, path);
                    } else {
                        engine.add_local_executable(path);
                    }
                }
                Err(error) => {
                    let mut state = engine.lock();
                    state.local.error = Some(error.to_string());
                    engine.publish(&mut state);
                }
            }
        });
    }

    /// Opens the native image picker and stores a converted WebP as a pending cover for the
    /// registration. The editor commits it through `edit_local_game`; closing the editor calls
    /// [`Engine::discard_local_cover`], which removes an uncommitted file.
    pub async fn pick_local_cover(
        &self,
        entry_key: String,
    ) -> Result<Option<String>, gamenode_platform::covers::CoverImageError> {
        use gamenode_platform::covers::CoverImageError;
        let engine = self.clone();
        self.shared
            .runtime
            .spawn_blocking(move || engine.pick_local_cover_blocking(entry_key))
            .await
            .map_err(|_| CoverImageError::Picker)?
    }

    fn pick_local_cover_blocking(
        &self,
        entry_key: String,
    ) -> Result<Option<String>, gamenode_platform::covers::CoverImageError> {
        use gamenode_platform::covers::{self, CoverImageError};
        let id = registration_id(&entry_key);
        let store = self
            .shared
            .ports
            .local_games
            .as_ref()
            .ok_or(CoverImageError::Unsupported)?;
        let _registration = store
            .registrations()
            .map_err(|_| CoverImageError::Picker)?
            .into_iter()
            .find(|registration| registration.id == id && !registration.removed)
            .ok_or(CoverImageError::InvalidTarget)?;
        let epoch = self.lock().local_cover_epoch.get(&id).copied().unwrap_or(0);

        let Some(source) = covers::pick_cover_image()? else {
            return Ok(None);
        };
        let file = covers::reencode_local_cover(&self.shared.ports.covers_dir, &id, &source)?;

        // A closed editor, a removed registration or a newer save invalidates this pick.
        let current = store.registrations().ok().and_then(|registrations| {
            registrations
                .into_iter()
                .find(|registration| registration.id == id && !registration.removed)
        });
        let accepted = current.is_some()
            && self.lock().local_cover_epoch.get(&id).copied().unwrap_or(0) == epoch;
        if !accepted {
            let registrations = store.registrations().unwrap_or_default();
            let pending = self.lock().local_cover_pending.clone();
            remove_cover_if_unreferenced(self, &registrations, &pending, &file);
            return Ok(None);
        }

        let previous = {
            let mut state = self.lock();
            state.local_cover_pending.insert(id.clone(), file.clone())
        };
        if let Some(previous) = previous
            && previous != file
        {
            let registrations = store.registrations().unwrap_or_default();
            let pending = self.lock().local_cover_pending.clone();
            remove_cover_if_unreferenced(self, &registrations, &pending, &previous);
        }
        Ok(Some(file))
    }

    /// Drops an uncommitted cover for a closed editor and removes its file. It also invalidates a
    /// pick that is still running, so no orphan is written after the editor is gone.
    pub fn discard_local_cover(&self, entry_key: String) {
        let id = registration_id(&entry_key);
        let pending = {
            let mut state = self.lock();
            *state.local_cover_epoch.entry(id.clone()).or_default() += 1;
            state.local_cover_pending.remove(&id)
        };
        let Some(file) = pending else {
            return;
        };
        let Some(store) = self.shared.ports.local_games.as_ref() else {
            return;
        };
        let registrations = store.registrations().unwrap_or_default();
        let pending = self.lock().local_cover_pending.clone();
        remove_cover_if_unreferenced(self, &registrations, &pending, &file);
    }
}

impl Engine {
    pub(super) fn queue_local_backup(
        &self,
        state: &mut State,
        key: String,
        period: String,
        last_exit: i64,
        play_seconds: u32,
    ) {
        use gamenode_core::drm_free::scheduler::automatic_job;
        let Some(installation) = state.local.registrations.iter().find(|r| r.key() == key) else {
            return;
        };
        let Some(profile) = state
            .local
            .profiles
            .iter()
            .rfind(|p| p.installation == installation.id)
        else {
            return;
        };
        let Some(mut job) = automatic_job(
            gamenode_platform::random::session_id(),
            installation,
            profile,
            &state.local.settings,
            period,
            last_exit,
        ) else {
            return;
        };
        job.play_seconds = Some(play_seconds);
        if state.local.jobs.iter().any(|j| {
            j.installation == job.installation
                && j.profile == job.profile
                && j.revision == job.revision
                && j.play_period == job.play_period
        }) {
            return;
        }
        if let Some(store) = self.shared.ports.local_games.clone() {
            let engine = self.clone();
            self.shared.storage.submit(move || {
                if let Ok(jobs) = store.jobs() {
                    for mut old in jobs.into_iter().filter(|old| {
                        old.installation == job.installation
                            && old.profile == job.profile
                            && old.play_period.is_some()
                            && old.play_period != job.play_period
                            && old.state == gamenode_core::drm_free::JobState::Queued
                    }) {
                        old.state = gamenode_core::drm_free::JobState::Cancelled;
                        if let Err(error) = store.save_job(&old) {
                            tracing::warn!(%error, "could not coalesce previous automatic job");
                            return;
                        }
                    }
                }
                if let Err(error) = store.save_job(&job) {
                    tracing::warn!(%error,"could not persist backup job");
                    return;
                }
                let jobs = match store.jobs() {
                    Ok(jobs) => jobs,
                    Err(_) => return,
                };
                let mut state = engine.lock();
                state.local.jobs = jobs;
                engine.publish(&mut state);
            });
        }
    }
    pub(super) fn drive_local_backups(&self, state: &mut State, now: i64) {
        use gamenode_core::drm_free::{BackupOperation, BackupRequest, JobState};
        if state.local_backup_busy || self.shared.ports.backups.is_none() {
            return;
        }
        let Some(job) = state
            .local
            .jobs
            .iter()
            .find(|j| {
                j.state == JobState::Queued
                    && j.ready_at <= now
                    && state.local.profiles.iter().any(|p| {
                        p.id == j.profile
                            && p.revision == j.revision
                            && (j.play_period.is_none()
                                || (state.local.settings.automatic && p.automatic != Some(false)))
                    })
                    && state.local.registrations.iter().any(|r| {
                        r.id == j.installation
                            && !r.removed
                            && !state.playtime.tracker.running().contains(&r.key())
                    })
            })
            .cloned()
        else {
            return;
        };
        let Some(mut profile) = state
            .local
            .profiles
            .iter()
            .find(|p| p.id == job.profile && p.revision == job.revision)
            .cloned()
        else {
            return;
        };
        let retention = profile
            .retention
            .filter(|value| gamenode_core::drm_free::valid_retention(*value))
            .or_else(|| {
                Some(state.local.settings.retention)
                    .filter(|value| gamenode_core::drm_free::valid_retention(*value))
            })
            .unwrap_or(gamenode_core::drm_free::DEFAULT_RETENTION);
        profile.retention = Some(retention);
        let Some(installation) = state
            .local
            .registrations
            .iter()
            .find(|r| r.id == job.installation && !r.removed)
            .cloned()
        else {
            return;
        };
        if state
            .playtime
            .tracker
            .running()
            .contains(&installation.key())
        {
            return;
        }
        if job.play_period.is_some()
            && (!state.local.settings.automatic || profile.automatic == Some(false))
        {
            return;
        }
        state.local_backup_busy = true;
        let engine = self.clone();
        self.shared.runtime.spawn_blocking(move || {
            let store = engine.shared.ports.local_games.as_ref().unwrap();
            let mut job = job;
            let result =
                (|| {
                    let processes =
                        engine.shared.ports.processes.list().map_err(|_| {
                            LocalError::Failed("cannot confirm game is stopped".into())
                        })?;
                    let equal = |a: &str, b: &str| {
                        gamenode_core::provider::path::normalize(a).to_lowercase()
                            == gamenode_core::provider::path::normalize(b).to_lowercase()
                    };
                    if processes.iter().any(|p| {
                        installation
                            .gameplay
                            .iter()
                            .any(|exe| equal(exe, &p.exe_path))
                    }) {
                        return Err(LocalError::Failed("game is running".into()));
                    }
                    job.state = JobState::Running;
                    store.save_job(&job).map_err(local_storage)?;
                    let request = BackupRequest {
                        job_id: Some(job.id.clone()),
                        installation,
                        profile,
                        destination: job.destination.clone(),
                        operation: BackupOperation::Backup,
                    };
                    let adapter = engine.shared.ports.backups.as_ref().unwrap();
                    let report = adapter.execute(&request, &engine.shared.local_cancel)?;
                    if matches!(report.state, JobState::Successful | JobState::Unchanged) {
                        let history = adapter.execute(
                            &BackupRequest {
                                operation: BackupOperation::History,
                                ..request
                            },
                            &engine.shared.local_cancel,
                        )?;
                        let history: serde_json::Value = serde_json::from_str(&history.json)
                            .map_err(|e| LocalError::Failed(e.to_string()))?;
                        job.backup_id = history["games"]
                            .as_object()
                            .into_iter()
                            .flat_map(|games| games.values())
                            .flat_map(|g| g["backups"].as_array().into_iter().flatten())
                            .max_by_key(|b| b["when"].as_str())
                            .and_then(|b| b["name"].as_str())
                            .map(str::to_owned);
                    }
                    Ok(report)
                })();
            match result {
                Ok(report) => {
                    let changed = report.state == JobState::Successful;
                    job.state = report.state;
                    job.error = None;
                    if changed && let Some(backup_id) = job.backup_id.as_deref() {
                        let reason = if job.play_period.is_some() {
                            BackupReason::AfterSession
                        } else {
                            BackupReason::Manual
                        };
                        if let Err(error) = record_backup_meta(
                            store.as_ref(),
                            &job.installation,
                            &job.profile,
                            backup_id,
                            reason,
                            job.play_seconds,
                            unix_now(),
                        ) {
                            tracing::warn!(%error, "could not record backup origin");
                        }
                    }
                }
                Err(error) => gamenode_core::drm_free::scheduler::failed(
                    &mut job,
                    error.to_string(),
                    unix_now(),
                ),
            }
            if let Err(error) = store.save_job(&job) {
                tracing::warn!(%error,"could not save backup outcome");
            }
            let installation_id = job.installation.clone();
            let _views = engine.lock_local_views();
            let mut view = engine.load_local_view();
            let mut state = engine.lock();
            view.inferences = state.local.inferences.clone();
            view.recently_added = state.local.recently_added.clone();
            view.versions = state.local.versions.clone();
            view.history_bytes = state.local.history_bytes;
            view.history_count = state.local.history_count;
            view.restore = state.local.restore.clone();
            view.review_selected = state.local.review_selected.clone();
            view.discovery_incomplete = state.local.discovery_incomplete.clone();
            view.discovery_scanning = state.local.discovery_scanning;
            view.discovery_completed = state.local.discovery_completed;
            view.discovery_error = state.local.discovery_error.clone();
            view.discovery_last_checked_at = state
                .local
                .discovery_last_checked_at
                .or(view.discovery_last_checked_at);
            engine.apply_review_defaults(&mut state, &mut view);
            state.local = view;
            state.local_backup_busy = false;
            engine.publish(&mut state);
            drop(state);
            engine.request_discovery_suggestions();
            engine.load_local_history(installation_id);
        });
    }
}

impl Engine {
    pub fn enrich_local_games(&self) {
        let (account, credential, generation) = {
            let state = self.lock();
            (
                state.library.account().map(|a| a.id.clone()),
                state.machine.verified_credential().cloned(),
                state.local_generation,
            )
        };
        let engine = self.clone();
        self.shared.runtime.spawn_blocking(move || {
            use gamenode_core::drm_free::{Inference, identity};
            let Some((store, host)) = engine
                .shared
                .ports
                .local_games
                .as_ref()
                .zip(engine.shared.ports.installations.as_ref())
            else {
                return;
            };
            let data = engine
                .shared
                .ports
                .covers_dir
                .parent()
                .unwrap_or(std::path::Path::new("."));
            let _release = gamenode_platform::manifest_cache::release_on_drop();
            let snapshot = gamenode_platform::manifest_cache::ManifestCache::bundled(data)
                .and_then(|cache| cache.load())
                .ok();
            let existing = store.profiles().unwrap_or_default();
            let registrations = store.registrations().unwrap_or_default();
            for registration in registrations.into_iter().filter(|r| !r.removed).take(100) {
                let evidence = host.evidence(&registration).unwrap_or_default();
                let evidence = if let Some(snapshot) = &snapshot {
                    let root = std::path::Path::new(&registration.root);
                    let relative = std::path::Path::new(&registration.executable)
                        .strip_prefix(root)
                        .unwrap_or(std::path::Path::new(&registration.executable))
                        .to_string_lossy();
                    snapshot.index.corroborate(
                        &evidence,
                        &root.file_name().unwrap_or_default().to_string_lossy(),
                        &relative,
                    )
                } else {
                    evidence
                };
                let Some(identity) = identity::infer(&evidence) else {
                    continue;
                };
                if identity.strong
                    && !existing.iter().any(|p| p.installation == registration.id)
                    && let Some(snapshot) = &snapshot
                {
                    let profiles = snapshot.index.exact(&identity.title);
                    if let [game] = profiles.as_slice()
                        && let Ok(profile) = identity::profile(
                            game,
                            &registration,
                            &snapshot.hash,
                            gamenode_platform::random::session_id(),
                        )
                        && let Ok(profile) = engine.preview_local_profile(&registration, profile)
                        && let Err(error) = store.save_profile(&profile)
                    {
                        tracing::warn!(%error,"could not save inferred save profile");
                    }
                }
                let (Some(account), Some(credential)) = (account.clone(), credential.clone())
                else {
                    continue;
                };
                let external_hints = snapshot
                    .as_ref()
                    .filter(|_| identity.strong && !identity.conflict)
                    .and_then(|snapshot| {
                        let matches = snapshot.index.exact(&identity.title);
                        (matches.len() == 1).then(|| matches[0])
                    })
                    .map(|game| {
                        [(1, game.steam.clone()), (5, game.gog.clone())]
                            .into_iter()
                            .filter_map(|(category, id)| id.map(|id| (category, id)))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let e = engine.clone();
                let registration = registration.clone();
                let evidence = evidence.clone();
                engine.shared.runtime.spawn(async move {
                    let Ok(_permit) = e.shared.local_catalog.clone().acquire_owned().await else {
                        return;
                    };
                    if e.lock().local_generation != generation {
                        return;
                    }
                    let response = e
                        .shared
                        .ports
                        .games
                        .search_page(&credential, &identity.title)
                        .await;
                    let Ok(page) = response else {
                        return;
                    };
                    let mut external_games = Vec::new();
                    for (category, id) in external_hints {
                        let Ok(matches) = e
                            .shared
                            .ports
                            .games
                            .lookup_external(&credential, category, &[id])
                            .await
                        else {
                            return;
                        };
                        external_games.extend(matches.into_iter().map(|m| m.game));
                    }
                    let ids: std::collections::BTreeSet<_> =
                        external_games.iter().map(|g| g.id).collect();
                    let game_id = if ids.len() == 1 {
                        ids.first().copied()
                    } else if ids.is_empty() {
                        identity::unique_exact(&identity, &page.games, page.complete == Some(true))
                    } else {
                        None
                    };
                    let game = game_id
                        .and_then(|id| {
                            page.games
                                .iter()
                                .chain(external_games.iter())
                                .find(|g| g.id == id)
                        })
                        .map(|g| gamenode_core::library::GameRecord {
                            game_id: g.id,
                            name: g.name.clone(),
                            release_year: g.release_year,
                            cover_url: g.cover_url.clone(),
                            cover_file: None,
                            cover_failed_at: None,
                            platforms: g.platforms.clone(),
                        });
                    let valid = {
                        let state = e.lock();
                        state.local_generation == generation
                            && state.library.account().is_some_and(|a| a.id == account)
                            && state.local.registrations.iter().any(|r| {
                                r.id == registration.id
                                    && r.revision == registration.revision
                                    && !r.removed
                            })
                    };
                    if !valid {
                        return;
                    }
                    let inference = Inference {
                        installation: registration.id.clone(),
                        revision: registration.revision,
                        title: identity.title,
                        evidence,
                        game_id,
                        account: account.clone(),
                    };
                    let persist = e.clone();
                    let saved_game = game.clone();
                    let saved = e
                        .shared
                        .runtime
                        .spawn_blocking(move || {
                            if let Some(store) = &persist.shared.ports.local_games {
                                store.save_inference(&inference)?;
                            }
                            if let Some(game) = saved_game {
                                persist.shared.ports.library.save_games(&[game])?;
                            }
                            Ok::<_, StorageError>(())
                        })
                        .await;
                    if !matches!(saved, Ok(Ok(()))) {
                        return;
                    }
                    let mut state = e.lock();
                    if state.local_generation == generation
                        && state.library.account().is_some_and(|a| a.id == account)
                        && state.local.registrations.iter().any(|r| {
                            r.id == registration.id
                                && r.revision == registration.revision
                                && !r.removed
                        })
                    {
                        let effects =
                            state
                                .library
                                .set_local_inference(registration.key(), game_id, game);
                        for effect in effects {
                            e.library_effect(&mut state, effect);
                        }
                        e.publish(&mut state);
                        drop(state);
                        e.refresh_local_games();
                    }
                });
            }
            engine.refresh_local_games();
        });
    }
}

impl Engine {
    fn preview_local_profile(
        &self,
        installation: &Registration,
        mut profile: gamenode_core::drm_free::SaveProfile,
    ) -> Result<gamenode_core::drm_free::SaveProfile, LocalError> {
        use gamenode_core::drm_free::*;
        let backup = self
            .shared
            .ports
            .backups
            .as_ref()
            .ok_or(LocalError::Unsupported)?;
        let destination = self.load_local_view().settings.destination;
        let request = BackupRequest {
            job_id: None,
            installation: installation.clone(),
            profile: profile.clone(),
            destination,
            operation: BackupOperation::Preview,
        };
        let report = backup.execute(&request, &self.shared.local_cancel)?;
        if report.state == JobState::Partial {
            return Err(LocalError::Failed("save preview failed".into()));
        }
        let json: serde_json::Value =
            serde_json::from_str(&report.json).map_err(|e| LocalError::Failed(e.to_string()))?;
        let game = &json["games"][&profile.title];
        profile.files = game["files"]
            .as_object()
            .map(|files| files.keys().cloned().collect())
            .unwrap_or_default();
        profile.registry = game["registry"]
            .as_object()
            .map(|keys| keys.keys().cloned().collect())
            .unwrap_or_default();
        profile.support = if profile.files.is_empty() && profile.registry.is_empty() {
            SaveSupport::NoSaves
        } else {
            SaveSupport::Supported
        };
        Ok(profile)
    }
}

impl Engine {
    /// Space used by local backups, per game, read from the destination on this thread's caller.
    /// Callers must run it on a blocking worker.
    pub fn local_backup_usage(&self) -> Result<gamenode_core::drm_free::BackupUsage, LocalError> {
        use gamenode_core::drm_free::BackupUsage;
        let store = self
            .shared
            .ports
            .local_games
            .as_ref()
            .ok_or(LocalError::Unsupported)?;
        let mut settings = store.backup_settings().map_err(local_storage)?;
        if settings.destination.is_empty() {
            settings.destination = default_backup_destination(&self.shared.ports.covers_dir);
        }
        let registrations = store.registrations().map_err(local_storage)?;
        let profiles = store.profiles().map_err(local_storage)?;
        let mut usage = backup_usage_at(&settings.destination, &registrations, &profiles);
        let state = self.lock();
        for game in &mut usage.games {
            game.cover_file = state.library.cover_for(&game.entry_key);
        }
        usage.total_bytes = usage.games.iter().map(|game| game.bytes).sum();
        usage.total_versions = usage.games.iter().map(|game| game.versions).sum();
        Ok::<BackupUsage, LocalError>(usage)
    }

    pub fn set_local_backup_retention(&self, entry_key: String, retention: u8) {
        self.local_work(move |engine| {
            use gamenode_core::drm_free::{SaveProfile, valid_retention};
            if !valid_retention(retention) {
                return Err(LocalError::InvalidTarget);
            }
            let id = entry_key
                .strip_prefix("manual:")
                .unwrap_or(&entry_key)
                .to_owned();
            let store = engine
                .shared
                .ports
                .local_games
                .as_ref()
                .ok_or(LocalError::Unsupported)?;
            let profile = store
                .profiles()
                .map_err(local_storage)?
                .into_iter()
                .rfind(|p| p.installation == id)
                .ok_or(LocalError::InvalidTarget)?;
            store
                .save_profile(&SaveProfile {
                    retention: Some(retention),
                    ..profile
                })
                .map_err(local_storage)
        });
    }
    pub fn backup_local_now(&self, id: String) {
        self.local_work(move |engine| {
            use gamenode_core::drm_free::*;
            let store = engine
                .shared
                .ports
                .local_games
                .as_ref()
                .ok_or(LocalError::Unsupported)?;
            let profile = store
                .profiles()
                .map_err(local_storage)?
                .into_iter()
                .rfind(|p| p.installation == id)
                .ok_or(LocalError::InvalidTarget)?;
            if !matches!(
                profile.support,
                SaveSupport::Supported | SaveSupport::NoSaves
            ) {
                return Err(LocalError::InvalidTarget);
            }
            let job = BackupJob {
                id: gamenode_platform::random::session_id(),
                installation: id,
                profile: profile.id,
                revision: profile.revision,
                play_period: None,
                destination: engine.load_local_view().settings.destination,
                state: JobState::Queued,
                attempts: 0,
                ready_at: unix_now(),
                backup_id: None,
                error: None,
                play_seconds: None,
            };
            store.save_job(&job).map_err(local_storage)
        });
    }
    pub fn set_local_profile_automatic(&self, id: String, automatic: bool) {
        self.local_work(move |engine| {
            let store = engine
                .shared
                .ports
                .local_games
                .as_ref()
                .ok_or(LocalError::Unsupported)?;
            let mut profile = store
                .profiles()
                .map_err(local_storage)?
                .into_iter()
                .rfind(|p| p.installation == id)
                .ok_or(LocalError::InvalidTarget)?;
            profile.automatic = Some(automatic);
            store.save_profile(&profile).map_err(local_storage)?;
            if !automatic {
                for mut job in store
                    .jobs()
                    .map_err(local_storage)?
                    .into_iter()
                    .filter(|j| {
                        j.installation == id
                            && j.play_period.is_some()
                            && j.state == gamenode_core::drm_free::JobState::Queued
                    })
                {
                    job.state = gamenode_core::drm_free::JobState::Cancelled;
                    store.save_job(&job).map_err(local_storage)?;
                }
            }
            Ok(())
        });
    }
    pub fn configure_local_saves(&self, id: String, location: String) {
        self.local_work(move |engine| {
            use gamenode_core::drm_free::*;
            let store = engine
                .shared
                .ports
                .local_games
                .as_ref()
                .ok_or(LocalError::Unsupported)?;
            let installation = store
                .registrations()
                .map_err(local_storage)?
                .into_iter()
                .find(|r| r.id == id && !r.removed)
                .ok_or(LocalError::InvalidTarget)?;
            let path = std::fs::canonicalize(&location).map_err(|_| LocalError::InvalidTarget)?;
            let literal = gamenode_platform::backups::glob_literal(&path.to_string_lossy());
            let literal = if path.is_dir() {
                format!("{literal}/**")
            } else {
                literal
            };
            let mut files = serde_json::Map::new();
            files.insert(literal, serde_json::json!({}));
            let profile = SaveProfile {
                id: gamenode_platform::random::session_id(),
                installation: id,
                revision: 1,
                title: installation.title.clone(),
                manifest_hash: "custom".into(),
                definition: serde_json::json!({"files":files}).to_string(),
                files: vec![],
                registry: vec![],
                support: SaveSupport::NeedsSelection,
                automatic: None,
                retention: None,
            };
            let profile = engine.preview_local_profile(&installation, profile)?;
            gamenode_platform::backups::validate_destination(
                std::path::Path::new(&engine.load_local_view().settings.destination),
                &[path, std::path::PathBuf::from(&installation.root)],
            )?;
            store.save_profile(&profile).map_err(local_storage)
        });
    }
    pub fn watch_local_folder(&self, path: String) {
        self.local_work(move |engine| {
            let store = engine
                .shared
                .ports
                .local_games
                .as_ref()
                .ok_or(LocalError::Unsupported)?;
            let path = std::fs::canonicalize(path).map_err(|_| LocalError::InvalidTarget)?;
            if !path.is_dir() {
                return Err(LocalError::InvalidTarget);
            }
            let path =
                gamenode_platform::host::strip_verbatim_prefix(path.to_string_lossy().into_owned());
            let existing = store
                .watches()
                .map_err(local_storage)?
                .into_iter()
                .find(|w| w.path.eq_ignore_ascii_case(&path));
            store
                .save_watch(&gamenode_core::drm_free::WatchRoot {
                    id: existing
                        .map(|w| w.id)
                        .unwrap_or_else(gamenode_platform::random::session_id),
                    path,
                    enabled: true,
                    ..Default::default()
                })
                .map_err(local_storage)?;
            engine.rescan_local_folders();
            Ok(())
        });
    }
    pub(super) fn auto_register_candidates(
        &self,
        candidates: Vec<gamenode_core::drm_free::Candidate>,
    ) -> Result<Vec<String>, LocalError> {
        let store = self
            .shared
            .ports
            .local_games
            .as_ref()
            .ok_or(LocalError::Unsupported)?;
        let prior = store.candidates().map_err(local_storage)?;
        let mut registrations = store.registrations().map_err(local_storage)?;
        let store_roots: Vec<String> = {
            let state = self.lock();
            state
                .library
                .all_entries()
                .filter(|e| {
                    e.installed && e.provider != "manual" && !e.install_dir.trim().is_empty()
                })
                .map(|e| gamenode_core::provider::path::normalize(&e.install_dir).to_lowercase())
                .collect()
        };
        let mut added_titles = Vec::new();
        for mut candidate in candidates {
            if candidate.ignored
                || prior
                    .iter()
                    .any(|old| old.fingerprint == candidate.fingerprint && old.ignored)
                || registrations.iter().any(|r| {
                    candidate_matches_registration(&candidate, r)
                        && (!r.removed || !r.rediscovery_allowed)
                })
            {
                continue;
            }
            let cand_root =
                gamenode_core::provider::path::normalize(&candidate.root).to_lowercase();
            let inside_store_install = store_roots.iter().any(|root| {
                let sep = if gamenode_core::provider::path::is_windows_style(root) {
                    '\\'
                } else {
                    '/'
                };
                let trimmed = root.trim_end_matches(['/', '\\']);
                cand_root == trimmed || cand_root.starts_with(&format!("{trimmed}{sep}"))
            });
            if inside_store_install {
                continue;
            }
            if candidate.selected_target.is_none() {
                candidate.selected_target = gamenode_core::drm_free::identity::preferred_target(
                    &candidate.root,
                    &candidate.targets,
                    &[],
                );
            }
            candidate.likely_non_game =
                gamenode_core::drm_free::identity::likely_non_game(&candidate);
            store.save_candidate(&candidate).map_err(local_storage)?;
            if candidate.likely_non_game {
                // A likely non-game stays in the review unchecked; it is never added on its own.
                continue;
            }
            if let (Some(host), Some(target)) = (
                self.shared.ports.installations.as_ref(),
                candidate.selected_target.as_deref(),
            ) {
                let mut registration = host.inspect(target)?;
                registration.root = candidate.root.clone();
                registration.title = candidate.title.clone();
                if let Some(pos) = registrations.iter().position(|r| {
                    r.removed
                        && r.rediscovery_allowed
                        && (r.file_identity == registration.file_identity
                            || candidate_matches_registration(&candidate, r))
                }) {
                    registration.id = registrations[pos].id.clone();
                    registration.revision = registrations[pos].revision + 1;
                    store
                        .save_registration(&registration)
                        .map_err(local_storage)?;
                    registrations[pos] = registration;
                    if !added_titles.contains(&candidate.title) {
                        added_titles.push(candidate.title);
                    }
                } else if !registrations
                    .iter()
                    .any(|r| !r.removed && r.file_identity == registration.file_identity)
                {
                    store
                        .save_registration(&registration)
                        .map_err(local_storage)?;
                    registrations.push(registration);
                    if !added_titles.contains(&candidate.title) {
                        added_titles.push(candidate.title);
                    }
                }
            }
        }
        Ok(added_titles)
    }
    /// Checks every enabled watched folder on the bounded discovery worker.
    pub fn rescan_local_folders(&self) {
        self.start_local_rescan(None);
    }

    /// Checks one watched folder on the bounded discovery worker. An unavailable folder keeps
    /// its entries and is not traversed.
    pub fn check_local_watch(&self, id: String) {
        use std::sync::atomic::Ordering;
        if self.shared.local_discovering.load(Ordering::Acquire) {
            return;
        }
        {
            let mut state = self.lock();
            let Some(watch) = state
                .local
                .watches
                .iter_mut()
                .find(|watch| watch.id == id && watch.enabled)
            else {
                return;
            };
            if watch.state == gamenode_core::drm_free::WatchState::Unavailable {
                return;
            }
            watch.state = gamenode_core::drm_free::WatchState::Checking;
            if let Ok(mut ids) = self.shared.local_checking.lock() {
                ids.insert(id.clone());
            }
            self.publish(&mut state);
        }
        self.start_local_rescan(Some(id));
    }

    fn start_local_rescan(&self, only: Option<String>) {
        use std::sync::atomic::Ordering;
        if self.shared.local_discovering.swap(true, Ordering::AcqRel) {
            if only.is_none() {
                self.shared
                    .local_discovery_again
                    .store(true, Ordering::Release);
            }
            return;
        }
        {
            let mut state = self.lock();
            state.local.discovery_scanning = true;
            state.local.discovery_error = None;
            self.publish(&mut state);
        }
        let engine = self.clone();
        self.shared.runtime.spawn_blocking(move || {
            use gamenode_core::drm_free::*;
            let finish = |engine: &Engine| {
                if let Ok(mut ids) = engine.shared.local_checking.lock() {
                    if only.is_some() {
                        if let Some(only) = &only {
                            ids.remove(only);
                        }
                    } else {
                        ids.clear();
                    }
                }
            };
            let Some(store) = &engine.shared.ports.local_games else {
                {
                    let mut state = engine.lock();
                    state.local.discovery_scanning = false;
                    state.local.discovery_completed = true;
                    state.local.discovery_error = Some(LocalError::Unsupported.to_string());
                    engine.publish(&mut state);
                }
                finish(&engine);
                engine
                    .shared
                    .local_discovering
                    .store(false, Ordering::Release);
                return;
            };
            let mut added_titles = Vec::new();
            let result = (|| {
                // One parse serves every watched root; the scan drops it when it ends.
                let _release = gamenode_platform::manifest_cache::release_on_drop();
                let mut watches = store.watches().map_err(local_storage)?;
                watches.sort_by_key(|w| w.path.len());
                let mut enabled_watches: Vec<_> =
                    watches.into_iter().filter(|w| w.enabled).collect();
                if let Some(only) = &only {
                    enabled_watches.retain(|w| &w.id == only);
                }
                if let Ok(mut ids) = engine.shared.local_checking.lock() {
                    ids.extend(enabled_watches.iter().map(|w| w.id.clone()));
                }
                if engine.shared.ports.installations.is_some() {
                    let pending: Vec<_> = store
                        .candidates()
                        .map_err(local_storage)?
                        .into_iter()
                        .filter(|c| !c.ignored && enabled_watches.iter().any(|w| w.id == c.watch))
                        .collect();
                    if !pending.is_empty() {
                        added_titles.extend(engine.auto_register_candidates(pending)?);
                    }
                }
                let mut scanned_roots = Vec::<String>::new();
                let mut incomplete = Vec::new();
                let mut checked_ids = Vec::new();
                for watch in enabled_watches {
                    if !std::path::Path::new(&watch.path).is_dir() {
                        continue;
                    }
                    let normalized =
                        gamenode_core::provider::path::normalize(&watch.path).to_lowercase();
                    if scanned_roots.iter().any(|root| {
                        let sep = if gamenode_core::provider::path::is_windows_style(root) {
                            '\\'
                        } else {
                            '/'
                        };
                        normalized == *root
                            || normalized.starts_with(&format!(
                                "{}{sep}",
                                root.trim_end_matches(['/', '\\'])
                            ))
                    }) {
                        continue;
                    }
                    scanned_roots.push(normalized);
                    checked_ids.push(watch.id.clone());
                    let found = match gamenode_platform::discovery::WindowsDiscovery
                        .discover(&watch, &engine.shared.local_cancel)
                    {
                        Ok(found) => found,
                        Err(LocalError::Unsupported)
                            if engine.shared.ports.installations.is_some() =>
                        {
                            DiscoveryReport::default()
                        }
                        Err(error) => return Err(error),
                    };
                    if found.incomplete {
                        incomplete.push(watch.path.clone());
                    }
                    for title in engine.auto_register_candidates(found.candidates)? {
                        if !added_titles.contains(&title) {
                            added_titles.push(title);
                        }
                    }
                }
                engine.lock().local.discovery_incomplete = incomplete;
                Ok::<_, LocalError>(checked_ids)
            })();
            let checked_ids = result.clone().unwrap_or_default();
            {
                let mut state = engine.lock();
                for title in &added_titles {
                    if !state.local.recently_added.contains(title) {
                        state.local.recently_added.push(title.clone());
                    }
                }
                state.local.discovery_scanning = false;
                state.local.discovery_completed = true;
                state.local.discovery_error = result.err().map(|error| error.to_string());
                // The check completed (successfully or not): remember when, for the status bar
                // after a restart. A failed write leaves the previous value.
                let checked_at = unix_now();
                if let Err(error) = engine
                    .shared
                    .ports
                    .settings
                    .save_discovery_last_checked_at(checked_at)
                {
                    tracing::warn!(%error, "could not save the watched-folder check time");
                } else {
                    state.local.discovery_last_checked_at = Some(checked_at);
                }
                if !checked_ids.is_empty()
                    && let Ok(mut roots) = store.watches()
                {
                    for root in roots.iter_mut() {
                        if !checked_ids.iter().any(|id| id == &root.id) {
                            continue;
                        }
                        root.last_checked_at = Some(checked_at);
                        root.unavailable_since = None;
                        if let Err(error) = store.save_watch(root) {
                            tracing::warn!(%error, "could not save the watched-folder check time");
                        }
                    }
                }
                engine.publish(&mut state);
            }
            if !added_titles.is_empty() {
                engine.rescan();
                engine.enrich_local_games();
            }
            finish(&engine);
            engine.refresh_local_games();
            gamenode_platform::manifest_cache::release_loaded();
            engine
                .shared
                .local_discovering
                .store(false, Ordering::Release);
            if engine
                .shared
                .local_discovery_again
                .swap(false, Ordering::AcqRel)
            {
                engine.rescan_local_folders();
            }
        });
    }
    pub fn dismiss_local_discovered(&self) {
        let mut state = self.lock();
        if !state.local.recently_added.is_empty() {
            state.local.recently_added.clear();
            self.publish(&mut state);
        }
    }
    pub fn select_local_candidate(&self, fingerprint: String) {
        let mut state = self.lock();
        if let Some(index) = state
            .local
            .review_selected
            .iter()
            .position(|f| f == &fingerprint)
        {
            state.local.review_selected.remove(index);
        } else {
            state.local.review_selected.push(fingerprint);
        }
        self.publish(&mut state);
    }
    pub fn review_selected_local_candidates(&self, add: bool, allow_rediscovery: bool) {
        let fingerprints = self.lock().local.review_selected.clone();
        self.local_work(move |engine| {
            let store = engine
                .shared
                .ports
                .local_games
                .as_ref()
                .ok_or(LocalError::Unsupported)?;
            let host = engine
                .shared
                .ports
                .installations
                .as_ref()
                .ok_or(LocalError::Unsupported)?;
            // The review may show a suggestion that was published just before its persistence
            // finished; merge it so Add still records what the user saw.
            let visible_suggestions: std::collections::BTreeMap<
                String,
                gamenode_core::library::SuggestionView,
            > = engine
                .lock()
                .local
                .candidates
                .iter()
                .filter_map(|candidate| {
                    candidate
                        .suggestion
                        .clone()
                        .map(|suggestion| (candidate.fingerprint.clone(), suggestion))
                })
                .collect();
            let candidates = store.candidates().map_err(local_storage)?;
            for mut candidate in candidates
                .into_iter()
                .filter(|c| fingerprints.contains(&c.fingerprint) && !c.ignored)
            {
                if candidate.suggestion.is_none()
                    && let Some(suggestion) = visible_suggestions.get(&candidate.fingerprint)
                {
                    candidate.suggestion = Some(suggestion.clone());
                }
                if add {
                    let target = candidate
                        .selected_target
                        .as_ref()
                        .ok_or(LocalError::InvalidTarget)?;
                    let mut registration = host.inspect(target)?;
                    registration.root = candidate.root.clone();
                    registration.title = candidate.title.clone();
                    let active = store
                        .registrations()
                        .map_err(local_storage)?
                        .into_iter()
                        .find(|r| !r.removed && r.file_identity == registration.file_identity);
                    let registration = match active {
                        Some(active) => active,
                        None => {
                            store
                                .save_registration(&registration)
                                .map_err(local_storage)?;
                            registration
                        }
                    };
                    if let Some(suggestion) = candidate.suggestion.clone() {
                        apply_candidate_suggestion(
                            engine,
                            store.as_ref(),
                            &registration,
                            &candidate,
                            suggestion,
                        )?;
                    }
                }
                if !add && allow_rediscovery {
                    // Forget the record: a later scan can propose the same installation again.
                    store
                        .delete_candidate(&candidate.fingerprint)
                        .map_err(local_storage)?;
                } else {
                    candidate.ignored = true;
                    store.save_candidate(&candidate).map_err(local_storage)?;
                }
            }
            Ok(())
        });
    }
    pub fn reset_local_ignores(&self) {
        self.library_input(LibInput::ResetExcluded);
        self.rescan();
        if self.shared.ports.local_games.is_none() {
            return;
        }
        self.local_work(move |engine| {
            let store = engine
                .shared
                .ports
                .local_games
                .as_ref()
                .ok_or(LocalError::Unsupported)?;
            for mut registration in store
                .registrations()
                .map_err(local_storage)?
                .into_iter()
                .filter(|r| r.removed)
            {
                registration.rediscovery_allowed = true;
                store
                    .save_registration(&registration)
                    .map_err(local_storage)?;
            }
            for mut candidate in store.candidates().map_err(local_storage)? {
                candidate.ignored = false;
                candidate.likely_non_game =
                    gamenode_core::drm_free::identity::likely_non_game(&candidate);
                store.save_candidate(&candidate).map_err(local_storage)?;
            }
            engine.rescan_local_folders();
            Ok(())
        });
    }
    pub fn remove_local_watch(&self, id: String) {
        self.local_work(move |engine| {
            let store = engine
                .shared
                .ports
                .local_games
                .as_ref()
                .ok_or(LocalError::Unsupported)?;
            if let Some(mut watch) = store
                .watches()
                .map_err(local_storage)?
                .into_iter()
                .find(|w| w.id == id)
            {
                watch.enabled = false;
                store.save_watch(&watch).map_err(local_storage)?;
            }
            Ok(())
        });
    }
}

impl Engine {
    pub fn load_local_history(&self, id: String) {
        let engine = self.clone();
        self.shared.runtime.spawn_blocking(move || {
            use gamenode_core::drm_free::*;
            let result = (|| {
                let store = engine
                    .shared
                    .ports
                    .local_games
                    .as_ref()
                    .ok_or(LocalError::Unsupported)?;
                let backup = engine
                    .shared
                    .ports
                    .backups
                    .as_ref()
                    .ok_or(LocalError::Unsupported)?;
                let installation = store
                    .registrations()
                    .map_err(local_storage)?
                    .into_iter()
                    .find(|r| r.id == id)
                    .ok_or(LocalError::InvalidTarget)?;
                let profiles = store.profiles().map_err(local_storage)?;
                let jobs = store.jobs().map_err(local_storage)?;
                let meta = store.backup_version_meta(&id).map_err(local_storage)?;
                let mut namespaces = std::collections::BTreeSet::new();
                let mut recovery: Vec<(gamenode_core::drm_free::SaveProfile, String)> = Vec::new();
                let mut versions = Vec::new();
                for job in jobs.into_iter().filter(|j| j.installation == id) {
                    if !namespaces.insert((job.profile.clone(), job.destination.clone())) {
                        continue;
                    }
                    let Some(profile) = profiles.iter().find(|p| p.id == job.profile) else {
                        continue;
                    };
                    let request = BackupRequest {
                        job_id: Some(job.id.clone()),
                        installation: installation.clone(),
                        profile: profile.clone(),
                        destination: job.destination.clone(),
                        operation: BackupOperation::History,
                    };
                    collect_history(
                        backup.as_ref(),
                        &engine.shared.local_cancel,
                        &request,
                        &id,
                        &profile.id,
                        &meta,
                        &mut versions,
                    )?;
                    let directory = std::path::Path::new(&job.destination)
                        .join("manual")
                        .join(&id);
                    let prefix = format!("{}_recovery_", profile.id);
                    if let Ok(entries) = std::fs::read_dir(directory) {
                        for entry in entries.flatten() {
                            let name = entry.file_name().to_string_lossy().into_owned();
                            if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false)
                                || !name.starts_with(&prefix)
                                || recovery
                                    .iter()
                                    .any(|(known, at)| known.id == name && at == &job.destination)
                            {
                                continue;
                            }
                            let mut recovery_profile = profile.clone();
                            recovery_profile.id = name;
                            recovery.push((recovery_profile, job.destination.clone()));
                        }
                    }
                }
                for (profile, destination) in recovery {
                    collect_history(
                        backup.as_ref(),
                        &engine.shared.local_cancel,
                        &BackupRequest {
                            job_id: None,
                            installation: installation.clone(),
                            profile: profile.clone(),
                            destination,
                            operation: BackupOperation::History,
                        },
                        &id,
                        &profile.id,
                        &meta,
                        &mut versions,
                    )?;
                }
                versions.sort_by(|left, right| {
                    right
                        .created_at
                        .cmp(&left.created_at)
                        .then_with(|| right.label.cmp(&left.label))
                });
                let (bytes, count) = history_totals(&versions);
                Ok::<_, LocalError>((versions, bytes, count))
            })();
            let mut state = engine.lock();
            match result {
                Ok((versions, bytes, count)) => {
                    if state
                        .library
                        .view()
                        .is_some_and(|v| v.selected == vec![format!("manual:{id}")])
                    {
                        state.local.versions = versions;
                        state.local.history_bytes = bytes;
                        state.local.history_count = count;
                    }
                }
                Err(error) => state.local.error = Some(error.to_string()),
            }
            engine.publish(&mut state);
        });
    }
    pub fn preview_local_restore(&self, key: String) {
        let version = self
            .lock()
            .local
            .versions
            .iter()
            .find(|v| v.key() == key)
            .cloned();
        let Some(version) = version else {
            return;
        };
        let generation = self.lock().local_generation;
        let engine = self.clone();
        self.shared.runtime.spawn_blocking(move || {
            use gamenode_core::drm_free::*;
            let result = (|| {
                let store = engine
                    .shared
                    .ports
                    .local_games
                    .as_ref()
                    .ok_or(LocalError::Unsupported)?;
                let backup = engine
                    .shared
                    .ports
                    .backups
                    .as_ref()
                    .ok_or(LocalError::Unsupported)?;
                let installation = store
                    .registrations()
                    .map_err(local_storage)?
                    .into_iter()
                    .find(|r| r.id == version.installation && !r.removed)
                    .ok_or(LocalError::InvalidTarget)?;
                let profiles = store.profiles().map_err(local_storage)?;
                let mut profile = match profiles.iter().find(|p| p.id == version.profile) {
                    Some(profile) => profile.clone(),
                    None => profiles
                        .into_iter()
                        .rfind(|p| p.installation == version.installation)
                        .ok_or(LocalError::InvalidTarget)?,
                };
                profile.id = version.profile.clone();
                let mut request = BackupRequest {
                    job_id: None,
                    installation,
                    profile,
                    destination: version.destination,
                    operation: BackupOperation::RestorePreview {
                        backup: version.id.clone(),
                    },
                };
                let report = backup.execute(&request, &engine.shared.local_cancel)?;
                if report.state == JobState::Partial {
                    return Err(LocalError::Failed("restore preview failed".into()));
                }
                let json: serde_json::Value = serde_json::from_str(&report.json)
                    .map_err(|e| LocalError::Failed(e.to_string()))?;
                let game = &json["games"][&request.profile.title];
                let targets = ["files", "registry"]
                    .into_iter()
                    .flat_map(|kind| {
                        game[kind]
                            .as_object()
                            .into_iter()
                            .flat_map(|m| m.keys().cloned())
                    })
                    .collect();
                request.operation = BackupOperation::Restore { backup: version.id };
                Ok::<_, LocalError>(RestorePlan { request, targets })
            })();
            let mut state = engine.lock();
            if state.local_generation != generation {
                return;
            }
            match result {
                Ok(plan) => {
                    if state
                        .library
                        .view()
                        .is_some_and(|v| v.selected == vec![plan.request.installation.key()])
                    {
                        state.local.restore = Some(plan);
                    }
                }
                Err(error) => state.local.error = Some(error.to_string()),
            }
            engine.publish(&mut state);
        });
    }
    pub fn cancel_local_restore(&self) {
        let mut state = self.lock();
        if state.local.restore.is_some() {
            state.local.restore = None;
            self.publish(&mut state);
        }
    }
    pub fn confirm_local_restore(&self) {
        let plan = self.lock().local.restore.take();
        let Some(mut plan) = plan else {
            return;
        };
        let installation_id = plan.request.installation.id.clone();
        self.local_work(move |engine| {
            let backup = engine
                .shared
                .ports
                .backups
                .as_ref()
                .ok_or(LocalError::Unsupported)?;
            let store = engine
                .shared
                .ports
                .local_games
                .as_ref()
                .ok_or(LocalError::Unsupported)?;
            plan.request.installation = store
                .registrations()
                .map_err(local_storage)?
                .into_iter()
                .find(|r| r.id == plan.request.installation.id && !r.removed)
                .ok_or(LocalError::InvalidTarget)?;
            let report = backup.execute(&plan.request, &engine.shared.local_cancel)?;
            if report.state == gamenode_core::drm_free::JobState::Partial {
                return Err(LocalError::Failed("restore partially completed; recovery snapshot retained in the backup namespace".into()));
            }
            if report.state == gamenode_core::drm_free::JobState::Successful
                && let Err(error) = record_restore_recovery_meta(
                    store.as_ref(),
                    backup.as_ref(),
                    &plan.request,
                    &engine.shared.local_cancel,
                    unix_now(),
                )
            {
                tracing::warn!(%error, "could not record the recovery backup origin");
            }
            engine.load_local_history(installation_id);
            Ok(())
        });
    }
}

impl Engine {
    pub(super) fn start_local_discovery(&self) {
        #[cfg(windows)]
        {
            use std::sync::atomic::Ordering;
            if !self.shared.local_realtime.load(Ordering::Acquire)
                || self.shared.local_watcher.swap(true, Ordering::AcqRel)
            {
                return;
            }
            let weak = Arc::downgrade(&self.shared);
            self.shared.runtime.spawn_blocking(move || {
                let mut watchers = std::collections::BTreeMap::<
                    String,
                    (
                        String,
                        Option<gamenode_platform::discovery::FolderNotifications>,
                    ),
                >::new();
                let mut next_roots = Instant::now();
                // The startup reconcile already ran when the preference asked for it; the
                // watcher only reacts to changes and to the periodic safety reconcile.
                let mut reconcile = Instant::now() + Duration::from_secs(30 * 60);
                let mut changed = None;
                let mut was_scanning = false;
                let mut last_scan_end: Option<Instant> = None;
                loop {
                    let Some(shared) = weak.upgrade() else {
                        break;
                    };
                    if shared.local_cancel.load(Ordering::Relaxed)
                        || !shared.local_realtime.load(Ordering::Acquire)
                    {
                        shared.local_watcher.store(false, Ordering::Release);
                        break;
                    }
                    let now = Instant::now();
                    if now >= next_roots {
                        if let Some(store) = &shared.ports.local_games
                            && let Ok(mut roots) = store.watches()
                        {
                            // A reconnected drive becomes available again and is reconciled by
                            // the next scheduled pass below.
                            for root in roots.iter_mut() {
                                if root.enabled
                                    && root.unavailable_since.is_some()
                                    && std::path::Path::new(&root.path).is_dir()
                                {
                                    root.unavailable_since = None;
                                    if let Err(error) = store.save_watch(root) {
                                        tracing::warn!(%error, "could not save watch availability");
                                    }
                                    changed = Some(now);
                                }
                            }
                            watchers.retain(|id, _| roots.iter().any(|r| r.enabled && r.id == *id));
                            for root in roots.iter().filter(|r| r.enabled) {
                                let entry = watchers
                                    .entry(root.id.clone())
                                    .or_insert_with(|| (root.path.clone(), None));
                                if entry.0 != root.path {
                                    *entry = (root.path.clone(), None);
                                }
                                if entry.1.is_none() {
                                    entry.1 =
                                        gamenode_platform::discovery::FolderNotifications::new(
                                            &root.path,
                                        )
                                        .ok();
                                    if entry.1.is_some() {
                                        changed = Some(now);
                                    }
                                }
                            }
                        }
                        next_roots = now + Duration::from_secs(5);
                    }
                    if watchers
                        .values()
                        .any(|(_, w)| w.as_ref().is_some_and(|w| w.changed()))
                    {
                        changed = Some(now);
                    }
                    let scanning = shared.local_discovering.load(Ordering::Acquire);
                    if was_scanning && !scanning {
                        reconcile = now + Duration::from_secs(30 * 60);
                        last_scan_end = Some(now);
                    }
                    was_scanning = scanning;
                    if !scanning
                        && (now >= reconcile
                            // Filesystem events settle for a few seconds and never retrigger a
                            // scan more often than once a minute (games and installers write
                            // constantly inside watched folders).
                            || (changed.is_some_and(|at| {
                                now.duration_since(at) >= Duration::from_secs(5)
                            }) && last_scan_end
                                .is_none_or(|end| now.duration_since(end) >= Duration::from_secs(60))))
                    {
                        Engine {
                            shared: shared.clone(),
                        }
                        .rescan_local_folders();
                        changed = None;
                        reconcile = now + Duration::from_secs(30 * 60);
                    }
                    drop(shared);
                    std::thread::sleep(Duration::from_millis(200));
                }
            });
        }
    }
}

impl Engine {
    pub fn set_local_candidate_executable(&self, fingerprint: String, executable: String) {
        self.local_work(move |engine| {
            let store = engine
                .shared
                .ports
                .local_games
                .as_ref()
                .ok_or(LocalError::Unsupported)?;
            let mut candidate = store
                .candidates()
                .map_err(local_storage)?
                .into_iter()
                .find(|c| c.fingerprint == fingerprint)
                .ok_or(LocalError::InvalidTarget)?;
            let inspected = engine
                .shared
                .ports
                .installations
                .as_ref()
                .ok_or(LocalError::Unsupported)?
                .inspect(&executable)?;
            if !candidate
                .targets
                .iter()
                .any(|target| target.eq_ignore_ascii_case(&inspected.executable))
            {
                return Err(LocalError::InvalidTarget);
            }
            candidate.selected_target = Some(inspected.executable);
            candidate.likely_non_game =
                gamenode_core::drm_free::identity::likely_non_game(&candidate);
            store.save_candidate(&candidate).map_err(local_storage)
        });
    }
}

impl Engine {
    pub fn pick_local_watch_folder(&self) {
        let engine = self.clone();
        self.shared.runtime.spawn_blocking(move || {
            match gamenode_platform::installations::pick_save_location(true) {
                Ok(Some(path)) => engine.watch_local_folder(path),
                Ok(None) => {}
                Err(error) => {
                    let mut state = engine.lock();
                    state.local.error = Some(error.to_string());
                    engine.publish(&mut state);
                }
            }
        });
    }

    pub fn pick_local_saves(&self, id: String, folder: bool) {
        let engine = self.clone();
        self.shared.runtime.spawn_blocking(move || {
            match gamenode_platform::installations::pick_save_location(folder) {
                Ok(Some(location)) => engine.configure_local_saves(id, location),
                Ok(None) => {}
                Err(error) => {
                    let mut state = engine.lock();
                    state.local.error = Some(error.to_string());
                    engine.publish(&mut state);
                }
            }
        });
    }
    pub fn choose_local_save_profile(&self, id: String, title: String) {
        self.local_work(move |engine| {
            use gamenode_core::drm_free::identity;
            let _release = gamenode_platform::manifest_cache::release_on_drop();
            let store = engine
                .shared
                .ports
                .local_games
                .as_ref()
                .ok_or(LocalError::Unsupported)?;
            let installation = store
                .registrations()
                .map_err(local_storage)?
                .into_iter()
                .find(|r| r.id == id && !r.removed)
                .ok_or(LocalError::InvalidTarget)?;
            let data = engine
                .shared
                .ports
                .covers_dir
                .parent()
                .ok_or(LocalError::InvalidTarget)?;
            let snapshot =
                gamenode_platform::manifest_cache::ManifestCache::bundled(data)?.load()?;
            let matches = snapshot.index.exact(&title);
            let [game] = matches.as_slice() else {
                return Err(LocalError::Failed(
                    "choose one exact save profile title".into(),
                ));
            };
            let profile = identity::profile(
                game,
                &installation,
                &snapshot.hash,
                gamenode_platform::random::session_id(),
            )
            .map_err(LocalError::Failed)?;
            let profile = engine.preview_local_profile(&installation, profile)?;
            store.save_profile(&profile).map_err(local_storage)
        });
    }
}

impl Engine {
    pub(super) fn recover_local_jobs(&self) {
        let engine = self.clone();
        self.shared.runtime.spawn_blocking(move || {
            use gamenode_core::drm_free::JobState;
            let Some(store) = &engine.shared.ports.local_games else {
                return;
            };
            if let Ok(jobs) = store.jobs() {
                for mut job in jobs.into_iter().filter(|j| j.state == JobState::Running) {
                    job.state = JobState::Queued;
                    job.ready_at = unix_now();
                    if let Err(error) = store.save_job(&job) {
                        tracing::warn!(%error, "could not recover interrupted backup job");
                    }
                }
            }
            engine.refresh_local_games();
        });
    }
    pub(super) fn start_local_manifest_updates(&self) {
        if self.shared.ports.platform != Platform::Windows {
            return;
        }
        let weak = Arc::downgrade(&self.shared);
        self.shared.runtime.spawn(async move {
            let mut timer = tokio::time::interval(Duration::from_secs(86400));
            loop {
                timer.tick().await;
                let Some(shared) = weak.upgrade() else {
                    return;
                };
                if shared
                    .local_cancel
                    .load(std::sync::atomic::Ordering::Relaxed)
                {
                    return;
                }
                let engine = Engine { shared };
                let busy = {
                    let state = engine.lock();
                    state.local_backup_busy || !state.playtime.tracker.running().is_empty()
                };
                if busy {
                    continue;
                }
                let runtime = engine.shared.runtime.clone();
                runtime.spawn_blocking(move || {
                    let Some(data) = engine.shared.ports.covers_dir.parent() else {
                        return;
                    };
                    if let Ok(cache) =
                        gamenode_platform::manifest_cache::ManifestCache::bundled(data)
                        && let Err(error) = cache.refresh(unix_now())
                    {
                        tracing::debug!(%error, "manifest update unavailable");
                    }
                });
            }
        });
    }
}

impl Engine {
    /// Called with a successful process listing on its worker, before publishing tracker input.
    pub(super) fn learn_local_gameplay(
        &self,
        processes: &[gamenode_core::ports::ProcessInfo],
    ) -> Vec<Registration> {
        use gamenode_core::drm_free::identity;
        let Some((store, host)) = self
            .shared
            .ports
            .local_games
            .as_ref()
            .zip(self.shared.ports.installations.as_ref())
        else {
            return vec![];
        };
        let Ok(registrations) = store.registrations() else {
            return vec![];
        };
        let mut learned = Vec::new();
        for mut registration in registrations.into_iter().filter(|r| !r.removed) {
            let normalize = |p: &str| gamenode_core::provider::path::normalize(p).to_lowercase();
            let sep = if gamenode_core::provider::path::is_windows_style(&registration.root) {
                '\\'
            } else {
                '/'
            };
            let root = format!(
                "{}{sep}",
                normalize(&registration.root).trim_end_matches(['/', '\\'])
            );
            // Most scans contain only already-known executables. Avoid reading PE
            // resources until there is a new process beneath this installation.
            let candidates: Vec<_> = processes
                .iter()
                .filter(|process| {
                    normalize(&process.exe_path).starts_with(&root)
                        && !registration
                            .gameplay
                            .iter()
                            .chain(&registration.launchers)
                            .any(|exe| normalize(exe) == normalize(&process.exe_path))
                })
                .collect();
            if candidates.is_empty() {
                continue;
            }
            let Ok(evidence) = host.evidence(&registration) else {
                continue;
            };
            let Some(baseline) =
                identity::infer(&evidence).filter(|identity| identity.strong && !identity.conflict)
            else {
                continue;
            };
            let revision = registration.revision;
            for process in candidates {
                let Ok(mut candidate) = host.inspect(&process.exe_path) else {
                    continue;
                };
                candidate.root = registration.root.clone();
                let Ok(evidence) = host.evidence(&candidate) else {
                    continue;
                };
                let Some(identity) = identity::infer(&evidence) else {
                    continue;
                };
                if identity.strong
                    && !identity.conflict
                    && identity::normalized(&identity.title)
                        == identity::normalized(&baseline.title)
                    && !identity::maintenance(
                        std::path::Path::new(&process.exe_path)
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .as_ref(),
                    )
                    && !identity::launcher(&process.exe_path)
                {
                    registration.gameplay.push(candidate.executable);
                }
            }
            if registration.gameplay.is_empty() {
                continue;
            }
            registration.gameplay.sort();
            registration.gameplay.dedup();
            let Ok(current) = store.registrations() else {
                continue;
            };
            if current
                .iter()
                .any(|r| r.id == registration.id && r.gameplay == registration.gameplay)
            {
                continue;
            }
            registration.revision += 1;
            if store
                .save_registration_if_revision(&registration, revision)
                .unwrap_or(false)
            {
                learned.push(registration);
            }
        }
        learned
    }
}

impl Engine {
    pub fn resolve_local_claim(&self, transfer: bool) {
        let (claim, draft) = {
            let mut state = self.lock();
            (
                state.pending_claim.take(),
                state.pending_claim_registration.take(),
            )
        };
        let Some(claim) = claim else {
            return;
        };
        if !transfer {
            self.library_input(LibInput::Select {
                key: claim.store_key,
                mode: gamenode_core::library::SelectMode::Replace,
            });
            return;
        }
        let Some(mut registration) = draft else {
            return;
        };
        self.local_work(move |engine| {
            let store = engine
                .shared
                .ports
                .local_games
                .as_ref()
                .ok_or(LocalError::Unsupported)?;
            let host = engine
                .shared
                .ports
                .installations
                .as_ref()
                .ok_or(LocalError::Unsupported)?;
            registration.transferred_from = Some(claim.store_key);
            let registration = host.validate_registration(&registration)?;
            store
                .save_registration(&registration)
                .map_err(local_storage)?;
            engine.lock().local_selection = Some(registration.key());
            Ok(())
        });
    }
    pub fn cancel_local_claim(&self) {
        let mut state = self.lock();
        state.pending_claim = None;
        state.pending_claim_registration = None;
        self.publish(&mut state);
    }
}

#[cfg(test)]
mod backup_history_tests {
    use super::*;

    #[test]
    fn ludusavi_archive_times_parse_as_utc_seconds() {
        assert_eq!(backup_archive_time("1970-01-01 00:00:00"), Some(0));
        assert_eq!(
            backup_archive_time("2026-11-15 14:20:00"),
            Some(1_794_752_400)
        );
        assert_eq!(
            backup_archive_time("2026-11-15T14:20:00.123"),
            Some(1_794_752_400)
        );
        assert_eq!(backup_archive_time("not a date"), None);
        assert_eq!(backup_archive_time("2026-13-15 14:20:00"), None);
    }

    #[test]
    fn history_join_uses_meta_origin_and_archive_time_first() {
        let item = serde_json::json!({
            "name": "backup-1",
            "when": "2026-11-15 14:20:00"
        });
        let meta = BackupVersionMeta {
            installation: "game".into(),
            profile: "profile".into(),
            backup_id: "backup-1".into(),
            reason: BackupReason::AfterSession,
            play_seconds: Some(2_520),
            created_at: 1_700_000_000,
        };
        let version =
            history_version("game", "profile", "/backups", &item, 7_100_000, Some(&meta)).unwrap();
        assert_eq!(version.created_at, Some(1_794_752_400));
        assert_eq!(version.reason, Some(BackupReason::AfterSession));
        assert_eq!(version.play_seconds, Some(2_520));

        let legacy =
            history_version("game", "profile", "/backups", &item, 7_100_000, None).unwrap();
        assert_eq!(legacy.created_at, Some(1_794_752_400));
        assert_eq!(legacy.reason, None);
        assert_eq!(legacy.play_seconds, None);

        let unresolved = serde_json::json!({"name": "old", "when": "yesterday"});
        let unresolved =
            history_version("game", "profile", "/backups", &unresolved, 10, Some(&meta)).unwrap();
        assert_eq!(
            unresolved.created_at,
            Some(1_700_000_000),
            "meta time is the fallback when the archive time is unreadable"
        );
        assert_eq!(unresolved.reason, Some(BackupReason::AfterSession));
    }

    struct HistoryOnly(serde_json::Value);
    impl gamenode_core::drm_free::BackupEngine for HistoryOnly {
        fn execute(
            &self,
            _: &gamenode_core::drm_free::BackupRequest,
            _: &std::sync::atomic::AtomicBool,
        ) -> Result<gamenode_core::drm_free::BackupReport, LocalError> {
            Ok(gamenode_core::drm_free::BackupReport {
                state: gamenode_core::drm_free::JobState::Successful,
                json: self.0.to_string(),
            })
        }
    }

    #[test]
    fn recovery_origin_is_written_only_when_the_recovery_backup_changed() {
        use gamenode_core::drm_free::{
            BackupRequest, BackupVersionMeta, Registration, SaveProfile, SaveSupport,
        };
        let profile = SaveProfile {
            id: "profile".into(),
            installation: "game".into(),
            revision: 1,
            title: "Fixture".into(),
            manifest_hash: "hash".into(),
            definition: "{}".into(),
            files: vec![],
            registry: vec![],
            support: SaveSupport::Supported,
            automatic: None,
            retention: None,
        };
        let request = BackupRequest {
            job_id: None,
            profile: profile.clone(),
            installation: Registration {
                id: "game".into(),
                title: "Fixture".into(),
                executable: "/games/fixture/fixture.exe".into(),
                file_identity: "fixture".into(),
                root: "/games/fixture".into(),
                working_directory: "/games/fixture".into(),
                arguments: vec![],
                gameplay: vec![],
                launchers: vec![],
                revision: 1,
                removed: false,
                rediscovery_allowed: false,
                transferred_from: None,
                custom_cover: None,
                available: true,
            },
            destination: String::new(),
            operation: gamenode_core::drm_free::BackupOperation::Restore {
                backup: "selected".into(),
            },
        };
        let history = HistoryOnly(serde_json::json!({
            "games": {
                "Fixture": {
                    "backups": [{
                        "name": "recovery-backup",
                        "when": "2026-11-15 14:20:00"
                    }]
                }
            }
        }));
        for (marker_state, expected_rows) in [("successful", 1), ("unchanged", 0)] {
            let fixture = tempfile::tempdir().unwrap();
            let destination = fixture.path().to_string_lossy().into_owned();
            let namespace = fixture.path().join("manual").join("game").join("profile");
            std::fs::create_dir_all(&namespace).unwrap();
            std::fs::write(
                namespace.join(".gamenode-restore-recovery.json"),
                serde_json::json!({
                    "recovery": fixture
                        .path()
                        .join("manual")
                        .join("game")
                        .join("profile_recovery_1")
                        .to_string_lossy(),
                    "state": marker_state
                })
                .to_string(),
            )
            .unwrap();
            let database = gamenode_platform::db::SqliteSettingsStore::new(
                gamenode_platform::db::open(std::path::Path::new(":memory:")).unwrap(),
            );
            let store = database.local_game_store();
            let mut request = request.clone();
            request.destination = destination;
            record_restore_recovery_meta(
                &store,
                &history,
                &request,
                &std::sync::atomic::AtomicBool::new(false),
                100,
            )
            .unwrap();
            let rows = store.backup_version_meta("game").unwrap();
            assert_eq!(rows.len(), expected_rows, "{marker_state}");
            if marker_state == "successful" {
                assert_eq!(
                    rows[0],
                    BackupVersionMeta {
                        installation: "game".into(),
                        profile: "profile_recovery_1".into(),
                        backup_id: "recovery-backup".into(),
                        reason: BackupReason::Recovery,
                        play_seconds: None,
                        created_at: 100,
                    }
                );
            }
        }
    }

    #[test]
    fn history_totals_count_recovery_versions_and_bytes() {
        let version = |id: &str, bytes: u64, reason: Option<BackupReason>| BackupVersion {
            installation: "game".into(),
            profile: "profile".into(),
            destination: "/backups".into(),
            id: id.into(),
            label: "when".into(),
            bytes,
            created_at: Some(1),
            reason,
            play_seconds: None,
        };
        let versions = vec![
            version("ordinary", 4_000, Some(BackupReason::AfterSession)),
            version("manual", 2_000, Some(BackupReason::Manual)),
            version("recovery", 1_000, Some(BackupReason::Recovery)),
        ];
        assert_eq!(history_totals(&versions), (7_000, 3));
        assert_eq!(history_totals(&[]), (0, 0));
    }
}
