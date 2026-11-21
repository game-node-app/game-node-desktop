use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use super::*;
use crate::plugin_host::grants::PluginIdentity;
use crate::plugin_host::namespace::ExternalNamespaceResolver;

const START: i64 = 1_000_000;

fn caller(
    plugin: &str,
    publisher: &str,
    profile: &str,
    epoch: u64,
    session: &str,
) -> ResourceCaller {
    ResourceCaller {
        declared_developer: "Fixture developer".into(),
        origin: "fixture".into(),
        package_digest: Vec::new(),
        session_id: session.into(),
        provider_id: "provider".into(),
        package_version: "1.0.0".into(),
        grants: Default::default(),
        grant_revision: 1,
        profile_epoch: epoch,
        namespace: Some(
            ExternalNamespaceResolver::new(
                PluginIdentity {
                    plugin_id: plugin.into(),
                    publisher_fingerprint: publisher.into(),
                    profile_id: profile.into(),
                },
                Some("provider".into()),
                epoch,
            )
            .unwrap(),
        ),
    }
}

fn plugin() -> ResourceCaller {
    caller(
        "org.example.plugin",
        "publisher-a",
        "profile-1",
        1,
        "session-1",
    )
}

fn allow() -> CommitGuard {
    Box::new(|| Ok(()))
}

struct Host {
    storage: PluginStorage,
    clock: Arc<AtomicI64>,
    _temp: tempfile::TempDir,
    root: PathBuf,
}

impl Host {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("plugin-data");
        let clock = Arc::new(AtomicI64::new(START));
        Self {
            storage: Self::storage_at(&root, &clock),
            clock,
            root,
            _temp: temp,
        }
    }

    fn storage_at(root: &Path, clock: &Arc<AtomicI64>) -> PluginStorage {
        let clock = clock.clone();
        PluginStorage::new(root).with_clock(move || clock.load(Ordering::SeqCst))
    }

    /// A new host process over the same data folder.
    fn restart(&mut self) {
        self.storage = Self::storage_at(&self.root, &self.clock);
    }

    async fn put(
        &self,
        who: &ResourceCaller,
        key: &str,
        value: &[u8],
        durability: Durability,
        ttl: u64,
        expected: u64,
    ) -> Result<PutOutcome, Status> {
        let (key, value) = (key.to_owned(), value.to_vec());
        self.storage
            .run(
                who,
                |_| true,
                move |ns| ns.put(&key, value, durability, ttl, expected, &allow()),
            )
            .await
    }

    async fn get(&self, who: &ResourceCaller, key: &str) -> Result<Option<Found>, Status> {
        let key = key.to_owned();
        self.storage
            .run(who, |_| true, move |ns| ns.get(&key))
            .await
    }

    async fn delete(
        &self,
        who: &ResourceCaller,
        key: &str,
        expected: u64,
    ) -> Result<DeleteOutcome, Status> {
        let key = key.to_owned();
        self.storage
            .run(who, |_| true, move |ns| ns.delete(&key, expected, &allow()))
            .await
    }

    async fn usage(&self, who: &ResourceCaller, durability: Durability) -> u64 {
        self.storage
            .run(who, |_| true, move |ns| ns.usage(durability))
            .await
            .unwrap()
    }

    fn files(&self) -> Vec<PathBuf> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else {
                    out.push(path);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.root, &mut out);
        out
    }
}

fn applied(outcome: PutOutcome) -> u64 {
    match outcome {
        PutOutcome::Applied { revision, .. } => revision,
        other => panic!("expected Applied, got {other:?}"),
    }
}

use Durability::{Persistent, SessionOnly};

#[tokio::test]
async fn values_keep_the_durability_they_were_written_with() {
    let host = Host::new();
    let who = plugin();
    applied(
        host.put(&who, "durable", b"d", Persistent, 0, 0)
            .await
            .unwrap(),
    );
    applied(
        host.put(&who, "volatile", b"v", SessionOnly, 0, 0)
            .await
            .unwrap(),
    );

    let durable = host.get(&who, "durable").await.unwrap().unwrap();
    assert_eq!(
        (durable.value.as_slice(), durable.durability),
        (&b"d"[..], Persistent)
    );
    let volatile = host.get(&who, "volatile").await.unwrap().unwrap();
    assert_eq!(
        (volatile.value.as_slice(), volatile.durability),
        (&b"v"[..], SessionOnly)
    );
    assert!(host.get(&who, "absent").await.unwrap().is_none());
    // Only the durable value touched the disk.
    assert_eq!(host.files().len(), 1);
}

