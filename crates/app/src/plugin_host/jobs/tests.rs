//! The job registry on its own (task 7.1): ownership, retries, cancellation, limits, retention
//! and profile changes, without a plugin session. The RPCs are tested over the real socket in
//! `resources/tests/jobs.rs`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::*;
use crate::plugin_host::profile::PluginProfiles;

const PERMISSION: &str = "fixture.permission";

struct Env {
    profiles: Arc<PluginProfiles>,
    jobs: Jobs,
    now: Arc<Mutex<Instant>>,
}

impl Env {
    fn new() -> Self {
        let profiles = Arc::new(PluginProfiles::default());
        profiles.update(Some("a"), true);
        let now = Arc::new(Mutex::new(Instant::now()));
        let jobs = Jobs::default()
            .with_cancel_grace(Duration::from_millis(100))
            .with_clock({
                let now = now.clone();
                move || *now.lock().unwrap()
            });
        Self {
            profiles,
            jobs,
            now,
        }
    }

    fn owner(&self, key: u8) -> JobOwner {
        JobOwner::for_tests(key, self.profiles.epoch())
    }

    fn advance(&self, by: Duration) {
        *self.now.lock().unwrap() += by;
    }

    fn submission(&self, key: &str, hash: u8) -> Submission {
        Submission {
            permission: PERMISSION,
            key: key.to_owned(),
            hash: [hash; 32],
            fence: Arc::new(|| Ok(())),
            profile: self.profiles.subscribe(),
            awaiting: None,
        }
    }

    fn start(&self, owner: &JobOwner, key: &str, run: Work) -> JobReply {
        self.jobs
            .submit(owner, self.submission(key, 1), run)
            .unwrap()
            .0
    }

