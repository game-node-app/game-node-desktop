//! The PC ports catalog worker (design D2, D3).
//!
//! One background thread owns every catalog fetch. Callers never wait on the network: [`open`]
//! and [`refresh`] post a command, and the result arrives through the event callback the app
//! bridges to `engine:snapshot`. [`status`] and [`entries`] are cheap shared reads.
//!
//! Refresh rules:
//!
//! - the first fetch runs when PC ports is first opened, never before;
//! - a snapshot older than a day is refreshed automatically, at most once a day and at most one
//!   attempt per day while a refresh keeps failing;
//! - a manual refresh is coalesced into a refresh that is already running;
//! - a new snapshot replaces the stored one only after the source returned a complete, validated
//!   listing; the overlay is applied after the swap. A failed refresh keeps the previous snapshot
//!   and reports `stale` (a snapshot exists) or `unavailable` (none does).
//!
//! The detail and release caches are in memory and expire after 15 minutes (D3). They perform
//! blocking I/O, so they also belong on a worker thread.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use gamenode_core::ports::{CatalogEntry, CatalogSnapshot, Overlay, PortRelease, PortStore};

use super::quiver::QuiverError;

/// A stored snapshot older than this is refreshed in the background.
pub const DAILY_REFRESH_MS: i64 = 24 * 60 * 60 * 1000;

/// Detail and release responses are reused for this long (design D3).
pub const LAZY_CACHE_MS: i64 = 15 * 60 * 1000;

/// The idle worker wakes at least this often to re-check the schedule. Production clocks advance
/// on their own; tests inject a clock and nudge the worker instead of waiting.
const MAX_IDLE_WAIT: Duration = Duration::from_secs(60);

/// The wall clock the worker schedules with. Tests inject a fake one.
pub trait Clock: Send + Sync {
    /// Unix milliseconds.
    fn now_millis(&self) -> i64;
}

/// The production clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_millis(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
            .unwrap_or(0)
    }
}

/// Where the catalog currently is. Serialized camelCase for the UI contract.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CatalogState {
    /// PC ports was never opened, so no fetch has run.
    #[default]
    Idle,
    /// A fetch is running; the previous snapshot may still be shown.
    Refreshing,
    /// The last refresh completed and the snapshot is current.
    Ready,
    /// No usable snapshot exists and the last fetch failed.
    Unavailable,
    /// A snapshot exists but the last refresh failed, or it is older than a day.
    Stale,
}

/// Why the last catalog refresh failed, in the UI's vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CatalogError {
    Unreachable,
    RateLimited,
    InvalidResponse,
    EmptyListing,
    Unknown,
}

/// The catalog status the app puts in `PortsView`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CatalogStatus {
    pub state: CatalogState,
    /// Unix milliseconds of the last complete stored snapshot; `None` before the first success.
    pub fetched_at: Option<i64>,
    /// Entries in the stored snapshot, before the overlay or any filter.
    pub entry_count: usize,
    /// Set while `stale` or `unavailable`, cleared by a successful refresh.
    pub error: Option<CatalogError>,
}

/// What the worker reports through its callback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogEvent {
    /// The stored snapshot was read and is available; `entries()` now holds the merged catalog.
    Loaded(CatalogStatus),
    /// A refresh started.
    Started(CatalogStatus),
    /// A refresh stored a new snapshot and re-applied the overlay.
    Updated(CatalogStatus),
    /// A refresh failed; the previous snapshot, if any, is unchanged.
    Failed(CatalogStatus),
}

impl CatalogEvent {
    /// The status carried by the event.
    pub fn status(&self) -> &CatalogStatus {
        match self {
            Self::Loaded(status)
            | Self::Started(status)
            | Self::Updated(status)
            | Self::Failed(status) => status,
        }
    }
}

/// The upstream operations the worker and the lazy caches need.
///
/// The production implementation is [`super::quiver::QuiverClient`]; tests supply their own.
pub trait CatalogSource: Send + Sync {
    /// The complete upstream listing.
    fn listing(&self) -> Result<Vec<CatalogEntry>, QuiverError>;
    /// One app with project, checking release and withdrawals.
    fn detail(&self, slug: &str) -> Result<CatalogEntry, QuiverError>;
    /// Every published release, newest first.
    fn releases(&self, slug: &str) -> Result<Vec<PortRelease>, QuiverError>;
}

