use std::os::unix::fs::symlink;

use super::*;
use crate::plugin_host::library::LibrarySource;

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

fn source(key: &str, folder: &Path) -> LibrarySource {
    LibrarySource {
        entry_key: key.into(),
        provider_id: "p".into(),
        game_id: None,
        name: key.into(),
        install_state: crate::plugin_host::library::InstallState::Installed,
        display_name: None,
        metadata_revision: 0,
        artwork_revision: 0,
        install_dir: folder.to_string_lossy().into_owned(),
    }
}

#[test]
fn handles_round_trip_their_approval_and_path_and_only_for_their_session() {
    let access = FileAccess::default();
    let handle = access.seal_handle("s1", "approval-1", &path(&["saves", "slot 1.sav"]));
    assert_eq!(
        access.open_handle("s1", &handle),
        Some(("approval-1".to_owned(), path(&["saves", "slot 1.sav"])))
    );
    let root = access.seal_handle("s1", "approval-1", &[]);
    assert_eq!(
        access.open_handle("s1", &root),
        Some(("approval-1".to_owned(), vec![]))
    );
    assert_eq!(access.open_handle("s2", &handle), None, "another session");
    assert_eq!(
        FileAccess::default().open_handle("s1", &handle),
        None,
        "another process"
    );
    // Neither the approval id nor any name can be read from a handle.
    assert!(!handle.contains("approval") && !handle.contains("saves"));
    assert_ne!(
        handle,
        access.seal_handle("s1", "approval-1", &path(&["saves", "slot 1.sav"]))
    );
}

#[test]
fn a_cursor_is_bound_to_its_session_and_directory_and_is_not_a_handle() {
    let access = FileAccess::default();
    let cursor = access.seal_cursor("s1", "dir-handle", "last-name");
    assert_eq!(
        access.open_cursor("s1", "dir-handle", &cursor).as_deref(),
        Some("last-name")
    );
    assert_eq!(access.open_cursor("s1", "other-dir", &cursor), None);
    assert_eq!(access.open_cursor("s2", "dir-handle", &cursor), None);
    assert_eq!(access.open_handle("s1", &cursor), None);
    let handle = access.seal_handle("s1", "a", &[]);
    assert_eq!(access.open_cursor("s1", &handle, &handle), None);
}

#[test]
fn paths_too_long_for_a_handle_are_refused_up_front() {
    assert!(FileAccess::fits_handle(&path(&["a", "b"])));
    assert!(!FileAccess::fits_handle(&vec!["x".repeat(255); 9]));
}

#[test]
fn only_one_approval_per_namespace_waits_at_a_time() {
    let access = Arc::new(FileAccess::default());
    let first = access.begin_approval([1; 32]).expect("the first may wait");
    assert!(access.begin_approval([1; 32]).is_none());
    assert!(
        access.begin_approval([2; 32]).is_some(),
        "another plugin is independent"
    );
    drop(first);
    assert!(
        access.begin_approval([1; 32]).is_some(),
        "the slot is released"
    );
}

#[test]
fn games_overlap_a_root_when_inside_equal_or_containing_it_even_through_links() {
    let temp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(temp.path()).unwrap();
    for dir in ["games/a", "games/b", "other", "real/c"] {
        std::fs::create_dir_all(base.join(dir)).unwrap();
    }
    // A library folder reached through a link still counts for what it really is.
    symlink(base.join("real/c"), base.join("games/c-link")).unwrap();
    let feed = LibraryFeed::new();
    feed.sync(vec![
        source("a", &base.join("games/a")),
        source("b", &base.join("games/b")),
        source("c", &base.join("games/c-link")),
        source("o", &base.join("other")),
        source("gone", &base.join("games/deleted")),
        source("none", Path::new("")),
    ]);
    let access = FileAccess::default();
    let keys = |root: &Path| affected_keys(&access.overlapping(&feed, root));

    assert_eq!(keys(&base.join("games/a")), ["a"], "equal");
    assert_eq!(
        keys(&base.join("games/a/saves")),
        ["a"],
        "a folder inside a game"
    );
    assert_eq!(
        keys(&base.join("games")),
        ["a", "b", "gone"],
        "a parent holds its games"
    );
    assert_eq!(keys(&base.join("real")), ["c"], "the link's real location");
    assert_eq!(keys(&base.join("real/c")), ["c"]);
    assert_eq!(
        keys(&base),
        ["a", "b", "c", "gone", "o"],
        "the empty folder never matches"
    );
    assert_eq!(keys(&base.join("unrelated")), Vec::<String>::new());
    // A sibling whose name merely starts the same is not inside.
    assert_eq!(keys(&base.join("games/a2")), Vec::<String>::new());
}
