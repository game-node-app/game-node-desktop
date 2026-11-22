//! Drives tracking and counter effects while the window is hidden or visible.
use super::*;
use gamenode_core::playtime::{
    PlaytimeSource, Total,
    submitter::{Candidate, Effect as SubmitEffect, Submitter, group_candidates},
};
use gamenode_core::ports::ProcessError;
use gamenode_core::sync::{SyncNowError, SyncPreferences, SyncSummary, SyncView};
use gamenode_core::tracker::{
    Effect as TrackEffect, Entry, Input as TrackInput, Tracker, runtime_sessions::RuntimeSessions,
};

/// How long a rejected total waits before a normal automatic pass retries it. An explicit
/// "Sync now" is itself the next opportunity and does not wait.
const REJECTED_RETRY_AFTER_SECS: i64 = 300;

pub(super) struct Playtime {
    pub tracker: Tracker,
    pub runtime_sessions: RuntimeSessions,
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
    /// One submission pass is reading totals or sending them. A new pass waits for it.
    pass_running: bool,
    /// "Sync now" was accepted and its pass has not ended yet.
    sync_now_running: bool,
    /// "Sync now" is waiting for the rescan it requested to finish.
    sync_now_awaiting_scan: bool,
    /// "Sync now" is waiting for the counter read that follows the rescan.
    sync_now_awaiting_counters: bool,
    /// A counter read was already in flight when the scan finished, so one more read is needed
    /// before the requested submission pass.
    sync_now_counters_again: bool,
    /// The next automatic pass may retry totals GameNode rejected, after a quiet period.
    rejected_retry_at: Option<i64>,
    /// This sync-now request may retry a rejected total. Consumed by the first pass it starts,
    /// so a resubmit inside the same request cannot hammer GameNode.
    sync_now_retry_allowed: bool,
    /// The last completed pass, for the signed-in account.
    pub summary: Option<SyncSummary>,
    /// The account `summary` belongs to, so a late read never lands on another account.
    summary_for: Option<String>,
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
    pub(super) fn is_stopped(&self) -> bool {
        self.stopped
    }

