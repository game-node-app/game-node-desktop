//! Logical Persistence administration stays in the authenticated private namespace.

use super::*;
use crate::plugin_host::storage::MAX_VALUE_BYTES;

const STORAGE: &[&str] = &[
    cap::STORAGE_READ,
    cap::STORAGE_WRITE,
    cap::STORAGE_DELETE,
    cap::STORAGE_PERSIST,
];

#[tokio::test]
async fn persistence_admin_is_logical_paged_and_bound_to_authenticated_owner() {
    let fixture = Fixture::new(STORAGE).await;
    let _first = fixture
        .store("alpha", b"one", StorageDurability::Persistent, 0)
        .await
        .unwrap();
    let second = fixture
        .store("beta", b"two", StorageDurability::Persistent, 0)
        .await
        .unwrap();
    let mut client = fixture.client();

    let usage = client
        .persistence_get_usage(PersistenceUsageRequest {
            context: Some(fixture.context()),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(usage.logical_bytes, (5 + 3 + 64) + (4 + 3 + 64));
    assert_eq!(usage.quota_bytes, crate::plugin_host::storage::QUOTA_BYTES);
    assert!(usage.physical_bytes > 0);
    assert!(usage.physical_bytes_namespace_wide);
    assert_eq!(usage.categories.len(), 1);
    assert_eq!(usage.categories[0].category, "private");
    assert_eq!(usage.categories[0].records, 2);
    assert_eq!(
        usage.categories[0].logical_bytes,
        (5 + 3 + 64) + (4 + 3 + 64)
    );
    assert_eq!(usage.categories[0].protected_records, 0);

    let first_page = client
        .persistence_list_entries(PersistenceListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                page_size: 1,
                ..Default::default()
            }),
            category: "private".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(first_page.entries.len(), 1);
    assert_eq!(first_page.entries[0].logical_key, "alpha");
    assert_eq!(first_page.entries[0].category, "private");
    assert_eq!(first_page.entries[0].size_bytes, 3);
    assert_eq!(first_page.entries[0].entry_id.len(), 64);
    assert!(!first_page.entries[0].entry_id.contains('/'));
    assert!(!first_page.next_cursor.is_empty());

    let second_page = client
        .persistence_list_entries(PersistenceListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                cursor: first_page.next_cursor.clone(),
                page_size: 1,
                ..Default::default()
            }),
            category: "private".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(second_page.entries[0].logical_key, "beta");
    assert!(second_page.next_cursor.is_empty());

    let consumer =
        super::data::consumer_session_with_grants(&fixture, &[cap::STORAGE_PERSIST]).await;
    let foreign_context = consumer.host.resource_context().unwrap().clone();
    let status = client
        .persistence_list_entries(PersistenceListRequest {
            page: Some(PageRequest {
                context: Some(foreign_context.clone()),
                cursor: first_page.next_cursor,
                page_size: 1,
                ..Default::default()
            }),
            category: "private".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);

    let mut foreign_mutation = fixture.mutation("foreign-delete", 0);
    foreign_mutation.context = Some(foreign_context);
    let status = client
        .persistence_delete_entries(PersistenceDeleteRequest {
            mutation: Some(foreign_mutation),
            entry_ids: vec![first_page.entries[0].entry_id.clone()],
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::NotFound);

    let status = client
        .persistence_list_entries(PersistenceListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                ..Default::default()
            }),
            category: "../private".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::InvalidArgument);

    let deletion = client
        .persistence_delete_entries(PersistenceDeleteRequest {
            mutation: Some(fixture.mutation("delete-alpha", second.revision)),
            entry_ids: vec![first_page.entries[0].entry_id.clone()],
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(deletion.outcome, MutationOutcome::Applied as i32);

    let policy = client
        .persistence_set_retention_policy(RetentionPolicyRequest {
            mutation: Some(fixture.mutation("retention", deletion.revision)),
            category: "private".into(),
            max_age_seconds: 3600,
            max_bytes: 1024,
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(policy.outcome, MutationOutcome::Applied as i32);

    let cleared = client
        .persistence_clear(PersistenceClearRequest {
            mutation: Some(fixture.mutation("clear", policy.revision)),
            category: "private".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(cleared.outcome, MutationOutcome::Applied as i32);
    assert!(!fixture.load("beta").await.unwrap().found);
}

#[tokio::test]
async fn persistence_admin_keeps_session_values_out_and_expires_by_retention_age() {
    let fixture = Fixture::new(STORAGE).await;
    fixture
        .store("session-only", b"two", StorageDurability::SessionOnly, 0)
        .await
        .unwrap();
    let stored = fixture
        .store("durable", b"one", StorageDurability::Persistent, 0)
        .await
        .unwrap();
    let mut client = fixture.client();

    let usage = client
        .persistence_get_usage(PersistenceUsageRequest {
            context: Some(fixture.context()),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(usage.logical_bytes, 7 + 3 + 64);

    let page = client
        .persistence_list_entries(PersistenceListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                ..Default::default()
            }),
            category: "private".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        page.entries
            .iter()
            .map(|entry| entry.logical_key.as_str())
            .collect::<Vec<_>>(),
        ["durable"]
    );

    fixture.clock.store(1_000_061, Ordering::SeqCst);
    let policy = client
        .persistence_set_retention_policy(RetentionPolicyRequest {
            mutation: Some(fixture.mutation("retention-age", stored.revision)),
            category: "private".into(),
            max_age_seconds: 60,
            max_bytes: 0,
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(policy.outcome, MutationOutcome::Applied as i32);
    let page = client
        .persistence_list_entries(PersistenceListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                ..Default::default()
            }),
            category: "private".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert!(page.entries.is_empty());
}

#[tokio::test]
async fn persistence_admin_rejects_a_stale_revision_and_replays_the_answer() {
    let fixture = Fixture::new(STORAGE).await;
    let stored = fixture
        .store("alpha", b"one", StorageDurability::Persistent, 0)
        .await
        .unwrap();
    let mut client = fixture.client();
    let entry = client
        .persistence_list_entries(PersistenceListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                ..Default::default()
            }),
            category: "private".into(),
        })
        .await
        .unwrap()
        .into_inner()
        .entries
        .remove(0);

    // The caller names a revision that is no longer current.
    let request = || PersistenceDeleteRequest {
        mutation: Some(fixture.mutation("stale-delete", 0)),
        entry_ids: vec![entry.entry_id.clone()],
    };
    let conflict = client
        .persistence_delete_entries(request())
        .await
        .unwrap()
        .into_inner();
    assert_eq!(conflict.outcome, MutationOutcome::RevisionConflict as i32);
    assert_eq!(conflict.revision, stored.revision);
    assert!(fixture.load("alpha").await.unwrap().found);

    // The same idempotency key replays the stored answer instead of deleting.
    let replay = client
        .persistence_delete_entries(request())
        .await
        .unwrap()
        .into_inner();
    assert_eq!(replay, conflict);
    assert!(fixture.load("alpha").await.unwrap().found);

    let deleted = client
        .persistence_delete_entries(PersistenceDeleteRequest {
            mutation: Some(fixture.mutation("fresh-delete", stored.revision)),
            entry_ids: vec![entry.entry_id],
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(deleted.outcome, MutationOutcome::Applied as i32);
    assert!(!fixture.load("alpha").await.unwrap().found);
}

#[tokio::test]
async fn host_protection_is_not_reachable_from_plugin_rpcs() {
    let fixture = Fixture::new(STORAGE).await;
    let alpha = fixture
        .store("alpha", b"one", StorageDurability::Persistent, 0)
        .await
        .unwrap();
    let beta = fixture
        .store("beta", b"two", StorageDurability::Persistent, 0)
        .await
        .unwrap();
    let resolver = fixture
        .api
        .profiles()
        .resolver("fixture.plugin", &"ab".repeat(32), None)
        .unwrap();
    let epoch = fixture.api.profiles().epoch();
    assert!(
        fixture
            .storage
            .set_persistence_protected(&resolver, epoch, "alpha", true)
            .await
            .unwrap()
    );
    let mut client = fixture.client();

    let usage = client
        .persistence_get_usage(PersistenceUsageRequest {
            context: Some(fixture.context()),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(usage.categories[0].protected_records, 1);
    assert_eq!(usage.categories[0].records, 2);

    let page = client
        .persistence_list_entries(PersistenceListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                ..Default::default()
            }),
            category: "private".into(),
        })
        .await
        .unwrap()
        .into_inner();
    let protected = page
        .entries
        .iter()
        .find(|entry| entry.logical_key == "alpha")
        .unwrap();
    assert!(protected.user_protected);

    // A plugin cannot overwrite, delete, or bulk-delete the protected record.
    let status = fixture
        .store(
            "alpha",
            b"changed",
            StorageDurability::Persistent,
            alpha.revision,
        )
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
    let status = fixture
        .remove("alpha", alpha.revision, "protected")
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
    let status = client
        .persistence_delete_entries(PersistenceDeleteRequest {
            mutation: Some(fixture.mutation("protected", beta.revision)),
            entry_ids: vec![protected.entry_id.clone()],
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);

    // A clear removes the unprotected entries and reports the protected one as retained.
    let cleared = client
        .persistence_clear(PersistenceClearRequest {
            mutation: Some(fixture.mutation("clear-unprotected", beta.revision)),
            category: "private".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(cleared.outcome, MutationOutcome::Applied as i32);
    assert!(fixture.load("alpha").await.unwrap().found);
    assert!(!fixture.load("beta").await.unwrap().found);

    // Only the host can release protection; then the plugin delete succeeds.
    assert!(
        fixture
            .storage
            .set_persistence_protected(&resolver, epoch, "alpha", false)
            .await
            .unwrap()
    );
    let removed = fixture
        .remove("alpha", alpha.revision, "protected-again")
        .await
        .unwrap();
    assert_eq!(removed.outcome, MutationOutcome::Applied as i32);
    assert!(!fixture.load("alpha").await.unwrap().found);
}

#[tokio::test]
async fn quota_exhaustion_preserves_previously_persisted_entries() {
    let fixture = Fixture::new(STORAGE).await;
    let big = vec![7u8; MAX_VALUE_BYTES];
    let mut stored = 0_u64;
    loop {
        let key = format!("big-{stored}");
        match fixture
            .store(&key, &big, StorageDurability::Persistent, 0)
            .await
        {
            Ok(_) => stored += 1,
            Err(status) => {
                assert_eq!(status.code(), Code::ResourceExhausted);
                break;
            }
        }
    }
    assert!(
        (14..=15).contains(&stored),
        "{stored} values of 1 MiB fit in the quota"
    );

    let expected: u64 = (0..stored)
        .map(|index| {
            let key = format!("big-{index}");
            key.len() as u64 + MAX_VALUE_BYTES as u64 + 64
        })
        .sum();
    let mut client = fixture.client();
    let before = client
        .persistence_get_usage(PersistenceUsageRequest {
            context: Some(fixture.context()),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(before.logical_bytes, expected);
    assert_eq!(before.categories[0].records, stored);
    assert_eq!(before.categories[0].logical_bytes, expected);

    // The refused write changes neither accounting nor any earlier entry.
    let status = fixture
        .store("one-more", &big, StorageDurability::Persistent, 0)
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::ResourceExhausted);
    let after = client
        .persistence_get_usage(PersistenceUsageRequest {
            context: Some(fixture.context()),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(after, before);

    let page = client
        .persistence_list_entries(PersistenceListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                page_size: 200,
                ..Default::default()
            }),
            category: "private".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(page.entries.len() as u64, stored);
    for index in 0..stored {
        assert!(
            page.entries
                .iter()
                .any(|entry| entry.logical_key == format!("big-{index}"))
        );
    }
    assert_eq!(
        fixture.load("big-0").await.unwrap().value,
        big,
        "the prior value is byte-for-byte intact"
    );
}

#[tokio::test]
async fn persistence_admin_requires_persist_permission() {
    let fixture = Fixture::new(&[cap::STORAGE_READ]).await;
    let mut client = fixture.client();
    let status = client
        .persistence_get_usage(PersistenceUsageRequest {
            context: Some(fixture.context()),
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
}
