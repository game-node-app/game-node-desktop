//! The SDK's resource helpers against the same fixture host as the raw gRPC tests: every call
//! is made through the SDK and again by hand, and the answers must be the same. A write is
//! compared by replaying it raw under the SDK's idempotency key: the host only returns the
//! original outcome for a request that is identical, so a match proves the SDK sent exactly what
//! the hand-written call sends.

use super::*;
use gamenode_plugin_sdk::{Outcome, Resources, Written, idempotency_key};

const OWN_AND_MORE: &[&str] = &[
    cap::LIBRARY_READ,
    cap::LIBRARY_METADATA_WRITE,
    cap::FAVORITES_READ,
    cap::FAVORITES_WRITE,
    cap::MATCHING_READ,
    cap::MATCHING_WRITE,
    cap::ARTWORK_READ,
    cap::ARTWORK_WRITE,
];

async fn ready() -> (Fixture, Resources, Arc<FakeHost>) {
    let fixture = Fixture::new(OWN_AND_MORE).await;
    fixture.library(SHARED);
    let host = fixture.with_host();
    let sdk = fixture.host.resources().expect("an API 1.6 session");
    (fixture, sdk, host)
}

fn raw_written(reply: MutationReply) -> Written {
    Written::try_from(reply).unwrap()
}

#[tokio::test]
async fn a_session_before_api_1_6_has_no_resource_helpers() {
    let fixture = Fixture::build(&[], false).await;
    assert!(fixture.host.resources().is_none());
    assert!(fixture.host.resource_context().is_none());
}

#[test]
fn idempotency_keys_are_unique_and_acceptable_to_the_host() {
    let keys: std::collections::HashSet<String> = (0..1000).map(|_| idempotency_key()).collect();
    assert_eq!(keys.len(), 1000);
    assert!(
        keys.iter()
            .all(|key| (1..=128).contains(&key.len()) && !key.chars().any(char::is_control))
    );
}

