//! The SDK's utility helpers (storage, files, network, notifications) and the published recipes
//! against the same fixture host as the raw gRPC tests (task 6.5). Every call is made through the
//! SDK and again by hand, and the answers must be the same. A write is compared by replaying it
//! raw under the SDK's idempotency key: the host only returns the original outcome for a request
//! that is identical, so a match proves the SDK sent exactly what the hand-written call sends.

use std::time::Duration as StdDuration;

use gamenode_plugin_sdk::{
    Durability, Fetch, Notice, Outcome, Resources, Severity, Written, idempotency_key,
};

use super::*;

#[path = "../../../../../plugin-sdk/examples/utility_recipes.rs"]
mod recipes;

const STORAGE: &[&str] = &[
    cap::STORAGE_READ,
    cap::STORAGE_WRITE,
    cap::STORAGE_DELETE,
    cap::STORAGE_PERSIST,
];
const FILES: &[&str] = &[cap::FILES_READ, cap::FILES_WRITE, cap::FILES_DELETE];
const NETWORK: &[&str] = &[cap::NETWORK_FETCH];
const NOTICES: &[&str] = &[cap::NOTIFICATIONS_PUBLISH];

fn sdk(fixture: &Fixture) -> Resources {
    fixture.host.resources().expect("an API 1.6 session")
}

fn raw_written(reply: MutationReply) -> Written {
    Written::try_from(reply).unwrap()
}

// ---- Storage ----

#[tokio::test]
async fn storage_helpers_equal_the_raw_calls() {
    let fixture = Fixture::new(STORAGE).await;
    let sdk = sdk(&fixture);

    // Absent key: not an error, revision 0.
    let absent = sdk.storage_get("settings").await.unwrap();
    let raw = fixture.load("settings").await.unwrap();
    assert!(!absent.found && absent.durability.is_none());
    assert_eq!((absent.revision, absent.found), (raw.revision, raw.found));

    // A write through the SDK, then the same request by hand under the SDK's key: the host
    // returns the original outcome, which it only does for an identical request.
    let key = idempotency_key();
    let put = sdk
        .storage_put(
            "settings",
            0,
            &key,
            b"{\"a\":1}".to_vec(),
            Durability::Persistent,
            Some(StdDuration::from_secs(3600)),
        )
        .await
        .unwrap();
    assert_eq!((put.revision, put.outcome), (1, Outcome::Applied));
    assert_eq!(put.durability, Some(Durability::Persistent));
    assert!(put.usage_bytes > 0 && put.quota_bytes >= put.usage_bytes);
    let replay = fixture
        .put_value(
            "settings",
            b"{\"a\":1}",
            StorageDurability::Persistent,
            3600,
            0,
            &key,
        )
        .await
        .unwrap();
    assert_eq!(
        (
            replay.revision,
            replay.usage_bytes,
            replay.quota_bytes,
            replay.expires_at
        ),
        (
            put.revision,
            put.usage_bytes,
            put.quota_bytes,
            put.expires_at
        )
    );
    assert_eq!(replay.outcome, MutationOutcome::Applied as i32);

    let stored = sdk.storage_get("settings").await.unwrap();
    let raw = fixture.load("settings").await.unwrap();
    assert_eq!(
        (stored.value.clone(), stored.revision, stored.expires_at),
        (raw.value.clone(), raw.revision, raw.expires_at)
    );
    assert_eq!(stored.durability, Some(Durability::Persistent));
    assert_eq!(stored.expires_at, put.expires_at);

    // A stale revision is an answer, not an error.
    let conflict = sdk
        .storage_put(
            "settings",
            0,
            &idempotency_key(),
            b"x".to_vec(),
            Durability::SessionOnly,
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        (conflict.outcome, conflict.revision),
        (Outcome::RevisionConflict, 1)
    );

    // Delete, then the same delete by hand: same outcome and revision.
    let key = idempotency_key();
    let deleted = sdk.storage_delete("settings", 1, &key).await.unwrap();
    assert_eq!(deleted.outcome, Outcome::Applied);
    assert_eq!(
        raw_written(fixture.remove("settings", 1, &key).await.unwrap()),
        deleted
    );
    assert!(!sdk.storage_get("settings").await.unwrap().found);
    let again = sdk
        .storage_delete("settings", 0, &idempotency_key())
        .await
        .unwrap();
    assert_eq!(again.outcome, Outcome::AlreadyApplied);
}