    pub fn new(platform: Platform) -> Self {
        Self {
            tracker: Tracker::new(platform == Platform::Windows),
            runtime_sessions: RuntimeSessions::default(),
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
            pass_running: false,
            sync_now_running: false,
            sync_now_awaiting_scan: false,
            sync_now_awaiting_counters: false,
            sync_now_counters_again: false,
            rejected_retry_at: None,
            sync_now_retry_allowed: false,
            summary: None,
            summary_for: None,
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

    /// The snapshot's sync block: the stored preferences, the last completed pass and whether one
    /// runs now.
    pub fn view(&self) -> SyncView {
        SyncView {
            enabled: self.preferences.enabled,
            explained: self.preferences.explained,
            summary: self.summary,
            running: self.pass_running || self.sync_now_running || self.submitting,
        }
    }
}

/// Package-manager games declare an exact executable (for example `/usr/bin/0ad`) while their
/// install root is the shared `/usr/bin`. Without this exact policy the tracker matches every
/// process under `/usr/bin`, so a long-running shell can be attributed to an arbitrary package.
/// The field only narrows process matching; it is not launch authority.
fn package_executable_policy(
    entry: &gamenode_core::library::StoredEntry,
) -> Option<(String, gamenode_core::drm_free::ProcessPolicy)> {
    let executable = entry.fields.get("executable")?.trim();
    if executable.is_empty() {
        return None;
    }
    Some((
        entry.key.clone(),
        gamenode_core::drm_free::ProcessPolicy::Explicit {
            gameplay: vec![executable.to_owned()],
            launchers: Vec::new(),
        },
    ))
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
    /// Feeds the tracker the current library and returns whether anything changed: the tracked
    /// account, the tracker's own output, or the set of running games. The caller decides whether
    /// that warrants publishing a new snapshot.
    pub(super) fn sync_tracker(&self, state: &mut State) -> bool {
        if !state.playtime.ready || state.playtime.stopped {
            return false;
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
            let runtime_effects = state.playtime.runtime_sessions.reset();
            let runtime_effects = runtime_track_effects(runtime_effects);
            self.track_effects(state, runtime_effects);
            self.cancel_submissions(state);
            state.playtime.display_totals.clear();
            state.playtime.display_sessions.clear();
            state.playtime.display_loaded = false;
            state.playtime.summary = None;
            state.playtime.summary_for = account.clone();
            if let Some(account) = account.clone() {
                self.load_sync_summary(account);
            }
        }
        // Package-manager entries share a broad install root such as `/usr/bin`, so a prefix
        // match would attribute any process under that root (a shell, for example) to an
        // arbitrary package. A declared executable is an exact process policy and takes
        // precedence over that broad root. Local registrations are inserted last and still win.
        let mut policies = std::collections::BTreeMap::new();
        for entry in state.library.installed_entries() {
            if let Some((key, policy)) = package_executable_policy(entry) {
                policies.insert(key, policy);
            }
        }
        for registration in state.local.registrations.iter().filter(|r| !r.removed) {
            policies.insert(registration.key(), registration.process_policy());
        }
        for registration in state.local.registrations.iter().filter(|r| !r.removed) {
            if let Some(key) = &registration.transferred_from {
                policies.insert(key.clone(), gamenode_core::drm_free::ProcessPolicy::Ignored);
            }
        }
        state.playtime.tracker.set_policies(policies);
        let effects = state.playtime.tracker.update(
            TrackInput::Library {
                account: account.clone(),
                entries,
                at: unix_now(),
            },
            gamenode_platform::random::session_id,
        );
        let tracker_changed = self.track_process_effects(state, effects);
        state.playtime.owner = account;
        if changed {
            self.refresh_playtime_display(state);
        }
        let running_changed = state.library.set_running(if state.playtime.available {
            state.playtime.tracker.running_started_at()
        } else {
            Default::default()
        });
        changed || tracker_changed || running_changed
    }
    /// Runs the tracker effects and returns whether there were any: any effect is a real change
    /// worth publishing, even when it only writes history in the background.
    pub(super) fn track_effects(&self, state: &mut State, effects: Vec<TrackEffect>) -> bool {
        let changed = !effects.is_empty();
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
                    play_seconds,
                } => {
                    self.schedule_exited_game_achievements(&key);
                    self.queue_local_backup(
                        state,
                        key,
                        play_period,
                        last_gameplay_exit,
                        play_seconds,
                    );
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
        changed
    }
    fn track_process_effects(&self, state: &mut State, effects: Vec<TrackEffect>) -> bool {
        let keys: std::collections::BTreeSet<_> = effects
            .iter()
            .filter_map(|effect| match effect {
                TrackEffect::OpenSession(session) => Some(session.entry_key.clone()),
                _ => None,
            })
            .collect();
        let runtime_effects = keys
            .into_iter()
            .flat_map(|key| state.playtime.runtime_sessions.fence_entry(&key))
            .collect();
        let runtime_changed = self.track_effects(state, runtime_track_effects(runtime_effects));
        self.track_effects(state, effects) || runtime_changed
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
        self.track_process_effects(&mut state, effects);
        let running = state.playtime.tracker.running_started_at();
        state.library.set_running(running);
        self.read_counters(&mut state);
        self.publish(&mut state);
    }
    /// Called by the UI's repeating timer, which continues while the Windows window is hidden.
    pub fn maintenance(&self) {
        self.tool_auto_check_at(std::time::Instant::now());
        self.maintenance_at(unix_now());
    }
    pub fn maintenance_at(&self, now: i64) {
        let mut state = self.lock();
        self.drive_local_backups(&mut state, now);
        if !state.playtime.ready || state.playtime.stopped {
            return;
        }
        // Each sub-step reports whether it changed something. An idle tick publishes nothing:
        // rebuilding the whole library just to refresh the tray tooltip is wasteful, and the UI
        // already holds the previous snapshot.
        let mut changed = self.sync_tracker(&mut state);
        let expired_runtime = state
            .playtime
            .runtime_sessions
            .expire(std::time::Instant::now());
        if !expired_runtime.is_empty() {
            changed |= self.track_effects(&mut state, runtime_track_effects(expired_runtime));
        }
        if now >= state.playtime.next_checkpoint {
            state.playtime.next_checkpoint = now + 60;
            let effects = state.playtime.tracker.update(
                TrackInput::Tick { now },
                gamenode_platform::random::session_id,
            );
            changed |= self.track_process_effects(&mut state, effects);
        }
        if now >= state.playtime.next_scan {
            state.playtime.next_scan = now + 1800;
            self.run_library(&mut state, LibInput::ScanRequested);
            // A scan changes the view (`scanning`) and starts work, even before it finds games.
            changed = true;
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
                let mut changed = false;
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
                    changed = true;
                }
                match result {
                    Ok(processes) => {
                        let effects = state.playtime.tracker.update(
                            TrackInput::Listed { at: now, processes },
                            gamenode_platform::random::session_id,
                        );
                        changed |= engine.track_effects(&mut state, effects);
                        let running = if state.playtime.available {
                            state.playtime.tracker.running_started_at()
                        } else {
                            Default::default()
                        };
                        changed |= state.library.set_running(running);
                    }
                    Err(ProcessError::Unsupported) => {
                        state.playtime.available = false;
                        state.library.set_running(Default::default());
                        // The snapshot reports availability, so this is a real change even when
                        // no game was running.
                        changed = true;
                    }
                    Err(ProcessError::Failed) => tracing::debug!("process listing failed"),
                }
                // A running game keeps publishing: its displayed time advances every tick.
                if changed
                    || (state.playtime.available && !state.playtime.tracker.running().is_empty())
                {
                    engine.publish(&mut state);
                }
            });
        }
        if state.playtime.retries.iter().any(|at| *at <= now) {
            state.playtime.retries.retain(|at| *at > now);
            changed |= self.read_counters(&mut state);
        }
        // While a game runs its live playtime changes; keep the periodic snapshot then.
        if changed || (state.playtime.available && !state.playtime.tracker.running().is_empty()) {
            self.publish(&mut state);
        }
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
        self.apply_favorites_gate(&mut state);
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
        self.apply_favorites_gate(&mut state);
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
        let runtime_effects = state.playtime.runtime_sessions.reset();
        self.track_effects(&mut state, runtime_track_effects(runtime_effects));
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
        state.playtime.pass_running = false;
        state.playtime.sync_now_running = false;
        state.playtime.sync_now_awaiting_scan = false;
        state.playtime.sync_now_awaiting_counters = false;
        state.playtime.sync_now_counters_again = false;
        state.playtime.sync_now_retry_allowed = false;
    }
    /// Close sessions before account state changes, preserving the old owner for storage writes.
    pub(super) fn stop_tracking(&self, state: &mut State) {
        let effects = state.playtime.tracker.update(
            TrackInput::Stop { at: unix_now() },
            gamenode_platform::random::session_id,
        );
        self.track_effects(state, effects);
        let runtime_effects = state.playtime.runtime_sessions.reset();
        self.track_effects(state, runtime_track_effects(runtime_effects));
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
        self.shutdown_ports();
        let (tx, rx) = mpsc::channel();
        self.shared.storage.submit(move || {
            let _ = tx.send(());
        });
        if rx.recv_timeout(Duration::from_secs(2)).is_err() {
            tracing::warn!("playtime storage flush timed out");
        }
    }
    /// Starts a counter read when one is due and none is running. Returns whether a read started;
    /// its answer later refreshes the playtime display and publishes.
    pub(super) fn read_counters(&self, state: &mut State) -> bool {
        if !state.playtime.ready
            || state.playtime.stopped
            || state.playtime.reading
            || state.playtime.source != PlaytimeSource::Store
        {
            return false;
        }
        let Some(account) = state.library.account().map(|a| a.id.clone()) else {
            return false;
        };
        state.playtime.reading = true;
        let generation = state.playtime.generation;
        let providers = state.library.providers().to_vec();
        let engine = self.clone();
        let ports = self.shared.ports.clone();
        self.shared.runtime.spawn(async move {
            let mut values = Vec::new();
            for provider in providers {
                if provider.counter_plugin().is_none() {
                    continue;
                }
                let Some(runtime) = engine.provider_runtime() else {
                    continue;
                };
                if !runtime.has_provider(provider.id()) {
                    continue;
                }
                let Ok(completion) = runtime
                    .execute(
                        provider.id(),
                        gamenode_plugin_protocol::proto::ProviderOperation::ReadCounter,
                    )
                    .await
                else {
                    continue;
                };
                if let Ok(counters) = completion.commit(|result, _| result.counters).await {
                    values.extend(counters.into_iter().map(|value| {
                        (
                            format!("{}:{}", provider.id(), value.id),
                            gamenode_core::provider::registry::CounterValue {
                                seconds: value.seconds,
                                last_played: value.last_played,
                            },
                        )
                    }));
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
                        total.last_played_at = value.last_played;
                        store.save_total(&total)?;
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    tracing::warn!(%error,"could not save store totals");
                }
                let mut state = engine2.lock();
                if state.playtime.sync_now_awaiting_counters {
                    if std::mem::take(&mut state.playtime.sync_now_counters_again) {
                        // The import that was in flight started before the scan finished. Read
                        // once more so the requested pass sees the new scan's counters.
                        if engine2.read_counters(&mut state) {
                            return;
                        }
                    }
                    state.playtime.sync_now_awaiting_counters = false;
                }
                engine2.submit_totals(&mut state);
                engine2.refresh_playtime_display(&mut state);
            });
        });
        true
    }
    pub(super) fn submit_totals(&self, state: &mut State) {
        // A requested "Sync now" owns the next pass: nothing may submit the totals held before
        // its rescan and counter import finish.
        if state.playtime.sync_now_awaiting_scan || state.playtime.sync_now_awaiting_counters {
            return;
        }
        if state.playtime.submitting || state.playtime.pass_running {
            state.playtime.resubmit = true;
            return;
        }
        if !state.playtime.ready
            || state.playtime.stopped
            || state.playtime.source != PlaytimeSource::Store
            || !state
                .playtime
                .preferences
                .may_submit(state.machine.verified_credential().is_some())
        {
            state.playtime.sync_now_running = false;
            state.playtime.sync_now_retry_allowed = false;
            return;
        }
        let Some(account) = state.library.account().map(|a| a.id.clone()) else {
            state.playtime.sync_now_running = false;
            state.playtime.sync_now_retry_allowed = false;
            return;
        };
        state.playtime.pass_running = true;
        let generation = state.playtime.generation;
        let store = self.shared.ports.playtime.clone();
        let engine = self.clone();
        self.shared.storage.submit(move || {
            let totals = store.totals(&account);
            let mut state = engine.lock();
            if generation != state.playtime.generation || state.playtime.stopped {
                state.playtime.pass_running = false;
                state.playtime.sync_now_running = false;
                state.playtime.sync_now_retry_allowed = false;
                return;
            }
            let Ok(totals) = totals else {
                state.playtime.submitting = false;
                state.playtime.pass_running = false;
                state.playtime.sync_now_running = false;
                state.playtime.sync_now_retry_allowed = false;
                return;
            };
            // Entries of one provider matched to the same game are submitted as one total, so
            // their values replace each other on the server instead of overwriting.
            let candidates = group_candidates(submission_candidates(&state, &totals), &totals);
            let preferences = state.playtime.preferences;
            let allow_rejected = state.playtime.sync_now_retry_allowed
                || state
                    .playtime
                    .rejected_retry_at
                    .is_some_and(|at| unix_now() >= at);
            // A retry is an explicit opportunity, not a property of every resubmit in the same
            // request. Consume it before the pass can queue another send.
            state.playtime.sync_now_retry_allowed = false;
            let effects = state.playtime.submitter.start_for_source_with_retry(
                PlaytimeSource::Store,
                &account,
                preferences,
                true,
                allow_rejected,
                candidates,
            );
            engine.submit_effects(&mut state, effects);
        });
    }

    /// Loads the last completed pass for one account without blocking the caller. A late answer
    /// for another account is dropped.
    fn load_sync_summary(&self, account: String) {
        let engine = self.clone();
        let store = self.shared.ports.playtime.clone();
        self.shared.storage.submit(move || {
            let summary = match store.load_sync_summary(&account) {
                Ok(summary) => summary,
                Err(error) => {
                    tracing::warn!(%error, "could not load the last sync summary");
                    return;
                }
            };
            let mut state = engine.lock();
            if state.playtime.stopped
                || state.playtime.summary_for.as_deref() != Some(account.as_str())
            {
                return;
            }
            if state.playtime.summary != summary {
                state.playtime.summary = summary;
                engine.publish(&mut state);
            }
        });
    }

    /// Records the pass that just ended: the accepted totals it leaves, the stores that
    /// contributed and the totals still held. The storage job runs after the pass's own saves.
    fn record_sync_summary(&self, state: &mut State) {
        let Some(account) = state.library.account().map(|a| a.id.clone()) else {
            state.playtime.pass_running = false;
            state.playtime.sync_now_running = false;
            state.playtime.sync_now_retry_allowed = false;
            return;
        };
        let engine = self.clone();
        let store = self.shared.ports.playtime.clone();
        self.shared.storage.submit(move || {
            let totals = store.totals(&account);
            let mut state = engine.lock();
            if state.playtime.stopped
                || state.library.account().map(|a| a.id.as_str()) != Some(account.as_str())
            {
                state.playtime.pass_running = false;
                state.playtime.sync_now_running = false;
                return;
            }
            let mut summary = None;
            if let Ok(totals) = totals {
                let candidates = group_candidates(submission_candidates(&state, &totals), &totals);
                let mut accepted_games = 0u32;
                let mut stores = std::collections::BTreeSet::new();
                let mut held = 0u32;
                let mut rejected = false;
                for candidate in candidates {
                    let Some(game_id) = candidate.game_id else {
                        continue;
                    };
                    if candidate.total.accepted_game_id == Some(game_id)
                        && candidate.total.accepted_seconds == Some(candidate.total.total_seconds)
                    {
                        accepted_games += 1;
                        stores.insert(candidate.provider);
                    } else {
                        held += 1;
                        rejected = rejected
                            || candidate.total.rejected_seconds
                                == Some(candidate.total.total_seconds);
                    }
                }
                state.playtime.rejected_retry_at =
                    rejected.then(|| unix_now().saturating_add(REJECTED_RETRY_AFTER_SECS));
                let recorded = SyncSummary {
                    completed_at: unix_now(),
                    accepted_games,
                    stores: u32::try_from(stores.len()).unwrap_or(u32::MAX),
                    held,
                };
                if let Err(error) = store.save_sync_summary(&account, recorded) {
                    tracing::warn!(%error, "could not save the last sync summary");
                }
                summary = Some(recorded);
            }
            if let Some(summary) = summary {
                state.playtime.summary = Some(summary);
                state.playtime.summary_for = Some(account);
            }
            state.playtime.pass_running = false;
            state.playtime.sync_now_running = false;
            state.playtime.sync_now_retry_allowed = false;
            if std::mem::take(&mut state.playtime.resubmit) {
                engine.submit_totals(&mut state);
            }
            engine.publish(&mut state);
        });
    }

    /// "Sync now": rescans the library, waits for that scan and the counter import it triggers,
    /// then submits every eligible total under the normal rules. A call while a pass is pending or
    /// running is coalesced; sync off or an unverified session refuses it.
    pub fn sync_now(&self) -> Result<(), SyncNowError> {
        {
            let mut state = self.lock();
            if state.playtime.pass_running
                || state.playtime.submitting
                || state.playtime.sync_now_running
            {
                return Ok(());
            }
            if state.machine.verified_credential().is_none() {
                return Err(SyncNowError::Unverified);
            }
            if !state.playtime.preferences.may_submit(true) {
                return Err(SyncNowError::Disabled);
            }
            if state.library.account().is_none() {
                return Err(SyncNowError::Unverified);
            }
            state.playtime.sync_now_running = true;
            state.playtime.sync_now_awaiting_scan = true;
            state.playtime.sync_now_awaiting_counters = false;
            state.playtime.sync_now_counters_again = false;
            state.playtime.sync_now_retry_allowed = true;
            self.publish(&mut state);
        }
        self.rescan();
        let mut state = self.lock();
        if state.playtime.sync_now_awaiting_scan
            && !state.library_loading
            && !state.library.scan_running()
        {
            self.sync_now_scan_finished(&mut state);
            self.publish(&mut state);
        }
        Ok(())
    }

    /// Continues the requested pass once the rescan ended: reads the store counters that the new
    /// scan may have made visible, then submits. A requested pass never sends the totals that were
    /// held before the scan.
    pub(super) fn sync_now_scan_finished(&self, state: &mut State) {
        if !state.playtime.sync_now_running || !state.playtime.sync_now_awaiting_scan {
            return;
        }
        state.playtime.sync_now_awaiting_scan = false;
        if state.playtime.stopped
            || state.machine.verified_credential().is_none()
            || !state.playtime.preferences.may_submit(true)
            || state.library.account().is_none()
        {
            state.playtime.sync_now_running = false;
            state.playtime.sync_now_retry_allowed = false;
            return;
        }
        if state.playtime.source != PlaytimeSource::Store {
            self.submit_totals(state);
            return;
        }
        state.playtime.sync_now_awaiting_counters = true;
        state.playtime.sync_now_counters_again = state.playtime.reading;
        if self.read_counters(state) {
            return;
        }
        if state.playtime.sync_now_counters_again {
            // The read already in flight has to finish before a fresh one can start.
            return;
        }
        state.playtime.sync_now_awaiting_counters = false;
        self.submit_totals(state);
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
                        engine.dispatch_on_worker(Input::SessionRejected).await;
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
        if !state.playtime.submitting {
            let resubmit = std::mem::take(&mut state.playtime.resubmit) && saved;
            if resubmit {
                state.playtime.pass_running = false;
                self.submit_totals(state);
            } else if state.playtime.pass_running {
                // The record job clears `running` with the new summary, so the button keeps
                // showing progress until the counts are ready.
                self.record_sync_summary(state);
            }
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
                        last_played_at: None,
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

fn runtime_track_effects(
    effects: Vec<gamenode_core::tracker::runtime_sessions::Effect>,
) -> Vec<TrackEffect> {
    effects
        .into_iter()
        .map(|effect| match effect {
            gamenode_core::tracker::runtime_sessions::Effect::Open(session) => {
                TrackEffect::OpenSession(session)
            }
            gamenode_core::tracker::runtime_sessions::Effect::Checkpoint { id, at } => {
                TrackEffect::Checkpoint { id, at }
            }
            gamenode_core::tracker::runtime_sessions::Effect::Close {
                id,
                ended_at,
                game_id,
            } => TrackEffect::CloseSession {
                id,
                ended_at,
                game_id,
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gamenode_core::drm_free::ProcessPolicy;
    use gamenode_core::library::StoredEntry;
    use std::collections::BTreeMap;

    fn package_entry(key: &str, executable: Option<&str>) -> StoredEntry {
        let mut fields = BTreeMap::new();
        if let Some(executable) = executable {
            fields.insert("executable".to_owned(), executable.to_owned());
        }
        StoredEntry {
            key: key.to_owned(),
            provider: "packagekit".to_owned(),
            store_id: key.trim_start_matches("packagekit:").to_owned(),
            name: None,
            install_dir: "/usr/bin".to_owned(),
            fields,
            installed: true,
            target: None,
            owned: false,
            shared: false,
            first_seen: 0,
            last_seen: 0,
        }
    }

    #[test]
    fn package_manager_entries_get_an_exact_executable_policy() {
        let entry = package_entry("packagekit:0ad", Some("/usr/bin/0ad"));
        assert_eq!(
            package_executable_policy(&entry),
            Some((
                "packagekit:0ad".to_owned(),
                ProcessPolicy::Explicit {
                    gameplay: vec!["/usr/bin/0ad".to_owned()],
                    launchers: Vec::new(),
                },
            ))
        );
    }

    #[test]
    fn entries_without_an_executable_keep_the_existing_root_match() {
        assert_eq!(
            package_executable_policy(&package_entry("packagekit:0ad", None)),
            None
        );
        assert_eq!(
            package_executable_policy(&package_entry("packagekit:0ad", Some("  "))),
            None
        );
    }
}

/// The totals that can be submitted right now: the entry is known, its provider reports playtime
/// and its game match exists. A pass and its summary both use this rule.
fn submission_candidates(state: &State, totals: &[Total]) -> Vec<Candidate> {
    totals
        .iter()
        .filter_map(|total| {
            let entry = state.library.entry(&total.entry_key)?;
            let provider = state.library.provider(&entry.provider)?;
            provider.manifest().playtime.as_ref()?;
            Some(Candidate {
                provider: entry.provider.clone(),
                game_id: state.library.match_status(entry).game_id(),
                source: provider.source().into(),
                total: total.clone(),
            })
        })
        .collect()
}
