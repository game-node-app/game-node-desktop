//! Caller-owned jobs of third-party plugins, served through `ResourceHost.JobsGet/Watch/Cancel`.
//!
//! A long operation (an install, a backup, a restore) answers with a job instead of blocking the
//! call. The job belongs to the host-authenticated owner ([`ExternalNamespaceResolver::job_owner`]:
//! plugin id, pinned publisher fingerprint and account profile) and to the profile epoch it was
//! created in. A request names a job by its opaque id and nothing else: an id that is unknown,
//! another owner's, from an earlier profile or already forgotten is the same `NOT_FOUND`, so an
//! id says nothing about jobs it does not own.
//!
//! Every job remembers the permission of the operation that created it. Reading, watching and
//! cancelling it need that permission *still*; the host checks it on every call and before every
//! event a watch delivers. The work itself rechecks through [`JobContext::check`] before it
//! commits an effect, so a permission lost, a profile switched or a cancellation asked for
//! between the start and the commit stops the effect.
//!
//! Retrying a request with the same idempotency key answers with the same job and starts no new
//! work; the same key for a different request is refused. Keys are remembered for 24 hours, but
//! a finished job is only kept for an hour (and at most [`MAX_FINISHED_PER_OWNER`] of them), so
//! a retry that finds its job already forgotten is refused rather than run again.
//!
//! Jobs live in the host's memory: a host restart forgets them and their keys, and the work they
//! stood for does not resume. An operation whose effect must never repeat across a restart has to
//! be idempotent in its own domain (it is, for the installs and backups that will use this).

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gamenode_plugin_protocol::proto::{JobEvent, JobReply, JobState};
use tokio::sync::watch;
use tonic::Status;

use super::profile::ProfileContext;
use super::provider::ResourceCaller;

/// Jobs one owner may have running at once.
pub const MAX_ACTIVE_PER_OWNER: usize = 8;
/// Jobs running at once, across every plugin.
pub const MAX_ACTIVE_TOTAL: usize = 32;
/// Finished jobs one owner keeps; the oldest go first.
pub const MAX_FINISHED_PER_OWNER: usize = 32;
/// How long a finished job stays readable.
pub const FINISHED_RETENTION: Duration = Duration::from_secs(60 * 60);
/// How long an idempotency key is remembered.
pub const KEY_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);
/// Idempotency keys one owner keeps; the oldest go first.
pub const MAX_KEYS_PER_OWNER: usize = 1024;
/// How long a cancellation waits for the work to stop before answering with the state it is in.
pub const CANCEL_GRACE: Duration = Duration::from_secs(2);
/// The longest result message, in bytes.
pub const MAX_MESSAGE_BYTES: usize = 512;
/// The longest result code, in bytes.
pub const MAX_CODE_BYTES: usize = 64;

/// The code a job gets when its work panics or stops unexpectedly.
pub const CODE_INTERNAL: &str = "internal";
/// The code a job gets when its work reaches the commit after the requesting session ended.
pub const CODE_SESSION_ENDED: &str = "session_ended";

/// The check a job's work makes before committing: the caller's permission and session still
/// stand. Built by the service that creates the job, from the same guard its mutations use.
pub type Fence = Arc<dyn Fn() -> Result<(), Status> + Send + Sync>;

/// A job's work: runs once, away from the caller's request.
pub type Work =
    Box<dyn FnOnce(JobContext) -> Pin<Box<dyn Future<Output = Outcome> + Send>> + Send + 'static>;

/// Wraps an async block as a job's work.
pub fn work<F, Fut>(run: F) -> Work
where
    F: FnOnce(JobContext) -> Fut + Send + 'static,
    Fut: Future<Output = Outcome> + Send + 'static,
{
    Box::new(move |context| Box::pin(run(context)))
}

/// How a job ended. The code is a short machine-readable word; the message is for people.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Succeeded { code: String, message: String },
    Failed { code: String, message: String },
    Cancelled { message: String },
}