#[tokio::test]
async fn storage_refusals_are_the_hosts_own() {
    // Durable writes need `storage.persist`; the host never turns them into a volatile success.
    let fixture = Fixture::new(&[cap::STORAGE_READ, cap::STORAGE_WRITE]).await;
    let sdk = sdk(&fixture);
    let denied = sdk
        .storage_put(
            "k",
            0,
            &idempotency_key(),
            b"v".to_vec(),
            Durability::Persistent,
            None,
        )
        .await
        .unwrap_err();
    let raw = fixture
        .store("k", b"v", StorageDurability::Persistent, 0)
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
    assert_eq!(
        (denied.code(), denied.message()),
        (raw.code(), raw.message())
    );
    assert!(!sdk.storage_get("k").await.unwrap().found);
    // Session-only is allowed with `storage.write` alone, and says so.
    let session = sdk
        .storage_put(
            "k",
            0,
            &idempotency_key(),
            b"v".to_vec(),
            Durability::SessionOnly,
            None,
        )
        .await
        .unwrap();
    assert_eq!(session.durability, Some(Durability::SessionOnly));

    // A value over the limit changes nothing.
    let big = vec![0u8; MAX_VALUE_BYTES + 1];
    let status = sdk
        .storage_put(
            "big",
            0,
            &idempotency_key(),
            big,
            Durability::SessionOnly,
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::InvalidArgument);
}

use crate::plugin_host::storage::MAX_VALUE_BYTES;

#[tokio::test(flavor = "multi_thread")]
async fn the_storage_recipes_read_modify_write_and_recover_from_a_conflict() {
    let fixture = Fixture::new(STORAGE).await;
    let sdk = sdk(&fixture);
    assert_eq!(recipes::recall(&sdk, "n").await.unwrap(), None);

    let first = recipes::update(&sdk, "n", Durability::Persistent, None, |old| {
        assert!(old.is_none());
        b"1".to_vec()
    })
    .await
    .unwrap();
    assert_eq!(first.outcome, Outcome::Applied);
    assert_eq!(
        recipes::recall(&sdk, "n").await.unwrap(),
        Some(b"1".to_vec())
    );

    // Another writer gets in between the read and the write: the first attempt conflicts, the
    // recipe reads again, and `change` sees what the other writer wrote.
    let mut calls = 0;
    let merged = recipes::update(&sdk, "n", Durability::Persistent, None, |old| {
        calls += 1;
        if calls == 1 {
            // Runs after the recipe's read, before its write.
            let raw = futures_block(fixture.store("n", b"other", StorageDurability::Persistent, 1));
            raw.unwrap();
        }
        let mut next = old.unwrap();
        next.extend_from_slice(b"+mine");
        next
    })
    .await
    .unwrap();
    assert_eq!(merged.outcome, Outcome::Applied);
    assert_eq!(calls, 2);
    assert_eq!(
        recipes::recall(&sdk, "n").await.unwrap(),
        Some(b"other+mine".to_vec())
    );

    // A value that keeps changing is reported, not overwritten.
    let moving = recipes::update(&sdk, "n", Durability::Persistent, None, |old| {
        let current = fixture_revision(&fixture);
        futures_block(fixture.store("n", b"again", StorageDurability::Persistent, current))
            .unwrap();
        old.unwrap()
    })
    .await
    .unwrap_err();
    assert_eq!(moving.code(), Code::Aborted);

    // Forget removes it, and forgetting what is not there succeeds.
    assert_eq!(
        recipes::forget(&sdk, "n").await.unwrap().outcome,
        Outcome::Applied
    );
    assert_eq!(recipes::recall(&sdk, "n").await.unwrap(), None);
    assert_eq!(
        recipes::forget(&sdk, "n").await.unwrap().outcome,
        Outcome::AlreadyApplied
    );
}

/// Runs a fixture call from inside a synchronous recipe callback.
fn futures_block<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(future))
}

