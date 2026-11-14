use std::fs;
use std::os::unix::fs::symlink;

use super::*;

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

struct Area {
    temp: tempfile::TempDir,
    root: PathBuf,
}

impl Area {
    /// `<temp>/outside` holds a secret; `<temp>/root` is the approved root.
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        fs::create_dir_all(root.join("saves")).unwrap();
        fs::create_dir_all(temp.path().join("outside")).unwrap();
        fs::write(temp.path().join("outside/secret.txt"), b"secret").unwrap();
        fs::write(root.join("a.txt"), b"hello world").unwrap();
        fs::write(root.join("saves/slot1.sav"), b"save-1").unwrap();
        Self { temp, root }
    }

    fn outside(&self) -> PathBuf {
        self.temp.path().join("outside")
    }

    fn confined(&self) -> Confined {
        self.confined_with(&Reserved::default())
    }

    fn confined_with(&self, reserved: &Reserved) -> Confined {
        let root = approve_root(&self.root, reserved).unwrap();
        Confined::open(&root, reserved).unwrap()
    }
}

fn names(entries: &[Entry]) -> Vec<&str> {
    entries.iter().map(|entry| entry.name.as_str()).collect()
}

#[test]
fn a_root_lists_reads_writes_and_deletes() {
    let area = Area::new();
    let files = area.confined();

    let (entries, more) = files.list(&[], None, 100).unwrap();
    assert!(!more);
    assert_eq!(names(&entries), ["a.txt", "saves"]);
    assert_eq!((entries[0].directory, entries[0].size), (false, 11));
    assert!(entries[1].directory);
    assert!(entries[0].modified > 1_000_000_000);

    let (bytes, eof) = files.read(&path(&["a.txt"]), 6, 100).unwrap();
    assert_eq!((bytes.as_slice(), eof), (&b"world"[..], true));
    let (bytes, eof) = files.read(&path(&["a.txt"]), 0, 5).unwrap();
    assert_eq!((bytes.as_slice(), eof), (&b"hello"[..], false));
    let (bytes, eof) = files.read(&path(&["saves", "slot1.sav"]), 100, 5).unwrap();
    assert_eq!((bytes.len(), eof), (0, true));

    let target = Target::Create {
        directory: &path(&["saves"]),
        name: "slot2.sav",
    };
    assert_eq!(files.write(target, 0, b"new save", false).unwrap(), 8);
    assert_eq!(
        fs::read(area.root.join("saves/slot2.sav")).unwrap(),
        b"new save"
    );
    // Overwrite in place, then truncate where the write ends.
    let existing = path(&["saves", "slot2.sav"]);
    assert_eq!(
        files
            .write(Target::Existing(&existing), 4, b"SAVE!!", false)
            .unwrap(),
        10
    );
    assert_eq!(
        fs::read(area.root.join("saves/slot2.sav")).unwrap(),
        b"new SAVE!!"
    );
    assert_eq!(
        files
            .write(Target::Existing(&existing), 0, b"x", true)
            .unwrap(),
        1
    );
    assert_eq!(fs::read(area.root.join("saves/slot2.sav")).unwrap(), b"x");

    files.delete(&existing).unwrap();
    assert!(!area.root.join("saves/slot2.sav").exists());
    assert_eq!(files.delete(&existing), Err(FileError::NotFound));
}

#[test]
fn listing_pages_by_name() {
    let area = Area::new();
    for index in 0..5 {
        fs::write(area.root.join(format!("f{index}")), b"x").unwrap();
    }
    let files = area.confined();
    let (first, more) = files.list(&[], None, 3).unwrap();
    assert_eq!(names(&first), ["a.txt", "f0", "f1"]);
    assert!(more);
    let (second, more) = files.list(&[], Some("f1"), 3).unwrap();
    assert_eq!(names(&second), ["f2", "f3", "f4"]);
    assert!(more, "saves still follows");
    let (last, more) = files.list(&[], Some("f4"), 3).unwrap();
    assert_eq!((names(&last), more), (vec!["saves"], false));
}