/// The catalog worker. Cheap to share behind an `Arc`.
pub struct CatalogWorker {
    commands: Sender<Command>,
    shared: Arc<Shared>,
    source: Arc<dyn CatalogSource>,
    clock: Arc<dyn Clock>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl CatalogWorker {
    /// Starts the worker thread. The callback runs on that thread; it must not block.
    pub fn start(
        store: Arc<dyn PortStore>,
        source: Arc<dyn CatalogSource>,
        overlay: Arc<Overlay>,
        clock: Arc<dyn Clock>,
        on_event: Arc<dyn Fn(CatalogEvent) + Send + Sync>,
    ) -> Self {
        let shared = Arc::new(Shared::new(overlay));
        let (commands, receiver) = mpsc::channel();
        let worker_shared = Arc::clone(&shared);
        let worker_source = Arc::clone(&source);
        let worker_clock = Arc::clone(&clock);
        let join = std::thread::Builder::new()
            .name("ports-catalog".into())
            .spawn(move || {
                run(
                    receiver,
                    store,
                    worker_source,
                    worker_clock,
                    worker_shared,
                    on_event,
                );
            })
            .expect("the PC ports catalog worker could not start");
        Self {
            commands,
            shared,
            source,
            clock,
            join: Mutex::new(Some(join)),
        }
    }

    /// The first visit: read the stored snapshot and, when it is missing or older than a day,
    /// start the first fetch. Does not block.
    pub fn open(&self) {
        self.send(Command::Open);
    }

    /// An on-demand refresh. Coalesced into a refresh that is already running. Does not block.
    pub fn refresh(&self) {
        self.shared.refresh_requested.store(true, Ordering::SeqCst);
        self.send(Command::Wake);
    }

    /// The current catalog status.
    pub fn status(&self) -> CatalogStatus {
        lock(&self.shared.status).clone()
    }

    /// The catalog with the overlay already applied: hidden entries removed and only entries
    /// supporting Windows kept (design D4).
    pub fn entries(&self) -> Arc<Vec<CatalogEntry>> {
        Arc::clone(&lock(&self.shared.catalog).merged)
    }

    /// Replaces the overlay and re-merges the stored snapshot without another fetch.
    ///
    /// The overlay source uses this after it validates a newer remote copy (design D4).
    pub fn set_overlay(&self, overlay: Arc<Overlay>) {
        let mut catalog = lock(&self.shared.catalog);
        catalog.merged = Arc::new(overlay.apply(catalog.raw.as_ref().clone()));
        catalog.overlay = overlay;
    }

    /// One entry's upstream detail, cached for 15 minutes and without the overlay; the caller
    /// merges the current overlay with [`Overlay::merge`]. Blocking: call from a worker thread.
    pub fn detail(&self, slug: &str) -> Result<Arc<CatalogEntry>, QuiverError> {
        let now = self.clock.now_millis();
        cached(&self.shared.details, now, slug, || self.source.detail(slug))
    }

    /// One entry's releases, cached for 15 minutes. Blocking: call from a worker thread.
    pub fn releases(&self, slug: &str) -> Result<Arc<Vec<PortRelease>>, QuiverError> {
        let now = self.clock.now_millis();
        cached(&self.shared.releases, now, slug, || {
            self.source.releases(slug)
        })
    }

    /// Stops the worker and waits for it to finish. An in-flight refresh is allowed to complete,
    /// so call this from a shutdown path, not from the UI thread.
    pub fn shutdown(&self) {
        self.send(Command::Stop);
        if let Some(join) = lock(&self.join).take() {
            let _ = join.join();
        }
    }

    fn send(&self, command: Command) {
        // A closed channel only means the worker already stopped.
        let _ = self.commands.send(command);
    }

    /// A test-only nudge: wakes the scheduler and waits until it processed the wake.
    #[cfg(test)]
    fn nudge(&self) {
        self.send(Command::Wake);
        let (reply, answered) = mpsc::channel();
        self.send(Command::Ping(reply));
        let _ = answered.recv_timeout(Duration::from_secs(10));
    }
}

impl Drop for CatalogWorker {
    fn drop(&mut self) {
        self.send(Command::Stop);
    }
}

enum Command {
    Open,
    Wake,
    Stop,
    #[cfg(test)]
    Ping(Sender<()>),
}

struct Shared {
    status: Mutex<CatalogStatus>,
    catalog: Mutex<Catalog>,
    details: Mutex<HashMap<String, Cached<CatalogEntry>>>,
    releases: Mutex<HashMap<String, Cached<Vec<PortRelease>>>>,
    opened: AtomicBool,
    loaded: AtomicBool,
    refresh_requested: AtomicBool,
    /// Unix milliseconds of the last refresh attempt; 0 means none yet.
    last_attempt: AtomicI64,
}

impl Shared {
    fn new(overlay: Arc<Overlay>) -> Self {
        Self {
            status: Mutex::new(CatalogStatus::default()),
            catalog: Mutex::new(Catalog {
                merged: Arc::new(Vec::new()),
                raw: Arc::new(Vec::new()),
                overlay,
            }),
            details: Mutex::new(HashMap::new()),
            releases: Mutex::new(HashMap::new()),
            opened: AtomicBool::new(false),
            loaded: AtomicBool::new(false),
            refresh_requested: AtomicBool::new(false),
            last_attempt: AtomicI64::new(0),
        }
    }

