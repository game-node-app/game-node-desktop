use super::*;
use rusqlite::Connection;
use tempfile::TempDir;

fn owner(byte: u8, epoch: u64) -> OwnerScope {
    OwnerScope::new([byte; 32], epoch).unwrap()
}

fn provenance(plugin: &str) -> RecordProvenance {
    RecordProvenance {
        source_plugin_id: plugin.to_owned(),
        source_publisher_fingerprint: format!("publisher-{plugin}"),
        source_profile_id: "profile-1".to_owned(),
        declared_developer: "Example Developer".to_owned(),
        origin: "registry".to_owned(),
        package_version: "1.2.3".to_owned(),
        package_digest: vec![7; 32],
    }
}

fn record<'a>(
    dataset: &'a str,
    resource: Option<&'a str>,
    fields: &'a str,
    provenance: &'a RecordProvenance,
    now: i64,
) -> RecordWrite<'a> {
    RecordWrite {
        dataset_id: dataset,
        category: None,
        class: "custom",
        schema_id: "org.example.catalog.v1",
        schema_version: 1,
        fields_json: fields,
        opaque_payload: &[],
        resource_id: resource,
        observed_at: now,
        sequence: 1,
        source_observation_id: "observation-1",
        expires_at: None,
        now,
        provenance,
        expected_revision: None,
    }
}

fn publish(store: &mut SharedDataStore, owner: OwnerScope, write: RecordWrite<'_>) -> WriteReceipt {
    match store.write(owner, write).unwrap() {
        WriteOutcome::Stored(receipt) => *receipt,
        other => panic!("unexpected write outcome: {other:?}"),
    }
}

fn schema(conn: &Connection) -> Vec<(String, String, String, String)> {
    let mut statement = conn
        .prepare(
            "SELECT type, name, tbl_name, COALESCE(sql, '') FROM sqlite_master
             WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
        )
        .unwrap();
    statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn user_version(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn a_legacy_sqlite_pool_is_renamed_to_the_gnpersistence_extension() {
    let dir = TempDir::new().unwrap();
    let legacy = dir.path().join(LEGACY_FILE_NAME);
    drop(Connection::open(&legacy).unwrap());
    assert!(legacy.is_file());

    let store = SharedDataStore::open_existing(dir.path())
        .unwrap()
        .expect("the legacy pool is migrated and opened");
    assert!(!legacy.exists(), "the legacy file name is not left behind");
    assert!(dir.path().join(FILE_NAME).is_file());
    assert_eq!(
        store.pool_revision().unwrap(),
        0,
        "the migrated pool keeps its schema state"
    );
}

#[test]
fn a_fresh_pool_has_the_current_schema_pragmas_and_no_rows() {
    let dir = TempDir::new().unwrap();
    let store = SharedDataStore::open(dir.path()).unwrap();

    assert_eq!(MIGRATIONS.last().unwrap().version, CURRENT_SCHEMA_VERSION);
    assert_eq!(user_version(&store.conn), CURRENT_SCHEMA_VERSION as i64);
    let journal_mode: String = store
        .conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal_mode.to_lowercase(), "wal");
    let foreign_keys: i64 = store
        .conn
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .unwrap();
    assert_eq!(foreign_keys, 1);

    let tables: Vec<String> = store
        .conn
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        tables,
        [
            "shared_owners",
            "shared_records",
            "shared_retention",
            "shared_state"
        ]
    );
    assert_eq!(store.pool_revision().unwrap(), 0);
    let usage = store.usage(&owner(1, 1), 100).unwrap();
    assert_eq!(usage.records, 0);
    assert_eq!(usage.logical_bytes, 0);
    assert_eq!(usage.max_bytes, MAX_OWNER_LOGICAL_BYTES);
    assert_eq!(usage.max_age_seconds, None);
}

#[test]
fn reading_an_absent_pool_creates_nothing() {
    let dir = TempDir::new().unwrap();
    assert!(
        SharedDataStore::open_existing(dir.path())
            .unwrap()
            .is_none()
    );
    assert!(!dir.path().join(FILE_NAME).exists());
}

