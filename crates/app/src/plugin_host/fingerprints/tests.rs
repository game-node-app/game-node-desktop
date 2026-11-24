//! The tree digest, the comparison and the snapshot store (task 13.4), without a plugin session.
//! The service is tested over the real socket in `resources/tests/fingerprints.rs`.

use std::sync::Mutex as StdMutex;

use super::*;

fn digest(byte: u8) -> Option<[u8; 32]> {
    Some([byte; 32])
}

fn file(label: &str, size: u64, byte: u8) -> Record {
    Record {
        label: label.into(),
        approval_id: "a".into(),
        path: label.split('/').map(str::to_owned).collect(),
        directory: false,
        size,
        digest: digest(byte),
        unreadable: false,
        unstable: false,
    }
}

fn folder(label: &str) -> Record {
    Record {
        directory: true,
        size: 0,
        digest: None,
        ..file(label, 0, 0)
    }
}

fn snapshot(id: &str, entries: Vec<Record>) -> Snapshot {
    let complete = entries.iter().all(|e| !e.unreadable && !e.unstable);
    Snapshot {
        id: id.into(),
        permission: "game.saves.inspect",
        scope: "place".into(),
        revision: 0,
        digest: complete.then(|| tree_digest(&entries)),
        entries,
        complete,
        unstable: false,
        started_at: 1,
        finished_at: 2,
    }
}

#[test]
fn the_tree_digest_follows_names_types_sizes_and_contents_and_not_the_order_given() {
    let base = vec![file("a.sav", 3, 1), folder("saves"), file("saves/s1", 5, 2)];
    let reordered = vec![file("saves/s1", 5, 2), file("a.sav", 3, 1), folder("saves")];
    assert_eq!(tree_digest(&base), tree_digest(&reordered));

    let differs = |entries: Vec<Record>| tree_digest(&entries) != tree_digest(&base);
    // A byte of content, a size, a name, a case, a type, an empty folder: each changes it.
    assert!(differs(vec![
        file("a.sav", 3, 9),
        folder("saves"),
        file("saves/s1", 5, 2)
    ]));
    assert!(differs(vec![
        file("a.sav", 4, 1),
        folder("saves"),
        file("saves/s1", 5, 2)
    ]));
    assert!(differs(vec![
        file("b.sav", 3, 1),
        folder("saves"),
        file("saves/s1", 5, 2)
    ]));
    assert!(differs(vec![
        file("A.sav", 3, 1),
        folder("saves"),
        file("saves/s1", 5, 2)
    ]));
    assert!(differs(vec![
        folder("a.sav"),
        folder("saves"),
        file("saves/s1", 5, 2)
    ]));
    let mut with_empty = base.clone();
    with_empty.push(folder("empty"));
    assert!(differs(with_empty));
    // The lengths keep names from running together: "ab"+"c" is not "a"+"bc".
    assert_ne!(
        tree_digest(&[file("ab", 1, 1), file("c", 1, 1)]),
        tree_digest(&[file("a", 1, 1), file("bc", 1, 1)])
    );
}

#[test]
fn the_tree_digest_has_a_fixed_known_value() {
    // A change to the encoding must be a new algorithm id, so the value is pinned.
    let entries = vec![file("a.sav", 3, 1)];
    let mut hash = Sha256::new();
    hash.update(b"gamenode.location.tree.v1\0");
    hash.update(5_u64.to_be_bytes());
    hash.update(b"a.sav");
    hash.update([1_u8]);
    hash.update(3_u64.to_be_bytes());
    hash.update([1_u8; 32]);
    assert_eq!(tree_digest(&entries), <[u8; 32]>::from(hash.finalize()));
    assert_eq!(ALGORITHM, "sha256-tree-v1");
}

#[test]
fn identical_complete_snapshots_are_unchanged() {
    let entries = vec![file("a", 1, 1), folder("d")];
    let result = compare(&snapshot("s1", entries.clone()), &snapshot("s2", entries));
    assert_eq!(
        (result.status, result.changes.len()),
        (Verdict::Unchanged, 0)
    );
}

#[test]
fn a_change_of_bytes_with_the_same_size_is_a_modification() {
    let before = snapshot("s1", vec![file("a", 4, 1)]);
    let after = snapshot("s2", vec![file("a", 4, 2)]);
    let result = compare(&before, &after);
    assert_eq!(result.status, Verdict::Changed);
    assert_eq!(result.changes.len(), 1);
    assert_eq!(
        (
            result.changes[0].kind,
            result.changes[0].previous_size,
            result.changes[0].current_size
        ),
        (Kind::Modified, 4, 4)
    );
}

#[test]
fn additions_deletions_and_empty_folders_are_found_and_a_rename_is_a_removal_plus_an_addition() {
    let before = snapshot(
        "s1",
        vec![
            file("keep", 1, 1),
            file("gone", 2, 2),
            file("old-name", 3, 3),
            folder("empty-old"),
        ],
    );
    let after = snapshot(
        "s2",
        vec![
            file("keep", 1, 1),
            file("new-name", 3, 3),
            file("fresh", 9, 9),
            folder("empty-new"),
        ],
    );
    let result = compare(&before, &after);
    assert_eq!(result.status, Verdict::Changed);
    let seen: Vec<_> = result
        .changes
        .iter()
        .map(|c| (c.label.as_str(), c.kind))
        .collect();
    assert_eq!(
        seen,
        [
            ("empty-new", Kind::Added),
            ("empty-old", Kind::Removed),
            ("fresh", Kind::Added),
            ("gone", Kind::Removed),
            ("new-name", Kind::Added),
            ("old-name", Kind::Removed),
        ]
    );
    // An added entry carries where it is now; a removed one does not.
    assert!(result.changes[0].current.is_some() && result.changes[1].current.is_none());
}

