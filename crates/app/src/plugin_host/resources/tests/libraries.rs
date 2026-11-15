//! Logical libraries over the real local socket, with scripted folder consent and Core dispatch.
use super::*;

const GRANTS: &[&str] = &[
    cap::LIBRARY_ROOTS_READ,
    cap::LIBRARY_ROOTS_CREATE,
    cap::LIBRARY_ROOTS_UPDATE,
    cap::LIBRARY_ROOTS_DELETE,
];
impl Fixture {
    async fn make_library(&self, key: &str) -> LibraryMutationReply {
        self.client()
            .libraries_create(LibraryCreateRequest {
                mutation: Some(self.mutation(key, 0)),
                name: "Games".into(),
                description: "Local library".into(),
            })
            .await
            .unwrap()
            .into_inner()
    }
    async fn logical_library(&self, id: &str) -> Result<LibraryRecord, Status> {
        self.client()
            .libraries_get(DomainGetRequest {
                context: Some(self.context()),
                resource_id: id.into(),
                ..Default::default()
            })
            .await
            .map(|r| r.into_inner().library.unwrap())
    }
    async fn add_library_root(
        &self,
        key: &str,
        library: &LibraryMutationReply,
        display: &str,
        suggestions: Vec<DeclaredLocation>,
    ) -> Result<LibraryMutationReply, Status> {
        self.client()
            .libraries_add_root(LibraryRootAddRequest {
                mutation: Some(self.mutation(key, library.revision)),
                library_id: library.resource_id.clone(),
                display_name: display.into(),
                declared_locations: suggestions,
                ..Default::default()
            })
            .await
            .map(Response::into_inner)
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn library_crud_is_revision_checked_replayable_private_and_durable() {
    let f = Fixture::new(GRANTS).await;
    let created = f.make_library("create").await;
    assert_eq!(f.make_library("create").await, created);
    let edited = f
        .client()
        .libraries_update(LibraryUpdateRequest {
            mutation: Some(f.mutation("rename", created.revision)),
            library_id: created.resource_id.clone(),
            name: "Renamed".into(),
            field_mask: vec!["name".into()],
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert!(edited.revision > created.revision);
    let conflict = f
        .client()
        .libraries_update(LibraryUpdateRequest {
            mutation: Some(f.mutation("stale", created.revision)),
            library_id: created.resource_id.clone(),
            name: "Lost".into(),
            field_mask: vec!["name".into()],
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(conflict.outcome, MutationOutcome::RevisionConflict as i32);
    assert_eq!(
        f.logical_library(&created.resource_id).await.unwrap().name,
        "Renamed"
    );
    assert_eq!(
        f.logical_library("library-foreign")
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    let restarted = Fixture::restarted(GRANTS, f.plugin_data.clone()).await;
    assert_eq!(
        restarted
            .logical_library(&created.resource_id)
            .await
            .unwrap()
            .name,
        "Renamed"
    );
    f.client()
        .libraries_remove(DomainRemoveRequest {
            mutation: Some(f.mutation("remove", edited.revision)),
            resource_id: created.resource_id.clone(),
        })
        .await
        .unwrap();
    assert_eq!(
        f.logical_library(&created.resource_id)
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn roots_need_consent_without_an_installation_and_disclose_both_scanners() {
    let f = Fixture::new(GRANTS).await;
    let host = f.with_host();
    let library = f.make_library("create").await;
    let folder = f.game_dir("unregistered", &[("game.exe", b"fictional")]);
    let approver = f.user_approving(&folder);
    let added = f
        .add_library_root(
            "root",
            &library,
            "Chosen root",
            vec![DeclaredLocation {
                kind: "library".into(),
                location: folder.to_str().unwrap().into(),
                description: "Suggested, unverified".into(),
            }],
        )
        .await
        .unwrap();
    let row = f.logical_library(&library.resource_id).await.unwrap();
    assert_eq!(row.roots.len(), 1);
    assert_eq!(row.roots[0].id, added.resource_id);
    assert_eq!(
        row.roots[0].availability,
        InspectionAvailability::Available as i32
    );
    let confirmed = approver.confirmed.lock().unwrap();
    assert_eq!(confirmed[0].kind, "library");
    assert!(confirmed[0].declared_by_plugin);
    assert_eq!(
        confirmed[0].operations,
        vec!["read", "plugin_discovery", "core_executable_discovery"]
    );
    drop(confirmed);
    let scans = host.scan_roots.lock().unwrap();
    let roots = scans.values().flatten().collect::<Vec<_>>();
    assert_eq!(roots.len(), 1);
    assert!((roots[0].valid)().is_ok());
    assert!(roots[0].root.declared);
    assert_eq!(
        std::fs::read(folder.join("game.exe")).unwrap(),
        b"fictional"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn declined_or_invalid_suggestions_never_register_or_dispatch_a_root() {
    let f = Fixture::new(GRANTS).await;
    let host = f.with_host();
    let library = f.make_library("create").await;
    let folder = f.game_dir("suggested", &[]);
    let approver = f.user_approving(&folder);
    approver.consent.store(false, Ordering::SeqCst);
    assert_eq!(
        f.add_library_root(
            "declined",
            &library,
            "Root",
            vec![DeclaredLocation {
                kind: "library".into(),
                location: folder.to_str().unwrap().into(),
                description: "Unverified".into()
            }]
        )
        .await
        .unwrap_err()
        .code(),
        Code::PermissionDenied
    );
    assert!(
        f.logical_library(&library.resource_id)
            .await
            .unwrap()
            .roots
            .is_empty()
    );
    assert!(
        host.scan_roots
            .lock()
            .unwrap()
            .values()
            .all(|r| r.is_empty())
    );
    assert_eq!(
        f.add_library_root(
            "invalid",
            &library,
            "Root",
            vec![DeclaredLocation {
                kind: "library".into(),
                location: "../escape".into(),
                description: String::new()
            }]
        )
        .await
        .unwrap_err()
        .code(),
        Code::InvalidArgument
    );
    assert_eq!(approver.asked.lock().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn unavailable_roots_do_not_prevent_healthy_additions_and_removing_roots_deletes_no_files() {
    let f = Fixture::new(GRANTS).await;
    let host = f.with_host();
    let library = f.make_library("create").await;
    let folder = f.game_dir("one", &[("save", b"keep")]);
    f.user_approving(&folder);
    let first = f
        .add_library_root("first", &library, "First", vec![])
        .await
        .unwrap();
    let previous = host
        .scan_roots
        .lock()
        .unwrap()
        .values()
        .flatten()
        .next()
        .unwrap()
        .clone();
    let away = folder.with_extension("away");
    std::fs::rename(&folder, &away).unwrap();
    let second_folder = f.game_dir("two", &[]);
    f.user_approving(&second_folder);
    let revision = LibraryMutationReply {
        resource_id: library.resource_id.clone(),
        revision: first.revision,
        outcome: first.outcome,
    };
    let second = f
        .add_library_root("second", &revision, "Second", vec![])
        .await
        .unwrap();
    assert!(
        (previous.valid)().is_err(),
        "a changed contribution fences the old scan"
    );
    let row = f.logical_library(&library.resource_id).await.unwrap();
    assert_eq!(
        row.roots[0].availability,
        InspectionAvailability::Disconnected as i32
    );
    assert_eq!(
        row.roots[1].availability,
        InspectionAvailability::Available as i32
    );
    f.client()
        .libraries_remove_root(LibraryRootRemoveRequest {
            mutation: Some(f.mutation("remove-root", second.revision)),
            library_id: library.resource_id.clone(),
            root_id: first.resource_id,
        })
        .await
        .unwrap();
    let scans = host.scan_roots.lock().unwrap();
    let roots = scans.values().flatten().collect::<Vec<_>>();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].root.path, second_folder.to_str().unwrap());
    drop(scans);
    assert_eq!(std::fs::read(away.join("save")).unwrap(), b"keep");
    assert!(!folder.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn crud_revocation_ends_api_delivery_but_only_session_loss_fences_approved_core_work() {
    let f = Fixture::new(GRANTS).await;
    let host = f.with_host();
    let mut watch = f
        .client()
        .libraries_watch(DomainWatchRequest {
            context: Some(f.context()),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    let library = f.make_library("create").await;
    let event = tokio::time::timeout(Duration::from_secs(5), watch.message())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(event.library.unwrap().id, library.resource_id);
    let folder = f.game_dir("root", &[]);
    f.user_approving(&folder);
    f.add_library_root("root", &library, "Root", vec![])
        .await
        .unwrap();
    let scan = host
        .scan_roots
        .lock()
        .unwrap()
        .values()
        .flatten()
        .next()
        .unwrap()
        .clone();
    f.api
        .revoke_session_permission("fixture", cap::LIBRARY_ROOTS_CREATE)
        .await;
    assert!(
        (scan.valid)().is_ok(),
        "standing root approval is independent of create permission"
    );
    assert!(
        f.logical_library(&library.resource_id).await.is_ok(),
        "read remains independently granted"
    );
    f.api
        .revoke_session_permission("fixture", cap::LIBRARY_ROOTS_READ)
        .await;
    assert_eq!(
        f.logical_library(&library.resource_id)
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    // A change already delivered before revocation can precede the terminal refusal.
    let denied = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match watch.message().await {
                Err(e) => break e,
                Ok(Some(_)) => {}
                Ok(None) => panic!("watch ended without denial"),
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(denied.code(), Code::PermissionDenied);
    assert!((scan.valid)().is_ok());
    f.api.disconnect("fixture").await;
    assert!(
        (scan.valid)().is_err(),
        "a disconnected plugin cannot continue Core work"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn library_operations_have_independent_grants_and_unknown_ids_reveal_nothing() {
    let f = Fixture::new(&[cap::LIBRARY_ROOTS_CREATE]).await;
    let library = f.make_library("create").await;
    assert_eq!(
        f.logical_library(&library.resource_id)
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    assert_eq!(
        f.client()
            .libraries_remove(DomainRemoveRequest {
                mutation: Some(f.mutation("remove", library.revision)),
                resource_id: library.resource_id
            })
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn library_watch_reports_volume_availability_without_a_configuration_edit() {
    let f = Fixture::new(GRANTS).await;
    f.with_host();
    let library = f.make_library("create").await;
    let folder = f.game_dir("watched", &[]);
    f.user_approving(&folder);
    let added = f
        .add_library_root("root", &library, "Root", vec![])
        .await
        .unwrap();
    let mut watch = f
        .client()
        .libraries_watch(DomainWatchRequest {
            context: Some(f.context()),
            after_revision: added.revision,
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    // Let the first metadata observation establish its baseline, then remove the path.
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let away = folder.with_extension("away");
    std::fs::rename(&folder, &away).unwrap();
    let event = tokio::time::timeout(Duration::from_secs(5), watch.message())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(event.event_type, "availability_changed");
    assert_eq!(
        event.library.unwrap().roots[0].availability,
        InspectionAvailability::Disconnected as i32
    );
    assert!(!folder.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn provider_free_plugins_cannot_replay_each_others_mutation_receipts() {
    let f = Fixture::new(GRANTS).await;
    let mut first = f
        .api
        .authorize_resource(Some(&f.context()), cap::LIBRARY_ROOTS_CREATE)
        .unwrap();
    first.provider_id.clear();
    let mut second = first.clone();
    let mut identity = first.namespace.as_ref().unwrap().identity().clone();
    identity.plugin_id = "org.example.other".into();
    second.namespace = Some(
        crate::plugin_host::namespace::ExternalNamespaceResolver::new(
            identity,
            None,
            second.profile_epoch,
        )
        .unwrap(),
    );
    let receipts = super::super::mutations::Mutations::default();
    let hash = [7u8; 32];
    receipts.remember(
        &first,
        "same-key",
        hash,
        &MutationReply {
            resource_id: "private-library".into(),
            revision: 1,
            outcome: MutationOutcome::Applied as i32,
        },
    );
    assert!(
        receipts
            .replay(&second, "same-key", hash)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        receipts
            .replay(&first, "same-key", hash)
            .unwrap()
            .unwrap()
            .resource_id,
        "private-library"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn approved_library_handles_allow_scoped_reads_only_and_replacement_fences_old_handles() {
    let mut grants = GRANTS.to_vec();
    grants.extend([cap::FILES_READ, cap::FILES_WRITE]);
    let f = Fixture::new(&grants).await;
    f.with_host();
    let library = f.make_library("create").await;
    let one = f.game_dir("source-one", &[("game.exe", b"first")]);
    f.user_approving(&one);
    let added = f
        .add_library_root("root", &library, "Source", vec![])
        .await
        .unwrap();
    let record = f.logical_library(&library.resource_id).await.unwrap();
    let handle = &record.roots[0].approved_root_handle;
    let file = f.child(handle, "game.exe").await;
    assert_eq!(f.cat(&file, 0, 20).await.unwrap().contents, b"first");
    assert_eq!(
        f.client()
            .files_write(FilesWriteRequest {
                mutation: Some(f.mutation("write", 0)),
                file_handle: file.clone(),
                contents: b"forbidden".to_vec(),
                ..Default::default()
            })
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    let two = f.game_dir("source-two", &[("game.exe", b"second")]);
    f.user_approving(&two);
    let updated = f
        .client()
        .libraries_update_root(LibraryRootUpdateRequest {
            mutation: Some(f.mutation("replace", added.revision)),
            library_id: library.resource_id.clone(),
            root_id: added.resource_id.clone(),
            field_mask: vec!["folder".into()],
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(f.ls(handle).await.unwrap_err().code(), Code::NotFound);
    assert_eq!(
        f.cat(&file, 0, 20).await.unwrap_err().code(),
        Code::NotFound
    );
    let replacement = f.logical_library(&library.resource_id).await.unwrap();
    assert_eq!(replacement.roots[0].id, added.resource_id);
    let current = &replacement.roots[0].approved_root_handle;
    assert_eq!(
        f.cat(&f.child(current, "game.exe").await, 0, 20)
            .await
            .unwrap()
            .contents,
        b"second"
    );
    f.client()
        .libraries_remove_root(LibraryRootRemoveRequest {
            mutation: Some(f.mutation("remove", updated.revision)),
            library_id: library.resource_id,
            root_id: added.resource_id,
        })
        .await
        .unwrap();
    assert_eq!(f.ls(current).await.unwrap_err().code(), Code::NotFound);
    assert_eq!(std::fs::read(one.join("game.exe")).unwrap(), b"first");
    assert_eq!(std::fs::read(two.join("game.exe")).unwrap(), b"second");
}

#[tokio::test(flavor = "multi_thread")]
async fn root_approval_resumes_on_session_start_without_any_crud_permission() {
    let f = Fixture::new(GRANTS).await;
    f.with_host();
    let library = f.make_library("create").await;
    let folder = f.game_dir("persistent-root", &[]);
    f.user_approving(&folder);
    f.add_library_root("root", &library, "Persistent", vec![])
        .await
        .unwrap();
    let reopened = Fixture::restarted(&[cap::NOTIFICATIONS_PUBLISH], f.plugin_data.clone()).await;
    let host = reopened.with_host();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let started = host
                .scan_roots
                .lock()
                .unwrap()
                .values()
                .flatten()
                .any(|r| r.root.path == folder.to_str().unwrap() && (r.valid)().is_ok());
            if started {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        reopened
            .logical_library(&library.resource_id)
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    reopened.api.disconnect("fixture").await;
    assert!(
        host.scan_roots
            .lock()
            .unwrap()
            .values()
            .flatten()
            .all(|r| (r.valid)().is_err())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn disabling_one_approved_root_stops_its_contribution_and_leaves_the_other_enabled() {
    let f = Fixture::new(GRANTS).await;
    let host = f.with_host();
    let library = f.make_library("create").await;
    let first = f.game_dir("first", &[]);
    f.user_approving(&first);
    let added = f
        .add_library_root("first", &library, "First", vec![])
        .await
        .unwrap();
    let second = f.game_dir("second", &[]);
    f.user_approving(&second);
    let parent = LibraryMutationReply {
        resource_id: library.resource_id.clone(),
        revision: added.revision,
        outcome: added.outcome,
    };
    let added_second = f
        .add_library_root("second", &parent, "Second", vec![])
        .await
        .unwrap();
    f.client()
        .libraries_update_root(LibraryRootUpdateRequest {
            mutation: Some(f.mutation("disable", added_second.revision)),
            library_id: library.resource_id,
            root_id: added.resource_id,
            watching: false,
            field_mask: vec!["watching".into()],
            ..Default::default()
        })
        .await
        .unwrap();
    let roots = host.scan_roots.lock().unwrap();
    let current = roots
        .values()
        .flatten()
        .filter(|r| (r.valid)().is_ok())
        .collect::<Vec<_>>();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].root.path, second.to_str().unwrap());
    assert!(first.exists());
    assert!(second.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn account_switch_fences_old_monitor_contexts_without_exposing_or_resuming_its_roots() {
    let mut f = Fixture::new(GRANTS).await;
    let host = f.with_host();
    let library = f.make_library("create").await;
    let folder = f.game_dir("account-a-root", &[]);
    f.user_approving(&folder);
    f.add_library_root("root", &library, "A", vec![])
        .await
        .unwrap();
    let old = host
        .scan_roots
        .lock()
        .unwrap()
        .values()
        .flatten()
        .next()
        .unwrap()
        .clone();
    let original_epoch = f.context().profile_epoch;
    f.api.profiles().update(Some("b"), true);
    assert!((old.valid)().is_err());
    assert!(
        f.api
            .resource_session_contexts()
            .iter()
            .all(|context| context.profile_epoch == original_epoch),
        "lifecycle contexts must retain the original epoch"
    );
    f.api.fence_third_party_sessions().await;
    f.reconnect().await;
    assert_eq!(
        f.logical_library(&library.resource_id)
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    assert!(
        host.scan_roots
            .lock()
            .unwrap()
            .values()
            .flatten()
            .all(|r| (r.valid)().is_err())
    );
    assert!(folder.exists());
}
