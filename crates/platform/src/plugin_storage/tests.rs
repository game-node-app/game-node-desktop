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
fn persistence_entries_are_logical_ordered_and_track_last_use() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = store(temp.path());
    store
        .put("beta", b"two", 1, None, 100, QUOTA_BYTES)
        .unwrap();
    store
        .put("alpha", b"one", 2, Some(200), 150, QUOTA_BYTES)
        .unwrap();
    store
        .put("expired", b"gone", 3, Some(50), 50, QUOTA_BYTES)
        .unwrap();
    assert!(store.get("beta", 160).unwrap().is_some());

    let entries = store.persistence_entries(180).unwrap();
    assert_eq!(
        entries,
        [
            PersistenceEntry {
                key: "alpha".into(),
                size_bytes: 3,
                last_used_at: 150,
                user_protected: false,
            },
            PersistenceEntry {
                key: "beta".into(),
                size_bytes: 3,
                last_used_at: 160,
                user_protected: false,
            },
        ]
    );
    let remaining = store.persistence_entries(200).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].key, "beta");
    assert!(store.physical_bytes().unwrap() > 0);
}

#[test]
fn retention_policy_prunes_by_age_and_size_without_touching_the_newest_entry() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = store(temp.path());
    for (index, key) in ["old", "middle", "new"].iter().enumerate() {
        let at = 100 + index as i64 * 100;
        store
            .put(key, b"value", index as u64 + 1, None, at, QUOTA_BYTES)
            .unwrap();
    }
    let policy = RetentionPolicy {
        max_age_seconds: 150,
        max_bytes: 0,
    };
    assert_eq!(
        store
            .set_retention_policy("private", policy, 300, 4)
            .unwrap(),
        charge("old", 5)
    );
    assert_eq!(
        store
            .persistence_entries(300)
            .unwrap()
            .into_iter()
            .map(|entry| entry.key)
            .collect::<Vec<_>>(),
        ["middle", "new"]
    );
    assert_eq!(store.retention_policy("private").unwrap(), Some(policy));

    // A smaller byte bound removes the oldest remaining entry first.
    let policy = RetentionPolicy {
        max_age_seconds: 0,
        max_bytes: charge("new", 5),
    };
    store
        .set_retention_policy("private", policy, 300, 5)
        .unwrap();
    assert!(store.get("middle", 300).unwrap().is_none());
    assert!(store.get("new", 300).unwrap().is_some());
    // A write does not prune itself; it remains bounded by the next policy or quota check.
    assert_eq!(
        store
            .put("newest", b"value", 6, None, 300, QUOTA_BYTES)
            .unwrap(),
        PutResult::Stored {
            usage: charge("new", 5) + charge("newest", 5)
        }
    );
    assert!(store.get("newest", 300).unwrap().is_some());
}

#[test]
fn usage_reports_category_records_and_a_namespace_wide_page_estimate() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = store(temp.path());
    store
        .put("alpha", b"one", 1, None, 100, QUOTA_BYTES)
        .unwrap();
    store
        .put("beta", b"two", 2, None, 100, QUOTA_BYTES)
        .unwrap();
    store
        .put("expired", b"gone", 3, Some(150), 100, QUOTA_BYTES)
        .unwrap();
    assert!(store.set_user_protected("beta", true).unwrap());

    let usage = store.store_usage(200).unwrap();
    let expected = charge("alpha", 3) + charge("beta", 3);
    assert_eq!(usage.logical_bytes, expected);
    assert_eq!(usage.categories.len(), 1);
    assert_eq!(
        usage.categories[0],
        CategoryUsage {
            category: "private".into(),
            records: 2,
            logical_bytes: expected,
            protected_records: 1,
        }
    );
    // The physical estimate covers the whole namespace file, not one category.
    assert_eq!(
        usage.physical_bytes_estimate,
        store.physical_bytes().unwrap()
    );
    assert!(usage.physical_bytes_estimate > 0);
}