impl Outcome {
    pub fn succeeded(code: &str, message: &str) -> Self {
        Self::Succeeded {
            code: code.to_owned(),
            message: message.to_owned(),
        }
    }

    pub fn failed(code: &str, message: &str) -> Self {
        Self::Failed {
            code: code.to_owned(),
            message: message.to_owned(),
        }
    }

    pub fn cancelled(message: &str) -> Self {
        Self::Cancelled {
            message: message.to_owned(),
        }
    }

    /// The outcome of work stopped by a refused [`JobContext::check`].
    pub fn stopped(status: &Status) -> Self {
        match status.code() {
            tonic::Code::Cancelled => Self::cancelled("cancelled before anything was committed"),
            // The session that asked for the operation is gone (the plugin stopped or
            // restarted): the authority it gave ended with it.
            tonic::Code::Unauthenticated => Self::failed(
                CODE_SESSION_ENDED,
                "the plugin's session ended before the effect was committed; nothing was committed",
            ),
            tonic::Code::PermissionDenied => Self::failed(
                "permission_denied",
                "the plugin lost the permission; nothing was committed",
            ),
            _ => Self::failed(
                "failed_precondition",
                "the plugin's permissions or profile changed; nothing was committed",
            ),
        }
    }
}

/// Who owns a job: the host-authenticated plugin/publisher/profile and the profile epoch.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct JobOwner {
    key: [u8; 32],
    epoch: u64,
}

impl JobOwner {
    /// The owner of the caller's jobs. Only authenticated third-party plugins have a namespace
    /// and so jobs; official providers use the host's own adapters.
    pub(crate) fn of(caller: &ResourceCaller) -> Result<Self, Status> {
        let resolver = caller.namespace.as_ref().ok_or_else(|| {
            Status::failed_precondition("jobs are available to third-party plugins only")
        })?;
        Ok(Self {
            key: *resolver.job_owner().key(),
            epoch: caller.profile_epoch,
        })
    }

    #[cfg(test)]
    pub(crate) fn for_tests(key: u8, epoch: u64) -> Self {
        Self {
            key: [key; 32],
            epoch,
        }
    }
}

/// What the service that starts a job decides, besides the work itself.
pub struct Submission {
    /// The permission of the operation that creates the job; every later call needs it too.
    pub permission: &'static str,
    /// The request's idempotency key and a digest of its meaningful fields.
    pub key: String,
    pub hash: [u8; 32],
    /// Rechecked by the work before it commits.
    pub fence: Fence,
    /// The account profile; a change ends the job.
    pub profile: watch::Receiver<Option<ProfileContext>>,
    /// `Some(message)` when the work first waits for something the user must do: the job stays
    /// `QUEUED` with this message until the work calls [`JobContext::start`].
    pub awaiting: Option<&'static str>,
}

/// What the work sees of its own job.
pub struct JobContext {
    id: String,
    jobs: Jobs,
    cancel: watch::Receiver<bool>,
    fence: Fence,
}

impl JobContext {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn is_cancelled(&self) -> bool {
        *self.cancel.borrow()
    }

    /// Resolves when cancellation is requested (at once if it already was).
    pub async fn cancelled(&mut self) {
        let _ = self.cancel.wait_for(|cancelled| *cancelled).await;
    }

    /// Moves a job that was waiting for the user to `RUNNING`. Refused when the job was cancelled
    /// while it waited (it ends as cancelled and the work must stop). A job that does not wait is
    /// started by the host and needs no call.
    pub fn start(&self) -> Result<(), Status> {
        if self.jobs.begin(&self.id) {
            Ok(())
        } else {
            Err(Status::cancelled("the job was cancelled before it started"))
        }
    }

    /// Reports progress, 0 to 100. A value that does not go forward is ignored, which also keeps
    /// the events a job can ever produce bounded.
    pub fn progress(&self, percent: u32) {
        self.jobs.progress(&self.id, percent.min(100));
    }

