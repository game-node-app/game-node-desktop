//! Achievement refresh scheduling: one serialized worker per provider, a priority queue, paced
//! background batches and a persistent cache. No GameNode request is ever made from this module.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use gamenode_core::achievements::{
    AchievementFilter, AchievementGameState, AchievementListView, AchievementRecord,
    RefreshTrigger, StoredAchievementGame, StoredAchievementSummary, is_stale, mask_hidden,
    ordered_achievements, summarize,
};
use gamenode_core::library::CatalogError;
use gamenode_plugin_protocol::models;
use gamenode_plugin_protocol::proto::OperationDisposition;

use super::*;

const MAX_BATCH: usize = 20;
/// Background imports and reconciliation run in smaller batches so a selected game never waits
/// behind a long batch of other games. The per-game pacing below keeps the overall rate.
const BACKGROUND_BATCH: usize = 5;
const BACKGROUND_PAUSE: Duration = Duration::from_secs(1);
/// How often a background pause re-checks for selected or game-exit work.
const PAUSE_POLL: Duration = Duration::from_millis(250);
const RATE_LIMIT_BACKOFF_START: Duration = Duration::from_secs(60);
const RATE_LIMIT_BACKOFF_MAX: Duration = Duration::from_secs(60 * 60);

/// A batch that could not run is retried after the first rate-limit pause. Tests use a short
/// delay so recovery can be observed without sleeping for a minute.
fn failure_retry_delay() -> Duration {
    if cfg!(test) {
        Duration::from_millis(10)
    } else {
        RATE_LIMIT_BACKOFF_START
    }
}
const RECONCILE_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Default)]
pub(super) struct Queue {
    pub(super) pending: Vec<Job>,
    pub(super) running: bool,
    pub(super) paused_until: Option<Instant>,
    pub(super) stopped: bool,
    pub(super) backoff: Duration,
    pub(super) next_sequence: u64,
    pub(super) notify: Arc<tokio::sync::Notify>,
}

#[derive(Clone)]
pub(super) struct Job {
    pub(super) entry_id: String,
    pub(super) priority: u8,
    pub(super) due: Instant,
    pub(super) sequence: u64,
}

fn priority(trigger: RefreshTrigger) -> u8 {
    match trigger {
        RefreshTrigger::Selected => 1,
        RefreshTrigger::GameExit => 2,
        RefreshTrigger::Import => 3,
        RefreshTrigger::Reconcile => 4,
    }
}

fn trigger_delay(trigger: RefreshTrigger) -> Duration {
    match trigger {
        RefreshTrigger::GameExit => {
            Duration::from_secs(gamenode_core::achievements::GAME_EXIT_SETTLING_SECS as u64)
        }
        _ => Duration::ZERO,
    }
}

/// The next batch size from an already sorted queue. Selected and game-exit jobs never share a
/// batch with background import or reconciliation jobs, so their result is not delayed by them.
/// While a rate-limit pause is active, only those urgent jobs may run.
pub(super) fn batch_size(queue: &Queue, paused: bool) -> usize {
    let urgent = queue
        .pending
        .iter()
        .take_while(|job| job.priority <= 2)
        .count();
    if paused || urgent > 0 {
        return urgent.min(MAX_BATCH);
    }
    queue.pending.len().min(BACKGROUND_BATCH)
}

/// Waits out the pacing pause after a background batch, waking early when selected or game-exit
/// work arrives.
async fn wait_background_pause(shared: &Shared, provider: &str, pause: Duration) {
    let deadline = std::time::Instant::now() + pause;
    loop {
        let urgent = {
            let queues = shared
                .achievement_queues
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match queues.get(provider) {
                Some(queue) => queue.stopped || queue.pending.iter().any(|job| job.priority <= 2),
                None => true,
            }
        };
        if urgent {
            return;
        }
        let wait = deadline.saturating_duration_since(std::time::Instant::now());
        if wait.is_zero() {
            return;
        }
        tokio::time::sleep(wait.min(PAUSE_POLL)).await;
    }
}

