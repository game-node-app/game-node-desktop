use std::time::Duration;

use gamenode_core::game_api::{ApiError, ExternalMatch, FavoriteError, GameSummary};
use gamenode_core::library::{Input as LibInput, MatchStatus};
use gamenode_core::{AuthView, Snapshot};

use super::{FakeApi, FakeCreds, Harness, harness};

fn portal_2_match() -> ExternalMatch {
    ExternalMatch {
        uid: "620".to_owned(),
        game: GameSummary {
            id: 7,
            name: "Portal 2".to_owned(),
            release_year: Some(2011),
            cover_url: None,
        },
    }
}

fn row_is_favorite(snapshot: &Snapshot) -> bool {
    snapshot
        .library
        .as_ref()
        .and_then(|view| view.rows.first())
        .is_some_and(|row| row.is_favorite)
}

async fn eventually(what: &str, mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for: {what}"));
}

fn matched_harness() -> Harness {
    let api = FakeApi::new();
    *api.lookup_result.lock().unwrap() = Some(Ok(vec![portal_2_match()]));
    harness(api, FakeCreds::with(None))
}

#[tokio::test(flavor = "multi_thread")]
async fn favoriting_with_sync_off_stays_local_and_persists_pending() {
    let mut h = matched_harness();
    h.signed_out().await;
    // Turn sync off before the gate is ever consulted.
    h.engine.set_sync(false);
    h.sign_in("token").await;
    h.until("the matched row", |s| {
        s.library.as_ref().is_some_and(|view| {
            view.rows
                .first()
                .is_some_and(|row| matches!(row.status, MatchStatus::Matched { .. }))
        })
    })
    .await;

    h.engine.library_input(LibInput::SetFavorite {
        key: "steam:620".to_owned(),
        favorite: true,
        now: 10,
    });
    let snapshot = h.until("the star appears", |s| row_is_favorite(s)).await;
    assert!(row_is_favorite(&snapshot));
    assert!(
        h.api.favorite_pushes.lock().unwrap().is_empty(),
        "no favorite change may leave while sync is off"
    );
    assert_eq!(*h.api.favorite_reads.lock().unwrap(), 0);
    eventually("the favorite is stored", || {
        h.store
            .favorites
            .lock()
            .unwrap()
            .iter()
            .any(|favorite| favorite.game_id == 7 && favorite.pending)
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn pending_favorites_wait_for_the_explanation_and_then_send() {
    let mut h = matched_harness();
    h.signed_out().await;
    h.engine.set_sync(false);
    h.sign_in("token").await;
    h.until("the matched row", |s| {
        s.library.as_ref().is_some_and(|view| {
            view.rows
                .first()
                .is_some_and(|row| matches!(row.status, MatchStatus::Matched { .. }))
        })
    })
    .await;
    h.engine.library_input(LibInput::SetFavorite {
        key: "steam:620".to_owned(),
        favorite: true,
        now: 10,
    });
    h.until("the star appears", row_is_favorite).await;

    // Sync is on but not explained: still nothing leaves.
    h.engine.set_sync(true);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(h.api.favorite_pushes.lock().unwrap().is_empty());
    assert_eq!(*h.api.favorite_reads.lock().unwrap(), 0);

    h.engine.dismiss_sync_explanation();
    eventually("the favorite is sent", || {
        h.api.favorite_pushes.lock().unwrap().contains(&(7, true))
    })
    .await;
    eventually("the read runs", || {
        *h.api.favorite_reads.lock().unwrap() > 0
    })
    .await;
    eventually("the change is confirmed", || {
        h.store
            .favorites
            .lock()
            .unwrap()
            .iter()
            .any(|favorite| favorite.game_id == 7 && !favorite.pending && favorite.is_favorite)
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_favorite_appears_after_the_cache_loads() {
    let api = FakeApi::new();
    *api.lookup_result.lock().unwrap() = Some(Ok(vec![portal_2_match()]));
    *api.preferred.lock().unwrap() = vec![7];
    let mut h = harness(api, FakeCreds::with(None));
    h.signed_out().await;
    h.engine.set_sync(true);
    h.engine.dismiss_sync_explanation();
    h.sign_in("token").await;

    let snapshot = h
        .until("the server favorite shows", |s| {
            s.library
                .as_ref()
                .is_some_and(|view| view.rows.iter().any(|row| row.is_favorite))
        })
        .await;
    assert!(row_is_favorite(&snapshot));
    eventually("the favorite is stored", || {
        h.store
            .favorites
            .lock()
            .unwrap()
            .iter()
            .any(|favorite| favorite.game_id == 7 && !favorite.pending)
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rejected_favorite_keeps_pending_and_drops_the_session() {
    let api = FakeApi::new();
    *api.lookup_result.lock().unwrap() = Some(Ok(vec![portal_2_match()]));
    *api.favorite_push.lock().unwrap() = Some(Err(FavoriteError::Rejected));
    let mut h = harness(api, FakeCreds::with(None));
    h.signed_out().await;
    h.engine.set_sync(true);
    h.engine.dismiss_sync_explanation();
    h.sign_in("token").await;
    h.until("the matched row", |s| {
        s.library.as_ref().is_some_and(|view| {
            view.rows
                .first()
                .is_some_and(|row| matches!(row.status, MatchStatus::Matched { .. }))
        })
    })
    .await;

    h.engine.library_input(LibInput::SetFavorite {
        key: "steam:620".to_owned(),
        favorite: true,
        now: 10,
    });
    h.until("the session is dropped", |s| {
        !matches!(s.auth, AuthView::SignedIn { .. })
    })
    .await;
    eventually("the change stays pending", || {
        h.store
            .favorites
            .lock()
            .unwrap()
            .iter()
            .any(|favorite| favorite.game_id == 7 && favorite.pending && favorite.is_favorite)
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_read_leaves_local_favorites_alone() {
    let api = FakeApi::new();
    *api.lookup_result.lock().unwrap() = Some(Ok(vec![portal_2_match()]));
    *api.favorite_read.lock().unwrap() = Some(Err(ApiError::Failed));
    let mut h = harness(api, FakeCreds::with(None));
    h.signed_out().await;
    h.engine.set_sync(true);
    h.engine.dismiss_sync_explanation();
    h.sign_in("token").await;
    h.until("the matched row", |s| {
        s.library.as_ref().is_some_and(|view| {
            view.rows
                .first()
                .is_some_and(|row| matches!(row.status, MatchStatus::Matched { .. }))
        })
    })
    .await;

    h.engine.library_input(LibInput::SetFavorite {
        key: "steam:620".to_owned(),
        favorite: true,
        now: 10,
    });
    let _ = h.until("the star appears", row_is_favorite).await;
    // The read failed, but the local star stays and the request is retried later.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(row_is_favorite(&h.snapshots.borrow().clone()));
}
