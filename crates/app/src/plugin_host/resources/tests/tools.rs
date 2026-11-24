//! Real socket authorization/job tests with a controlled Core review adapter.
use super::*;
use crate::plugin_host::jobs::JobOwner;

const GRANTS: &[&str] = &[
    cap::GAME_LAUNCH,
    cap::TOOLS_READ,
    cap::TOOLS_APPLY,
    cap::TOOLS_REMOVE,
];
async fn setup() -> (Fixture, Arc<FakeHost>, u64) {
    let f = Fixture::new(GRANTS).await;
    f.library(&[
        ("fixture:1", Some(3), "Game"),
        ("steam:1", Some(5), "Foreign"),
    ]);
    let host = f.with_host();
    host.tool_support.store(true, Ordering::SeqCst);
    let revision = f.feed.revision_of("fixture:1").unwrap();
    (f, host, revision)
}
async fn apply(
    f: &Fixture,
    key: &str,
    revision: u64,
    tool: &str,
    game: &str,
) -> Result<JobReply, Status> {
    f.client()
        .tools_request_apply(ToolApplyRequest {
            mutation: Some(f.mutation(key, revision)),
            tool_id: tool.into(),
            game_id: game.into(),
            user_intent_id: String::new(),
        })
        .await
        .map(Response::into_inner)
}
async fn reviewed(host: &FakeHost) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while host.tool_reviews.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
async fn ended(f: &Fixture, id: &str) -> JobReply {
    let caller = f
        .api
        .authorize_resource_session(Some(&f.context()))
        .unwrap();
    let owner = JobOwner::of(&caller).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let job = f.jobs.get(&owner, id).unwrap();
            if !matches!(job.state(), JobState::Queued | JobState::Running) {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn catalog_is_paged_scoped_and_does_not_disclose_installer_inputs() {
    let (f, _, _) = setup().await;
    let list = |cursor: String| ToolsListRequest {
        page: Some(PageRequest {
            context: Some(f.context()),
            page_size: 1,
            cursor,
            ..Default::default()
        }),
        game_id: "3".into(),
        platform: "windows".into(),
    };
    let first = f
        .client()
        .tools_list(list(String::new()))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(first.tools.len(), 1);
    assert!(first.next_cursor.is_empty()); // The production catalog currently has one tool.
    assert_eq!(first.tools[0].version, "fixture-v1");
    assert_eq!(
        f.client()
            .tools_list(list("edited".into()))
            .await
            .unwrap_err()
            .code(),
        Code::FailedPrecondition
    );
    let mut foreign = list(String::new());
    foreign.game_id = "5".into();
    assert!(f.client().tools_list(foreign).await.is_err());
    let mut linux = list(String::new());
    linux.platform = "linux".into();
    assert!(
        f.client()
            .tools_list(linux)
            .await
            .unwrap()
            .into_inner()
            .tools
            .is_empty()
    );
    f.api
        .revoke_session_permission("fixture", cap::TOOLS_READ)
        .await;
    assert_eq!(
        f.client()
            .tools_list(list(first.next_cursor))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
}
#[tokio::test]
async fn apply_and_remove_wait_for_review_retries_do_not_duplicate_and_permissions_are_independent()
{
    let (f, host, revision) = setup().await;
    let job = apply(&f, "apply", revision, "optiscaler", "3")
        .await
        .unwrap();
    reviewed(&host).await;
    assert_eq!(host.tool_effects.load(Ordering::SeqCst), 0);
    assert_eq!(
        apply(&f, "apply", revision, "optiscaler", "3")
            .await
            .unwrap()
            .job_id,
        job.job_id
    );
    assert_eq!(host.tool_reviews.lock().unwrap().len(), 1);
    host.tool_gate.notify_one();
    assert_eq!(ended(&f, &job.job_id).await.state(), JobState::Succeeded);
    assert_eq!(host.tool_effects.load(Ordering::SeqCst), 1);
    let removed = f
        .client()
        .tools_request_remove(ToolRemoveRequest {
            mutation: Some(f.mutation("remove", revision)),
            tool_id: "optiscaler".into(),
            game_id: "3".into(),
            user_intent_id: String::new(),
        })
        .await
        .unwrap()
        .into_inner();
    tokio::time::timeout(Duration::from_secs(5), async {
        while host.tool_reviews.lock().unwrap().len() != 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(host.tool_reviews.lock().unwrap()[1].2);
    host.tool_gate.notify_one();
    assert_eq!(
        ended(&f, &removed.job_id).await.state(),
        JobState::Succeeded
    );
    f.api
        .revoke_session_permission("fixture", cap::TOOLS_APPLY)
        .await;
    assert_eq!(
        apply(&f, "denied", revision, "optiscaler", "3")
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
}
#[tokio::test]
async fn arbitrary_installers_foreign_ambiguous_stale_and_unsupported_targets_never_reach_review() {
    let (f, host, revision) = setup().await;
    for bad in [
        "https://evil.invalid/install.exe",
        "../optiscaler",
        "",
        "arbitrary",
    ] {
        assert_eq!(
            apply(&f, "bad", revision, bad, "3")
                .await
                .unwrap_err()
                .code(),
            Code::NotFound
        );
    }
    assert!(
        apply(&f, "foreign", revision, "optiscaler", "5")
            .await
            .is_err()
    );
    assert_eq!(
        apply(&f, "stale", revision + 1, "optiscaler", "3")
            .await
            .unwrap_err()
            .code(),
        Code::Aborted
    );
    host.tool_support.store(false, Ordering::SeqCst);
    assert_eq!(
        apply(&f, "unsupported", revision, "optiscaler", "3")
            .await
            .unwrap_err()
            .code(),
        Code::Unavailable
    );
    host.tool_support.store(true, Ordering::SeqCst);
    f.library(&[
        ("fixture:1", Some(3), "Game"),
        ("fixture:2", Some(3), "Other install"),
    ]);
    assert_eq!(
        apply(&f, "ambiguous", revision, "optiscaler", "3")
            .await
            .unwrap_err()
            .code(),
        Code::FailedPrecondition
    );
    assert!(host.tool_reviews.lock().unwrap().is_empty());
}
#[tokio::test]
async fn decline_revocation_cancellation_and_changed_target_prevent_effects() {
    for mode in ["decline", "revoke", "cancel", "change"] {
        let (f, host, revision) = setup().await;
        let job = apply(&f, mode, revision, "optiscaler", "3").await.unwrap();
        reviewed(&host).await;
        match mode {
            "decline" => host.tool_declined.store(true, Ordering::SeqCst),
            "revoke" => {
                f.api
                    .revoke_session_permission("fixture", cap::TOOLS_APPLY)
                    .await;
            }
            "change" => f.library(&[("fixture:1", Some(3), "Changed")]),
            "cancel" => {
                let caller = f
                    .api
                    .authorize_resource_session(Some(&f.context()))
                    .unwrap();
                let owner = JobOwner::of(&caller).unwrap();
                let jobs = f.jobs.clone();
                let id = job.job_id.clone();
                let cancel = tokio::spawn(async move { jobs.cancel(&owner, &id).await });
                tokio::time::sleep(Duration::from_millis(20)).await;
                host.tool_gate.notify_one();
                cancel.await.unwrap().unwrap();
            }
            _ => unreachable!(),
        }
        if mode != "cancel" {
            host.tool_gate.notify_one();
        }
        assert_ne!(ended(&f, &job.job_id).await.state(), JobState::Succeeded);
        assert_eq!(host.tool_effects.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn an_extension_can_review_only_its_host_selected_foreign_provider_game() {
    let f = Fixture::start_with_target(
        GRANTS,
        true,
        None,
        Some(TargetGameContext {
            game_id: "5".into(),
            installation_id: "id-steam:1".into(),
            profile_id: String::new(),
            revision: String::new(),
        }),
    )
    .await;
    f.library(&[
        ("steam:1", Some(5), "Selected"),
        ("steam:2", Some(6), "Other"),
    ]);
    let host = f.with_host();
    host.tool_support.store(true, Ordering::SeqCst);
    let revision = f.feed.revision_of("steam:1").unwrap();
    assert!(
        apply(&f, "other", revision, "optiscaler", "6")
            .await
            .is_err()
    );
    let job = apply(&f, "selected", revision, "OPTISCALER", "5")
        .await
        .unwrap();
    reviewed(&host).await;
    assert_eq!(
        host.tool_reviews.lock().unwrap()[0],
        ("steam:1".into(), "optiscaler".into(), false)
    );
    host.tool_gate.notify_one();
    assert_eq!(ended(&f, &job.job_id).await.state(), JobState::Succeeded);
}