    fn last_attempt(&self) -> Option<i64> {
        let at = self.last_attempt.load(Ordering::SeqCst);
        (at > 0).then_some(at)
    }
}

struct Catalog {
    overlay: Arc<Overlay>,
    /// The upstream entries exactly as fetched, before the overlay.
    raw: Arc<Vec<CatalogEntry>>,
    /// `raw` with the overlay applied; what the app reads.
    merged: Arc<Vec<CatalogEntry>>,
}

struct Cached<T> {
    at: i64,
    value: Arc<T>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn cached<T>(
    cache: &Mutex<HashMap<String, Cached<T>>>,
    now: i64,
    slug: &str,
    fetch: impl FnOnce() -> Result<T, QuiverError>,
) -> Result<Arc<T>, QuiverError> {
    if let Some(cached) = lock(cache).get(slug)
        && now.saturating_sub(cached.at) < LAZY_CACHE_MS
    {
        return Ok(Arc::clone(&cached.value));
    }
    let value = Arc::new(fetch()?);
    lock(cache).insert(
        slug.to_owned(),
        Cached {
            at: now,
            value: Arc::clone(&value),
        },
    );
    Ok(value)
}

fn run(
    commands: Receiver<Command>,
    store: Arc<dyn PortStore>,
    source: Arc<dyn CatalogSource>,
    clock: Arc<dyn Clock>,
    shared: Arc<Shared>,
    on_event: Arc<dyn Fn(CatalogEvent) + Send + Sync>,
) {
    loop {
        let now = clock.now_millis();
        let wait = idle_wait(&shared, now);
        match commands.recv_timeout(wait) {
            Ok(Command::Open) => {
                load_snapshot(&store, clock.now_millis(), &shared, &on_event);
                shared.opened.store(true, Ordering::SeqCst);
                if auto_refresh_due(&shared, clock.now_millis()) {
                    refresh(&store, &source, &clock, &shared, &on_event);
                }
            }
            Ok(Command::Wake) => {
                handle_wake(&store, &source, &clock, &shared, &on_event);
            }
            Ok(Command::Stop) => break,
            #[cfg(test)]
            Ok(Command::Ping(reply)) => {
                let _ = reply.send(());
            }
            Err(RecvTimeoutError::Timeout) => {
                if shared.opened.load(Ordering::SeqCst) {
                    handle_wake(&store, &source, &clock, &shared, &on_event);
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// Runs a refresh when one was requested or a scheduled one is due.
fn handle_wake(
    store: &Arc<dyn PortStore>,
    source: &Arc<dyn CatalogSource>,
    clock: &Arc<dyn Clock>,
    shared: &Arc<Shared>,
    on_event: &Arc<dyn Fn(CatalogEvent) + Send + Sync>,
) {
    let requested = shared.refresh_requested.swap(false, Ordering::SeqCst);
    let due = shared.opened.load(Ordering::SeqCst) && auto_refresh_due(shared, clock.now_millis());
    if requested || due {
        refresh(store, source, clock, shared, on_event);
    }
}

fn load_snapshot(
    store: &Arc<dyn PortStore>,
    now: i64,
    shared: &Arc<Shared>,
    on_event: &Arc<dyn Fn(CatalogEvent) + Send + Sync>,
) {
    if shared.loaded.swap(true, Ordering::SeqCst) {
        return;
    }
    match store.catalog_snapshot() {
        Ok(Some(snapshot)) => {
            let state = if snapshot_is_fresh(&snapshot, now) {
                CatalogState::Ready
            } else {
                CatalogState::Stale
            };
            replace_catalog(shared, &snapshot);
            let status = set_status(
                shared,
                state,
                Some(snapshot.fetched_at),
                snapshot.entry_count(),
                None,
            );
            on_event(CatalogEvent::Loaded(status));
        }
        Ok(None) => {}
        Err(_) => {
            let status = set_status(
                shared,
                CatalogState::Unavailable,
                None,
                0,
                Some(CatalogError::Unknown),
            );
            on_event(CatalogEvent::Failed(status));
        }
    }
}

fn refresh(
    store: &Arc<dyn PortStore>,
    source: &Arc<dyn CatalogSource>,
    clock: &Arc<dyn Clock>,
    shared: &Arc<Shared>,
    on_event: &Arc<dyn Fn(CatalogEvent) + Send + Sync>,
) {
    // A manual refresh can be the first thing that runs; read the stored snapshot first so a
    // failure can still fall back to it.
    if !shared.loaded.load(Ordering::SeqCst) {
        load_snapshot(store, clock.now_millis(), shared, on_event);
    }
    shared
        .last_attempt
        .store(clock.now_millis().max(1), Ordering::SeqCst);
    let started = set_state(shared, CatalogState::Refreshing, None);
    on_event(CatalogEvent::Started(started));

    let result = source.listing().and_then(validate_listing);
    match result {
        Ok(entries) => {
            // The stamp is the completed, validated fetch time, not the request time.
            let snapshot = CatalogSnapshot::new(clock.now_millis(), entries);
            match store.replace_catalog_snapshot(&snapshot) {
                Ok(()) => {
                    // The store swap is the commit point; the overlay is applied after it.
                    replace_catalog(shared, &snapshot);
                    let status = set_status(
                        shared,
                        CatalogState::Ready,
                        Some(snapshot.fetched_at),
                        snapshot.entry_count(),
                        None,
                    );
                    on_event(CatalogEvent::Updated(status));
                }
                Err(_) => {
                    let status = fail(shared, CatalogError::Unknown);
                    on_event(CatalogEvent::Failed(status));
                }
            }
        }
        Err(error) => {
            let status = fail(shared, catalog_error(&error));
            on_event(CatalogEvent::Failed(status));
        }
    }
    // A refresh requested while this one ran is folded into it instead of running again.
    shared.refresh_requested.store(false, Ordering::SeqCst);
}

/// The worker's own safety net: a snapshot only ever contains complete, non-empty listings.
fn validate_listing(entries: Vec<CatalogEntry>) -> Result<Vec<CatalogEntry>, QuiverError> {
    if entries.is_empty() {
        return Err(QuiverError::EmptyListing);
    }
    if entries.iter().any(|entry| !entry.is_complete()) {
        return Err(QuiverError::InvalidResponse);
    }
    Ok(entries)
}

fn replace_catalog(shared: &Shared, snapshot: &CatalogSnapshot) {
    let mut catalog = lock(&shared.catalog);
    catalog.raw = Arc::new(snapshot.entries.clone());
    catalog.merged = Arc::new(catalog.overlay.apply(snapshot.entries.clone()));
}

fn set_status(
    shared: &Shared,
    state: CatalogState,
    fetched_at: Option<i64>,
    entry_count: usize,
    error: Option<CatalogError>,
) -> CatalogStatus {
    let mut status = lock(&shared.status);
    status.state = state;
    status.fetched_at = fetched_at;
    status.entry_count = entry_count;
    status.error = error;
    status.clone()
}

fn set_state(shared: &Shared, state: CatalogState, error: Option<CatalogError>) -> CatalogStatus {
    let mut status = lock(&shared.status);
    status.state = state;
    status.error = error;
    status.clone()
}

/// Keeps the previous snapshot and reports why the refresh failed.
fn fail(shared: &Shared, error: CatalogError) -> CatalogStatus {
    let has_snapshot = lock(&shared.status).fetched_at.is_some();
    let state = if has_snapshot {
        CatalogState::Stale
    } else {
        CatalogState::Unavailable
    };
    set_state(shared, state, Some(error))
}

fn catalog_error(error: &QuiverError) -> CatalogError {
    match error {
        QuiverError::Unreachable => CatalogError::Unreachable,
        QuiverError::RateLimited => CatalogError::RateLimited,
        QuiverError::EmptyListing => CatalogError::EmptyListing,
        QuiverError::NotFound
        | QuiverError::InvalidResponse
        | QuiverError::Truncated
        | QuiverError::TooManySkipped { .. } => CatalogError::InvalidResponse,
    }
}

fn snapshot_is_fresh(snapshot: &CatalogSnapshot, now: i64) -> bool {
    now.saturating_sub(snapshot.fetched_at) < DAILY_REFRESH_MS
}

/// True when an automatic refresh may run now: the snapshot is a day old and the previous
/// attempt is a day old too, so a failing upstream cannot create a retry storm.
fn auto_refresh_due(shared: &Shared, now: i64) -> bool {
    let success_age_ok = lock(&shared.status)
        .fetched_at
        .is_none_or(|at| now.saturating_sub(at) >= DAILY_REFRESH_MS);
    let attempt_age_ok = shared
        .last_attempt()
        .is_none_or(|at| now.saturating_sub(at) >= DAILY_REFRESH_MS);
    success_age_ok && attempt_age_ok
}

fn idle_wait(shared: &Shared, now: i64) -> Duration {
    if !shared.opened.load(Ordering::SeqCst) {
        return MAX_IDLE_WAIT;
    }
    if shared.refresh_requested.load(Ordering::SeqCst) {
        return Duration::ZERO;
    }
    let success_due = lock(&shared.status)
        .fetched_at
        .map(|at| at.saturating_add(DAILY_REFRESH_MS));
    let attempt_due = shared
        .last_attempt()
        .map(|at| at.saturating_add(DAILY_REFRESH_MS));
    let due = match (success_due, attempt_due) {
        (Some(success), Some(attempt)) => success.max(attempt),
        (Some(due), None) | (None, Some(due)) => due,
        (None, None) => return MAX_IDLE_WAIT,
    };
    let remaining = due.saturating_sub(now);
    if remaining <= 0 {
        // A due refresh runs from the timeout branch; do not spin when it cannot.
        return if auto_refresh_due(shared, now) {
            Duration::ZERO
        } else {
            MAX_IDLE_WAIT
        };
    }
    Duration::from_millis(u64::try_from(remaining).unwrap_or(u64::MAX)).min(MAX_IDLE_WAIT)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::AtomicUsize;
    use std::time::Instant;

    use gamenode_core::drm_free::Registration;
    use gamenode_core::ports::{
        PortInstallation, PortJobResume, PortJobRow, PortOverlayRow, PortPreferences, StorageError,
    };

    use super::*;

    const T0: i64 = 1_790_000_000_000;

    #[derive(Clone, Default)]
    struct FakeClock {
        now: Arc<AtomicI64>,
    }

    impl FakeClock {
        fn new(now: i64) -> Self {
            Self {
                now: Arc::new(AtomicI64::new(now)),
            }
        }

        fn set(&self, now: i64) {
            self.now.store(now, Ordering::SeqCst);
        }

        fn advance(&self, millis: i64) {
            self.now.fetch_add(millis, Ordering::SeqCst);
        }
    }

    impl Clock for FakeClock {
        fn now_millis(&self) -> i64 {
            self.now.load(Ordering::SeqCst)
        }
    }

    #[derive(Clone, Default)]
    struct FakeStore {
        snapshot: Arc<Mutex<Option<CatalogSnapshot>>>,
        replaced: Arc<Mutex<Vec<CatalogSnapshot>>>,
    }

    impl FakeStore {
        fn with_snapshot(snapshot: CatalogSnapshot) -> Self {
            Self {
                snapshot: Arc::new(Mutex::new(Some(snapshot))),
                replaced: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn snapshot(&self) -> Option<CatalogSnapshot> {
            lock(&self.snapshot).clone()
        }

        fn replaced(&self) -> Vec<CatalogSnapshot> {
            lock(&self.replaced).clone()
        }
    }

    impl PortStore for FakeStore {
        fn replace_catalog_snapshot(&self, snapshot: &CatalogSnapshot) -> Result<(), StorageError> {
            *lock(&self.snapshot) = Some(snapshot.clone());
            lock(&self.replaced).push(snapshot.clone());
            Ok(())
        }

        fn catalog_snapshot(&self) -> Result<Option<CatalogSnapshot>, StorageError> {
            Ok(self.snapshot())
        }

        fn overlay(&self) -> Result<Option<PortOverlayRow>, StorageError> {
            Ok(None)
        }

        fn save_overlay(&self, _overlay: &PortOverlayRow) -> Result<(), StorageError> {
            Ok(())
        }

        fn jobs(&self) -> Result<Vec<PortJobRow>, StorageError> {
            Ok(Vec::new())
        }

        fn job(&self, _id: &str) -> Result<Option<PortJobRow>, StorageError> {
            Ok(None)
        }

        fn save_job(&self, _job: &PortJobRow) -> Result<(), StorageError> {
            Ok(())
        }

        fn save_job_resume(
            &self,
            _job_id: &str,
            _resume: &PortJobResume,
        ) -> Result<(), StorageError> {
            Ok(())
        }

        fn job_resume(&self, _job_id: &str) -> Result<Option<PortJobResume>, StorageError> {
            Ok(None)
        }

        fn record_created_path(&self, _job_id: &str, _path: &str) -> Result<(), StorageError> {
            Ok(())
        }

        fn created_paths(&self, _job_id: &str) -> Result<Vec<String>, StorageError> {
            Ok(Vec::new())
        }

        fn commit_install(
            &self,
            _job: &PortJobRow,
            _registration: &Registration,
            _installation: &PortInstallation,
        ) -> Result<(), StorageError> {
            Ok(())
        }

        fn installations(&self) -> Result<Vec<PortInstallation>, StorageError> {
            Ok(Vec::new())
        }

        fn save_installation(&self, _installation: &PortInstallation) -> Result<(), StorageError> {
            Ok(())
        }

        fn relink_installation(
            &self,
            _job_id: &str,
            _registration_id: &str,
        ) -> Result<(), StorageError> {
            Ok(())
        }

        fn preferences(&self) -> Result<PortPreferences, StorageError> {
            Ok(PortPreferences::default())
        }

        fn save_preferences(&self, _preferences: &PortPreferences) -> Result<(), StorageError> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeSource {
        listing_calls: AtomicUsize,
        listing_results: Mutex<VecDeque<Result<Vec<CatalogEntry>, QuiverError>>>,
        listing_started: Mutex<Option<Sender<()>>>,
        listing_gate: Mutex<Option<Receiver<()>>>,
        detail_calls: AtomicUsize,
        detail_results: Mutex<VecDeque<Result<CatalogEntry, QuiverError>>>,
        release_calls: AtomicUsize,
        release_results: Mutex<VecDeque<Result<Vec<PortRelease>, QuiverError>>>,
    }

    impl FakeSource {
        fn listing_calls(&self) -> usize {
            self.listing_calls.load(Ordering::SeqCst)
        }

        fn release_calls(&self) -> usize {
            self.release_calls.load(Ordering::SeqCst)
        }

        fn push_listing(&self, result: Result<Vec<CatalogEntry>, QuiverError>) {
            lock(&self.listing_results).push_back(result);
        }

        fn push_detail(&self, result: Result<CatalogEntry, QuiverError>) {
            lock(&self.detail_results).push_back(result);
        }

        fn push_releases(&self, result: Result<Vec<PortRelease>, QuiverError>) {
            lock(&self.release_results).push_back(result);
        }
    }

    impl CatalogSource for FakeSource {
        fn listing(&self) -> Result<Vec<CatalogEntry>, QuiverError> {
            self.listing_calls.fetch_add(1, Ordering::SeqCst);
            if let Some(started) = lock(&self.listing_started).take() {
                let _ = started.send(());
            }
            if let Some(gate) = lock(&self.listing_gate).take() {
                let _ = gate.recv_timeout(Duration::from_secs(10));
            }
            lock(&self.listing_results)
                .pop_front()
                .unwrap_or_else(|| Ok(vec![entry("qcat_default")]))
        }

        fn detail(&self, _slug: &str) -> Result<CatalogEntry, QuiverError> {
            self.detail_calls.fetch_add(1, Ordering::SeqCst);
            lock(&self.detail_results)
                .pop_front()
                .unwrap_or_else(|| Ok(entry("qcat_detail")))
        }

        fn releases(&self, _slug: &str) -> Result<Vec<PortRelease>, QuiverError> {
            self.release_calls.fetch_add(1, Ordering::SeqCst);
            lock(&self.release_results)
                .pop_front()
                .unwrap_or_else(|| Ok(vec![release("v1")]))
        }
    }

    fn entry(catalog_id: &str) -> CatalogEntry {
        CatalogEntry {
            catalog_id: catalog_id.into(),
            slug: format!("{catalog_id}-slug"),
            name: format!("{catalog_id} name"),
            supported_os: vec!["windows".into()],
            ..CatalogEntry::default()
        }
    }

    fn release(version: &str) -> PortRelease {
        PortRelease {
            release_id: format!("release-{version}"),
            version: version.into(),
            ..PortRelease::default()
        }
    }

    struct Harness {
        worker: CatalogWorker,
        store: FakeStore,
        source: Arc<FakeSource>,
        events: Receiver<CatalogEvent>,
    }

    fn start(store: FakeStore, source: Arc<FakeSource>, clock: FakeClock) -> Harness {
        start_with_overlay(store, source, clock, Arc::new(Overlay::default()))
    }

    fn start_with_overlay(
        store: FakeStore,
        source: Arc<FakeSource>,
        clock: FakeClock,
        overlay: Arc<Overlay>,
    ) -> Harness {
        let (events, received) = mpsc::channel();
        let on_event = Arc::new(move |event| {
            let _ = events.send(event);
        });
        let worker = CatalogWorker::start(
            Arc::new(store.clone()),
            Arc::clone(&source) as Arc<dyn CatalogSource>,
            overlay,
            Arc::new(clock),
            on_event,
        );
        Harness {
            worker,
            store,
            source,
            events: received,
        }
    }

    fn wait_event(
        events: &Receiver<CatalogEvent>,
        predicate: impl Fn(&CatalogEvent) -> bool,
    ) -> CatalogEvent {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "timed out waiting for a catalog event"
            );
            match events.recv_timeout(remaining) {
                Ok(event) if predicate(&event) => return event,
                Ok(_) => {}
                Err(_) => panic!("timed out waiting for a catalog event"),
            }
        }
    }

    fn wait_updated(events: &Receiver<CatalogEvent>) -> CatalogStatus {
        wait_event(events, |event| matches!(event, CatalogEvent::Updated(_)))
            .status()
            .clone()
    }

    #[test]
    fn the_worker_is_shareable_across_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CatalogWorker>();
        assert_send_sync::<super::super::quiver::QuiverClient>();
    }

    #[test]
    fn catalog_status_uses_the_ui_wire_vocabulary() {
        let status = CatalogStatus {
            state: CatalogState::Stale,
            fetched_at: Some(T0),
            entry_count: 3,
            error: Some(CatalogError::RateLimited),
        };
        let json = serde_json::to_value(&status).expect("serializable");
        assert_eq!(json["state"], "stale");
        assert_eq!(json["error"], "rateLimited");
        assert_eq!(json["fetchedAt"], T0);
        assert_eq!(json["entryCount"], 3);
    }

    #[test]
    fn the_worker_starts_idle() {
        let harness = start(
            FakeStore::default(),
            Arc::new(FakeSource::default()),
            FakeClock::new(T0),
        );
        let status = harness.worker.status();
        assert_eq!(status.state, CatalogState::Idle);
        assert_eq!(status.fetched_at, None);
        assert_eq!(status.entry_count, 0);
        assert!(harness.worker.entries().is_empty());
        assert_eq!(harness.source.listing_calls(), 0);
    }

    #[test]
    fn first_visit_fetches_validates_and_stores_the_snapshot() {
        let source = Arc::new(FakeSource::default());
        source.push_listing(Ok(vec![entry("qcat_a"), entry("qcat_b")]));
        let harness = start(
            FakeStore::default(),
            Arc::clone(&source),
            FakeClock::new(T0),
        );

        harness.worker.open();
        let status = wait_updated(&harness.events);

        assert_eq!(status.state, CatalogState::Ready);
        assert_eq!(status.fetched_at, Some(T0));
        assert_eq!(status.entry_count, 2);
        assert_eq!(status.error, None);
        assert_eq!(source.listing_calls(), 1);
        assert_eq!(harness.worker.entries().len(), 2);

        let stored = harness.store.snapshot().expect("the snapshot was stored");
        assert_eq!(stored.fetched_at, T0);
        assert_eq!(stored.entry_count(), 2);
        assert_eq!(harness.store.replaced().len(), 1);
    }

    #[test]
    fn a_cached_snapshot_is_shown_without_a_fetch_within_a_day() {
        let store = FakeStore::with_snapshot(CatalogSnapshot::new(T0, vec![entry("qcat_cached")]));
        let source = Arc::new(FakeSource::default());
        let harness = start(
            store,
            Arc::clone(&source),
            FakeClock::new(T0 + DAILY_REFRESH_MS - 1),
        );

        harness.worker.open();
        let status = wait_event(&harness.events, |event| {
            matches!(event, CatalogEvent::Loaded(_))
        })
        .status()
        .clone();

        assert_eq!(status.state, CatalogState::Ready);
        assert_eq!(status.fetched_at, Some(T0));
        assert_eq!(status.entry_count, 1);
        assert_eq!(harness.worker.entries().len(), 1);
        harness.worker.nudge();
        assert_eq!(source.listing_calls(), 0);
    }

    #[test]
    fn daily_refresh_fires_exactly_when_due() {
        let source = Arc::new(FakeSource::default());
        source.push_listing(Ok(vec![entry("qcat_first")]));
        source.push_listing(Ok(vec![entry("qcat_second")]));
        let clock = FakeClock::new(T0);
        let harness = start(FakeStore::default(), Arc::clone(&source), clock.clone());

        harness.worker.open();
        wait_updated(&harness.events);
        assert_eq!(source.listing_calls(), 1);

        clock.set(T0 + DAILY_REFRESH_MS - 1);
        harness.worker.nudge();
        assert_eq!(source.listing_calls(), 1, "a day has not passed yet");

        clock.set(T0 + DAILY_REFRESH_MS);
        harness.worker.nudge();
        assert_eq!(source.listing_calls(), 2, "the daily refresh ran when due");
        let status = harness.worker.status();
        assert_eq!(status.state, CatalogState::Ready);
        assert_eq!(status.fetched_at, Some(T0 + DAILY_REFRESH_MS));
    }

    #[test]
    fn a_failed_refresh_keeps_the_old_snapshot_and_reports_stale() {
        let source = Arc::new(FakeSource::default());
        source.push_listing(Ok(vec![entry("qcat_a")]));
        source.push_listing(Err(QuiverError::Unreachable));
        let clock = FakeClock::new(T0);
        let harness = start(FakeStore::default(), Arc::clone(&source), clock.clone());

        harness.worker.open();
        wait_updated(&harness.events);
        let before = harness.store.snapshot().expect("initial snapshot");

        clock.set(T0 + DAILY_REFRESH_MS);
        harness.worker.nudge();

        let status = harness.worker.status();
        assert_eq!(status.state, CatalogState::Stale);
        assert_eq!(status.error, Some(CatalogError::Unreachable));
        assert_eq!(status.fetched_at, Some(T0));
        assert_eq!(status.entry_count, 1);
        assert_eq!(harness.store.snapshot(), Some(before));
        assert_eq!(
            harness.store.replaced().len(),
            1,
            "a failed refresh must not replace the stored snapshot"
        );
        assert_eq!(harness.worker.entries().len(), 1);
    }

    #[test]
    fn a_failed_first_refresh_is_unavailable() {
        let source = Arc::new(FakeSource::default());
        source.push_listing(Err(QuiverError::RateLimited));
        let harness = start(
            FakeStore::default(),
            Arc::clone(&source),
            FakeClock::new(T0),
        );

        harness.worker.open();
        let status = wait_event(&harness.events, |event| {
            matches!(event, CatalogEvent::Failed(_))
        })
        .status()
        .clone();

        assert_eq!(status.state, CatalogState::Unavailable);
        assert_eq!(status.error, Some(CatalogError::RateLimited));
        assert_eq!(status.fetched_at, None);
        assert_eq!(status.entry_count, 0);
        assert!(harness.store.snapshot().is_none());
        assert!(harness.worker.entries().is_empty());
    }

    #[test]
    fn an_empty_listing_is_a_failed_refresh() {
        let source = Arc::new(FakeSource::default());
        source.push_listing(Ok(Vec::new()));
        let harness = start(
            FakeStore::default(),
            Arc::clone(&source),
            FakeClock::new(T0),
        );

        harness.worker.open();
        let status = wait_event(&harness.events, |event| {
            matches!(event, CatalogEvent::Failed(_))
        })
        .status()
        .clone();

        assert_eq!(status.state, CatalogState::Unavailable);
        assert_eq!(status.error, Some(CatalogError::EmptyListing));
        assert!(harness.store.snapshot().is_none());
    }

    #[test]
    fn manual_refresh_coalesces_while_one_runs() {
        let source = Arc::new(FakeSource::default());
        let (started, fetched) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        *lock(&source.listing_started) = Some(started);
        *lock(&source.listing_gate) = Some(gate);
        source.push_listing(Ok(vec![entry("qcat_a")]));
        let harness = start(
            FakeStore::default(),
            Arc::clone(&source),
            FakeClock::new(T0),
        );

        harness.worker.open();
        fetched
            .recv_timeout(Duration::from_secs(10))
            .expect("the first fetch started");
        // Both requests arrive while the fetch is blocked in the source.
        harness.worker.refresh();
        harness.worker.refresh();
        release.send(()).expect("release the fetch");

        wait_updated(&harness.events);
        harness.worker.nudge();
        assert_eq!(
            source.listing_calls(),
            1,
            "queued manual refreshes fold into the running one"
        );
        assert_eq!(harness.store.replaced().len(), 1);
        assert_eq!(harness.worker.status().state, CatalogState::Ready);
    }

    #[test]
    fn the_overlay_is_applied_after_the_snapshot_swap() {
        let overlay = Overlay::parse(
            r#"
schema_version = 1

[entry."qcat_hidden"]
hidden = true
"#,
        )
        .expect("valid overlay");
        let source = Arc::new(FakeSource::default());
        let linux = CatalogEntry {
            catalog_id: "qcat_linux".into(),
            slug: "linux".into(),
            name: "Linux".into(),
            supported_os: vec!["linux".into()],
            ..CatalogEntry::default()
        };
        source.push_listing(Ok(vec![entry("qcat_visible"), entry("qcat_hidden"), linux]));
        let harness = start_with_overlay(
            FakeStore::default(),
            Arc::clone(&source),
            FakeClock::new(T0),
            Arc::new(overlay),
        );

        harness.worker.open();
        wait_updated(&harness.events);

        let entries = harness.worker.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].catalog_id, "qcat_visible");
    }

    #[test]
    fn replacing_the_overlay_remerges_without_a_fetch() {
        let source = Arc::new(FakeSource::default());
        source.push_listing(Ok(vec![entry("qcat_a"), entry("qcat_b")]));
        let harness = start(
            FakeStore::default(),
            Arc::clone(&source),
            FakeClock::new(T0),
        );

        harness.worker.open();
        wait_updated(&harness.events);
        assert_eq!(harness.worker.entries().len(), 2);

        let overlay = Overlay::parse(
            r#"
schema_version = 1

[entry."qcat_b"]
hidden = true
"#,
        )
        .expect("valid overlay");
        harness.worker.set_overlay(Arc::new(overlay));

        let entries = harness.worker.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].catalog_id, "qcat_a");
        assert_eq!(source.listing_calls(), 1);
    }

    #[test]
    fn the_release_cache_expires_after_fifteen_minutes() {
        let source = Arc::new(FakeSource::default());
        source.push_releases(Ok(vec![release("v1")]));
        source.push_releases(Ok(vec![release("v2")]));
        source.push_releases(Ok(vec![release("v3")]));
        let clock = FakeClock::new(T0);
        let harness = start(FakeStore::default(), Arc::clone(&source), clock.clone());

        let first = harness.worker.releases("slug-a").expect("first fetch");
        assert_eq!(first[0].version, "v1");
        assert_eq!(source.release_calls(), 1);

        clock.advance(LAZY_CACHE_MS - 1);
        let cached = harness.worker.releases("slug-a").expect("cache hit");
        assert_eq!(cached[0].version, "v1");
        assert_eq!(source.release_calls(), 1, "still inside 15 minutes");

        clock.advance(1);
        let refreshed = harness.worker.releases("slug-a").expect("cache expired");
        assert_eq!(refreshed[0].version, "v2");
        assert_eq!(source.release_calls(), 2, "15 minutes expired the cache");

        // A different slug has its own entry.
        let other = harness.worker.releases("slug-b").expect("other slug");
        assert_eq!(other[0].version, "v3");
        assert_eq!(source.release_calls(), 3);
    }

    #[test]
    fn a_failed_release_fetch_is_not_cached() {
        let source = Arc::new(FakeSource::default());
        source.push_releases(Err(QuiverError::Unreachable));
        source.push_releases(Ok(vec![release("v1")]));
        let harness = start(
            FakeStore::default(),
            Arc::clone(&source),
            FakeClock::new(T0),
        );

        assert_eq!(
            harness.worker.releases("slug-a").unwrap_err(),
            QuiverError::Unreachable
        );
        assert_eq!(harness.worker.releases("slug-a").unwrap()[0].version, "v1");
        assert_eq!(source.release_calls(), 2);
    }

    #[test]
    fn the_detail_cache_expires_after_fifteen_minutes() {
        let source = Arc::new(FakeSource::default());
        let mut first = entry("qcat_first");
        first.name = "First detail".into();
        let mut second = entry("qcat_second");
        second.name = "Second detail".into();
        source.push_detail(Ok(first));
        source.push_detail(Ok(second));
        let clock = FakeClock::new(T0);
        let harness = start(FakeStore::default(), Arc::clone(&source), clock.clone());

        assert_eq!(
            harness.worker.detail("slug-a").unwrap().name,
            "First detail"
        );
        clock.advance(LAZY_CACHE_MS - 1);
        assert_eq!(
            harness.worker.detail("slug-a").unwrap().name,
            "First detail"
        );
        assert_eq!(source.detail_calls.load(Ordering::SeqCst), 1);

        clock.advance(1);
        assert_eq!(
            harness.worker.detail("slug-a").unwrap().name,
            "Second detail"
        );
        assert_eq!(source.detail_calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn failed_catalog_errors_map_to_the_ui_vocabulary() {
        for (error, expected) in [
            (QuiverError::Unreachable, CatalogError::Unreachable),
            (QuiverError::RateLimited, CatalogError::RateLimited),
            (QuiverError::EmptyListing, CatalogError::EmptyListing),
            (QuiverError::NotFound, CatalogError::InvalidResponse),
            (QuiverError::InvalidResponse, CatalogError::InvalidResponse),
            (QuiverError::Truncated, CatalogError::InvalidResponse),
            (
                QuiverError::TooManySkipped {
                    skipped: 3,
                    total: 10,
                },
                CatalogError::InvalidResponse,
            ),
        ] {
            assert_eq!(catalog_error(&error), expected, "{error:?}");
        }
    }
}