impl Engine {
    /// Providers whose manifest declares an achievement backend.
    pub(super) fn achievement_providers(&self) -> Vec<String> {
        self.lock()
            .library
            .providers()
            .iter()
            .filter(|provider| provider.achievement_plugin().is_some())
            .map(|provider| provider.id().to_owned())
            .collect()
    }

    fn provider_has_achievements(&self, provider: &str) -> bool {
        self.lock()
            .library
            .provider(provider)
            .is_some_and(|provider| provider.achievement_plugin().is_some())
    }

    fn language(&self) -> String {
        self.lock().achievement_language.clone()
    }

    /// Loads cached summaries for the account currently connected to each provider. Runs on the
    /// storage worker; the cache is not read on the UI thread.
    pub(super) fn load_achievement_cache(&self) {
        let providers = self.achievement_providers();
        if providers.is_empty() {
            return;
        }
        let (engine, library, achievements) = (
            self.clone(),
            self.shared.ports.library.clone(),
            self.shared.ports.achievements.clone(),
        );
        self.shared.storage.submit(move || {
            let accounts = library.load_store_accounts().unwrap_or_else(|error| {
                tracing::warn!(%error, "could not read store accounts for achievements");
                Vec::new()
            });
            let mut summaries = BTreeMap::new();
            for provider in providers {
                let Some(account) = accounts.iter().find(|account| account.provider == provider)
                else {
                    continue;
                };
                match achievements.load_summaries(&provider, &account.account_id) {
                    Ok(rows) => {
                        for row in rows {
                            summaries.insert((provider.clone(), row.entry_id.clone()), row);
                        }
                    }
                    Err(error) => {
                        tracing::warn!(%error, "could not read the achievement cache");
                    }
                }
            }
            engine.apply_achievement_summaries(summaries);
        });
    }

    fn apply_achievement_summaries(
        &self,
        summaries: BTreeMap<(String, String), StoredAchievementSummary>,
    ) {
        {
            let mut state = self.lock();
            state.achievement_summaries = summaries;
        }
        self.publish_current();
    }

    pub(super) fn clear_achievement_memory(&self, provider: &str) {
        {
            let mut state = self.lock();
            state
                .achievement_summaries
                .retain(|(entry_provider, _), _| entry_provider != provider);
        }
        if let Ok(mut queues) = self.shared.achievement_queues.lock()
            && let Some(queue) = queues.get_mut(provider)
        {
            // Keep the entry and its `running` flag: a worker mid-batch must be the only one.
            // Its outcomes are fenced by the account re-check before persistence.
            queue.pending.clear();
            queue.paused_until = None;
            queue.stopped = false;
            queue.backoff = Duration::ZERO;
            queue.notify.notify_waiters();
        }
        self.publish_current();
    }

    /// Clears a stopped/paused worker so it may run after the account reconnects.
    pub(super) fn resume_achievements(&self, provider: &str) {
        if let Ok(mut queues) = self.shared.achievement_queues.lock()
            && let Some(queue) = queues.get_mut(provider)
        {
            queue.stopped = false;
            queue.paused_until = None;
            queue.backoff = Duration::ZERO;
            queue.notify.notify_one();
        }
        self.request_achievement_refresh_for_provider(provider, RefreshTrigger::Reconcile);
    }

    /// The selected game is refreshed ahead of background work when it is stale. Called after the
    /// engine state lock is released.
    pub(super) fn refresh_selected_achievements(&self, key: String) {
        let Some((provider, entry_id)) = split_entry_key(&key) else {
            return;
        };
        if !self.provider_has_achievements(&provider) {
            return;
        }
        self.enqueue_achievements(&provider, &[entry_id], RefreshTrigger::Selected);
    }

