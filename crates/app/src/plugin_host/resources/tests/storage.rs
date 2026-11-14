//! `StorageGet`/`StoragePut`/`StorageDelete` over the real local socket (task 6.1).

use super::*;
use crate::plugin_host::storage::{MAX_KEY_BYTES, MAX_TTL_SECONDS, MAX_VALUE_BYTES, QUOTA_BYTES};

const STORAGE: &[&str] = &[
    cap::STORAGE_READ,
    cap::STORAGE_WRITE,
    cap::STORAGE_DELETE,
    cap::STORAGE_PERSIST,
];
const SESSION: StorageDurability = StorageDurability::SessionOnly;
const DURABLE: StorageDurability = StorageDurability::Persistent;

impl Fixture {
    async fn put_value(
        &self,
        key: &str,
        value: &[u8],
        durability: StorageDurability,
        ttl_seconds: u64,
        expected: u64,
        idempotency: &str,
    ) -> Result<StorageMutationReply, Status> {
        self.client()
            .storage_put(StoragePutRequest {
                mutation: Some(self.mutation(idempotency, expected)),
                key: key.into(),
                value: value.to_vec(),
                durability: durability as i32,
                ttl_seconds,
            })
            .await
            .map(Response::into_inner)
    }

    /// Writes with a fresh idempotency key each time.
    async fn store(
        &self,
        key: &str,
        value: &[u8],
        durability: StorageDurability,
        expected: u64,
    ) -> Result<StorageMutationReply, Status> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let idempotency = format!("put-{}", NEXT.fetch_add(1, Ordering::SeqCst));
        self.put_value(key, value, durability, 0, expected, &idempotency)
            .await
    }

    async fn load(&self, key: &str) -> Result<StorageGetReply, Status> {
        self.client()
            .storage_get(StorageGetRequest {
                context: Some(self.context()),
                key: key.into(),
            })
            .await
            .map(Response::into_inner)
    }

    async fn remove(
        &self,
        key: &str,
        expected: u64,
        idempotency: &str,
    ) -> Result<MutationReply, Status> {
        self.client()
            .storage_delete(StorageDeleteRequest {
                mutation: Some(self.mutation(idempotency, expected)),
                key: key.into(),
            })
            .await
            .map(Response::into_inner)
    }

    /// Ends the plugin's session and opens a new one, as when its process restarts.
    async fn reconnect(&mut self) {
        self.api.disconnect("fixture").await;
        let launch = self.api.begin_launch("fixture").unwrap();
        let credential = launch
            .credential
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|s| u8::from_str_radix(std::str::from_utf8(s).unwrap(), 16).unwrap())
            .collect();
        let session = Session::open_with_options(
            Launch {
                endpoint: self.endpoint.clone(),
                credential,
                operation_id: "launch".into(),
            },
            "fixture.plugin",
            "fixture",
            "1.0",
            &self.capabilities,
            SessionOptions {
                min_api_major: 1,
                min_api_minor: cap::THIRD_PARTY_API_MINOR,
                max_api_major: 1,
                max_api_minor: gamenode_plugin_protocol::API_MINOR,
                ..SessionOptions::default()
            },
        )
        .await
        .unwrap();
        launch.ready.await.unwrap();
        self.host = session.host;
        self._commands = session.commands;
    }

    fn stored_files(&self) -> usize {
        fn count(dir: &std::path::Path) -> usize {
            std::fs::read_dir(dir).map_or(0, |entries| {
                entries
                    .flatten()
                    .map(|entry| {
                        if entry.path().is_dir() {
                            count(&entry.path())
                        } else {
                            1
                        }
                    })
                    .sum()
            })
        }
        count(&self.plugin_data)
    }
}