#[test]
fn traversal_and_malformed_paths_are_refused_before_the_disk_is_touched() {
    let area = Area::new();
    let files = area.confined();
    for bad in [
        path(&[".."]),
        path(&["..", "outside", "secret.txt"]),
        path(&["saves", "..", "..", "outside", "secret.txt"]),
        path(&["/etc/passwd"]),
        path(&["a/b"]),
        path(&["saves\\..\\a.txt"]),
        path(&["."]),
        path(&[""]),
        path(&["bad\0name"]),
        path(&["a.txt\n"]),
    ] {
        assert_eq!(files.read(&bad, 0, 10), Err(FileError::Denied), "{bad:?}");
        assert_eq!(files.delete(&bad), Err(FileError::Denied), "{bad:?}");
        assert_eq!(
            files.list(&bad, None, 10).unwrap_err(),
            FileError::Denied,
            "{bad:?}"
        );
        assert_eq!(
            files.write(Target::Existing(&bad), 0, b"x", false),
            Err(FileError::Denied),
            "{bad:?}"
        );
    }
    let too_deep: Vec<String> = (0..=MAX_DEPTH).map(|_| "d".to_owned()).collect();
    assert_eq!(
        files.list(&too_deep, None, 1).unwrap_err(),
        FileError::Denied
    );
    assert!(area.outside().join("secret.txt").exists());
}

#[test]
fn names_for_new_files_must_be_plain_and_portable() {
    let area = Area::new();
    let files = area.confined();
    for bad in [
        "",
        ".",
        "..",
        "a/b",
        "a\\b",
        "..\\x",
        "con",
        "NUL.txt",
        "com1",
        "LPT9.sav",
        "a:b",
        "a?b",
        "trailing.",
        "trailing ",
        "q\"q",
        &"n".repeat(256),
    ] {
        let target = Target::Create {
            directory: &[],
            name: bad,
        };
        assert_eq!(
            files.write(target, 0, b"x", false),
            Err(FileError::Denied),
            "{bad:?}"
        );
    }
    assert_eq!(
        fs::read_dir(&area.root).unwrap().count(),
        2,
        "nothing was created by the refused names"
    );
    for good in [
        "slot.sav",
        "archive.tar.gz",
        ".hidden",
        "ação.sav",
        "COM10",
        "a b",
    ] {
        let target = Target::Create {
            directory: &[],
            name: good,
        };
        files.write(target, 0, b"x", false).unwrap();
    }
}

#[test]
fn links_are_never_followed_at_any_depth() {
    let area = Area::new();
    symlink(
        area.outside().join("secret.txt"),
        area.root.join("link.txt"),
    )
    .unwrap();
    symlink(area.outside(), area.root.join("saves/out")).unwrap();
    symlink(area.root.join("a.txt"), area.root.join("inside-link")).unwrap();
    let files = area.confined();

    // They do not exist for the plugin: not listed.
    assert_eq!(
        names(&files.list(&[], None, 100).unwrap().0),
        ["a.txt", "saves"]
    );
    assert_eq!(
        names(&files.list(&path(&["saves"]), None, 100).unwrap().0),
        ["slot1.sav"]
    );

    // Naming one anyway does not follow it, even when it points inside the root.
    for link in [path(&["link.txt"]), path(&["inside-link"])] {
        assert_eq!(files.read(&link, 0, 10), Err(FileError::Denied), "{link:?}");
        assert_eq!(files.delete(&link), Err(FileError::Denied), "{link:?}");
        assert_eq!(
            files.write(Target::Existing(&link), 0, b"x", false),
            Err(FileError::Denied)
        );
    }
    let through = path(&["saves", "out", "secret.txt"]);
    assert_eq!(files.read(&through, 0, 10), Err(FileError::Denied));
    assert_eq!(
        files.list(&path(&["saves", "out"]), None, 10).unwrap_err(),
        FileError::Denied
    );
    let create = Target::Create {
        directory: &path(&["saves", "out"]),
        name: "dropped.txt",
    };
    assert_eq!(files.write(create, 0, b"x", false), Err(FileError::Denied));
    assert!(!area.outside().join("dropped.txt").exists());
    // Creating a file where a link to outside sits is refused and writes nothing through it.
    let create = Target::Create {
        directory: &[],
        name: "link.txt",
    };
    assert!(files.write(create, 0, b"overwritten", false).is_err());
    assert_eq!(
        fs::read(area.outside().join("secret.txt")).unwrap(),
        b"secret"
    );
}