    /// Call right before committing an effect: refuses after a cancellation was requested, a
    /// permission was lost or changed, or the profile changed.
    pub fn check(&self) -> Result<(), Status> {
        if self.is_cancelled() {
            return Err(Status::cancelled("the job was cancelled"));
        }
        (self.fence)()
    }
}

struct Job {
    owner: JobOwner,
    permission: &'static str,
    state: JobState,
    code: String,
    message: String,
    progress: u32,
    /// Every event so far. Progress only moves forward, so at most 103 exist.
    events: Vec<JobEvent>,
    cancel: watch::Sender<bool>,
    changed: watch::Sender<u64>,
    finished_at: Option<Instant>,
}

impl Job {
    fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            JobState::Succeeded | JobState::Failed | JobState::Cancelled
        )
    }

    fn sequence(&self) -> u64 {
        self.events.len() as u64
    }

    fn record(&mut self) {
        let sequence = self.sequence() + 1;
        self.events.push(JobEvent {
            job_id: String::new(),
            sequence,
            state: self.state as i32,
            progress_percent: self.progress,
            result_code: self.code.clone(),
            message: self.message.clone(),
        });
        self.changed.send_replace(sequence);
    }

    fn reply(&self, id: &str) -> JobReply {
        JobReply {
            job_id: id.to_owned(),
            state: self.state as i32,
            result_code: self.code.clone(),
            message: self.message.clone(),
            revision: self.sequence(),
        }
    }
}

struct Key {
    hash: [u8; 32],
    job_id: String,
    at: Instant,
}

#[derive(Default)]
struct State {
    jobs: HashMap<String, Job>,
    keys: HashMap<(JobOwner, String), Key>,
    /// The newest profile epoch any caller has presented.
    newest_epoch: u64,
}

type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

/// The host's job registry. Cheap to clone; clones share the same jobs.
#[derive(Clone)]
pub struct Jobs {
    state: Arc<Mutex<State>>,
    clock: Clock,
    cancel_grace: Duration,
}

impl Default for Jobs {
    fn default() -> Self {
        Self {
            state: Arc::default(),
            clock: Arc::new(Instant::now),
            cancel_grace: CANCEL_GRACE,
        }
    }
}

fn clip(text: &str, limit: usize) -> String {
    let mut clean: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if clean.len() > limit {
        let mut end = limit;
        while !clean.is_char_boundary(end) {
            end -= 1;
        }
        clean.truncate(end);
    }
    clean
}

fn not_found() -> Status {
    Status::not_found("job not found")
}