    pub(super) fn request_import_achievements(&self) {
        for provider in self.achievement_providers() {
            self.request_achievement_refresh_for_provider(&provider, RefreshTrigger::Import);
        }
    }

    pub(super) fn request_reconcile_achievements(&self) {
        for provider in self.achievement_providers() {
            self.request_achievement_refresh_for_provider(&provider, RefreshTrigger::Reconcile);
        }
    }

    pub(super) fn start_achievement_reconciliation(&self) {
        let weak = Arc::downgrade(&self.shared);
        let runtime = self.shared.runtime.clone();
        runtime.spawn(async move {
            loop {
                tokio::time::sleep(RECONCILE_INTERVAL).await;
                let Some(shared) = weak.upgrade() else {
                    return;
                };
                Engine { shared }.request_reconcile_achievements();
            }
        });
    }

    fn request_achievement_refresh_for_provider(&self, provider: &str, trigger: RefreshTrigger) {
        let entries = self
            .lock()
            .library
            .visible_entries()
            .filter(|entry| entry.provider == provider)
            .map(|entry| entry.store_id.clone())
            .collect::<Vec<_>>();
        self.enqueue_achievements(provider, &entries, trigger);
    }

    pub(super) fn schedule_exited_game_achievements(&self, key: &str) {
        let Some((provider, entry_id)) = split_entry_key(key) else {
            return;
        };
        let engine = self.clone();
        self.shared.runtime.spawn(async move {
            tokio::time::sleep(trigger_delay(RefreshTrigger::GameExit)).await;
            if engine.provider_has_achievements(&provider) && engine.provider_runtime().is_some() {
                engine.enqueue_achievements(&provider, &[entry_id], RefreshTrigger::GameExit);
            }
        });
    }

    fn enqueue_achievements(&self, provider: &str, entry_ids: &[String], trigger: RefreshTrigger) {
        if entry_ids.is_empty() || !self.provider_has_achievements(provider) {
            return;
        }
        let now = Instant::now();
        let language = self.language();
        let due = now + trigger_delay(trigger);
        let mut queued = Vec::new();
        {
            let state = self.lock();
            for entry_id in entry_ids {
                let cached = state
                    .achievement_summaries
                    .get(&(provider.to_owned(), entry_id.clone()));
                if !matches!(trigger, RefreshTrigger::GameExit)
                    && !is_stale(cached, trigger, unix_now(), &language)
                {
                    continue;
                }
                queued.push(entry_id.clone());
            }
        }
        if queued.is_empty() {
            return;
        }
        let start = {
            let mut queues = self
                .shared
                .achievement_queues
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let queue = queues.entry(provider.to_owned()).or_default();
            for entry_id in queued {
                if let Some(existing) = queue
                    .pending
                    .iter_mut()
                    .find(|job| job.entry_id == entry_id)
                {
                    if priority(trigger) < existing.priority {
                        existing.priority = priority(trigger);
                        existing.due = due;
                        existing.sequence = queue.next_sequence;
                        queue.next_sequence += 1;
                    }
                    continue;
                }
                queue.pending.push(Job {
                    entry_id,
                    priority: priority(trigger),
                    due,
                    sequence: queue.next_sequence,
                });
                queue.next_sequence += 1;
            }
            queue.notify.notify_waiters();
            if queue.running || queue.stopped {
                false
            } else {
                queue.running = true;
                true
            }
        };
        if start {
            let engine = self.clone();
            let provider = provider.to_owned();
            self.shared.runtime.spawn(async move {
                engine.run_achievement_worker(provider).await;
            });
        }
    }

