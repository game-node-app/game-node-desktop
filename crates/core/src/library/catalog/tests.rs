use super::*;
use crate::game_api::{AccountEntry, SearchPage};
use crate::library::machine::tests::{finish_scan, loaded, record, stored};
use crate::library::machine::{Access, Input};
use crate::library::membership::EntryStatus;
use crate::library::store::{AccountLibrary, AutoMatch, GameRecord, LibraryCache};

fn summary(id: u64, name: &str, year: Option<i32>) -> GameSummary {
    GameSummary {
        id,
        name: name.to_owned(),
        release_year: year,
        cover_url: Some(format!("https://covers.test/{id}.jpg")),
    }
}

fn page(games: Vec<GameSummary>) -> SearchPage {
    SearchPage {
        games,
        complete: Some(true),
    }
}

fn machine_with(cache: LibraryCache) -> LibraryMachine {
    let (mut machine, _) = loaded(false, cache);
    finish_scan(
        &mut machine,
        vec![record("620", "Hollow Knight", "/games/Hollow Knight")],
        10,
    );
    machine
}

fn verified(cache: LibraryCache) -> LibraryMachine {
    let mut machine = machine_with(cache);
    machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: 10,
    });
    machine
}

fn catalog_effect(effects: &[Effect]) -> Option<(u64, String)> {
    effects.iter().find_map(|effect| match effect {
        Effect::CatalogSearch { op, query } => Some((*op, query.clone())),
        _ => None,
    })
}

fn search(machine: &mut LibraryMachine, query: &str) -> Option<(u64, String)> {
    catalog_effect(&machine.handle(Input::CatalogSearch(query.to_owned())))
}

#[test]
fn a_short_query_sends_nothing_and_shows_the_idle_state() {
    let mut machine = verified(LibraryCache::default());
    assert!(search(&mut machine, " c ").is_none());
    let catalog = machine.view().unwrap().catalog;
    assert_eq!(catalog.query, " c ");
    assert_eq!(catalog.status, CatalogStatus::Idle);
    assert!(catalog.results.is_empty());
    assert!(!catalog.incomplete);

    assert!(search(&mut machine, "").is_none());
    assert_eq!(machine.view().unwrap().catalog.status, CatalogStatus::Idle);
}

#[test]
fn a_search_requires_a_verified_session_and_reports_offline() {
    let mut machine = machine_with(LibraryCache::default());
    assert_eq!(machine.access(), Access::Unverified);
    assert!(search(&mut machine, "celeste").is_none());
    let catalog = machine.view().unwrap().catalog;
    assert_eq!(catalog.status, CatalogStatus::Offline);
    assert!(catalog.results.is_empty());

    machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: 10,
    });
    // Still offline until the user retries; becoming verified does not send by itself.
    assert_eq!(
        machine.view().unwrap().catalog.status,
        CatalogStatus::Offline
    );
    assert!(search(&mut machine, "celeste").is_some());

    let (op, _) = search(&mut machine, "celeste").unwrap();
    machine.handle(Input::CatalogSearchDone {
        op,
        query: "celeste".to_owned(),
        result: Ok(page(vec![summary(2, "Celeste", Some(2018))])),
    });
    assert_eq!(machine.view().unwrap().catalog.status, CatalogStatus::Ready);
    machine.handle(Input::SetAccess {
        access: Access::Unverified,
        now: 11,
    });
    assert_eq!(
        machine.view().unwrap().catalog.status,
        CatalogStatus::Offline
    );
}

