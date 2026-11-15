//! The `Saves` service over the real local socket (task 13.1): profiles that say where one
//! runtime keeps a game's saves, resting on folders the user already approved.

use std::path::{Path, PathBuf};

use gamenode_platform::plugin_storage::SaveProfileRow;
use gamenode_plugin_protocol::prost_types::{Struct, Value, value::Kind};

use super::*;
use crate::plugin_host::saves::{ForeignGrant, ProfileGrants, access};

const GRANTS: &[&str] = &[
    cap::GAME_SAVES_READ,
    cap::GAME_SAVES_CREATE,
    cap::GAME_SAVES_UPDATE,
    cap::GAME_SAVES_DELETE,
    cap::FILES_READ,
];
pub(super) const INSTALLATION: &str = "id-fixture:1";

impl Fixture {
    pub(super) fn saves(
        &self,
    ) -> gamenode_plugin_protocol::proto::saves_client::SavesClient<Channel> {
        self.host.saves_client()
    }

    /// A plugin with an emulator folder (`GC/USA/card.gci`, `states/s1.sav`) for one game entry
    /// and another entry, and a user who approves whatever folder is set next.
    pub(super) async fn emulator(grants: &[&'static str]) -> (Self, PathBuf) {
        let fixture = Self::new(grants).await;
        let saves = fixture.game_dir(
            "dolphin-saves",
            &[("GC/USA/card.gci", b"card"), ("states/s1.sav", b"state")],
        );
        let rom = fixture.game_dir("rom", &[]);
        let other = fixture.game_dir("other", &[]);
        fixture.library_in(&[
            ("fixture:1", Some(3), "ROM", &rom),
            ("fixture:2", Some(4), "Other", &other),
            ("steam:1", Some(5), "Foreign", &other),
        ]);
        (fixture, saves)
    }

    /// The root handle of a folder the user approves for `installation` as `kind`.
    pub(super) async fn approve(&self, installation: &str, kind: &str, folder: &Path) -> String {
        self.user_approving(folder);
        self.request_access(installation, &["read"], &[kind])
            .await
            .unwrap()
            .handles
            .remove(0)
            .handle
    }

    pub(super) async fn make(
        &self,
        key: &str,
        profile: SaveProfile,
    ) -> Result<SaveProfileMutationReply, Status> {
        self.saves()
            .create_profile(SaveProfileCreateRequest {
                mutation: Some(self.mutation(key, 0)),
                profile: Some(profile),
            })
            .await
            .map(Response::into_inner)
    }

    pub(super) async fn profile(&self, id: &str) -> Result<SaveProfileReply, Status> {
        self.saves()
            .get_profile(SaveProfileGetRequest {
                context: Some(self.context()),
                game_id: String::new(),
                profile_id: id.into(),
                shared_records: None,
            })
            .await
            .map(Response::into_inner)
    }

    pub(super) async fn profiles(
        &self,
        installation: &str,
        page_size: u32,
        cursor: &str,
        shared: Option<CommonDataSelector>,
    ) -> Result<SaveProfileListReply, Status> {
        self.saves()
            .list_profiles(SaveProfileListRequest {
                context: Some(self.context()),
                game_id: String::new(),
                installation_id: installation.into(),
                cursor: cursor.into(),
                page_size,
                shared_records: shared,
            })
            .await
            .map(Response::into_inner)
    }

    pub(super) async fn patch(
        &self,
        key: &str,
        id: &str,
        expected: u64,
        mask: &[&str],
        new: SaveProfile,
    ) -> Result<SaveProfileMutationReply, Status> {
        self.saves()
            .update_profile(SaveProfileUpdateRequest {
                mutation: Some(self.mutation(key, expected)),
                profile_id: id.into(),
                patch: Some(SaveProfilePatch {
                    field_mask: mask.iter().map(|path| (*path).into()).collect(),
                    profile: Some(new),
                }),
            })
            .await
            .map(Response::into_inner)
    }

    pub(super) async fn drop_profile(
        &self,
        key: &str,
        id: &str,
        expected: u64,
    ) -> Result<SaveProfileMutationReply, Status> {
        self.saves()
            .remove_profile(SaveProfileRemoveRequest {
                mutation: Some(self.mutation(key, expected)),
                game_id: String::new(),
                profile_id: id.into(),
            })
            .await
            .map(Response::into_inner)
    }
}

pub(super) fn directory(handle: &str, name: &str, patterns: &[&str]) -> SaveDirectory {
    SaveDirectory {
        approved_resource_handle: handle.into(),
        display_name: name.into(),
        naming_patterns: patterns.iter().map(|p| (*p).into()).collect(),
    }
}

pub(super) fn profile(name: &str, runtime: &str, directories: Vec<SaveDirectory>) -> SaveProfile {
    SaveProfile {
        installation_id: INSTALLATION.into(),
        name: name.into(),
        runtime_id: runtime.into(),
        directories,
        ..SaveProfile::default()
    }
}

fn text(value: &str) -> Value {
    Value {
        kind: Some(Kind::StringValue(value.into())),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_creates_and_reads_back_a_profile_that_rests_on_an_approved_folder() {
    let (fixture, saves) = Fixture::emulator(GRANTS).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let mut new = profile(
        "  Dolphin card A ",
        "org.dolphin-emu",
        vec![directory(
            &root,
            "Memory card",
            &["GC/**/*.gci", "!cache/**"],
        )],
    );
    new.format_id = "gci".into();
    new.format_version = 2;
    new.descriptive_metadata = Some(Struct {
        fields: [("region".to_owned(), text("USA"))].into(),
    });
    let created = fixture.make("one", new).await.unwrap();
    assert_eq!(created.outcome, MutationOutcome::Applied as i32);
    assert!(created.profile_id.starts_with("profile-"));
    assert!(created.revision > 0);

    let read = fixture
        .profile(&created.profile_id)
        .await
        .unwrap()
        .profile
        .unwrap();
    assert_eq!(
        (
            read.name.as_str(),
            read.game_id.as_str(),
            read.installation_id.as_str(),
            read.runtime_id.as_str(),
            read.format_id.as_str(),
            read.format_version,
            read.creator_plugin_id.as_str(),
            read.revision,
        ),
        (
            "Dolphin card A",
            "3",
            INSTALLATION,
            "org.dolphin-emu",
            "gci",
            2,
            "fixture.plugin",
            created.revision
        )
    );
    assert_eq!(
        read.descriptive_metadata.unwrap().fields["region"],
        text("USA")
    );
    let [folder] = read.directories.try_into().unwrap();
    assert_eq!(folder.display_name, "Memory card");
    assert_eq!(folder.naming_patterns, ["GC/**/*.gci", "!cache/**"]);
    // The folder comes back as a handle of this session, which works like the one it came from.
    let listed = fixture.ls(&folder.approved_resource_handle).await.unwrap();
    assert_eq!(names(&listed), ["GC", "states"]);

    let all = fixture.profiles("", 0, "", None).await.unwrap();
    assert_eq!(all.profiles.len(), 1);
    assert_eq!(all.revision, created.revision);
}

fn names(reply: &FilesListReply) -> Vec<&str> {
    reply
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn one_rom_can_have_distinct_save_folders_for_distinct_runtimes() {
    let (fixture, dolphin) = Fixture::emulator(GRANTS).await;
    let other_emulator = fixture.game_dir("other-emulator", &[("slot1.sav", b"s")]);
    let first = fixture.approve(INSTALLATION, "save", &dolphin).await;
    let second = fixture
        .approve(INSTALLATION, "prefix", &other_emulator)
        .await;

    let a = fixture
        .make(
            "a",
            profile("Dolphin", "dolphin", vec![directory(&first, "", &[])]),
        )
        .await
        .unwrap();
    let b = fixture
        .make(
            "b",
            profile("Other emulator", "other", vec![directory(&second, "", &[])]),
        )
        .await
        .unwrap();
    assert_ne!(a.profile_id, b.profile_id);
    assert!(b.revision > a.revision, "one counter, never repeated");

    let both = fixture.profiles(INSTALLATION, 0, "", None).await.unwrap();
    let runtimes: Vec<_> = both
        .profiles
        .iter()
        .map(|p| p.runtime_id.as_str())
        .collect();
    assert_eq!(runtimes.len(), 2);
    assert!(runtimes.contains(&"dolphin") && runtimes.contains(&"other"));
    // Another game entry has none.
    assert!(
        fixture
            .profiles("id-fixture:2", 0, "", None)
            .await
            .unwrap()
            .profiles
            .is_empty()
    );

    // The name is unique per game entry, without regard to case; another entry may reuse it.
    let again = fixture
        .make(
            "c",
            profile("DOLPHIN", "dolphin", vec![directory(&first, "", &[])]),
        )
        .await
        .unwrap_err();
    assert_eq!(again.code(), Code::AlreadyExists);
}

#[tokio::test(flavor = "multi_thread")]
async fn lists_are_paged_in_a_stable_order() {
    let (fixture, saves) = Fixture::emulator(GRANTS).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let mut ids = Vec::new();
    for n in 0..5 {
        let created = fixture
            .make(
                &format!("k{n}"),
                profile(
                    &format!("Profile {n}"),
                    "rt",
                    vec![directory(&root, "", &[])],
                ),
            )
            .await
            .unwrap();
        ids.push(created.profile_id);
    }
    let mut seen = Vec::new();
    let mut cursor = String::new();
    loop {
        let page = fixture.profiles("", 2, &cursor, None).await.unwrap();
        assert!(page.profiles.len() <= 2);
        seen.extend(page.profiles.iter().map(|p| p.profile_id.clone()));
        if page.next_cursor.is_empty() {
            break;
        }
        cursor = page.next_cursor;
    }
    let mut sorted = ids.clone();
    sorted.sort();
    seen.sort();
    assert_eq!(seen, sorted, "every profile once");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_reaches_only_folders_approved_for_its_own_game_entry() {
    let (fixture, saves) = Fixture::emulator(GRANTS).await;
    let mine = fixture.approve(INSTALLATION, "save", &saves).await;
    let elsewhere = fixture.game_dir("elsewhere", &[]);
    let theirs = fixture.approve("id-fixture:2", "save", &elsewhere).await;

    // A folder approved for another game entry is not this one's.
    let refused = fixture
        .make("a", profile("P", "rt", vec![directory(&theirs, "", &[])]))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), Code::PermissionDenied);
    // A forged, edited or path-like handle is just not found.
    for handle in ["forged", "", "/etc", &format!("{mine}x")] {
        let refused = fixture
            .make("b", profile("P", "rt", vec![directory(handle, "", &[])]))
            .await
            .unwrap_err();
        assert_eq!(refused.code(), Code::NotFound, "{handle:?}");
    }
    // The same folder twice, and no folder at all.
    assert_eq!(
        fixture
            .make(
                "c",
                profile(
                    "P",
                    "rt",
                    vec![directory(&mine, "", &[]), directory(&mine, "", &[])]
                )
            )
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    assert_eq!(
        fixture
            .make("d", profile("P", "rt", vec![]))
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    // A folder below the approved one is fine, and so is the same folder with other rules.
    let below = fixture.ls(&mine).await.unwrap();
    let states = below
        .entries
        .iter()
        .find(|entry| entry.name == "states")
        .unwrap();
    let created = fixture
        .make(
            "e",
            profile(
                "States",
                "rt",
                vec![
                    directory(&states.handle, "States", &["*.sav"]),
                    directory(&mine, "All", &[]),
                ],
            ),
        )
        .await
        .unwrap();
    assert_eq!(created.outcome, MutationOutcome::Applied as i32);
    assert!(
        fixture
            .profiles("", 0, "", None)
            .await
            .unwrap()
            .profiles
            .len()
            == 1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_revoked_approval_leaves_the_profile_with_no_reachable_folder() {
    let (fixture, saves) = Fixture::emulator(GRANTS).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let created = fixture
        .make(
            "a",
            profile("P", "rt", vec![directory(&root, "Card", &["*.gci"])]),
        )
        .await
        .unwrap();

    // The user takes the folder approval back.
    let caller = fixture.caller();
    let approval = fixture
        .storage
        .run_for_host(
            caller.namespace.as_ref().unwrap(),
            caller.profile_epoch,
            |namespace| {
                let id = namespace
                    .approval_for(INSTALLATION, "save")?
                    .expect("the folder was approved")
                    .id;
                namespace.revoke_approval(&id)
            },
        )
        .await
        .unwrap();
    assert!(approval);

    let read = fixture
        .profile(&created.profile_id)
        .await
        .unwrap()
        .profile
        .unwrap();
    let [folder] = read.directories.try_into().unwrap();
    // The profile and its rules remain; the folder is simply not reachable any more.
    assert_eq!(folder.approved_resource_handle, "");
    assert_eq!(
        (folder.display_name.as_str(), folder.naming_patterns.len()),
        ("Card", 1)
    );
    // A new profile cannot rest on it either.
    assert_eq!(
        fixture
            .make("b", profile("Q", "rt", vec![directory(&root, "", &[])]))
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn configuring_a_profile_authorizes_no_file_access() {
    let (fixture, saves) = Fixture::emulator(GRANTS).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let created = fixture
        .make("a", profile("P", "rt", vec![directory(&root, "", &[])]))
        .await
        .unwrap();
    let read = fixture.profile(&created.profile_id).await.unwrap();
    // The reply carries no path of the user's computer.
    let shown = format!("{read:?}");
    assert!(!shown.contains(saves.to_str().unwrap()) && !shown.contains("dolphin-saves"));

    // The handle in it is worth only what files.read is: lose that and it opens nothing, though the
    // profile is still readable.
    let handle = read.profile.unwrap().directories[0]
        .approved_resource_handle
        .clone();
    fixture
        .api
        .revoke_session_permission("fixture", cap::FILES_READ)
        .await;
    assert_eq!(
        fixture.ls(&handle).await.unwrap_err().code(),
        Code::PermissionDenied
    );
    assert!(fixture.profile(&created.profile_id).await.is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn what_a_plugin_may_not_set_or_send_is_refused() {
    let (fixture, saves) = Fixture::emulator(GRANTS).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let dirs = || vec![directory(&root, "", &[])];
    let good = || profile("P", "rt", dirs());
    let code = |result: Result<SaveProfileMutationReply, Status>| result.unwrap_err().code();

    // The host's fields are the host's.
    let mut with_id = good();
    with_id.profile_id = "mine".into();
    assert_eq!(
        code(fixture.make("a", with_id).await),
        Code::InvalidArgument
    );
    let mut with_creator = good();
    with_creator.creator_plugin_id = "someone.else".into();
    assert_eq!(
        code(fixture.make("b", with_creator).await),
        Code::InvalidArgument
    );
    let mut with_revision = good();
    with_revision.revision = 4;
    assert_eq!(
        code(fixture.make("c", with_revision).await),
        Code::InvalidArgument
    );
    // A create is revision 0.
    let stale = fixture
        .saves()
        .create_profile(SaveProfileCreateRequest {
            mutation: Some(fixture.mutation("d", 5)),
            profile: Some(good()),
        })
        .await
        .unwrap_err();
    assert_eq!(stale.code(), Code::InvalidArgument);

    // Names, ids, rules and metadata.
    for bad in [
        profile("", "rt", dirs()),
        profile(&"x".repeat(121), "rt", dirs()),
        profile("line\nbreak", "rt", dirs()),
        profile("P", "has space", dirs()),
        profile("P", &"x".repeat(129), dirs()),
        profile("P", "rt", vec![directory(&root, "", &["/etc/passwd"])]),
        profile("P", "rt", vec![directory(&root, "", &["../up"])]),
        profile("P", "rt", vec![directory(&root, "", &["a", "a"])]),
        profile("P", "rt", vec![directory(&root, "bad\nname", &[])]),
    ] {
        assert_eq!(code(fixture.make("x", bad).await), Code::InvalidArgument);
    }
    let mut huge = good();
    huge.descriptive_metadata = Some(Struct {
        fields: [("k".to_owned(), text(&"y".repeat(9000)))].into(),
    });
    assert_eq!(code(fixture.make("y", huge).await), Code::InvalidArgument);
    // Too many folders.
    let many: Vec<_> = (0..9).map(|_| directory(&root, "", &[])).collect();
    assert_eq!(
        code(fixture.make("z", profile("P", "rt", many)).await),
        Code::InvalidArgument
    );

    // The entry must be this plugin's own: another provider's is not found, nor is an unknown one.
    for installation in ["id-steam:1", "id-nothing"] {
        let mut foreign = good();
        foreign.installation_id = installation.into();
        assert_eq!(code(fixture.make("w", foreign).await), Code::NotFound);
    }
    let mut wrong_game = good();
    wrong_game.game_id = "999".into();
    assert_eq!(code(fixture.make("v", wrong_game).await), Code::NotFound);
    // Nothing of that was kept.
    assert!(
        fixture
            .profiles("", 0, "", None)
            .await
            .unwrap()
            .profiles
            .is_empty()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_is_updated_by_revision_and_field_mask() {
    let (fixture, saves) = Fixture::emulator(GRANTS).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let created = fixture
        .make(
            "a",
            profile("Card", "rt", vec![directory(&root, "Root", &["*.gci"])]),
        )
        .await
        .unwrap();

    let renamed = fixture
        .patch(
            "u1",
            &created.profile_id,
            created.revision,
            &["name", "format_id", "format_version"],
            SaveProfile {
                name: "Card B".into(),
                format_id: "gci".into(),
                format_version: 3,
                // Not in the mask, so ignored.
                runtime_id: "ignored".into(),
                ..SaveProfile::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(renamed.outcome, MutationOutcome::Applied as i32);
    assert!(renamed.revision > created.revision);
    let read = fixture
        .profile(&created.profile_id)
        .await
        .unwrap()
        .profile
        .unwrap();
    assert_eq!(
        (
            read.name.as_str(),
            read.runtime_id.as_str(),
            read.format_id.as_str(),
            read.format_version
        ),
        ("Card B", "rt", "gci", 3)
    );
    // The folders were not in the mask: they are as they were.
    assert_eq!(read.directories[0].naming_patterns, ["*.gci"]);

    // A stale revision changes nothing and says where the profile is.
    let stale = fixture
        .patch(
            "u2",
            &created.profile_id,
            created.revision,
            &["name"],
            SaveProfile {
                name: "Lost".into(),
                ..SaveProfile::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        (stale.outcome, stale.revision),
        (MutationOutcome::RevisionConflict as i32, renamed.revision)
    );
    assert_eq!(
        fixture
            .profile(&created.profile_id)
            .await
            .unwrap()
            .profile
            .unwrap()
            .name,
        "Card B"
    );

    // New folders and rules replace the old ones, through handles of this session.
    let moved = fixture
        .patch(
            "u3",
            &created.profile_id,
            renamed.revision,
            &["directories", "descriptive_metadata"],
            SaveProfile {
                directories: vec![directory(&root, "Everything", &["**", "!*.tmp"])],
                descriptive_metadata: Some(Struct {
                    fields: [("core".to_owned(), text("gc"))].into(),
                }),
                ..SaveProfile::default()
            },
        )
        .await
        .unwrap();
    let read = fixture
        .profile(&created.profile_id)
        .await
        .unwrap()
        .profile
        .unwrap();
    assert_eq!(read.directories[0].naming_patterns, ["**", "!*.tmp"]);
    assert_eq!(read.revision, moved.revision);

    // What may not be changed, and what is malformed.
    for (mask, expected) in [
        (&["profile_id"][..], moved.revision),
        (&["installation_id"], moved.revision),
        (&["game_id"], moved.revision),
        (&["creator_plugin_id"], moved.revision),
        (&["revision"], moved.revision),
        (&[], moved.revision),
        (&["name"], 0),
    ] {
        let refused = fixture
            .patch(
                "bad",
                &created.profile_id,
                expected,
                mask,
                SaveProfile::default(),
            )
            .await
            .unwrap_err();
        assert_eq!(
            refused.code(),
            Code::InvalidArgument,
            "{mask:?} at {expected}"
        );
    }
    // A profile that is not there, and a name another profile of the entry already has.
    assert_eq!(
        fixture
            .patch(
                "n",
                "profile-nothing",
                1,
                &["name"],
                SaveProfile {
                    name: "x".into(),
                    ..SaveProfile::default()
                }
            )
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    let other = fixture
        .make(
            "o",
            profile("Second", "rt", vec![directory(&root, "", &[])]),
        )
        .await
        .unwrap();
    let taken = fixture
        .patch(
            "t",
            &other.profile_id,
            other.revision,
            &["name"],
            SaveProfile {
                name: "card b".into(),
                ..SaveProfile::default()
            },
        )
        .await
        .unwrap_err();
    assert_eq!(taken.code(), Code::AlreadyExists);
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_a_profile_touches_no_file_and_never_repeats_a_revision() {
    let (fixture, saves) = Fixture::emulator(GRANTS).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let created = fixture
        .make("a", profile("Card", "rt", vec![directory(&root, "", &[])]))
        .await
        .unwrap();

    // A stale revision removes nothing.
    let stale = fixture
        .drop_profile("r0", &created.profile_id, created.revision + 9)
        .await
        .unwrap();
    assert_eq!(
        (stale.outcome, stale.revision),
        (MutationOutcome::RevisionConflict as i32, created.revision)
    );
    assert!(fixture.profile(&created.profile_id).await.is_ok());
    // Revision 0 is not a revision to remove at.
    assert_eq!(
        fixture
            .drop_profile("r1", &created.profile_id, 0)
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );

    let removed = fixture
        .drop_profile("r2", &created.profile_id, created.revision)
        .await
        .unwrap();
    assert_eq!(removed.outcome, MutationOutcome::Applied as i32);
    assert_eq!(
        fixture
            .profile(&created.profile_id)
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    // The folder and what is in it are exactly where the user keeps them.
    assert_eq!(
        std::fs::read(saves.join("GC/USA/card.gci")).unwrap(),
        b"card"
    );
    assert_eq!(
        std::fs::read(saves.join("states/s1.sav")).unwrap(),
        b"state"
    );
    // Removing again is not found; a profile made afterwards gets a higher revision than the removal.
    assert_eq!(
        fixture
            .drop_profile("r3", &created.profile_id, removed.revision)
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    let again = fixture
        .make("b", profile("Card", "rt", vec![directory(&root, "", &[])]))
        .await
        .unwrap();
    assert!(again.revision > removed.revision);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_retry_returns_the_original_outcome_and_a_reused_key_is_refused() {
    let (fixture, saves) = Fixture::emulator(GRANTS).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let same = || profile("Card", "rt", vec![directory(&root, "", &[])]);
    let first = fixture.make("key", same()).await.unwrap();
    let retry = fixture.make("key", same()).await.unwrap();
    assert_eq!(
        (&retry.profile_id, retry.revision, retry.outcome),
        (&first.profile_id, first.revision, first.outcome)
    );
    assert_eq!(
        fixture
            .profiles("", 0, "", None)
            .await
            .unwrap()
            .profiles
            .len(),
        1
    );
    // The same key for another request is refused.
    let other = fixture
        .make(
            "key",
            profile("Different", "rt", vec![directory(&root, "", &[])]),
        )
        .await
        .unwrap_err();
    assert_eq!(other.code(), Code::FailedPrecondition);

    // Updates and removals are retried the same way.
    let patch = || {
        fixture.patch(
            "u",
            &first.profile_id,
            first.revision,
            &["name"],
            SaveProfile {
                name: "Renamed".into(),
                ..SaveProfile::default()
            },
        )
    };
    let one = patch().await.unwrap();
    let two = patch().await.unwrap();
    assert_eq!((one.revision, one.outcome), (two.revision, two.outcome));
    let gone = fixture
        .drop_profile("d", &first.profile_id, one.revision)
        .await
        .unwrap();
    let again = fixture
        .drop_profile("d", &first.profile_id, one.revision)
        .await
        .unwrap();
    assert_eq!(gone.revision, again.revision);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_four_permissions_are_independent_and_checked_at_the_commit() {
    // Read only: nothing can be changed.
    let (reader, saves) = Fixture::emulator(&[cap::GAME_SAVES_READ, cap::FILES_READ]).await;
    let root = reader.approve(INSTALLATION, "save", &saves).await;
    let denied = reader
        .make("a", profile("P", "rt", vec![directory(&root, "", &[])]))
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
    assert!(
        reader
            .profiles("", 0, "", None)
            .await
            .unwrap()
            .profiles
            .is_empty()
    );

    // Create without read: it can be made but not looked at.
    let (maker, saves) = Fixture::emulator(&[cap::GAME_SAVES_CREATE, cap::FILES_READ]).await;
    let root = maker.approve(INSTALLATION, "save", &saves).await;
    let created = maker
        .make("a", profile("P", "rt", vec![directory(&root, "", &[])]))
        .await
        .unwrap();
    assert_eq!(
        maker.profile(&created.profile_id).await.unwrap_err().code(),
        Code::PermissionDenied
    );
    assert_eq!(
        maker.profiles("", 0, "", None).await.unwrap_err().code(),
        Code::PermissionDenied
    );
    assert_eq!(
        maker
            .patch(
                "u",
                &created.profile_id,
                created.revision,
                &["name"],
                SaveProfile::default()
            )
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    assert_eq!(
        maker
            .drop_profile("d", &created.profile_id, created.revision)
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );

    // Losing the permission ends it for good, and a retry of a committed write is still answered.
    maker
        .api
        .revoke_session_permission("fixture", cap::GAME_SAVES_CREATE)
        .await;
    let after = maker
        .make("b", profile("Q", "rt", vec![directory(&root, "", &[])]))
        .await
        .unwrap_err();
    assert_eq!(after.code(), Code::PermissionDenied);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_forged_context_and_a_missing_request_part_are_refused() {
    let (fixture, saves) = Fixture::emulator(GRANTS).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let mut forged = fixture.context();
    forged.session_credential = vec![0; forged.session_credential.len()];
    let denied = fixture
        .saves()
        .list_profiles(SaveProfileListRequest {
            context: Some(forged),
            ..SaveProfileListRequest::default()
        })
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
    let none = fixture
        .saves()
        .list_profiles(SaveProfileListRequest::default())
        .await
        .unwrap_err();
    assert_eq!(none.code(), Code::Unauthenticated);
    assert_eq!(
        fixture
            .saves()
            .create_profile(SaveProfileCreateRequest {
                mutation: None,
                profile: Some(profile("P", "rt", vec![directory(&root, "", &[])])),
            })
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    // Common-data categories are not served for profiles.
    let categories = fixture
        .profiles(
            "",
            0,
            "",
            Some(CommonDataSelector {
                categories: vec![CommonDataCategory::GamePerformanceHistory as i32],
                selected_record_ids: vec![],
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(categories.code(), Code::Unimplemented);
}

#[tokio::test(flavor = "multi_thread")]
async fn profiles_belong_to_the_account_profile_and_survive_a_restart() {
    let (mut fixture, saves) = Fixture::emulator(GRANTS).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let created = fixture
        .make("a", profile("Card", "rt", vec![directory(&root, "", &[])]))
        .await
        .unwrap();

    // The plugin restarts in the same account: its profile is there, and its folder is reachable
    // again through a handle of the new session.
    fixture.reconnect().await;
    let read = fixture
        .profile(&created.profile_id)
        .await
        .unwrap()
        .profile
        .unwrap();
    let handle = &read.directories[0].approved_resource_handle;
    assert!(!handle.is_empty());
    assert!(fixture.ls(handle).await.is_ok());

    // Another account signs in and sees none of it.
    fixture.api.profiles().update(Some("b"), true);
    fixture.api.fence_third_party_sessions().await;
    fixture.reconnect().await;
    assert!(
        fixture
            .profiles("", 0, "", None)
            .await
            .unwrap()
            .profiles
            .is_empty()
    );
    assert_eq!(
        fixture
            .profile(&created.profile_id)
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );

    // Back in the first account the profile is where it was, with its revision.
    fixture.api.profiles().update(Some("a"), true);
    fixture.api.fence_third_party_sessions().await;
    fixture.reconnect().await;
    let back = fixture
        .profile(&created.profile_id)
        .await
        .unwrap()
        .profile
        .unwrap();
    assert_eq!(back.revision, created.revision);
}

struct Granted(Vec<ForeignGrant>);

impl ProfileGrants for Granted {
    fn grants(&self, plugin_id: &str, publisher: &str) -> Vec<ForeignGrant> {
        assert_eq!(plugin_id, "fixture.plugin");
        assert_eq!(publisher, "ab".repeat(32));
        self.0.clone()
    }
}

impl Fixture {
    /// A profile another plugin ("org.maker") contributed, written into its own store.
    async fn makers_profile(&self) -> SaveProfileRow {
        let profiles = self.api.profiles();
        let resolver = profiles.resolver("org.maker", "cd", None).unwrap();
        let row = SaveProfileRow {
            id: "profile-maker".into(),
            installation_id: "maker-installation".into(),
            game_id: "9".into(),
            name: "Maker card".into(),
            runtime_id: "maker-runtime".into(),
            format_id: String::new(),
            format_version: 0,
            metadata: "{}".into(),
            directories: crate::plugin_host::saves::directories_json(&[
                crate::plugin_host::saves::Directory {
                    approval_id: "makers-approval".into(),
                    path: vec![],
                    display_name: "Maker folder".into(),
                    naming_patterns: vec!["*.sav".into()],
                },
            ]),
            revision: 0,
            created_at: 0,
        };
        let stored = row.clone();
        self.storage
            .run_for_host(&resolver, profiles.epoch(), move |namespace| {
                let guard: crate::plugin_host::library::CommitGuard = Box::new(|| Ok(()));
                namespace.put_save_profile(stored, 0, &guard)
            })
            .await
            .unwrap();
        row
    }

    async fn makers_row(&self) -> Option<SaveProfileRow> {
        let profiles = self.api.profiles();
        let resolver = profiles.resolver("org.maker", "cd", None).unwrap();
        self.storage
            .run_for_host(&resolver, profiles.epoch(), |namespace| {
                namespace.save_profile("profile-maker")
            })
            .await
            .unwrap()
    }

    fn grant_makers_profile(&self, operations: u8) {
        self.saves.set_grants(Arc::new(Granted(vec![ForeignGrant {
            creator_plugin_id: "org.maker".into(),
            creator_publisher: "cd".into(),
            profile_id: "profile-maker".into(),
            operations,
        }])));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn another_plugins_profile_is_reached_only_through_an_explicit_grant_and_keeps_its_provenance()
 {
    let (fixture, _) = Fixture::emulator(GRANTS).await;
    let row = fixture.makers_profile().await;

    // No grant: it is not there for this plugin, in any operation.
    assert_eq!(
        fixture.profile(&row.id).await.unwrap_err().code(),
        Code::NotFound
    );
    assert_eq!(
        fixture
            .patch(
                "u",
                &row.id,
                1,
                &["name"],
                SaveProfile {
                    name: "x".into(),
                    ..SaveProfile::default()
                }
            )
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    assert_eq!(
        fixture
            .drop_profile("d", &row.id, 1)
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );

    // Read granted: it can be read, with the creator named and no handle to its folders.
    fixture.grant_makers_profile(access::READ);
    let read = fixture.profile(&row.id).await.unwrap().profile.unwrap();
    assert_eq!(
        (
            read.creator_plugin_id.as_str(),
            read.name.as_str(),
            read.game_id.as_str()
        ),
        ("org.maker", "Maker card", "9")
    );
    assert_eq!(read.directories[0].approved_resource_handle, "");
    assert_eq!(read.directories[0].display_name, "Maker folder");
    // It is listed only when named; another profile of the maker is not reachable by this grant.
    assert!(
        fixture
            .profiles("", 0, "", None)
            .await
            .unwrap()
            .profiles
            .is_empty()
    );
    let named = fixture
        .profiles(
            "",
            0,
            "",
            Some(CommonDataSelector {
                categories: vec![],
                selected_record_ids: vec![row.id.clone(), "profile-other".into()],
            }),
        )
        .await
        .unwrap();
    assert_eq!(named.profiles.len(), 1);
    // Read does not mean update or remove.
    assert_eq!(
        fixture
            .patch(
                "u",
                &row.id,
                1,
                &["name"],
                SaveProfile {
                    name: "x".into(),
                    ..SaveProfile::default()
                }
            )
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    assert_eq!(
        fixture
            .drop_profile("d", &row.id, 1)
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );

    // Update granted: the name may change, the creator's folders may not.
    fixture.grant_makers_profile(access::READ | access::UPDATE);
    let revision = read.revision;
    let denied = fixture
        .patch(
            "dirs",
            &row.id,
            revision,
            &["directories"],
            SaveProfile {
                directories: vec![directory("anything", "", &[])],
                ..SaveProfile::default()
            },
        )
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
    let renamed = fixture
        .patch(
            "name",
            &row.id,
            revision,
            &["name"],
            SaveProfile {
                name: "Edited by reader".into(),
                ..SaveProfile::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(renamed.outcome, MutationOutcome::Applied as i32);
    let stored = fixture.makers_row().await.unwrap();
    assert_eq!(
        (stored.name.as_str(), stored.revision),
        ("Edited by reader", renamed.revision)
    );
    // The provenance and the folders are the creator's, unchanged.
    assert_eq!(stored.directories, row.directories);
    let again = fixture.profile(&row.id).await.unwrap().profile.unwrap();
    assert_eq!(again.creator_plugin_id, "org.maker");

    // Delete granted: it can be removed, and then it is gone for the creator too.
    fixture.grant_makers_profile(access::DELETE);
    let removed = fixture
        .drop_profile("rm", &row.id, renamed.revision)
        .await
        .unwrap();
    assert_eq!(removed.outcome, MutationOutcome::Applied as i32);
    assert!(fixture.makers_row().await.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_grant_the_user_takes_back_ends_the_access() {
    let (fixture, _) = Fixture::emulator(GRANTS).await;
    let row = fixture.makers_profile().await;
    fixture.grant_makers_profile(access::READ);
    assert!(fixture.profile(&row.id).await.is_ok());
    fixture.saves.set_grants(Arc::new(Granted(vec![])));
    assert_eq!(
        fixture.profile(&row.id).await.unwrap_err().code(),
        Code::NotFound
    );
    // And the profile's own permission still matters: without game.saves.read the grant is idle.
    fixture.grant_makers_profile(access::READ);
    fixture
        .api
        .revoke_session_permission("fixture", cap::GAME_SAVES_READ)
        .await;
    assert_eq!(
        fixture.profile(&row.id).await.unwrap_err().code(),
        Code::PermissionDenied
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn describe_api_lists_the_saves_methods_with_their_own_permissions() {
    let (fixture, _) = Fixture::emulator(GRANTS).await;
    let described = fixture.host.describe_api().await.unwrap();
    let saves: Vec<_> = described
        .methods
        .iter()
        .filter(|method| method.service == "Saves")
        .map(|method| (method.method.as_str(), method.permissions.clone()))
        .collect();
    assert_eq!(
        saves,
        [
            ("ListProfiles", vec![cap::GAME_SAVES_READ.to_owned()]),
            ("GetProfile", vec![cap::GAME_SAVES_READ.to_owned()]),
            ("CreateProfile", vec![cap::GAME_SAVES_CREATE.to_owned()]),
            ("UpdateProfile", vec![cap::GAME_SAVES_UPDATE.to_owned()]),
            ("RemoveProfile", vec![cap::GAME_SAVES_DELETE.to_owned()]),
        ]
    );
}