#[tokio::test]
async fn a_plugin_stores_reads_and_deletes_its_own_values() {
    let fixture = Fixture::new(STORAGE).await;
    assert!(!fixture.load("settings").await.unwrap().found);

    let put = fixture
        .store("settings", b"{\"a\":1}", DURABLE, 0)
        .await
        .unwrap();
    assert_eq!(put.outcome, MutationOutcome::Applied as i32);
    assert_eq!(put.durability, DURABLE as i32);
    assert_eq!(put.quota_bytes, QUOTA_BYTES);
    assert_eq!(put.usage_bytes, "settings".len() as u64 + 7 + 64);
    assert_eq!(put.expires_at, 0);

    let got = fixture.load("settings").await.unwrap();
    assert!(got.found);
    assert_eq!(got.value, b"{\"a\":1}");
    assert_eq!(got.revision, put.revision);
    assert_eq!(got.durability, DURABLE as i32);

    let next = fixture
        .store("settings", b"v2", SESSION, put.revision)
        .await
        .unwrap();
    assert!(next.revision > put.revision);
    assert_eq!(
        fixture.load("settings").await.unwrap().durability,
        SESSION as i32
    );

    let deleted = fixture
        .remove("settings", next.revision, "del-1")
        .await
        .unwrap();
    assert_eq!(deleted.outcome, MutationOutcome::Applied as i32);
    assert_eq!(deleted.revision, next.revision);
    assert!(!fixture.load("settings").await.unwrap().found);
    // Deleting what is gone is a no-op, not an error.
    let again = fixture.remove("settings", 0, "del-2").await.unwrap();
    assert_eq!(again.outcome, MutationOutcome::AlreadyApplied as i32);
}

