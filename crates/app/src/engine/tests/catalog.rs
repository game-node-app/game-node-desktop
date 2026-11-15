//! Engine tests for catalog search and match suggestions.

use gamenode_core::game_api::{GameSummary, SearchPage};
use gamenode_core::library::{
    CatalogStatus, Input as LibInput, LibraryCache, MatchSource, StoredEntry,
};
use gamenode_core::{AuthView, Snapshot};

use super::{FakeApi, FakeCreds, harness};

fn ana_is_signed_in(snapshot: &Snapshot) -> bool {
    matches!(snapshot.auth, AuthView::SignedIn { .. })
}

fn summary(id: u64, name: &str, year: Option<i32>) -> GameSummary {
    GameSummary {
        id,
        name: name.to_owned(),
        release_year: year,
        cover_url: Some(format!(
            "//images.igdb.com/igdb/image/upload/t_thumb/co{id}.jpg"
        )),
    }
}

fn catalog_games() -> Vec<GameSummary> {
    vec![
        summary(2, "Celeste", Some(2018)),
        summary(3, "Celeste Classic", Some(2012)),
    ]
}

#[tokio::test(flavor = "multi_thread")]
async fn catalog_search_uses_search_page_and_publishes_the_latest_answer() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(Some("stored")));
    *h.api.search_result.lock().unwrap() = Some(Ok(SearchPage {
        games: catalog_games(),
        complete: Some(false),
    }));
    h.engine.start();
    h.until("signed in", ana_is_signed_in).await;

    h.engine
        .library_input(LibInput::CatalogSearch("celeste".to_owned()));
    let snapshot = h
        .until("catalog results", |snapshot| {
            snapshot.library.as_ref().is_some_and(|library| {
                library.catalog.status == CatalogStatus::Ready && library.catalog.results.len() == 2
            })
        })
        .await;
    let catalog = &snapshot.library.unwrap().catalog;
    assert_eq!(catalog.query, "celeste");
    assert!(catalog.incomplete);
    assert_eq!(catalog.results[0].title, "Celeste");
    assert_eq!(catalog.results[0].year, Some(2018));
    assert!(!catalog.results[0].in_library);
    assert_eq!(catalog.results[0].local_key, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_short_catalog_query_sends_no_request() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(Some("stored")));
    h.engine.start();
    h.until("signed in", ana_is_signed_in).await;

    h.engine
        .library_input(LibInput::CatalogSearch("c".to_owned()));
    let snapshot = h
        .until("idle catalog", |snapshot| {
            snapshot
                .library
                .as_ref()
                .is_some_and(|library| library.catalog.status == CatalogStatus::Idle)
        })
        .await;
    assert_eq!(snapshot.library.unwrap().catalog.query, "c");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_selected_unmatched_entry_gets_engine_fetched_suggestions_and_can_use_one() {
    let mut h = harness(FakeApi::new(), FakeCreds::with(Some("stored")));
    *h.store.cache.lock().unwrap() = LibraryCache {
        entries: vec![StoredEntry {
            key: "steam:999".to_owned(),
            provider: "steam".to_owned(),
            store_id: "999".to_owned(),
            name: Some("hollow_knight".to_owned()),
            install_dir: "/games/Hollow_Knight_2017".to_owned(),
            fields: Default::default(),
            target: None,
            installed: false,
            owned: true,
            shared: false,
            first_seen: 1,
            last_seen: 1,
        }],
        ..LibraryCache::default()
    };
    *h.api.search_result.lock().unwrap() = Some(Ok(SearchPage {
        games: vec![
            summary(7, "Hollow Knight", Some(2017)),
            summary(8, "Hollow Knight 2", Some(2022)),
        ],
        complete: Some(true),
    }));
    h.engine.start();
    h.until("signed in", ana_is_signed_in).await;
    // The row has to be in the view before selecting it; a select input for a key that is not
    // visible yet is ignored.
    h.until("the cached row", |snapshot| {
        snapshot
            .library
            .as_ref()
            .is_some_and(|library| library.rows.iter().any(|row| row.key == "steam:999"))
    })
    .await;

    h.engine.library_input(LibInput::Select {
        key: "steam:999".to_owned(),
        mode: gamenode_core::library::SelectMode::Replace,
    });
    let snapshot = h
        .until("suggestions", |snapshot| {
            snapshot
                .library
                .as_ref()
                .and_then(|library| library.detail.as_ref())
                .is_some_and(|detail| detail.suggestions.len() == 2)
        })
        .await;
    let detail = snapshot.library.unwrap().detail.unwrap();
    assert_eq!(detail.suggestions[0].game_id, 7);
    assert_eq!(detail.suggestions[0].confidence, 100);

    h.engine.library_input(LibInput::UseSuggestion {
        key: "steam:999".to_owned(),
        game_id: 7,
        now: 100,
    });
    let snapshot = h
        .until("chosen match", |snapshot| {
            snapshot
                .library
                .as_ref()
                .and_then(|library| library.detail.as_ref())
                .is_some_and(|detail| {
                    detail.row.status
                        == gamenode_core::library::MatchStatus::Matched {
                            game_id: 7,
                            source: MatchSource::Chosen,
                        }
                })
        })
        .await;
    assert!(
        snapshot
            .library
            .unwrap()
            .detail
            .unwrap()
            .suggestions
            .is_empty()
    );
    let covers = h.api.covers_fetched.lock().unwrap();
    assert!(
        covers.iter().any(|url| url.ends_with("co7.jpg")),
        "the chosen game's cover is fetched like a match-dialog choice"
    );
}
