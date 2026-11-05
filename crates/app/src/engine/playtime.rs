//! Drives tracking and counter effects while the window is hidden or visible.
use super::*;
use gamenode_core::playtime::{
    PlaytimeSource, Total,
    submitter::{Candidate, Effect as SubmitEffect, Submitter},
};
use gamenode_core::ports::ProcessError;
use gamenode_core::sync::SyncPreferences;
use gamenode_core::tracker::{Effect as TrackEffect, Entry, Input as TrackInput, Tracker};

pub(super) struct Playtime {
    pub tracker: Tracker,
    pub preferences: SyncPreferences,
    pub source: PlaytimeSource,
    pub available: bool,
    pub error: bool,
    pub(super) ready: bool,
    stopped: bool,
    generation: u64,
    pub(super) listing: bool,
    reading: bool,
    submitting: bool,
    resubmit: bool,
    submitter: Submitter,
    next_list: i64,
    next_checkpoint: i64,
    next_scan: i64,
    retries: Vec<i64>,
    owner: Option<String>,
    display_totals: Vec<Total>,
    display_sessions: Vec<gamenode_core::playtime::Session>,
    display_loaded: bool,
    display_loading: bool,
    display_dirty: bool,
}
impl Playtime {
    pub fn new(platform: Platform) -> Self {
        Self {
            tracker: Tracker::new(platform == Platform::Windows),
            preferences: SyncPreferences::default(),
            source: PlaytimeSource::Store,
            available: true,
            error: false,
            ready: false,
            stopped: false,
            generation: 0,
            listing: false,
            reading: false,
            submitting: false,
            resubmit: false,
            submitter: Submitter::default(),
            next_list: 0,
            next_checkpoint: 0,
            next_scan: 0,
            retries: Vec::new(),
            owner: None,
            display_totals: Vec::new(),
            display_sessions: Vec::new(),
            display_loaded: false,
            display_loading: false,
            display_dirty: false,
        }
    }
}
impl Engine {
    pub(super) fn start_playtime(&self) -> Result<(), StorageError> {
        let store = &self.shared.ports.playtime;
        store.recover()?;
        let preferences = store.load_sync()?;
        let source = store.load_source()?;
        let floor = self
            .shared
            .ports
            .library
            .load_last_account()?
            .and_then(|account| store.sessions(&account.id).ok())
            .and_then(|sessions| sessions.iter().filter_map(|s| s.ended_at).max());
        let mut state = self.lock();
        state.playtime.preferences = preferences;
        state.playtime.source = source;
        if let Some(floor) = floor {
            state.playtime.tracker.set_floor(floor);
        }
        state.playtime.ready = true;
        Ok(())
    }
    pub(super) fn sync_tracker(&self, state: &mut State) {
        if !state.playtime.ready || state.playtime.stopped {
            return;
        }
        let account = state.library.account().map(|a| a.id.clone());
        let entries = state
            .library
            .installed_entries()
            .filter(|entry| {
                !state.local.registrations.iter().any(|r| {
                    !r.removed && r.transferred_from.as_deref() == Some(entry.key.as_str())
                })
            })
            .map(|e| {
                let provider = state.library.provider(&e.provider);
                Entry {
                    key: e.key.clone(),
                    install_dir: e.install_dir.clone(),
                    source: provider.map_or_else(|| e.provider.clone(), |p| p.source().into()),
                    counter: state
                        .playtime
                        .source
                        .uses_counter(provider.is_some_and(|p| p.manifest().playtime.is_some())),
                    game_id: state.library.match_status(e).game_id(),
                    title: e.name.clone().unwrap_or_else(|| e.store_id.clone()),
                }
            })
            .collect();
        let changed = account != state.playtime.owner;
        if changed {
            self.cancel_submissions(state);
            state.playtime.display_totals.clear();
            state.playtime.display_sessions.clear();
            state.playtime.display_loaded = false;
        }
        state.playtime.tracker.set_policies(
            state
                .local
                .registrations
                .iter()
                .filter(|r| !r.removed)
                .map(|r| {
                    (
                        r.key(),
                        gamenode_core::drm_free::ProcessPolicy::Explicit {
                            gameplay: r.gameplay.clone(),
                            launchers: r.launchers.clone(),
                        },
                    )
                })
                .chain(
                    state
                        .local
                        .registrations
                        .iter()
                        .filter(|r| !r.removed)
                        .filter_map(|r| {
                            r.transferred_from
                                .clone()
                                .map(|key| (key, gamenode_core::drm_free::ProcessPolicy::Ignored))
                        }),
                )
                .collect(),
        );
        let effects = state.playtime.tracker.update(
            TrackInput::Library {
                account: account.clone(),
                entries,
                at: unix_now(),
            },
            gamenode_platform::random::session_id,
        );
        self.track_effects(state, effects);
        state.playtime.owner = account;
        if changed {
            self.refresh_playtime_display(state);
        }
        state.library.set_running(if state.playtime.available {
            state.playtime.tracker.running()
        } else {
            Default::default()
        });
    }
    fn track_effects(&self, state: &mut State, effects: Vec<TrackEffect>) {
        let changed_history = effects.iter().any(|e| {
            matches!(
                e,
                TrackEffect::OpenSession(_)
                    | TrackEffect::Checkpoint { .. }
                    | TrackEffect::CloseSession { .. }
            )
        });
        for effect in effects {
            match effect {
                TrackEffect::GameStopped {
                    key,
                    play_period,
                    last_gameplay_exit,
                } => {
                    self.queue_local_backup(state, key, play_period, last_gameplay_exit);
                }
                TrackEffect::Watch(process) => {
                    let adapter = self.shared.ports.processes.clone();
                    let weak = Arc::downgrade(&self.shared);
                    let runtime = self.shared.runtime.clone();
                    self.shared.runtime.spawn_blocking(move || {
                        let exited = process.clone();
                        if adapter
                            .watch(
                                &process,
                                Box::new(move |at| {
                                    runtime.spawn(async move {
                                        if let Some(shared) = weak.upgrade() {
                                            let engine = Engine { shared };
                                            let at = at
                                                .duration_since(UNIX_EPOCH)
                                                .map_or(0, |d| d.as_secs() as i64);
                                            engine.tracker_input(TrackInput::Exited {
                                                process: exited,
                                                at,
                                            });
                                        }
                                    });
                                }),
                            )
                            .is_err()
                        {
                            tracing::debug!("process wait unavailable; using listing fallback");
                        }
                    });
                }
                TrackEffect::ReadCounter { .. } => {
                    let now = unix_now();
                    state.playtime.retries.extend([now + 60, now + 300]);
                }
                effect => {
                    let store = self.shared.ports.playtime.clone();
                    let account = state.playtime.owner.clone();
                    self.shared.storage.submit(move || {
                        let result = match effect {
                            TrackEffect::OpenSession(row) => store.open_session(&row),
                            TrackEffect::Checkpoint { id, at } => account
                                .as_deref()
                                .map_or(Ok(()), |a| store.checkpoint(a, &id, at)),
                            TrackEffect::CloseSession {
                                id,
                                ended_at,
                                game_id,
                            } => account
                                .as_deref()
                                .map_or(Ok(()), |a| store.close_session(a, &id, ended_at, game_id)),
                            _ => Ok(()),
                        };
                        if let Err(error) = result {
                            tracing::warn!(%error,"could not save playtime");
                        }
                    });
                }
            }
        }
        if changed_history {
            self.refresh_playtime_display(state);
        }
    }
    fn tracker_input(&self, input: TrackInput) {
        let mut state = self.lock();
        if state.playtime.stopped {
            return;
        }
        let effects = state
            .playtime
            .tracker
            .update(input, gamenode_platform::random::session_id);
        self.track_effects(&mut state, effects);
        let running = state.playtime.tracker.running();
        state.library.set_running(running);
        self.read_counters(&mut state);
        self.publish(&mut state);
    }
    /// Called by the UI's repeating timer, which continues while the Windows window is hidden.
    pub fn maintenance(&self) {
        self.maintenance_at(unix_now());
    }
    pub fn maintenance_at(&self, now: i64) {
        let mut state = self.lock();
        self.drive_local_backups(&mut state, now);
        if !state.playtime.ready || state.playtime.stopped {
            return;
        }
        self.sync_tracker(&mut state);
        if now >= state.playtime.next_checkpoint {
            state.playtime.next_checkpoint = now + 60;
            let effects = state.playtime.tracker.update(
                TrackInput::Tick { now },
                gamenode_platform::random::session_id,
            );
            self.track_effects(&mut state, effects);
        }
        if now >= state.playtime.next_scan {
            state.playtime.next_scan = now + 1800;
            self.run_library(&mut state, LibInput::ScanRequested);
        }
        if state.playtime.available
            && state.library.account().is_some()
            && !state.playtime.listing
            && now >= state.playtime.next_list
        {
            state.playtime.listing = true;
            state.playtime.next_list = now + 5;
            let generation = state.playtime.generation;
            let engine = self.clone();
            let adapter = self.shared.ports.processes.clone();
            self.shared.runtime.spawn_blocking(move || {
                let result = adapter.list();
                let learned = result
                    .as_ref()
                    .map(|processes| engine.learn_local_gameplay(processes))
                    .unwrap_or_default();
                let mut state = engine.lock();
                state.playtime.listing = false;
                if generation != state.playtime.generation || state.playtime.stopped {
                    return;
                }
                if !learned.is_empty() {
                    for registration in learned {
                        if let Some(current) =
                            state.local.registrations.iter_mut().find(|r| {
                                r.id == registration.id && r.revision < registration.revision
                            })
                        {
                            *current = registration;
                        }
                    }
                    engine.sync_tracker(&mut state);
                }
                match result {
                    Ok(processes) => {
                        let effects = state.playtime.tracker.update(
                            TrackInput::Listed { at: now, processes },
                            gamenode_platform::random::session_id,
                        );
                        engine.track_effects(&mut state, effects);
                        let running = state.playtime.tracker.running();
                        state.library.set_running(running);
                    }
                    Err(ProcessError::Unsupported) => {
                        state.playtime.available = false;
                        state.library.set_running(Default::default());
                    }
                    Err(ProcessError::Failed) => tracing::debug!("process listing failed"),
                }
                engine.publish(&mut state);
            });
        }
        if state.playtime.retries.iter().any(|at| *at <= now) {
            state.playtime.retries.retain(|at| *at > now);
            self.read_counters(&mut state);
        }
        self.publish(&mut state);
    }
    pub fn window_shown(&self) {
        self.rescan();
    }
    pub fn set_sync(&self, enabled: bool) {
        let mut state = self.lock();
        state.playtime.preferences.enabled = enabled;
        self.cancel_submissions(&mut state);
        let preferences = state.playtime.preferences;
        let store = self.shared.ports.playtime.clone();
        self.shared.storage.submit(move || {
            if let Err(error) = store.save_sync(preferences) {
                tracing::warn!(%error,"could not save sync preference");
            }
        });
        self.submit_totals(&mut state);
        self.publish(&mut state);
    }
    pub fn dismiss_sync_explanation(&self) {
        let mut state = self.lock();
        state.playtime.preferences.explained = true;
        let preferences = state.playtime.preferences;
        let store = self.shared.ports.playtime.clone();
        self.shared.storage.submit(move || {
            if let Err(error) = store.save_sync(preferences) {
                tracing::warn!(%error,"could not save sync explanation");
            }
        });
        self.submit_totals(&mut state);
        self.publish(&mut state);
    }
    pub fn set_playtime_source(&self, source: PlaytimeSource) {
        let mut state = self.lock();
        if source == state.playtime.source {
            return;
        }
        let now = unix_now();
        let effects = state.playtime.tracker.update(
            TrackInput::Stop { at: now },
            gamenode_platform::random::session_id,
        );
        self.track_effects(&mut state, effects);
        state.playtime.tracker.set_floor(now);
        state.playtime.source = source;
        self.cancel_submissions(&mut state);
        let store = self.shared.ports.playtime.clone();
        self.shared.storage.submit(move || {
            if let Err(error) = store.save_source(source) {
                tracing::warn!(%error,"could not save playtime source");
            }
        });
        self.sync_tracker(&mut state);
        state.playtime.next_list = 0;
        self.read_counters(&mut state);
        self.submit_totals(&mut state);
        self.publish(&mut state);
    }
    fn cancel_submissions(&self, state: &mut State) {
        state.playtime.generation += 1;
        state.playtime.submitter.cancel();
        state.playtime.submitting = false;
        state.playtime.resubmit = false;
    }
    /// Close sessions before account state changes, preserving the old owner for storage writes.
    pub(super) fn stop_tracking(&self, state: &mut State) {
        let effects = state.playtime.tracker.update(
            TrackInput::Stop { at: unix_now() },
            gamenode_platform::random::session_id,
        );
        self.track_effects(state, effects);
        self.cancel_submissions(state);
        state.library.set_running(Default::default());
    }
    pub fn shutdown(&self) {
        self.shared
            .local_cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
        {
            let mut state = self.lock();
            self.stop_tracking(&mut state);
            state.playtime.stopped = true;
        }
        let (tx, rx) = mpsc::channel();
        self.shared.storage.submit(move || {
            let _ = tx.send(());
        });
        if rx.recv_timeout(Duration::from_secs(2)).is_err() {
            tracing::warn!("playtime storage flush timed out");
        }
    }
    pub(super) fn read_counters(&self, state: &mut State) {
        if !state.playtime.ready
            || state.playtime.stopped
            || state.playtime.reading
            || state.playtime.source != PlaytimeSource::Store
        {
            return;
        }
        let Some(account) = state.library.account().map(|a| a.id.clone()) else {
            return;
        };
        state.playtime.reading = true;
        let generation = state.playtime.generation;
        let providers = state.library.providers().to_vec();
        let engine = self.clone();
        let ports = self.shared.ports.clone();
        self.shared.runtime.spawn_blocking(move || {
            let mut values = Vec::new();
            for provider in providers {
                let Some(counter) = provider
                    .manifest()
                    .playtime
                    .as_ref()
                    .and_then(|p| gamenode_core::provider::registry::counter(&p.counter))
                else {
                    continue;
                };
                let roots = provider.resolve_roots(ports.platform, &*ports.roots);
                for (id, value) in counter(&gamenode_core::provider::registry::ScanInput {
                    files: &*ports.files,
                    roots: &roots,
                }) {
                    values.push((format!("{}:{id}", provider.id()), value));
                }
            }
            let mut state = engine.lock();
            state.playtime.reading = false;
            if generation != state.playtime.generation
                || state.library.account().map(|a| a.id.as_str()) != Some(account.as_str())
                || state.playtime.stopped
            {
                engine.read_counters(&mut state);
                return;
            }
            let store = ports.playtime.clone();
            let engine2 = engine.clone();
            engine.shared.storage.submit(move || {
                let result = (|| -> Result<(), StorageError> {
                    let old = store.totals(&account)?;
                    for (key, value) in values {
                        let mut total =
                            old.iter()
                                .find(|t| t.entry_key == key)
                                .cloned()
                                .unwrap_or(Total {
                                    account_id: account.clone(),
                                    entry_key: key,
                                    total_seconds: 0,
                                    last_played_at: None,
                                    accepted_game_id: None,
                                    accepted_seconds: None,
                                    rejected_seconds: None,
                                });
                        total.total_seconds = value.seconds;
                        total.last_played_at = Some(value.last_played);
                        store.save_total(&total)?;
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    tracing::warn!(%error,"could not save store totals");
                    return;
                }
                let mut state = engine2.lock();
                engine2.submit_totals(&mut state);
                engine2.refresh_playtime_display(&mut state);
            });
        });
    }
    pub(super) fn submit_totals(&self, state: &mut State) {
        if state.playtime.submitting {
            state.playtime.resubmit = true;
            return;
        }
        if !state.playtime.ready
            || state.playtime.stopped
            || state.playtime.submitting
            || state.playtime.source != PlaytimeSource::Store
            || !state
                .playtime
                .preferences
                .may_submit(state.machine.verified_credential().is_some())
        {
            return;
        }
        let Some(account) = state.library.account().map(|a| a.id.clone()) else {
            return;
        };
        state.playtime.submitting = true;
        let generation = state.playtime.generation;
        let store = self.shared.ports.playtime.clone();
        let engine = self.clone();
        self.shared.storage.submit(move || {
            let totals = store.totals(&account);
            let mut state = engine.lock();
            if generation != state.playtime.generation || state.playtime.stopped {
                return;
            }
            let Ok(totals) = totals else {
                state.playtime.submitting = false;
                return;
            };
            let candidates = totals
                .into_iter()
                .filter_map(|total| {
                    let entry = state.library.entry(&total.entry_key)?;
                    let provider = state.library.provider(&entry.provider)?;
                    provider.manifest().playtime.as_ref()?;
                    Some(Candidate {
                        game_id: state.library.match_status(entry).game_id(),
                        source: provider.source().into(),
                        total,
                    })
                })
                .collect();
            let preferences = state.playtime.preferences;
            let effects = state.playtime.submitter.start_for_source(
                PlaytimeSource::Store,
                &account,
                preferences,
                true,
                candidates,
            );
            engine.submit_effects(&mut state, effects);
        });
    }
    fn submit_effects(&self, state: &mut State, effects: Vec<SubmitEffect>) {
        let saved = effects.iter().any(|e| matches!(e, SubmitEffect::Save(_)));
        state.playtime.submitting = effects
            .iter()
            .any(|e| matches!(e, SubmitEffect::Send { .. }));
        for effect in effects {
            match effect {
                SubmitEffect::Save(total) => {
                    let store = self.shared.ports.playtime.clone();
                    self.shared.storage.submit(move || {
                        let result = (|| -> Result<(), StorageError> {
                            let mut latest = store
                                .totals(&total.account_id)?
                                .into_iter()
                                .find(|t| t.entry_key == total.entry_key)
                                .unwrap_or_else(|| total.clone());
                            latest.accepted_game_id = total.accepted_game_id;
                            latest.accepted_seconds = total.accepted_seconds;
                            latest.rejected_seconds = total.rejected_seconds;
                            store.save_total(&latest)
                        })();
                        if let Err(error) = result {
                            tracing::warn!(%error,"could not save accepted playtime");
                        }
                    });
                }
                SubmitEffect::SessionRejected => {
                    let engine = self.clone();
                    self.shared.runtime.spawn(async move {
                        engine.dispatch(Input::SessionRejected);
                    });
                }
                SubmitEffect::Send { submission, .. } => {
                    let Some(credential) = state.machine.verified_credential().cloned() else {
                        continue;
                    };
                    let generation = state.playtime.generation;
                    let engine = self.clone();
                    let api = self.shared.ports.playtime_api.clone();
                    self.shared.runtime.spawn(async move {
                        let result = api.submit(&credential, &submission).await;
                        let mut state = engine.lock();
                        if generation != state.playtime.generation || state.playtime.stopped {
                            return;
                        }
                        state.playtime.error = result.is_err();
                        if result == Err(gamenode_core::playtime::SubmitError::Rejected) {
                            tracing::warn!("GameNode rejected a playtime total");
                        }
                        let effects = state.playtime.submitter.complete(result);
                        engine.submit_effects(&mut state, effects);
                        engine.publish(&mut state);
                    });
                }
            }
        }
        if !state.playtime.submitting && std::mem::take(&mut state.playtime.resubmit) && saved {
            self.submit_totals(state);
        }
    }
    pub(super) fn update_playtime_view(&self, state: &mut State, now: i64) {
        let Some(account) = state.library.account().map(|a| a.id.clone()) else {
            state.library.set_playtime(Default::default());
            return;
        };
        let values = state
            .library
            .installed_entries()
            .map(|entry| {
                let counter = state
                    .library
                    .provider(&entry.provider)
                    .is_some_and(|p| p.manifest().playtime.is_some());
                let source = if state.playtime.source.uses_counter(counter) {
                    PlaytimeSource::Store
                } else {
                    PlaytimeSource::Internal
                };
                let view = if state.playtime.display_loaded {
                    gamenode_core::playtime::view_for_entry(
                        &account,
                        &entry.key,
                        source,
                        &state.playtime.display_totals,
                        &state.playtime.display_sessions,
                        now,
                    )
                } else {
                    gamenode_core::playtime::PlaytimeView {
                        seconds: None,
                        source,
                    }
                };
                (entry.key.clone(), view)
            })
            .collect();
        state.library.set_playtime(values);
    }
    fn refresh_playtime_display(&self, state: &mut State) {
        if !state.playtime.ready || state.playtime.stopped {
            return;
        }
        if state.playtime.display_loading {
            state.playtime.display_dirty = true;
            return;
        }
        let Some(account) = state.library.account().map(|a| a.id.clone()) else {
            return;
        };
        state.playtime.display_loading = true;
        let generation = state.playtime.generation;
        let store = self.shared.ports.playtime.clone();
        let engine = self.clone();
        self.shared.storage.submit(move || {
            let result = store
                .totals(&account)
                .and_then(|totals| store.sessions(&account).map(|sessions| (totals, sessions)));
            let mut state = engine.lock();
            state.playtime.display_loading = false;
            if state.playtime.stopped {
                return;
            }
            let stale = generation != state.playtime.generation
                || state.library.account().map(|a| a.id.as_str()) != Some(account.as_str());
            if !stale {
                match result {
                    Ok((totals, sessions)) => {
                        state.playtime.display_totals = totals;
                        state.playtime.display_sessions = sessions;
                        state.playtime.display_loaded = true;
                    }
                    Err(error) => tracing::warn!(%error,"could not load playtime for display"),
                }
                engine.publish(&mut state);
            }
            if std::mem::take(&mut state.playtime.display_dirty) || stale {
                engine.refresh_playtime_display(&mut state);
            }
        });
    }
}