fn fixture_revision(fixture: &Fixture) -> u64 {
    futures_block(fixture.load("n")).unwrap().revision
}

// ---- Files ----

#[tokio::test]
async fn file_helpers_equal_the_raw_calls() {
    let fixture = Fixture::new(FILES).await;
    let dir = fixture.game_dir(
        "portal",
        &[
            ("readme.txt", b"hello world"),
            ("saves/slot1.sav", b"s1"),
            ("big.bin", &vec![7u8; 10_000]),
        ],
    );
    fixture.library_in(&[("fixture:1", Some(7), "Portal", &dir)]);
    fixture.user_approving(&dir);
    let sdk = sdk(&fixture);

    let key = idempotency_key();
    let handles = sdk
        .files_request_access("id-fixture:1", &["read", "write", "delete"], &[], &key)
        .await
        .unwrap();
    let raw = fixture
        .request_access("id-fixture:1", &["read", "write", "delete"], &[])
        .await
        .unwrap();
    // Handles are sealed per call, so compare what the plugin can tell apart.
    let describe = |handles: &[FileHandle]| -> Vec<(String, String, bool)> {
        handles
            .iter()
            .map(|h| (h.display_name.clone(), h.resource_kind.clone(), h.writable))
            .collect()
    };
    assert_eq!(describe(&handles), describe(&raw.handles));
    let root = &handles[0].handle;

    // Listing: every page is walked, in the host's order, with the same names and sizes.
    let listed = sdk.files_list(root).await.unwrap();
    let by_hand = fixture.ls(root).await.unwrap();
    let summary = |entries: &[FileEntry]| -> Vec<(String, bool, u64)> {
        entries
            .iter()
            .map(|e| (e.name.clone(), e.directory, e.size_bytes))
            .collect()
    };
    assert_eq!(summary(&listed), summary(&by_hand.entries));
    assert_eq!(
        summary(&listed),
        [
            ("big.bin".to_owned(), false, 10_000),
            ("readme.txt".to_owned(), false, 11),
            ("saves".to_owned(), true, 0)
        ]
    );

    // Reading: a chunk equals the raw chunk, and the whole file is assembled from chunks.
    let readme = listed.iter().find(|e| e.name == "readme.txt").unwrap();
    let chunk = sdk.files_read(&readme.handle, 6, 5).await.unwrap();
    let raw_chunk = fixture.cat(&readme.handle, 6, 5).await.unwrap();
    assert_eq!(chunk, raw_chunk);
    assert_eq!(chunk.contents, b"world");
    let big = listed.iter().find(|e| e.name == "big.bin").unwrap();
    assert_eq!(
        sdk.files_read_all(&big.handle, 10_000).await.unwrap(),
        vec![7u8; 10_000]
    );
    // A file longer than the limit is refused whole, never returned in part.
    let over = sdk.files_read_all(&big.handle, 9_999).await.unwrap_err();
    assert_eq!(over.code(), Code::ResourceExhausted);

    // Creating a file, then the same request by hand under the SDK's key.
    let key = idempotency_key();
    let created = sdk
        .files_create(root, "notes.txt", b"first".to_vec(), &key)
        .await
        .unwrap();
    assert_eq!(created.outcome, Outcome::Applied);
    let replay = fixture
        .write_to(root, "notes.txt", b"first", 0, true, &key)
        .await
        .unwrap();
    assert_eq!(raw_written(replay), created);
    assert_eq!(std::fs::read(dir.join("notes.txt")).unwrap(), b"first");

    // Writing into the existing file by handle, and the same request by hand.
    let key = idempotency_key();
    let written = sdk
        .files_write(&created.resource_id, 5, b"+more".to_vec(), false, &key)
        .await
        .unwrap();
    let replay = fixture
        .write_to(&created.resource_id, "", b"+more", 5, false, &key)
        .await
        .unwrap();
    assert_eq!(raw_written(replay), written);
    assert_eq!(std::fs::read(dir.join("notes.txt")).unwrap(), b"first+more");

    // Deleting, and the same delete by hand.
    let key = idempotency_key();
    let deleted = sdk.files_delete(&created.resource_id, &key).await.unwrap();
    assert_eq!(
        raw_written(fixture.rm(&created.resource_id, &key).await.unwrap()),
        deleted
    );
    assert!(!dir.join("notes.txt").exists());

    // A forged handle and a directory read are the host's own refusals.
    for bad in ["forged", ""] {
        let via_sdk = sdk.files_read(bad, 0, 1).await.unwrap_err();
        let raw = fixture.cat(bad, 0, 1).await.unwrap_err();
        assert_eq!(
            (via_sdk.code(), via_sdk.message()),
            (raw.code(), raw.message()),
            "{bad:?}"
        );
    }
}