#[test]
fn protected_records_survive_plugin_deletion_retention_and_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = store(temp.path());
    store
        .put("protected", b"keep", 1, None, 100, QUOTA_BYTES)
        .unwrap();
    store
        .put("unprotected", b"old", 2, None, 100, QUOTA_BYTES)
        .unwrap();
    assert!(store.set_user_protected("protected", true).unwrap());

    // A plugin cannot overwrite, remove, or bulk-delete a protected record.
    assert!(matches!(
        store.put("protected", b"new", 3, None, 100, QUOTA_BYTES),
        Err(PluginStorageError::Protected)
    ));
    assert!(matches!(
        store.remove("protected"),
        Err(PluginStorageError::Protected)
    ));
    assert!(matches!(
        store.delete_persistence_entries(&["protected".to_owned()], 100, 3),
        Err(PluginStorageError::Protected)
    ));
    assert_eq!(store.get("protected", 100).unwrap().unwrap().value, b"keep");

    // Age retention removes the old unprotected record and leaves the protected one.
    store
        .set_retention_policy(
            "private",
            RetentionPolicy {
                max_age_seconds: 50,
                max_bytes: 0,
            },
            200,
            4,
        )
        .unwrap();
    assert!(store.get("unprotected", 200).unwrap().is_none());
    assert!(store.get("protected", 200).unwrap().is_some());

    // Cleanup reclaims the unprotected, expired record only.
    store
        .put("expiring", b"x", 5, Some(210), 200, QUOTA_BYTES)
        .unwrap();
    let cleanup = store.cleanup(220).unwrap();
    assert_eq!(cleanup.removed_records, 1);
    assert_eq!(cleanup.reclaimed_bytes, charge("expiring", 1));
    assert!(store.get("expiring", 220).unwrap().is_none());
    assert!(store.get("protected", 220).unwrap().is_some());

    // Only the host can lift protection; then the normal delete path works again.
    assert!(store.set_user_protected("protected", false).unwrap());
    assert_eq!(store.remove("protected").unwrap(), Some(1));
}

#[test]
fn a_version_twelve_store_gains_host_only_user_protection() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("ns");
    fs::create_dir_all(&dir).unwrap();
    let conn = Connection::open(dir.join(STORE_FILE)).unwrap();
    conn.execute_batch(
        "CREATE TABLE meta (id INTEGER PRIMARY KEY CHECK (id = 1), revision INTEGER NOT NULL) STRICT;
         INSERT INTO meta (id, revision) VALUES (1, 7);
         CREATE TABLE entries (
             key TEXT PRIMARY KEY,
             value BLOB NOT NULL,
             revision INTEGER NOT NULL CHECK (revision >= 1),
             expires_at INTEGER,
             last_used_at INTEGER NOT NULL DEFAULT 0
         ) STRICT;
         INSERT INTO entries (key, value, revision, expires_at, last_used_at)
         VALUES ('legacy', X'00', 7, NULL, 50);
         PRAGMA user_version = 12;",
    )
    .unwrap();
    drop(conn);

    let mut upgraded = store(&dir);
    let entry = &upgraded.persistence_entries(100).unwrap()[0];
    assert_eq!(entry.key, "legacy");
    assert!(!entry.user_protected);
    assert!(upgraded.set_user_protected("legacy", true).unwrap());
    drop(upgraded);

    let reopened = store(&dir);
    assert!(reopened.persistence_entries(100).unwrap()[0].user_protected);
}

#[test]
fn a_version_eleven_store_gains_shared_resources_and_consent() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("ns");
    let mut current = store(&dir);
    current
        .save_profile(&profile("a", "inst", "Card", 1))
        .unwrap();
    drop(current);

    let conn = Connection::open(dir.join(STORE_FILE)).unwrap();
    conn.execute_batch(
        "DROP TABLE shared_resource_consents;
         ALTER TABLE save_profiles DROP COLUMN shared_resources;
         PRAGMA user_version = 11;",
    )
    .unwrap();
    drop(conn);

    let mut upgraded = store(&dir);
    assert_eq!(
        upgraded
            .save_profile_row("a")
            .unwrap()
            .unwrap()
            .shared_resources,
        "[]"
    );
    let consent = SharedResourceConsentRow {
        profile_id: "a".into(),
        resource_id: "memory-card".into(),
        declaration: "declaration".into(),
        scope: "scope".into(),
        profile_revision: 1,
        approved_at: 100,
    };
    upgraded.save_shared_resource_consent(&consent).unwrap();
    assert_eq!(
        upgraded
            .shared_resource_consent("a", "memory-card")
            .unwrap(),
        Some(consent.clone())
    );
    assert!(
        upgraded
            .revoke_shared_resource_consent("a", "memory-card")
            .unwrap()
    );
    assert_eq!(
        upgraded
            .shared_resource_consent("a", "memory-card")
            .unwrap(),
        None
    );
    // Removing the profile also removes its remaining decisions.
    upgraded.save_shared_resource_consent(&consent).unwrap();
    upgraded.delete_save_profile("a", 2).unwrap();
    assert!(upgraded.shared_resource_consents("a").unwrap().is_empty());
}