    /// Waits until the work has started.
    async fn running(&self, owner: &JobOwner, id: &str) {
        for _ in 0..400 {
            if self.jobs.get(owner, id).unwrap().state() != JobState::Queued {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("the job did not start");
    }

    /// Waits until the job is over.
    async fn over(&self, owner: &JobOwner, id: &str) -> JobReply {
        for _ in 0..400 {
            let reply = self.jobs.get(owner, id).unwrap();
            if is_over(reply.state()) {
                return reply;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("the job did not finish");
    }
}

/// Work that finishes when told to, keeping the job running until then.
fn held() -> (Work, tokio::sync::oneshot::Sender<()>) {
    let (release, wait) = tokio::sync::oneshot::channel::<()>();
    (
        work(move |_| async move {
            let _ = wait.await;
            Outcome::succeeded("released", "")
        }),
        release,
    )
}

fn quick() -> Work {
    work(|_| async { Outcome::succeeded("ok", "done") })
}

#[tokio::test]
async fn a_job_runs_to_its_outcome_and_reports_ordered_events() {
    let env = Env::new();
    let owner = env.owner(1);
    let reply = env.start(
        &owner,
        "k",
        work(|ctx| async move {
            ctx.progress(30);
            ctx.progress(20); // backwards: ignored
            ctx.progress(30); // not forward: ignored
            ctx.progress(60);
            Outcome::succeeded("installed", "3 files")
        }),
    );
    assert!(reply.job_id.starts_with("job-"));
    assert_eq!(reply.state(), JobState::Queued);

    let done = env.over(&owner, &reply.job_id).await;
    assert_eq!(
        (
            done.state(),
            done.result_code.as_str(),
            done.message.as_str()
        ),
        (JobState::Succeeded, "installed", "3 files")
    );
    let (events, over, _) = env.jobs.events_after(&owner, &reply.job_id, 0).unwrap();
    assert!(over);
    let seen: Vec<_> = events
        .iter()
        .map(|e| (e.sequence, e.state(), e.progress_percent))
        .collect();
    assert_eq!(
        seen,
        [
            (1, JobState::Queued, 0),
            (2, JobState::Running, 0),
            (3, JobState::Running, 30),
            (4, JobState::Running, 60),
            (5, JobState::Succeeded, 100),
        ]
    );
    assert!(events.iter().all(|e| e.job_id == reply.job_id));
    assert_eq!(done.revision, 5, "the reply's revision is its newest event");

    // Resuming after an event gives only what followed; the end of the list is not an error.
    let (rest, _, _) = env.jobs.events_after(&owner, &reply.job_id, 3).unwrap();
    assert_eq!(rest.iter().map(|e| e.sequence).collect::<Vec<_>>(), [4, 5]);
    assert!(
        env.jobs
            .events_after(&owner, &reply.job_id, 5)
            .unwrap()
            .0
            .is_empty()
    );
    assert_eq!(
        env.jobs
            .events_after(&owner, &reply.job_id, 6)
            .unwrap_err()
            .code(),
        tonic::Code::OutOfRange
    );
}

#[tokio::test]
async fn a_retry_with_the_same_key_starts_no_new_work() {
    let env = Env::new();
    let owner = env.owner(1);
    let runs = Arc::new(AtomicUsize::new(0));
    let counted = |runs: &Arc<AtomicUsize>| {
        let runs = runs.clone();
        work(move |_| async move {
            runs.fetch_add(1, Ordering::SeqCst);
            Outcome::succeeded("ok", "")
        })
    };

    let first = env
        .jobs
        .submit(&owner, env.submission("same", 7), counted(&runs))
        .unwrap();
    let again = env
        .jobs
        .submit(&owner, env.submission("same", 7), counted(&runs))
        .unwrap();
    assert!(!first.1 && again.1, "the retry is a replay");
    assert_eq!(first.0.job_id, again.0.job_id);
    env.over(&owner, &first.0.job_id).await;
    // After it finished too: still the same job, still one run.
    let late = env
        .jobs
        .submit(&owner, env.submission("same", 7), counted(&runs))
        .unwrap();
    assert!(late.1);
    assert_eq!(late.0.state(), JobState::Succeeded);
    assert_eq!(runs.load(Ordering::SeqCst), 1);

    // The key for a different request is refused; another key is another job.
    let other = env
        .jobs
        .submit(&owner, env.submission("same", 8), counted(&runs));
    assert_eq!(other.unwrap_err().code(), tonic::Code::FailedPrecondition);
    let fresh = env
        .jobs
        .submit(&owner, env.submission("other", 7), counted(&runs))
        .unwrap();
    assert_ne!(fresh.0.job_id, first.0.job_id);
    env.over(&owner, &fresh.0.job_id).await;
    assert_eq!(runs.load(Ordering::SeqCst), 2);

    // Another owner's key space is its own.
    let stranger = env.owner(2);
    let theirs = env
        .jobs
        .submit(&stranger, env.submission("same", 9), counted(&runs))
        .unwrap();
    assert!(!theirs.1);
    assert_ne!(theirs.0.job_id, first.0.job_id);
}

#[tokio::test]
async fn nobody_else_sees_or_touches_a_job() {
    let env = Env::new();
    let owner = env.owner(1);
    let stranger = env.owner(2);
    let (run, release) = held();
    let id = env.start(&owner, "k", run).job_id;
    env.running(&owner, &id).await;

    // Another owner, or an id nobody has: the same refusal, word for word.
    let unknown = env.jobs.get(&owner, "job-nothing").unwrap_err();
    for status in [
        env.jobs.get(&stranger, &id).unwrap_err(),
        env.jobs.permission(&stranger, &id).unwrap_err(),
        env.jobs.events_after(&stranger, &id, 0).unwrap_err(),
        env.jobs.cancel(&stranger, &id).await.unwrap_err(),
    ] {
        assert_eq!(status.code(), tonic::Code::NotFound);
        assert_eq!(status.message(), unknown.message());
    }
    // The failed attempts changed nothing: the job was not cancelled.
    assert_eq!(
        env.jobs.get(&owner, &id).unwrap().state(),
        JobState::Running
    );
    release.send(()).unwrap();
    assert_eq!(env.over(&owner, &id).await.state(), JobState::Succeeded);
}

#[tokio::test]
async fn cancelling_stops_cooperating_work_and_is_repeatable() {
    let env = Env::new();
    let owner = env.owner(1);
    let id = env
        .start(
            &owner,
            "k",
            work(|mut ctx| async move {
                ctx.cancelled().await;
                assert!(ctx.check().is_err(), "a cancelled job commits nothing");
                Outcome::cancelled("stopped by the plugin")
            }),
        )
        .job_id;
    env.running(&owner, &id).await;

    let first = env.jobs.cancel(&owner, &id).await.unwrap();
    assert_eq!(first.state(), JobState::Cancelled);
    assert_eq!(first.message, "stopped by the plugin");
    let again = env.jobs.cancel(&owner, &id).await.unwrap();
    assert_eq!(
        (again.state(), again.revision),
        (JobState::Cancelled, first.revision)
    );
}

#[tokio::test]
async fn cancelling_a_finished_job_reports_how_it_finished() {
    let env = Env::new();
    let owner = env.owner(1);
    let id = env.start(&owner, "k", quick()).job_id;
    env.over(&owner, &id).await;
    let reply = env.jobs.cancel(&owner, &id).await.unwrap();
    assert_eq!(reply.state(), JobState::Succeeded);
    assert_eq!(reply.result_code, "ok");
}

#[tokio::test]
async fn a_job_cancelled_before_it_starts_never_runs() {
    let env = Env::new();
    let owner = env.owner(1);
    let runs = Arc::new(AtomicUsize::new(0));
    let id = env
        .start(&owner, "k", {
            let runs = runs.clone();
            work(move |_| async move {
                runs.fetch_add(1, Ordering::SeqCst);
                Outcome::succeeded("ok", "")
            })
        })
        .job_id;
    // The runtime has not run the job yet: cancel first.
    let reply = env.jobs.cancel(&owner, &id).await.unwrap();
    assert_eq!(reply.state(), JobState::Cancelled);
    assert_eq!(runs.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn work_that_ignores_cancellation_is_reported_as_still_running() {
    let env = Env::new();
    let owner = env.owner(1);
    let (run, release) = held();
    let id = env.start(&owner, "k", run).job_id;
    env.running(&owner, &id).await;
    // The reply is honest: the work did not stop within the grace period.
    let reply = env.jobs.cancel(&owner, &id).await.unwrap();
    assert_eq!(reply.state(), JobState::Running);
    release.send(()).unwrap();
    // It finished its effect, so that is what the job says.
    assert_eq!(env.over(&owner, &id).await.state(), JobState::Succeeded);
}

#[tokio::test]
async fn a_panic_ends_the_job_as_failed() {
    let env = Env::new();
    let owner = env.owner(1);
    let id = env
        .start(&owner, "k", work(|_| async { panic!("the work broke") }))
        .job_id;
    let reply = env.over(&owner, &id).await;
    assert_eq!(
        (reply.state(), reply.result_code.as_str()),
        (JobState::Failed, CODE_INTERNAL)
    );
    assert!(!reply.message.contains("broke"), "no internal detail leaks");
    assert_eq!(env.jobs.active(), 0);
}

#[tokio::test]
async fn results_are_clipped_and_cleaned() {
    let env = Env::new();
    let owner = env.owner(1);
    let id = env
        .start(
            &owner,
            "k",
            work(|_| async {
                Outcome::failed(
                    &"c".repeat(500),
                    &format!("line\u{7}one\n{}", "é".repeat(600)),
                )
            }),
        )
        .job_id;
    let reply = env.over(&owner, &id).await;
    assert_eq!(reply.result_code.len(), MAX_CODE_BYTES);
    assert!(reply.message.len() <= MAX_MESSAGE_BYTES);
    assert!(reply.message.starts_with("line one "));
    assert!(!reply.message.chars().any(char::is_control));
}

#[tokio::test]
async fn running_jobs_are_bounded_per_owner_and_overall() {
    let env = Env::new();
    let owner = env.owner(1);
    let mut releases = Vec::new();
    for n in 0..MAX_ACTIVE_PER_OWNER {
        let (run, release) = held();
        env.start(&owner, &format!("k{n}"), run);
        releases.push(release);
    }
    let (run, _release) = held();
    let refused = env
        .jobs
        .submit(&owner, env.submission("one-too-many", 1), run)
        .unwrap_err();
    assert_eq!(refused.code(), tonic::Code::ResourceExhausted);
    // A retry of a job already running is still answered at the limit.
    let replay = env
        .jobs
        .submit(&owner, env.submission("k0", 1), quick())
        .unwrap();
    assert!(replay.1);
    // Another plugin is not held back by this one's jobs.
    let (run, release) = held();
    env.start(&env.owner(2), "k", run);
    releases.push(release);

    // The host as a whole is bounded too.
    let mut owner_key = 3;
    while env.jobs.active() < MAX_ACTIVE_TOTAL {
        for n in 0..MAX_ACTIVE_PER_OWNER.min(MAX_ACTIVE_TOTAL - env.jobs.active()) {
            let (run, release) = held();
            env.start(&env.owner(owner_key), &format!("k{n}"), run);
            releases.push(release);
        }
        owner_key += 1;
    }
    let (run, _release) = held();
    let refused = env
        .jobs
        .submit(&env.owner(owner_key), env.submission("late", 1), run)
        .unwrap_err();
    assert_eq!(refused.code(), tonic::Code::ResourceExhausted);

    // Finishing one makes room again.
    releases.remove(0).send(()).unwrap();
    for _ in 0..200 {
        if env.jobs.active() < MAX_ACTIVE_TOTAL {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    env.jobs
        .submit(&owner, env.submission("room", 1), quick())
        .unwrap();
}

#[tokio::test]
async fn finished_jobs_are_forgotten_but_their_key_never_runs_the_work_twice() {
    let env = Env::new();
    let owner = env.owner(1);
    let runs = Arc::new(AtomicUsize::new(0));
    let counted = |runs: &Arc<AtomicUsize>| {
        let runs = runs.clone();
        work(move |_| async move {
            runs.fetch_add(1, Ordering::SeqCst);
            Outcome::succeeded("ok", "")
        })
    };
    let id = env
        .jobs
        .submit(&owner, env.submission("k", 1), counted(&runs))
        .unwrap()
        .0
        .job_id;
    env.over(&owner, &id).await;

    // An hour later the job is gone, and its id is the same refusal as any unknown id.
    env.advance(FINISHED_RETENTION + Duration::from_secs(1));
    assert_eq!(
        env.jobs.get(&owner, &id).unwrap_err().code(),
        tonic::Code::NotFound
    );
    // The retry is refused, not run again.
    let retry = env
        .jobs
        .submit(&owner, env.submission("k", 1), counted(&runs))
        .unwrap_err();
    assert_eq!(retry.code(), tonic::Code::FailedPrecondition);
    assert_eq!(runs.load(Ordering::SeqCst), 1);

    // After the key's own 24 hours it is free to use again.
    env.advance(KEY_RETENTION);
    let again = env
        .jobs
        .submit(&owner, env.submission("k", 1), counted(&runs))
        .unwrap();
    assert!(!again.1);
    env.over(&owner, &again.0.job_id).await;
    assert_eq!(runs.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn an_owner_keeps_only_so_many_finished_jobs() {
    let env = Env::new();
    let owner = env.owner(1);
    let mut ids = Vec::new();
    for n in 0..MAX_FINISHED_PER_OWNER + 3 {
        let id = env.start(&owner, &format!("k{n}"), quick()).job_id;
        env.over(&owner, &id).await;
        env.advance(Duration::from_secs(1));
        ids.push(id);
    }
    assert_eq!(
        env.jobs.get(&owner, &ids[0]).unwrap_err().code(),
        tonic::Code::NotFound
    );
    assert_eq!(
        env.jobs.get(&owner, &ids[2]).unwrap_err().code(),
        tonic::Code::NotFound
    );
    assert!(env.jobs.get(&owner, &ids[3]).is_ok());
    assert!(env.jobs.get(&owner, ids.last().unwrap()).is_ok());
}

#[tokio::test]
async fn a_profile_switch_ends_the_jobs_of_the_old_profile_and_hides_them() {
    let env = Env::new();
    let old = env.owner(1);
    let ran = Arc::new(AtomicUsize::new(0));
    let id = env
        .start(&old, "k", {
            let ran = ran.clone();
            work(move |mut ctx| async move {
                ctx.cancelled().await;
                // A fence that would pass still cannot commit: the job was cancelled.
                if ctx.check().is_ok() {
                    ran.fetch_add(1, Ordering::SeqCst);
                }
                Outcome::cancelled("profile changed")
            })
        })
        .job_id;
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(env.jobs.get(&old, &id).unwrap().state(), JobState::Running);

    env.profiles.update(Some("b"), true);
    let new = env.owner(1); // the same plugin, the new profile epoch
    assert_ne!(old, new);
    // The new profile cannot see it, whatever it asks.
    assert_eq!(
        env.jobs.get(&new, &id).unwrap_err().code(),
        tonic::Code::NotFound
    );
    assert_eq!(
        env.jobs.events_after(&new, &id, 0).unwrap_err().code(),
        tonic::Code::NotFound
    );
    // The old job was told to stop and committed nothing.
    for _ in 0..400 {
        if env.jobs.active() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(env.jobs.active(), 0);
    assert_eq!(ran.load(Ordering::SeqCst), 0);
    // Once the new profile asks anything, the old profile's records are gone for good.
    let _ = env.jobs.get(&new, "job-x");
    assert_eq!(
        env.jobs.get(&old, &id).unwrap_err().code(),
        tonic::Code::NotFound
    );
    // A request still carrying the old epoch cannot start anything.
    let late = env
        .jobs
        .submit(&old, env.submission("late", 1), quick())
        .unwrap_err();
    assert_eq!(late.code(), tonic::Code::FailedPrecondition);
    assert_eq!(env.jobs.active(), 0);
}

#[tokio::test]
async fn a_profile_that_changed_before_the_job_started_never_starts_it() {
    let env = Env::new();
    let old = env.owner(1);
    let submission = env.submission("k", 1);
    // The profile changes after the caller was authorized and before the job is registered.
    env.profiles.update(Some("b"), true);
    let runs = Arc::new(AtomicUsize::new(0));
    env.jobs
        .submit(&old, submission, {
            let runs = runs.clone();
            work(move |_| async move {
                runs.fetch_add(1, Ordering::SeqCst);
                Outcome::succeeded("ok", "")
            })
        })
        .unwrap();
    for _ in 0..400 {
        if env.jobs.active() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(runs.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn stopped_work_reports_why_without_internal_detail() {
    assert_eq!(
        Outcome::stopped(&Status::cancelled("x")),
        Outcome::cancelled("cancelled before anything was committed")
    );
    assert!(matches!(
        Outcome::stopped(&Status::permission_denied("secret detail")),
        Outcome::Failed { code, message } if code == "permission_denied" && !message.contains("secret")
    ));
    assert!(matches!(
        Outcome::stopped(&Status::failed_precondition("secret detail")),
        Outcome::Failed { code, .. } if code == "failed_precondition"
    ));
}

#[tokio::test]
async fn a_job_that_waits_for_the_user_stays_queued_until_its_work_starts_it() {
    let env = Env::new();
    let owner = env.owner(1);
    let (release, wait) = tokio::sync::oneshot::channel::<()>();
    let mut submission = env.submission("k", 1);
    submission.awaiting = Some("awaiting confirmation");
    let (reply, _) = env
        .jobs
        .submit(
            &owner,
            submission,
            work(move |ctx| async move {
                let _ = wait.await;
                ctx.start().unwrap();
                ctx.progress(40);
                Outcome::succeeded("done", "")
            }),
        )
        .unwrap();
    assert_eq!(
        (reply.state(), reply.message.as_str()),
        (JobState::Queued, "awaiting confirmation")
    );
    // It stays queued, with its message, for as long as the work waits.
    tokio::time::sleep(Duration::from_millis(30)).await;
    let waiting = env.jobs.get(&owner, &reply.job_id).unwrap();
    assert_eq!(
        (waiting.state(), waiting.message.as_str()),
        (JobState::Queued, "awaiting confirmation")
    );

    release.send(()).unwrap();
    let done = env.over(&owner, &reply.job_id).await;
    assert_eq!(done.state(), JobState::Succeeded);
    let (events, _, _) = env.jobs.events_after(&owner, &reply.job_id, 0).unwrap();
    let seen: Vec<_> = events
        .iter()
        .map(|e| (e.state(), e.message.as_str(), e.progress_percent))
        .collect();
    assert_eq!(
        seen,
        [
            (JobState::Queued, "awaiting confirmation", 0),
            (JobState::Running, "", 0),
            (JobState::Running, "", 40),
            (JobState::Succeeded, "", 100),
        ]
    );
}

#[tokio::test]
async fn cancelling_a_job_that_waits_for_the_user_withdraws_it() {
    let env = Env::new();
    let owner = env.owner(1);
    let mut submission = env.submission("k", 1);
    submission.awaiting = Some("awaiting confirmation");
    let started = Arc::new(AtomicUsize::new(0));
    let (reply, _) = env
        .jobs
        .submit(
            &owner,
            submission,
            work({
                let started = started.clone();
                move |mut ctx| async move {
                    ctx.cancelled().await;
                    // Cancelled while waiting: it may not start, whatever the user decides next.
                    if ctx.start().is_ok() {
                        started.fetch_add(1, Ordering::SeqCst);
                    }
                    Outcome::cancelled("withdrawn")
                }
            }),
        )
        .unwrap();
    let cancelled = env.jobs.cancel(&owner, &reply.job_id).await.unwrap();
    assert_eq!(cancelled.state(), JobState::Cancelled);
    assert_eq!(started.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_retry_can_be_answered_before_anything_is_spent_on_it() {
    let env = Env::new();
    let owner = env.owner(1);
    assert_eq!(env.jobs.replay(&owner, "k", [1; 32]).unwrap(), None);
    let id = env.start(&owner, "k", quick()).job_id;
    let replayed = env.jobs.replay(&owner, "k", [1; 32]).unwrap().unwrap();
    assert_eq!(replayed.job_id, id);
    assert_eq!(
        env.jobs.replay(&owner, "k", [2; 32]).unwrap_err().code(),
        tonic::Code::FailedPrecondition
    );
    // Another owner's key space is its own.
    assert_eq!(env.jobs.replay(&env.owner(2), "k", [1; 32]).unwrap(), None);
    env.over(&owner, &id).await;
    env.advance(FINISHED_RETENTION + Duration::from_secs(1));
    assert_eq!(
        env.jobs.replay(&owner, "k", [1; 32]).unwrap_err().code(),
        tonic::Code::FailedPrecondition
    );
}