#[test]
fn a_directory_replaced_by_a_link_after_listing_is_refused_when_used() {
    let area = Area::new();
    let files = area.confined();
    // The plugin listed `saves` and holds its path.
    let slot = path(&["saves", "slot1.sav"]);
    assert_eq!(files.read(&slot, 0, 10).unwrap().0, b"save-1");

    // Then the directory is swapped for a link to a folder holding a same-named file.
    fs::write(area.outside().join("slot1.sav"), b"stolen").unwrap();
    fs::remove_dir_all(area.root.join("saves")).unwrap();
    symlink(area.outside(), area.root.join("saves")).unwrap();

    assert_eq!(files.read(&slot, 0, 10), Err(FileError::Denied));
    assert_eq!(files.delete(&slot), Err(FileError::Denied));
    assert_eq!(
        files.write(Target::Existing(&slot), 0, b"x", false),
        Err(FileError::Denied)
    );
    assert_eq!(
        files.list(&path(&["saves"]), None, 10).unwrap_err(),
        FileError::Denied
    );
    assert_eq!(
        fs::read(area.outside().join("slot1.sav")).unwrap(),
        b"stolen"
    );
}

#[test]
fn a_file_replaced_by_a_link_is_not_followed_on_write_or_delete() {
    let area = Area::new();
    let files = area.confined();
    let file = path(&["a.txt"]);
    fs::remove_file(area.root.join("a.txt")).unwrap();
    symlink(area.outside().join("secret.txt"), area.root.join("a.txt")).unwrap();
    assert_eq!(
        files.write(Target::Existing(&file), 0, b"pwned", true),
        Err(FileError::Denied)
    );
    assert_eq!(files.read(&file, 0, 10), Err(FileError::Denied));
    assert_eq!(
        fs::read(area.outside().join("secret.txt")).unwrap(),
        b"secret"
    );
    // Deleting a path that is a link never removes anything it points to.
    assert_eq!(files.delete(&file), Err(FileError::Denied));
    assert!(area.outside().join("secret.txt").exists());
}

#[test]
fn reserved_host_folders_are_neither_listed_nor_reachable_under_an_approved_parent() {
    let area = Area::new();
    let reserved_dir = area.root.join("saves/gamenode-data");
    fs::create_dir_all(reserved_dir.join("plugin-data")).unwrap();
    fs::write(reserved_dir.join("plugin-data/storage.sqlite3"), b"private").unwrap();
    let reserved = Reserved::new(std::slice::from_ref(&reserved_dir));
    let files = area.confined_with(&reserved);

    // The reserved folder does not appear, and nothing beneath it can be named.
    assert_eq!(
        names(&files.list(&path(&["saves"]), None, 100).unwrap().0),
        ["slot1.sav"]
    );
    for target in [
        path(&["saves", "gamenode-data"]),
        path(&["saves", "gamenode-data", "plugin-data"]),
    ] {
        assert_eq!(
            files.list(&target, None, 10).unwrap_err(),
            FileError::Denied,
            "{target:?}"
        );
    }
    let secret = path(&["saves", "gamenode-data", "plugin-data", "storage.sqlite3"]);
    assert_eq!(files.read(&secret, 0, 10), Err(FileError::Denied));
    assert_eq!(files.delete(&secret), Err(FileError::Denied));
    assert_eq!(
        files.write(Target::Existing(&secret), 0, b"x", true),
        Err(FileError::Denied)
    );
    let create = Target::Create {
        directory: &path(&["saves", "gamenode-data"]),
        name: "x",
    };
    assert_eq!(files.write(create, 0, b"x", false), Err(FileError::Denied));
    assert_eq!(
        fs::read(reserved_dir.join("plugin-data/storage.sqlite3")).unwrap(),
        b"private"
    );
}