#[test]
fn deleting_persistence_entries_is_selected_and_advances_the_revision() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = store(temp.path());
    store.put("keep", b"k", 1, None, 0, QUOTA_BYTES).unwrap();
    store
        .put("drop", b"value", 2, None, 0, QUOTA_BYTES)
        .unwrap();
    assert_eq!(
        store
            .delete_persistence_entries(&["drop".into()], 0, 3)
            .unwrap(),
        charge("drop", 5)
    );
    assert_eq!(store.high_water().unwrap(), 3);
    assert!(store.get("keep", 0).unwrap().is_some());
    assert!(store.get("drop", 0).unwrap().is_none());
    // A missing key is not an error when the caller already validated the set.
    assert_eq!(
        store
            .delete_persistence_entries(&["missing".into()], 0, 4)
            .unwrap(),
        0
    );
}

#[test]
fn a_version_ten_store_gains_last_use_and_retention_tables() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("ns");
    let mut current = store(&dir);
    current.put("old", b"v", 1, None, 100, QUOTA_BYTES).unwrap();
    drop(current);

    let conn = Connection::open(dir.join(STORE_FILE)).unwrap();
    conn.execute_batch(
        "DROP TABLE retention_policies;
         ALTER TABLE entries DROP COLUMN last_used_at;
         PRAGMA user_version = 10;",
    )
    .unwrap();
    drop(conn);

    let mut upgraded = store(&dir);
    assert_eq!(
        upgraded
            .persistence_entries(100)
            .unwrap()
            .into_iter()
            .map(|entry| entry.key)
            .collect::<Vec<_>>(),
        ["old"]
    );
    assert_eq!(upgraded.retention_policy("private").unwrap(), None);
    upgraded
        .set_retention_policy("private", RetentionPolicy::default(), 100, 2)
        .unwrap();
    assert_eq!(
        upgraded.retention_policy("private").unwrap(),
        Some(RetentionPolicy::default())
    );
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

fn approval(id: &str, installation: &str, kind: &str) -> FileApproval {
    FileApproval {
        id: id.into(),
        installation_id: installation.into(),
        game_id: "40".into(),
        kind: kind.into(),
        root: "/games/portal".into(),
        volume_token: "opaque-volume-token".into(),
        root_volume: u64::MAX,
        root_node: 12345,
        operations: operation::READ | operation::WRITE,
        affected: vec!["steam:620".into(), "steam:621".into()],
        approved_at: 100,
        revoked_at: None,
        origin: ORIGIN_USER.into(),
    }
}

#[test]
fn approvals_round_trip_survive_reopening_and_are_not_charged_to_the_quota() {
    let temp = tempfile::tempdir().unwrap();
    let dir = namespace_dir(temp.path(), "pub", "org.example", "profile");
    let mut first = store(&dir);
    let saved = approval("a1", "inst-1", "installation");
    first.save_approval(&saved).unwrap();
    first.put("k", b"v", 1, None, 0, QUOTA_BYTES).unwrap();
    drop(first);

    let second = store(&dir);
    assert_eq!(second.approval("a1").unwrap(), Some(saved.clone()));
    assert_eq!(
        second.approval_for("inst-1", "installation").unwrap(),
        Some(saved.clone())
    );
    assert_eq!(second.approval_for("inst-1", "save").unwrap(), None);
    assert_eq!(second.approval("nope").unwrap(), None);
    assert_eq!(second.approvals().unwrap(), vec![saved]);
    assert_eq!(
        second.usage(0).unwrap(),
        charge("k", 1),
        "only entries are charged"
    );
}