#[tokio::test]
async fn a_key_lives_in_one_layer_and_moves_with_its_new_durability() {
    let host = Host::new();
    let who = plugin();
    let first = applied(host.put(&who, "k", b"one", Persistent, 0, 0).await.unwrap());
    let moved = applied(
        host.put(&who, "k", b"two", SessionOnly, 0, first)
            .await
            .unwrap(),
    );
    assert!(moved > first);
    let found = host.get(&who, "k").await.unwrap().unwrap();
    assert_eq!(
        (found.value.as_slice(), found.durability),
        (&b"two"[..], SessionOnly)
    );
    assert_eq!(
        host.usage(&who, Persistent).await,
        0,
        "the durable copy is gone"
    );

    // And back: after a restart nothing of the volatile copy remains, durable one is current.
    let back = applied(
        host.put(&who, "k", b"three", Persistent, 0, moved)
            .await
            .unwrap(),
    );
    assert_eq!(
        host.usage(&who, SessionOnly).await,
        0,
        "the volatile copy is gone"
    );
    let mut host = host;
    host.restart();
    let found = host.get(&who, "k").await.unwrap().unwrap();
    assert_eq!(
        (found.value.as_slice(), found.revision),
        (&b"three"[..], back)
    );
}

#[tokio::test]
async fn a_keys_revisions_never_repeat_across_deletes_layers_and_restarts() {
    let mut host = Host::new();
    let who = plugin();
    let mut seen = vec![applied(
        host.put(&who, "k", b"1", Persistent, 0, 0).await.unwrap(),
    )];
    let DeleteOutcome::Deleted { revision } = host.delete(&who, "k", seen[0]).await.unwrap() else {
        panic!("not deleted");
    };
    assert_eq!(revision, seen[0]);
    // Recreating the key never repeats a revision a stale writer could still hold.
    seen.push(applied(
        host.put(&who, "k", b"2", Persistent, 0, 0).await.unwrap(),
    ));
    // Moving it to the volatile layer and back keeps counting.
    seen.push(applied(
        host.put(&who, "k", b"3", SessionOnly, 0, seen[1])
            .await
            .unwrap(),
    ));
    seen.push(applied(
        host.put(&who, "k", b"4", Persistent, 0, seen[2])
            .await
            .unwrap(),
    ));
    host.restart();
    seen.push(applied(
        host.put(&who, "k", b"5", Persistent, 0, seen[3])
            .await
            .unwrap(),
    ));
    assert!(seen.windows(2).all(|pair| pair[0] < pair[1]), "{seen:?}");
}

#[tokio::test]
async fn a_put_or_delete_needs_the_current_revision_and_a_conflict_changes_nothing() {
    let host = Host::new();
    let who = plugin();
    // A key that does not exist is revision 0.
    assert_eq!(
        host.put(&who, "k", b"x", SessionOnly, 0, 7).await.unwrap(),
        PutOutcome::Conflict { current: 0 }
    );
    let revision = applied(host.put(&who, "k", b"one", Persistent, 0, 0).await.unwrap());
    // Creating again with 0 conflicts too.
    assert_eq!(
        host.put(&who, "k", b"two", Persistent, 0, 0).await.unwrap(),
        PutOutcome::Conflict { current: revision }
    );
    assert_eq!(
        host.delete(&who, "k", revision + 1).await.unwrap(),
        DeleteOutcome::Conflict { current: revision }
    );
    let found = host.get(&who, "k").await.unwrap().unwrap();
    assert_eq!(
        (found.value.as_slice(), found.revision),
        (&b"one"[..], revision)
    );
    // Deleting what is not there is already done, whatever revision is named.
    assert_eq!(
        host.delete(&who, "nothing", 9).await.unwrap(),
        DeleteOutcome::Absent
    );
}

