//! `GamesRequestLaunch` and `GamesRequestInstall` over the real local socket (tasks 7.2 and
//! 10.10): the user's intent, the dialog, remembered confirmations and the scope per game.

use std::collections::VecDeque;
use std::sync::Mutex as StdMutex;

use tokio::sync::Notify;

use super::*;
use crate::plugin_host::intents::{Answer, Binding, ConfirmationRequest, Operation, Prompter};
use crate::plugin_host::jobs::JobOwner;

const GRANTS: &[&str] = &[cap::GAME_LAUNCH, cap::GAMES_INSTALL];
const ENTRY: &str = "fixture:1";
const INSTALLATION: &str = "id-fixture:1";

/// The user's side of the dialog: answers in order (declining by default), optionally held until
/// the test lets it go, remembering what it was shown.
#[derive(Default)]
struct Dialog {
    answers: StdMutex<VecDeque<Answer>>,
    shown: StdMutex<Vec<ConfirmationRequest>>,
    held: Option<Arc<Notify>>,
}

#[tonic::async_trait]
impl Prompter for Dialog {
    async fn confirm(&self, request: ConfirmationRequest) -> Result<Answer, Status> {
        self.shown.lock().unwrap().push(request);
        if let Some(gate) = &self.held {
            gate.notified().await;
        }
        Ok(self.answers.lock().unwrap().pop_front().unwrap_or(Answer {
            approved: false,
            remember: false,
        }))
    }
}

const APPROVE: Answer = Answer {
    approved: true,
    remember: false,
};
const APPROVE_AND_REMEMBER: Answer = Answer {
    approved: true,
    remember: true,
};