#[test]
fn a_root_inside_or_equal_to_a_reserved_folder_cannot_be_approved() {
    let area = Area::new();
    let reserved = Reserved::new(&[area.root.join("saves")]);
    assert_eq!(
        approve_root(&area.root.join("saves"), &reserved).unwrap_err(),
        FileError::Denied
    );
    fs::create_dir_all(area.root.join("saves/deeper")).unwrap();
    assert_eq!(
        approve_root(&area.root.join("saves/deeper"), &reserved).unwrap_err(),
        FileError::Denied
    );
    // Even through a link the user picked: the canonical location decides.
    symlink(area.root.join("saves"), area.temp.path().join("shortcut")).unwrap();
    assert_eq!(
        approve_root(&area.temp.path().join("shortcut"), &reserved).unwrap_err(),
        FileError::Denied
    );
    // A parent that merely contains the reserved folder is fine, with the folder excluded.
    assert!(approve_root(&area.root, &reserved).is_ok());
}

#[test]
fn roots_must_be_real_directories_and_never_the_filesystem_root() {
    let area = Area::new();
    let none = Reserved::default();
    assert_eq!(
        approve_root(Path::new("/"), &none).unwrap_err(),
        FileError::Denied
    );
    assert_eq!(
        approve_root(&area.root.join("a.txt"), &none).unwrap_err(),
        FileError::NotADirectory
    );
    assert_eq!(
        approve_root(&area.root.join("missing"), &none).unwrap_err(),
        FileError::NotFound
    );
    let approved = approve_root(&area.root, &none).unwrap();
    assert_eq!(approved.path, fs::canonicalize(&area.root).unwrap());
}

#[test]
fn a_root_that_was_replaced_is_no_longer_the_approved_folder() {
    let area = Area::new();
    let none = Reserved::default();
    let approved = approve_root(&area.root, &none).unwrap();
    assert!(Confined::open(&approved, &none).is_ok());

    fs::rename(&area.root, area.temp.path().join("moved")).unwrap();
    assert_eq!(
        Confined::open(&approved, &none).err(),
        Some(FileError::RootChanged)
    );
    // A different folder created at the same path is not the approved one either.
    fs::create_dir_all(&area.root).unwrap();
    fs::write(area.root.join("a.txt"), b"impostor").unwrap();
    assert_eq!(
        Confined::open(&approved, &none).err(),
        Some(FileError::RootChanged)
    );
}

#[test]
fn special_files_and_other_non_regular_entries_do_not_exist_for_a_plugin() {
    let area = Area::new();
    let fifo = area.root.join("pipe");
    rustix::fs::mknodat(
        rustix::fs::CWD,
        &fifo,
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::from_bits_truncate(0o600),
        0,
    )
    .unwrap();
    let files = area.confined();
    assert_eq!(
        names(&files.list(&[], None, 100).unwrap().0),
        ["a.txt", "saves"]
    );
    // Opening one must neither hang nor succeed.
    assert!(files.read(&path(&["pipe"]), 0, 10).is_err());
    assert!(
        files
            .write(Target::Existing(&path(&["pipe"])), 0, b"x", false)
            .is_err()
    );
    assert_eq!(files.delete(&path(&["pipe"])), Err(FileError::NotAFile));
}

#[test]
fn directories_are_never_deleted_or_read_as_files() {
    let area = Area::new();
    let files = area.confined();
    let dir = path(&["saves"]);
    assert_eq!(files.delete(&dir), Err(FileError::NotAFile));
    assert_eq!(files.read(&dir, 0, 10), Err(FileError::NotAFile));
    assert_eq!(
        files.write(Target::Existing(&dir), 0, b"x", false),
        Err(FileError::NotAFile)
    );
    assert!(area.root.join("saves/slot1.sav").exists());
    // And files are not directories.
    assert_eq!(
        files.list(&path(&["a.txt"]), None, 10).unwrap_err(),
        FileError::NotADirectory
    );
    assert_eq!(files.read(&[], 0, 10), Err(FileError::NotAFile));
    assert_eq!(files.delete(&[]), Err(FileError::NotAFile));
}

#[test]
fn writes_cannot_leave_holes_or_grow_without_bound() {
    let area = Area::new();
    let files = area.confined();
    let file = path(&["a.txt"]);
    assert_eq!(
        files.write(Target::Existing(&file), 12, b"x", false),
        Err(FileError::OutOfRange)
    );
    assert_eq!(
        files
            .write(Target::Existing(&file), 11, b"!", false)
            .unwrap(),
        12
    );
    assert_eq!(
        files.write(Target::Existing(&file), MAX_FILE_BYTES, b"x", false),
        Err(FileError::TooLarge)
    );
    assert_eq!(
        files.write(Target::Existing(&file), u64::MAX, b"x", false),
        Err(FileError::TooLarge)
    );
    assert_eq!(fs::read(area.root.join("a.txt")).unwrap(), b"hello world!");
    // A new file may not start past its own end either.
    let create = Target::Create {
        directory: &[],
        name: "new",
    };
    assert_eq!(
        files.write(create, 1, b"x", false),
        Err(FileError::OutOfRange)
    );
    assert!(
        !area.root.join("new").exists(),
        "a refused write creates nothing"
    );
}

