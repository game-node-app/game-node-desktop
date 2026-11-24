//! Ports over the real transport, with a controlled guided-setup adapter.
use super::*;
use crate::plugin_host::jobs::JobOwner;
const GRANTS: &[&str] = &[cap::PORTS_READ, cap::PORTS_INSTALL];
fn catalog() -> Vec<PortRecord> {
    ["alpha", "beta"]
        .into_iter()
        .map(|s| PortRecord {
            port_id: format!("qcat_{s}"),
            name: s.into(),
            platform: "windows".into(),
            availability: "available".into(),
        })
        .collect()
}
async fn setup() -> (Fixture, Arc<FakeHost>) {
    let f = Fixture::new(GRANTS).await;
    let host = f.with_host();
    host.port_support.store(true, Ordering::SeqCst);
    *host.port_records.lock().unwrap() = catalog();
    (f, host)
}
async fn install(
    f: &Fixture,
    key: &str,
    port: &str,
    handles: Vec<String>,
    revision: u64,
    game: &str,
) -> Result<JobReply, Status> {
    f.client()
        .ports_request_install(PortInstallRequest {
            mutation: Some(f.mutation(key, revision)),
            port_id: port.into(),
            game_id: game.into(),
            supplied_file_handles: handles,
            user_intent_id: String::new(),
        })
        .await
        .map(Response::into_inner)
}
async fn reviewed(host: &FakeHost) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while host.port_reviews.lock().unwrap().is_empty() {
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
async fn catalog_and_details_are_bounded_read_only_and_reject_stale_cursors_and_urls() {
    let (f, host) = setup().await;
    let list = |cursor: String| PortsListRequest {
        page: Some(PageRequest {
            context: Some(f.context()),
            cursor,
            page_size: 1,
            ..Default::default()
        }),
        platform: "windows".into(),
        query: String::new(),
    };
    let first = f
        .client()
        .ports_list(list(String::new()))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(first.ports[0].port_id, "qcat_alpha");
    assert!(!first.next_cursor.is_empty());
    let next = f
        .client()
        .ports_list(list(first.next_cursor.clone()))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(next.ports[0].port_id, "qcat_beta");
    let detail = f
        .client()
        .ports_detail(PortDetailRequest {
            context: Some(f.context()),
            port_id: "qcat_alpha".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(detail.port.unwrap(), first.ports[0]);
    for id in ["https://bad.invalid/install.exe", "../file"] {
        assert_eq!(
            f.client()
                .ports_detail(PortDetailRequest {
                    context: Some(f.context()),
                    port_id: id.into()
                })
                .await
                .unwrap_err()
                .code(),
            Code::InvalidArgument
        );
    }
    host.port_records.lock().unwrap()[0].availability = "blocked".into();
    assert_eq!(
        f.client()
            .ports_list(list(first.next_cursor))
            .await
            .unwrap_err()
            .code(),
        Code::FailedPrecondition
    );
    let mut linux = list(String::new());
    linux.platform = "linux".into();
    assert!(
        f.client()
            .ports_list(linux)
            .await
            .unwrap()
            .into_inner()
            .ports
            .is_empty()
    );
    f.api
        .revoke_session_permission("fixture", cap::PORTS_READ)
        .await;
    assert_eq!(
        f.client()
            .ports_list(list(String::new()))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
}
#[tokio::test]
async fn installation_waits_for_core_and_retries_reuse_the_job() {
    let (f, host) = setup().await;
    let job = install(&f, "one", "qcat_alpha", vec![], 0, "")
        .await
        .unwrap();
    reviewed(&host).await;
    assert_eq!(host.port_effects.load(Ordering::SeqCst), 0);
    assert_eq!(
        install(&f, "one", "qcat_alpha", vec![], 0, "")
            .await
            .unwrap()
            .job_id,
        job.job_id
    );
    assert_eq!(host.port_reviews.lock().unwrap().len(), 1);
    host.port_gate.notify_one();
    assert_eq!(ended(&f, &job.job_id).await.state(), JobState::Succeeded);
    assert_eq!(host.port_effects.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn linux_arbitrary_installers_and_unapproved_handles_never_open_setup() {
    let (f, host) = setup().await;
    host.port_support.store(false, Ordering::SeqCst);
    assert_eq!(
        install(&f, "linux", "qcat_alpha", vec![], 0, "")
            .await
            .unwrap_err()
            .code(),
        Code::Unavailable
    );
    host.port_support.store(true, Ordering::SeqCst);
    assert_eq!(
        install(&f, "url", "https://bad.invalid/file.exe", vec![], 0, "")
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    assert_eq!(
        install(
            &f,
            "files",
            "qcat_alpha",
            vec!["path/or/handle".into()],
            0,
            ""
        )
        .await
        .unwrap_err()
        .code(),
        Code::PermissionDenied
    );
    assert!(host.port_reviews.lock().unwrap().is_empty());
}
#[tokio::test]
async fn declined_revoked_and_cancelled_requests_have_no_installation_effects() {
    for mode in ["decline", "revoke", "cancel"] {
        let (f, host) = setup().await;
        let job = install(&f, mode, "qcat_alpha", vec![], 0, "")
            .await
            .unwrap();
        reviewed(&host).await;
        match mode {
            "decline" => host.port_declined.store(true, Ordering::SeqCst),
            "revoke" => {
                f.api
                    .revoke_session_permission("fixture", cap::PORTS_INSTALL)
                    .await
            }
            "cancel" => {
                let caller = f
                    .api
                    .authorize_resource_session(Some(&f.context()))
                    .unwrap();
                let owner = JobOwner::of(&caller).unwrap();
                let jobs = f.jobs.clone();
                let id = job.job_id.clone();
                let cancellation = tokio::spawn(async move { jobs.cancel(&owner, &id).await });
                tokio::time::sleep(Duration::from_millis(20)).await;
                host.port_gate.notify_one();
                cancellation.await.unwrap().unwrap();
            }
            _ => unreachable!(),
        }
        if mode != "cancel" {
            host.port_gate.notify_one();
        }
        assert_ne!(ended(&f, &job.job_id).await.state(), JobState::Succeeded);
        assert_eq!(host.port_effects.load(Ordering::SeqCst), 0);
    }
}
#[tokio::test]
async fn supplied_handles_stay_scoped_and_replaced_source_folders_are_rejected_after_review() {
    let grants = [cap::PORTS_INSTALL, cap::FILES_READ];
    let (f, folder) = Fixture::emulator(&grants).await;
    let host = f.with_host();
    host.port_support.store(true, Ordering::SeqCst);
    *host.port_records.lock().unwrap() = catalog();
    let handle = f.approve("id-fixture:1", "save", &folder).await;
    let revision = f.feed.revision_of("fixture:1").unwrap();
    assert_eq!(
        install(
            &f,
            "tamper",
            "qcat_alpha",
            vec!["edited".into()],
            revision,
            "3"
        )
        .await
        .unwrap_err()
        .code(),
        Code::NotFound
    );
    let job = install(&f, "source", "qcat_alpha", vec![handle], revision, "3")
        .await
        .unwrap();
    reviewed(&host).await;
    let review = host.port_reviews.lock().unwrap()[0].clone();
    assert_eq!(review.sources.len(), 1);
    assert_eq!(review.sources[0].path, folder.to_string_lossy());
    assert!(review.sources[0].directory);
    std::fs::rename(&folder, folder.with_extension("old")).unwrap();
    std::fs::create_dir(&folder).unwrap();
    host.port_gate.notify_one();
    assert_eq!(ended(&f, &job.job_id).await.state(), JobState::Failed);
    assert_eq!(host.port_effects.load(Ordering::SeqCst), 0);
}