#[test]
fn a_new_approval_replaces_the_old_one_for_the_same_installation_and_kind() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = store(temp.path());
    store
        .save_approval(&approval("old", "inst-1", "save"))
        .unwrap();
    store
        .save_approval(&approval("other", "inst-2", "save"))
        .unwrap();
    let mut wider = approval("new", "inst-1", "save");
    wider.operations = operation::READ | operation::WRITE | operation::DELETE;
    store.save_approval(&wider).unwrap();

    assert_eq!(
        store.approval("old").unwrap(),
        None,
        "the replaced id opens nothing"
    );
    assert_eq!(store.approval("new").unwrap(), Some(wider));
    assert_eq!(store.approvals().unwrap().len(), 2);
}

#[test]
fn a_revoked_approval_stays_visible_as_revoked_and_revokes_once() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = store(temp.path());
    store
        .save_approval(&approval("a1", "inst-1", "installation"))
        .unwrap();
    assert!(store.revoke_approval("a1", 500).unwrap());
    assert!(!store.revoke_approval("a1", 600).unwrap());
    assert!(!store.revoke_approval("missing", 600).unwrap());
    assert_eq!(store.approval("a1").unwrap().unwrap().revoked_at, Some(500));
    // Approving again makes a fresh, active record.
    store
        .save_approval(&approval("a2", "inst-1", "installation"))
        .unwrap();
    assert_eq!(
        store
            .approval_for("inst-1", "installation")
            .unwrap()
            .unwrap()
            .revoked_at,
        None
    );
}

#[test]
fn a_version_1_store_gains_the_approvals_table_and_keeps_its_values() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("ns");
    fs::create_dir_all(&dir).unwrap();
    {
        let conn = Connection::open(dir.join(STORE_FILE)).unwrap();
        conn.execute_batch(
            "CREATE TABLE meta (id INTEGER PRIMARY KEY CHECK (id = 1), revision INTEGER NOT NULL) STRICT;
             INSERT INTO meta (id, revision) VALUES (1, 4);
             CREATE TABLE entries (key TEXT PRIMARY KEY, value BLOB NOT NULL,
                 revision INTEGER NOT NULL, expires_at INTEGER) STRICT;
             INSERT INTO entries VALUES ('old', x'01', 4, NULL);
             PRAGMA user_version = 1;",
        )
        .unwrap();
    }
    let mut upgraded = store(&dir);
    assert_eq!(upgraded.get("old", 0).unwrap().unwrap().revision, 4);
    assert_eq!(upgraded.high_water().unwrap(), 4);
    upgraded
        .save_approval(&approval("a1", "inst-1", "installation"))
        .unwrap();
    assert!(upgraded.approval("a1").unwrap().is_some());
}

#[test]
fn a_version_2_store_gains_the_origin_column_and_old_approvals_count_as_the_users() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("ns");
    fs::create_dir_all(&dir).unwrap();
    {
        let conn = Connection::open(dir.join(STORE_FILE)).unwrap();
        conn.execute_batch(
            "CREATE TABLE meta (id INTEGER PRIMARY KEY CHECK (id = 1), revision INTEGER NOT NULL) STRICT;
             INSERT INTO meta (id, revision) VALUES (1, 0);
             CREATE TABLE entries (key TEXT PRIMARY KEY, value BLOB NOT NULL,
                 revision INTEGER NOT NULL, expires_at INTEGER) STRICT;
             CREATE TABLE file_roots (
                 id TEXT NOT NULL UNIQUE, installation_id TEXT NOT NULL, game_id TEXT NOT NULL,
                 kind TEXT NOT NULL, root TEXT NOT NULL, root_volume INTEGER NOT NULL,
                 root_node INTEGER NOT NULL, operations INTEGER NOT NULL, affected TEXT NOT NULL,
                 approved_at INTEGER NOT NULL, revoked_at INTEGER,
                 PRIMARY KEY (installation_id, kind)) STRICT;
             INSERT INTO file_roots VALUES ('old', 'inst', '', 'save', '/g', 1, 2, 1, '[]', 5, NULL);
             PRAGMA user_version = 2;",
        )
        .unwrap();
    }
    let mut upgraded = store(&dir);
    let old = upgraded.approval("old").unwrap().unwrap();
    assert_eq!(old.origin, ORIGIN_USER);
    assert!(
        old.volume_token.is_empty(),
        "migration must never invent identity approval"
    );
    let mut declared = approval("new", "inst-2", "save");
    declared.origin = ORIGIN_DECLARED.into();
    upgraded.save_approval(&declared).unwrap();
    assert_eq!(
        upgraded.approval("new").unwrap().unwrap().origin,
        ORIGIN_DECLARED
    );
}