#[test]
fn upgrading_the_first_schema_keeps_records_and_adds_no_session_rows() {
    let upgraded_dir = TempDir::new().unwrap();
    {
        let version_one =
            SharedDataStore::open_with(upgraded_dir.path(), &MIGRATIONS[..1]).unwrap();
        assert_eq!(user_version(&version_one.conn), 1);
        version_one
            .conn
            .execute(
                "INSERT INTO shared_owners (owner_key, profile_epoch, updated_at)
                 VALUES (?1, 1, 100)",
                [[9_u8; 32]],
            )
            .unwrap();
        version_one
            .conn
            .execute(
                "INSERT INTO shared_records
                     (record_id, owner_key, dataset_id, resource_id, category, class,
                      schema_id, schema_version, fields_json, opaque_payload,
                      source_plugin_id, source_publisher_fingerprint, source_profile_id,
                      declared_developer, origin, package_version, package_digest,
                      observed_at, received_at, updated_at, expires_at, sequence,
                      source_observation_id, revision, pool_revision, logical_bytes)
                 VALUES ('record-v1', ?1, 'org.example.catalog', NULL, NULL, 'custom',
                         'org.example.catalog.v1', 1, '{\"round\":\"trip\"}', X'00',
                         'producer', 'publisher-producer', 'profile-1', 'Example Developer',
                         'registry', '1.2.3', ?2, 100, 100, 100, NULL, 1,
                         'observation-1', 1, 1, 256)",
                params![[9_u8; 32], vec![7_u8; 32]],
            )
            .unwrap();
        version_one
            .conn
            .execute("UPDATE shared_state SET revision = 1 WHERE id = 1", [])
            .unwrap();
    }

    let store = SharedDataStore::open(upgraded_dir.path()).unwrap();
    assert_eq!(user_version(&store.conn), CURRENT_SCHEMA_VERSION as i64);
    let record = store.get(&owner(9, 1), "record-v1", 200).unwrap().unwrap();
    assert_eq!(record.fields_json, r#"{"round":"trip"}"#);
    assert_eq!(record.opaque_payload, [0]);
    assert_eq!(record.provenance.source_plugin_id, "producer");
    assert_eq!(record.revision, 1);
    assert_eq!(record.pool_revision, 1);
    assert!(!record.user_protected);
    assert!(!record.archived);
    assert_eq!(store.pool_revision().unwrap(), 1);
    let records: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM shared_records", [], |row| row.get(0))
        .unwrap();
    assert_eq!(records, 1, "upgrade preserves durable rows only");
    let session_tables: i64 = store
        .conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name LIKE '%session%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(session_tables, 0);

    let fresh_dir = TempDir::new().unwrap();
    let fresh = SharedDataStore::open(fresh_dir.path()).unwrap();
    assert_eq!(schema(&store.conn), schema(&fresh.conn));
}

#[test]
fn a_newer_or_foreign_file_is_refused_without_writing() {
    let newer_dir = TempDir::new().unwrap();
    let newer_path = newer_dir.path().join(FILE_NAME);
    {
        let conn = Connection::open(&newer_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE marker (id INTEGER) STRICT;
             INSERT INTO marker VALUES (1);
             PRAGMA user_version = 99;",
        )
        .unwrap();
    }
    let result = SharedDataStore::open(newer_dir.path()).map(|_| ());
    assert!(matches!(result, Err(SharedDataError::NewerVersion)));
    let conn = Connection::open(&newer_path).unwrap();
    assert_eq!(user_version(&conn), 99);
    let markers: i64 = conn
        .query_row("SELECT COUNT(*) FROM marker", [], |row| row.get(0))
        .unwrap();
    assert_eq!(markers, 1);

    let foreign_dir = TempDir::new().unwrap();
    {
        let conn = Connection::open(foreign_dir.path().join(FILE_NAME)).unwrap();
        conn.execute_batch("CREATE TABLE foreign_table (id INTEGER) STRICT;")
            .unwrap();
    }
    let result = SharedDataStore::open(foreign_dir.path()).map(|_| ());
    assert!(matches!(result, Err(SharedDataError::ForeignDatabase)));
}

#[test]
fn replacing_a_resource_record_keeps_its_id_and_raises_both_revisions() {
    let dir = TempDir::new().unwrap();
    let mut store = SharedDataStore::open(dir.path()).unwrap();
    let owner = owner(1, 1);
    let provenance = provenance("producer");

    let first = publish(
        &mut store,
        owner,
        record(
            "org.example.catalog",
            Some("disc-1"),
            r#"{"v":1}"#,
            &provenance,
            100,
        ),
    );
    assert!(!first.replaced);
    assert_eq!(first.record.revision, 1);
    assert_eq!(first.record.pool_revision, 1);
    assert_eq!(first.record.received_at, 100);

    let second = publish(
        &mut store,
        owner,
        record(
            "org.example.catalog",
            Some("disc-1"),
            r#"{"v":2}"#,
            &provenance,
            110,
        ),
    );
    assert!(second.replaced);
    assert_eq!(second.record.record_id, first.record.record_id);
    assert_eq!(second.record.revision, 2);
    assert_eq!(second.record.pool_revision, 2);
    assert_eq!(second.record.received_at, 100);
    assert_eq!(second.record.updated_at, 110);
    assert_eq!(second.record.fields_json, r#"{"v":2}"#);

    let listed = store
        .list_dataset(&owner, "org.example.catalog", 200)
        .unwrap();
    assert_eq!(listed.records, [second.record]);
    assert_eq!(listed.pool_revision, 2);
}

#[test]
fn appending_without_a_resource_id_creates_distinct_records_in_a_stable_order() {
    let dir = TempDir::new().unwrap();
    let mut store = SharedDataStore::open(dir.path()).unwrap();
    let owner = owner(2, 1);
    let provenance = provenance("producer");

    let first = publish(
        &mut store,
        owner,
        record("org.example.notes", None, r#"{"n":1}"#, &provenance, 100),
    );
    let second = publish(
        &mut store,
        owner,
        record("org.example.notes", None, r#"{"n":2}"#, &provenance, 100),
    );
    let third = publish(
        &mut store,
        owner,
        record("org.example.notes", None, r#"{"n":3}"#, &provenance, 101),
    );
    assert_ne!(first.record.record_id, second.record.record_id);
    assert_ne!(second.record.record_id, third.record.record_id);

    let listed = store
        .list_dataset(&owner, "org.example.notes", 200)
        .unwrap();
    assert_eq!(
        listed
            .records
            .iter()
            .map(|record| record.record_id.as_str())
            .collect::<Vec<_>>(),
        [
            first.record.record_id.as_str(),
            second.record.record_id.as_str(),
            third.record.record_id.as_str()
        ]
    );
    assert_eq!(listed.pool_revision, 3);

    let replaced = publish(
        &mut store,
        owner,
        record(
            "org.example.notes",
            Some("resource-1"),
            r#"{"n":4}"#,
            &provenance,
            105,
        ),
    );
    let replaced_again = publish(
        &mut store,
        owner,
        record(
            "org.example.notes",
            Some("resource-1"),
            r#"{"n":5}"#,
            &provenance,
            110,
        ),
    );
    assert_eq!(replaced.record.revision, 1);
    assert_eq!(replaced_again.record.revision, 2);
    assert_eq!(replaced_again.record.received_at, 105);
    let listed = store
        .list_dataset(&owner, "org.example.notes", 200)
        .unwrap();
    assert_eq!(
        listed
            .records
            .iter()
            .map(|record| record.record_id.as_str())
            .collect::<Vec<_>>(),
        [
            first.record.record_id.as_str(),
            second.record.record_id.as_str(),
            third.record.record_id.as_str(),
            replaced.record.record_id.as_str()
        ],
        "a replacement keeps its position"
    );
    assert_eq!(listed.pool_revision, 5);
}

#[test]
fn another_owner_cannot_read_or_replace_a_record() {
    let dir = TempDir::new().unwrap();
    let mut store = SharedDataStore::open(dir.path()).unwrap();
    let first_owner = owner(3, 1);
    let second_owner = owner(4, 1);
    let first_provenance = provenance("first");
    let second_provenance = provenance("second");

    let first = publish(
        &mut store,
        first_owner,
        record(
            "org.example.catalog",
            Some("shared-resource"),
            r#"{"owner":"first"}"#,
            &first_provenance,
            100,
        ),
    );
    let second = publish(
        &mut store,
        second_owner,
        record(
            "org.example.catalog",
            Some("shared-resource"),
            r#"{"owner":"second"}"#,
            &second_provenance,
            105,
        ),
    );
    assert_ne!(first.record.record_id, second.record.record_id);
    assert_eq!(
        store
            .get(&first_owner, &second.record.record_id, 200)
            .unwrap(),
        None
    );
    assert_eq!(
        store
            .get(&second_owner, &first.record.record_id, 200)
            .unwrap(),
        None
    );

    let replaced = publish(
        &mut store,
        first_owner,
        record(
            "org.example.catalog",
            Some("shared-resource"),
            r#"{"owner":"first-updated"}"#,
            &first_provenance,
            110,
        ),
    );
    assert_eq!(replaced.record.record_id, first.record.record_id);
    assert_eq!(
        store
            .get(&second_owner, &second.record.record_id, 200)
            .unwrap(),
        Some(second.record)
    );
    let first_records = store
        .list_dataset(&first_owner, "org.example.catalog", 200)
        .unwrap();
    let second_records = store
        .list_dataset(&second_owner, "org.example.catalog", 200)
        .unwrap();
    assert_eq!(first_records.records.len(), 1);
    assert_eq!(second_records.records.len(), 1);
    assert_eq!(
        second_records.records[0].fields_json,
        r#"{"owner":"second"}"#
    );
}

#[test]
fn cross_owner_reads_share_a_dataset_without_disclosing_another_owners_scope() {
    let dir = TempDir::new().unwrap();
    let mut store = SharedDataStore::open(dir.path()).unwrap();
    let first_owner = owner(6, 1);
    let second_owner = owner(7, 1);
    let first_provenance = provenance("first");
    let second_provenance = provenance("second");

    let first = publish(
        &mut store,
        first_owner,
        record(
            "org.example.catalog",
            Some("disc-1"),
            r#"{"title":"one"}"#,
            &first_provenance,
            100,
        ),
    );
    let second = publish(
        &mut store,
        second_owner,
        record(
            "org.example.catalog",
            Some("disc-1"),
            r#"{"title":"two"}"#,
            &second_provenance,
            101,
        ),
    );
    assert!(first.record.record_id.starts_with("durable-"));
    assert!(second.record.record_id.starts_with("durable-"));
    assert_ne!(
        first.record.record_id, second.record.record_id,
        "each producer keeps its own record id"
    );

    // A cross-owner read by id finds the record that an owner-scoped read hides.
    assert_eq!(
        store
            .get(&second_owner, &first.record.record_id, 200)
            .unwrap(),
        None
    );
    assert_eq!(
        store
            .get_any(&first.record.record_id, 200)
            .unwrap()
            .unwrap(),
        first.record
    );

    let listed = store.list_dataset_any("org.example.catalog", 200).unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|record| record.record_id.as_str())
            .collect::<Vec<_>>(),
        [
            first.record.record_id.as_str(),
            second.record.record_id.as_str()
        ],
        "cross-owner lists use the same receipt order as an owner list"
    );
    assert!(
        store
            .list_dataset_any("org.example.other", 200)
            .unwrap()
            .is_empty()
    );

    // Expired and replaced values follow the same rules across owners.
    let mut expiring = record(
        "org.example.catalog",
        Some("disc-2"),
        r#"{"title":"short"}"#,
        &first_provenance,
        100,
    );
    expiring.expires_at = Some(150);
    let expiring = publish(&mut store, first_owner, expiring);
    assert_eq!(
        store
            .list_dataset_any("org.example.catalog", 200)
            .unwrap()
            .len(),
        2,
        "an expired record is not listed"
    );
    assert_eq!(
        store.get_any(&expiring.record.record_id, 200).unwrap(),
        None,
        "an expired record cannot be read by id either"
    );

    let replaced = publish(
        &mut store,
        first_owner,
        record(
            "org.example.catalog",
            Some("disc-1"),
            r#"{"title":"one-updated"}"#,
            &first_provenance,
            120,
        ),
    );
    assert_eq!(replaced.record.record_id, first.record.record_id);
    let listed = store.list_dataset_any("org.example.catalog", 200).unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].record_id, first.record.record_id);
    assert_eq!(listed[0].fields_json, r#"{"title":"one-updated"}"#);
    assert_eq!(listed[0].revision, 2);
}

#[test]
fn a_stale_profile_epoch_fails_closed_for_reads_writes_and_lists() {
    let dir = TempDir::new().unwrap();
    let mut store = SharedDataStore::open(dir.path()).unwrap();
    assert!(matches!(
        OwnerScope::new([1; 32], 0),
        Err(SharedDataError::InvalidOwner)
    ));

    let current = owner(5, 10);
    let stale = owner(5, 9);
    let provenance = provenance("producer");
    let stored = publish(
        &mut store,
        current,
        record(
            "org.example.catalog",
            Some("resource"),
            r#"{"v":1}"#,
            &provenance,
            100,
        ),
    );

    assert!(matches!(
        store.get(&stale, &stored.record.record_id, 100).map(|_| ()),
        Err(SharedDataError::StaleProfileEpoch)
    ));
    assert!(matches!(
        store
            .list_dataset(&stale, "org.example.catalog", 100)
            .map(|_| ()),
        Err(SharedDataError::StaleProfileEpoch)
    ));
    assert!(matches!(
        store.write(
            stale,
            record(
                "org.example.catalog",
                Some("resource"),
                r#"{"v":2}"#,
                &provenance,
                110
            )
        ),
        Err(SharedDataError::StaleProfileEpoch)
    ));
    assert_eq!(
        store
            .get(&current, &stored.record.record_id, 110)
            .unwrap()
            .unwrap(),
        stored.record
    );

    let newer = owner(5, 11);
    let replaced = publish(
        &mut store,
        newer,
        record(
            "org.example.catalog",
            Some("resource"),
            r#"{"v":2}"#,
            &provenance,
            120,
        ),
    );
    assert_eq!(replaced.record.revision, 2);
    assert!(matches!(
        store
            .get(&current, &stored.record.record_id, 130)
            .map(|_| ()),
        Err(SharedDataError::StaleProfileEpoch)
    ));
    assert!(
        store
            .get(&newer, &stored.record.record_id, 130)
            .unwrap()
            .is_some()
    );
}

#[test]
fn revision_conflicts_change_nothing_and_do_not_advance_the_pool() {
    let dir = TempDir::new().unwrap();
    let mut store = SharedDataStore::open(dir.path()).unwrap();
    let owner = owner(6, 1);
    let provenance = provenance("producer");

    let stored = publish(
        &mut store,
        owner,
        record(
            "org.example.catalog",
            Some("resource"),
            r#"{"v":1}"#,
            &provenance,
            100,
        ),
    );

    let mut conflicting = record(
        "org.example.catalog",
        Some("resource"),
        r#"{"v":2}"#,
        &provenance,
        110,
    );
    conflicting.expected_revision = Some(0);
    assert_eq!(
        store.write(owner, conflicting).unwrap(),
        WriteOutcome::Conflict {
            current_revision: 1
        }
    );
    assert_eq!(
        store
            .get(&owner, &stored.record.record_id, 110)
            .unwrap()
            .unwrap(),
        stored.record
    );
    assert_eq!(store.pool_revision().unwrap(), 1);

    let mut accepted = record(
        "org.example.catalog",
        Some("resource"),
        r#"{"v":2}"#,
        &provenance,
        120,
    );
    accepted.expected_revision = Some(1);
    let replaced = publish(&mut store, owner, accepted);
    assert_eq!(replaced.record.revision, 2);
    assert_eq!(replaced.record.pool_revision, 2);

    let mut append_conflict = record("org.example.notes", None, r#"{"v":1}"#, &provenance, 130);
    append_conflict.expected_revision = Some(1);
    assert_eq!(
        store.write(owner, append_conflict).unwrap(),
        WriteOutcome::Conflict {
            current_revision: 0
        }
    );

    let mut missing_conflict = record(
        "org.example.catalog",
        Some("missing"),
        r#"{"v":1}"#,
        &provenance,
        130,
    );
    missing_conflict.expected_revision = Some(2);
    assert_eq!(
        store.write(owner, missing_conflict).unwrap(),
        WriteOutcome::Conflict {
            current_revision: 0
        }
    );
    assert_eq!(store.pool_revision().unwrap(), 2);
}

#[test]
fn expired_records_disappear_from_reads_and_lists_and_prune_reclaims_them() {
    let dir = TempDir::new().unwrap();
    let mut store = SharedDataStore::open(dir.path()).unwrap();
    let owner = owner(7, 1);
    let provenance = provenance("producer");

    let mut expiring = record(
        "org.example.catalog",
        Some("resource"),
        r#"{"v":1}"#,
        &provenance,
        100,
    );
    expiring.expires_at = Some(110);
    let stored = publish(&mut store, owner, expiring);
    assert!(
        store
            .get(&owner, &stored.record.record_id, 109)
            .unwrap()
            .is_some()
    );
    assert_eq!(
        store.get(&owner, &stored.record.record_id, 110).unwrap(),
        None
    );
    let listed = store
        .list_dataset(&owner, "org.example.catalog", 110)
        .unwrap();
    assert!(listed.records.is_empty());
    assert_eq!(listed.pool_revision, 1);
    let usage = store.usage(&owner, 110).unwrap();
    assert_eq!(usage.records, 0);
    assert_eq!(usage.logical_bytes, 0);

    let pruned = store.prune(&owner, 110).unwrap();
    assert_eq!(
        pruned,
        PruneOutcome {
            removed_records: 1,
            reclaimed_bytes: stored.record.size_bytes,
            pool_revision: 2,
        }
    );
    assert_eq!(
        store.get(&owner, &stored.record.record_id, 109).unwrap(),
        None
    );
}

#[test]
fn a_quota_prune_on_write_removes_only_the_owners_oldest_unprotected_record() {
    let dir = TempDir::new().unwrap();
    let mut store = SharedDataStore::open(dir.path()).unwrap();
    let pruned_owner = owner(8, 1);
    let other_owner = owner(9, 1);
    let pruned_provenance = provenance("pruned");
    let other_provenance = provenance("other");

    let first = publish(
        &mut store,
        pruned_owner,
        record(
            "org.example.catalog",
            Some("resource-1"),
            r#"{"payload":"same-size"}"#,
            &pruned_provenance,
            100,
        ),
    );
    let other = publish(
        &mut store,
        other_owner,
        record(
            "org.example.catalog",
            Some("resource-1"),
            r#"{"payload":"same-size"}"#,
            &other_provenance,
            105,
        ),
    );
    let second = publish(
        &mut store,
        pruned_owner,
        record(
            "org.example.catalog",
            Some("resource-2"),
            r#"{"payload":"same-size"}"#,
            &pruned_provenance,
            110,
        ),
    );
    assert_eq!(first.record.size_bytes, second.record.size_bytes);
    assert!(
        store
            .set_user_protected(&pruned_owner, &first.record.record_id, true, 115)
            .unwrap()
    );
    let limit = first.record.size_bytes + second.record.size_bytes;
    store
        .set_retention(
            &pruned_owner,
            RetentionPolicy {
                max_age_seconds: None,
                max_bytes: Some(limit),
            },
            115,
        )
        .unwrap();

    let third = publish(
        &mut store,
        pruned_owner,
        record(
            "org.example.catalog",
            Some("resource-3"),
            r#"{"payload":"same-size"}"#,
            &pruned_provenance,
            120,
        ),
    );
    assert_eq!(third.pruned_records, 1);
    assert_eq!(third.reclaimed_bytes, second.record.size_bytes);
    assert!(
        store
            .get(&pruned_owner, &second.record.record_id, 130)
            .unwrap()
            .is_none()
    );
    let listed = store
        .list_dataset(&pruned_owner, "org.example.catalog", 130)
        .unwrap();
    assert_eq!(
        listed
            .records
            .iter()
            .map(|record| record.record_id.as_str())
            .collect::<Vec<_>>(),
        [
            first.record.record_id.as_str(),
            third.record.record_id.as_str()
        ]
    );
    assert!(listed.records[0].user_protected);
    assert_eq!(
        store.usage(&pruned_owner, 130).unwrap().logical_bytes,
        first.record.size_bytes + third.record.size_bytes
    );

    assert_eq!(
        store
            .get(&other_owner, &other.record.record_id, 130)
            .unwrap(),
        Some(other.record.clone())
    );
    assert_eq!(store.usage(&other_owner, 130).unwrap().records, 1);
    assert_eq!(
        store.usage(&other_owner, 130).unwrap().logical_bytes,
        other.record.size_bytes
    );
}

#[test]
fn age_retention_prunes_only_this_owner_and_keeps_protected_rows() {
    let dir = TempDir::new().unwrap();
    let mut store = SharedDataStore::open(dir.path()).unwrap();
    let pruned_owner = owner(10, 1);
    let other_owner = owner(11, 1);
    let pruned_provenance = provenance("pruned");
    let other_provenance = provenance("other");

    let protected = publish(
        &mut store,
        pruned_owner,
        record(
            "org.example.catalog",
            Some("protected"),
            r#"{"payload":"old"}"#,
            &pruned_provenance,
            100,
        ),
    );
    let stale = publish(
        &mut store,
        pruned_owner,
        record(
            "org.example.catalog",
            Some("stale"),
            r#"{"payload":"old"}"#,
            &pruned_provenance,
            105,
        ),
    );
    let fresh = publish(
        &mut store,
        pruned_owner,
        record(
            "org.example.catalog",
            Some("fresh"),
            r#"{"payload":"old"}"#,
            &pruned_provenance,
            195,
        ),
    );
    let other = publish(
        &mut store,
        other_owner,
        record(
            "org.example.catalog",
            Some("other"),
            r#"{"payload":"old"}"#,
            &other_provenance,
            100,
        ),
    );
    assert!(
        store
            .set_user_protected(&pruned_owner, &protected.record.record_id, true, 196)
            .unwrap()
    );

    let pruned = store
        .set_retention(
            &pruned_owner,
            RetentionPolicy {
                max_age_seconds: Some(10),
                max_bytes: None,
            },
            200,
        )
        .unwrap();
    assert_eq!(pruned.removed_records, 1);
    assert_eq!(pruned.reclaimed_bytes, stale.record.size_bytes);
    let listed = store
        .list_dataset(&pruned_owner, "org.example.catalog", 200)
        .unwrap();
    assert_eq!(
        listed
            .records
            .iter()
            .map(|record| record.record_id.as_str())
            .collect::<Vec<_>>(),
        [
            protected.record.record_id.as_str(),
            fresh.record.record_id.as_str()
        ]
    );
    let usage = store.usage(&pruned_owner, 200).unwrap();
    assert_eq!(usage.records, 2);
    assert_eq!(usage.max_age_seconds, Some(10));
    assert_eq!(
        store
            .get(&other_owner, &other.record.record_id, 200)
            .unwrap(),
        Some(other.record)
    );
    assert_eq!(store.usage(&other_owner, 200).unwrap().records, 1);
}

#[test]
fn usage_reports_count_bytes_and_effective_limits() {
    let dir = TempDir::new().unwrap();
    let mut store = SharedDataStore::open(dir.path()).unwrap();
    let owner = owner(12, 1);
    let provenance = provenance("producer");
    let stored = publish(
        &mut store,
        owner,
        record(
            "org.example.catalog",
            Some("resource"),
            r#"{"v":1}"#,
            &provenance,
            100,
        ),
    );

    let usage = store.usage(&owner, 100).unwrap();
    assert_eq!(usage.records, 1);
    assert_eq!(usage.logical_bytes, stored.record.size_bytes);
    assert_eq!(usage.max_bytes, MAX_OWNER_LOGICAL_BYTES);
    assert_eq!(usage.max_age_seconds, None);
    assert_eq!(usage.oldest_updated_at, Some(100));
    assert!(usage.physical_bytes_estimate > 0);
    assert!(!format!("{usage:?}").contains(dir.path().to_str().unwrap()));

    store
        .set_retention(
            &owner,
            RetentionPolicy {
                max_age_seconds: Some(60),
                max_bytes: Some(4096),
            },
            110,
        )
        .unwrap();
    let usage = store.usage(&owner, 110).unwrap();
    assert_eq!(usage.records, 1);
    assert_eq!(usage.max_bytes, 4096);
    assert_eq!(usage.max_age_seconds, Some(60));
}

#[test]
fn deleting_one_owners_records_reclaims_its_bytes_and_keeps_another_owner() {
    let dir = TempDir::new().unwrap();
    let mut store = SharedDataStore::open(dir.path()).unwrap();
    let deleted_owner = owner(14, 1);
    let other_owner = owner(15, 1);
    let producer_provenance = provenance("producer");
    let other_provenance = provenance("other");

    let first = publish(
        &mut store,
        deleted_owner,
        record(
            "org.example.catalog",
            Some("resource-a"),
            r#"{"v":1}"#,
            &producer_provenance,
            100,
        ),
    );
    let second = publish(
        &mut store,
        deleted_owner,
        record(
            "org.example.catalog",
            None,
            r#"{"v":2}"#,
            &producer_provenance,
            101,
        ),
    );
    let other = publish(
        &mut store,
        other_owner,
        record(
            "org.example.catalog",
            Some("resource-b"),
            r#"{"other":true}"#,
            &other_provenance,
            102,
        ),
    );
    store
        .set_retention(
            &deleted_owner,
            RetentionPolicy {
                max_age_seconds: Some(60),
                max_bytes: Some(4096),
            },
            103,
        )
        .unwrap();
    store
        .set_user_protected(&deleted_owner, &first.record.record_id, true, 103)
        .unwrap();

    let before = store.usage(&deleted_owner, 104).unwrap();
    let other_before = store.usage(&other_owner, 104).unwrap();
    assert_eq!(before.records, 2);
    assert_eq!(
        before.logical_bytes,
        first.record.size_bytes + second.record.size_bytes
    );

    let deleted = store.delete_owner(&deleted_owner, 104).unwrap();
    assert_eq!(deleted.removed_records, 2);
    assert_eq!(deleted.reclaimed_bytes, before.logical_bytes);
    assert_eq!(deleted.pool_revision, store.pool_revision().unwrap());

    let after = store.usage(&deleted_owner, 104).unwrap();
    assert_eq!(after.records, 0);
    assert_eq!(after.logical_bytes, 0);
    assert_eq!(after.max_bytes, 4096, "the retention policy stays");
    assert_eq!(after.max_age_seconds, Some(60));
    assert_eq!(
        store
            .get(&other_owner, &other.record.record_id, 104)
            .unwrap(),
        Some(other.record.clone())
    );
    let other_after = store.usage(&other_owner, 104).unwrap();
    assert_eq!(other_after.records, other_before.records);
    assert_eq!(other_after.logical_bytes, other_before.logical_bytes);
    assert_eq!(other_after.max_bytes, MAX_OWNER_LOGICAL_BYTES);

    let again = store.delete_owner(&deleted_owner, 105).unwrap();
    assert_eq!(again.removed_records, 0);
    assert_eq!(again.reclaimed_bytes, 0);
    assert_eq!(again.pool_revision, deleted.pool_revision);
}

#[test]
fn deleting_with_a_stale_profile_epoch_changes_nothing() {
    let dir = TempDir::new().unwrap();
    let mut store = SharedDataStore::open(dir.path()).unwrap();
    let stale = owner(16, 1);
    let current = owner(16, 2);
    let provenance = provenance("producer");
    publish(
        &mut store,
        current,
        record(
            "org.example.catalog",
            Some("resource"),
            r#"{"v":1}"#,
            &provenance,
            100,
        ),
    );

    assert!(matches!(
        store.delete_owner(&stale, 101),
        Err(SharedDataError::StaleProfileEpoch)
    ));
    assert_eq!(store.usage(&current, 101).unwrap().records, 1);
}

#[test]
fn archiving_retains_only_the_owners_rows_until_a_reviewed_reuse() {
    let dir = TempDir::new().unwrap();
    let mut store = SharedDataStore::open(dir.path()).unwrap();
    let archived_owner = owner(17, 1);
    let other_owner = owner(18, 1);
    let archived_provenance = provenance("producer");
    let other_provenance = provenance("other");

    let first = publish(
        &mut store,
        archived_owner,
        record(
            "org.example.catalog",
            Some("disc-1"),
            r#"{"title":"Dolphin"}"#,
            &archived_provenance,
            100,
        ),
    );
    let second = publish(
        &mut store,
        archived_owner,
        record(
            "org.example.catalog",
            None,
            r#"{"title":"Second"}"#,
            &archived_provenance,
            101,
        ),
    );
    let other = publish(
        &mut store,
        other_owner,
        record(
            "org.example.catalog",
            Some("disc-2"),
            r#"{"title":"Other"}"#,
            &other_provenance,
            102,
        ),
    );
    let archived_bytes = first.record.size_bytes + second.record.size_bytes;
    let other_bytes = other.record.size_bytes;
    let pool_before = store.pool_revision().unwrap();

    let outcome = store.archive_owner(&archived_owner, 110).unwrap();
    assert_eq!(outcome.records, 2);
    assert_eq!(outcome.pool_revision, pool_before + 1);

    // Ordinary owner and cross-owner reads skip archived rows.
    assert!(
        store
            .get(&archived_owner, &first.record.record_id, 110)
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .get_any(&first.record.record_id, 110)
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .list_dataset(&archived_owner, "org.example.catalog", 110)
            .unwrap()
            .records
            .is_empty(),
        "the archived owner sees no live row"
    );
    let any = store.list_dataset_any("org.example.catalog", 110).unwrap();
    assert_eq!(any.len(), 1, "only the other owner's row stays visible");
    assert_eq!(any[0].record_id, other.record.record_id);
    assert!(!any[0].archived);

    let usage = store.usage(&archived_owner, 110).unwrap();
    assert_eq!(usage.records, 0);
    assert_eq!(usage.logical_bytes, 0);
    assert_eq!(usage.archived_records, 2);
    assert_eq!(usage.archived_logical_bytes, archived_bytes);
    let other_usage = store.usage(&other_owner, 110).unwrap();
    assert_eq!(other_usage.records, 1);
    assert_eq!(other_usage.logical_bytes, other_bytes);
    assert_eq!(other_usage.archived_records, 0);

    // A reinstall cannot replace the archived resource invisibly, and retention never prunes it.
    let mut replacement = record(
        "org.example.catalog",
        Some("disc-1"),
        r#"{"title":"Replacement"}"#,
        &archived_provenance,
        120,
    );
    replacement.expected_revision = None;
    assert!(matches!(
        store.write(archived_owner, replacement),
        Err(SharedDataError::ArchivedRecordExists)
    ));
    assert_eq!(store.pool_revision().unwrap(), outcome.pool_revision);
    let pruned = store.prune(&archived_owner, 130).unwrap();
    assert_eq!(pruned.removed_records, 0);
    assert_eq!(
        store.usage(&archived_owner, 130).unwrap().archived_records,
        2
    );

    // An explicit host review is the only path back to ordinary reads.
    let reused = store.reuse_archived_owner(&archived_owner, 140).unwrap();
    assert_eq!(reused.records, 2);
    assert_eq!(reused.pool_revision, pruned.pool_revision + 1);
    let listed = store
        .list_dataset(&archived_owner, "org.example.catalog", 140)
        .unwrap();
    assert_eq!(listed.records.len(), 2);
    assert!(listed.records.iter().all(|record| !record.archived));
    assert_eq!(
        store
            .get(&archived_owner, &first.record.record_id, 140)
            .unwrap()
            .map(|record| record.fields_json),
        Some(r#"{"title":"Dolphin"}"#.to_owned())
    );
    let usage = store.usage(&archived_owner, 140).unwrap();
    assert_eq!(usage.records, 2);
    assert_eq!(usage.logical_bytes, archived_bytes);
    assert_eq!(usage.archived_records, 0);
    assert_eq!(
        store
            .get(&other_owner, &other.record.record_id, 140)
            .unwrap(),
        Some(other.record.clone())
    );
    assert_eq!(store.usage(&other_owner, 140).unwrap().records, 1);
}

#[test]
fn archiving_with_a_stale_profile_epoch_changes_nothing() {
    let dir = TempDir::new().unwrap();
    let mut store = SharedDataStore::open(dir.path()).unwrap();
    let stale = owner(19, 1);
    let current = owner(19, 2);
    publish(
        &mut store,
        current,
        record(
            "org.example.catalog",
            Some("resource"),
            r#"{"v":1}"#,
            &provenance("producer"),
            100,
        ),
    );

    assert!(matches!(
        store.archive_owner(&stale, 101),
        Err(SharedDataError::StaleProfileEpoch)
    ));
    assert_eq!(store.usage(&current, 101).unwrap().records, 1);
    assert_eq!(store.usage(&current, 101).unwrap().archived_records, 0);
}

#[test]
fn invalid_records_are_rejected_before_anything_is_stored() {
    let dir = TempDir::new().unwrap();
    let mut store = SharedDataStore::open(dir.path()).unwrap();
    let owner = owner(13, 1);
    let provenance = provenance("producer");

    let mut empty = record("org.example.catalog", None, "", &provenance, 100);
    empty.opaque_payload = &[];
    assert!(matches!(
        store.write(owner, empty),
        Err(SharedDataError::InvalidRecord)
    ));

    let mut both = record("org.example.catalog", None, r#"{"v":1}"#, &provenance, 100);
    both.opaque_payload = b"bytes";
    assert!(matches!(
        store.write(owner, both),
        Err(SharedDataError::InvalidRecord)
    ));

    for fields in ["[]", "{}", "not json"] {
        let invalid = record("org.example.catalog", None, fields, &provenance, 100);
        assert!(matches!(
            store.write(owner, invalid),
            Err(SharedDataError::InvalidRecord)
        ));
    }

    let large = format!(r#"{{"value":"{}"}}"#, "x".repeat(MAX_RECORD_BYTES));
    let oversized = record("org.example.catalog", None, &large, &provenance, 100);
    assert!(matches!(
        store.write(owner, oversized),
        Err(SharedDataError::RecordTooLarge)
    ));

    let mut long_ttl = record("org.example.catalog", None, r#"{"v":1}"#, &provenance, 100);
    long_ttl.expires_at = Some(100 + MAX_TTL_SECONDS as i64 + 1);
    assert!(matches!(
        store.write(owner, long_ttl),
        Err(SharedDataError::InvalidRecord)
    ));

    let zero_time = record("org.example.catalog", None, r#"{"v":1}"#, &provenance, 0);
    assert!(matches!(
        store.write(owner, zero_time),
        Err(SharedDataError::InvalidRecord)
    ));

    assert_eq!(store.usage(&owner, 100).unwrap().records, 0);
    assert_eq!(store.pool_revision().unwrap(), 0);
}

#[test]
fn a_reviewed_reclaim_removes_only_one_owners_unprotected_archived_rows() {
    let dir = TempDir::new().unwrap();
    let mut store = SharedDataStore::open(dir.path()).unwrap();
    let archived_owner = owner(1, 1);
    let other_owner = owner(2, 1);
    let first_provenance = provenance("org.example.first");
    let second_provenance = provenance("org.example.first");
    let other_provenance = provenance("org.example.other");
    let first = publish(
        &mut store,
        archived_owner,
        record(
            "org.example.catalog",
            Some("disc-1"),
            r#"{"title":"First"}"#,
            &first_provenance,
            100,
        ),
    );
    let second = publish(
        &mut store,
        archived_owner,
        record(
            "org.example.catalog",
            Some("disc-2"),
            r#"{"title":"Second"}"#,
            &second_provenance,
            101,
        ),
    );
    let other = publish(
        &mut store,
        other_owner,
        record(
            "org.example.catalog",
            Some("disc-3"),
            r#"{"title":"Other"}"#,
            &other_provenance,
            102,
        ),
    );
    assert!(
        store
            .set_user_protected(&archived_owner, &first.record.record_id, true, 105)
            .unwrap()
    );
    store.archive_owner(&archived_owner, 110).unwrap();
    assert_eq!(
        store.usage(&archived_owner, 110).unwrap().archived_records,
        2
    );

    let pool_before = store.pool_revision().unwrap();
    let reclaimed = store.reclaim_archived_owner(&archived_owner).unwrap();
    assert_eq!(reclaimed.removed_records, 1, "the protected row stays");
    assert_eq!(reclaimed.reclaimed_bytes, second.record.size_bytes);
    assert_eq!(reclaimed.pool_revision, pool_before + 1);
    let usage = store.usage(&archived_owner, 110).unwrap();
    assert_eq!(usage.archived_records, 1);
    assert_eq!(usage.archived_logical_bytes, first.record.size_bytes);
    assert_eq!(
        store
            .list_dataset_any("org.example.catalog", 110)
            .unwrap()
            .len(),
        1,
        "another owner's live row is untouched"
    );
    assert_eq!(store.usage(&other_owner, 110).unwrap().records, 1);
    assert!(!other.record.archived);
}

#[test]
fn migration_source_reports_records_without_changing_the_pool() {
    let dir = TempDir::new().unwrap();
    let mut store = SharedDataStore::open(dir.path()).unwrap();
    let old = owner(1, 1);
    let new = owner(2, 1);
    let old_provenance = provenance("old");
    let resource = publish(
        &mut store,
        old,
        record(
            "org.example.catalog",
            Some("disc-1"),
            r#"{"title":"Dolphin"}"#,
            &old_provenance,
            100,
        ),
    );
    publish(
        &mut store,
        old,
        record(
            "org.example.notes",
            None,
            r#"{"text":"hello"}"#,
            &old_provenance,
            100,
        ),
    );
    let revision_before = store.pool_revision().unwrap();

    let source = store.migration_source(&old, 100).unwrap();
    assert_eq!(source.records.len(), 2);
    assert!(source.logical_bytes > 0);
    assert_eq!(source.expired_records, 0);
    assert_eq!(source.archived_records, 0);
    assert_eq!(source.records[0].schema_id, "org.example.catalog.v1");
    assert_eq!(store.pool_revision().unwrap(), revision_before);
    assert_eq!(store.usage(&new, 100).unwrap().records, 0);
    assert!(
        store
            .get_any(&resource.record.record_id, 100)
            .unwrap()
            .is_some()
    );
}

#[test]
fn migration_copies_records_with_original_provenance_and_keeps_the_source() {
    let dir = TempDir::new().unwrap();
    let mut store = SharedDataStore::open(dir.path()).unwrap();
    let old = owner(1, 1);
    let new = owner(2, 1);
    let old_provenance = provenance("old");
    let first = publish(
        &mut store,
        old,
        record(
            "org.example.catalog",
            Some("disc-1"),
            r#"{"title":"Dolphin"}"#,
            &old_provenance,
            100,
        ),
    );
    publish(
        &mut store,
        old,
        record(
            "org.example.notes",
            None,
            r#"{"text":"hello"}"#,
            &old_provenance,
            100,
        ),
    );

    let ids = store
        .migration_source(&old, 100)
        .unwrap()
        .records
        .into_iter()
        .map(|record| record.record_id)
        .collect::<Vec<_>>();
    let outcome = store.migrate_records(&old, &new, &ids, 100).unwrap();
    assert_eq!(outcome.copied.len(), 2);
    assert!(outcome.skipped.is_empty());
    assert_ne!(outcome.copied[0].record_id, first.record.record_id);

    let migrated = store
        .list_dataset(&new, "org.example.catalog", 100)
        .unwrap();
    assert_eq!(migrated.records.len(), 1);
    let copy = &migrated.records[0];
    assert_eq!(copy.fields_json, r#"{"title":"Dolphin"}"#);
    assert_eq!(
        copy.provenance, old_provenance,
        "the original source is preserved"
    );
    assert_eq!(
        copy.provenance.source_publisher_fingerprint,
        "publisher-old"
    );
    assert_eq!(copy.revision, 1);
    assert_eq!(copy.record_id, outcome.copied[0].record_id);
    assert_eq!(store.usage(&new, 100).unwrap().records, 2);

    let original = store
        .list_dataset(&old, "org.example.catalog", 100)
        .unwrap();
    assert_eq!(original.records.len(), 1, "the source data is untouched");
    assert_eq!(original.records[0].record_id, first.record.record_id);
    assert_eq!(store.usage(&old, 100).unwrap().records, 2);

    assert!(matches!(
        store.migrate_records(&old, &old, &ids, 100),
        Err(SharedDataError::InvalidMigration)
    ));
}

#[test]
fn migration_reports_conflicts_and_rollback_drops_only_unprotected_copies() {
    let dir = TempDir::new().unwrap();
    let mut store = SharedDataStore::open(dir.path()).unwrap();
    let old = owner(1, 1);
    let new = owner(2, 1);
    let old_provenance = provenance("old");
    let new_provenance = provenance("new");
    publish(
        &mut store,
        old,
        record(
            "org.example.catalog",
            Some("disc-1"),
            r#"{"title":"Old"}"#,
            &old_provenance,
            100,
        ),
    );
    publish(
        &mut store,
        old,
        record(
            "org.example.notes",
            None,
            r#"{"text":"old"}"#,
            &old_provenance,
            100,
        ),
    );
    let own = publish(
        &mut store,
        new,
        record(
            "org.example.catalog",
            Some("disc-1"),
            r#"{"title":"New"}"#,
            &new_provenance,
            100,
        ),
    );

    let ids = store
        .migration_source(&old, 100)
        .unwrap()
        .records
        .into_iter()
        .map(|record| record.record_id)
        .collect::<Vec<_>>();
    let outcome = store.migrate_records(&old, &new, &ids, 100).unwrap();
    assert_eq!(outcome.copied.len(), 1, "only the non-conflicting record");
    assert_eq!(outcome.skipped.len(), 1);
    assert_eq!(outcome.skipped[0].reason, "conflict");
    let migrated_id = outcome.copied[0].record_id.clone();
    assert!(
        store
            .set_user_protected(&new, &migrated_id, true, 100)
            .unwrap()
    );

    // The protected copy and the new identity's own record stay; the other copy is removed.
    let reverted = store
        .revert_migrated(&new, "publisher-old", "old", 100)
        .unwrap();
    assert_eq!(reverted.removed_records, 0);
    assert_eq!(reverted.kept_protected_records, 1);
    assert!(store.get(&new, &migrated_id, 100).unwrap().is_some());
    assert!(
        store
            .get(&new, &own.record.record_id, 100)
            .unwrap()
            .is_some()
    );
    assert_eq!(store.usage(&old, 100).unwrap().records, 2);
}