#[tokio::test]
async fn exceeding_the_quota_is_resource_exhausted_and_keeps_every_stored_value() {
    for durability in [SessionOnly, Persistent] {
        let host = Host::new();
        let who = plugin();
        let big = vec![7u8; MAX_VALUE_BYTES];
        let mut stored = 0;
        // Fill up with 1 MiB values until the next one would not fit.
        while stored < 40 {
            let key = format!("big-{stored}");
            match host.put(&who, &key, &big, durability, 0, 0).await {
                Ok(outcome) => {
                    applied(outcome);
                    stored += 1;
                }
                Err(status) => {
                    assert_eq!(
                        status.code(),
                        tonic::Code::ResourceExhausted,
                        "{durability:?}"
                    );
                    break;
                }
            }
        }
        assert!(
            (14..=15).contains(&stored),
            "{durability:?}: {stored} values of 1 MiB fit in {QUOTA_BYTES}"
        );
        let usage = host.usage(&who, durability).await;
        assert!(usage <= QUOTA_BYTES);

        // A replacement that would no longer fit fails whole: the old value stays.
        let status = host
            .put(&who, "big-0", &vec![1u8; MAX_VALUE_BYTES], durability, 0, 0)
            .await
            .map(|_| ())
            .err();
        // (expected revision 0 conflicts first; use the real one)
        assert!(status.is_none());
        let current = host.get(&who, "big-0").await.unwrap().unwrap();
        // Growing in place past the quota: fill the remainder first, then try to grow.
        let rest = QUOTA_BYTES - host.usage(&who, durability).await;
        let filler =
            vec![0u8; (rest as usize).saturating_sub(durable::charge("filler", 0) as usize)];
        applied(
            host.put(&who, "filler", &filler, durability, 0, 0)
                .await
                .unwrap(),
        );
        let before = host.usage(&who, durability).await;
        let status = host
            .put(
                &who,
                "filler",
                &vec![0u8; filler.len() + 1],
                durability,
                0,
                host.get(&who, "filler").await.unwrap().unwrap().revision,
            )
            .await
            .unwrap_err();
        assert_eq!(status.code(), tonic::Code::ResourceExhausted);
        assert_eq!(host.usage(&who, durability).await, before, "{durability:?}");
        assert_eq!(
            host.get(&who, "filler").await.unwrap().unwrap().value.len(),
            filler.len()
        );
        assert_eq!(
            host.get(&who, "big-0").await.unwrap().unwrap().revision,
            current.revision
        );
    }
}

#[tokio::test]
async fn the_layers_have_independent_quotas() {
    let host = Host::new();
    let who = plugin();
    let big = vec![1u8; MAX_VALUE_BYTES];
    for index in 0..14 {
        applied(
            host.put(&who, &format!("s{index}"), &big, SessionOnly, 0, 0)
                .await
                .unwrap(),
        );
    }
    // A full volatile layer does not take space from the durable one.
    applied(host.put(&who, "d", &big, Persistent, 0, 0).await.unwrap());
}

#[tokio::test]
async fn values_expire_after_their_ttl_and_free_their_space() {
    let host = Host::new();
    let who = plugin();
    let PutOutcome::Applied { expires_at, .. } = host
        .put(&who, "short", b"x", Persistent, 60, 0)
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(expires_at, Some(START + 60));
    applied(
        host.put(&who, "volatile", b"y", SessionOnly, 120, 0)
            .await
            .unwrap(),
    );
    applied(
        host.put(&who, "forever", b"z", Persistent, 0, 0)
            .await
            .unwrap(),
    );

    host.clock.store(START + 59, Ordering::SeqCst);
    assert!(host.get(&who, "short").await.unwrap().is_some());
    host.clock.store(START + 60, Ordering::SeqCst);
    assert!(host.get(&who, "short").await.unwrap().is_none());
    assert!(host.get(&who, "volatile").await.unwrap().is_some());
    host.clock.store(START + 120, Ordering::SeqCst);
    assert!(host.get(&who, "volatile").await.unwrap().is_none());
    assert_eq!(host.usage(&who, SessionOnly).await, 0);
    assert_eq!(
        host.usage(&who, Persistent).await,
        durable::charge("forever", 1)
    );
    // An expired key is a key that does not exist: it is created with revision 0.
    applied(
        host.put(&who, "short", b"again", Persistent, 0, 0)
            .await
            .unwrap(),
    );
}

