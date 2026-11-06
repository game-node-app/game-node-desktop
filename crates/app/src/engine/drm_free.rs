//! Short UI commands; filesystem and persistence work run on blocking workers.
use super::*;
use gamenode_core::drm_free::{LocalError, LocalView, Registration};
impl Engine {
    pub fn refresh_local_games(&self) {
        let engine = self.clone();
        self.shared.runtime.spawn_blocking(move || {
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
                            engine.library_effect(&state, effect);
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
            view.restore = state.local.restore.clone();
            view.review_selected = state.local.review_selected.clone();
            view.recently_added = state.local.recently_added.clone();
            view.discovery_incomplete = state.local.discovery_incomplete.clone();
            view.discovery_scanning = state.local.discovery_scanning;
            view.discovery_completed = state.local.discovery_completed;
            view.discovery_error = state.local.discovery_error.clone();
            state.local = view;
            engine.publish(&mut state);
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
                        .map(|entry| gamenode_core::drm_free::PendingClaim {
                            registration: registration.clone(),
                            store_key: entry.key.clone(),
                            store_title: entry
                                .name
                                .clone()
                                .unwrap_or_else(|| entry.store_id.clone()),
                        })
                };
                if let Some(claim) = claim {
                    engine.lock().pending_claim = Some(claim);
                    return Ok(());
                }
                store
                    .save_registration(&registration)
                    .map_err(local_storage)?;
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
            let mut inspected = host.inspect(&value.executable)?;
            inspected.id = value.id;
            inspected.title = value.title;
            inspected.root = value.root;
            inspected.working_directory = value.working_directory;
            inspected.arguments = value.arguments;
            inspected.gameplay = value.gameplay;
            inspected.launchers = value.launchers;
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
            store
                .save_registration(&replacement)
                .map_err(local_storage)?;
            Ok(())
        });
    }
    fn local_work(&self, work: impl FnOnce(&Engine) -> Result<(), LocalError> + Send + 'static) {
        {
            let mut state = self.lock();
            if state.local.busy {
                return;
            }
            state.local_generation += 1;
            state.local.busy = true;
            state.local.error = None;
            self.publish(&mut state);
        }
        let engine = self.clone();
        self.shared.runtime.spawn_blocking(move || {
            let result = work(&engine);
            let view = engine.load_local_view();
            {
                let mut state = engine.lock();
                let mut view = view;
                view.inferences = state.local.inferences.clone();
                view.versions = state.local.versions.clone();
                view.restore = state.local.restore.clone();
                view.recently_added = state.local.recently_added.clone();
                view.discovery_incomplete = state.local.discovery_incomplete.clone();
                view.discovery_scanning = state.local.discovery_scanning;
                view.discovery_completed = state.local.discovery_completed;
                view.discovery_error = state.local.discovery_error.clone();
                view.review_selected = state
                    .local
                    .review_selected
                    .iter()
                    .filter(|id| {
                        view.candidates
                            .iter()
                            .any(|candidate| !candidate.ignored && &candidate.fingerprint == *id)
                    })
                    .cloned()
                    .collect();
                state.local = view;
                state.local.error = result.err().map(|e| e.to_string());
                engine.publish(&mut state);
            }
            engine.rescan();
            engine.enrich_local_games();
        });
    }
    fn load_local_view(&self) -> LocalView {
        let Some(store) = &self.shared.ports.local_games else {
            return LocalView::default();
        };
        let result = (|| {
            let watches = store.watches()?;
            let registrations = store.registrations()?;
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
                .collect();
            Ok::<_, StorageError>(LocalView {
                watches,
                registrations,
                candidates,
                profiles: store.profiles()?,
                jobs: store.jobs()?,
                settings: {
                    let mut settings = store.backup_settings()?;
                    if settings.destination.is_empty() {
                        settings.destination = self
                            .shared
                            .ports
                            .covers_dir
                            .parent()
                            .unwrap_or(std::path::Path::new("."))
                            .join("backups")
                            .to_string_lossy()
                            .into_owned();
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
}
fn local_storage(error: StorageError) -> LocalError {
    LocalError::Failed(error.to_string())
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
}

impl Engine {
    pub(super) fn queue_local_backup(
        &self,
        state: &mut State,
        key: String,
        period: String,
        last_exit: i64,
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
        let Some(job) = automatic_job(
            gamenode_platform::random::session_id(),
            installation,
            profile,
            &state.local.settings,
            period,
            last_exit,
        ) else {
            return;
        };
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
        let Some(profile) = state
            .local
            .profiles
            .iter()
            .find(|p| p.id == job.profile && p.revision == job.revision)
            .cloned()
        else {
            return;
        };
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
                    job.state = report.state;
                    job.error = None;
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
            let mut view = engine.load_local_view();
            let mut state = engine.lock();
            view.inferences = state.local.inferences.clone();
            view.recently_added = state.local.recently_added.clone();
            view.versions = state.local.versions.clone();
            view.restore = state.local.restore.clone();
            view.review_selected = state.local.review_selected.clone();
            view.discovery_incomplete = state.local.discovery_incomplete.clone();
            view.discovery_scanning = state.local.discovery_scanning;
            view.discovery_completed = state.local.discovery_completed;
            view.discovery_error = state.local.discovery_error.clone();
            state.local = view;
            state.local_backup_busy = false;
            engine.publish(&mut state);
            drop(state);
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
                            e.library_effect(&state, effect);
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
    pub fn set_local_backup_settings(&self, automatic: bool, destination: String) {
        self.local_work(move |engine| {
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
            store
                .save_backup_settings(&gamenode_core::drm_free::BackupSettings {
                    automatic,
                    destination,
                })
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
            let path = path.to_string_lossy().into_owned();
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
            store.save_candidate(&candidate).map_err(local_storage)?;
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
    pub fn rescan_local_folders(&self) {
        use std::sync::atomic::Ordering;
        if self.shared.local_discovering.swap(true, Ordering::AcqRel) {
            self.shared
                .local_discovery_again
                .store(true, Ordering::Release);
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
            let Some(store) = &engine.shared.ports.local_games else {
                {
                    let mut state = engine.lock();
                    state.local.discovery_scanning = false;
                    state.local.discovery_completed = true;
                    state.local.discovery_error = Some(LocalError::Unsupported.to_string());
                    engine.publish(&mut state);
                }
                engine
                    .shared
                    .local_discovering
                    .store(false, Ordering::Release);
                return;
            };
            let mut added_titles = Vec::new();
            let result = (|| {
                let mut watches = store.watches().map_err(local_storage)?;
                watches.sort_by_key(|w| w.path.len());
                let enabled_watches: Vec<_> = watches.into_iter().filter(|w| w.enabled).collect();
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
                for watch in enabled_watches {
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
                Ok::<_, LocalError>(())
            })();
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
                engine.publish(&mut state);
            }
            if !added_titles.is_empty() {
                engine.rescan();
                engine.enrich_local_games();
            }
            engine.refresh_local_games();
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
    pub fn review_selected_local_candidates(&self, add: bool) {
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
            let candidates = store.candidates().map_err(local_storage)?;
            for mut candidate in candidates
                .into_iter()
                .filter(|c| fingerprints.contains(&c.fingerprint) && !c.ignored)
            {
                if add {
                    let target = candidate
                        .selected_target
                        .as_ref()
                        .ok_or(LocalError::InvalidTarget)?;
                    let mut registration = host.inspect(target)?;
                    registration.root = candidate.root.clone();
                    registration.title = candidate.title.clone();
                    if !store
                        .registrations()
                        .map_err(local_storage)?
                        .iter()
                        .any(|r| !r.removed && r.file_identity == registration.file_identity)
                    {
                        store
                            .save_registration(&registration)
                            .map_err(local_storage)?;
                    }
                }
                candidate.ignored = true;
                store.save_candidate(&candidate).map_err(local_storage)?;
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
                let mut namespaces = std::collections::BTreeSet::new();
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
                    let report = backup.execute(&request, &engine.shared.local_cancel)?;
                    let json: serde_json::Value = serde_json::from_str(&report.json)
                        .map_err(|e| LocalError::Failed(e.to_string()))?;
                    if let Some(backups) = json["games"][&profile.title]["backups"].as_array() {
                        for item in backups {
                            let Some(name) = item["name"].as_str() else {
                                continue;
                            };
                            let preview = backup.execute(
                                &BackupRequest {
                                    operation: BackupOperation::RestorePreview {
                                        backup: name.to_owned(),
                                    },
                                    ..request.clone()
                                },
                                &engine.shared.local_cancel,
                            )?;
                            let size: serde_json::Value = serde_json::from_str(&preview.json)
                                .map_err(|e| LocalError::Failed(e.to_string()))?;
                            versions.push(BackupVersion {
                                installation: id.clone(),
                                profile: profile.id.clone(),
                                destination: job.destination.clone(),
                                id: name.into(),
                                label: item["when"].as_str().unwrap_or(name).to_owned(),
                                bytes: size["overall"]["totalBytes"].as_u64().unwrap_or(0),
                            });
                        }
                    }
                }
                Ok::<_, LocalError>(versions)
            })();
            let mut state = engine.lock();
            match result {
                Ok(versions) => {
                    if state
                        .library
                        .view()
                        .is_some_and(|v| v.selected == vec![format!("manual:{id}")])
                    {
                        state.local.versions = versions;
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
                let profile = store
                    .profiles()
                    .map_err(local_storage)?
                    .into_iter()
                    .find(|p| p.id == version.profile)
                    .ok_or(LocalError::InvalidTarget)?;
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
            engine.load_local_history(installation_id);
            Ok(())
        });
    }
}

impl Engine {
    pub(super) fn start_local_discovery(&self) {
        #[cfg(windows)]
        {
            let weak = Arc::downgrade(&self.shared);
            self.shared.runtime.spawn_blocking(move || {
                use std::sync::atomic::Ordering;
                let mut watchers = std::collections::BTreeMap::<
                    String,
                    (
                        String,
                        Option<gamenode_platform::discovery::FolderNotifications>,
                    ),
                >::new();
                let mut next_roots = Instant::now();
                let mut reconcile = Instant::now();
                let mut changed = None;
                let mut was_scanning = false;
                loop {
                    let Some(shared) = weak.upgrade() else {
                        break;
                    };
                    if shared.local_cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let now = Instant::now();
                    if now >= next_roots {
                        if let Some(store) = &shared.ports.local_games
                            && let Ok(roots) = store.watches()
                        {
                            watchers.retain(|id, _| roots.iter().any(|r| r.enabled && r.id == *id));
                            for root in roots.into_iter().filter(|r| r.enabled) {
                                let entry = watchers
                                    .entry(root.id)
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
                    }
                    was_scanning = scanning;
                    if !scanning
                        && (now >= reconcile
                            || changed
                                .is_some_and(|at| now.duration_since(at) >= Duration::from_secs(1)))
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
                        && let Err(error) = cache.update(unix_now())
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
        for mut registration in registrations
            .into_iter()
            .filter(|r| !r.removed && !r.launchers.is_empty())
        {
            let Ok(evidence) = host.evidence(&registration) else {
                continue;
            };
            let Some(baseline) =
                identity::infer(&evidence).filter(|identity| identity.strong && !identity.conflict)
            else {
                continue;
            };
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
            let revision = registration.revision;
            for process in processes {
                if !normalize(&process.exe_path).starts_with(&root)
                    || registration
                        .gameplay
                        .iter()
                        .chain(&registration.launchers)
                        .any(|exe| normalize(exe) == normalize(&process.exe_path))
                {
                    continue;
                }
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
        let claim = self.lock().pending_claim.take();
        let Some(mut claim) = claim else {
            return;
        };
        if !transfer {
            self.library_input(LibInput::Select {
                key: claim.store_key,
                mode: gamenode_core::library::SelectMode::Replace,
            });
            return;
        }
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
            claim.registration.transferred_from = Some(claim.store_key);
            let registration = host.validate_registration(&claim.registration)?;
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
        self.publish(&mut state);
    }
}