impl Fixture {
    /// A fixture with one own entry, a fake engine side and (when `dialog` is given) a dialog.
    async fn game(
        grants: &[&'static str],
        dialog: Option<Dialog>,
    ) -> (Self, Arc<FakeHost>, Option<Arc<Dialog>>) {
        let fixture = Self::new(grants).await;
        fixture.library(&[(ENTRY, Some(3), "Game"), ("steam:1", Some(5), "Other")]);
        let host = fixture.with_host();
        let dialog = dialog.map(Arc::new);
        if let Some(dialog) = &dialog {
            fixture.intents.set_prompter(dialog.clone());
        }
        (fixture, host, dialog)
    }

    /// The revision of the fixture's own entry, as the host holds it.
    async fn revision(&self) -> u64 {
        let own = Scope {
            own_provider: "fixture".into(),
            read_all: false,
            provider_filter: None,
        };
        self.feed.target(&own, INSTALLATION).unwrap().1.revision
    }

    async fn launch_game(&self, key: &str, intent: &str) -> Result<GameLaunchReply, Status> {
        let revision = self.revision().await;
        self.launch_at(key, INSTALLATION, revision, intent).await
    }

    async fn launch_at(
        &self,
        key: &str,
        installation_id: &str,
        expected_revision: u64,
        intent: &str,
    ) -> Result<GameLaunchReply, Status> {
        self.client()
            .games_request_launch(GameLaunchRequest {
                context: Some(self.context()),
                game_id: String::new(),
                installation_id: installation_id.into(),
                expected_revision,
                user_intent_id: intent.into(),
                idempotency_key: key.into(),
            })
            .await
            .map(Response::into_inner)
    }

    async fn install_game(&self, key: &str, intent: &str) -> Result<JobReply, Status> {
        self.client()
            .games_request_install(GameInstallRequest {
                context: Some(self.context()),
                game_id: String::new(),
                installation_id: INSTALLATION.into(),
                user_intent_id: intent.into(),
                idempotency_key: key.into(),
            })
            .await
            .map(Response::into_inner)
    }

    /// What the host binds an intent to for the fixture's own entry.
    fn binding(&self, operation: Operation, target_revision: u64) -> Binding {
        let caller = self
            .api
            .authorize_resource(Some(&self.context()), GRANTS[0])
            .unwrap();
        Binding {
            plugin_id: "fixture.plugin".into(),
            publisher_fingerprint: "ab".repeat(32),
            profile_epoch: caller.profile_epoch,
            game_id: "3".into(),
            installation_id: INSTALLATION.into(),
            operation,
            target_revision,
        }
    }

    fn game_owner(&self) -> JobOwner {
        JobOwner::of(
            &self
                .api
                .authorize_resource(Some(&self.context()), GRANTS[0])
                .unwrap(),
        )
        .unwrap()
    }

    /// Waits until the job is over, reading the host's own record.
    async fn ended(&self, id: &str) -> JobReply {
        self.ended_for(&self.game_owner(), id).await
    }

    async fn ended_for(&self, owner: &JobOwner, id: &str) -> JobReply {
        for _ in 0..400 {
            let reply = self.jobs.get(owner, id).unwrap();
            if !matches!(reply.state(), JobState::Queued | JobState::Running) {
                return reply;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("the job did not end");
    }

    async fn dialog_shown(dialog: &Dialog, count: usize) {
        for _ in 0..400 {
            if dialog.shown.lock().unwrap().len() >= count {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("the dialog was not shown");
    }
}

fn held(answers: &[Answer]) -> (Dialog, Arc<Notify>) {
    let gate = Arc::new(Notify::new());
    (
        Dialog {
            answers: StdMutex::new(answers.iter().copied().collect()),
            held: Some(gate.clone()),
            ..Dialog::default()
        },
        gate,
    )
}

fn answering(answers: &[Answer]) -> Dialog {
    Dialog {
        answers: StdMutex::new(answers.iter().copied().collect()),
        ..Dialog::default()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn without_an_intent_the_job_waits_for_the_user_and_an_approval_starts_the_game() {
    let (dialog, gate) = held(&[APPROVE]);
    let (fixture, host, dialog) = Fixture::game(GRANTS, Some(dialog)).await;
    let dialog = dialog.unwrap();

    let reply = fixture.launch_game("one", "").await.unwrap();
    assert_eq!(
        (reply.state(), reply.message.as_str()),
        (JobState::Queued, "awaiting confirmation")
    );
    Fixture::dialog_shown(&dialog, 1).await;
    // Nothing starts while the user has not answered.
    assert!(host.opened.lock().unwrap().is_empty());
    let shown = dialog.shown.lock().unwrap()[0].clone();
    assert_eq!(
        (
            shown.plugin_id.as_str(),
            shown.game_name.as_str(),
            shown.operation
        ),
        ("fixture.plugin", "Game", Operation::Launch)
    );
    assert_eq!(
        fixture.job(&reply.job_id).await.unwrap().state(),
        JobState::Queued
    );

    gate.notify_one();
    let done = fixture.ended(&reply.job_id).await;
    assert_eq!(
        (done.state(), done.result_code.as_str()),
        (JobState::Succeeded, "opened")
    );
    assert_eq!(*host.opened.lock().unwrap(), [(ENTRY.to_owned(), false)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn declining_starts_nothing() {
    let (fixture, host, _) = Fixture::game(GRANTS, Some(answering(&[]))).await;
    let reply = fixture.launch_game("one", "").await.unwrap();
    let done = fixture.ended(&reply.job_id).await;
    assert_eq!(
        (done.state(), done.result_code.as_str()),
        (JobState::Failed, "declined")
    );
    assert!(host.opened.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_waiting_request_withdraws_it_and_frees_the_game() {
    let (dialog, gate) = held(&[APPROVE]);
    let (fixture, host, dialog) = Fixture::game(GRANTS, Some(dialog)).await;
    let dialog = dialog.unwrap();
    let reply = fixture.launch_game("one", "").await.unwrap();
    Fixture::dialog_shown(&dialog, 1).await;

    // One dialog per game at a time: another request while it waits is refused.
    assert_eq!(
        fixture.launch_game("two", "").await.unwrap_err().code(),
        Code::FailedPrecondition
    );
    let withdrawn = fixture.cancel_job("c", &reply.job_id).await.unwrap();
    assert_eq!(withdrawn.state(), JobState::Cancelled);
    // A late approval changes nothing.
    gate.notify_one();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(host.opened.lock().unwrap().is_empty());
    // The slot is free again.
    let again = fixture.launch_game("three", "").await.unwrap();
    assert_eq!(again.state(), JobState::Queued);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_retry_opens_no_second_dialog() {
    let (dialog, gate) = held(&[APPROVE]);
    let (fixture, host, dialog) = Fixture::game(GRANTS, Some(dialog)).await;
    let dialog = dialog.unwrap();
    let first = fixture.launch_game("same", "").await.unwrap();
    Fixture::dialog_shown(&dialog, 1).await;
    let retry = fixture.launch_game("same", "").await.unwrap();
    assert_eq!(retry.job_id, first.job_id);
    gate.notify_one();
    fixture.ended(&first.job_id).await;
    // Even after it ended, the same request is still the same job.
    assert_eq!(
        fixture.launch_game("same", "").await.unwrap().job_id,
        first.job_id
    );
    assert_eq!(dialog.shown.lock().unwrap().len(), 1);
    assert_eq!(host.opened.lock().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pre_issued_intent_skips_the_dialog_and_is_spent_once() {
    // No dialog at all in this build: the intent is enough.
    let (fixture, host, _) = Fixture::game(GRANTS, None).await;
    let id = fixture
        .intents
        .issue(fixture.binding(Operation::Launch, 0))
        .unwrap();
    let reply = fixture.launch_game("one", &id).await.unwrap();
    let done = fixture.ended(&reply.job_id).await;
    assert_eq!(done.state(), JobState::Succeeded);
    assert_eq!(host.opened.lock().unwrap().len(), 1);

    // The same request again is the same job; the intent is not spent twice.
    assert_eq!(
        fixture.launch_game("one", &id).await.unwrap().job_id,
        reply.job_id
    );
    // The spent intent is worthless for a new request.
    assert_eq!(
        fixture.launch_game("two", &id).await.unwrap_err().code(),
        Code::FailedPrecondition
    );
    assert_eq!(host.opened.lock().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_intent_for_another_operation_target_or_nothing_at_all_is_refused() {
    let (fixture, host, _) = Fixture::game(GRANTS, None).await;
    // Issued for an install: not a launch.
    let install = fixture
        .intents
        .issue(fixture.binding(Operation::Install, 0))
        .unwrap();
    // Issued for a target that is not the current one.
    let stale = fixture
        .intents
        .issue(fixture.binding(Operation::Launch, 99))
        .unwrap();
    let refusals = [
        fixture.launch_game("a", &install).await.unwrap_err(),
        fixture.launch_game("b", &stale).await.unwrap_err(),
        fixture.launch_game("c", "intent-forged").await.unwrap_err(),
    ];
    for refused in &refusals {
        assert_eq!(refused.code(), Code::FailedPrecondition);
        assert_eq!(
            refused.message(),
            refusals[0].message(),
            "nothing says which part was wrong"
        );
    }
    assert!(host.opened.lock().unwrap().is_empty());
    // The refusals spent neither: the install intent still installs.
    let reply = fixture.install_game("d", &install).await.unwrap();
    assert_eq!(
        fixture.ended(&reply.job_id).await.state(),
        JobState::Succeeded
    );
    assert_eq!(*host.opened.lock().unwrap(), [(ENTRY.to_owned(), true)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_build_without_a_dialog_cannot_ask_and_starts_no_job() {
    let (fixture, host, _) = Fixture::game(GRANTS, None).await;
    let status = fixture.launch_game("one", "").await.unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
    assert!(host.opened.lock().unwrap().is_empty());
    assert_eq!(fixture.jobs.active(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_remembered_confirmation_skips_the_dialog_until_the_target_or_operation_changes() {
    let (fixture, host, dialog) = Fixture::game(
        GRANTS,
        Some(answering(&[
            APPROVE_AND_REMEMBER,
            APPROVE,
            APPROVE_AND_REMEMBER,
        ])),
    )
    .await;
    let dialog = dialog.unwrap();

    // The first launch asks, and the user chooses to remember.
    let first = fixture.launch_game("one", "").await.unwrap();
    assert_eq!(
        fixture.ended(&first.job_id).await.state(),
        JobState::Succeeded
    );
    assert_eq!(dialog.shown.lock().unwrap().len(), 1);

    // The next one starts at once: no dialog, no waiting job.
    let second = fixture.launch_game("two", "").await.unwrap();
    assert_ne!(second.message, "awaiting confirmation");
    assert_eq!(
        fixture.ended(&second.job_id).await.state(),
        JobState::Succeeded
    );
    assert_eq!(dialog.shown.lock().unwrap().len(), 1);
    assert_eq!(host.opened.lock().unwrap().len(), 2);

    // The game's detected target changed: it asks again.
    host.launch_target.store(41, Ordering::SeqCst);
    let third = fixture.launch_game("three", "").await.unwrap();
    assert_eq!(third.message, "awaiting confirmation");
    fixture.ended(&third.job_id).await;
    assert_eq!(dialog.shown.lock().unwrap().len(), 2);

    // Launching and installing are different approvals.
    let install = fixture.install_game("four", "").await.unwrap();
    assert_eq!(install.message, "awaiting confirmation");
    fixture.ended(&install.job_id).await;
    assert_eq!(dialog.shown.lock().unwrap().len(), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_remembered_confirmation_survives_a_restart_and_can_be_revoked() {
    let (fixture, _, dialog) =
        Fixture::game(GRANTS, Some(answering(&[APPROVE_AND_REMEMBER, APPROVE]))).await;
    let first = fixture.launch_game("one", "").await.unwrap();
    fixture.ended(&first.job_id).await;

    let caller = fixture
        .api
        .authorize_resource(Some(&fixture.context()), GRANTS[0])
        .unwrap();
    let listed = fixture
        .storage
        .run_for_host(
            caller.namespace.as_ref().unwrap(),
            caller.profile_epoch,
            |namespace| namespace.intent_approvals(),
        )
        .await
        .unwrap();
    let [approval] = listed.try_into().unwrap();
    assert_eq!(
        (
            approval.operation.as_str(),
            approval.game_id.as_str(),
            approval.game_name.as_str()
        ),
        ("launch", "3", "Game")
    );

    // Revoked from the settings: the next request asks again.
    let id = approval.id.clone();
    assert!(
        fixture
            .storage
            .run_for_host(
                caller.namespace.as_ref().unwrap(),
                caller.profile_epoch,
                move |namespace| namespace.revoke_intent_approval(&id),
            )
            .await
            .unwrap()
    );
    let again = fixture.launch_game("two", "").await.unwrap();
    assert_eq!(again.message, "awaiting confirmation");
    fixture.ended(&again.job_id).await;
    assert_eq!(dialog.unwrap().shown.lock().unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn losing_the_permission_while_the_dialog_is_open_prevents_the_start() {
    let (dialog, gate) = held(&[APPROVE]);
    let (fixture, host, dialog) = Fixture::game(GRANTS, Some(dialog)).await;
    let reply = fixture.launch_game("one", "").await.unwrap();
    Fixture::dialog_shown(&dialog.unwrap(), 1).await;

    // The host's own record needs no session, so keep the owner before the permission goes.
    let owner = fixture.game_owner();
    fixture
        .api
        .revoke_session_permission("fixture", cap::GAME_LAUNCH)
        .await;
    gate.notify_one();
    let done = fixture.ended_for(&owner, &reply.job_id).await;
    assert_eq!(done.state(), JobState::Failed);
    assert!(host.opened.lock().unwrap().is_empty());
    // And the plugin can no longer ask for anything.
    assert_eq!(
        fixture.launch_game("two", "").await.unwrap_err().code(),
        Code::PermissionDenied
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_target_that_changed_while_the_dialog_was_open_is_not_started() {
    let (dialog, gate) = held(&[APPROVE]);
    let (fixture, host, dialog) = Fixture::game(GRANTS, Some(dialog)).await;
    let reply = fixture.launch_game("one", "").await.unwrap();
    Fixture::dialog_shown(&dialog.unwrap(), 1).await;

    host.launch_target.store(7, Ordering::SeqCst);
    gate.notify_one();
    let done = fixture.ended(&reply.job_id).await;
    assert_eq!(
        (done.state(), done.result_code.as_str()),
        (JobState::Failed, "target_changed")
    );
    assert!(host.opened.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_install_opens_the_providers_flow_and_reports_what_it_cannot_do() {
    let (fixture, host, _) = Fixture::game(GRANTS, Some(answering(&[APPROVE, APPROVE]))).await;
    let reply = fixture.install_game("one", "").await.unwrap();
    assert_eq!(fixture.ended(&reply.job_id).await.result_code, "opened");
    assert_eq!(*host.opened.lock().unwrap(), [(ENTRY.to_owned(), true)]);

    // A provider with no such action: the job says so, instead of pretending.
    *host.open_fails.lock().unwrap() = Some(Code::Unimplemented);
    let unsupported = fixture.install_game("two", "").await.unwrap();
    let done = fixture.ended(&unsupported.job_id).await;
    assert_eq!(
        (done.state(), done.result_code.as_str()),
        (JobState::Failed, "unsupported")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_permission_of_the_operation_and_a_valid_request_get_this_far() {
    let (dialog, _gate) = held(&[APPROVE]);
    let (fixture, host, dialog) = Fixture::game(&[cap::GAME_LAUNCH], Some(dialog)).await;
    let dialog = dialog.unwrap();
    // The launch grant is not an install grant, and nothing is asked.
    assert_eq!(
        fixture.install_game("a", "").await.unwrap_err().code(),
        Code::PermissionDenied
    );
    // A stale revision, a foreign game, an empty installation and a bad key never ask either.
    let revision = fixture.revision().await;
    assert_eq!(
        fixture
            .launch_at("b", INSTALLATION, revision + 1, "")
            .await
            .unwrap_err()
            .code(),
        Code::FailedPrecondition
    );
    assert_eq!(
        fixture
            .launch_at("c", "id-steam:1", 0, "")
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    assert_eq!(
        fixture.launch_at("d", "", 0, "").await.unwrap_err().code(),
        Code::InvalidArgument
    );
    assert_eq!(
        fixture
            .launch_at("", INSTALLATION, revision, "")
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    assert_eq!(
        fixture
            .launch_at("e", INSTALLATION, revision, &"x".repeat(129))
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    assert!(dialog.shown.lock().unwrap().is_empty());
    assert!(host.opened.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_extension_session_reaches_only_its_target_and_reading_the_library_adds_nothing() {
    let (fixture, _, _) = Fixture::game(&[cap::GAME_LAUNCH, cap::LIBRARY_READ_ALL], None).await;
    let caller = fixture
        .api
        .authorize_resource(Some(&fixture.context()), GRANTS[0])
        .unwrap();
    let service = fixture.service();
    let extension = |installation: &str, game: &str| {
        let mut context = fixture.context();
        context.session_kind = SessionKind::Extension as i32;
        context.target_game = Some(TargetGameContext {
            game_id: game.into(),
            installation_id: installation.into(),
            profile_id: String::new(),
            revision: String::new(),
        });
        context
    };

    // The selected game of another provider is reachable, whole library or not.
    let (key, entry) = service
        .action_target(&caller, &extension("id-steam:1", "5"), "", "id-steam:1")
        .unwrap();
    assert_eq!(
        (key.as_str(), entry.provider_id.as_str()),
        ("steam:1", "steam")
    );
    // Any other game is not, even though the plugin can read it.
    for (context, installation) in [
        (extension("id-steam:1", "5"), INSTALLATION),
        (extension("id-steam:1", "6"), "id-steam:1"),
        (extension(INSTALLATION, "3"), "id-steam:1"),
    ] {
        assert_eq!(
            service
                .action_target(&caller, &context, "", installation)
                .unwrap_err()
                .code(),
            Code::NotFound
        );
    }
    // An extension session with no selected game names nothing.
    let mut bare = fixture.context();
    bare.session_kind = SessionKind::Extension as i32;
    assert_eq!(
        service
            .action_target(&caller, &bare, "", INSTALLATION)
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    // A provider session reaches only its own entries, whatever library.read.all says.
    assert_eq!(
        service
            .action_target(&caller, &fixture.context(), "", "id-steam:1")
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    assert!(
        service
            .action_target(&caller, &fixture.context(), "", INSTALLATION)
            .is_ok()
    );
}