#[tokio::test]
async fn the_file_recipes_find_read_and_save_by_name_and_name_what_to_do_next() {
    let fixture = Fixture::new(FILES).await;
    let dir = fixture.game_dir("portal", &[("slot1.sav", b"s1")]);
    fixture.library_in(&[("fixture:1", Some(7), "Portal", &dir)]);
    let sdk = sdk(&fixture);

    // No approval dialog: ask again later. Declined: do not ask again at once.
    assert!(matches!(
        recipes::open_installation(&sdk, "id-fixture:1", &["read"]).await,
        Err(recipes::Access::AskAgain)
    ));
    let user = fixture.user_approving(&dir);
    user.consent.store(false, Ordering::SeqCst);
    assert!(matches!(
        recipes::open_installation(&sdk, "id-fixture:1", &["read"]).await,
        Err(recipes::Access::Declined)
    ));
    user.consent.store(true, Ordering::SeqCst);

    // Read first; a read-only approval gives no write, and widening asks the user again.
    let root = recipes::open_installation(&sdk, "id-fixture:1", &["read"])
        .await
        .unwrap();
    assert_eq!(
        recipes::read_named(&sdk, &root, "slot1.sav", 1024)
            .await
            .unwrap(),
        Some(b"s1".to_vec())
    );
    assert_eq!(
        recipes::read_named(&sdk, &root, "nope.sav", 1024)
            .await
            .unwrap(),
        None
    );
    let too_small = recipes::read_named(&sdk, &root, "slot1.sav", 1)
        .await
        .unwrap_err();
    assert_eq!(too_small.code(), Code::ResourceExhausted);
    let no_write = recipes::save_named(&sdk, &root, "slot2.sav", b"s2")
        .await
        .unwrap_err();
    assert_eq!(no_write.code(), Code::PermissionDenied);
    assert!(!dir.join("slot2.sav").exists());

    let asked = user.asked();
    let root = recipes::open_installation(&sdk, "id-fixture:1", &["read", "write"])
        .await
        .unwrap();
    assert_eq!(user.asked(), asked + 1);
    let handle = recipes::save_named(&sdk, &root, "slot2.sav", b"s2")
        .await
        .unwrap();
    assert!(!handle.is_empty());
    assert_eq!(std::fs::read(dir.join("slot2.sav")).unwrap(), b"s2");
    // A name that is not one plain component never reaches the disk.
    let escape = recipes::save_named(&sdk, &root, "../escape.sav", b"x")
        .await
        .unwrap_err();
    assert_eq!(escape.code(), Code::InvalidArgument);
    assert!(!dir.parent().unwrap().join("escape.sav").exists());
}

// ---- Network ----