#[test]
fn a_directory_with_too_many_entries_is_refused_not_cut_short() {
    let area = Area::new();
    let big = area.root.join("big");
    fs::create_dir_all(&big).unwrap();
    for index in 0..(MAX_DIR_ENTRIES + 1) {
        fs::File::create(big.join(format!("{index}"))).unwrap();
    }
    let files = area.confined();
    assert_eq!(
        files.list(&path(&["big"]), None, 10).unwrap_err(),
        FileError::TooLarge
    );
    assert!(files.list(&[], None, 10).is_ok());
}

#[test]
fn errors_never_contain_a_path() {
    let area = Area::new();
    let files = area.confined();
    let root = area.root.to_string_lossy().into_owned();
    for error in [
        files.read(&path(&["missing"]), 0, 1).unwrap_err(),
        files.read(&path(&[".."]), 0, 1).unwrap_err(),
        files.delete(&path(&["saves"])).unwrap_err(),
    ] {
        let text = format!("{error} {error:?}");
        assert!(!text.contains(&root) && !text.contains('/'), "{text}");
    }
}

// ---- Inspection (task 13.3): nothing is read, nothing is changed ----

use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

fn running_as_root() -> bool {
    rustix::process::geteuid().is_root()
}

fn budget<'a>(cancel: &'a AtomicBool, max_entries: usize) -> Budget<'a> {
    Budget {
        max_entries,
        deadline: Instant::now() + Duration::from_secs(30),
        cancel,
    }
}

fn tree(files: &Confined, start: &[String], max_entries: usize) -> (Vec<String>, Walk) {
    let cancel = AtomicBool::new(false);
    let mut seen = Vec::new();
    let walk = files
        .walk_tree(start, &budget(&cancel, max_entries), &mut |path, entry| {
            seen.push(format!(
                "{}{}",
                path.join("/"),
                if entry.directory { "/" } else { "" }
            ));
        })
        .unwrap();
    (seen, walk)
}

#[test]
fn a_walk_visits_every_entry_in_name_order_and_says_it_is_complete() {
    let area = Area::new();
    fs::create_dir_all(area.root.join("saves/states/deep")).unwrap();
    fs::write(area.root.join("saves/states/deep/s9.sav"), b"x").unwrap();
    fs::write(area.root.join("saves/states/s1.sav"), b"y").unwrap();
    let files = area.confined();

    let (seen, walk) = tree(&files, &[], 1000);
    assert_eq!(
        seen,
        [
            "a.txt",
            "saves/",
            "saves/slot1.sav",
            "saves/states/",
            "saves/states/deep/",
            "saves/states/deep/s9.sav",
            "saves/states/s1.sav",
        ]
    );
    assert_eq!(
        walk,
        Walk {
            complete: true,
            unreadable: 0,
            visited: 7,
            stopped: false
        }
    );
    // A walk may start below the root.
    let (below, walk) = tree(&files, &path(&["saves", "states"]), 1000);
    assert_eq!(
        below,
        [
            "saves/states/deep/",
            "saves/states/deep/s9.sav",
            "saves/states/s1.sav"
        ]
    );
    assert!(walk.complete);
}

#[test]
fn a_walk_stops_at_its_budget_and_is_then_not_complete() {
    let area = Area::new();
    let files = area.confined();
    let (seen, walk) = tree(&files, &[], 2);
    assert_eq!(seen.len(), 2);
    assert!(!walk.complete && walk.stopped);

    // Cancelled before it starts: nothing is visited.
    let cancel = AtomicBool::new(true);
    let mut count = 0;
    let walk = files
        .walk_tree(&[], &budget(&cancel, 1000), &mut |_, _| count += 1)
        .unwrap();
    assert_eq!(count, 0);
    assert!(!walk.complete && walk.stopped);

    // A deadline that already passed does the same.
    let cancel = AtomicBool::new(false);
    let expired = Budget {
        max_entries: 1000,
        deadline: Instant::now(),
        cancel: &cancel,
    };
    let walk = files.walk_tree(&[], &expired, &mut |_, _| {}).unwrap();
    assert!(!walk.complete && walk.stopped);
}