#[tokio::test]
async fn session_only_values_end_with_the_last_open_session() {
    let host = Host::new();
    let first = caller(
        "org.example.plugin",
        "publisher-a",
        "profile-1",
        1,
        "session-1",
    );
    let second = caller(
        "org.example.plugin",
        "publisher-a",
        "profile-1",
        1,
        "session-2",
    );
    let open = |ids: &'static [&'static str]| move |id: &str| ids.contains(&id);
    async fn get_while_open(
        host: &Host,
        who: &ResourceCaller,
        ids: &'static [&'static str],
    ) -> Option<Found> {
        host.storage
            .run(who, move |id: &str| ids.contains(&id), |ns| ns.get("k"))
            .await
            .unwrap()
    }

    host.storage
        .run(&first, open(&["session-1"]), |ns| {
            ns.put("k", b"v".to_vec(), SessionOnly, 0, 0, &allow())
        })
        .await
        .unwrap();
    // Another open session of the same plugin shares the namespace.
    assert!(
        get_while_open(&host, &second, &["session-1", "session-2"])
            .await
            .is_some()
    );
    // The first one closes; the second is still using it.
    assert!(
        get_while_open(&host, &second, &["session-2"])
            .await
            .is_some()
    );
    // All sessions closed: a session that starts later finds an empty namespace.
    let third = caller(
        "org.example.plugin",
        "publisher-a",
        "profile-1",
        1,
        "session-3",
    );
    assert!(
        get_while_open(&host, &third, &["session-3"])
            .await
            .is_none()
    );
}

#[tokio::test]
async fn namespaces_are_separated_by_plugin_publisher_and_profile() {
    let host = Host::new();
    let base = plugin();
    let others = [
        caller("org.example.other", "publisher-a", "profile-1", 1, "s"),
        caller("org.example.plugin", "publisher-b", "profile-1", 1, "s"),
        caller("org.example.plugin", "publisher-a", "profile-2", 1, "s"),
    ];
    applied(
        host.put(&base, "k", b"secret", Persistent, 0, 0)
            .await
            .unwrap(),
    );
    applied(
        host.put(&base, "m", b"secret", SessionOnly, 0, 0)
            .await
            .unwrap(),
    );
    for other in &others {
        assert!(host.get(other, "k").await.unwrap().is_none());
        assert!(host.get(other, "m").await.unwrap().is_none());
        assert_eq!(host.usage(other, Persistent).await, 0);
        assert_eq!(
            host.delete(other, "k", 1).await.unwrap(),
            DeleteOutcome::Absent
        );
        // Their own writes and deletions never reach the first namespace.
        applied(
            host.put(other, "k", b"theirs", Persistent, 0, 0)
                .await
                .unwrap(),
        );
    }
    assert_eq!(
        host.get(&base, "k").await.unwrap().unwrap().value,
        b"secret"
    );
    assert_eq!(
        host.get(&base, "m").await.unwrap().unwrap().value,
        b"secret"
    );
    assert_eq!(host.files().len(), 4, "one private file per namespace");
}

#[tokio::test]
async fn a_new_profile_epoch_discards_session_values_and_rejects_stale_callers() {
    let host = Host::new();
    let before = plugin();
    applied(
        host.put(&before, "k", b"durable", Persistent, 0, 0)
            .await
            .unwrap(),
    );
    applied(
        host.put(&before, "v", b"volatile", SessionOnly, 0, 0)
            .await
            .unwrap(),
    );

    // The same account signs in again: a later epoch, the same persistent namespace.
    let after = caller(
        "org.example.plugin",
        "publisher-a",
        "profile-1",
        2,
        "session-1",
    );
    assert_eq!(
        host.get(&after, "k").await.unwrap().unwrap().value,
        b"durable"
    );
    assert!(host.get(&after, "v").await.unwrap().is_none());
    // A caller of the earlier epoch can no longer reach the namespace.
    let status = host.get(&before, "k").await.unwrap_err();
    assert_eq!(status.code(), tonic::Code::FailedPrecondition);
}

#[tokio::test]
async fn callers_without_a_namespace_are_refused() {
    let host = Host::new();
    let mut official = plugin();
    official.namespace = None;
    let status = host.get(&official, "k").await.unwrap_err();
    assert_eq!(status.code(), tonic::Code::FailedPrecondition);
    assert!(host.files().is_empty());
}