fn new_id() -> Result<String, Status> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| Status::internal("no source of randomness"))?;
    Ok(format!(
        "job-{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ))
}

impl Jobs {
    #[cfg(test)]
    pub(crate) fn with_clock(
        mut self,
        clock: impl Fn() -> Instant + Send + Sync + 'static,
    ) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_cancel_grace(mut self, grace: Duration) -> Self {
        self.cancel_grace = grace;
        self
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Forgets what expired and what belongs to a profile that is over.
    fn prune(&self, state: &mut State, epoch: u64) {
        let now = (self.clock)();
        state.newest_epoch = state.newest_epoch.max(epoch);
        let newest = state.newest_epoch;
        for job in state.jobs.values() {
            if !job.is_terminal() && job.owner.epoch < newest {
                job.cancel.send_replace(true);
            }
        }
        state.jobs.retain(|_, job| match job.finished_at {
            Some(at) => now.duration_since(at) < FINISHED_RETENTION && job.owner.epoch >= newest,
            None => true,
        });
        // The oldest finished jobs of an owner beyond the cap.
        let mut finished: HashMap<JobOwner, Vec<(Instant, String)>> = HashMap::new();
        for (id, job) in &state.jobs {
            if let Some(at) = job.finished_at {
                finished
                    .entry(job.owner.clone())
                    .or_default()
                    .push((at, id.clone()));
            }
        }
        for (_, mut ids) in finished {
            if ids.len() > MAX_FINISHED_PER_OWNER {
                ids.sort();
                for (_, id) in &ids[..ids.len() - MAX_FINISHED_PER_OWNER] {
                    state.jobs.remove(id);
                }
            }
        }
        state.keys.retain(|(owner, _), key| {
            now.duration_since(key.at) < KEY_RETENTION && owner.epoch >= newest
        });
    }

    /// The job an earlier request with the same idempotency key started, if any: `Ok(None)` when
    /// the key is new, and the same refusals as [`submit`](Self::submit) for a key used for
    /// another request or whose job is no longer kept. Lets a caller answer a retry before it
    /// spends anything (an intent, a dialog) on the request.
    pub fn replay(
        &self,
        owner: &JobOwner,
        key: &str,
        hash: [u8; 32],
    ) -> Result<Option<JobReply>, Status> {
        let mut state = self.lock();
        self.prune(&mut state, owner.epoch);
        let Some(seen) = state.keys.get(&(owner.clone(), key.to_owned())) else {
            return Ok(None);
        };
        if seen.hash != hash {
            return Err(Status::failed_precondition(
                "the idempotency key was already used for a different request",
            ));
        }
        match state.jobs.get(&seen.job_id) {
            Some(job) => Ok(Some(job.reply(&seen.job_id))),
            None => Err(Status::failed_precondition(
                "the job of this idempotency key finished and is no longer kept; \
                 use a new key to start it again",
            )),
        }
    }

    /// Starts `work` as a job of `owner`, or answers with the job an earlier request with the
    /// same idempotency key already started (`true`: it is a replay and nothing was started).
    ///
    /// Must be called inside the Tokio runtime.
    pub fn submit(
        &self,
        owner: &JobOwner,
        submission: Submission,
        work: Work,
    ) -> Result<(JobReply, bool), Status> {
        let Submission {
            permission,
            key,
            hash,
            fence,
            profile,
            awaiting,
        } = submission;
        let id = new_id()?;
        let (cancel, cancel_rx) = watch::channel(false);
        let (reply, context) = {
            let mut state = self.lock();
            self.prune(&mut state, owner.epoch);
            if owner.epoch < state.newest_epoch {
                return Err(Status::failed_precondition("resource context is stale"));
            }
            if let Some(seen) = state.keys.get(&(owner.clone(), key.clone())) {
                if seen.hash != hash {
                    return Err(Status::failed_precondition(
                        "the idempotency key was already used for a different request",
                    ));
                }
                return match state.jobs.get(&seen.job_id) {
                    Some(job) => Ok((job.reply(&seen.job_id), true)),
                    None => Err(Status::failed_precondition(
                        "the job of this idempotency key finished and is no longer kept; \
                         use a new key to start it again",
                    )),
                };
            }
            let active = |job: &&Job| !job.is_terminal();
            if state.jobs.values().filter(active).count() >= MAX_ACTIVE_TOTAL {
                return Err(Status::resource_exhausted(
                    "the host is running as many plugin jobs as it allows; try again later",
                ));
            }
            if state
                .jobs
                .values()
                .filter(active)
                .filter(|job| job.owner == *owner)
                .count()
                >= MAX_ACTIVE_PER_OWNER
            {
                return Err(Status::resource_exhausted(
                    "too many jobs are running for this plugin; wait for one to finish",
                ));
            }
            // The oldest key goes when the owner has too many.
            let own: Vec<_> = state
                .keys
                .iter()
                .filter(|((o, _), _)| o == owner)
                .map(|(id, seen)| (seen.at, id.clone()))
                .collect();
            if own.len() >= MAX_KEYS_PER_OWNER
                && let Some((_, oldest)) = own.into_iter().min_by_key(|(at, _)| *at)
            {
                state.keys.remove(&oldest);
            }
            let (changed, _) = watch::channel(0);
            let now = (self.clock)();
            let mut job = Job {
                owner: owner.clone(),
                permission,
                state: JobState::Queued,
                code: String::new(),
                message: awaiting
                    .map(|text| clip(text, MAX_MESSAGE_BYTES))
                    .unwrap_or_default(),
                progress: 0,
                events: Vec::new(),
                cancel,
                changed,
                finished_at: None,
            };
            job.record();
            let reply = job.reply(&id);
            state.jobs.insert(id.clone(), job);
            state.keys.insert(
                (owner.clone(), key),
                Key {
                    hash,
                    job_id: id.clone(),
                    at: now,
                },
            );
            (
                reply,
                JobContext {
                    id: id.clone(),
                    jobs: self.clone(),
                    cancel: cancel_rx,
                    fence,
                },
            )
        };
        tokio::spawn(
            self.clone()
                .run(id, owner.epoch, profile, work, context, awaiting.is_some()),
        );
        Ok((reply, false))
    }

    async fn run(
        self,
        id: String,
        epoch: u64,
        mut profile: watch::Receiver<Option<ProfileContext>>,
        work: Work,
        context: JobContext,
        awaits_start: bool,
    ) {
        // A profile that already changed while the job was being set up ends it before it starts.
        if profile.borrow().as_ref().map(ProfileContext::epoch) != Some(epoch) {
            self.cancel_job(&id);
        }
        // A job that waits for the user is started by its own work, once the user has decided.
        if !awaits_start && !self.begin(&id) {
            return;
        }
        // The work runs in its own task so that a panic ends the job instead of leaving it
        // running for good.
        let mut handle = tokio::spawn(work(context));
        let mut watching = true;
        let outcome = loop {
            tokio::select! {
                joined = &mut handle => {
                    break joined.unwrap_or_else(|_| {
                        Outcome::failed(CODE_INTERNAL, "the job stopped unexpectedly")
                    });
                }
                changed = profile.changed(), if watching => {
                    if changed.is_err() {
                        watching = false;
                    } else if profile.borrow().as_ref().map(ProfileContext::epoch) != Some(epoch) {
                        // The profile is over: the work is told to stop and its outcome is
                        // recorded for nobody, since no one in the new profile can see it.
                        self.cancel_job(&id);
                        watching = false;
                    }
                }
            }
        };
        self.finish(&id, outcome);
    }

    /// Queued → running, unless the job was cancelled before it started (then it ends here).
    fn begin(&self, id: &str) -> bool {
        let mut state = self.lock();
        let Some(job) = state.jobs.get_mut(id) else {
            return false;
        };
        if *job.cancel.borrow() {
            drop(state);
            self.finish(id, Outcome::cancelled("cancelled before it started"));
            return false;
        }
        if job.state == JobState::Queued {
            job.state = JobState::Running;
            job.message.clear();
            job.record();
        }
        true
    }

    fn progress(&self, id: &str, percent: u32) {
        let mut state = self.lock();
        if let Some(job) = state.jobs.get_mut(id)
            && !job.is_terminal()
            && percent > job.progress
        {
            job.progress = percent;
            job.record();
        }
    }

    fn finish(&self, id: &str, outcome: Outcome) {
        let mut state = self.lock();
        let Some(job) = state.jobs.get_mut(id) else {
            return;
        };
        if job.is_terminal() {
            return;
        }
        let (state_now, code, message) = match outcome {
            Outcome::Succeeded { code, message } => (JobState::Succeeded, code, message),
            Outcome::Failed { code, message } => (JobState::Failed, code, message),
            Outcome::Cancelled { message } => (JobState::Cancelled, String::new(), message),
        };
        job.state = state_now;
        job.code = clip(&code, MAX_CODE_BYTES);
        job.message = clip(&message, MAX_MESSAGE_BYTES);
        if state_now == JobState::Succeeded {
            job.progress = 100;
        }
        job.finished_at = Some((self.clock)());
        job.record();
    }

    /// Asks the work to stop (the job ends when the work does).
    fn cancel_job(&self, id: &str) {
        if let Some(job) = self.lock().jobs.get(id) {
            job.cancel.send_replace(true);
        }
    }

    // ---- What a plugin's calls use. Every one takes the caller's own owner. ----

    fn with<T>(
        &self,
        owner: &JobOwner,
        id: &str,
        read: impl FnOnce(&Job) -> T,
    ) -> Result<T, Status> {
        let mut state = self.lock();
        self.prune(&mut state, owner.epoch);
        state
            .jobs
            .get(id)
            .filter(|job| job.owner == *owner)
            .map(read)
            .ok_or_else(not_found)
    }

    /// The permission of the operation that created the job.
    pub fn permission(&self, owner: &JobOwner, id: &str) -> Result<&'static str, Status> {
        self.with(owner, id, |job| job.permission)
    }

    pub fn get(&self, owner: &JobOwner, id: &str) -> Result<JobReply, Status> {
        self.with(owner, id, |job| job.reply(id))
    }

    /// The events after `after_sequence`, whether the job is over, and a receiver that signals
    /// each new event. A position beyond the newest event is `OUT_OF_RANGE`.
    pub fn events_after(
        &self,
        owner: &JobOwner,
        id: &str,
        after_sequence: u64,
    ) -> Result<(Vec<JobEvent>, bool, watch::Receiver<u64>), Status> {
        self.with(owner, id, |job| {
            if after_sequence > job.sequence() {
                return Err(Status::out_of_range("watch position is in the future"));
            }
            let events = job.events[after_sequence as usize..]
                .iter()
                .map(|event| JobEvent {
                    job_id: id.to_owned(),
                    ..event.clone()
                })
                .collect();
            Ok((events, job.is_terminal(), job.changed.subscribe()))
        })?
    }

    /// Waits up to `limit` for the job to end and answers with how it stands: its outcome if it
    /// ended, its state then if it did not.
    pub async fn wait_over(
        &self,
        owner: &JobOwner,
        id: &str,
        limit: Duration,
    ) -> Result<JobReply, Status> {
        let mut changed = self.with(owner, id, |job| job.changed.subscribe())?;
        let wait = async {
            loop {
                let reply = self.get(owner, id)?;
                if is_over(reply.state()) {
                    return Ok(reply);
                }
                if changed.changed().await.is_err() {
                    return self.get(owner, id);
                }
            }
        };
        match tokio::time::timeout(limit, wait).await {
            Ok(reply) => reply,
            Err(_) => self.get(owner, id),
        }
    }

    /// Asks the job to stop and waits a short while for it to. Cancelling a finished job answers
    /// with how it finished, unchanged. A job whose work ignores the request keeps running and
    /// the reply says so (`Running`); the work decides, because stopping it halfway could leave
    /// an effect half done.
    pub async fn cancel(&self, owner: &JobOwner, id: &str) -> Result<JobReply, Status> {
        let mut changed = self.with(owner, id, |job| {
            if !job.is_terminal() {
                job.cancel.send_replace(true);
            }
            job.changed.subscribe()
        })?;
        let wait = async {
            loop {
                match self.get(owner, id) {
                    Ok(reply) if is_over(reply.state()) => {
                        return Ok(reply);
                    }
                    Ok(_) => {}
                    Err(status) => return Err(status),
                }
                if changed.changed().await.is_err() {
                    return self.get(owner, id);
                }
            }
        };
        match tokio::time::timeout(self.cancel_grace, wait).await {
            Ok(reply) => reply,
            Err(_) => self.get(owner, id),
        }
    }

    /// Jobs not yet over, across every owner.
    #[cfg(test)]
    pub(crate) fn active(&self) -> usize {
        self.lock()
            .jobs
            .values()
            .filter(|job| !job.is_terminal())
            .count()
    }
}

fn is_over(state: JobState) -> bool {
    matches!(
        state,
        JobState::Succeeded | JobState::Failed | JobState::Cancelled
    )
}

#[cfg(test)]
mod tests;