fn remembered(id: &str, installation: &str, operation: &str) -> IntentApproval {
    IntentApproval {
        id: id.into(),
        game_id: "42".into(),
        installation_id: installation.into(),
        operation: operation.into(),
        target_revision: 7,
        recipe_id: String::new(),
        recipe_revision: 0,
        plan_fingerprint: "v1:approval-plan".into(),
        game_name: "Game".into(),
        approved_at: 100,
    }
}

#[test]
fn remembered_confirmations_are_kept_per_installation_and_operation_and_can_be_revoked() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("ns");
    let mut first = store(&dir);
    first
        .save_intent_approval(&remembered("a", "inst-1", "launch"))
        .unwrap();
    first
        .save_intent_approval(&remembered("b", "inst-1", "install"))
        .unwrap();
    // The same installation and operation again replaces the earlier approval.
    let mut newer = remembered("c", "inst-1", "launch");
    newer.target_revision = 9;
    first.save_intent_approval(&newer).unwrap();
    drop(first);

    let mut second = store(&dir);
    assert_eq!(
        second
            .intent_approval("inst-1", "launch")
            .unwrap()
            .map(|a| (a.id, a.target_revision)),
        Some(("c".into(), 9))
    );
    assert!(
        second
            .intent_approval("inst-2", "launch")
            .unwrap()
            .is_none()
    );
    assert_eq!(second.intent_approvals().unwrap().len(), 2);
    assert!(!second.revoke_intent_approval("nothing").unwrap());
    assert!(second.revoke_intent_approval("c").unwrap());
    assert!(
        second
            .intent_approval("inst-1", "launch")
            .unwrap()
            .is_none()
    );
    assert!(
        second
            .intent_approval("inst-1", "install")
            .unwrap()
            .is_some()
    );
}

#[test]
fn custom_launch_approval_persists_recipe_identity_and_revision() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = store(&temp.path().join("ns"));
    let mut approval = remembered("custom", "inst", "custom_launch");
    approval.recipe_id = "recipe-one".into();
    approval.recipe_revision = 4;
    store.save_intent_approval(&approval).unwrap();
    assert_eq!(
        store.intent_approval("inst", "custom_launch").unwrap(),
        Some(approval)
    );
}

#[test]
fn schema_eight_approvals_upgrade_without_losing_their_target_revision() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("ns");
    let mut current = store(&dir);
    current
        .save_intent_approval(&remembered("old", "inst", "launch"))
        .unwrap();
    drop(current);

    let conn = Connection::open(dir.join(STORE_FILE)).unwrap();
    conn.execute_batch(
        "ALTER TABLE intent_approvals RENAME TO intent_approvals_v9;
         CREATE TABLE intent_approvals (
             id TEXT NOT NULL UNIQUE,
             game_id TEXT NOT NULL,
             installation_id TEXT NOT NULL,
             operation TEXT NOT NULL CHECK (operation IN ('launch', 'install')),
             target_revision INTEGER NOT NULL CHECK (target_revision >= 0),
             game_name TEXT NOT NULL,
             approved_at INTEGER NOT NULL,
             PRIMARY KEY (installation_id, operation)
         ) STRICT;
         INSERT INTO intent_approvals
             (id, game_id, installation_id, operation, target_revision, game_name, approved_at)
         SELECT id, game_id, installation_id, operation, target_revision, game_name, approved_at
         FROM intent_approvals_v9;
         DROP TABLE intent_approvals_v9;
         PRAGMA user_version = 8;",
    )
    .unwrap();
    drop(conn);

    let mut upgraded = store(&dir);
    let old = upgraded.intent_approval("inst", "launch").unwrap().unwrap();
    assert_eq!(old.target_revision, 7);
    assert_eq!(old.recipe_id, "");
    assert_eq!(old.recipe_revision, 0);
    assert!(
        old.plan_fingerprint.is_empty(),
        "legacy approvals must reconfirm"
    );
    let mut custom = remembered("new", "inst", "custom_launch");
    custom.recipe_id = "custom-recipe".into();
    custom.recipe_revision = 3;
    upgraded.save_intent_approval(&custom).unwrap();
    assert_eq!(
        upgraded
            .intent_approval("inst", "custom_launch")
            .unwrap()
            .unwrap(),
        custom
    );
}

