//! Backups over the real local transport, with a deterministic worker engine (no live saves).
use super::saves::{INSTALLATION, directory, profile};
use super::*;
use crate::plugin_host::intents::{Answer, ConfirmationRequest, Prompter};
use crate::plugin_host::jobs::JobOwner;
use gamenode_core::drm_free as d;
use std::path::PathBuf;
use tokio::sync::Notify;

use super::action_recipes;

const GRANTS: &[&str] = &[
    cap::GAME_LAUNCH,
    cap::FILES_READ,
    cap::GAME_SAVES_CREATE,
    cap::GAME_SAVES_READ,
    cap::GAME_SAVES_UPDATE,
    cap::BACKUPS_READ,
    cap::BACKUPS_CREATE,
    cap::BACKUPS_RESTORE,
];

#[derive(Default)]
struct Backend {
    snapshots: Mutex<BTreeMap<String, BTreeMap<String, Vec<u8>>>>,
    requests: Mutex<Vec<d::ScopedBackupRequest>>,
    recovery: Mutex<Vec<(String, Vec<u8>)>>,
    holding: AtomicBool,
    entered: AtomicBool,
}
impl d::BackupEngine for Backend {
    fn execute(
        &self,
        _: &d::BackupRequest,
        _: &AtomicBool,
    ) -> Result<d::BackupReport, d::LocalError> {
        panic!("plugins must not use the manual backup port");
    }
    fn scoped_available(&self) -> bool {
        true
    }
    fn execute_scoped(
        &self,
        r: &d::ScopedBackupRequest,
        cancel: &AtomicBool,
        authority: &dyn d::BackupAuthority,
    ) -> Result<d::BackupReport, d::LocalError> {
        if cancel.load(Ordering::SeqCst) {
            return Err(d::LocalError::Cancelled);
        }
        self.requests.lock().unwrap().push(r.clone());
        self.entered.store(true, Ordering::SeqCst);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while self.holding.load(Ordering::SeqCst) {
            authority.check()?;
            if cancel.load(Ordering::SeqCst) {
                return Err(d::LocalError::Cancelled);
            }
            assert!(
                std::time::Instant::now() < deadline,
                "worker was never released"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        authority.check()?;
        let mut snapshots = self.snapshots.lock().unwrap();
        let json = match &r.operation {
            d::BackupOperation::Backup => {
                authority.validate_targets(&r.profile.files, &[])?;
                let files = r
                    .profile
                    .files
                    .iter()
                    .map(|p| (p.clone(), std::fs::read(p).unwrap()))
                    .collect();
                let name = format!("snapshot-{}", snapshots.len() + 1);
                snapshots.insert(name, files);
                serde_json::json!({"games":{&r.profile.title:{"files":{}}}})
            }
            d::BackupOperation::History => {
                serde_json::json!({"games":{&r.profile.title:{"backups":snapshots.keys().map(|name| serde_json::json!({"name":name,"created_at":1700000000,"size_bytes":4})).collect::<Vec<_>>()}}})
            }
            d::BackupOperation::RestorePreview { backup }
            | d::BackupOperation::Restore { backup } => {
                let files = snapshots.get(backup).ok_or(d::LocalError::InvalidTarget)?;
                let targets = files.keys().cloned().collect::<Vec<_>>();
                if matches!(r.operation, d::BackupOperation::Restore { .. }) {
                    authority.validate_targets(&targets, &[])?;
                    for (path, bytes) in files {
                        self.recovery
                            .lock()
                            .unwrap()
                            .push((path.clone(), std::fs::read(path).unwrap_or_default()));
                        authority.check()?;
                        std::fs::write(path, bytes).unwrap();
                    }
                }
                serde_json::json!({"games":{&r.profile.title:{"files":files.keys().map(|p| (p.clone(),serde_json::json!({}))).collect::<serde_json::Map<_,_>>()}}})
            }
            _ => return Err(d::LocalError::Unsupported),
        };
        Ok(d::BackupReport {
            state: d::JobState::Successful,
            json: json.to_string(),
        })
    }
}

struct Dialog {
    shown: Mutex<Vec<ConfirmationRequest>>,
    gate: Option<Arc<Notify>>,
    approved: bool,
}
#[tonic::async_trait]
impl Prompter for Dialog {
    async fn confirm(&self, request: ConfirmationRequest) -> Result<Answer, Status> {
        self.shown.lock().unwrap().push(request);
        if let Some(gate) = &self.gate {
            gate.notified().await;
        }
        Ok(Answer {
            approved: self.approved,
            remember: true,
        })
    }
}

struct Ready {
    f: Fixture,
    folder: PathBuf,
    profile: SaveProfileMutationReply,
    backend: Arc<Backend>,
}
impl Ready {
    async fn new() -> Self {
        let (f, folder) = Fixture::emulator(GRANTS).await;
        let host = f.with_host();
        let backend = Arc::new(Backend::default());
        *host.backup.lock().unwrap() = Some(crate::plugin_host::library::BackupHostContext {
            engine: backend.clone(),
            destination: f
                ._temp
                .path()
                .join("archives")
                .to_string_lossy()
                .into_owned(),
            target: d::BackupTarget {
                installation_id: INSTALLATION.into(),
                root: f._temp.path().join("rom").to_string_lossy().into_owned(),
                gameplay: vec![],
                track_folder: true,
            },
        });
        let handle = f.approve(INSTALLATION, "save", &folder).await;
        let profile = f
            .make(
                "profile",
                profile(
                    "Saves",
                    "",
                    vec![directory(&handle, "Slots", &["**/*.sav"])],
                ),
            )
            .await
            .unwrap();
        Self {
            f,
            folder,
            profile,
            backend,
        }
    }
    async fn create(&self, key: &str) -> Result<JobReply, Status> {
        self.f
            .client()
            .backups_request_create(BackupCreateRequest {
                mutation: Some(self.f.mutation(key, self.profile.revision)),
                game_id: "3".into(),
                profile_id: self.profile.profile_id.clone(),
                reason: "User requested".into(),
            })
            .await
            .map(Response::into_inner)
    }
    async fn list(&self, cursor: &str, size: u32) -> Result<BackupsListReply, Status> {
        self.f
            .client()
            .backups_list(BackupsListRequest {
                page: Some(PageRequest {
                    context: Some(self.f.context()),
                    page_size: size,
                    cursor: cursor.into(),
                    ..Default::default()
                }),
                game_id: "3".into(),
                profile_id: self.profile.profile_id.clone(),
            })
            .await
            .map(Response::into_inner)
    }
    async fn restore(&self, key: &str, id: &str) -> Result<JobReply, Status> {
        self.f
            .client()
            .backups_request_restore(BackupRestoreRequest {
                mutation: Some(self.f.mutation(key, self.profile.revision)),
                backup_id: id.into(),
                game_id: "3".into(),
                profile_id: self.profile.profile_id.clone(),
                user_intent_id: String::new(),
            })
            .await
            .map(Response::into_inner)
    }
    fn dialog(&self, approved: bool, gate: Option<Arc<Notify>>) -> Arc<Dialog> {
        let dialog = Arc::new(Dialog {
            shown: Mutex::new(vec![]),
            approved,
            gate,
        });
        self.f.intents.set_prompter(dialog.clone());
        dialog
    }
    async fn ended(&self, id: &str) -> JobReply {
        let caller = self
            .f
            .api
            .authorize_resource_session(Some(&self.f.context()))
            .unwrap();
        let owner = JobOwner::of(&caller).unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let job = self.f.jobs.get(&owner, id).unwrap();
                if !matches!(job.state(), JobState::Queued | JobState::Running) {
                    return job;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("backup did not end")
    }
    async fn created(&self, key: &str) -> JobReply {
        let job = self.create(key).await.unwrap();
        let finished = self.ended(&job.job_id).await;
        assert_eq!(finished.state(), JobState::Succeeded, "{finished:?}");
        job
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

#[tokio::test(flavor = "multi_thread")]
async fn sdk_backup_calls_match_the_raw_calls_and_follow_the_job() {
    let r = Ready::new().await;
    let actions = r.f.host.actions().expect("an API 1.6 session");
    let created = actions
        .create_backup(
            "3",
            &r.profile.profile_id,
            "User requested",
            r.profile.revision,
            &gamenode_plugin_sdk::idempotency_key(),
        )
        .await
        .unwrap();
    assert_eq!(
        action_recipes::wait_for_job(&actions, &created.job_id)
            .await
            .unwrap()
            .state,
        JobState::Succeeded
    );
    let listed = r.list("", 50).await.unwrap();
    let through_sdk = actions.backups("3", &r.profile.profile_id).await.unwrap();
    assert_eq!(through_sdk.len(), listed.backups.len());
    assert_eq!(
        (
            &through_sdk[0].game_id,
            through_sdk[0].created_at,
            through_sdk[0].size_bytes,
            &through_sdk[0].state,
        ),
        (
            &listed.backups[0].game_id,
            listed.backups[0].created_at,
            listed.backups[0].size_bytes,
            &listed.backups[0].state,
        )
    );
    let dialog = r.dialog(true, None);
    let restored = actions
        .restore_backup(
            &listed.backups[0].backup_id,
            "3",
            &r.profile.profile_id,
            r.profile.revision,
            &gamenode_plugin_sdk::idempotency_key(),
        )
        .await
        .unwrap();
    assert_eq!(
        action_recipes::wait_for_job(&actions, &restored.job_id)
            .await
            .unwrap()
            .state,
        JobState::Succeeded
    );
    assert_eq!(dialog.shown.lock().unwrap().len(), 1);
    assert_eq!(
        std::fs::read(r.folder.join("states/s1.sav")).unwrap(),
        b"state"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn create_selects_only_approved_rules_is_replayable_and_restores_only_after_confirmation() {
    let r = Ready::new().await;
    let created = r.created("create").await;
    assert_eq!(r.create("create").await.unwrap().job_id, created.job_id);
    assert_eq!(r.backend.snapshots.lock().unwrap().len(), 1);
    let request = r.backend.requests.lock().unwrap()[0].clone();
    assert!(!request.owner.is_empty());
    assert_eq!(
        request.profile.files,
        vec![r.folder.join("states/s1.sav").to_string_lossy()]
    );
    let listed = r.list("", 50).await.unwrap();
    assert_eq!(listed.backups.len(), 1);
    assert!(
        !listed.backups[0]
            .backup_id
            .contains(&*r.folder.to_string_lossy())
    );
    std::fs::write(r.folder.join("states/s1.sav"), b"newer").unwrap();
    let gate = Arc::new(Notify::new());
    let dialog = r.dialog(true, Some(gate.clone()));
    let job = r
        .restore("restore", &listed.backups[0].backup_id)
        .await
        .unwrap();
    shown(&dialog).await;
    assert_eq!(
        std::fs::read(r.folder.join("states/s1.sav")).unwrap(),
        b"newer"
    );
    assert!(
        dialog.shown.lock().unwrap()[0]
            .details
            .iter()
            .any(|p| p.ends_with("s1.sav"))
    );
    gate.notify_one();
    assert_eq!(r.ended(&job.job_id).await.state(), JobState::Succeeded);
    assert_eq!(
        std::fs::read(r.folder.join("states/s1.sav")).unwrap(),
        b"state"
    );
    assert_eq!(
        std::fs::read(r.folder.join("GC/USA/card.gci")).unwrap(),
        b"card"
    );
    assert_eq!(r.backend.recovery.lock().unwrap()[0].1, b"newer");
}

#[tokio::test(flavor = "multi_thread")]
async fn history_cursors_survive_new_tokens_but_not_a_changed_history_and_foreign_tokens_fail() {
    let r = Ready::new().await;
    r.created("one").await;
    r.created("two").await;
    let first = r.list("", 1).await.unwrap();
    let second = r.list(&first.next_cursor, 1).await.unwrap();
    assert_eq!(second.backups.len(), 1);
    assert!(second.next_cursor.is_empty());
    r.created("three").await;
    assert_eq!(
        r.list(&first.next_cursor, 1).await.unwrap_err().code(),
        Code::FailedPrecondition
    );
    let other = Ready::new().await;
    other.dialog(true, None);
    assert_eq!(
        other
            .restore("foreign", &first.backups[0].backup_id)
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn declined_cancelled_and_revoked_restores_write_nothing() {
    let r = Ready::new().await;
    r.created("one").await;
    let id = r.list("", 50).await.unwrap().backups[0].backup_id.clone();
    let dialog = r.dialog(false, None);
    let job = r.restore("decline", &id).await.unwrap();
    assert_eq!(r.ended(&job.job_id).await.state(), JobState::Failed);
    shown(&dialog).await;
    let gate = Arc::new(Notify::new());
    let dialog = r.dialog(true, Some(gate.clone()));
    let job = r.restore("cancel", &id).await.unwrap();
    shown(&dialog).await;
    r.f.client()
        .jobs_cancel(JobCancelRequest {
            mutation: Some(r.f.mutation("cancel-job", 0)),
            job_id: job.job_id.clone(),
        })
        .await
        .unwrap();
    assert_eq!(r.ended(&job.job_id).await.state(), JobState::Cancelled);
    let gate = Arc::new(Notify::new());
    let dialog = r.dialog(true, Some(gate.clone()));
    let job = r.restore("revoke", &id).await.unwrap();
    shown(&dialog).await;
    r.f.api
        .revoke_session_permission("fixture", cap::BACKUPS_RESTORE)
        .await;
    gate.notify_one();
    assert_ne!(r.ended(&job.job_id).await.state(), JobState::Succeeded);
    assert!(r.backend.recovery.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn restore_rejects_archive_targets_outside_rules_before_showing_confirmation() {
    let r = Ready::new().await;
    r.created("one").await;
    r.backend
        .snapshots
        .lock()
        .unwrap()
        .get_mut("snapshot-1")
        .unwrap()
        .insert(
            r.folder
                .join("GC/USA/card.gci")
                .to_string_lossy()
                .into_owned(),
            b"attack".to_vec(),
        );
    let dialog = r.dialog(true, None);
    let id = r.list("", 50).await.unwrap().backups[0].backup_id.clone();
    let job = r.restore("restore", &id).await.unwrap();
    assert_eq!(r.ended(&job.job_id).await.state(), JobState::Failed);
    assert!(dialog.shown.lock().unwrap().is_empty());
    assert_eq!(
        std::fs::read(r.folder.join("GC/USA/card.gci")).unwrap(),
        b"card"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn scope_revision_and_replaced_folders_fail_before_any_backup_and_unavailable_engine_is_explicit()
 {
    let mut r = Ready::new().await;
    r.profile.revision += 1;
    assert_eq!(r.create("stale").await.unwrap_err().code(), Code::Aborted);
    r.profile.revision -= 1;
    let original = r.folder.with_extension("away");
    std::fs::rename(&r.folder, &original).unwrap();
    std::fs::create_dir(&r.folder).unwrap();
    assert!(r.create("replaced").await.is_err());
    assert!(r.backend.requests.lock().unwrap().is_empty());
    let r = Ready::new().await;
    let host = r.f.with_host(); // No engine installed on this host.
    assert_eq!(
        r.create("no-engine").await.unwrap_err().code(),
        Code::Unavailable
    );
    assert!(host.backup.lock().unwrap().is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_extension_backs_up_a_selected_store_game_without_manual_ownership_or_broad_reads() {
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
    let install = f.game_dir("store-install", &[]);
    let folder = f.game_dir("store-saves", &[("slot.sav", b"save")]);
    f.library_in(&[
        ("steam:1", Some(5), "Selected store game", &install),
        ("steam:2", Some(6), "Other game", &install),
    ]);
    let host = f.with_host();
    let backend = Arc::new(Backend::default());
    *host.backup.lock().unwrap() = Some(crate::plugin_host::library::BackupHostContext {
        engine: backend.clone(),
        destination: f
            ._temp
            .path()
            .join("archives")
            .to_string_lossy()
            .into_owned(),
        target: d::BackupTarget {
            installation_id: String::new(),
            root: install.to_string_lossy().into_owned(),
            gameplay: vec![],
            track_folder: true,
        },
    });
    let handle = f.approve("id-steam:1", "save", &folder).await;
    assert!(
        f.request_access("id-steam:2", &["read"], &["save"])
            .await
            .is_err()
    );
    let mut proposed = profile(
        "Store saves",
        "",
        vec![directory(&handle, "Slot", &["*.sav"])],
    );
    proposed.installation_id = "id-steam:1".into();
    let profile = f.make("profile", proposed).await.unwrap();
    let job = f
        .client()
        .backups_request_create(BackupCreateRequest {
            mutation: Some(f.mutation("create", profile.revision)),
            game_id: "5".into(),
            profile_id: profile.profile_id,
            reason: String::new(),
        })
        .await
        .unwrap()
        .into_inner();
    let r = Ready {
        f,
        folder,
        profile: SaveProfileMutationReply::default(),
        backend,
    };
    assert_eq!(r.ended(&job.job_id).await.state(), JobState::Succeeded);
    let requests = r.backend.requests.lock().unwrap();
    assert_eq!(requests[0].profile.installation, "id-steam:1");
    assert_eq!(r.f.feed.folders().len(), 2);
    assert!(
        r.f.feed
            .folders()
            .iter()
            .all(|entry| entry.entry_key.starts_with("steam:"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_running_worker_and_revising_a_confirmed_profile_prevent_effects() {
    let r = Ready::new().await;
    r.backend.holding.store(true, Ordering::SeqCst);
    let job = r.create("cancel-running").await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !r.backend.entered.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    r.f.client()
        .jobs_cancel(JobCancelRequest {
            mutation: Some(r.f.mutation("cancel", 0)),
            job_id: job.job_id.clone(),
        })
        .await
        .unwrap();
    r.backend.holding.store(false, Ordering::SeqCst);
    assert_eq!(r.ended(&job.job_id).await.state(), JobState::Cancelled);
    assert!(r.backend.snapshots.lock().unwrap().is_empty());
    r.created("one").await;
    let id = r.list("", 50).await.unwrap().backups[0].backup_id.clone();
    let gate = Arc::new(Notify::new());
    let dialog = r.dialog(true, Some(gate.clone()));
    let job = r.restore("restore", &id).await.unwrap();
    shown(&dialog).await;
    r.f.patch(
        "rename",
        &r.profile.profile_id,
        r.profile.revision,
        &["name"],
        SaveProfile {
            name: "Changed".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    gate.notify_one();
    assert_eq!(r.ended(&job.job_id).await.state(), JobState::Failed);
    assert!(r.backend.recovery.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_backup_destination_inside_the_live_saves_is_refused_before_a_job_or_engine_call() {
    let r = Ready::new().await;
    let mut context =
        r.f.feed
            .host()
            .unwrap()
            .backup_context("fixture:1".into())
            .await
            .unwrap();
    context.destination = r.folder.join("archives").to_string_lossy().into_owned();
    *r.f.with_host().backup.lock().unwrap() = Some(context);
    assert_eq!(
        r.create("overlap").await.unwrap_err().code(),
        Code::FailedPrecondition
    );
    assert!(r.backend.requests.lock().unwrap().is_empty());
    assert!(!r.folder.join("archives").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn changing_a_snapshot_after_its_preview_cannot_add_unreviewed_restore_targets() {
    let r = Ready::new().await;
    r.created("one").await;
    let id = r.list("", 50).await.unwrap().backups[0].backup_id.clone();
    let gate = Arc::new(Notify::new());
    let dialog = r.dialog(true, Some(gate.clone()));
    let job = r.restore("restore", &id).await.unwrap();
    shown(&dialog).await;
    let extra = r.folder.join("states/unreviewed.sav");
    r.backend
        .snapshots
        .lock()
        .unwrap()
        .get_mut("snapshot-1")
        .unwrap()
        .insert(
            extra.to_string_lossy().into_owned(),
            b"not approved".to_vec(),
        );
    gate.notify_one();
    assert_eq!(r.ended(&job.job_id).await.state(), JobState::Failed);
    assert!(r.backend.recovery.lock().unwrap().is_empty());
    assert!(!extra.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn runtime_profiles_fail_closed_without_external_process_inspection() {
    let mut r = Ready::new().await;
    r.profile =
        r.f.patch(
            "runtime",
            &r.profile.profile_id,
            r.profile.revision,
            &["runtime_id"],
            SaveProfile {
                runtime_id: "unresolved-runtime".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        r.create("unsafe-runtime").await.unwrap_err().code(),
        Code::Unavailable
    );
    assert!(r.backend.requests.lock().unwrap().is_empty());
    assert!(r.f.profile(&r.profile.profile_id).await.is_ok());
}
