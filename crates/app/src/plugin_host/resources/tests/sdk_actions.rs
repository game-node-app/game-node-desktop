//! The SDK's action helpers and recipes against the fixture host: the same requests a raw call
//! sends, jobs followed to completion, and a permission revoked while a confirmation is open.
use std::sync::Mutex as StdMutex;

use super::*;
use crate::plugin_host::intents::{Answer, ConfirmationRequest, Prompter};
use gamenode_plugin_sdk::{Actions, Code, idempotency_key, proto::JobState};
use tokio::sync::Notify;

use super::action_recipes;

const GRANTS: &[&str] = &[
    cap::LIBRARY_READ,
    cap::GAME_LAUNCH,
    cap::TOOLS_READ,
    cap::TOOLS_APPLY,
    cap::TOOLS_REMOVE,
    cap::PORTS_READ,
    cap::PORTS_INSTALL,
];

/// The user's side of Core's confirmation dialog: answers when the test lets it go.
#[derive(Default)]
struct Dialog {
    shown: StdMutex<Vec<ConfirmationRequest>>,
    gate: Option<Arc<Notify>>,
    approve: bool,
}

#[tonic::async_trait]
impl Prompter for Dialog {
    async fn confirm(&self, request: ConfirmationRequest) -> Result<Answer, Status> {
        self.shown.lock().unwrap().push(request);
        if let Some(gate) = &self.gate {
            gate.notified().await;
        }
        Ok(Answer {
            approved: self.approve,
            remember: false,
        })
    }
}

async fn shown(dialog: &Dialog) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while dialog.shown.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

fn catalog() -> Vec<PortRecord> {
    ["alpha", "beta"]
        .into_iter()
        .map(|name| PortRecord {
            port_id: format!("qcat_{name}"),
            name: name.into(),
            platform: "windows".into(),
            availability: "available".into(),
        })
        .collect()
}

async fn setup() -> (Fixture, Arc<FakeHost>, Actions) {
    let f = Fixture::new(GRANTS).await;
    f.library(&[("fixture:1", Some(3), "Game")]);
    let host = f.with_host();
    host.tool_support.store(true, Ordering::SeqCst);
    host.port_support.store(true, Ordering::SeqCst);
    *host.port_records.lock().unwrap() = catalog();
    let actions = f.host.actions().expect("an API 1.6 session");
    (f, host, actions)
}

#[tokio::test(flavor = "multi_thread")]
async fn tool_calls_through_the_sdk_match_raw_calls_and_the_job_runs_to_the_end() {
    let (f, host, actions) = setup().await;
    let revision = f.feed.revision_of("fixture:1").unwrap();
    let raw = f
        .client()
        .tools_list(ToolsListRequest {
            page: Some(PageRequest {
                context: Some(f.context()),
                page_size: 200,
                ..Default::default()
            }),
            game_id: "3".into(),
            platform: "windows".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(actions.tools("3", "windows").await.unwrap(), raw.tools);

    let job = actions
        .apply_tool("optiscaler", "3", revision, &idempotency_key())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while host.tool_reviews.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let waiting = tokio::spawn({
        let actions = actions.clone();
        let id = job.job_id.clone();
        async move { action_recipes::wait_for_job(&actions, &id).await }
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!waiting.is_finished(), "the review is still open");
    host.tool_gate.notify_one();
    let done = waiting.await.unwrap().unwrap();
    assert_eq!(done.state, JobState::Succeeded);
    assert_eq!(host.tool_effects.load(Ordering::SeqCst), 1);

    let removed = actions
        .remove_tool("optiscaler", "3", revision, &idempotency_key())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while host.tool_reviews.lock().unwrap().len() < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    host.tool_gate.notify_one();
    let removed = action_recipes::wait_for_job(&actions, &removed.job_id)
        .await
        .unwrap();
    assert_eq!(removed.state, JobState::Succeeded);
    assert_eq!(host.tool_effects.load(Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn revoking_the_permission_while_a_confirmation_is_open_prevents_the_effect() {
    let (f, host, actions) = setup().await;
    let revision = f.feed.revision_of("fixture:1").unwrap();
    let job = actions
        .apply_tool("optiscaler", "3", revision, &idempotency_key())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while host.tool_reviews.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    // The user is still looking at the dialog when the grant goes away.
    f.api
        .revoke_session_permission("fixture", cap::TOOLS_APPLY)
        .await;
    host.tool_gate.notify_one();
    let result = action_recipes::wait_for_job(&actions, &job.job_id).await;
    match result {
        Ok(done) => assert_eq!(done.state, JobState::Failed),
        Err(status) => assert_eq!(status.code(), Code::PermissionDenied),
    }
    assert_eq!(host.tool_effects.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn port_calls_through_the_sdk_match_raw_calls_and_install_waits_for_core() {
    let (f, host, actions) = setup().await;
    let raw = f
        .client()
        .ports_list(PortsListRequest {
            page: Some(PageRequest {
                context: Some(f.context()),
                page_size: 200,
                ..Default::default()
            }),
            platform: "windows".into(),
            query: String::new(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(actions.ports("windows", "").await.unwrap(), raw.ports);
    assert_eq!(
        actions.port("qcat_alpha").await.unwrap(),
        raw.ports[0].clone()
    );

    let job = actions
        .install_port("qcat_alpha", "", vec![], 0, &idempotency_key())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while host.port_reviews.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let waiting = tokio::spawn({
        let actions = actions.clone();
        let id = job.job_id.clone();
        async move { action_recipes::wait_for_job(&actions, &id).await }
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!waiting.is_finished());
    host.port_gate.notify_one();
    assert_eq!(waiting.await.unwrap().unwrap().state, JobState::Succeeded);
    assert_eq!(host.port_effects.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn launching_through_the_sdk_waits_for_the_user_and_a_revoked_launch_starts_nothing() {
    let f = Fixture::new(&[cap::LIBRARY_READ, cap::GAME_LAUNCH]).await;
    f.library(&[("fixture:1", Some(3), "Game")]);
    let host = f.with_host();
    let dialog = Arc::new(Dialog {
        approve: true,
        ..Dialog::default()
    });
    f.intents.set_prompter(dialog.clone());
    let actions = f.host.actions().unwrap();
    let revision = f.feed.revision_of("fixture:1").unwrap();
    let waiting = tokio::spawn({
        let actions = actions.clone();
        async move { action_recipes::launch(&actions, "id-fixture:1", revision).await }
    });
    shown(&dialog).await;
    let done = waiting.await.unwrap().unwrap();
    assert_eq!(
        (done.state, done.result_code.as_str()),
        (JobState::Succeeded, "opened")
    );
    assert_eq!(
        *host.opened.lock().unwrap(),
        [("fixture:1".to_owned(), false)]
    );

    let gate = Arc::new(Notify::new());
    let dialog = Arc::new(Dialog {
        gate: Some(gate.clone()),
        approve: true,
        ..Dialog::default()
    });
    f.intents.set_prompter(dialog.clone());
    let waiting = tokio::spawn({
        let actions = actions.clone();
        async move { action_recipes::launch(&actions, "id-fixture:1", revision).await }
    });
    shown(&dialog).await;
    f.api
        .revoke_session_permission("fixture", cap::GAME_LAUNCH)
        .await;
    gate.notify_one();
    let result = waiting.await.unwrap();
    match result {
        Ok(done) => assert_eq!(done.state, JobState::Failed),
        Err(status) => assert_eq!(status.code(), Code::PermissionDenied),
    }
    assert_eq!(
        *host.opened.lock().unwrap(),
        [("fixture:1".to_owned(), false)]
    );
}