#[test]
fn only_launch_custom_launch_and_install_can_be_remembered() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = store(&temp.path().join("ns"));
    assert!(
        store
            .save_intent_approval(&remembered("a", "inst", "restore"))
            .is_err()
    );
}

#[test]
fn a_version_3_store_gains_the_remembered_confirmations_table() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("ns");
    {
        let first = store(&dir);
        drop(first);
        let conn = Connection::open(dir.join(STORE_FILE)).unwrap();
        conn.execute_batch("DROP TABLE intent_approvals; ALTER TABLE file_roots DROP COLUMN volume_token; PRAGMA user_version = 3;")
            .unwrap();
    }
    let mut upgraded = store(&dir);
    upgraded
        .save_intent_approval(&remembered("a", "inst", "launch"))
        .unwrap();
    assert_eq!(upgraded.intent_approvals().unwrap().len(), 1);
}

fn profile(id: &str, installation: &str, name: &str, revision: u64) -> SaveProfileRow {
    SaveProfileRow {
        id: id.into(),
        installation_id: installation.into(),
        game_id: "42".into(),
        name: name.into(),
        runtime_id: "dolphin".into(),
        format_id: String::new(),
        format_version: 0,
        metadata: "{}".into(),
        directories: "[]".into(),
        shared_resources: "[]".into(),
        revision,
        created_at: 100,
    }
}

#[test]
fn save_profiles_round_trip_replace_and_keep_the_revision_high_water() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("ns");
    let mut first = store(&dir);
    assert_eq!(
        first
            .save_profile(&profile("a", "inst", "Card A", 3))
            .unwrap(),
        ProfileSaved::Saved
    );
    let mut renamed = profile("a", "inst", "Card A2", 9);
    renamed.created_at = 999; // the creation time is never rewritten
    first.save_profile(&renamed).unwrap();
    assert_eq!(first.high_water().unwrap(), 9);
    drop(first);

    let mut second = store(&dir);
    let kept = second.save_profile_row("a").unwrap().unwrap();
    assert_eq!(
        (kept.name.as_str(), kept.revision, kept.created_at),
        ("Card A2", 9, 100)
    );
    // Removing keeps the high-water mark, so a recreated profile never repeats a revision.
    assert!(second.delete_save_profile("a", 12).unwrap());
    assert!(!second.delete_save_profile("a", 13).unwrap());
    assert_eq!(second.high_water().unwrap(), 13);
    assert!(second.save_profile_row("a").unwrap().is_none());
}

#[test]
fn a_profile_name_is_unique_per_installation_without_regard_to_case() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = store(&temp.path().join("ns"));
    store
        .save_profile(&profile("a", "inst", "Card A", 1))
        .unwrap();
    assert_eq!(
        store
            .save_profile(&profile("b", "inst", "card a", 2))
            .unwrap(),
        ProfileSaved::NameTaken
    );
    // Nothing of the refused write was kept, high-water mark included.
    assert!(store.save_profile_row("b").unwrap().is_none());
    assert_eq!(store.high_water().unwrap(), 1);
    // Another installation may use the name; a profile may keep its own name.
    assert_eq!(
        store
            .save_profile(&profile("c", "other", "Card A", 3))
            .unwrap(),
        ProfileSaved::Saved
    );
    assert_eq!(
        store
            .save_profile(&profile("a", "inst", "Card A", 4))
            .unwrap(),
        ProfileSaved::Saved
    );
    let ids: Vec<_> = store
        .save_profile_rows()
        .unwrap()
        .into_iter()
        .map(|row| row.id)
        .collect();
    assert_eq!(ids, ["a", "c"]);
}

#[test]
fn a_version_4_store_gains_the_save_profiles_table() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("ns");
    {
        let first = store(&dir);
        drop(first);
        let conn = Connection::open(dir.join(STORE_FILE)).unwrap();
        conn.execute_batch("DROP TABLE save_profiles; ALTER TABLE file_roots DROP COLUMN volume_token; PRAGMA user_version = 4;")
            .unwrap();
    }
    let mut upgraded = store(&dir);
    upgraded
        .save_profile(&profile("a", "inst", "Card", 1))
        .unwrap();
    assert_eq!(upgraded.save_profile_rows().unwrap().len(), 1);
}

