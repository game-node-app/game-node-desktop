//! `Locations.GetFingerprint` and `Compare` over the real local socket (task 13.4): content
//! fingerprints of approved places, read fresh, and their comparison.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Mutex as StdMutex;
use std::time::{Duration as StdDuration, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use super::saves::{INSTALLATION, directory, profile};
use super::*;
use crate::plugin_host::fingerprints::{ALGORITHM, Record, tree_digest};

const INSPECT: &[&str] = &[
    cap::GAME_STORAGE_INSPECT,
    cap::GAME_SAVES_INSPECT,
    cap::GAME_SAVES_READ,
    cap::GAME_SAVES_CREATE,
    cap::GAME_SAVES_UPDATE,
    cap::FILES_READ,
];

fn running_as_root() -> bool {
    rustix::process::geteuid().is_root()
}

impl Fixture {
    async fn fingerprint(
        &self,
        key: &str,
        handle: &str,
        max_entries: u32,
    ) -> Result<LocationFingerprintReply, Status> {
        self.host
            .locations_client()
            .get_fingerprint(LocationFingerprintRequest {
                context: Some(self.context()),
                approved_resource_handle: handle.into(),
                algorithm: "sha256".into(),
                require_fresh: true,
                max_entries,
                idempotency_key: key.into(),
            })
            .await
            .map(Response::into_inner)
    }

    async fn diff(
        &self,
        before: &str,
        after: &str,
        page_size: u32,
        cursor: &str,
    ) -> Result<LocationCompareReply, Status> {
        self.host
            .locations_client()
            .compare(LocationCompareRequest {
                context: Some(self.context()),
                before_snapshot_id: before.into(),
                after_snapshot_id: after.into(),
                expected_revision: 0,
                page_size,
                cursor: cursor.into(),
            })
            .await
            .map(Response::into_inner)
    }

    /// Fingerprints `handle` and returns the finished reply.
    async fn take(&self, key: &str, handle: &str) -> LocationFingerprintReply {
        let reply = self.fingerprint(key, handle, 0).await.unwrap();
        assert!(!reply.snapshot_id.is_empty(), "{reply:?}");
        reply
    }
}

fn sha(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn set_time(path: &Path, seconds: u64) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(UNIX_EPOCH + StdDuration::from_secs(seconds))
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_small_place_is_fingerprinted_at_once_with_the_documented_digest() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let reply = fixture.fingerprint("k", &root, 0).await.unwrap();

    assert_eq!(reply.algorithm, ALGORITHM);
    assert!(reply.complete);
    assert_eq!(reply.consistency, "best_effort");
    assert_eq!(reply.availability, InspectionAvailability::Available as i32);
    assert!(!reply.job_id.is_empty() && reply.snapshot_id.starts_with("snap-"));
    assert!(reply.observed_start_at > 0 && reply.observed_end_at >= reply.observed_start_at);
    // The digest is the one the documentation describes, recomputed here from the files.
    let record = |label: &str, directory: bool, size: u64, content: &[u8]| Record {
        label: label.into(),
        approval_id: String::new(),
        path: vec![],
        directory,
        size,
        digest: (!directory).then(|| sha(content)),
        unreadable: false,
        unstable: false,
    };
    let expected = tree_digest(&[
        record("GC", true, 0, b""),
        record("GC/USA", true, 0, b""),
        record("GC/USA/card.gci", false, 4, b"card"),
        record("states", true, 0, b""),
        record("states/s1.sav", false, 5, b"state"),
    ]);
    assert_eq!(reply.digest, expected.to_vec());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_retry_is_the_same_fingerprint_and_a_reused_key_for_another_place_is_refused() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let states = fixture.child(&root, "states").await;
    let first = fixture.fingerprint("same", &root, 0).await.unwrap();
    // The files change, and the retry still answers with the first result, not a second run.
    std::fs::write(saves.join("GC/USA/card.gci"), b"CARD").unwrap();
    let again = fixture.fingerprint("same", &root, 0).await.unwrap();
    assert_eq!(
        (again.snapshot_id, again.digest),
        (first.snapshot_id, first.digest)
    );
    assert_eq!(
        fixture
            .fingerprint("same", &states, 0)
            .await
            .unwrap_err()
            .code(),
        Code::FailedPrecondition
    );
    assert_eq!(
        fixture.fingerprint("", &root, 0).await.unwrap_err().code(),
        Code::InvalidArgument
    );
    let other = fixture
        .host
        .locations_client()
        .get_fingerprint(LocationFingerprintRequest {
            context: Some(fixture.context()),
            approved_resource_handle: root.clone(),
            algorithm: "md5".into(),
            require_fresh: true,
            max_entries: 0,
            idempotency_key: "md5".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(other.code(), Code::InvalidArgument);
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_changed_is_unchanged_and_a_same_size_same_time_edit_is_found() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let card = saves.join("GC/USA/card.gci");
    set_time(&card, 1_600_000_000);
    let first = fixture.take("a", &root).await;
    let second = fixture.take("b", &root).await;
    assert_eq!(first.digest, second.digest);
    let same = fixture
        .diff(&first.snapshot_id, &second.snapshot_id, 0, "")
        .await
        .unwrap();
    assert_eq!((same.status.as_str(), same.changes.len()), ("unchanged", 0));

    // The bytes change; the size and the modification time do not.
    std::fs::write(&card, b"CARD").unwrap();
    set_time(&card, 1_600_000_000);
    let third = fixture.take("c", &root).await;
    assert_ne!(first.digest, third.digest);
    let changed = fixture
        .diff(&first.snapshot_id, &third.snapshot_id, 0, "")
        .await
        .unwrap();
    assert_eq!(changed.status, "changed");
    let [change] = changed.changes.try_into().unwrap();
    assert_eq!(
        (
            change.change_kind.as_str(),
            change.relative_path.as_str(),
            change.previous_size_bytes,
            change.current_size_bytes
        ),
        ("modified", "GC/USA/card.gci", 4, 4)
    );
    // The entry that changed comes with a handle for the next call.
    assert!(!change.entry_handle.is_empty());
    assert_eq!(
        fixture
            .cat(&change.entry_handle, 0, 10)
            .await
            .unwrap()
            .contents,
        b"CARD"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn additions_deletions_empty_folders_and_renames_are_all_found_and_paged() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    std::fs::create_dir(saves.join("empty-old")).unwrap();
    std::fs::write(saves.join("old-name.sav"), b"rename me").unwrap();
    let before = fixture.take("a", &root).await;

    std::fs::remove_dir(saves.join("empty-old")).unwrap();
    std::fs::create_dir(saves.join("empty-new")).unwrap();
    std::fs::rename(saves.join("old-name.sav"), saves.join("new-name.sav")).unwrap();
    std::fs::remove_file(saves.join("states/s1.sav")).unwrap();
    std::fs::write(saves.join("fresh.sav"), b"fresh").unwrap();
    let after = fixture.take("b", &root).await;
    assert_ne!(before.digest, after.digest);

    let all = fixture
        .diff(&before.snapshot_id, &after.snapshot_id, 0, "")
        .await
        .unwrap();
    assert_eq!(all.status, "changed");
    let seen: Vec<_> = all
        .changes
        .iter()
        .map(|c| (c.relative_path.as_str(), c.change_kind.as_str()))
        .collect();
    assert_eq!(
        seen,
        [
            ("empty-new", "added"),
            ("empty-old", "removed"),
            ("fresh.sav", "added"),
            ("new-name.sav", "added"),
            ("old-name.sav", "removed"),
            ("states/s1.sav", "removed"),
        ]
    );
    // What no longer exists has no handle; what does, has one.
    let handles: Vec<bool> = all
        .changes
        .iter()
        .map(|c| !c.entry_handle.is_empty())
        .collect();
    assert_eq!(handles, [true, false, true, true, false, false]);

    // Paged: two at a time, every change once.
    let mut paged = Vec::new();
    let mut cursor = String::new();
    loop {
        let page = fixture
            .diff(&before.snapshot_id, &after.snapshot_id, 2, &cursor)
            .await
            .unwrap();
        assert!(page.changes.len() <= 2);
        paged.extend(page.changes.iter().map(|c| c.relative_path.clone()));
        if page.next_cursor.is_empty() {
            break;
        }
        cursor = page.next_cursor;
    }
    assert_eq!(paged.len(), 6);
    assert_eq!(
        fixture
            .diff(&before.snapshot_id, &after.snapshot_id, 2, "not-a-cursor")
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_that_cannot_be_read_makes_the_fingerprint_incomplete_and_never_unchanged() {
    if running_as_root() {
        return; // root reads everything
    }
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let complete = fixture.take("a", &root).await;
    let locked = saves.join("states/s1.sav");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let partial = fixture.take("b", &root).await;
    let again = fixture.take("c", &root).await;
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();

    assert!(!partial.complete);
    assert!(
        partial.digest.is_empty(),
        "no digest is given for what was not wholly read"
    );
    // Two incomplete fingerprints that look the same are still not "unchanged".
    let verdict = fixture
        .diff(&partial.snapshot_id, &again.snapshot_id, 0, "")
        .await
        .unwrap();
    assert_eq!(verdict.status, "indeterminate");
    let verdict = fixture
        .diff(&complete.snapshot_id, &partial.snapshot_id, 0, "")
        .await
        .unwrap();
    assert_eq!(verdict.status, "indeterminate");
    assert!(
        verdict.changes.is_empty(),
        "the unreadable file is not reported as modified"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_that_changes_while_it_is_read_makes_the_fingerprint_unstable() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let stable = fixture.take("a", &root).await;

    // A game writes the save right after the host read it.
    let target = saves.join("states/s1.sav");
    let written = Arc::new(StdMutex::new(false));
    fixture.snapshots.set_hook({
        let (target, written) = (target.clone(), written.clone());
        move |path| {
            if path.last().map(String::as_str) == Some("s1.sav") {
                *written.lock().unwrap() = true;
                std::fs::write(&target, b"state, but longer").unwrap();
            }
        }
    });
    let unstable = fixture.fingerprint("b", &root, 0).await.unwrap();
    assert!(*written.lock().unwrap());
    assert!(!unstable.complete);
    assert_eq!(unstable.consistency, "unstable");
    assert!(unstable.digest.is_empty());
    let verdict = fixture
        .diff(&stable.snapshot_id, &unstable.snapshot_id, 0, "")
        .await
        .unwrap();
    assert_eq!(verdict.status, "indeterminate");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cut_walk_is_incomplete() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let cut = fixture.fingerprint("a", &root, 2).await.unwrap();
    assert!(!cut.complete && cut.digest.is_empty());
    let whole = fixture.fingerprint("b", &root, 10).await.unwrap();
    assert!(whole.complete);
    assert_eq!(
        fixture
            .diff(&cut.snapshot_id, &whole.snapshot_id, 0, "")
            .await
            .unwrap()
            .status,
        "indeterminate"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_fingerprints_what_its_rules_select_and_a_new_revision_is_another_place() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let created = fixture
        .make(
            "p",
            profile(
                "Cards",
                "rt",
                vec![directory(&root, "Cards", &["**/*.gci"])],
            ),
        )
        .await
        .unwrap();
    let before = fixture.take("a", &created.profile_id).await;
    assert_eq!(before.revision, created.revision);

    // A file the rules do not select changes: the profile's fingerprint does not.
    std::fs::write(saves.join("states/s1.sav"), b"STATE").unwrap();
    let same = fixture.take("b", &created.profile_id).await;
    assert_eq!(before.digest, same.digest);
    // A file they select changes.
    std::fs::write(saves.join("GC/USA/card.gci"), b"CARD").unwrap();
    let changed = fixture.take("c", &created.profile_id).await;
    assert_ne!(before.digest, changed.digest);
    let verdict = fixture
        .diff(&before.snapshot_id, &changed.snapshot_id, 0, "")
        .await
        .unwrap();
    assert_eq!(verdict.status, "changed");
    assert_eq!(verdict.changes[0].relative_path, "d0/GC/USA/card.gci");

    // The profile is edited: what was taken before is of another revision and cannot be compared.
    let edited = fixture
        .patch(
            "u",
            &created.profile_id,
            created.revision,
            &["name"],
            SaveProfile {
                name: "Cards 2".into(),
                ..SaveProfile::default()
            },
        )
        .await
        .unwrap();
    let later = fixture.take("d", &created.profile_id).await;
    assert_eq!(later.revision, edited.revision);
    let verdict = fixture
        .diff(&changed.snapshot_id, &later.snapshot_id, 0, "")
        .await
        .unwrap();
    assert_eq!(verdict.status, "incomparable");
    assert!(verdict.changes.is_empty());
    // Asking at a revision the snapshots are not at is a conflict.
    let stale = fixture
        .host
        .locations_client()
        .compare(LocationCompareRequest {
            context: Some(fixture.context()),
            before_snapshot_id: later.snapshot_id.clone(),
            after_snapshot_id: later.snapshot_id.clone(),
            expected_revision: created.revision,
            page_size: 0,
            cursor: String::new(),
        })
        .await
        .unwrap_err();
    assert_eq!(stale.code(), Code::FailedPrecondition);
}

#[tokio::test(flavor = "multi_thread")]
async fn snapshots_of_two_places_are_incomparable() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let states = fixture.child(&root, "states").await;
    let whole = fixture.take("a", &root).await;
    let part = fixture.take("b", &states).await;
    let verdict = fixture
        .diff(&whole.snapshot_id, &part.snapshot_id, 0, "")
        .await
        .unwrap();
    assert_eq!(verdict.status, "incomparable");
    // A single file has a fingerprint of its own.
    let file = fixture.child(&states, "s1.sav").await;
    let one = fixture.take("c", &file).await;
    assert!(one.complete && one.digest.len() == 32);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_place_that_is_gone_is_reported_without_a_digest_or_a_job() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let away = saves.with_extension("away");
    std::fs::rename(&saves, &away).unwrap();
    let reply = fixture.fingerprint("a", &root, 0).await.unwrap();
    assert_eq!(reply.availability, InspectionAvailability::Missing as i32);
    assert!(reply.job_id.is_empty() && reply.digest.is_empty() && !reply.complete);
    assert_eq!(reply.consistency, "unavailable");
    assert!(!saves.exists(), "nothing is created in its place");
    std::fs::rename(&away, &saves).unwrap();
    // The same key asks again, since nothing was started.
    assert!(fixture.fingerprint("a", &root, 0).await.unwrap().complete);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_long_fingerprint_answers_with_its_job_and_can_be_cancelled_or_waited_for() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    fixture
        .snapshots
        .set_fast_wait(StdDuration::from_millis(100));
    // The reading is held up until the test lets it go.
    let (release, held) = std::sync::mpsc::channel::<()>();
    let held = Arc::new(StdMutex::new(held));
    fixture.snapshots.set_hook({
        let held = held.clone();
        move |_| {
            let _ = held
                .lock()
                .unwrap()
                .recv_timeout(StdDuration::from_secs(20));
        }
    });

    let pending = fixture.fingerprint("slow", &root, 0).await.unwrap();
    assert_eq!(pending.consistency, "pending");
    assert!(
        !pending.job_id.is_empty() && pending.snapshot_id.is_empty() && pending.digest.is_empty()
    );
    assert_eq!(
        fixture.job(&pending.job_id).await.unwrap().state(),
        JobState::Running
    );
    // The plugin may have only one at a time; the retry of the same request is just the job.
    assert_eq!(
        fixture
            .fingerprint("other", &root, 0)
            .await
            .unwrap_err()
            .code(),
        Code::ResourceExhausted
    );
    assert_eq!(
        fixture.fingerprint("slow", &root, 0).await.unwrap().job_id,
        pending.job_id
    );

    // Let it go: the job ends and the same key gives the result.
    for _ in 0..8 {
        let _ = release.send(());
    }
    let mut result = pending.clone();
    for _ in 0..200 {
        result = fixture.fingerprint("slow", &root, 0).await.unwrap();
        if !result.snapshot_id.is_empty() {
            break;
        }
        tokio::time::sleep(StdDuration::from_millis(25)).await;
    }
    assert!(result.complete && result.digest.len() == 32, "{result:?}");
    assert_eq!(result.job_id, pending.job_id);
    let done = fixture.job(&pending.job_id).await.unwrap();
    assert_eq!(
        (done.state(), done.result_code.as_str()),
        (JobState::Succeeded, "fingerprint_ready")
    );
    assert_eq!(done.message, result.snapshot_id);
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_the_job_stops_the_reading_keeps_nothing_and_frees_the_slot() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    fixture
        .snapshots
        .set_fast_wait(StdDuration::from_millis(100));
    let (release, held) = std::sync::mpsc::channel::<()>();
    let held = Arc::new(StdMutex::new(held));
    fixture.snapshots.set_hook({
        let held = held.clone();
        move |_| {
            let _ = held
                .lock()
                .unwrap()
                .recv_timeout(StdDuration::from_secs(20));
        }
    });
    let pending = fixture.fingerprint("slow", &root, 0).await.unwrap();
    // Cancel while the first file is being held; only then let the reading go on and see it stop.
    let (cancelled, ()) = tokio::join!(fixture.cancel_job("c", &pending.job_id), async {
        tokio::time::sleep(StdDuration::from_millis(150)).await;
        for _ in 0..8 {
            let _ = release.send(());
        }
    });
    let cancelled = cancelled.unwrap();
    assert_eq!(cancelled.state(), JobState::Cancelled);

    let reply = fixture.fingerprint("slow", &root, 0).await.unwrap();
    assert_eq!(
        (reply.consistency.as_str(), reply.snapshot_id.as_str()),
        ("cancelled", "")
    );
    // Nothing of it was kept, and the plugin may take a new one at once.
    fixture.snapshots.set_hook(|_| {});
    assert!(fixture.take("fresh", &root).await.complete);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_permission_and_the_owner_of_a_fingerprint_are_checked() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let taken = fixture.take("a", &root).await;
    // A fingerprint that is not this plugin's, or not at all, is not found.
    assert_eq!(
        fixture
            .diff("snap-nothing", &taken.snapshot_id, 0, "")
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    assert_eq!(
        fixture
            .fingerprint("b", "forged", 0)
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    // Without the permission of the place, neither taking nor comparing works.
    fixture
        .api
        .revoke_session_permission("fixture", cap::GAME_SAVES_INSPECT)
        .await;
    assert_eq!(
        fixture.fingerprint("c", &root, 0).await.unwrap_err().code(),
        Code::PermissionDenied
    );
    assert_eq!(
        fixture
            .diff(&taken.snapshot_id, &taken.snapshot_id, 0, "")
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn hashing_needs_no_file_permission_and_changes_nothing() {
    let (fixture, saves) = Fixture::emulator(INSPECT).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;
    let file = saves.join("states/s1.sav");
    set_time(&file, 1_650_000_000);
    let before = std::fs::metadata(&file).unwrap();
    fixture
        .api
        .revoke_session_permission("fixture", cap::FILES_READ)
        .await;
    let taken = fixture.take("a", &root).await;
    assert!(taken.complete);
    let after = std::fs::metadata(&file).unwrap();
    assert_eq!(after.modified().unwrap(), before.modified().unwrap());
    assert_eq!(std::fs::read(&file).unwrap(), b"state");
    assert_eq!(
        fixture.ls(&root).await.unwrap_err().code(),
        Code::PermissionDenied
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn describe_api_lists_the_fingerprint_methods() {
    let (fixture, _) = Fixture::emulator(INSPECT).await;
    let described = fixture.host.describe_api().await.unwrap();
    let served: Vec<_> = described
        .methods
        .iter()
        .filter(|method| method.service == "Locations")
        .map(|method| method.method.as_str())
        .collect();
    assert_eq!(
        served,
        [
            "GetSize",
            "GetAccess",
            "GetLastModified",
            "GetFingerprint",
            "Compare"
        ]
    );
}