#[tokio::test]
async fn fetch_helpers_equal_the_raw_call() {
    let fixture = Fixture::new(NETWORK).await;
    fixture.user_approving_origins(&["https://api.example.com"]);
    let sdk = sdk(&fixture);

    let fetched = sdk
        .network_fetch(
            &Fetch::get("https://api.example.com/v1/items?limit=3")
                .method("POST", b"{\"a\":1}".to_vec())
                .header("Content-Type", "application/json")
                .max_response_bytes(2048)
                .timeout(StdDuration::from_millis(1500)),
        )
        .await
        .unwrap();
    let raw = fixture
        .fetch_with(
            "https://api.example.com/v1/items?limit=3",
            "POST",
            &[("Content-Type", "application/json")],
            b"{\"a\":1}",
            2048,
            1500,
        )
        .await
        .unwrap();
    assert_eq!(fetched.status_code, 200);
    assert_eq!(u32::from(fetched.status_code), raw.status_code);
    assert_eq!(fetched.body, raw.body);
    assert_eq!(fetched.final_origin, raw.final_origin);
    assert_eq!(
        fetched.headers.into_iter().collect::<Vec<_>>(),
        raw.headers.into_iter().collect::<Vec<_>>()
    );
    // Both reached the fetch with the same request.
    let [via_sdk, by_hand] = fixture.fetcher.calls().try_into().unwrap();
    assert_eq!(
        (
            &via_sdk.url,
            &via_sdk.method,
            &via_sdk.body,
            via_sdk.max_response_bytes,
            via_sdk.timeout
        ),
        (
            &by_hand.url,
            &by_hand.method,
            &by_hand.body,
            by_hand.max_response_bytes,
            by_hand.timeout
        )
    );
    assert_eq!(via_sdk.headers, by_hand.headers);

    // A refusal is the host's own.
    let denied = sdk
        .network_fetch(&Fetch::get("https://other.example.com/"))
        .await
        .unwrap_err();
    let raw = fixture
        .fetch_url_text("https://other.example.com/")
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
    assert_eq!(
        (denied.code(), denied.message()),
        (raw.code(), raw.message())
    );
}

#[tokio::test]
async fn the_fetch_recipe_says_whether_to_ask_the_user_or_retry() {
    let fixture = Fixture::new(NETWORK).await;
    fixture.user_approving_origins(&["https://api.example.com"]);
    let sdk = sdk(&fixture);

    let ok = recipes::fetch_bytes(&sdk, "https://api.example.com/x", 1024)
        .await
        .unwrap();
    assert_eq!(ok.body, b"fetched");
    // The recipe sent its own header and deadline, and nothing of GameNode's.
    let [seen] = fixture.fetcher.calls().try_into().unwrap();
    assert_eq!(
        seen.headers,
        [("accept".to_owned(), "application/json".to_owned())]
    );
    assert_eq!(
        (seen.max_response_bytes, seen.timeout),
        (1024, StdDuration::from_secs(10))
    );

    assert!(matches!(
        recipes::fetch_bytes(&sdk, "https://elsewhere.example.com/", 1024).await,
        Err(recipes::FetchFailure::NotApproved)
    ));
    assert!(matches!(
        recipes::fetch_bytes(&sdk, "http://api.example.com/", 1024).await,
        Err(recipes::FetchFailure::Other(_))
    ));
    fixture
        .fetcher
        .script(|_, _| Err(gamenode_platform::plugin_network::FetchError::TooLarge));
    assert!(matches!(
        recipes::fetch_bytes(&sdk, "https://api.example.com/big", 1024).await,
        Err(recipes::FetchFailure::Busy(status)) if status.code() == Code::ResourceExhausted
    ));
}

// ---- Notifications ----

#[tokio::test]
async fn notification_helpers_equal_the_raw_call() {
    let fixture = Fixture::new(NOTICES).await;
    let sdk = sdk(&fixture);
    let notice = Notice::new("en", "Sync finished")
        .body("3 games updated")
        .severity(Severity::Success)
        .translation("pt-BR", "Sincronização concluída", "3 jogos atualizados");

    let key = idempotency_key();
    let written = sdk.notify(&notice, &key).await.unwrap();
    assert_eq!(written.outcome, Outcome::Applied);
    // The same notice by hand under the SDK's key is a replay: shown once, not twice, which the
    // host only does for an identical request.
    let replay = fixture
        .notify_full(
            &key,
            "Sync finished",
            "3 games updated",
            "en",
            "success",
            vec![("pt-BR", "Sincronização concluída", "3 jogos atualizados")],
        )
        .await
        .unwrap();
    assert_eq!(raw_written(replay), written);
    let shown = fixture.notices.all();
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].plugin_id, "fixture.plugin");
    assert_eq!(shown[0].severity, Severity_::Success);

    // A refusal is the host's own.
    let bad = sdk
        .notify(&Notice::new("en", ""), &idempotency_key())
        .await
        .unwrap_err();
    assert_eq!(bad.code(), Code::InvalidArgument);
}

