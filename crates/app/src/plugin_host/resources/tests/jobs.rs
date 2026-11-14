//! `JobsGet`, `JobsWatch` and `JobsCancel` over the real local socket (task 7.1).
//!
//! No operation returns a job yet (installs and backups come with 7.2 onward), so the tests
//! start jobs from the host side the way those operations will: `ResourceApi::start_job`.

use std::sync::atomic::AtomicUsize;

use tokio::sync::Notify;
use tokio_stream::StreamExt;

use super::*;
use crate::plugin_host::jobs::{JobContext, JobOwner, Outcome, work};

/// The permission of the operation that creates the test jobs.
const WRITE: &str = cap::STORAGE_WRITE;
const GRANTS: &[&str] = &[cap::STORAGE_WRITE, cap::STORAGE_READ];

impl Fixture {
    pub(super) fn service(&self) -> ResourceApi {
        ResourceApi::new(
            self.api.clone(),
            self.feed.clone(),
            self.storage.clone(),
            self.files.clone(),
            self.network.clone(),
            self.notifications.clone(),
            self.jobs.clone(),
            self.intents.clone(),
        )
    }

    fn job_caller(&self) -> ResourceCaller {
        self.api
            .authorize_resource(Some(&self.context()), WRITE)
            .unwrap()
    }

    fn owner(&self) -> JobOwner {
        JobOwner::of(&self.job_caller()).unwrap()
    }

    /// What the operation that returns a job does: starts it for the authenticated caller.
    fn start_job(&self, key: &str, hash: u8, run: crate::plugin_host::jobs::Work) -> JobReply {
        self.service()
            .start_job(
                &self.job_caller(),
                self.context(),
                WRITE,
                key,
                [hash; 32],
                None,
                run,
            )
            .unwrap()
            .0
    }

    pub(super) async fn job(&self, id: &str) -> Result<JobReply, Status> {
        self.client()
            .jobs_get(JobGetRequest {
                context: Some(self.context()),
                job_id: id.into(),
            })
            .await
            .map(Response::into_inner)
    }

    pub(super) async fn cancel_job(&self, key: &str, id: &str) -> Result<JobReply, Status> {
        self.client()
            .jobs_cancel(JobCancelRequest {
                mutation: Some(self.mutation(key, 0)),
                job_id: id.into(),
            })
            .await
            .map(Response::into_inner)
    }

    async fn watch_job(
        &self,
        id: &str,
        after_sequence: u64,
    ) -> Result<tonic::Streaming<JobEvent>, Status> {
        self.client()
            .jobs_watch(JobWatchRequest {
                context: Some(self.context()),
                job_id: id.into(),
                after_sequence,
            })
            .await
            .map(Response::into_inner)
    }