#[test]
fn the_latest_query_wins_and_an_older_answer_is_dropped() {
    let mut machine = verified(LibraryCache::default());
    let (first_op, _) = search(&mut machine, "cel").unwrap();
    let (second_op, _) = search(&mut machine, "celeste").unwrap();
    assert!(second_op > first_op);
    assert_eq!(
        machine.view().unwrap().catalog.status,
        CatalogStatus::Loading
    );

    machine.handle(Input::CatalogSearchDone {
        op: first_op,
        query: "cel".to_owned(),
        result: Ok(page(vec![summary(1, "Cel", None)])),
    });
    let catalog = machine.view().unwrap().catalog;
    assert_eq!(catalog.status, CatalogStatus::Loading);
    assert!(catalog.results.is_empty());

    machine.handle(Input::CatalogSearchDone {
        op: second_op,
        query: "celeste".to_owned(),
        result: Ok(SearchPage {
            games: vec![
                summary(2, "Celeste", Some(2018)),
                summary(3, "Celeste Classic", Some(2012)),
            ],
            complete: Some(false),
        }),
    });
    let catalog = machine.view().unwrap().catalog;
    assert_eq!(catalog.status, CatalogStatus::Ready);
    assert_eq!(catalog.results.len(), 2);
    assert!(catalog.incomplete);
    assert_eq!(catalog.results[0].title, "Celeste");
}

#[test]
fn an_answer_that_echoes_another_query_is_dropped() {
    let mut machine = verified(LibraryCache::default());
    let (op, _) = search(&mut machine, "celeste").unwrap();
    machine.handle(Input::CatalogSearchDone {
        op,
        query: "cel".to_owned(),
        result: Ok(page(vec![summary(2, "Celeste", Some(2018))])),
    });
    let catalog = machine.view().unwrap().catalog;
    assert_eq!(catalog.status, CatalogStatus::Loading);
    assert!(catalog.results.is_empty());
}

#[test]
fn clearing_a_query_invalidates_an_in_flight_answer() {
    let mut machine = verified(LibraryCache::default());
    let (op, _) = search(&mut machine, "celeste").unwrap();
    assert!(search(&mut machine, "c").is_none());
    machine.handle(Input::CatalogSearchDone {
        op,
        query: "celeste".to_owned(),
        result: Ok(page(vec![summary(2, "Celeste", Some(2018))])),
    });
    let catalog = machine.view().unwrap().catalog;
    assert_eq!(catalog.status, CatalogStatus::Idle);
    assert!(catalog.results.is_empty());

    let (op, _) = search(&mut machine, "celeste").unwrap();
    machine.handle(Input::CatalogClear);
    machine.handle(Input::CatalogSearchDone {
        op,
        query: "celeste".to_owned(),
        result: Ok(page(vec![summary(2, "Celeste", Some(2018))])),
    });
    let catalog = machine.view().unwrap().catalog;
    assert_eq!(catalog.status, CatalogStatus::Idle);
    assert!(catalog.query.is_empty());
    assert!(catalog.results.is_empty());
}

#[test]
fn results_read_membership_from_the_account_cache_and_the_local_row_by_game_id() {
    let cache = LibraryCache {
        entries: vec![stored("steam", "620", "Hollow Knight", true)],
        auto_matches: vec![AutoMatch {
            category: 1,
            uid: "620".to_owned(),
            game_ids: vec![7],
            checked_at: 1,
        }],
        games: vec![GameRecord {
            game_id: 7,
            name: "Hollow Knight".to_owned(),
            release_year: Some(2017),
            cover_url: None,
            cover_file: None,
            cover_failed_at: None,
        }],
        account_library: Some(AccountLibrary {
            entries: [
                (
                    7,
                    AccountEntry {
                        id: "entry-7".to_owned(),
                        game_id: 7,
                        status: Some(EntryStatus::Finished),
                        collection_ids: vec!["backlog".to_owned()],
                    },
                ),
                (
                    99,
                    AccountEntry {
                        id: "entry-99".to_owned(),
                        game_id: 99,
                        status: Some(EntryStatus::Planned),
                        collection_ids: Vec::new(),
                    },
                ),
            ]
            .into_iter()
            .collect(),
            collections: Vec::new(),
            read_at: 5,
        }),
        ..LibraryCache::default()
    };
    let mut machine = verified(cache);
    let (op, _) = search(&mut machine, "hollow").unwrap();
    machine.handle(Input::CatalogSearchDone {
        op,
        query: "hollow".to_owned(),
        result: Ok(page(vec![
            summary(7, "Hollow Knight", Some(2017)),
            summary(99, "Account only", Some(2020)),
            summary(5, "Not owned", None),
        ])),
    });
    let catalog = machine.view().unwrap().catalog;
    assert_eq!(catalog.results[0].local_key.as_deref(), Some("steam:620"));
    assert!(catalog.results[0].in_library);
    assert_eq!(catalog.results[1].local_key, None);
    assert!(catalog.results[1].in_library);
    assert!(!catalog.results[2].in_library);
    assert_eq!(catalog.results[2].local_key, None);

    // Account-only games never become library rows.
    let view = machine.view().unwrap();
    assert_eq!(view.rows.len(), 1);
    assert_eq!(view.total, 1);
}