#[test]
fn upgrading_schema_seven_preserves_values_and_keeps_backup_destinations_separate_from_private_quota()
 {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let mut original = store(dir);
    original
        .put("key", b"keep", 4, None, 0, QUOTA_BYTES)
        .unwrap();
    drop(original);
    let conn = Connection::open(dir.join(STORE_FILE)).unwrap();
    conn.execute_batch("DROP TABLE backup_sources; PRAGMA user_version=7;")
        .unwrap();
    drop(conn);
    let upgraded = store(dir);
    let usage = upgraded.usage(0).unwrap();
    upgraded
        .remember_backup_source("profile-a", "old-destination")
        .unwrap();
    upgraded
        .remember_backup_source("profile-a", "new-destination")
        .unwrap();
    upgraded
        .remember_backup_source("profile-a", "old-destination")
        .unwrap();
    assert_eq!(upgraded.get("key", 0).unwrap().unwrap().value, b"keep");
    assert_eq!(upgraded.usage(0).unwrap(), usage);
    assert!(upgraded.backup_sources("profile-b").unwrap().is_empty());
    drop(upgraded);
    assert_eq!(
        store(dir).backup_sources("profile-a").unwrap(),
        ["new-destination", "old-destination"]
    );
}

#[test]
fn archived_entries_are_inert_and_need_a_reviewed_reuse() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = store(temp.path());
    store
        .put("kept", b"old", 1, None, 100, QUOTA_BYTES)
        .unwrap();
    store
        .put("expiring", b"gone", 2, Some(180), 100, QUOTA_BYTES)
        .unwrap();
    store
        .put("other", b"live", 3, None, 100, QUOTA_BYTES)
        .unwrap();
    assert!(store.set_user_protected("kept", true).unwrap());

    let archived = store.archive_entries(4).unwrap();
    assert_eq!(archived.records, 3);
    assert_eq!(
        archived.logical_bytes,
        charge("kept", 3) + charge("expiring", 4) + charge("other", 4)
    );
    let usage = store.store_usage(100).unwrap();
    assert_eq!(usage.logical_bytes, 0);
    assert_eq!(usage.categories[0].records, 0);
    assert_eq!(usage.archived_records, 3);
    assert_eq!(usage.archived_logical_bytes, archived.logical_bytes);
    assert_eq!(store.archived_usage().unwrap(), archived);

    // Ordinary reads, lists, deletion and cleanup ignore archived rows.
    for key in ["kept", "expiring", "other"] {
        assert!(store.get(key, 100).unwrap().is_none());
    }
    assert!(store.persistence_entries(100).unwrap().is_empty());
    assert_eq!(store.remove("other").unwrap(), None);
    assert_eq!(store.cleanup(10_000).unwrap().removed_records, 0);
    assert!(matches!(
        store.put("kept", b"new", 4, None, 100, QUOTA_BYTES),
        Err(PluginStorageError::Archived)
    ));

    // Retention and the quota count only live rows; a new key may still be written.
    assert!(matches!(
        store.put("fresh", b"value", 5, None, 100, QUOTA_BYTES),
        Ok(PutResult::Stored { .. })
    ));
    store
        .set_retention_policy(
            "private",
            RetentionPolicy {
                max_age_seconds: 1,
                max_bytes: 0,
            },
            10_000,
            6,
        )
        .unwrap();
    assert_eq!(store.archived_usage().unwrap().records, 3);

    // The host reviewed reuse restores the unexpired rows only; the expired archived row is
    // discarded and the protected flag survives.
    let reused = store.reuse_archived_entries(10_000, 8).unwrap();
    assert_eq!(reused.records, 2);
    assert_eq!(store.store_usage(10_000).unwrap().archived_records, 0);
    assert!(store.get("kept", 10_000).unwrap().is_some());
    assert!(store.get("other", 10_000).unwrap().is_some());
    assert!(store.get("expiring", 10_000).unwrap().is_none());
    assert!(matches!(
        store.put("kept", b"new", 7, None, 10_000, QUOTA_BYTES),
        Err(PluginStorageError::Protected)
    ));
}

