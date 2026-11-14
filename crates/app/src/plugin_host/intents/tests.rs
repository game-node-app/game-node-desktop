//! The intent table and the user's port (task 10.10), without a plugin session. The requests
//! that use them are tested over the real socket in `resources/tests/games.rs`.

use std::sync::Mutex as StdMutex;

use super::*;

fn binding() -> Binding {
    Binding {
        plugin_id: "org.example".into(),
        publisher_fingerprint: "ab".repeat(32),
        profile_epoch: 3,
        game_id: "42".into(),
        installation_id: "inst-1".into(),
        operation: Operation::Launch,
        target_revision: 7,
    }
}

fn clocked() -> (Intents, Arc<StdMutex<Instant>>) {
    let now = Arc::new(StdMutex::new(Instant::now()));
    let intents = Intents::default().with_clock({
        let now = now.clone();
        move || *now.lock().unwrap()
    });
    (intents, now)
}

#[test]
fn an_intent_is_spent_by_its_first_redemption_only() {
    let intents = Intents::default();
    let id = intents.issue(binding()).unwrap();
    assert!(id.starts_with("intent-") && id.len() > 20);
    assert_ne!(
        id,
        intents.issue(binding()).unwrap(),
        "ids are never repeated"
    );
    intents.redeem(&binding(), &id).unwrap();
    assert_eq!(
        intents.redeem(&binding(), &id).unwrap_err().code(),
        tonic::Code::FailedPrecondition
    );
}

#[test]
fn an_intent_cannot_be_shifted_to_another_plugin_profile_game_operation_or_revision() {
    let intents = Intents::default();
    let id = intents.issue(binding()).unwrap();
    let shifted = [
        Binding {
            plugin_id: "org.other".into(),
            ..binding()
        },
        Binding {
            publisher_fingerprint: "cd".repeat(32),
            ..binding()
        },
        Binding {
            profile_epoch: 4,
            ..binding()
        },
        Binding {
            game_id: "43".into(),
            ..binding()
        },
        Binding {
            installation_id: "inst-2".into(),
            ..binding()
        },
        Binding {
            operation: Operation::Install,
            ..binding()
        },
        Binding {
            target_revision: 8,
            ..binding()
        },
    ];
    let unknown = intents.redeem(&binding(), "intent-nothing").unwrap_err();
    for other in &shifted {
        let refused = intents.redeem(other, &id).unwrap_err();
        // Every cause answers the same, so an id cannot be probed.
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
        assert_eq!(refused.message(), unknown.message());
    }
    // The failed attempts spent nothing: the right request still works, once.
    intents.redeem(&binding(), &id).unwrap();
}

#[test]
fn an_intent_expires() {
    let (intents, now) = clocked();
    let id = intents.issue(binding()).unwrap();
    *now.lock().unwrap() += INTENT_TTL - Duration::from_secs(1);
    let still = intents.issue(binding()).unwrap();
    *now.lock().unwrap() += Duration::from_secs(2);
    assert!(intents.redeem(&binding(), &id).is_err());
    intents.redeem(&binding(), &still).unwrap();
}

#[test]
fn the_table_is_bounded_and_the_oldest_intent_goes_first() {
    let (intents, now) = clocked();
    let first = intents.issue(binding()).unwrap();
    for _ in 1..MAX_ISSUED {
        *now.lock().unwrap() += Duration::from_millis(1);
        intents.issue(binding()).unwrap();
    }
    *now.lock().unwrap() += Duration::from_millis(1);
    let newest = intents.issue(binding()).unwrap();
    assert!(intents.redeem(&binding(), &first).is_err());
    intents.redeem(&binding(), &newest).unwrap();
}

#[test]
fn one_confirmation_per_game_and_a_few_per_plugin_may_wait() {
    let intents = Arc::new(Intents::default());
    let first = intents.begin_pending(&binding()).unwrap();
    assert_eq!(
        intents.begin_pending(&binding()).err().unwrap().code(),
        tonic::Code::FailedPrecondition
    );
    // Another game, or the same game's other operation, may wait too, up to the cap.
    let install = Binding {
        operation: Operation::Install,
        ..binding()
    };
    let mut held = vec![intents.begin_pending(&install).unwrap()];
    for game in 0..MAX_PENDING_PER_PLUGIN - 2 {
        held.push(
            intents
                .begin_pending(&Binding {
                    game_id: format!("g{game}"),
                    ..binding()
                })
                .unwrap(),
        );
    }
    assert_eq!(
        intents
            .begin_pending(&Binding {
                game_id: "one-too-many".into(),
                ..binding()
            })
            .err()
            .unwrap()
            .code(),
        tonic::Code::ResourceExhausted
    );
    // Another plugin is not held back.
    let other = Binding {
        plugin_id: "org.other".into(),
        ..binding()
    };
    let _theirs = intents.begin_pending(&other).unwrap();
    // Dropping a slot frees it.
    drop(first);
    held.push(intents.begin_pending(&binding()).unwrap());
}

struct Scripted(StdMutex<Option<Result<Answer, Status>>>, Duration);

#[async_trait]
impl Prompter for Scripted {
    async fn confirm(&self, request: ConfirmationRequest) -> Result<Answer, Status> {
        assert_eq!(request.plugin_id, "org.example");
        tokio::time::sleep(self.1).await;
        self.0.lock().unwrap().take().unwrap()
    }
}

fn request() -> ConfirmationRequest {
    ConfirmationRequest {
        plugin_id: "org.example".into(),
        publisher_fingerprint: "ab".repeat(32),
        game_name: "Game".into(),
        operation: Operation::Launch,
    }
}

fn answering(answer: Result<Answer, Status>, after: Duration) -> Arc<Scripted> {
    Arc::new(Scripted(StdMutex::new(Some(answer)), after))
}

#[tokio::test]
async fn without_a_dialog_a_confirmation_cannot_be_asked() {
    let status = Intents::default().confirm(request()).await.unwrap_err();
    assert_eq!(status.code(), tonic::Code::FailedPrecondition);
}

#[tokio::test]
async fn the_users_answer_is_reported_as_given() {
    for (answer, expected) in [
        (
            Answer {
                approved: true,
                remember: true,
            },
            Verdict::Approved { remember: true },
        ),
        (
            Answer {
                approved: true,
                remember: false,
            },
            Verdict::Approved { remember: false },
        ),
        // Remembering is part of an approval, never of a refusal.
        (
            Answer {
                approved: false,
                remember: true,
            },
            Verdict::Declined,
        ),
    ] {
        let intents = Intents::default();
        intents.set_prompter(answering(Ok(answer), Duration::ZERO));
        assert_eq!(intents.confirm(request()).await.unwrap(), expected);
    }
}

#[tokio::test]
async fn an_unanswered_dialog_times_out_as_declined_and_an_error_is_passed_on() {
    let intents = Intents::default().with_timeout(Duration::from_millis(20));
    intents.set_prompter(answering(
        Ok(Answer {
            approved: true,
            remember: false,
        }),
        Duration::from_secs(5),
    ));
    assert_eq!(intents.confirm(request()).await.unwrap(), Verdict::TimedOut);

    let failing = Intents::default();
    failing.set_prompter(answering(
        Err(Status::unavailable("no window")),
        Duration::ZERO,
    ));
    assert_eq!(
        failing.confirm(request()).await.unwrap_err().code(),
        tonic::Code::Unavailable
    );
}