use crate::plugin_host::notifications::Severity as Severity_;

#[tokio::test]
async fn the_notification_recipe_stops_at_the_rate_limit_and_reports_the_wait() {
    let fixture = Fixture::new(NOTICES).await;
    let sdk = sdk(&fixture);
    for index in 0..5 {
        fixture.advance_notice_clock(1000);
        assert!(
            recipes::tell_user(
                &sdk,
                (&format!("Title {index}"), "body"),
                ("Título", "corpo"),
                Severity::Info,
            )
            .await
            .unwrap()
        );
    }
    assert_eq!(fixture.notices.all().len(), 5);

    // The sixth is over the limit: the recipe says so, and nothing more was shown.
    assert!(
        !recipes::tell_user(
            &sdk,
            ("Too many", "body"),
            ("Demais", "corpo"),
            Severity::Info
        )
        .await
        .unwrap()
    );
    assert_eq!(fixture.notices.all().len(), 5);
    // The wait is available to a plugin that wants to try again later.
    let status = sdk
        .notify(&Notice::new("en", "Later"), &idempotency_key())
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::ResourceExhausted);
    let wait = recipes::retry_after(&status).expect("the host says how long to wait");
    assert!(wait > StdDuration::ZERO && wait <= StdDuration::from_secs(60));

    // After the window the same recipe works again.
    fixture.advance_notice_clock(61_000);
    assert!(
        recipes::tell_user(
            &sdk,
            ("Back", "body"),
            ("De volta", "corpo"),
            Severity::Warning
        )
        .await
        .unwrap()
    );
}

// ---- The examples of the development guide ----

#[tokio::test]
async fn the_documented_examples_count_launches_and_read_a_save_slot() {
    let fixture = Fixture::new(&[
        cap::STORAGE_READ,
        cap::STORAGE_WRITE,
        cap::STORAGE_PERSIST,
        cap::FILES_READ,
    ])
    .await;
    let sdk = sdk(&fixture);
    for _ in 0..3 {
        recipes::count_launch(&sdk).await.unwrap();
    }
    assert_eq!(fixture.load("launches").await.unwrap().value, b"3");

    let dir = fixture.game_dir("portal", &[("slot1.sav", b"save")]);
    fixture.library_in(&[("fixture:1", Some(7), "Portal", &dir)]);
    // Before any dialog is connected the example carries on without the file.
    assert_eq!(
        recipes::read_slot(&sdk, "id-fixture:1").await.unwrap(),
        None
    );
    fixture.user_approving(&dir);
    assert_eq!(
        recipes::read_slot(&sdk, "id-fixture:1").await.unwrap(),
        Some(b"save".to_vec())
    );
}

#[tokio::test]
async fn the_save_folder_recipe_suggests_per_platform_and_takes_what_the_user_chose() {
    let fixture = Fixture::new(FILES).await;
    let saves = fixture.base().join("saves");
    std::fs::create_dir_all(&saves).unwrap();
    std::fs::write(saves.join("slot1.sav"), b"s1").unwrap();
    let game = fixture.game_dir("portal", &[("a.txt", b"a")]);
    fixture.library_in(&[("fixture:1", Some(7), "Portal", &game)]);
    let sdk = sdk(&fixture);
    let user = fixture.user_approving(&saves);

    let root = recipes::open_save_folder(&sdk, "id-fixture:1")
        .await
        .unwrap();
    // The user chose their own folder; the plugin can read it through the handle.
    assert_eq!(
        recipes::read_named(&sdk, &root, "slot1.sav", 1024)
            .await
            .unwrap(),
        Some(b"s1".to_vec())
    );
    // Only the suggestions that apply here reached the user (none of the Windows ones), and the
    // folder the user chose was not one of them.
    let asked = user.asked.lock().unwrap()[0].clone();
    assert_eq!(asked.kind, "save");
    assert!(
        asked
            .declared
            .iter()
            .all(|folder| !folder.path.to_string_lossy().contains("AppData"))
    );
    assert!(asked.declared.iter().all(|folder| folder.path != saves));
    let [scope] = user.confirmed.lock().unwrap().clone().try_into().unwrap();
    assert!(!scope.declared_by_plugin);
}