#[test]
fn a_walk_never_follows_links_or_leaves_the_root_and_skips_what_is_not_a_file() {
    let area = Area::new();
    symlink(area.outside(), area.root.join("escape")).unwrap();
    symlink(
        area.outside().join("secret.txt"),
        area.root.join("saves/link.sav"),
    )
    .unwrap();
    let files = area.confined();
    let (seen, walk) = tree(&files, &[], 1000);
    assert_eq!(seen, ["a.txt", "saves/", "saves/slot1.sav"]);
    assert!(
        walk.complete,
        "a link does not exist for the plugin, so nothing is missing"
    );
    // The start must be inside the root.
    assert_eq!(
        files
            .walk_tree(
                &path(&["escape"]),
                &budget(&AtomicBool::new(false), 10),
                &mut |_, _| {}
            )
            .unwrap_err(),
        FileError::Denied
    );
    assert_eq!(
        files
            .walk_tree(
                &path(&["missing"]),
                &budget(&AtomicBool::new(false), 10),
                &mut |_, _| {}
            )
            .unwrap_err(),
        FileError::NotFound
    );
}

#[test]
fn a_folder_that_cannot_be_read_is_counted_and_the_walk_goes_on() {
    if running_as_root() {
        return; // root reads everything
    }
    let area = Area::new();
    fs::create_dir_all(area.root.join("locked")).unwrap();
    fs::write(area.root.join("locked/hidden.sav"), b"h").unwrap();
    fs::set_permissions(area.root.join("locked"), fs::Permissions::from_mode(0o000)).unwrap();
    let files = area.confined();
    let (seen, walk) = tree(&files, &[], 1000);
    fs::set_permissions(area.root.join("locked"), fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(seen, ["a.txt", "locked/", "saves/", "saves/slot1.sav"]);
    assert_eq!(
        (walk.complete, walk.unreadable, walk.stopped),
        (false, 1, false)
    );
}

#[test]
fn a_tree_deeper_than_the_limit_is_cut_and_not_complete() {
    let area = Area::new();
    let mut deep = area.root.clone();
    for n in 0..MAX_DEPTH + 2 {
        deep = deep.join(format!("d{n}"));
    }
    fs::create_dir_all(&deep).unwrap();
    let files = area.confined();
    let (seen, walk) = tree(&files, &[], 10_000);
    assert!(seen.len() < MAX_DEPTH + 10);
    assert!(!walk.complete && !walk.stopped);
}

#[test]
fn stat_describes_the_root_a_file_and_a_folder_without_following_links() {
    let area = Area::new();
    symlink(area.outside(), area.root.join("escape")).unwrap();
    let files = area.confined();

    let root = files.stat(&[]).unwrap();
    assert_eq!(
        (root.name.as_str(), root.directory, root.size),
        ("", true, 0)
    );
    let file = files.stat(&path(&["a.txt"])).unwrap();
    assert_eq!(
        (file.name.as_str(), file.directory, file.size),
        ("a.txt", false, 11)
    );
    assert!(!file.read_only);
    assert!(file.modified > 1_000_000_000);
    let folder = files.stat(&path(&["saves"])).unwrap();
    assert!(folder.directory);
    assert_eq!(
        files.stat(&path(&["escape"])).unwrap_err(),
        FileError::Denied
    );
    assert_eq!(
        files.stat(&path(&["nothing"])).unwrap_err(),
        FileError::NotFound
    );
    assert_eq!(
        files.stat(&path(&["..", "outside"])).unwrap_err(),
        FileError::Denied
    );
}

#[test]
fn stat_reports_the_sub_second_part_of_a_modification_time_as_the_system_gave_it() {
    let area = Area::new();
    let file = fs::File::options()
        .write(true)
        .open(area.root.join("a.txt"))
        .unwrap();
    let stamp = std::time::UNIX_EPOCH + Duration::new(1_700_000_000, 123_456_789);
    file.set_modified(stamp).unwrap();
    let entry = area.confined().stat(&path(&["a.txt"])).unwrap();
    assert_eq!(
        (entry.modified, entry.modified_nanos),
        (1_700_000_000, 123_456_789)
    );
    // A whole-second time is reported as zero nanoseconds, not as unknown.
    file.set_modified(std::time::UNIX_EPOCH + Duration::new(1_700_000_001, 0))
        .unwrap();
    let entry = area.confined().stat(&path(&["a.txt"])).unwrap();
    assert_eq!((entry.modified, entry.modified_nanos), (1_700_000_001, 0));
}

#[test]
fn access_tells_the_read_only_metadata_apart_from_what_the_user_may_do() {
    let area = Area::new();
    let files = area.confined();
    let file = area.root.join("a.txt");

    let open = files.access(&path(&["a.txt"])).unwrap();
    assert_eq!(
        (
            open.directory,
            open.read_only,
            open.read,
            open.write,
            open.create,
            open.delete
        ),
        (
            false,
            false,
            Tri::Allowed,
            Tri::Allowed,
            Tri::Unknown,
            Tri::Allowed
        )
    );

    // The mode says read-only, and the user really cannot write (a different thing from the mode,
    // which root ignores).
    fs::set_permissions(&file, fs::Permissions::from_mode(0o444)).unwrap();
    let locked = files.access(&path(&["a.txt"])).unwrap();
    assert!(locked.read_only);
    assert_eq!(locked.read, Tri::Allowed);
    if !running_as_root() {
        assert_eq!(locked.write, Tri::Denied);
        // Deleting a file depends on its folder, not on the file's own mode.
        assert_eq!(locked.delete, Tri::Allowed);
    }

    // The mode allows writing, yet the folder does not let the user delete or create.
    fs::set_permissions(area.root.join("saves"), fs::Permissions::from_mode(0o555)).unwrap();
    let folder = files.access(&path(&["saves"])).unwrap();
    let inside = files.access(&path(&["saves", "slot1.sav"])).unwrap();
    fs::set_permissions(area.root.join("saves"), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(folder.directory);
    assert_eq!(folder.read, Tri::Allowed);
    if !running_as_root() {
        assert_eq!((folder.write, folder.create), (Tri::Denied, Tri::Denied));
        assert_eq!((inside.write, inside.delete), (Tri::Allowed, Tri::Denied));
    }
}

#[test]
fn access_to_the_root_cannot_say_whether_it_can_be_deleted_and_a_link_is_not_followed() {
    let area = Area::new();
    symlink(area.outside(), area.root.join("escape")).unwrap();
    let files = area.confined();
    let root = files.access(&[]).unwrap();
    assert!(root.directory);
    assert_eq!(
        root.delete,
        Tri::Unknown,
        "the root's own folder is outside the approval"
    );
    assert_eq!(
        files.access(&path(&["escape"])).unwrap_err(),
        FileError::Denied
    );
}

#[test]
fn inspecting_changes_nothing_not_even_a_modification_time() {
    let area = Area::new();
    let files = area.confined();
    let file = area.root.join("a.txt");
    let stamp = std::time::UNIX_EPOCH + Duration::new(1_600_000_000, 5);
    fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(stamp)
        .unwrap();
    let before = fs::metadata(&file).unwrap();
    let mode_before = fs::metadata(area.root.join("saves"))
        .unwrap()
        .permissions()
        .mode();

    let _ = tree(&files, &[], 1000);
    files.stat(&path(&["a.txt"])).unwrap();
    files.access(&path(&["a.txt"])).unwrap();
    files.access(&path(&["saves"])).unwrap();

    let after = fs::metadata(&file).unwrap();
    assert_eq!(after.modified().unwrap(), before.modified().unwrap());
    assert_eq!(after.len(), before.len());
    assert_eq!(fs::read(&file).unwrap(), b"hello world");
    assert_eq!(
        fs::metadata(area.root.join("saves"))
            .unwrap()
            .permissions()
            .mode(),
        mode_before
    );
    assert_eq!(
        fs::read_dir(&area.root).unwrap().count(),
        2,
        "no file was created"
    );
}
