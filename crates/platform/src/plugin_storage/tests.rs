use super::*;

fn store(dir: &Path) -> DurableStore {
    DurableStore::open(dir).unwrap()
}

#[test]
fn a_namespace_that_never_stored_anything_has_no_file() {
    let temp = tempfile::tempdir().unwrap();
    let dir = namespace_dir(temp.path(), "pub", "org.example", "profile");
    assert!(DurableStore::open_existing(&dir).unwrap().is_none());
    assert!(!dir.exists(), "reading must not create the namespace");
    store(&dir);
    assert!(DurableStore::open_existing(&dir).unwrap().is_some());
}

#[test]
fn values_round_trip_and_survive_reopening() {
    let temp = tempfile::tempdir().unwrap();
    let dir = namespace_dir(temp.path(), "pub", "org.example", "profile");
    let mut first = store(&dir);
    assert_eq!(
        first.put("k", b"value", 3, None, 100, QUOTA_BYTES).unwrap(),
        PutResult::Stored {
            usage: charge("k", 5)
        }
    );
    drop(first);
    let second = store(&dir);
    let stored = second.get("k", 100).unwrap().unwrap();
    assert_eq!(stored.value, b"value");
    assert_eq!(stored.revision, 3);
    assert_eq!(second.high_water().unwrap(), 3);
}

#[test]
fn replacing_a_value_charges_only_the_difference() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = store(temp.path());
    store.put("k", &[0; 1000], 1, None, 0, 5000).unwrap();
    // Replacing in place fits although old + new would not.
    assert_eq!(
        store.put("k", &[0; 3000], 2, None, 0, 5000).unwrap(),
        PutResult::Stored {
            usage: charge("k", 3000)
        }
    );
    assert_eq!(store.get("k", 0).unwrap().unwrap().revision, 2);
}

#[test]
fn a_write_over_the_quota_changes_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = store(temp.path());
    let quota = charge("a", 100) + charge("b", 100);
    store.put("a", &[1; 100], 1, None, 0, quota).unwrap();
    store.put("b", &[2; 100], 2, None, 0, quota).unwrap();
    let before = store.usage(0).unwrap();

    // A new key and a larger replacement both fail whole.
    assert_eq!(
        store.put("c", &[3; 1], 3, None, 0, quota).unwrap(),
        PutResult::OverQuota { usage: before }
    );
    assert_eq!(
        store.put("a", &[9; 200], 4, None, 0, quota).unwrap(),
        PutResult::OverQuota { usage: before }
    );
    assert!(store.get("c", 0).unwrap().is_none());
    let a = store.get("a", 0).unwrap().unwrap();
    assert_eq!((a.value, a.revision), (vec![1; 100], 1));
    assert_eq!(store.usage(0).unwrap(), before);
    // The refused writes did not burn revisions.
    assert_eq!(store.high_water().unwrap(), 2);
}

#[test]
fn tiny_entries_are_charged_an_overhead() {
    assert_eq!(charge("k", 0), 1 + ENTRY_OVERHEAD_BYTES);
    let temp = tempfile::tempdir().unwrap();
    let mut store = store(temp.path());
    let quota = 3 * charge("k0", 0);
    for index in 0..3 {
        let key = format!("k{index}");
        assert!(matches!(
            store.put(&key, b"", index + 1, None, 0, quota).unwrap(),
            PutResult::Stored { .. }
        ));
    }
    assert!(matches!(
        store.put("k3", b"", 4, None, 0, quota).unwrap(),
        PutResult::OverQuota { .. }
    ));
}

#[test]
fn expired_entries_are_gone_and_free_their_space() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = store(temp.path());
    store.put("k", b"v", 1, Some(100), 0, QUOTA_BYTES).unwrap();
    assert!(store.get("k", 99).unwrap().is_some());
    assert!(store.get("k", 100).unwrap().is_none());
    assert_eq!(store.usage(100).unwrap(), 0);
    // The next write sweeps the expired row away.
    store.put("other", b"v", 2, None, 100, QUOTA_BYTES).unwrap();
    assert_eq!(store.usage(0).unwrap(), charge("other", 1));
}

#[test]
fn remove_reports_the_revision_it_deleted_and_keeps_the_high_water() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = store(temp.path());
    store.put("k", b"v", 7, None, 0, QUOTA_BYTES).unwrap();
    assert_eq!(store.remove("k").unwrap(), Some(7));
    assert_eq!(store.remove("k").unwrap(), None);
    assert!(store.get("k", 0).unwrap().is_none());
    assert_eq!(store.high_water().unwrap(), 7);
}

#[test]
fn keys_are_measured_in_bytes_not_characters() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = store(temp.path());
    let key = "ação";
    assert_eq!(key.len(), 6);
    assert_eq!(
        store.put(key, b"v", 1, None, 0, QUOTA_BYTES).unwrap(),
        PutResult::Stored {
            usage: charge(key, 1)
        }
    );
    assert_eq!(store.usage(0).unwrap(), charge(key, 1));
}

#[test]
fn a_store_from_a_newer_version_is_refused_untouched() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("ns");
    store(&dir);
    Connection::open(dir.join(STORE_FILE))
        .unwrap()
        .execute_batch("PRAGMA user_version = 99;")
        .unwrap();
    assert!(matches!(
        DurableStore::open(&dir),
        Err(PluginStorageError::NewerVersion)
    ));
}

#[test]
fn a_damaged_file_is_an_error_and_is_not_replaced() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("ns");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join(STORE_FILE),
        b"this is not a database at all, really",
    )
    .unwrap();
    assert!(DurableStore::open(&dir).is_err());
    assert_eq!(
        fs::read(dir.join(STORE_FILE)).unwrap(),
        b"this is not a database at all, really"
    );
}

#[test]
fn directories_never_escape_the_root_and_depend_on_the_identity() {
    let root = Path::new("/data/plugin-data");
    for hostile in ["..", ".", "../..", "a/b", "a\\b", "", "C:\\x", "x\0y", "."] {
        let dir = namespace_dir(root, hostile, hostile, hostile);
        assert!(dir.starts_with(root), "{hostile:?} -> {dir:?}");
        for part in dir.strip_prefix(root).unwrap().components() {
            assert!(
                matches!(part, std::path::Component::Normal(_)),
                "{hostile:?} -> {dir:?}"
            );
        }
    }
    let base = namespace_dir(root, "pub-a", "org.example", "p1");
    assert_eq!(base, root.join("pub-a").join("org.example").join("p1"));
    assert_ne!(base, namespace_dir(root, "pub-b", "org.example", "p1"));
    assert_ne!(base, namespace_dir(root, "pub-a", "org.other", "p1"));
    assert_ne!(base, namespace_dir(root, "pub-a", "org.example", "p2"));
    // Different identities that would collapse after cleaning stay different.
    assert_ne!(
        namespace_dir(root, "a/b", "x", "p"),
        namespace_dir(root, "a%2Fb", "x", "p")
    );
    let long = "x".repeat(300);
    let dir = namespace_dir(root, &long, &long, "p");
    assert!(dir.file_name().is_some());
    for part in dir.strip_prefix(root).unwrap().components() {
        assert!(part.as_os_str().len() <= 255);
    }
}