#[test]
fn membership_refreshes_after_an_add_or_undo_without_another_search() {
    let cache = LibraryCache {
        entries: vec![stored("steam", "620", "Hollow Knight", true)],
        account_library: Some(AccountLibrary::default()),
        ..LibraryCache::default()
    };
    let mut machine = verified(cache);
    let (op, _) = search(&mut machine, "celeste").unwrap();
    machine.handle(Input::CatalogSearchDone {
        op,
        query: "celeste".to_owned(),
        result: Ok(page(vec![summary(7, "Hollow Knight", Some(2017))])),
    });
    assert!(!machine.view().unwrap().catalog.results[0].in_library);

    machine.record_entry_read(
        7,
        &Ok(Some(crate::game_api::LibraryEntry {
            id: "entry-7".to_owned(),
            status: Some(EntryStatus::Playing),
            collection_ids: Vec::new(),
            platform_ids: Vec::new(),
            finished_at: None,
        })),
    );
    machine.view_dirty = true;
    assert!(machine.view().unwrap().catalog.results[0].in_library);

    machine.record_entry_read(7, &Ok(None));
    machine.view_dirty = true;
    assert!(!machine.view().unwrap().catalog.results[0].in_library);
}

#[test]
fn a_failed_search_shows_the_error_state_and_keeps_the_query() {
    let mut machine = verified(LibraryCache::default());
    let (op, _) = search(&mut machine, "celeste").unwrap();
    machine.handle(Input::CatalogSearchDone {
        op,
        query: "celeste".to_owned(),
        result: Err(ApiError::Failed),
    });
    let catalog = machine.view().unwrap().catalog;
    assert_eq!(catalog.status, CatalogStatus::Error);
    assert_eq!(catalog.query, "celeste");
    assert!(catalog.results.is_empty());
}

#[test]
fn opening_add_for_a_catalog_game_records_metadata_without_creating_a_row() {
    let (mut machine, _) = loaded(false, LibraryCache::default());
    machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: 10,
    });
    let effects = machine.handle(Input::OpenAddForGame {
        game_id: 42,
        title: "Hollow Knight".to_owned(),
        cover_url: Some("https://covers.test/42.jpg".to_owned()),
    });
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::PersistMatches { games, .. }
            if games.iter().any(|game| game.game_id == 42
                && game.name == "Hollow Knight"
                && game.cover_url.as_deref() == Some("https://covers.test/42.jpg"))
    )));
    let view = machine.view().unwrap();
    assert!(view.rows.is_empty(), "no local row is invented");
    let dialog = view.add_dialog.expect("the catalog add dialog");
    assert_eq!(dialog.mode, crate::library::AddMode::Catalog);
    assert_eq!(
        dialog.targets[0].target,
        crate::library::AddTarget::Game(42)
    );
    assert_eq!(dialog.targets[0].title, "Hollow Knight");
    assert!(dialog.targets[0].matched);
    assert_eq!(
        machine.game(42).map(|game| game.name.as_str()),
        Some("Hollow Knight")
    );
}