    async fn run_achievement_worker(self, provider: String) {
        enum Next {
            Stop,
            Idle,
            Wait(Duration, Arc<tokio::sync::Notify>),
            Run,
        }
        loop {
            let next = {
                let queues = self
                    .shared
                    .achievement_queues
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let Some(queue) = queues.get(&provider) else {
                    return;
                };
                if queue.stopped {
                    Next::Stop
                } else if queue.pending.is_empty() {
                    Next::Idle
                } else if let Some(until) = queue.paused_until
                    && until > Instant::now()
                {
                    // Rate limiting pauses background work only. Selected and game-exit jobs
                    // still run, one batch at a time.
                    let high_priority_due = queue
                        .pending
                        .iter()
                        .filter(|job| job.priority <= 2)
                        .map(|job| job.due)
                        .min();
                    match high_priority_due {
                        Some(due) if due <= Instant::now() => Next::Run,
                        Some(due) => Next::Wait(
                            due.min(until).saturating_duration_since(Instant::now()),
                            queue.notify.clone(),
                        ),
                        None => Next::Wait(
                            until.saturating_duration_since(Instant::now()),
                            queue.notify.clone(),
                        ),
                    }
                } else {
                    let due = queue
                        .pending
                        .iter()
                        .min_by_key(|job| (job.priority, job.due, job.sequence))
                        .map(|job| job.due)
                        .unwrap_or_else(Instant::now);
                    if due > Instant::now() {
                        Next::Wait(
                            due.saturating_duration_since(Instant::now()),
                            queue.notify.clone(),
                        )
                    } else {
                        Next::Run
                    }
                }
            };
            match next {
                Next::Stop => {
                    // Clearing `running` in the same critical section avoids stranding a job
                    // enqueued after an earlier idle check.
                    let mut queues = self
                        .shared
                        .achievement_queues
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    if let Some(queue) = queues.get_mut(&provider) {
                        queue.running = false;
                    }
                    return;
                }
                Next::Idle => {
                    let mut queues = self
                        .shared
                        .achievement_queues
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let Some(queue) = queues.get_mut(&provider) else {
                        return;
                    };
                    if !queue.pending.is_empty() {
                        // A job arrived between the decision and this lock: keep running.
                        continue;
                    }
                    queue.running = false;
                    return;
                }
                Next::Wait(wait, notify) => {
                    tokio::select! {
                        _ = tokio::time::sleep(wait) => {}
                        _ = notify.notified() => {}
                    }
                }
                Next::Run => {
                    let jobs = {
                        let mut queues = self
                            .shared
                            .achievement_queues
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        let Some(queue) = queues.get_mut(&provider) else {
                            return;
                        };
                        queue
                            .pending
                            .sort_by_key(|job| (job.priority, job.due, job.sequence));
                        let paused = queue
                            .paused_until
                            .is_some_and(|until| until > Instant::now());
                        let count = batch_size(queue, paused);
                        queue.pending.drain(..count).collect::<Vec<_>>()
                    };
                    if jobs.is_empty() {
                        continue;
                    }
                    let background = jobs.iter().all(|job| job.priority >= 3);
                    let count = jobs.len();
                    self.dispatch_achievement_batch(&provider, jobs).await;
                    if background {
                        // Background imports and reconciliation stay around one game per second,
                        // but a selected game interrupts the pause instead of waiting it out.
                        let pause = if cfg!(test) {
                            Duration::from_millis(5)
                        } else {
                            BACKGROUND_PAUSE
                        };
                        wait_background_pause(&self.shared, &provider, pause * count as u32).await;
                    }
                }
            }
        }
    }