#[tokio::test]
async fn library_reads_equal_the_raw_reads_across_pages() {
    let fixture = Fixture::new(&[cap::LIBRARY_READ]).await;
    let rows: Vec<(String, u64)> = (0..450).map(|n| (format!("fixture:{n}"), n + 1)).collect();
    let borrowed: Vec<(&str, Option<u64>, &str)> = rows
        .iter()
        .map(|(key, game)| (key.as_str(), Some(*game), "Game"))
        .collect();
    fixture.library(&borrowed);
    let sdk = fixture.host.resources().unwrap();

    // The helper walks three pages; walking by hand gives the same entries in the same order.
    let all = sdk.library_entries("").await.unwrap();
    assert_eq!(all.len(), 450);
    let mut by_hand = Vec::new();
    let mut cursor = String::new();
    loop {
        let page = fixture
            .client()
            .library_list(fixture.list_request(200, &cursor, ""))
            .await
            .unwrap()
            .into_inner();
        by_hand.extend(page.entries);
        if page.next_cursor.is_empty() {
            break;
        }
        cursor = page.next_cursor;
    }
    assert_eq!(all, by_hand);

    // One page, with a search and another provider's filter, matches the raw call.
    let page = sdk.library_page("", "Game", 5, "").await.unwrap();
    let raw = fixture
        .client()
        .library_list(LibraryListRequest {
            search: "Game".into(),
            ..fixture.list_request(5, "", "")
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!((&page.entries, page.revision), (&raw.entries, raw.revision));
    assert!(!page.next_cursor.is_empty());
    assert_eq!(
        sdk.library_get("id-fixture:7").await.unwrap(),
        fixture.get("id-fixture:7", "").await.unwrap()
    );

    // A cursor the library has moved past is the host's own stale-cursor refusal.
    let first = sdk.library_page("", "", 2, "").await.unwrap();
    fixture.library(&[("fixture:1", Some(1), "Renamed")]);
    let stale = sdk
        .library_page("", "", 2, &first.next_cursor)
        .await
        .unwrap_err();
    let raw_stale = fixture
        .client()
        .library_list(fixture.list_request(2, &first.next_cursor, ""))
        .await
        .unwrap_err();
    assert_eq!(stale.code(), Code::FailedPrecondition);
    assert_eq!(
        (stale.code(), stale.message()),
        (raw_stale.code(), raw_stale.message())
    );
}

#[tokio::test]
async fn library_errors_are_the_hosts_own() {
    let (fixture, sdk, _host) = ready().await;
    // A foreign installation, even by id, and an unknown one.
    for id in ["id-steam:620", "id-nope"] {
        let via_sdk = sdk.library_get(id).await.unwrap_err();
        let raw = fixture.get(id, "").await.unwrap_err();
        assert_eq!(via_sdk.code(), Code::NotFound, "{id}");
        assert_eq!(
            (via_sdk.code(), via_sdk.message()),
            (raw.code(), raw.message())
        );
    }
    // Another provider's entries are not disclosed without library.read.all: the list is empty,
    // exactly as the raw call answers.
    let other = sdk.library_page("steam", "", 10, "").await.unwrap();
    let raw = fixture.list("steam").await.unwrap();
    assert!(other.entries.is_empty());
    assert_eq!(other.entries, raw.entries);
}

#[tokio::test]
async fn watch_through_the_sdk_delivers_the_same_events_as_the_raw_stream() {
    let (fixture, sdk, _host) = ready().await;
    let revision = fixture.feed.revision();
    let mut by_sdk = sdk.library_watch("", revision).await.unwrap();
    let mut by_hand = fixture.watch(revision).await.unwrap();
    fixture.library(&[
        ("fixture:1", Some(7), "Portal Two"),
        ("fixture:2", None, "Homebrew"),
    ]);
    let (a, b) = (
        next(&mut by_sdk).await.unwrap(),
        next(&mut by_hand).await.unwrap(),
    );
    assert_eq!(a, b);
    assert_eq!(a.event_type, "upserted");
    // A position the host no longer retains is its refusal, unchanged.
    let status = sdk.library_watch("", 10_000).await.unwrap_err();
    assert_eq!(status.code(), Code::OutOfRange);
}

#[tokio::test]
async fn display_names_set_through_the_sdk_are_what_a_raw_call_would_set() {
    let (fixture, sdk, host) = ready().await;
    let key = idempotency_key();
    let set = sdk
        .set_display_name("id-fixture:1", 0, &key, Some("Portal Two"))
        .await
        .unwrap();
    assert_eq!(
        set,
        Written {
            resource_id: "id-fixture:1".into(),
            revision: 1,
            outcome: Outcome::Applied
        }
    );
    // The same key with the hand-written request replays the SDK's outcome, so it was identical.
    let replay = fixture
        .set_name(
            &key,
            0,
            "id-fixture:1",
            "",
            &[("display_name", "Portal Two")],
        )
        .await
        .unwrap();
    assert_eq!(raw_written(replay), set);
    assert_eq!(host.writes.load(Ordering::SeqCst), 1);

    // A stale revision is an answer, not an error, in both.
    let stale = sdk
        .set_display_name("id-fixture:1", 0, &idempotency_key(), Some("Other"))
        .await
        .unwrap();
    let raw = fixture
        .rename(&idempotency_key(), 0, "Other")
        .await
        .unwrap();
    assert_eq!(
        (stale.outcome, stale.revision),
        (Outcome::RevisionConflict, 1)
    );
    assert_eq!(
        raw_written(raw),
        Written {
            resource_id: "id-fixture:1".into(),
            ..stale
        }
    );

    // None restores the detected name; the raw call for it is an empty display_name.
    let clear_key = idempotency_key();
    let cleared = sdk
        .set_display_name("id-fixture:1", 1, &clear_key, None)
        .await
        .unwrap();
    assert_eq!((cleared.revision, cleared.outcome), (2, Outcome::Applied));
    let replay = fixture
        .set_name(&clear_key, 1, "id-fixture:1", "", &[("display_name", "")])
        .await
        .unwrap();
    assert_eq!(raw_written(replay), cleared);

    // A foreign entry and a bad value fail with the host's own codes.
    let foreign = sdk
        .set_display_name("id-steam:620", 0, &idempotency_key(), Some("X"))
        .await
        .unwrap_err();
    assert_eq!(foreign.code(), Code::NotFound);
    let control = sdk
        .set_display_name("id-fixture:1", 2, &idempotency_key(), Some("a\u{7}b"))
        .await
        .unwrap_err();
    assert_eq!(control.code(), Code::InvalidArgument);
}

#[tokio::test]
async fn favorites_through_the_sdk_equal_the_raw_calls() {
    let (fixture, sdk, host) = ready().await;
    host.games
        .lock()
        .unwrap()
        .insert(("fixture".into(), 7), false);

    let key = idempotency_key();
    let set = sdk.set_favorite("7", true, &key).await.unwrap();
    assert_eq!(
        (set.resource_id.as_str(), set.revision, set.outcome),
        ("7", 0, Outcome::Applied)
    );
    let replay = fixture.favorite(&key, 0, "", "7", true).await.unwrap();
    assert_eq!(raw_written(replay), set);
    assert_eq!(host.writes.load(Ordering::SeqCst), 1);

    let raw_list = fixture.favorites(0, "", "").await.unwrap();
    let listed = sdk.favorite_games().await.unwrap();
    assert_eq!(listed, [("7".to_owned(), true)]);
    assert_eq!(
        listed,
        raw_list
            .entries
            .iter()
            .map(|e| (e.game_id.clone(), e.favorite))
            .collect::<Vec<_>>()
    );
    // The same state again is already applied; a game no own entry is matched to is not found.
    let again = sdk
        .set_favorite("7", true, &idempotency_key())
        .await
        .unwrap();
    assert_eq!(again.outcome, Outcome::AlreadyApplied);
    let nobody = sdk
        .set_favorite("8", true, &idempotency_key())
        .await
        .unwrap_err();
    assert_eq!(nobody.code(), Code::NotFound);
}

#[tokio::test]
async fn matching_through_the_sdk_equals_the_raw_calls() {
    let (fixture, sdk, host) = ready().await;
    *host.catalog.lock().unwrap() = vec![
        catalog_game(7, "Portal 2", Some(2011)),
        catalog_game(8, "Portal", Some(2007)),
    ];
    let found = sdk.search_catalog("portal", 0).await.unwrap();
    let raw = fixture.search("portal", 0).await.unwrap();
    assert_eq!(found, raw.candidates);
    assert_eq!(
        found
            .iter()
            .map(|c| (c.game_id.as_str(), c.release_year))
            .collect::<Vec<_>>(),
        [("7", 2011), ("8", 2007)]
    );

    let key = idempotency_key();
    let assigned = sdk
        .assign_match("id-fixture:2", "7", 1, &key)
        .await
        .unwrap();
    assert_eq!(
        (
            assigned.resource_id.as_str(),
            assigned.revision,
            assigned.outcome
        ),
        ("id-fixture:2", 2, Outcome::Applied)
    );
    let replay = fixture.assign(&key, 1, "7").await.unwrap();
    assert_eq!(raw_written(replay), assigned);
    assert_eq!(host.writes.load(Ordering::SeqCst), 1);

    // A stale revision conflicts; a game nobody offered is not found; the user's choice stands.
    let conflict = sdk
        .assign_match("id-fixture:2", "8", 1, &idempotency_key())
        .await
        .unwrap();
    assert_eq!(conflict.outcome, Outcome::RevisionConflict);
    let unknown = sdk
        .assign_match("id-fixture:2", "999", 2, &idempotency_key())
        .await
        .unwrap_err();
    assert_eq!(unknown.code(), Code::NotFound);
    host.matches
        .lock()
        .unwrap()
        .insert("fixture:1".into(), (Some(7), true, 1));
    let user = sdk
        .assign_match("id-fixture:1", "8", 1, &idempotency_key())
        .await
        .unwrap_err();
    let raw_user = fixture
        .assign_request(&idempotency_key(), 1, "id-fixture:1", "8")
        .await
        .unwrap_err();
    assert_eq!(user.code(), Code::FailedPrecondition);
    assert_eq!(
        (user.code(), user.message()),
        (raw_user.code(), raw_user.message())
    );
}

#[tokio::test]
async fn covers_through_the_sdk_equal_the_raw_calls() {
    let (fixture, sdk, host) = ready().await;
    host.game_covers
        .lock()
        .unwrap()
        .insert("fixture:1".into(), b"game cover".to_vec());
    let before = sdk.read_cover("id-fixture:1", 0).await.unwrap();
    assert_eq!(
        (before.bytes.as_slice(), before.revision),
        (b"game cover".as_slice(), 0)
    );

    let key = idempotency_key();
    let set = sdk
        .set_cover("id-fixture:1", 0, &key, png(900, 1200), "image/png")
        .await
        .unwrap();
    assert_eq!((set.revision, set.outcome), (1, Outcome::Applied));
    let replay = fixture
        .set_cover(&key, 0, "id-fixture:1", png(900, 1200), "image/png")
        .await
        .unwrap();
    assert_eq!(raw_written(replay), set);
    assert_eq!(host.writes.load(Ordering::SeqCst), 1);

    let cover = sdk.read_cover("id-fixture:1", 0).await.unwrap();
    let raw = fixture.read_cover("id-fixture:1", 0).await.unwrap();
    assert_eq!(
        (
            cover.bytes.as_slice(),
            cover.media_type.as_str(),
            cover.revision
        ),
        (
            raw.encoded_image.as_slice(),
            raw.media_type.as_str(),
            raw.revision
        )
    );
    assert_eq!(cover.revision, 1);

    // Clearing restores the game's cover, with the raw call for it being an empty image.
    let clear_key = idempotency_key();
    let cleared = sdk
        .clear_cover("id-fixture:1", 1, &clear_key)
        .await
        .unwrap();
    assert_eq!((cleared.revision, cleared.outcome), (2, Outcome::Applied));
    let replay = fixture
        .set_cover(&clear_key, 1, "id-fixture:1", Vec::new(), "")
        .await
        .unwrap();
    assert_eq!(raw_written(replay), cleared);
    assert_eq!(
        sdk.read_cover("id-fixture:1", 0).await.unwrap().bytes,
        b"game cover"
    );

    // A refused image, a missing cover and an over-budget read are the host's own errors.
    let refused = sdk
        .set_cover(
            "id-fixture:1",
            2,
            &idempotency_key(),
            b"<svg/>".to_vec(),
            "image/png",
        )
        .await
        .unwrap_err();
    let raw_refused = fixture
        .set_cover(
            &idempotency_key(),
            2,
            "id-fixture:1",
            b"<svg/>".to_vec(),
            "image/png",
        )
        .await
        .unwrap_err();
    assert_eq!(refused.code(), Code::InvalidArgument);
    assert_eq!(
        (refused.code(), refused.message()),
        (raw_refused.code(), raw_refused.message())
    );
    assert_eq!(
        sdk.read_cover("id-fixture:2", 0).await.unwrap_err().code(),
        Code::NotFound
    );
    assert_eq!(
        sdk.read_cover("id-fixture:1", 3).await.unwrap_err().code(),
        Code::ResourceExhausted
    );
}

#[tokio::test]
async fn without_a_grant_or_the_engine_the_sdk_gets_the_raw_refusals() {
    // No write grants: every write is denied exactly as the raw call is.
    let fixture = Fixture::new(&[cap::LIBRARY_READ]).await;
    fixture.library(SHARED);
    fixture.with_host();
    let sdk = fixture.host.resources().unwrap();
    let denied = sdk
        .set_display_name("id-fixture:1", 0, &idempotency_key(), Some("X"))
        .await
        .unwrap_err();
    let raw = fixture
        .rename(&idempotency_key(), 0, "X")
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
    assert_eq!(
        (denied.code(), denied.message()),
        (raw.code(), raw.message())
    );
    assert_eq!(
        sdk.set_favorite("7", true, &idempotency_key())
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    assert_eq!(
        sdk.search_catalog("portal", 0).await.unwrap_err().code(),
        Code::PermissionDenied
    );
    assert_eq!(
        sdk.read_cover("id-fixture:1", 0).await.unwrap_err().code(),
        Code::PermissionDenied
    );

    // Granted but with no engine side connected: unavailable, as raw.
    let fixture = Fixture::new(OWN_AND_MORE).await;
    fixture.library(SHARED);
    let sdk = fixture.host.resources().unwrap();
    let down = sdk
        .set_display_name("id-fixture:1", 0, &idempotency_key(), Some("X"))
        .await
        .unwrap_err();
    assert_eq!(down.code(), Code::Unavailable);
}

// ---- The published recipes, run against the same fixture host ----

#[path = "../../../../../plugin-sdk/examples/library_recipes.rs"]
mod recipes;

#[tokio::test(start_paused = true)]
async fn a_change_is_resent_with_its_key_only_after_a_lost_connection() {
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let sent = recipes::send(|| async {
        match calls.fetch_add(1, Ordering::SeqCst) {
            0 | 1 => Err(Status::unavailable("connection lost")),
            _ => Ok("done"),
        }
    })
    .await;
    assert_eq!((sent.unwrap(), calls.load(Ordering::SeqCst)), ("done", 3));

    // Three failures give up with the last one; a real answer or refusal is never resent.
    calls.store(0, Ordering::SeqCst);
    let sent: Result<(), Status> = recipes::send(|| async {
        calls.fetch_add(1, Ordering::SeqCst);
        Err(Status::deadline_exceeded("slow"))
    })
    .await;
    assert_eq!(sent.unwrap_err().code(), Code::DeadlineExceeded);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    calls.store(0, Ordering::SeqCst);
    let sent: Result<(), Status> = recipes::send(|| async {
        calls.fetch_add(1, Ordering::SeqCst);
        Err(Status::not_found("no"))
    })
    .await;
    assert_eq!(sent.unwrap_err().code(), Code::NotFound);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn the_rename_recipe_writes_with_the_revision_it_read_and_gives_up_on_a_moving_entry() {
    let (_fixture, sdk, host) = ready().await;
    let named = recipes::rename(&sdk, "id-fixture:1", Some("Portal Two"))
        .await
        .unwrap();
    assert_eq!((named.revision, named.outcome), (1, Outcome::Applied));
    assert_eq!(
        host.name_of("fixture:1"),
        Some((Some("Portal Two".into()), 1))
    );
    // The fixture library still reports revision 0 (it is not fed back), so the host sees a stale
    // revision on every attempt: the recipe reads again, then reports that the entry kept moving.
    let moving = recipes::rename(&sdk, "id-fixture:1", Some("Again"))
        .await
        .unwrap_err();
    assert_eq!(moving.code(), Code::Aborted);
    assert_eq!(
        host.name_of("fixture:1"),
        Some((Some("Portal Two".into()), 1))
    );
    // Other errors are not retried.
    let foreign = recipes::rename(&sdk, "id-steam:620", Some("X"))
        .await
        .unwrap_err();
    assert_eq!(foreign.code(), Code::NotFound);
}

#[tokio::test]
async fn the_match_recipe_finds_the_title_and_leaves_the_users_choice_alone() {
    let (_fixture, sdk, host) = ready().await;
    *host.catalog.lock().unwrap() = vec![
        catalog_game(8, "Portal", Some(2007)),
        catalog_game(7, "Portal 2", Some(2011)),
    ];
    // The exact title, not the first search result.
    let written = recipes::match_by_title(&sdk, "id-fixture:2", "portal 2")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(written.outcome, Outcome::Applied);
    assert_eq!(host.matches.lock().unwrap()["fixture:2"].0, Some(7));
    // Nothing in the catalog is that title.
    assert!(
        recipes::match_by_title(&sdk, "id-fixture:2", "Half-Life")
            .await
            .unwrap()
            .is_none()
    );
    // The user chose this entry's game: the recipe reports nothing to do.
    host.matches
        .lock()
        .unwrap()
        .insert("fixture:1".into(), (Some(8), true, 1));
    assert!(
        recipes::match_by_title(&sdk, "id-fixture:1", "Portal 2")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn the_cover_recipes_set_clear_and_read_without_downloading() {
    let (_fixture, sdk, host) = ready().await;
    assert!(
        recipes::current_cover(&sdk, "id-fixture:1")
            .await
            .unwrap()
            .is_none()
    );
    let set = recipes::use_cover(&sdk, "id-fixture:1", Some((png(300, 450), "image/png")))
        .await
        .unwrap();
    assert_eq!((set.revision, set.outcome), (1, Outcome::Applied));
    let cover = recipes::current_cover(&sdk, "id-fixture:1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (cover.media_type.as_str(), cover.revision),
        ("image/jpeg", 1)
    );
    // A refused image is the host's refusal, not retried.
    let refused = recipes::use_cover(&sdk, "id-fixture:1", Some((b"nope".to_vec(), "image/png")))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), Code::InvalidArgument);
    assert_eq!(host.writes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn the_favorites_recipe_stars_each_matched_game_once() {
    let (_fixture, sdk, host) = ready().await;
    {
        let mut games = host.games.lock().unwrap();
        games.insert(("fixture".into(), 7), false);
        games.insert(("fixture".into(), 9), true);
        games.insert(("fixture".into(), 11), false);
    }
    assert_eq!(recipes::star_matched_games(&sdk).await.unwrap(), 2);
    assert!(
        host.games
            .lock()
            .unwrap()
            .values()
            .all(|favorite| *favorite)
    );
    assert_eq!(recipes::star_matched_games(&sdk).await.unwrap(), 0);
}

#[tokio::test]
async fn the_follow_recipe_lists_then_receives_the_changes_after_the_list() {
    let (fixture, sdk, _host) = ready().await;
    let listed = std::sync::atomic::AtomicUsize::new(usize::MAX);
    let mut seen = Vec::new();
    let follow = recipes::follow_library(
        &sdk,
        |entries| listed.store(entries.len(), Ordering::SeqCst),
        |event| {
            seen.push((event.event_type, event.entry.unwrap().name));
            false
        },
    );
    let change = async {
        while listed.load(Ordering::SeqCst) == usize::MAX {
            tokio::task::yield_now().await;
        }
        // The change lands after the snapshot; the watch resumes from the snapshot's revision,
        // so it is delivered even if the stream opens later.
        fixture.library(&[
            ("fixture:1", Some(7), "Portal Two"),
            ("fixture:2", None, "Homebrew"),
        ]);
    };
    let (followed, ()) = tokio::join!(follow, change);
    followed.unwrap();
    assert_eq!(listed.load(Ordering::SeqCst), 2);
    assert_eq!(seen, [("upserted".to_owned(), "Portal Two".to_owned())]);
}