#[tokio::test]
async fn each_operation_needs_its_own_permission() {
    let reader = Fixture::new(&[cap::STORAGE_READ]).await;
    assert!(!reader.load("k").await.unwrap().found);
    assert_eq!(
        reader
            .store("k", b"v", SESSION, 0)
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    assert_eq!(
        reader.remove("k", 0, "d").await.unwrap_err().code(),
        Code::PermissionDenied
    );

    let writer = Fixture::new(&[cap::STORAGE_WRITE]).await;
    writer.store("k", b"v", SESSION, 0).await.unwrap();
    assert_eq!(
        writer.load("k").await.unwrap_err().code(),
        Code::PermissionDenied
    );
    assert_eq!(
        writer.remove("k", 1, "d").await.unwrap_err().code(),
        Code::PermissionDenied
    );

    // storage.persist supplements the write grant; it does not replace it.
    let persist_only = Fixture::new(&[cap::STORAGE_PERSIST, cap::STORAGE_READ]).await;
    for durability in [SESSION, DURABLE] {
        assert_eq!(
            persist_only
                .store("k", b"v", durability, 0)
                .await
                .unwrap_err()
                .code(),
            Code::PermissionDenied
        );
    }
    assert_eq!(persist_only.stored_files(), 0);

    let deleter = Fixture::new(&[cap::STORAGE_WRITE, cap::STORAGE_READ, cap::STORAGE_DELETE]).await;
    let put = deleter.store("k", b"v", SESSION, 0).await.unwrap();
    deleter.remove("k", put.revision, "d").await.unwrap();
}

#[tokio::test]
async fn a_durable_write_without_persist_is_denied_never_downgraded() {
    let fixture = Fixture::new(&[cap::STORAGE_READ, cap::STORAGE_WRITE, cap::STORAGE_DELETE]).await;
    let status = fixture.store("k", b"v", DURABLE, 0).await.unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    // The machine-readable denial names the missing permission.
    use gamenode_plugin_protocol::prost::Message;
    let detail = DenialDetail::decode(status.details()).unwrap();
    assert_eq!(detail.permission_id, cap::STORAGE_PERSIST);
    // Not stored anywhere: not on disk and not in memory.
    assert!(!fixture.load("k").await.unwrap().found);
    assert_eq!(fixture.stored_files(), 0);
    // Explicit session-only writes remain possible.
    let put = fixture.store("k", b"v", SESSION, 0).await.unwrap();
    assert_eq!(put.durability, SESSION as i32);
    assert_eq!(fixture.stored_files(), 0);
    // And it stays a session-only value, whatever else is attempted.
    assert_eq!(
        fixture
            .store("k", b"w", DURABLE, put.revision)
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    assert_eq!(fixture.load("k").await.unwrap().value, b"v");
}

#[tokio::test]
async fn a_revoked_persist_grant_stops_durable_writes_but_not_reads_or_session_use() {
    let fixture = Fixture::new(STORAGE).await;
    let put = fixture.store("k", b"kept", DURABLE, 0).await.unwrap();
    fixture
        .api
        .revoke_session_permission("fixture", cap::STORAGE_PERSIST)
        .await;
    // The session's context is the same; its grant set changed.
    assert_eq!(
        fixture
            .store("k", b"x", DURABLE, put.revision)
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    assert_eq!(fixture.load("k").await.unwrap().value, b"kept");
    fixture.store("session", b"x", SESSION, 0).await.unwrap();
    fixture
        .api
        .revoke_session_permission("fixture", cap::STORAGE_WRITE)
        .await;
    assert_eq!(
        fixture
            .store("session", b"y", SESSION, 0)
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
}

#[tokio::test]
async fn durability_must_be_chosen() {
    let fixture = Fixture::new(STORAGE).await;
    for durability in [StorageDurability::Unspecified as i32, 99] {
        let status = fixture
            .client()
            .storage_put(StoragePutRequest {
                mutation: Some(fixture.mutation("k", 0)),
                key: "k".into(),
                value: b"v".to_vec(),
                durability,
                ttl_seconds: 0,
            })
            .await
            .unwrap_err();
        assert_eq!(status.code(), Code::InvalidArgument, "{durability}");
    }
    assert!(!fixture.load("k").await.unwrap().found);
}

#[tokio::test]
async fn keys_values_and_lifetimes_are_bounded() {
    let fixture = Fixture::new(STORAGE).await;
    let invalid = |status: Status| assert_eq!(status.code(), Code::InvalidArgument);
    invalid(fixture.store("", b"v", SESSION, 0).await.unwrap_err());
    invalid(
        fixture
            .store(&"k".repeat(MAX_KEY_BYTES + 1), b"v", SESSION, 0)
            .await
            .unwrap_err(),
    );
    invalid(
        fixture
            .store("bad\nkey", b"v", SESSION, 0)
            .await
            .unwrap_err(),
    );
    invalid(fixture.load("").await.unwrap_err());
    invalid(
        fixture
            .load(&"k".repeat(MAX_KEY_BYTES + 1))
            .await
            .unwrap_err(),
    );
    invalid(
        fixture
            .store("k", &vec![0; MAX_VALUE_BYTES + 1], SESSION, 0)
            .await
            .unwrap_err(),
    );
    invalid(
        fixture
            .put_value("k", b"v", SESSION, MAX_TTL_SECONDS + 1, 0, "ttl")
            .await
            .unwrap_err(),
    );
    invalid(
        fixture
            .put_value("k", b"v", SESSION, 0, 0, "")
            .await
            .unwrap_err(),
    );
    // The limits themselves are allowed.
    fixture
        .store(
            &"k".repeat(MAX_KEY_BYTES),
            &vec![0; MAX_VALUE_BYTES],
            DURABLE,
            0,
        )
        .await
        .unwrap();
    fixture
        .put_value("t", b"v", SESSION, MAX_TTL_SECONDS, 0, "ttl-max")
        .await
        .unwrap();
}

#[tokio::test]
async fn a_retried_write_returns_the_original_outcome_and_a_reused_key_is_refused() {
    let fixture = Fixture::new(STORAGE).await;
    let first = fixture
        .put_value("k", b"one", DURABLE, 0, 0, "write-1")
        .await
        .unwrap();
    // The retry repeats the request exactly, so it neither conflicts nor writes again.
    let retry = fixture
        .put_value("k", b"one", DURABLE, 0, 0, "write-1")
        .await
        .unwrap();
    assert_eq!(retry.revision, first.revision);
    assert_eq!(retry.outcome, MutationOutcome::Applied as i32);
    assert_eq!(fixture.load("k").await.unwrap().revision, first.revision);

    // The same key for another request is an error, not a second write.
    let status = fixture
        .put_value("k", b"two", DURABLE, 0, 0, "write-1")
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
    let status = fixture
        .remove("k", first.revision, "write-1")
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
    assert_eq!(fixture.load("k").await.unwrap().value, b"one");

    // A retried delete is answered from the first one.
    let deleted = fixture
        .remove("k", first.revision, "delete-1")
        .await
        .unwrap();
    let again = fixture
        .remove("k", first.revision, "delete-1")
        .await
        .unwrap();
    assert_eq!(
        (again.outcome, again.revision),
        (deleted.outcome, deleted.revision)
    );
}

#[tokio::test]
async fn a_stale_revision_is_a_conflict_that_changes_nothing_and_is_not_remembered() {
    let fixture = Fixture::new(STORAGE).await;
    let put = fixture.store("k", b"one", DURABLE, 0).await.unwrap();

    let conflict = fixture
        .put_value("k", b"two", DURABLE, 0, 0, "retry-me")
        .await
        .unwrap();
    assert_eq!(conflict.outcome, MutationOutcome::RevisionConflict as i32);
    assert_eq!(conflict.revision, put.revision);
    let conflict = fixture.remove("k", put.revision + 5, "del").await.unwrap();
    assert_eq!(conflict.outcome, MutationOutcome::RevisionConflict as i32);
    assert_eq!(conflict.revision, put.revision);
    assert_eq!(fixture.load("k").await.unwrap().value, b"one");

    // Nothing was committed, so the same key may be used again with the right revision.
    let applied = fixture
        .put_value("k", b"two", DURABLE, 0, put.revision, "retry-me")
        .await
        .unwrap();
    assert_eq!(applied.outcome, MutationOutcome::Applied as i32);
    assert_eq!(fixture.load("k").await.unwrap().value, b"two");
}

#[tokio::test]
async fn exceeding_the_quota_is_resource_exhausted_and_preserves_stored_values() {
    for durability in [SESSION, DURABLE] {
        let fixture = Fixture::new(STORAGE).await;
        let big = vec![9u8; MAX_VALUE_BYTES];
        let mut stored = 0;
        let status = loop {
            match fixture
                .store(&format!("big-{stored}"), &big, durability, 0)
                .await
            {
                Ok(_) => stored += 1,
                Err(status) => break status,
            }
            assert!(stored < 40, "the quota never applied");
        };
        assert_eq!(status.code(), Code::ResourceExhausted, "{durability:?}");
        assert!(status.message().contains("nothing was changed"));
        assert!((14..=15).contains(&stored), "{durability:?}: {stored}");
        // Every earlier value is intact, and smaller values still fit.
        for index in 0..stored {
            let got = fixture.load(&format!("big-{index}")).await.unwrap();
            assert!(got.found && got.value == big, "{durability:?} big-{index}");
        }
        fixture
            .store("small", b"fits", durability, 0)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn durable_values_survive_a_restart_session_only_values_do_not() {
    let first = Fixture::new(STORAGE).await;
    let durable = first.store("durable", b"stays", DURABLE, 0).await.unwrap();
    first.store("volatile", b"goes", SESSION, 0).await.unwrap();
    let data = first.plugin_data.clone();

    let second = Fixture::restarted(STORAGE, data).await;
    let got = second.load("durable").await.unwrap();
    assert!(got.found);
    assert_eq!(got.value, b"stays");
    assert_eq!(got.revision, durable.revision);
    assert!(!second.load("volatile").await.unwrap().found);
    // Revisions keep growing across the restart.
    let next = second
        .store("durable", b"newer", DURABLE, durable.revision)
        .await
        .unwrap();
    assert!(next.revision > durable.revision);
}

#[tokio::test]
async fn session_only_values_end_with_the_plugin_session() {
    let mut fixture = Fixture::new(STORAGE).await;
    fixture
        .store("volatile", b"goes", SESSION, 0)
        .await
        .unwrap();
    fixture
        .store("durable", b"stays", DURABLE, 0)
        .await
        .unwrap();
    assert!(fixture.load("volatile").await.unwrap().found);

    fixture.reconnect().await;
    assert!(!fixture.load("volatile").await.unwrap().found);
    assert_eq!(fixture.load("durable").await.unwrap().value, b"stays");
}

#[tokio::test]
async fn namespaces_follow_the_account_profile() {
    let mut fixture = Fixture::new(STORAGE).await;
    fixture.store("k", b"account a", DURABLE, 0).await.unwrap();
    fixture.store("v", b"account a", SESSION, 0).await.unwrap();

    // Another account signs in: the plugin restarts into a profile with nothing of a's.
    fixture.api.profiles().update(Some("b"), true);
    fixture.api.fence_third_party_sessions().await;
    fixture.reconnect().await;
    assert!(!fixture.load("k").await.unwrap().found);
    assert!(!fixture.load("v").await.unwrap().found);
    fixture.store("k", b"account b", DURABLE, 0).await.unwrap();
    assert_eq!(fixture.stored_files(), 2, "one private store per profile");

    // Back to a: its durable value is there, its session-only value is not.
    fixture.api.profiles().update(Some("a"), true);
    fixture.api.fence_third_party_sessions().await;
    fixture.reconnect().await;
    assert_eq!(fixture.load("k").await.unwrap().value, b"account a");
    assert!(!fixture.load("v").await.unwrap().found);
}

#[tokio::test]
async fn values_expire_after_their_lifetime() {
    let fixture = Fixture::new(STORAGE).await;
    let put = fixture
        .put_value("k", b"v", DURABLE, 60, 0, "ttl")
        .await
        .unwrap();
    assert_eq!(put.expires_at, 1_000_060);
    let got = fixture.load("k").await.unwrap();
    assert_eq!((got.found, got.expires_at), (true, 1_000_060));

    fixture.clock.store(1_000_060, Ordering::SeqCst);
    assert!(!fixture.load("k").await.unwrap().found);
    // A retry of the original write still answers from its original outcome.
    let retry = fixture
        .put_value("k", b"v", DURABLE, 60, 0, "ttl")
        .await
        .unwrap();
    assert_eq!(
        (retry.revision, retry.expires_at),
        (put.revision, 1_000_060)
    );
    // An expired key is a key that does not exist.
    fixture.store("k", b"again", DURABLE, 0).await.unwrap();
}

#[tokio::test]
async fn failures_never_reveal_where_the_data_lives() {
    let fixture = Fixture::new(STORAGE).await;
    // Something that is not a folder where the plugin-data folder should be.
    std::fs::write(&fixture.plugin_data, b"in the way").unwrap();
    let status = fixture.store("k", b"v", DURABLE, 0).await.unwrap_err();
    assert_eq!(status.code(), Code::Unavailable);
    let shown = format!("{status:?}");
    let root = fixture.plugin_data.to_str().unwrap();
    assert!(
        !shown.contains(root) && !shown.contains("plugin-data"),
        "{shown}"
    );
    // Session-only use does not need the disk.
    fixture.store("k", b"v", SESSION, 0).await.unwrap();
}

#[tokio::test]
async fn nothing_in_a_reply_is_a_path_or_another_namespaces_data() {
    let fixture = Fixture::new(STORAGE).await;
    let put = fixture.store("k", b"v", DURABLE, 0).await.unwrap();
    let got = fixture.load("k").await.unwrap();
    let data = fixture.plugin_data.to_str().unwrap().to_owned();
    for text in [format!("{put:?}"), format!("{got:?}")] {
        assert!(
            !text.contains(&data) && !text.contains("storage.sqlite"),
            "{text}"
        );
    }
    // The request has no field that could name another owner: a key that looks like a path or
    // an identity is just a key in the caller's own namespace.
    for key in ["../other/k", "org.other.plugin/k", "C:\\x", "k/../../k"] {
        fixture.store(key, b"mine", SESSION, 0).await.unwrap();
        assert_eq!(fixture.load(key).await.unwrap().value, b"mine");
    }
}