#[test]
fn a_version_thirteen_store_gains_host_only_archiving() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("ns");
    fs::create_dir_all(&dir).unwrap();
    let conn = Connection::open(dir.join(STORE_FILE)).unwrap();
    conn.execute_batch(
        "CREATE TABLE meta (id INTEGER PRIMARY KEY CHECK (id = 1), revision INTEGER NOT NULL) STRICT;
         INSERT INTO meta (id, revision) VALUES (1, 7);
         CREATE TABLE entries (
             key TEXT PRIMARY KEY,
             value BLOB NOT NULL,
             revision INTEGER NOT NULL CHECK (revision >= 1),
             expires_at INTEGER,
             last_used_at INTEGER NOT NULL DEFAULT 0,
             user_protected INTEGER NOT NULL DEFAULT 0
         ) STRICT;
         INSERT INTO entries (key, value, revision, expires_at, last_used_at, user_protected)
         VALUES ('legacy', X'00', 7, NULL, 50, 0);
         PRAGMA user_version = 13;",
    )
    .unwrap();
    drop(conn);

    let mut upgraded = store(&dir);
    assert_eq!(upgraded.get("legacy", 100).unwrap().unwrap().revision, 7);
    let outcome = upgraded.archive_entries(8).unwrap();
    assert_eq!(outcome.records, 1);
    assert!(upgraded.get("legacy", 100).unwrap().is_none());
    drop(upgraded);

    let reopened = store(&dir);
    assert!(reopened.get("legacy", 100).unwrap().is_none());
    assert_eq!(reopened.archived_usage().unwrap().records, 1);
}

#[test]
fn namespace_helpers_archive_and_remove_only_one_profile() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let kept = namespace_dir(root, "publisher-a", "org.example", "profile-1");
    let other = namespace_dir(root, "publisher-a", "org.example", "profile-2");
    let mut first = store(&kept);
    first
        .put("key", b"value", 1, None, 100, QUOTA_BYTES)
        .unwrap();
    drop(first);
    let mut second = store(&other);
    second
        .put("key", b"other", 1, None, 100, QUOTA_BYTES)
        .unwrap();
    drop(second);

    assert_eq!(
        namespace_usage(root, "publisher-a", "org.example", "profile-1", 100)
            .unwrap()
            .unwrap()
            .logical_bytes,
        charge("key", 5)
    );
    let archived = archive_namespace(root, "publisher-a", "org.example", "profile-1")
        .unwrap()
        .unwrap();
    assert_eq!(archived.records, 1);
    assert_eq!(
        namespace_archived_usage(root, "publisher-a", "org.example", "profile-1")
            .unwrap()
            .unwrap(),
        archived
    );
    assert_eq!(
        namespace_usage(root, "publisher-a", "org.example", "profile-2", 100)
            .unwrap()
            .unwrap()
            .logical_bytes,
        charge("key", 5)
    );

    assert!(remove_namespace(root, "publisher-a", "org.example", "profile-1").unwrap());
    assert!(!kept.exists());
    assert!(other.exists(), "another profile is untouched");
    assert!(!remove_namespace(root, "publisher-a", "org.example", "profile-1").unwrap());
    assert_eq!(
        namespace_usage(root, "publisher-a", "org.example", "profile-2", 100)
            .unwrap()
            .unwrap()
            .logical_bytes,
        charge("key", 5)
    );
}

#[test]
fn a_reviewed_reclaim_drops_archived_unprotected_entries_only() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = store(temp.path());
    store
        .put("protected", b"keep", 1, None, 100, QUOTA_BYTES)
        .unwrap();
    store
        .put("plain", b"drop", 2, None, 100, QUOTA_BYTES)
        .unwrap();
    assert!(store.set_user_protected("protected", true).unwrap());
    store.archive_entries(3).unwrap();

    let reclaimed = store.reclaim_archived(4).unwrap();
    assert_eq!(reclaimed.records, 1);
    assert_eq!(reclaimed.logical_bytes, charge("plain", 4));
    assert_eq!(store.archived_usage().unwrap().records, 1);
    assert_eq!(store.high_water().unwrap(), 4);
    assert!(
        store.reuse_archived_entries(100, 5).unwrap().records == 1,
        "the protected archived row is still restorable"
    );
    assert!(store.get("protected", 100).unwrap().is_some());
}