    async fn dispatch_achievement_batch(&self, provider: &str, jobs: Vec<Job>) {
        let Some(runtime) = self.provider_runtime() else {
            self.requeue_achievement_jobs(provider, jobs, failure_retry_delay());
            return;
        };
        if let Some(status) = runtime.status(provider) {
            if status.state == models::AccountState::Expired {
                // Keep the jobs for a reconnect, but stop the worker as the expiry rules require.
                self.requeue_achievement_jobs(provider, jobs, Duration::ZERO);
                if let Ok(mut queues) = self.shared.achievement_queues.lock()
                    && let Some(queue) = queues.get_mut(provider)
                {
                    queue.stopped = true;
                }
                return;
            }
            if matches!(
                status.state,
                models::AccountState::NotConnected | models::AccountState::CredentialError
            ) {
                self.requeue_achievement_jobs(provider, jobs, failure_retry_delay());
                return;
            }
        }
        let Some(account) = self.achievement_account(provider).await else {
            self.requeue_achievement_jobs(provider, jobs, failure_retry_delay());
            return;
        };
        let language = self.language();
        let request = models::AchievementsRequest {
            game_ids: jobs.iter().map(|job| job.entry_id.clone()).collect(),
            language: language.clone(),
        };
        let Ok(completion) = runtime.execute_achievements(provider, request).await else {
            let delay = failure_retry_delay();
            self.pause_achievements(provider, delay);
            self.requeue_achievement_jobs(provider, jobs, delay);
            return;
        };
        let outcomes = completion.achievements;
        if completion.result.disposition == OperationDisposition::Cancelled as i32 {
            return;
        }
        self.save_achievement_outcomes(provider, &account, outcomes, &language, unix_now())
            .await;
    }

    pub(super) async fn save_achievement_outcomes(
        &self,
        provider: &str,
        account: &str,
        outcomes: Vec<models::AchievementOutcome>,
        language: &str,
        now: i64,
    ) {
        let store = self.shared.ports.achievements.clone();
        let library = self.shared.ports.library.clone();
        let provider_owned = provider.to_owned();
        let account_owned = account.to_owned();
        let language = language.to_owned();
        let result = tokio::task::spawn_blocking(move || {
            // The account may have been replaced or signed out while the provider was reading.
            // Re-read the connected account on this worker and write nothing when it changed.
            let current = library
                .load_store_accounts()
                .ok()
                .and_then(|accounts| {
                    accounts
                        .into_iter()
                        .find(|candidate| candidate.provider == provider_owned)
                })
                .map(|candidate| candidate.account_id);
            if current.as_deref() != Some(account_owned.as_str()) {
                return Ok(Vec::new());
            }
            let mut summaries = Vec::new();
            for outcome in outcomes {
                match outcome {
                    models::AchievementOutcome::Ready {
                        game_id,
                        achievements,
                    } => {
                        let records = achievement_records(achievements);
                        store.save_ready(
                            &provider_owned,
                            &account_owned,
                            &game_id,
                            &language,
                            &records,
                            now,
                        )?;
                        summaries.push((
                            game_id.clone(),
                            summarize(&StoredAchievementGame {
                                entry_id: game_id,
                                state: AchievementGameState::Ready,
                                language: language.clone(),
                                last_success_at: Some(now),
                                last_attempt_at: now,
                                failure: None,
                                achievements: records,
                            }),
                        ));
                    }
                    models::AchievementOutcome::None { game_id } => {
                        store.save_none(
                            &provider_owned,
                            &account_owned,
                            &game_id,
                            &language,
                            now,
                        )?;
                        summaries.push((
                            game_id.clone(),
                            StoredAchievementSummary {
                                entry_id: game_id,
                                state: AchievementGameState::None,
                                language: language.clone(),
                                last_success_at: Some(now),
                                last_attempt_at: now,
                                failure: None,
                                unlocked: 0,
                                total: 0,
                                latest_unlock: None,
                            },
                        ));
                    }
                    models::AchievementOutcome::Failed { game_id, error } => {
                        let category = super::providers::catalog_error(error);
                        store.save_failure(
                            &provider_owned,
                            &account_owned,
                            &game_id,
                            category,
                            now,
                        )?;
                        summaries.push((
                            game_id.clone(),
                            StoredAchievementSummary {
                                entry_id: game_id,
                                state: AchievementGameState::Failed,
                                language: String::new(),
                                last_success_at: None,
                                last_attempt_at: now,
                                failure: Some(category),
                                unlocked: 0,
                                total: 0,
                                latest_unlock: None,
                            },
                        ));
                    }
                }
            }
            Ok::<_, StorageError>(summaries)
        })
        .await;
        let Ok(Ok(summaries)) = result else {
            tracing::warn!(provider, "could not save achievement results");
            return;
        };
        let mut rate_limited = false;
        let mut expired = false;
        {
            let mut state = self.lock();
            for (entry_id, summary) in summaries {
                if summary.failure == Some(CatalogError::RateLimited) {
                    rate_limited = true;
                }
                if summary.failure == Some(CatalogError::Expired) {
                    expired = true;
                }
                let key = (provider.to_owned(), entry_id);
                let merged = match (state.achievement_summaries.get(&key), summary.state) {
                    (Some(previous), AchievementGameState::Failed) => StoredAchievementSummary {
                        last_success_at: previous.last_success_at,
                        unlocked: previous.unlocked,
                        total: previous.total,
                        latest_unlock: previous.latest_unlock.clone(),
                        language: previous.language.clone(),
                        failure: summary.failure,
                        last_attempt_at: summary.last_attempt_at,
                        ..previous.clone()
                    },
                    _ => summary,
                };
                state.achievement_summaries.insert(key, merged);
            }
        }
        if expired
            && let Ok(mut queues) = self.shared.achievement_queues.lock()
            && let Some(queue) = queues.get_mut(provider)
        {
            queue.stopped = true;
        }
        if rate_limited {
            self.pause_achievements(provider, self.next_achievement_backoff(provider));
        } else if !expired {
            self.reset_achievement_backoff(provider);
        }
        self.publish_current();
    }

