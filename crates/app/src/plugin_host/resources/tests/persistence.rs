//! Logical Persistence administration stays in the authenticated private namespace.

use super::*;

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