    /// Waits for the job to end, reading the host's own record: it needs no session.
    async fn job_over(&self, owner: &JobOwner, id: &str) -> JobReply {
        for _ in 0..400 {
            let reply = self.jobs.get(owner, id).unwrap();
            if matches!(
                reply.state(),
                JobState::Succeeded | JobState::Failed | JobState::Cancelled
            ) {
                return reply;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("the job did not finish");
    }
}

/// Work that waits for `go`, then commits one effect if its fence still allows it.
fn commits_after(go: Arc<Notify>, effects: Arc<AtomicUsize>) -> crate::plugin_host::jobs::Work {
    work(move |ctx: JobContext| async move {
        go.notified().await;
        ctx.progress(50);
        match ctx.check() {
            Ok(()) => {
                effects.fetch_add(1, Ordering::SeqCst);
                Outcome::succeeded("committed", "")
            }
            Err(status) => Outcome::stopped(&status),
        }
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_reads_cancels_and_watches_its_own_job() {
    let fixture = Fixture::new(GRANTS).await;
    let go = Arc::new(Notify::new());
    let effects = Arc::new(AtomicUsize::new(0));
    let started = fixture.start_job("one", 1, commits_after(go.clone(), effects.clone()));

    let reply = fixture.job(&started.job_id).await.unwrap();
    assert_eq!(reply.job_id, started.job_id);
    assert!(matches!(
        reply.state(),
        JobState::Queued | JobState::Running
    ));

    let mut events = fixture.watch_job(&started.job_id, 0).await.unwrap();
    go.notify_one();
    let mut seen = Vec::new();
    while let Some(event) = events.next().await {
        seen.push(event.unwrap());
    }
    // The stream replays from the start, in order, and ends after the outcome.
    let sequences: Vec<_> = seen.iter().map(|e| e.sequence).collect();
    assert_eq!(sequences, (1..=seen.len() as u64).collect::<Vec<_>>());
    let last = seen.last().unwrap();
    assert_eq!(
        (
            last.state(),
            last.result_code.as_str(),
            last.progress_percent
        ),
        (JobState::Succeeded, "committed", 100)
    );
    assert!(seen.iter().all(|e| e.job_id == started.job_id));
    assert_eq!(effects.load(Ordering::SeqCst), 1);

    // Resuming after an event gives what followed; a position in the future is refused.
    let mut rest = fixture
        .watch_job(&started.job_id, last.sequence - 1)
        .await
        .unwrap();
    assert_eq!(rest.next().await.unwrap().unwrap().sequence, last.sequence);
    assert!(rest.next().await.is_none());
    let future = fixture
        .watch_job(&started.job_id, last.sequence + 1)
        .await
        .unwrap_err();
    assert_eq!(future.code(), Code::OutOfRange);

    // Cancelling a finished job says how it finished.
    let cancelled = fixture.cancel_job("c", &started.job_id).await.unwrap();
    assert_eq!(cancelled.state(), JobState::Succeeded);
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_running_job_stops_it_before_it_commits() {
    let fixture = Fixture::new(GRANTS).await;
    let effects = Arc::new(AtomicUsize::new(0));
    let id = fixture
        .start_job(
            "one",
            1,
            work({
                let effects = effects.clone();
                move |mut ctx: JobContext| async move {
                    ctx.cancelled().await;
                    match ctx.check() {
                        Ok(()) => {
                            effects.fetch_add(1, Ordering::SeqCst);
                            Outcome::succeeded("committed", "")
                        }
                        Err(status) => Outcome::stopped(&status),
                    }
                }
            }),
        )
        .job_id;
    let reply = fixture.cancel_job("stop", &id).await.unwrap();
    assert_eq!(reply.state(), JobState::Cancelled);
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    // Asking again, even with a new key, is the same answer.
    let again = fixture.cancel_job("stop-again", &id).await.unwrap();
    assert_eq!(
        (again.state(), again.revision),
        (JobState::Cancelled, reply.revision)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_requests_are_refused_without_naming_anything() {
    let fixture = Fixture::new(GRANTS).await;
    let id = fixture
        .start_job("one", 1, work(|_| async { Outcome::succeeded("ok", "") }))
        .job_id;

    let unknown = fixture.job("job-0000").await.unwrap_err();
    assert_eq!(unknown.code(), Code::NotFound);
    assert_eq!(
        fixture.job("").await.unwrap_err().code(),
        Code::InvalidArgument
    );
    assert_eq!(
        fixture.job(&"x".repeat(129)).await.unwrap_err().code(),
        Code::InvalidArgument
    );
    // The cancel call needs a key like every mutation, and its context.
    assert_eq!(
        fixture.cancel_job("", &id).await.unwrap_err().code(),
        Code::InvalidArgument
    );
    let no_context = fixture
        .client()
        .jobs_get(JobGetRequest {
            context: None,
            job_id: id.clone(),
        })
        .await
        .unwrap_err();
    assert_eq!(no_context.code(), Code::Unauthenticated);
    // A forged context is not accepted either.
    let mut forged = fixture.context();
    forged.session_credential = vec![0; forged.session_credential.len()];
    let denied = fixture
        .client()
        .jobs_get(JobGetRequest {
            context: Some(forged),
            job_id: id,
        })
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
}

#[tokio::test(flavor = "multi_thread")]
async fn losing_the_creating_permission_denies_every_call_and_blocks_the_commit() {
    let fixture = Fixture::new(GRANTS).await;
    let go = Arc::new(Notify::new());
    let effects = Arc::new(AtomicUsize::new(0));
    let id = fixture
        .start_job("one", 1, commits_after(go.clone(), effects.clone()))
        .job_id;
    let owner = fixture.owner();
    let mut events = fixture.watch_job(&id, 0).await.unwrap();
    assert_eq!(events.next().await.unwrap().unwrap().sequence, 1);

    // The plugin loses the permission of the operation that made the job.
    fixture
        .api
        .revoke_session_permission("fixture", WRITE)
        .await;
    for denied in [
        fixture.job(&id).await.unwrap_err(),
        fixture.cancel_job("c", &id).await.unwrap_err(),
        fixture.watch_job(&id, 0).await.unwrap_err(),
    ] {
        assert_eq!(denied.code(), Code::PermissionDenied);
    }
    // A watch that was open ends with the denial instead of going on delivering.
    go.notify_one();
    let mut ended = None;
    while let Some(item) = events.next().await {
        if let Err(status) = item {
            ended = Some(status);
            break;
        }
    }
    assert_eq!(
        ended.expect("the watch must end").code(),
        Code::PermissionDenied
    );

    // The work reached its commit and was stopped: no effect, and the host's own record says why.
    let over = fixture.job_over(&owner, &id).await;
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    assert_eq!(
        (over.state(), over.result_code.as_str()),
        (JobState::Failed, "permission_denied")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_changed_grant_set_stops_the_commit_even_if_the_permission_is_kept() {
    let fixture = Fixture::new(GRANTS).await;
    let go = Arc::new(Notify::new());
    let effects = Arc::new(AtomicUsize::new(0));
    let id = fixture
        .start_job("one", 1, commits_after(go.clone(), effects.clone()))
        .job_id;
    let owner = fixture.owner();

    // Another permission goes: the creating one is still held, so the plugin can still ask...
    fixture
        .api
        .revoke_session_permission("fixture", cap::STORAGE_READ)
        .await;
    assert!(fixture.job(&id).await.is_ok());
    // ...but the effect, which was authorized under the old grants, is not committed.
    go.notify_one();
    let over = fixture.job_over(&owner, &id).await;
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    assert_eq!(
        (over.state(), over.result_code.as_str()),
        (JobState::Failed, "failed_precondition")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restarted_plugin_finds_its_jobs_but_not_the_authority_to_commit() {
    let mut fixture = Fixture::new(GRANTS).await;
    let owner = fixture.owner();
    let go = Arc::new(Notify::new());
    let effects = Arc::new(AtomicUsize::new(0));
    let finished = fixture.start_job("done", 1, quick()).job_id;
    fixture.job_over(&owner, &finished).await;
    let waiting = fixture
        .start_job("waiting", 2, commits_after(go.clone(), effects.clone()))
        .job_id;

    // The same plugin in a new session owns both jobs...
    fixture.reconnect().await;
    let reply = fixture.job(&finished).await.unwrap();
    assert_eq!(reply.state(), JobState::Succeeded);
    assert_eq!(fixture.job(&waiting).await.unwrap().job_id, waiting);
    // ...and a retry of a start, in the new session, is the same job.
    let (retry, replayed) = fixture
        .service()
        .start_job(
            &fixture.job_caller(),
            fixture.context(),
            WRITE,
            "done",
            [1; 32],
            None,
            quick(),
        )
        .unwrap();
    assert!(replayed);
    assert_eq!(retry.job_id, finished);

    // The waiting job was authorized by the session that is gone: it commits nothing.
    go.notify_one();
    let over = fixture.job_over(&owner, &waiting).await;
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    assert_eq!(
        (over.state(), over.result_code.as_str()),
        (JobState::Failed, "session_ended")
    );
    assert_eq!(
        fixture.job(&waiting).await.unwrap().result_code,
        "session_ended"
    );

    // An owner of another plugin gets nothing, not even a different refusal.
    let stranger = JobOwner::for_tests(9, fixture.job_caller().profile_epoch);
    assert_eq!(
        fixture.jobs.get(&stranger, &waiting).unwrap_err().code(),
        Code::NotFound
    );
}

fn quick() -> crate::plugin_host::jobs::Work {
    work(|_| async { Outcome::succeeded("ok", "") })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_switch_hides_the_old_jobs_and_stops_their_commit() {
    let mut fixture = Fixture::new(GRANTS).await;
    let go = Arc::new(Notify::new());
    let effects = Arc::new(AtomicUsize::new(0));
    let id = fixture
        .start_job("one", 1, commits_after(go.clone(), effects.clone()))
        .job_id;
    let old_owner = fixture.owner();

    // Another account signs in. The plugin restarts into its profile, which has no such job.
    fixture.api.profiles().update(Some("b"), true);
    fixture.api.fence_third_party_sessions().await;
    fixture.reconnect().await;
    assert_eq!(fixture.job(&id).await.unwrap_err().code(), Code::NotFound);
    assert_eq!(
        fixture.cancel_job("c", &id).await.unwrap_err().code(),
        Code::NotFound
    );
    assert_eq!(
        fixture.watch_job(&id, 0).await.unwrap_err().code(),
        Code::NotFound
    );

    // The old job was ended and its effect never committed, even if the work wakes up now.
    go.notify_one();
    for _ in 0..400 {
        if fixture.jobs.active() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    // The same plugin and key in the new profile start a job of their own.
    let (fresh, replayed) = fixture
        .service()
        .start_job(
            &fixture.job_caller(),
            fixture.context(),
            WRITE,
            "one",
            [1; 32],
            None,
            quick(),
        )
        .unwrap();
    assert!(!replayed);
    assert_ne!(fresh.job_id, id);
    assert_ne!(fixture.owner(), old_owner);
}

#[tokio::test]
async fn describe_api_lists_the_jobs_methods_without_a_permission_of_their_own() {
    let served: Vec<_> = crate::plugin_host::resources::SERVED
        .iter()
        .filter(|served| served.method.starts_with("Jobs"))
        .collect();
    assert_eq!(
        served
            .iter()
            .map(|served| served.method)
            .collect::<Vec<_>>(),
        ["JobsGet", "JobsWatch", "JobsCancel"]
    );
    assert!(served.iter().all(|served| served.permission.is_empty()));
}