#[test]
fn a_file_that_became_a_folder_is_a_removal_plus_an_addition() {
    let result = compare(
        &snapshot("s1", vec![file("x", 4, 1)]),
        &snapshot("s2", vec![folder("x")]),
    );
    let kinds: Vec<_> = result.changes.iter().map(|c| c.kind).collect();
    assert_eq!(kinds, [Kind::Removed, Kind::Added]);
    assert_eq!(result.status, Verdict::Changed);
}

#[test]
fn an_incomplete_or_unstable_snapshot_never_gives_unchanged() {
    let entries = vec![file("a", 1, 1)];
    let complete = snapshot("s1", entries.clone());

    let mut unreadable = entries.clone();
    unreadable[0].unreadable = true;
    unreadable[0].digest = None;
    let partial = snapshot("s2", unreadable);
    assert!(!partial.complete && partial.digest.is_none());
    // Identical on every readable entry, and still not certified; the unreadable one is not
    // reported as modified either.
    let result = compare(&complete, &partial);
    assert_eq!(
        (result.status, result.changes.len()),
        (Verdict::Indeterminate, 0)
    );
    assert_eq!(compare(&partial, &complete).status, Verdict::Indeterminate);

    let mut cut = snapshot("s3", entries.clone());
    cut.complete = false;
    assert_eq!(compare(&complete, &cut).status, Verdict::Indeterminate);
    let mut unstable = snapshot("s4", entries);
    unstable.unstable = true;
    assert_eq!(compare(&unstable, &complete).status, Verdict::Indeterminate);
    // A real difference is still reported, with the verdict held back.
    let other = snapshot("s5", vec![file("a", 1, 7)]);
    let mut incomplete_other = other.clone();
    incomplete_other.complete = false;
    let result = compare(&complete, &incomplete_other);
    assert_eq!(
        (result.status, result.changes.len()),
        (Verdict::Indeterminate, 1)
    );
}

#[test]
fn snapshots_of_another_place_selection_or_revision_are_incomparable() {
    let a = snapshot("s1", vec![file("a", 1, 1)]);
    let mut other_scope = snapshot("s2", vec![file("a", 1, 1)]);
    other_scope.scope = "another place".into();
    let mut other_revision = snapshot("s3", vec![file("a", 1, 1)]);
    other_revision.revision = 4;
    for other in [&other_scope, &other_revision] {
        let result = compare(&a, other);
        assert_eq!(
            (result.status, result.changes.len()),
            (Verdict::Incomparable, 0)
        );
    }
}

fn owner(key: u8) -> JobOwner {
    JobOwner::for_tests(key, 1)
}

#[test]
fn snapshots_belong_to_their_owner_expire_and_are_bounded() {
    let now = Arc::new(StdMutex::new(Instant::now()));
    let store = Snapshots::default().with_clock({
        let now = now.clone();
        move || *now.lock().unwrap()
    });
    let kept = store.keep(&owner(1), snapshot("snap-mine", vec![]));
    assert_eq!(kept.id, "snap-mine");
    assert!(store.get(&owner(1), "snap-mine").is_some());
    // Another owner, and an unknown id, are the same nothing.
    assert!(store.get(&owner(2), "snap-mine").is_none());
    assert!(store.get(&owner(1), "snap-none").is_none());

    // The oldest goes when the owner has too many.
    for n in 0..MAX_SNAPSHOTS_PER_OWNER {
        *now.lock().unwrap() += Duration::from_secs(1);
        store.keep(&owner(1), snapshot(&format!("snap-{n}"), vec![]));
    }
    assert!(store.get(&owner(1), "snap-mine").is_none());
    assert!(store.get(&owner(1), "snap-0").is_some());

    // And an hour later nothing is left.
    *now.lock().unwrap() += SNAPSHOT_TTL;
    assert!(store.get(&owner(1), "snap-0").is_none());
}

#[test]
fn only_so_many_fingerprints_run_at_once() {
    let store = Arc::new(Snapshots::default());
    let first = store.begin(&owner(1)).unwrap();
    // The same plugin may not run a second one.
    assert_eq!(
        store.begin(&owner(1)).err().unwrap().code(),
        tonic::Code::ResourceExhausted
    );
    let second = store.begin(&owner(2)).unwrap();
    // The host as a whole is limited too.
    assert_eq!(
        store.begin(&owner(3)).err().unwrap().code(),
        tonic::Code::ResourceExhausted
    );
    drop(first);
    let third = store.begin(&owner(3)).unwrap();
    drop((second, third));
    assert!(store.begin(&owner(1)).is_ok());
}

#[test]
fn snapshot_ids_are_host_made_and_never_repeat() {
    let first = new_snapshot_id().unwrap();
    assert!(first.starts_with("snap-") && first.len() == 37);
    assert_ne!(first, new_snapshot_id().unwrap());
}