#[tokio::test]
async fn reading_never_creates_files_and_a_missing_data_folder_only_blocks_durable_writes() {
    let host = Host::new();
    let who = plugin();
    assert!(host.get(&who, "k").await.unwrap().is_none());
    assert!(host.delete(&who, "k", 0).await.unwrap() == DeleteOutcome::Absent);
    assert!(!host.root.exists(), "nothing was created by reading");

    let storage = PluginStorage::default();
    let put = |key: &'static str, durability| {
        storage.run(
            &who,
            |_| true,
            move |ns| ns.put(key, b"v".to_vec(), durability, 0, 0, &allow()),
        )
    };
    applied(put("volatile", SessionOnly).await.unwrap());
    assert_eq!(
        put("durable", Persistent).await.unwrap_err().code(),
        tonic::Code::Unavailable
    );
}

#[tokio::test]
async fn a_failed_commit_check_changes_nothing() {
    let host = Host::new();
    let who = plugin();
    let revision = applied(host.put(&who, "k", b"one", Persistent, 0, 0).await.unwrap());
    let denied = Arc::new(AtomicBool::new(true));
    let guard = || -> CommitGuard {
        let denied = denied.clone();
        Box::new(move || {
            if denied.load(Ordering::SeqCst) {
                Err(Status::permission_denied("revoked"))
            } else {
                Ok(())
            }
        })
    };
    for durability in [SessionOnly, Persistent] {
        let status = host
            .storage
            .run(&who, |_| true, {
                let guard = guard();
                move |ns| ns.put("k", b"two".to_vec(), durability, 0, revision, &guard)
            })
            .await
            .unwrap_err();
        assert_eq!(status.code(), tonic::Code::PermissionDenied);
    }
    let status = host
        .storage
        .run(&who, |_| true, {
            let guard = guard();
            move |ns| ns.delete("k", revision, &guard)
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), tonic::Code::PermissionDenied);
    let found = host.get(&who, "k").await.unwrap().unwrap();
    assert_eq!(
        (found.value.as_slice(), found.revision, found.durability),
        (&b"one"[..], revision, Persistent)
    );
}

#[tokio::test]
async fn an_unusable_store_is_unavailable_without_leaking_where_it_is() {
    let host = Host::new();
    let who = plugin();
    std::fs::create_dir_all(host.root.parent().unwrap()).unwrap();
    // A file where the plugin-data folder should be.
    std::fs::write(&host.root, b"in the way").unwrap();
    let status = host
        .put(&who, "k", b"v", Persistent, 0, 0)
        .await
        .unwrap_err();
    assert_eq!(status.code(), tonic::Code::Unavailable);
    let message = status.message().to_owned();
    assert!(!message.contains("plugin-data"), "{message}");
    assert!(!message.contains(host.root.to_str().unwrap()), "{message}");
}

#[tokio::test]
async fn host_cleanup_keeps_protected_entries_and_reclaims_only_the_owner() {
    let host = Host::new();
    let owner = plugin();
    let other = caller(
        "org.example.other",
        "publisher-a",
        "profile-1",
        1,
        "session-2",
    );
    let resolver = |plugin: &str| {
        ExternalNamespaceResolver::new(
            PluginIdentity {
                plugin_id: plugin.into(),
                publisher_fingerprint: "publisher-a".into(),
                profile_id: "profile-1".into(),
            },
            Some("provider".into()),
            1,
        )
        .unwrap()
    };

    applied(
        host.put(&owner, "keep", b"protected", Persistent, 0, 0)
            .await
            .unwrap(),
    );
    applied(
        host.put(&owner, "expiring", b"old", Persistent, 10, 0)
            .await
            .unwrap(),
    );
    applied(
        host.put(&other, "other", b"still here", Persistent, 0, 0)
            .await
            .unwrap(),
    );
    assert!(
        host.storage
            .set_persistence_protected(&resolver("org.example.plugin"), 1, "keep", true)
            .await
            .unwrap()
    );

    host.clock.store(START + 20, Ordering::SeqCst);
    let cleaned = host
        .storage
        .cleanup_persistence(&resolver("org.example.plugin"), 1)
        .await
        .unwrap();
    assert_eq!(cleaned.removed_records, 1);
    assert_eq!(cleaned.reclaimed_bytes, durable::charge("expiring", 3));
    assert!(host.get(&owner, "keep").await.unwrap().is_some());
    assert!(host.get(&owner, "expiring").await.unwrap().is_none());
    assert!(
        host.get(&other, "other").await.unwrap().is_some(),
        "another owner's store is untouched"
    );
}
