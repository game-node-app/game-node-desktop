//! The window bridge and the settings list of remembered confirmations (task 10.10).

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::*;
use crate::plugin_host::intents::Operation;
use gamenode_platform::plugin_storage::IntentApproval;

fn request() -> ConfirmationRequest {
    ConfirmationRequest {
        plugin_id: "org.example".into(),
        publisher_fingerprint: "ab".repeat(32),
        game_name: "Game <b>one</b>".into(),
        operation: Operation::Install,
    }
}

/// A window that records what it was told and whether it is open.
fn window() -> (Emit, Arc<Mutex<Vec<Signal>>>, Arc<AtomicBool>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let open = Arc::new(AtomicBool::new(true));
    let emit: Emit = {
        let (seen, open) = (seen.clone(), open.clone());
        Arc::new(move |signal| {
            seen.lock().unwrap().push(signal);
            open.load(Ordering::SeqCst)
        })
    };
    (emit, seen, open)
}

async fn shown(seen: &Arc<Mutex<Vec<Signal>>>) -> PluginConfirmation {
    for _ in 0..200 {
        if let Some(Signal::Open(confirmation)) = seen.lock().unwrap().first().cloned() {
            return confirmation;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("nothing was shown");
}

#[tokio::test]
async fn the_window_is_shown_the_request_and_its_answer_comes_back_once() {
    let (emit, seen, _) = window();
    let bridge = Arc::new(ConfirmationBridge::new(emit));
    let asking = tokio::spawn({
        let bridge = bridge.clone();
        async move { bridge.confirm(request()).await }
    });
    let confirmation = shown(&seen).await;
    assert_eq!(
        (
            confirmation.plugin_id.as_str(),
            confirmation.game_name.as_str(),
            confirmation.operation.as_str(),
            confirmation.timeout_seconds,
        ),
        ("org.example", "Game <b>one</b>", "install", 300)
    );
    assert_eq!(confirmation.id.len(), 32);

    // An unknown id does nothing; the real one answers, and only once.
    assert!(!bridge.answer("nothing", true, false));
    assert!(bridge.answer(&confirmation.id, true, true));
    assert!(!bridge.answer(&confirmation.id, false, false));
    assert_eq!(
        asking.await.unwrap().unwrap(),
        Answer {
            approved: true,
            remember: true
        }
    );
    // An answered confirmation is closed by the window itself: no second signal.
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_confirmation_that_stops_waiting_is_closed_in_the_window() {
    let (emit, seen, _) = window();
    let bridge = Arc::new(ConfirmationBridge::new(emit));
    let asking = tokio::spawn({
        let bridge = bridge.clone();
        async move { bridge.confirm(request()).await }
    });
    let confirmation = shown(&seen).await;
    // The host stops waiting (the request was withdrawn, or the time ran out).
    asking.abort();
    let _ = asking.await;
    for _ in 0..200 {
        if seen.lock().unwrap().len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(
        seen.lock().unwrap()[1],
        Signal::Closed(confirmation.id.clone())
    );
    // A late answer to a closed confirmation is ignored.
    assert!(!bridge.answer(&confirmation.id, true, false));
}

#[tokio::test]
async fn with_no_window_nothing_can_be_asked() {
    let (emit, _, open) = window();
    open.store(false, Ordering::SeqCst);
    let bridge = ConfirmationBridge::new(emit);
    let status = bridge.confirm(request()).await.unwrap_err();
    assert_eq!(status.code(), tonic::Code::Unavailable);
}

#[tokio::test]
async fn the_settings_list_reads_and_revokes_a_plugins_remembered_confirmations() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Arc::new(PluginStorage::new(temp.path()));
    let profiles = Arc::new(PluginProfiles::default());
    let approvals = PluginApprovals::new(storage.clone(), profiles.clone());
    let publisher = "ab".repeat(32);

    // No account profile: nothing to list.
    assert!(approvals.list("org.example", &publisher).await.is_err());

    profiles.update(Some("a"), true);
    assert!(
        approvals
            .list("org.example", &publisher)
            .await
            .unwrap()
            .is_empty()
    );

    // The host remembers a confirmation for the plugin (as a request's approval does).
    let resolver = profiles.resolver("org.example", &publisher, None).unwrap();
    storage
        .run_for_host(&resolver, profiles.epoch(), |namespace| {
            let guard: crate::plugin_host::library::CommitGuard = Box::new(|| Ok(()));
            namespace.save_intent_approval(
                &IntentApproval {
                    id: "one".into(),
                    game_id: "3".into(),
                    installation_id: "inst".into(),
                    operation: "launch".into(),
                    target_revision: 0,
                    game_name: "Game".into(),
                    approved_at: 77,
                },
                &guard,
            )
        })
        .await
        .unwrap();

    let listed = approvals.list("org.example", &publisher).await.unwrap();
    assert_eq!(
        listed,
        [RememberedConfirmation {
            id: "one".into(),
            game_name: "Game".into(),
            operation: "launch".into(),
            approved_at: 77
        }]
    );
    // Another plugin has none, and cannot revoke this one's.
    assert!(
        approvals
            .list("org.other", &publisher)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        !approvals
            .revoke("org.other", &publisher, "one")
            .await
            .unwrap()
    );
    assert!(
        approvals
            .revoke("org.example", &publisher, "one")
            .await
            .unwrap()
    );
    assert!(
        !approvals
            .revoke("org.example", &publisher, "one")
            .await
            .unwrap()
    );
    assert!(
        approvals
            .list("org.example", &publisher)
            .await
            .unwrap()
            .is_empty()
    );

    // A different account has its own (empty) list.
    profiles.update(Some("b"), true);
    assert!(
        approvals
            .list("org.example", &publisher)
            .await
            .unwrap()
            .is_empty()
    );
}