    fn requeue_achievement_jobs(&self, provider: &str, jobs: Vec<Job>, delay: Duration) {
        let mut queues = self
            .shared
            .achievement_queues
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(queue) = queues.get_mut(provider) else {
            return;
        };
        let due = Instant::now() + delay;
        for mut job in jobs {
            job.due = due;
            if let Some(existing) = queue
                .pending
                .iter_mut()
                .find(|existing| existing.entry_id == job.entry_id)
            {
                if (job.priority, job.due) < (existing.priority, existing.due) {
                    *existing = job;
                }
            } else {
                queue.pending.push(job);
            }
        }
        queue.notify.notify_waiters();
    }

    fn next_achievement_backoff(&self, provider: &str) -> Duration {
        self.shared
            .achievement_queues
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(provider)
            .map(|queue| {
                if queue.backoff.is_zero() {
                    RATE_LIMIT_BACKOFF_START
                } else {
                    (queue.backoff * 2).min(RATE_LIMIT_BACKOFF_MAX)
                }
            })
            .unwrap_or(RATE_LIMIT_BACKOFF_START)
    }

    fn pause_achievements(&self, provider: &str, duration: Duration) {
        if let Ok(mut queues) = self.shared.achievement_queues.lock()
            && let Some(queue) = queues.get_mut(provider)
        {
            queue.backoff = duration;
            queue.paused_until = Some(Instant::now() + duration);
        }
    }

    fn reset_achievement_backoff(&self, provider: &str) {
        if let Ok(mut queues) = self.shared.achievement_queues.lock()
            && let Some(queue) = queues.get_mut(provider)
        {
            queue.backoff = Duration::ZERO;
        }
    }

    async fn achievement_account(&self, provider: &str) -> Option<String> {
        let library = self.shared.ports.library.clone();
        let provider = provider.to_owned();
        tokio::task::spawn_blocking(move || {
            library.load_store_accounts().ok().and_then(|accounts| {
                accounts
                    .into_iter()
                    .find(|account| account.provider == provider)
                    .map(|account| account.account_id)
            })
        })
        .await
        .ok()
        .flatten()
    }

