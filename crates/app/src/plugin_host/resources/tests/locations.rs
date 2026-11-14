//! The `Locations` service over the real local socket (task 13.3): the size, the access and the
//! modification times of approved places, read from metadata alone.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration as StdDuration, UNIX_EPOCH};

use super::saves::{INSTALLATION, directory, profile};
use super::*;

const INSPECT: &[&str] = &[
    cap::GAME_STORAGE_INSPECT,
    cap::GAME_SAVES_INSPECT,
    cap::GAME_SAVES_READ,
    cap::GAME_SAVES_CREATE,
    cap::FILES_READ,
];

fn running_as_root() -> bool {
    rustix::process::geteuid().is_root()
}

impl Fixture {
    fn locations(
        &self,
    ) -> gamenode_plugin_protocol::proto::locations_client::LocationsClient<Channel> {
        self.host.locations_client()
    }

    async fn size(&self, handle: &str, max_entries: u32) -> Result<LocationSizeReply, Status> {
        self.locations()
            .get_size(LocationSizeRequest {
                context: Some(self.context()),
                approved_resource_handle: handle.into(),
                max_entries,
            })
            .await
            .map(Response::into_inner)
    }

    async fn access(&self, handle: &str, entries: &[&str]) -> Result<LocationAccessReply, Status> {
        self.locations()
            .get_access(LocationAccessRequest {
                context: Some(self.context()),
                approved_resource_handle: handle.into(),
                entry_handles: entries.iter().map(|entry| (*entry).into()).collect(),
            })
            .await
            .map(Response::into_inner)
    }

    async fn modified(
        &self,
        handle: &str,
        max_entries: u32,
    ) -> Result<LocationLastModifiedReply, Status> {
        self.locations()
            .get_last_modified(LocationLastModifiedRequest {
                context: Some(self.context()),
                approved_resource_handle: handle.into(),
                max_entries,
            })
            .await
            .map(Response::into_inner)
    }
}