#[test]
fn open_add_for_a_game_keeps_known_metadata_when_the_input_omits_it() {
    let cache = LibraryCache {
        games: vec![GameRecord {
            game_id: 42,
            name: "Known title".to_owned(),
            release_year: Some(2017),
            cover_url: Some("https://covers.test/known.jpg".to_owned()),
            cover_file: Some("42.jpg".to_owned()),
            cover_failed_at: None,
        }],
        ..LibraryCache::default()
    };
    let mut machine = verified(cache);
    machine.handle(Input::OpenAddForGame {
        game_id: 42,
        title: "  ".to_owned(),
        cover_url: None,
    });
    let game = machine.game(42).expect("the metadata");
    assert_eq!(game.name, "Known title");
    assert_eq!(game.release_year, Some(2017));
    assert_eq!(
        game.cover_url.as_deref(),
        Some("https://covers.test/known.jpg")
    );
    assert_eq!(game.cover_file.as_deref(), Some("42.jpg"));
}

#[test]
fn catalog_covers_use_the_bounded_download_queue_and_publish_local_files() {
    let mut machine = verified(LibraryCache::default());
    let (op, query) = search(&mut machine, "aster").unwrap();
    let games = (100..106)
        .map(|id| GameSummary {
            cover_url: Some(format!(
                "//images.igdb.com/igdb/image/upload/t_thumb/co{id}.jpg"
            )),
            ..summary(id, "Aster", None)
        })
        .collect();
    let effects = machine.handle(Input::CatalogSearchDone {
        op,
        query,
        result: Ok(page(games)),
    });
    assert_eq!(
        effects
            .iter()
            .filter(|effect| matches!(effect, Effect::FetchCover { .. }))
            .count(),
        4
    );
    assert!(
        machine
            .view()
            .unwrap()
            .catalog
            .results
            .iter()
            .all(|game| game.cover_file.is_none())
    );
    let effects = machine.handle(Input::CoverDone {
        game_id: 100,
        result: Ok("100.jpg".into()),
        now: 10,
    });
    assert_eq!(
        effects
            .iter()
            .filter(|effect| matches!(effect, Effect::FetchCover { .. }))
            .count(),
        1
    );
    assert_eq!(
        machine.view().unwrap().catalog.results[0]
            .cover_file
            .as_deref(),
        Some("100.jpg")
    );
}

#[test]
fn catalog_artwork_follows_selection_and_ignores_late_answers() {
    let mut machine = verified(LibraryCache::default());
    machine.handle(Input::SetDetailOpen(false));
    let (op, query) = search(&mut machine, "aster").unwrap();
    machine.handle(Input::CatalogSearchDone {
        op,
        query,
        result: Ok(page(vec![
            summary(100, "Aster", None),
            summary(101, "Aster II", None),
        ])),
    });
    let effects = machine.handle(Input::CatalogFocus(Some(100)));
    let op = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::FetchArtwork { op, game_id: 100 } => Some(*op),
            _ => None,
        })
        .unwrap();
    assert!(machine.handle(Input::CatalogFocus(Some(100))).is_empty());
    machine.handle(Input::CatalogFocus(Some(101)));
    machine.handle(Input::ArtworkDone {
        op,
        game_id: 100,
        result: Ok(Some(vec![1, 2, 3])),
    });
    assert!(!machine.view().unwrap().catalog.results[0].has_artwork);
    assert!(machine.handle(Input::CatalogFocus(Some(100))).is_empty());
    assert!(machine.view().unwrap().catalog.results[0].has_artwork);
    assert!(machine.artwork_for(100).is_some());
    machine.handle(Input::CatalogFocus(None));
    assert!(machine.artwork_for(100).is_none());
    assert!(machine.handle(Input::CatalogFocus(Some(999))).is_empty());
    machine.handle(Input::CatalogClear);
    assert!(machine.catalog.focused_game.is_none());
}