    /// The async full-list command. Reads only the local cache, so it works with no GameNode match
    /// and no GameNode network.
    pub async fn achievement_list(
        &self,
        key: String,
        filter: AchievementFilter,
    ) -> Result<Option<AchievementListView>, String> {
        let Some((provider, entry_id)) = split_entry_key(&key) else {
            return Ok(None);
        };
        if !self.provider_has_achievements(&provider) {
            return Ok(None);
        }
        let library = self.shared.ports.library.clone();
        let achievements = self.shared.ports.achievements.clone();
        let result =
            tokio::task::spawn_blocking(move || -> Result<Option<AchievementListView>, String> {
                let account = library
                    .load_store_accounts()
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .find(|account| account.provider == provider)
                    .map(|account| account.account_id);
                let Some(account) = account else {
                    return Ok(Some(AchievementListView {
                        connected: false,
                        state: None,
                        last_success_at: None,
                        failure: None,
                        achievements: Vec::new(),
                    }));
                };
                let Some(game) = achievements
                    .load_game(&provider, &account, &entry_id)
                    .map_err(|error| error.to_string())?
                else {
                    return Ok(None);
                };
                let mut records = ordered_achievements(&game.achievements, filter);
                mask_hidden(&mut records);
                Ok(Some(AchievementListView {
                    connected: true,
                    state: Some(game.state),
                    last_success_at: game.last_success_at,
                    failure: game.failure,
                    achievements: records,
                }))
            })
            .await
            .map_err(|_| "the achievement reader stopped".to_owned())?;
        let mut view = result?;
        if let Some(view) = &mut view {
            // Only viewed games download icons, and only trusted URLs. The list gets the stable
            // asset route; an untrusted or missing icon keeps its placeholder. Fetch just the
            // variant the list shows: colour for unlocked achievements, grey for locked ones.
            let urls = view
                .achievements
                .iter()
                .map(|achievement| {
                    if achievement.unlocked {
                        &achievement.icon_url
                    } else {
                        &achievement.icon_gray_url
                    }
                })
                .filter(|url| gamenode_platform::achievement_icons::trusted_icon_url(url))
                .cloned()
                .collect::<Vec<_>>();
            if !urls.is_empty() {
                let covers_dir = self.shared.ports.covers_dir.clone();
                self.shared.runtime.spawn_blocking(move || {
                    if let Ok(loader) =
                        gamenode_platform::achievement_icons::AchievementIconLoader::production(
                            &covers_dir,
                        )
                    {
                        loader.ensure(urls);
                    }
                });
            }
            for achievement in &mut view.achievements {
                achievement.icon_url = icon_asset(&achievement.icon_url);
                achievement.icon_gray_url = icon_asset(&achievement.icon_gray_url);
            }
        }
        Ok(view)
    }
}

pub(super) fn icon_asset(url: &str) -> String {
    if !gamenode_platform::achievement_icons::trusted_icon_url(url) {
        return String::new();
    }
    gamenode_platform::achievement_icons::achievement_icon_asset_url(
        &gamenode_platform::achievement_icons::achievement_icon_hash(url),
    )
}
fn split_entry_key(key: &str) -> Option<(String, String)> {
    let (provider, entry_id) = key.split_once(':')?;
    (!provider.is_empty() && !entry_id.is_empty())
        .then(|| (provider.to_owned(), entry_id.to_owned()))
}

fn achievement_records(achievements: Vec<models::Achievement>) -> Vec<AchievementRecord> {
    achievements
        .into_iter()
        .enumerate()
        .map(|(position, achievement)| AchievementRecord {
            id: achievement.id,
            name: achievement.name,
            description: achievement.description,
            hidden: achievement.hidden,
            icon_url: achievement.icon_url,
            icon_gray_url: achievement.icon_gray_url,
            unlocked: achievement.unlocked,
            unlocked_at: achievement.unlocked_at,
            position: i64::try_from(position).unwrap_or(i64::MAX),
        })
        .collect()
}
