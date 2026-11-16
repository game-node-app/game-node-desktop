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

    async fn extension_game(
        grants: &[&'static str],
        target: TargetGameContext,
        dialog: Option<Dialog>,
    ) -> (Self, Arc<FakeHost>, Option<Arc<Dialog>>) {
        let fixture = Fixture::start_with_target(grants, true, None, Some(target)).await;
        fixture.library(&[
            ("steam:1", Some(5), "Selected Steam game"),
            ("steam:2", Some(6), "Other Steam game"),
            ("manual:1", Some(9), "Manual game"),
        ]);
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
    async fn binding(&self, operation: Operation, target_revision: u64) -> Binding {
        self.binding_for(operation, target_revision, "3", INSTALLATION, "")
            .await
    }

    async fn binding_for(
        &self,
        operation: Operation,
        target_revision: u64,
        game_id: &str,
        installation_id: &str,
        recipe_id: &str,
    ) -> Binding {
        let permission = match operation {
            Operation::Launch => cap::GAME_LAUNCH,
            Operation::CustomLaunch => cap::GAME_CUSTOM_LAUNCH,
            Operation::Install => cap::GAMES_INSTALL,
            Operation::BackupRestore => cap::BACKUPS_RESTORE,
        };
        let caller = self
            .api
            .authorize_resource(Some(&self.context()), permission)
            .unwrap();
        let (_, entry) = self
            .feed
            .target(
                &super::super::Scope {
                    own_provider: caller.provider_id.clone(),
                    read_all: true,
                    provider_filter: None,
                },
                installation_id,
            )
            .unwrap();
        let (recipe, dispatch, resources) = if operation == Operation::CustomLaunch {
            let host = self.feed.host().unwrap();
            let recipe = host
                .custom_launch_recipe(
                    super::super::mutations::mutation_owner(&caller),
                    recipe_id.to_owned(),
                )
                .await
                .unwrap()
                .unwrap();
            let dispatch = host
                .prepare_custom_launch(recipe.clone(), None, None, None)
                .await
                .unwrap();
            (Some(recipe), Some(dispatch), Vec::new())
        } else {
            (None, None, Vec::new())
        };
        let plan = super::super::super::launch_plan::resolve(
            &caller,
            &entry,
            target_revision,
            recipe.as_ref(),
            dispatch.as_ref(),
            &resources,
        )
        .unwrap();
        let plan_fingerprint = if matches!(operation, Operation::Launch | Operation::CustomLaunch) {
            plan.fingerprint().unwrap()
        } else {
            String::new()
        };
        Binding {
            plugin_id: "fixture.plugin".into(),
            publisher_fingerprint: "ab".repeat(32),
            profile_epoch: caller.profile_epoch,
            game_id: game_id.into(),
            installation_id: installation_id.into(),
            operation,
            target_revision,
            recipe_id: recipe_id.into(),
            recipe_revision: u64::from(operation == Operation::CustomLaunch),
            plan_fingerprint,
        }
    }

    fn game_owner(&self) -> JobOwner {
        let permission = [
            cap::GAME_LAUNCH,
            cap::GAME_CUSTOM_LAUNCH,
            cap::GAMES_INSTALL,
            cap::BACKUPS_RESTORE,
        ]
        .into_iter()
        .find(|permission| self.capabilities.contains(permission))
        .expect("game fixture needs an action grant");
        JobOwner::of(
            &self
                .api
                .authorize_resource(Some(&self.context()), permission)
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
async fn custom_launch_recipes_are_idempotent_requestable_and_removable() {
    let (fixture, host, _) = Fixture::game(
        &[cap::GAME_LAUNCH, cap::GAME_CUSTOM_LAUNCH],
        Some(answering(&[APPROVE])),
    )
    .await;
    host.launch_target.store(1, Ordering::SeqCst);
    let revision = fixture.revision().await;
    let proposal = CustomLaunchRequest {
        context: Some(fixture.context()),
        game_id: "3".into(),
        installation_id: INSTALLATION.into(),
        expected_revision: revision,
        idempotency_key: "propose-custom-1".into(),
        uri: "steam://rungameid/3".into(),
        ..CustomLaunchRequest::default()
    };

    let first = fixture
        .client()
        .games_propose_custom_launch(proposal.clone())
        .await
        .unwrap()
        .into_inner();
    let replay = fixture
        .client()
        .games_propose_custom_launch(proposal.clone())
        .await
        .unwrap()
        .into_inner();
    assert_eq!(replay.recipe_id, first.recipe_id);
    assert_eq!(replay.recipe_revision, first.recipe_revision);
    assert_eq!(host.custom_launch_recipes.lock().unwrap().len(), 1);

    let mut changed = proposal;
    changed.uri = "steam://rungameid/4".into();
    assert_eq!(
        fixture
            .client()
            .games_propose_custom_launch(changed)
            .await
            .unwrap_err()
            .code(),
        Code::FailedPrecondition
    );

    let intent = fixture
        .intents
        .issue(
            fixture
                .binding_for(
                    Operation::CustomLaunch,
                    1,
                    "3",
                    INSTALLATION,
                    &first.recipe_id,
                )
                .await,
        )
        .unwrap();
    let request = CustomLaunchRequest {
        context: Some(fixture.context()),
        game_id: "3".into(),
        installation_id: INSTALLATION.into(),
        expected_revision: revision,
        user_intent_id: intent,
        idempotency_key: "execute-custom-1".into(),
        recipe_id: first.recipe_id.clone(),
        recipe_revision: first.recipe_revision,
        ..CustomLaunchRequest::default()
    };
    let reply = fixture
        .client()
        .games_request_custom_launch(request)
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        fixture.ended(&reply.job_id).await.state(),
        JobState::Succeeded
    );
    assert!(matches!(
        host.custom_launch_dispatches.lock().unwrap().as_slice(),
        [crate::plugin_host::library::CustomLaunchDispatch::Uri { uri, .. }]
            if uri.as_str() == "steam://rungameid/3"
    ));

    let removed = fixture
        .client()
        .games_remove_custom_launch(CustomLaunchRemoveRequest {
            mutation: Some(fixture.mutation("remove-custom-1", first.recipe_revision)),
            game_id: "3".into(),
            recipe_id: first.recipe_id,
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(removed.outcome, MutationOutcome::Applied as i32);
    assert!(host.custom_launch_recipes.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn user_play_reviews_selected_recipe_remembers_exact_plan_and_never_falls_back() {
    let (fixture, host, dialog) = Fixture::game(
        &[cap::GAME_LAUNCH, cap::GAME_CUSTOM_LAUNCH],
        Some(answering(&[APPROVE_AND_REMEMBER])),
    )
    .await;
    host.launch_target.store(1, Ordering::SeqCst);

    let profiles = fixture.api.profiles();
    let layout = crate::plugin_host::profile::PluginStateLayout::new(
        fixture.plugin_data.join("host-launch-profile"),
    );
    profiles.set_state_layout(layout.clone());
    let profile = profiles.current().unwrap();
    let identity = crate::plugin_host::grants::PluginIdentity {
        plugin_id: "fixture.plugin".into(),
        publisher_fingerprint: "ab".repeat(32),
        profile_id: profile.profile_id().to_owned(),
    };
    let scope = crate::plugin_host::grants::GrantScope::Declared {
        value: "host-selected-game".into(),
    };
    let request = crate::plugin_host::grants::PermissionRequest {
        permission_id: cap::GAME_CUSTOM_LAUNCH.into(),
        required: false,
        allowed_scopes: [scope.clone()].into(),
    };
    layout
        .open_grants(&profile)
        .unwrap()
        .replace_grants(
            identity.clone(),
            &[request],
            &[crate::plugin_host::grants::PermissionDecision {
                permission_id: cap::GAME_CUSTOM_LAUNCH.into(),
                scope,
                granted: true,
            }],
        )
        .unwrap();
    layout
        .open_activation(&profile)
        .unwrap()
        .set_enabled(&identity.plugin_id, true)
        .unwrap();

    let revision = fixture.revision().await;
    let recipe = fixture
        .client()
        .games_propose_custom_launch(CustomLaunchRequest {
            context: Some(fixture.context()),
            game_id: "3".into(),
            installation_id: INSTALLATION.into(),
            expected_revision: revision,
            idempotency_key: "host-play-recipe".into(),
            uri: "steam://rungameid/3".into(),
            ..CustomLaunchRequest::default()
        })
        .await
        .unwrap()
        .into_inner();
    let resolver = profiles
        .resolver("fixture.plugin", &"ab".repeat(32), None)
        .unwrap();
    host.launch_recipe_selections.lock().unwrap().insert(
        "fixture:3".into(),
        gamenode_core::library::LaunchRecipeSelection {
            entry_key: "fixture:3".into(),
            owner_key: resolver.library_recipe_owner_key(),
            plugin_id: identity.plugin_id.clone(),
            publisher_fingerprint: identity.publisher_fingerprint.clone(),
            recipe_id: recipe.recipe_id,
            recipe_revision: recipe.recipe_revision,
            base_target_revision: 1,
            approved_plan_fingerprint: String::new(),
        },
    );

    let api = fixture._server.resource_api();
    assert!(
        api.launch_selected_recipe("fixture:3".into())
            .await
            .unwrap()
    );
    let shown = dialog.as_ref().unwrap().shown.lock().unwrap().clone();
    assert_eq!(shown.len(), 1);
    let reviewed = shown[0].launch_plan.as_ref().unwrap();
    assert_eq!(reviewed.source_id, "fixture");
    assert_eq!(reviewed.target, "steam://rungameid/3");
    assert!(!reviewed.fingerprint.is_empty());

    // The exact same plan uses the user-approved fingerprint and does not prompt again.
    assert!(
        api.launch_selected_recipe("fixture:3".into())
            .await
            .unwrap()
    );
    assert_eq!(dialog.as_ref().unwrap().shown.lock().unwrap().len(), 1);
    assert_eq!(host.custom_launch_dispatches.lock().unwrap().len(), 2);

    // A changed detected target invalidates the selected recipe and blocks all dispatch.
    host.launch_target.store(2, Ordering::SeqCst);
    assert_eq!(
        api.launch_selected_recipe("fixture:3".into())
            .await
            .unwrap_err()
            .code(),
        Code::FailedPrecondition
    );
    assert_eq!(host.custom_launch_dispatches.lock().unwrap().len(), 2);

    // Revoking custom-launch permission after approval is also fail-closed.
    let revoked_request = crate::plugin_host::grants::PermissionRequest {
        permission_id: cap::GAME_CUSTOM_LAUNCH.into(),
        required: false,
        allowed_scopes: [crate::plugin_host::grants::GrantScope::Declared {
            value: "host-selected-game".into(),
        }]
        .into(),
    };
    layout
        .open_grants(&profile)
        .unwrap()
        .replace_grants(identity, &[revoked_request], &[])
        .unwrap();
    host.launch_target.store(1, Ordering::SeqCst);
    assert_eq!(
        api.launch_selected_recipe("fixture:3".into())
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    assert_eq!(host.custom_launch_dispatches.lock().unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn custom_launch_recipe_removal_during_confirmation_blocks_dispatch() {
    let (dialog, gate) = held(&[APPROVE]);
    let (fixture, host, dialog) =
        Fixture::game(&[cap::GAME_LAUNCH, cap::GAME_CUSTOM_LAUNCH], Some(dialog)).await;
    host.launch_target.store(1, Ordering::SeqCst);
    let revision = fixture.revision().await;
    let recipe = fixture
        .client()
        .games_propose_custom_launch(CustomLaunchRequest {
            context: Some(fixture.context()),
            game_id: "3".into(),
            installation_id: INSTALLATION.into(),
            expected_revision: revision,
            idempotency_key: "propose-recipe-before-confirmation".into(),
            uri: "steam://rungameid/3".into(),
            ..CustomLaunchRequest::default()
        })
        .await
        .unwrap()
        .into_inner();
    let pending = fixture
        .client()
        .games_request_custom_launch(CustomLaunchRequest {
            context: Some(fixture.context()),
            game_id: "3".into(),
            installation_id: INSTALLATION.into(),
            expected_revision: revision,
            idempotency_key: "execute-recipe-after-confirmation".into(),
            recipe_id: recipe.recipe_id.clone(),
            recipe_revision: recipe.recipe_revision,
            ..CustomLaunchRequest::default()
        })
        .await
        .unwrap()
        .into_inner();
    Fixture::dialog_shown(&dialog.unwrap(), 1).await;

    let removed = fixture
        .client()
        .games_remove_custom_launch(CustomLaunchRemoveRequest {
            mutation: Some(fixture.mutation("remove-during-confirmation", recipe.recipe_revision)),
            game_id: "3".into(),
            recipe_id: recipe.recipe_id,
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(removed.outcome, MutationOutcome::Applied as i32);
    gate.notify_one();

    let ended = fixture.ended(&pending.job_id).await;
    assert_eq!(ended.state(), JobState::Failed);
    assert_eq!(ended.result_code, "recipe_changed");
    assert!(host.custom_launch_dispatches.lock().unwrap().is_empty());
    assert!(host.opened.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn remembered_custom_launch_approval_is_bound_to_recipe_identity() {
    let (fixture, host, dialog) = Fixture::game(
        &[cap::GAME_LAUNCH, cap::GAME_CUSTOM_LAUNCH],
        Some(answering(&[APPROVE_AND_REMEMBER, APPROVE])),
    )
    .await;
    let dialog = dialog.unwrap();
    host.launch_target.store(1, Ordering::SeqCst);
    let revision = fixture.revision().await;
    let propose = |key: &'static str, uri: &'static str| CustomLaunchRequest {
        context: Some(fixture.context()),
        game_id: "3".into(),
        installation_id: INSTALLATION.into(),
        expected_revision: revision,
        idempotency_key: key.into(),
        uri: uri.into(),
        ..CustomLaunchRequest::default()
    };
    let first = fixture
        .client()
        .games_propose_custom_launch(propose("remember-recipe-a", "steam://rungameid/3"))
        .await
        .unwrap()
        .into_inner();
    let first_recipe_id = first.recipe_id.clone();
    let first_recipe_revision = first.recipe_revision;
    let first_job = fixture
        .client()
        .games_request_custom_launch(CustomLaunchRequest {
            context: Some(fixture.context()),
            game_id: "3".into(),
            installation_id: INSTALLATION.into(),
            expected_revision: revision,
            idempotency_key: "run-recipe-a".into(),
            recipe_id: first.recipe_id,
            recipe_revision: first.recipe_revision,
            ..CustomLaunchRequest::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        fixture.ended(&first_job.job_id).await.state(),
        JobState::Succeeded
    );
    assert_eq!(dialog.shown.lock().unwrap().len(), 1);
    let remembered_job = fixture
        .client()
        .games_request_custom_launch(CustomLaunchRequest {
            context: Some(fixture.context()),
            game_id: "3".into(),
            installation_id: INSTALLATION.into(),
            expected_revision: revision,
            idempotency_key: "run-remembered-recipe-a".into(),
            recipe_id: first_recipe_id,
            recipe_revision: first_recipe_revision,
            ..CustomLaunchRequest::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        fixture.ended(&remembered_job.job_id).await.state(),
        JobState::Succeeded
    );
    assert_eq!(dialog.shown.lock().unwrap().len(), 1);

    let second = fixture
        .client()
        .games_propose_custom_launch(propose("remember-recipe-b", "steam://rungameid/4"))
        .await
        .unwrap()
        .into_inner();
    let second_job = fixture
        .client()
        .games_request_custom_launch(CustomLaunchRequest {
            context: Some(fixture.context()),
            game_id: "3".into(),
            installation_id: INSTALLATION.into(),
            expected_revision: revision,
            idempotency_key: "run-recipe-b".into(),
            recipe_id: second.recipe_id,
            recipe_revision: second.recipe_revision,
            ..CustomLaunchRequest::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        fixture.ended(&second_job.job_id).await.state(),
        JobState::Succeeded
    );
    assert_eq!(dialog.shown.lock().unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pre_issued_intent_skips_the_dialog_and_is_spent_once() {
    // No dialog at all in this build: the intent is enough.
    let (fixture, host, _) = Fixture::game(GRANTS, None).await;
    let id = fixture
        .intents
        .issue(fixture.binding(Operation::Launch, 0).await)
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
        .issue(fixture.binding(Operation::Install, 0).await)
        .unwrap();
    // Issued for a target that is not the current one.
    let stale = fixture
        .intents
        .issue(fixture.binding(Operation::Launch, 99).await)
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

#[tokio::test(flavor = "multi_thread")]
async fn extension_custom_overlay_targets_selected_steam_without_read_or_provider_identity() {
    let target = TargetGameContext {
        game_id: "5".into(),
        installation_id: "id-steam:1".into(),
        profile_id: "account-a".into(),
        revision: "steam-target-r1".into(),
    };
    let (fixture, host, _) =
        Fixture::extension_game(&[cap::GAME_CUSTOM_LAUNCH], target, None).await;
    host.launch_target.store(1, Ordering::SeqCst);
    let context = fixture.context();
    assert_eq!(context.session_kind(), SessionKind::Extension);
    assert!(context.provider_id.is_empty());
    assert_eq!(
        context.target_game.as_ref().unwrap().installation_id,
        "id-steam:1"
    );
    assert_eq!(
        fixture.list("").await.unwrap_err().code(),
        Code::PermissionDenied
    );

    let revision = fixture.feed.revision_of("steam:1").unwrap();
    assert_eq!(
        fixture
            .client()
            .games_request_launch(GameLaunchRequest {
                context: Some(context.clone()),
                game_id: "5".into(),
                installation_id: "id-steam:1".into(),
                expected_revision: revision,
                user_intent_id: String::new(),
                idempotency_key: "normal-without-grant".into(),
            })
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    let proposal = CustomLaunchRequest {
        context: Some(context.clone()),
        game_id: "5".into(),
        installation_id: "id-steam:1".into(),
        expected_revision: revision,
        idempotency_key: "extension-custom-proposal".into(),
        uri: "steam://rungameid/5".into(),
        ..CustomLaunchRequest::default()
    };
    let recipe = fixture
        .client()
        .games_propose_custom_launch(proposal.clone())
        .await
        .unwrap()
        .into_inner();
    assert_eq!(host.custom_launch_recipes.lock().unwrap().len(), 1);
    let stored = host
        .custom_launch_recipes
        .lock()
        .unwrap()
        .values()
        .next()
        .cloned()
        .unwrap();
    assert_eq!(
        (stored.game_id, stored.installation_id.as_str()),
        (5, "id-steam:1")
    );

    let mut forged_provider = proposal.clone();
    forged_provider.context.as_mut().unwrap().provider_id = "steam".into();
    forged_provider.idempotency_key = "forged-provider".into();
    assert!(
        fixture
            .client()
            .games_propose_custom_launch(forged_provider)
            .await
            .is_err()
    );
    let mut other_game = proposal;
    other_game.game_id = "6".into();
    other_game.installation_id = "id-steam:2".into();
    other_game.idempotency_key = "other-steam-game".into();
    assert_eq!(
        fixture
            .client()
            .games_propose_custom_launch(other_game)
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );

    let intent = fixture
        .intents
        .issue(
            fixture
                .binding_for(
                    Operation::CustomLaunch,
                    1,
                    "5",
                    "id-steam:1",
                    &recipe.recipe_id,
                )
                .await,
        )
        .unwrap();
    let request = CustomLaunchRequest {
        context: Some(context),
        game_id: "5".into(),
        installation_id: "id-steam:1".into(),
        expected_revision: revision,
        user_intent_id: intent,
        idempotency_key: "extension-custom-execution".into(),
        recipe_id: recipe.recipe_id.clone(),
        recipe_revision: recipe.recipe_revision,
        ..CustomLaunchRequest::default()
    };
    let executed = fixture
        .client()
        .games_request_custom_launch(request.clone())
        .await
        .unwrap()
        .into_inner();
    let replay = fixture
        .client()
        .games_request_custom_launch(request.clone())
        .await
        .unwrap()
        .into_inner();
    assert_eq!(replay.job_id, executed.job_id);
    assert_eq!(
        fixture.ended(&executed.job_id).await.state(),
        JobState::Succeeded
    );
    let mut reused_intent = request;
    reused_intent.idempotency_key = "extension-custom-reuse-intent".into();
    assert_eq!(
        fixture
            .client()
            .games_request_custom_launch(reused_intent)
            .await
            .unwrap_err()
            .code(),
        Code::FailedPrecondition
    );
    assert_eq!(host.custom_launch_dispatches.lock().unwrap().len(), 1);
    assert!(host.opened.lock().unwrap().is_empty());
    let steam_scope = Scope {
        own_provider: "steam".into(),
        read_all: false,
        provider_filter: None,
    };
    let (_, selected_steam_entry) = fixture.feed.target(&steam_scope, "id-steam:1").unwrap();
    assert_eq!(selected_steam_entry.name, "Selected Steam game");
    assert_eq!(selected_steam_entry.game_id, Some(5));
}

#[tokio::test(flavor = "multi_thread")]
async fn normal_and_custom_launch_permissions_are_independent_for_extensions() {
    let target = TargetGameContext {
        game_id: "5".into(),
        installation_id: "id-steam:1".into(),
        profile_id: "account-a".into(),
        revision: "steam-target-r1".into(),
    };
    let (fixture, host, _) = Fixture::extension_game(&[cap::GAME_LAUNCH], target, None).await;
    host.launch_target.store(1, Ordering::SeqCst);
    let revision = fixture.feed.revision_of("steam:1").unwrap();
    let custom_proposal = fixture
        .client()
        .games_propose_custom_launch(CustomLaunchRequest {
            context: Some(fixture.context()),
            game_id: "5".into(),
            installation_id: "id-steam:1".into(),
            expected_revision: revision,
            idempotency_key: "without-custom-grant".into(),
            uri: "steam://rungameid/5".into(),
            ..CustomLaunchRequest::default()
        })
        .await
        .unwrap_err();
    assert_eq!(custom_proposal.code(), Code::PermissionDenied);

    let intent = fixture
        .intents
        .issue(
            fixture
                .binding_for(Operation::Launch, 1, "5", "id-steam:1", "")
                .await,
        )
        .unwrap();
    let launched = fixture
        .client()
        .games_request_launch(GameLaunchRequest {
            context: Some(fixture.context()),
            game_id: "5".into(),
            installation_id: "id-steam:1".into(),
            expected_revision: revision,
            user_intent_id: intent,
            idempotency_key: "normal-launch".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        fixture.ended(&launched.job_id).await.state(),
        JobState::Succeeded
    );
    assert_eq!(*host.opened.lock().unwrap(), [("steam:1".into(), false)]);
    assert!(host.custom_launch_recipes.lock().unwrap().is_empty());
}