fn set_time(path: &Path, seconds: u64, nanos: u32) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(UNIX_EPOCH + StdDuration::new(seconds, nanos))
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_size_of_an_approved_folder_is_its_logical_bytes_and_counts() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let size = fixture.size(&root, 0).await.unwrap();
    assert_eq!(
        (
            size.logical_bytes,
            size.file_count,
            size.directory_count,
            size.complete
        ),
        (4 + 5, 2, 3, true) // card.gci and s1.sav; GC, USA and states
    );
    assert_eq!(size.availability, InspectionAvailability::Available as i32);
    // A folder below the approved one measures what is in it.
    let states = fixture.child(&root, "states").await;
    let below = fixture.size(&states, 0).await.unwrap();
    assert_eq!(
        (below.logical_bytes, below.file_count, below.directory_count),
        (5, 1, 0)
    );
    // A single file is its own size.
    let file = fixture.child(&states, "s1.sav").await;
    let one = fixture.size(&file, 0).await.unwrap();
    assert_eq!(
        (one.logical_bytes, one.file_count, one.directory_count),
        (5, 1, 0)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_permission_follows_the_kind_of_folder_and_needs_no_file_permission() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let install = fixture.approve(INSTALLATION, "installation", &saves).await;
    let save = fixture.approve(INSTALLATION, "save", &saves).await;
    assert!(fixture.size(&install, 0).await.is_ok());
    assert!(fixture.size(&save, 0).await.is_ok());

    // Losing the permission of one kind ends that kind only.
    fixture
        .api
        .revoke_session_permission("fixture", cap::GAME_STORAGE_INSPECT)
        .await;
    assert_eq!(
        fixture.size(&install, 0).await.unwrap_err().code(),
        Code::PermissionDenied
    );
    assert!(fixture.size(&save, 0).await.is_ok());
    fixture
        .api
        .revoke_session_permission("fixture", cap::GAME_SAVES_INSPECT)
        .await;
    assert_eq!(
        fixture.size(&save, 0).await.unwrap_err().code(),
        Code::PermissionDenied
    );
    for denied in [
        fixture.access(&save, &[]).await.unwrap_err(),
        fixture.modified(&save, 0).await.unwrap_err(),
    ] {
        assert_eq!(denied.code(), Code::PermissionDenied);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn inspecting_needs_no_right_to_read_files() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let file = fixture
        .child(&fixture.child(&root, "states").await, "s1.sav")
        .await;
    fixture
        .api
        .revoke_session_permission("fixture", cap::FILES_READ)
        .await;
    // The metadata is still there, and the content still cannot be read.
    assert_eq!(fixture.size(&root, 0).await.unwrap().file_count, 2);
    assert_eq!(fixture.size(&file, 0).await.unwrap().logical_bytes, 5);
    assert_eq!(
        fixture.ls(&root).await.unwrap_err().code(),
        Code::PermissionDenied
    );
    assert_eq!(
        fixture.cat(&file, 0, 10).await.unwrap_err().code(),
        Code::PermissionDenied
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_forged_unknown_or_revoked_place_is_not_found_and_another_plugins_profile_is_not_there() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    for handle in ["forged", "profile-unknown", &format!("{root}x")] {
        assert_eq!(
            fixture.size(handle, 0).await.unwrap_err().code(),
            Code::NotFound,
            "{handle}"
        );
    }
    assert_eq!(
        fixture.size("", 0).await.unwrap_err().code(),
        Code::InvalidArgument
    );

    // Revoke the approval: its handle stops working at once.
    let caller = fixture.caller();
    fixture
        .storage
        .run_for_host(
            caller.namespace.as_ref().unwrap(),
            caller.profile_epoch,
            |namespace| {
                let id = namespace.approval_for(INSTALLATION, "save")?.unwrap().id;
                namespace.revoke_approval(&id)
            },
        )
        .await
        .unwrap();
    assert_eq!(
        fixture.size(&root, 0).await.unwrap_err().code(),
        Code::NotFound
    );
    // A forged or missing context is refused.
    let mut forged = fixture.context();
    forged.session_credential = vec![0; forged.session_credential.len()];
    let denied = fixture
        .locations()
        .get_size(LocationSizeRequest {
            context: Some(forged),
            approved_resource_handle: root,
            max_entries: 0,
        })
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
    assert_eq!(
        fixture
            .locations()
            .get_size(LocationSizeRequest::default())
            .await
            .unwrap_err()
            .code(),
        Code::Unauthenticated
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_walk_that_was_cut_says_it_is_not_complete() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let cut = fixture.size(&root, 2).await.unwrap();
    assert!(!cut.complete);
    assert!(cut.file_count + cut.directory_count <= 2);
    assert!(fixture.size(&root, 5).await.unwrap().complete);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_that_is_gone_or_unreadable_is_reported_by_availability_not_as_zero_bytes() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;

    if !running_as_root() {
        std::fs::set_permissions(&saves, std::fs::Permissions::from_mode(0o000)).unwrap();
        let locked = fixture.size(&root, 0).await;
        std::fs::set_permissions(&saves, std::fs::Permissions::from_mode(0o755)).unwrap();
        let locked = locked.unwrap();
        assert_eq!(
            locked.availability,
            InspectionAvailability::Inaccessible as i32
        );
        assert!(!locked.complete);
    }

    // The folder is moved away (an unmounted drive looks the same to this layer): it is missing,
    // and nothing is created in its place.
    let away = saves.with_extension("away");
    std::fs::rename(&saves, &away).unwrap();
    let missing = fixture.size(&root, 0).await.unwrap();
    assert_eq!(missing.availability, InspectionAvailability::Missing as i32);
    assert!(!missing.complete);
    assert!(!saves.exists(), "no replacement folder is made");
    for reply in [
        fixture.modified(&root, 0).await.unwrap().availability,
        fixture.access(&root, &[]).await.unwrap().entries[0].availability,
    ] {
        assert_eq!(reply, InspectionAvailability::Missing as i32);
    }

    // It comes back and answers again, with what is there.
    std::fs::rename(&away, &saves).unwrap();
    assert_eq!(fixture.size(&root, 0).await.unwrap().file_count, 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_measures_the_union_of_its_folders_narrowed_by_its_rules_counting_overlaps_once()
{
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let states = fixture.child(&root, "states").await;
    std::fs::write(saves.join("states/old.tmp"), b"tmp!").unwrap();
    std::fs::write(saves.join("notes.txt"), b"notes").unwrap();

    // The whole folder and the folder below it overlap: states/ files count once.
    let overlapping = fixture
        .make(
            "a",
            profile(
                "Everything",
                "rt",
                vec![
                    directory(&root, "All", &[]),
                    directory(&states, "States", &[]),
                ],
            ),
        )
        .await
        .unwrap();
    let all = fixture.size(&overlapping.profile_id, 0).await.unwrap();
    // card.gci 4 + s1.sav 5 + old.tmp 4 + notes.txt 5, each once.
    assert_eq!(
        (all.logical_bytes, all.file_count, all.complete),
        (18, 4, true)
    );

    // Rules narrow the selection: only .gci anywhere and .sav in states, minus temporary files.
    let narrowed = fixture
        .make(
            "b",
            profile(
                "Saves only",
                "rt",
                vec![directory(
                    &root,
                    "Cards",
                    &["**/*.gci", "**/*.sav", "!**/*.tmp"],
                )],
            ),
        )
        .await
        .unwrap();
    let only = fixture.size(&narrowed.profile_id, 0).await.unwrap();
    assert_eq!((only.logical_bytes, only.file_count), (4 + 5, 2));

    // A rule that selects nothing is a total of zero that is complete, which is an honest zero.
    let nothing = fixture
        .make(
            "c",
            profile("None", "rt", vec![directory(&root, "x", &["*.nothing"])]),
        )
        .await
        .unwrap();
    let zero = fixture.size(&nothing.profile_id, 0).await.unwrap();
    assert_eq!(
        (zero.logical_bytes, zero.file_count, zero.complete),
        (0, 0, true)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_with_a_folder_whose_approval_is_gone_gives_a_partial_total() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let other = fixture.game_dir("other-emulator", &[("x.sav", b"123456")]);
    let second = fixture.approve(INSTALLATION, "prefix", &other).await;
    let created = fixture
        .make(
            "a",
            profile(
                "Two",
                "rt",
                vec![directory(&root, "", &[]), directory(&second, "", &[])],
            ),
        )
        .await
        .unwrap();
    assert!(fixture.size(&created.profile_id, 0).await.unwrap().complete);

    let caller = fixture.caller();
    fixture
        .storage
        .run_for_host(
            caller.namespace.as_ref().unwrap(),
            caller.profile_epoch,
            |namespace| {
                let id = namespace.approval_for(INSTALLATION, "prefix")?.unwrap().id;
                namespace.revoke_approval(&id)
            },
        )
        .await
        .unwrap();
    let partial = fixture.size(&created.profile_id, 0).await.unwrap();
    // What can still be reached is counted; the total says it is not the whole.
    assert_eq!((partial.file_count, partial.complete), (2, false));
}

#[tokio::test(flavor = "multi_thread")]
async fn last_modified_gives_each_entry_its_time_and_the_newest_apart_from_the_folders_own() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    set_time(&saves.join("GC/USA/card.gci"), 1_700_000_100, 250_000_000);
    set_time(&saves.join("states/s1.sav"), 1_700_000_200, 0);
    // The folder itself was touched later than any file in it.
    set_time_dir(&saves, 1_800_000_000);

    let reply = fixture.modified(&root, 0).await.unwrap();
    assert_eq!(reply.availability, InspectionAvailability::Available as i32);
    // The folder's own time comes first, under the handle that was asked about.
    let own = &reply.entries[0];
    assert_eq!(
        (own.entry_handle.as_str(), own.modified_at),
        (root.as_str(), 1_800_000_000)
    );
    // The newest is the newest of the entries below, not the folder's time.
    assert_eq!(reply.aggregate_latest_at, Some(1_700_000_200));
    let by_time: Vec<_> = reply.entries[1..]
        .iter()
        .map(|entry| (entry.modified_at, entry.precision.as_str()))
        .collect();
    assert!(by_time.contains(&(1_700_000_100, "subsecond")));
    assert!(by_time.contains(&(1_700_000_200, "whole_second")));
    // Each entry comes with a handle that is good for the next call.
    assert!(
        reply
            .entries
            .iter()
            .all(|entry| !entry.entry_handle.is_empty())
    );
    let one = fixture.size(&reply.entries[1].entry_handle, 0).await;
    assert!(one.is_ok());

    // The list is bounded.
    let few = fixture.modified(&root, 1).await.unwrap();
    assert_eq!(few.entries.len(), 2, "the folder and one entry");
}

fn set_time_dir(path: &Path, seconds: u64) {
    std::fs::File::open(path)
        .unwrap()
        .set_modified(UNIX_EPOCH + StdDuration::from_secs(seconds))
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profiles_last_modified_covers_its_selected_files_only() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    set_time(&saves.join("GC/USA/card.gci"), 1_700_000_100, 0);
    set_time(&saves.join("states/s1.sav"), 1_700_000_900, 0);
    let created = fixture
        .make(
            "a",
            profile("Cards", "rt", vec![directory(&root, "", &["GC/**/*.gci"])]),
        )
        .await
        .unwrap();
    let reply = fixture.modified(&created.profile_id, 0).await.unwrap();
    assert_eq!(reply.aggregate_latest_at, Some(1_700_000_100));
    assert_eq!(reply.entries.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn access_tells_read_only_metadata_apart_from_what_the_user_may_do() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let states = fixture.child(&root, "states").await;
    let file = fixture.child(&states, "s1.sav").await;
    let path = saves.join("states/s1.sav");

    // The folder itself, when no entry is named.
    let folder = fixture.access(&root, &[]).await.unwrap();
    assert_eq!(
        folder.availability,
        InspectionAvailability::Available as i32
    );
    let [entry] = folder.entries.try_into().unwrap();
    assert_eq!(entry.entry_handle, root);
    assert_eq!(entry.read_only_state, "writable");
    assert!(
        entry
            .effective_access
            .starts_with("read=allowed write=allowed create=allowed")
    );

    // A file whose mode says read-only: the metadata and the real answer are separate fields.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
    let reply = fixture.access(&root, &[&file]).await.unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let [locked] = reply.entries.try_into().unwrap();
    assert_eq!(locked.entry_handle, file);
    assert_eq!(locked.read_only_state, "read_only");
    assert!(locked.effective_access.starts_with("read=allowed"));
    if !running_as_root() {
        assert!(locked.effective_access.contains("write=denied"));
    }
    // "create" does not apply to a file, and nothing is guessed for it.
    assert!(locked.effective_access.contains("create=unknown"));
}

#[tokio::test(flavor = "multi_thread")]
async fn access_refuses_entries_of_another_place_and_too_many() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let elsewhere = fixture.game_dir("elsewhere", &[("a.sav", b"1")]);
    let theirs = fixture.approve("id-fixture:2", "save", &elsewhere).await;
    let theirs_file = fixture.child(&theirs, "a.sav").await;
    // An entry of another approval is not an entry of this place.
    assert_eq!(
        fixture
            .access(&root, &[&theirs_file])
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    assert_eq!(
        fixture.access(&root, &["forged"]).await.unwrap_err().code(),
        Code::NotFound
    );
    let many: Vec<String> = (0..101).map(|n| format!("h{n}")).collect();
    let refs: Vec<&str> = many.iter().map(String::as_str).collect();
    assert_eq!(
        fixture.access(&root, &refs).await.unwrap_err().code(),
        Code::InvalidArgument
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profiles_access_lists_each_of_its_folders_and_reports_a_lost_one_as_missing() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let other = fixture.game_dir("other-emulator", &[("x.sav", b"123456")]);
    let second = fixture.approve(INSTALLATION, "prefix", &other).await;
    let created = fixture
        .make(
            "a",
            profile(
                "Two",
                "rt",
                vec![directory(&root, "", &[]), directory(&second, "", &[])],
            ),
        )
        .await
        .unwrap();
    let reply = fixture.access(&created.profile_id, &[]).await.unwrap();
    assert_eq!(reply.entries.len(), 2);
    assert!(
        reply
            .entries
            .iter()
            .all(|e| e.availability == InspectionAvailability::Available as i32)
    );
    // A profile is inspected as a whole.
    assert_eq!(
        fixture
            .access(&created.profile_id, &["h"])
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    // Revoke one folder's approval: that folder is reported missing, the other still answers.
    let caller = fixture.caller();
    fixture
        .storage
        .run_for_host(
            caller.namespace.as_ref().unwrap(),
            caller.profile_epoch,
            |namespace| {
                let id = namespace.approval_for(INSTALLATION, "prefix")?.unwrap().id;
                namespace.revoke_approval(&id)
            },
        )
        .await
        .unwrap();
    let after = fixture.access(&created.profile_id, &[]).await.unwrap();
    let states: Vec<i32> = after.entries.iter().map(|e| e.availability).collect();
    assert!(states.contains(&(InspectionAvailability::Missing as i32)));
    assert!(states.contains(&(InspectionAvailability::Available as i32)));
}

#[tokio::test(flavor = "multi_thread")]
async fn inspecting_changes_nothing() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let file = saves.join("states/s1.sav");
    set_time(&file, 1_650_000_000, 7);
    let before = std::fs::metadata(&file).unwrap();
    let dir_mode = std::fs::metadata(&saves).unwrap().permissions().mode();
    let dir_time = std::fs::metadata(&saves).unwrap().modified().unwrap();
    let created = fixture
        .make("a", profile("P", "rt", vec![directory(&root, "", &[])]))
        .await
        .unwrap();

    for handle in [root.as_str(), created.profile_id.as_str()] {
        fixture.size(handle, 0).await.unwrap();
        fixture.modified(handle, 0).await.unwrap();
        fixture.access(handle, &[]).await.unwrap();
    }
    let after = std::fs::metadata(&file).unwrap();
    assert_eq!(after.modified().unwrap(), before.modified().unwrap());
    assert_eq!(after.permissions().mode(), before.permissions().mode());
    assert_eq!(std::fs::read(&file).unwrap(), b"state");
    assert_eq!(
        std::fs::metadata(&saves).unwrap().permissions().mode(),
        dir_mode
    );
    assert_eq!(
        std::fs::metadata(&saves).unwrap().modified().unwrap(),
        dir_time
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn describe_api_lists_the_locations_methods_and_the_rest_is_not_served() {
    let (fixture, _) = Fixture::emulator(INSPECT).await;
    let described = fixture.host.describe_api().await.unwrap();
    let served: Vec<_> = described
        .methods
        .iter()
        .filter(|method| method.service == "Locations")
        .map(|method| (method.method.as_str(), method.permissions.clone()))
        .collect();
    let either = vec![
        cap::GAME_STORAGE_INSPECT.to_owned(),
        cap::GAME_SAVES_INSPECT.to_owned(),
    ];
    assert_eq!(
        served,
        [
            ("GetSize", either.clone()),
            ("GetAccess", either.clone()),
            ("GetLastModified", either),
        ]
    );
    let mut client = fixture.locations();
    let context = Some(fixture.context());
    assert_eq!(
        client
            .inspect(LocationInspectRequest {
                context: context.clone(),
                ..Default::default()
            })
            .await
            .unwrap_err()
            .code(),
        Code::Unimplemented
    );
    assert_eq!(
        client
            .get_fingerprint(LocationFingerprintRequest {
                context: context.clone(),
                ..Default::default()
            })
            .await
            .unwrap_err()
            .code(),
        Code::Unimplemented
    );
    assert_eq!(
        client
            .compare(LocationCompareRequest {
                context: context.clone(),
                ..Default::default()
            })
            .await
            .unwrap_err()
            .code(),
        Code::Unimplemented
    );
    assert_eq!(
        client
            .watch_availability(LocationAvailabilityWatchRequest {
                context,
                ..Default::default()
            })
            .await
            .unwrap_err()
            .code(),
        Code::Unimplemented
    );
}
