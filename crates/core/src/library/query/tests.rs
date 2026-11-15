use super::*;
use crate::game_api::{AccountEntry, Collection};
use crate::library::layout::{GameNodeStatusFilter, GroupBy, LibraryScope, RECENT_DAYS};
use crate::library::machine::Input;
use crate::library::machine::tests::{finish_scan, loaded, record, stored, stored_with};
use crate::library::matching::MatchSource;
use crate::library::store::{AccountLibrary, AutoMatch, ChosenMatch, GameRecord, LibraryCache};
use crate::library::{Effect, SavedViewPayload};
use std::collections::BTreeMap;

fn row(provider: &str, id: &str, title: &str, status: MatchStatus) -> Row {
    Row {
        playtime: Default::default(),
        running: false,
        session_started_at: None,
        key: format!("{provider}:{id}"),
        title: title.to_owned(),
        detected_name: title.to_owned(),
        matched_name: None,
        provider: provider.to_owned(),
        provider_name: provider.to_uppercase(),
        installed: true,
        owned: false,
        shared: false,
        status,
        cover_file: None,
        release_year: None,
        added_at: 0,
        is_favorite: false,
        hidden: false,
        gamenode: None,
    }
}

fn gamenode(status: Option<EntryStatus>, collections: &[&str]) -> RowGameNode {
    RowGameNode {
        status,
        collection_ids: collections.iter().map(|id| (*id).to_owned()).collect(),
        entry_id: format!("entry-{status:?}"),
    }
}

fn installed_filter() -> LibraryFilters {
    LibraryFilters {
        availability: AvailabilityFilter {
            installed: true,
            ..AvailabilityFilter::default()
        },
        ..LibraryFilters::default()
    }
}

fn matched(game_id: u64) -> MatchStatus {
    MatchStatus::Matched {
        game_id,
        source: MatchSource::Automatic,
    }
}

fn keys(rows: &[Row]) -> Vec<&str> {
    rows.iter().map(|row| row.key.as_str()).collect()
}

fn sample() -> Vec<Row> {
    vec![
        row("steam", "3", "Portal 2", matched(1)),
        row("shop", "1", "portal", MatchStatus::Unmatched),
        row("steam", "2", "Half-Life", MatchStatus::NoGame),
        row(
            "shop",
            "4",
            "Zork",
            MatchStatus::Ambiguous { candidates: vec![] },
        ),
    ]
}

fn providers() -> Vec<(String, String)> {
    vec![
        ("steam".to_owned(), "Steam".to_owned()),
        ("gog".to_owned(), "GOG".to_owned()),
        ("shop".to_owned(), "Shop".to_owned()),
    ]
}

fn filters(change: impl FnOnce(&mut LibraryFilters)) -> LibraryFilters {
    let mut filters = LibraryFilters::default();
    change(&mut filters);
    filters
}

fn playtime(seconds: Option<i64>, last_played_at: Option<i64>) -> crate::playtime::PlaytimeView {
    crate::playtime::PlaytimeView {
        seconds,
        source: crate::playtime::PlaytimeSource::Store,
        last_played_at,
    }
}

fn prepared<'a>(
    rows: &'a [Row],
    scope: &'a LibraryScope,
    filters: &'a LibraryFilters,
    search: &str,
    now: i64,
) -> Prepared<'a> {
    Prepared::new(rows, scope, filters, search, now)
}

#[test]
fn filters_by_scope_and_searches_case_insensitively() {
    let rows = sample();
    let all = LibraryScope::All;
    let steam = filters(|f| f.stores = vec!["steam".to_owned()]);

    assert_eq!(
        keys(&filter(&rows, &all, &LibraryFilters::default(), "", 0)),
        keys(&rows)
    );
    assert_eq!(
        keys(&filter(&rows, &all, &steam, "", 0)),
        ["steam:3", "steam:2"]
    );
    assert_eq!(
        keys(&filter(&rows, &all, &LibraryFilters::default(), "PORT", 0)),
        ["steam:3", "shop:1"]
    );
    // Store filter and search combine.
    assert_eq!(keys(&filter(&rows, &all, &steam, " port ", 0)), ["steam:3"]);
    // Several stores are alternatives within the facet.
    let both = filters(|f| f.stores = vec!["steam".to_owned(), "shop".to_owned()]);
    assert_eq!(filter(&rows, &all, &both, "", 0).len(), 4);
    // Blank search (only spaces) keeps everything.
    assert_eq!(
        filter(&rows, &all, &LibraryFilters::default(), "   ", 0).len(),
        4
    );
}

#[test]
fn search_covers_the_matched_title_as_well_as_the_detected_one() {
    let mut rows = sample();
    rows[0].matched_name = Some("Portal 2: Aperture".to_owned());
    rows[0].title = "Portal 2: Aperture".to_owned();
    let all = LibraryScope::All;

    assert_eq!(
        keys(&filter(
            &rows,
            &all,
            &LibraryFilters::default(),
            "aperture",
            0
        )),
        ["steam:3"]
    );
    // The detected title still finds it.
    assert_eq!(
        keys(&filter(
            &rows,
            &all,
            &LibraryFilters::default(),
            "portal 2",
            0
        )),
        ["steam:3"]
    );
}

#[test]
fn nothing_matches_gives_an_empty_list_not_an_error() {
    let rows = sample();
    assert!(
        filter(
            &rows,
            &LibraryScope::All,
            &LibraryFilters::default(),
            "zzz",
            0
        )
        .is_empty()
    );
    assert!(
        filter(
            &rows,
            &LibraryScope::All,
            &filters(|f| f.stores = vec!["nowhere".to_owned()]),
            "",
            0
        )
        .is_empty()
    );
    assert!(filter(&[], &LibraryScope::All, &LibraryFilters::default(), "", 0).is_empty());
}

#[test]
fn sorts_by_title_in_both_directions() {
    let by = |direction| Sort {
        key: SortKey::Title,
        direction,
    };
    let mut rows = sample();
    sort(&mut rows, by(SortDirection::Ascending));
    // Case-insensitive: "Half-Life" < "portal" < "Portal 2" < "Zork".
    assert_eq!(keys(&rows), ["steam:2", "shop:1", "steam:3", "shop:4"]);
    sort(&mut rows, by(SortDirection::Descending));
    assert_eq!(keys(&rows), ["shop:4", "steam:3", "shop:1", "steam:2"]);
}

#[test]
fn equal_titles_are_ordered_by_provider_then_key_in_either_direction() {
    let mut rows = vec![
        row("steam", "9", "Same", MatchStatus::Unmatched),
        row("shop", "2", "same", MatchStatus::Unmatched),
        row("steam", "1", "Same", MatchStatus::Unmatched),
        row("shop", "1", "same", MatchStatus::Unmatched),
    ];
    for direction in [SortDirection::Ascending, SortDirection::Descending] {
        rows.reverse();
        sort(
            &mut rows,
            Sort {
                key: SortKey::Title,
                direction,
            },
        );
        let expected: [&str; 4] = match direction {
            SortDirection::Ascending => ["steam:1", "steam:9", "shop:1", "shop:2"],
            SortDirection::Descending => ["shop:1", "shop:2", "steam:1", "steam:9"],
        };
        assert_eq!(keys(&rows), expected, "{direction:?}");
    }
}

#[test]
fn sorts_by_provider_with_titles_breaking_ties() {
    let mut rows = sample();
    sort(
        &mut rows,
        Sort {
            key: SortKey::Provider,
            direction: SortDirection::Ascending,
        },
    );
    assert_eq!(keys(&rows), ["shop:1", "shop:4", "steam:2", "steam:3"]);
    sort(
        &mut rows,
        Sort {
            key: SortKey::Provider,
            direction: SortDirection::Descending,
        },
    );
    assert_eq!(keys(&rows), ["steam:2", "steam:3", "shop:1", "shop:4"]);
}

#[test]
fn sorts_by_match_status() {
    let mut rows = sample();
    sort(
        &mut rows,
        Sort {
            key: SortKey::MatchStatus,
            direction: SortDirection::Ascending,
        },
    );
    assert_eq!(keys(&rows), ["steam:3", "shop:4", "shop:1", "steam:2"]);
    sort(
        &mut rows,
        Sort {
            key: SortKey::MatchStatus,
            direction: SortDirection::Descending,
        },
    );
    assert_eq!(keys(&rows), ["steam:2", "shop:1", "shop:4", "steam:3"]);
}

#[test]
fn game_node_status_sort_uses_its_own_rank_and_flips_with_direction() {
    // The sort rank differs from the group order: ongoing comes before planned.
    let mut rows = vec![
        row(
            "steam",
            "1",
            "Playing",
            MatchStatus::Matched {
                game_id: 1,
                source: MatchSource::Automatic,
            },
        ),
        row(
            "steam",
            "2",
            "Finished",
            MatchStatus::Matched {
                game_id: 2,
                source: MatchSource::Automatic,
            },
        ),
        row(
            "steam",
            "3",
            "Planned",
            MatchStatus::Matched {
                game_id: 3,
                source: MatchSource::Automatic,
            },
        ),
        row(
            "steam",
            "4",
            "Ongoing",
            MatchStatus::Matched {
                game_id: 4,
                source: MatchSource::Automatic,
            },
        ),
        row(
            "steam",
            "5",
            "Dropped",
            MatchStatus::Matched {
                game_id: 5,
                source: MatchSource::Automatic,
            },
        ),
        row(
            "steam",
            "6",
            "In library",
            MatchStatus::Matched {
                game_id: 6,
                source: MatchSource::Automatic,
            },
        ),
        row("steam", "7", "Not in library", MatchStatus::Unmatched),
    ];
    let statuses = [
        Some(EntryStatus::Playing),
        Some(EntryStatus::Finished),
        Some(EntryStatus::Planned),
        Some(EntryStatus::Ongoing),
        Some(EntryStatus::Dropped),
        None,
    ];
    for (row, status) in rows.iter_mut().take(6).zip(statuses) {
        row.gamenode = Some(gamenode(status, &[]));
    }

    let sorted = |direction| {
        let mut rows = rows.clone();
        sort(
            &mut rows,
            Sort {
                key: SortKey::GameNodeStatus,
                direction,
            },
        );
        rows.into_iter().map(|row| row.key).collect::<Vec<_>>()
    };
    assert_eq!(
        sorted(SortDirection::Ascending),
        [
            "steam:1", "steam:4", "steam:3", "steam:2", "steam:5", "steam:6", "steam:7"
        ]
    );
    assert_eq!(
        sorted(SortDirection::Descending),
        [
            "steam:7", "steam:6", "steam:5", "steam:2", "steam:3", "steam:4", "steam:1"
        ]
    );

    // Ties break by title, then key, in both directions; the direction only flips the rank.
    let mut tied = vec![
        row(
            "steam",
            "9",
            "Zulu",
            MatchStatus::Matched {
                game_id: 9,
                source: MatchSource::Automatic,
            },
        ),
        row(
            "steam",
            "1",
            "Alpha",
            MatchStatus::Matched {
                game_id: 1,
                source: MatchSource::Automatic,
            },
        ),
    ];
    for row in &mut tied {
        row.gamenode = Some(gamenode(Some(EntryStatus::Playing), &[]));
    }
    for direction in [SortDirection::Ascending, SortDirection::Descending] {
        tied.reverse();
        sort(
            &mut tied,
            Sort {
                key: SortKey::GameNodeStatus,
                direction,
            },
        );
        assert_eq!(keys(&tied), ["steam:1", "steam:9"], "{direction:?}");
    }
}

#[test]
fn sorting_does_not_depend_on_the_input_order() {
    let ascending = Sort::default();
    let mut forward = sample();
    let mut backward = sample();
    backward.reverse();
    sort(&mut forward, ascending);
    sort(&mut backward, ascending);
    assert_eq!(forward, backward);
}

#[test]
fn counts_rows_per_provider_in_the_order_given() {
    let mut rows = sample();
    rows[3].provider = "gog".to_owned();
    let counted = counts(
        &rows,
        [
            ("steam", "Steam"),
            ("shop", "Shop"),
            ("gog", "GOG"),
            ("none", "None"),
        ],
    );
    let pairs: Vec<_> = counted.iter().map(|c| (c.id.as_str(), c.count)).collect();
    assert_eq!(pairs, [("steam", 2), ("shop", 1), ("gog", 1), ("none", 0)]);
    assert_eq!(counted[0].name, "Steam");
}

#[test]
fn provider_counts_reach_the_frontend_with_the_keys_it_reads() {
    let counted = counts(&sample(), [("steam", "Steam")]);
    let json = serde_json::to_value(&counted[0]).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "id": "steam",
            "name": "Steam",
            "count": 2,
            "installedCount": 0,
            "ownedCount": 0,
            "sharedCount": 0,
            "sharedLabel": null
        })
    );
}

#[test]
fn the_machine_shows_only_installed_games_with_matched_titles_and_covers() {
    let cache = LibraryCache {
        entries: vec![
            stored("steam", "620", "portal 2", true),
            stored("steam", "70", "Half-Life", true),
            stored("steam", "999", "Uninstalled", false),
        ],
        auto_matches: vec![AutoMatch {
            category: 1,
            uid: "620".to_owned(),
            game_ids: vec![7],
            checked_at: 1,
        }],
        chosen_matches: vec![ChosenMatch {
            entry_key: "steam:70".to_owned(),
            game_id: None,
            chosen_at: 1,
            source: Default::default(),
        }],
        games: vec![GameRecord {
            game_id: 7,
            name: "Portal 2".to_owned(),
            release_year: Some(2011),
            cover_url: None,
            cover_file: Some("7.jpg".to_owned()),
            cover_failed_at: None,
        }],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    finish_scan(
        &mut machine,
        vec![
            record("620", "portal 2", "/p"),
            record("70", "Half-Life", "/h"),
        ],
        9,
    );

    let rows = machine.visible_rows("");
    assert_eq!(keys(&rows), ["steam:70", "steam:620"]);
    let portal = &rows[1];
    assert_eq!(portal.title, "Portal 2", "the matched title is shown");
    assert_eq!(portal.detected_name, "portal 2");
    assert_eq!(portal.cover_file.as_deref(), Some("7.jpg"));
    assert_eq!(rows[0].status, MatchStatus::NoGame);
    assert_eq!(rows[0].cover_file, None);

    assert_eq!(keys(&machine.visible_rows("aperture")), Vec::<&str>::new());
    assert_eq!(machine.provider_counts()[0].count, 2);
}

#[test]
fn an_unknown_saved_store_filter_matches_nothing() {
    let (mut machine, _) = loaded(false, LibraryCache::default());
    finish_scan(&mut machine, vec![record("1", "One", "/1")], 9);
    machine.layout.scope = LibraryScope::All;
    machine.layout.filters.stores = vec!["removed".to_owned()];
    assert!(machine.visible_rows("").is_empty());
    assert_eq!(
        machine.query("").title_source.unwrap(),
        TitleSource {
            kind: TitleSourceKind::Scope,
            id: "all".to_owned(),
        }
    );
}

#[test]
fn the_installed_filter_composes_with_the_scope_and_search() {
    let mut rows = sample();
    rows[0].installed = true;
    rows[1].installed = false;
    rows[2].installed = false;
    rows[3].installed = true;
    let installed = installed_filter();
    let all = LibraryScope::All;

    assert_eq!(
        keys(&filter(&rows, &all, &installed, "", 0)),
        ["steam:3", "shop:4"]
    );
    assert_eq!(
        keys(&filter(
            &rows,
            &LibraryScope::All,
            &filters(|f| {
                f.availability = installed.availability;
                f.stores = vec!["steam".to_owned()];
            }),
            "",
            0
        )),
        ["steam:3"]
    );
    assert_eq!(
        keys(&filter(
            &rows,
            &LibraryScope::All,
            &filters(|f| {
                f.availability = installed.availability;
                f.stores = vec!["shop".to_owned()];
            }),
            "zork",
            0
        )),
        ["shop:4"]
    );
    assert!(
        filter(
            &rows,
            &LibraryScope::All,
            &filters(|f| {
                f.availability = installed.availability;
                f.stores = vec!["steam".to_owned()];
            }),
            "half",
            0
        )
        .is_empty()
    );
}

#[test]
fn an_owned_only_game_is_a_row_with_unknown_playtime() {
    let cache = LibraryCache {
        entries: vec![stored_with("steam", "70", "Half-Life", false, true, false)],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    finish_scan(&mut machine, vec![], 9);

    let rows = machine.visible_rows("");
    assert_eq!(keys(&rows), ["steam:70"]);
    assert!(!rows[0].installed && rows[0].owned && !rows[0].shared);
    assert_eq!(
        rows[0].playtime.seconds, None,
        "unknown, not a made-up zero"
    );

    machine.set_playtime(std::collections::BTreeMap::from([(
        "steam:70".to_owned(),
        crate::playtime::PlaytimeView {
            seconds: None,
            source: crate::playtime::PlaytimeSource::Internal,
            last_played_at: None,
        },
    )]));
    let rows = machine.visible_rows("");
    assert_eq!(rows[0].playtime.seconds, None);
    assert_eq!(
        rows[0].playtime.source,
        crate::playtime::PlaytimeSource::Internal
    );
}

#[test]
fn provider_counts_add_up_each_visible_fact() {
    let cache = LibraryCache {
        entries: vec![
            stored("steam", "1", "Installed", true),
            stored_with("steam", "2", "Owned", false, true, false),
            stored_with("steam", "3", "Shared", false, false, true),
            stored_with("steam", "4", "Owned and shared", false, true, true),
            stored_with("steam", "5", "Gone", false, false, false),
        ],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    finish_scan(&mut machine, vec![record("1", "Installed", "/1")], 9);

    let count = &machine.provider_counts()[0];
    assert_eq!((count.count, count.installed_count), (4, 1));
    assert_eq!((count.owned_count, count.shared_count), (2, 1));
    let rows = machine.visible_rows("");
    assert_eq!(
        keys(&rows),
        ["steam:1", "steam:2", "steam:4", "steam:3"],
        "invisible entries are not rows"
    );
    let shared = rows.iter().find(|row| row.key == "steam:3").unwrap();
    assert!(shared.shared && !shared.owned);
    let both = rows.iter().find(|row| row.key == "steam:4").unwrap();
    assert!(both.owned && !both.shared, "owned wins in the row");
}

#[test]
fn availability_facts_are_alternatives_and_empty_means_any() {
    let mut rows = vec![
        row("steam", "1", "Installed", MatchStatus::Unmatched),
        row("steam", "2", "Owned", MatchStatus::Unmatched),
        row("steam", "3", "Shared", MatchStatus::Unmatched),
    ];
    rows[0].installed = true;
    rows[1].installed = false;
    rows[1].owned = true;
    rows[2].installed = false;
    rows[2].shared = true;

    let all = LibraryScope::All;
    assert_eq!(
        filter(&rows, &all, &LibraryFilters::default(), "", 0).len(),
        3,
        "no availability fact selected keeps everything"
    );
    let installed = filters(|f| f.availability.installed = true);
    assert_eq!(keys(&filter(&rows, &all, &installed, "", 0)), ["steam:1"]);
    let owned_or_shared = filters(|f| {
        f.availability.owned = true;
        f.availability.shared = true;
    });
    assert_eq!(
        keys(&filter(&rows, &all, &owned_or_shared, "", 0)),
        ["steam:2", "steam:3"]
    );
    let installed_and_shared = filters(|f| {
        f.availability.installed = true;
        f.availability.shared = true;
    });
    assert_eq!(
        keys(&filter(&rows, &all, &installed_and_shared, "", 0)),
        ["steam:1", "steam:3"],
        "selected facts are alternatives"
    );
}

#[test]
fn each_state_facet_narrows_the_rows() {
    let mut rows = vec![
        row("steam", "1", "Matched", matched(1)),
        row(
            "steam",
            "2",
            "Ambiguous",
            MatchStatus::Ambiguous { candidates: vec![] },
        ),
        row("steam", "3", "Unmatched", MatchStatus::Unmatched),
        row("steam", "4", "No game", MatchStatus::NoGame),
    ];
    rows[0].running = true;
    rows[0].is_favorite = true;
    let all = LibraryScope::All;

    let running = filters(|f| f.running_only = true);
    assert_eq!(keys(&filter(&rows, &all, &running, "", 0)), ["steam:1"]);
    for (state, expected) in [
        (MatchFilterValue::Matched, "steam:1"),
        (MatchFilterValue::Ambiguous, "steam:2"),
        (MatchFilterValue::Unmatched, "steam:3"),
        (MatchFilterValue::NoGame, "steam:4"),
    ] {
        let by_state = filters(|f| f.match_states = vec![state]);
        assert_eq!(keys(&filter(&rows, &all, &by_state, "", 0)), [expected]);
    }
    let favorites = filters(|f| f.favorites_only = true);
    assert_eq!(keys(&filter(&rows, &all, &favorites, "", 0)), ["steam:1"]);
    let everything = filters(|f| {
        f.running_only = true;
        f.match_states = vec![MatchFilterValue::Matched];
        f.favorites_only = true;
    });
    assert_eq!(keys(&filter(&rows, &all, &everything, "", 0)), ["steam:1"]);
    rows[0].running = false;
    assert!(filter(&rows, &all, &everything, "", 0).is_empty());
}

#[test]
fn game_node_status_facets_use_the_cached_library_and_cover_unmatched_rows() {
    let mut rows = vec![
        row("steam", "1", "Playing", matched(1)),
        row("steam", "2", "Finished", matched(2)),
        row("steam", "3", "Planned", matched(3)),
        row("steam", "4", "Ongoing", matched(4)),
        row("steam", "5", "Dropped", matched(5)),
        row("steam", "6", "Unknown status", matched(6)),
        row("steam", "7", "Unmatched", MatchStatus::Unmatched),
    ];
    for (row, status) in rows.iter_mut().take(6).zip([
        Some(EntryStatus::Playing),
        Some(EntryStatus::Finished),
        Some(EntryStatus::Planned),
        Some(EntryStatus::Ongoing),
        Some(EntryStatus::Dropped),
        None,
    ]) {
        row.gamenode = Some(gamenode(status, &[]));
    }
    let all = LibraryScope::All;
    for (status_filter, expected) in [
        (GameNodeStatusFilter::Playing, "steam:1"),
        (GameNodeStatusFilter::Finished, "steam:2"),
        (GameNodeStatusFilter::Planned, "steam:3"),
        (GameNodeStatusFilter::Ongoing, "steam:4"),
        (GameNodeStatusFilter::Dropped, "steam:5"),
        (GameNodeStatusFilter::InLibrary, "steam:6"),
        (GameNodeStatusFilter::NotInLibrary, "steam:7"),
    ] {
        let by_status = filters(|f| f.gamenode_status = vec![status_filter]);
        assert_eq!(
            keys(&filter(&rows, &all, &by_status, "", 0)),
            [expected],
            "{status_filter:?}"
        );
    }
    // Two selected statuses are alternatives, and Not on GameNode includes unmatched rows.
    let alternatives = filters(|f| {
        f.gamenode_status = vec![
            GameNodeStatusFilter::Playing,
            GameNodeStatusFilter::NotInLibrary,
        ]
    });
    assert_eq!(
        keys(&filter(&rows, &all, &alternatives, "", 0)),
        ["steam:1", "steam:7"]
    );
}

#[test]
fn play_history_and_unknown_playtime() {
    let mut rows = vec![
        row("steam", "1", "Unknown", MatchStatus::Unmatched),
        row("steam", "2", "Zero", MatchStatus::Unmatched),
        row("steam", "3", "Ten", MatchStatus::Unmatched),
        row("steam", "4", "Hundred", MatchStatus::Unmatched),
    ];
    rows[0].playtime = playtime(None, None);
    rows[1].playtime = playtime(Some(0), None);
    rows[2].playtime = playtime(Some(10 * 3600), None);
    rows[3].playtime = playtime(Some(100 * 3600), None);
    let all = LibraryScope::All;

    let unplayed = filters(|f| f.played = PlayedFilter::Unplayed);
    assert_eq!(
        keys(&filter(&rows, &all, &unplayed, "", 0)),
        ["steam:2"],
        "unknown playtime is not presented as unplayed"
    );
    let played = filters(|f| f.played = PlayedFilter::Played);
    assert_eq!(
        keys(&filter(&rows, &all, &played, "", 0)),
        ["steam:3", "steam:4"]
    );
    let ten = filters(|f| f.playtime = PlaytimeThreshold::AtLeast10h);
    assert_eq!(
        keys(&filter(&rows, &all, &ten, "", 0)),
        ["steam:3", "steam:4"]
    );
    let hundred = filters(|f| f.playtime = PlaytimeThreshold::AtLeast100h);
    assert_eq!(keys(&filter(&rows, &all, &hundred, "", 0)), ["steam:4"]);
}

#[test]
fn recent_scope_uses_13_and_15_day_boundaries() {
    const NOW: i64 = 10_000_000;
    let mut rows = vec![
        row("steam", "1", "Recent", MatchStatus::Unmatched),
        row("steam", "2", "Almost recent", MatchStatus::Unmatched),
        row("steam", "3", "Unknown", MatchStatus::Unmatched),
    ];
    rows[0].playtime = playtime(Some(1), Some(NOW - 13 * DAY_SECONDS));
    rows[1].playtime = playtime(Some(1), Some(NOW - 15 * DAY_SECONDS));
    rows[2].playtime = playtime(Some(1), None);
    assert_eq!(
        keys(&filter(
            &rows,
            &LibraryScope::Recent,
            &LibraryFilters::default(),
            "",
            NOW
        )),
        ["steam:1"]
    );
    assert_eq!(RECENT_DAYS, 14);
}

#[test]
fn each_sidebar_scope_is_its_own_predicate() {
    const NOW: i64 = 10_000_000;
    let mut rows = vec![
        row("steam", "1", "Installed", MatchStatus::Unmatched),
        row("steam", "2", "Favorite", MatchStatus::Unmatched),
        row("steam", "3", "Running", MatchStatus::Unmatched),
        row("steam", "4", "Recent", MatchStatus::Unmatched),
        row("steam", "5", "In collection", MatchStatus::Unmatched),
    ];
    rows[0].installed = true;
    for row in rows.iter_mut().skip(1) {
        row.installed = false;
    }
    rows[1].is_favorite = true;
    rows[2].running = true;
    rows[3].playtime = playtime(Some(1), Some(NOW - DAY_SECONDS));
    rows[4].gamenode = Some(gamenode(None, &["coop"]));
    for (scope, expected) in [
        (LibraryScope::Installed, "steam:1"),
        (LibraryScope::Favorites, "steam:2"),
        (LibraryScope::Running, "steam:3"),
        (LibraryScope::Recent, "steam:4"),
        (LibraryScope::Collection("coop".to_owned()), "steam:5"),
    ] {
        assert_eq!(
            keys(&filter(&rows, &scope, &LibraryFilters::default(), "", NOW)),
            [expected],
            "{scope:?}"
        );
    }
    assert_eq!(
        filter(
            &rows,
            &LibraryScope::All,
            &LibraryFilters::default(),
            "",
            NOW
        )
        .len(),
        5
    );
}

#[test]
fn relative_windows_use_now_and_exclude_unknown_values() {
    const NOW: i64 = 10_000_000;
    let mut rows = vec![
        row("steam", "1", "Recent", MatchStatus::Unmatched),
        row("steam", "2", "Old", MatchStatus::Unmatched),
        row("steam", "3", "Unknown", MatchStatus::Unmatched),
    ];
    rows[0].playtime = playtime(Some(1), Some(NOW - DAY_SECONDS));
    rows[0].added_at = NOW - 2 * DAY_SECONDS;
    rows[1].playtime = playtime(Some(1), Some(NOW - 200 * DAY_SECONDS));
    rows[1].added_at = NOW - 200 * DAY_SECONDS;
    rows[2].playtime = playtime(Some(1), None);
    rows[2].added_at = NOW - DAY_SECONDS;
    let all = LibraryScope::All;

    let week = filters(|f| f.last_played = LastPlayedFilter::Last7Days);
    assert_eq!(keys(&filter(&rows, &all, &week, "", NOW)), ["steam:1"]);
    let month = filters(|f| f.last_played = LastPlayedFilter::Last30Days);
    assert_eq!(keys(&filter(&rows, &all, &month, "", NOW)), ["steam:1"]);
    let quarter = filters(|f| f.last_played = LastPlayedFilter::Last90Days);
    assert_eq!(keys(&filter(&rows, &all, &quarter, "", NOW)), ["steam:1"]);
    let year = filters(|f| f.last_played = LastPlayedFilter::LastYear);
    assert_eq!(
        keys(&filter(&rows, &all, &year, "", NOW)),
        ["steam:1", "steam:2"]
    );
    let older = filters(|f| f.last_played = LastPlayedFilter::OlderThanYear);
    assert!(
        filter(&rows, &all, &older, "", NOW).is_empty(),
        "unknown timestamps and recent rows never count as older"
    );
    let added_week = filters(|f| f.added = AddedFilter::Last7Days);
    assert_eq!(
        keys(&filter(&rows, &all, &added_week, "", NOW)),
        ["steam:1", "steam:3"]
    );
    let added_quarter = filters(|f| f.added = AddedFilter::Last90Days);
    assert_eq!(
        keys(&filter(&rows, &all, &added_quarter, "", NOW)),
        ["steam:1", "steam:3"]
    );
    let added_old = filters(|f| f.added = AddedFilter::LastYear);
    assert_eq!(
        keys(&filter(&rows, &all, &added_old, "", NOW)),
        ["steam:1", "steam:2", "steam:3"]
    );
    let added_older = filters(|f| f.added = AddedFilter::OlderThanYear);
    assert!(filter(&rows, &all, &added_older, "", NOW).is_empty());
}

#[test]
fn release_decades_earlier_and_unknown() {
    let mut rows = vec![
        row("steam", "1", "Recent", MatchStatus::Unmatched),
        row("steam", "2", "Nineties", MatchStatus::Unmatched),
        row("steam", "3", "Eighties", MatchStatus::Unmatched),
        row("steam", "4", "Ancient", MatchStatus::Unmatched),
        row("steam", "5", "Unknown", MatchStatus::Unmatched),
    ];
    for (row, year) in rows
        .iter_mut()
        .zip([Some(2021), Some(1995), Some(1984), Some(1979), None])
    {
        row.release_year = year;
    }
    let all = LibraryScope::All;

    let twenties = filters(|f| f.release = ReleaseFilter::Decade(2020));
    assert_eq!(keys(&filter(&rows, &all, &twenties, "", 0)), ["steam:1"]);
    let nineties = filters(|f| f.release = ReleaseFilter::Decade(1990));
    assert_eq!(keys(&filter(&rows, &all, &nineties, "", 0)), ["steam:2"]);
    let earlier = filters(|f| f.release = ReleaseFilter::Earlier);
    assert_eq!(
        keys(&filter(&rows, &all, &earlier, "", 0)),
        ["steam:3", "steam:4"],
        "Earlier now means before 1990"
    );
    let unknown = filters(|f| f.release = ReleaseFilter::Unknown);
    assert_eq!(keys(&filter(&rows, &all, &unknown, "", 0)), ["steam:5"]);
}

#[test]
fn facets_combine_with_and_and_search_still_applies() {
    let mut rows = vec![
        row("steam", "1", "Portal Two", matched(1)),
        row("steam", "2", "Portal One", matched(2)),
        row("shop", "3", "Portal Stories", matched(3)),
    ];
    rows[0].installed = true;
    rows[0].is_favorite = true;
    rows[0].release_year = Some(2011);
    rows[1].installed = false;
    rows[1].owned = true;
    rows[1].is_favorite = true;
    rows[1].release_year = Some(2007);
    rows[2].installed = true;
    rows[2].release_year = Some(2011);

    let combined = filters(|f| {
        f.availability.installed = true;
        f.favorites_only = true;
        f.stores = vec!["steam".to_owned()];
        f.release = ReleaseFilter::Decade(2010);
    });
    assert_eq!(
        keys(&filter(&rows, &LibraryScope::All, &combined, "", 0)),
        ["steam:1"]
    );
    assert!(filter(&rows, &LibraryScope::All, &combined, "stories", 0).is_empty());
}

#[test]
fn new_sort_keys_use_their_natural_direction_and_unknowns_last() {
    let mut a = row("steam", "1", "Alpha", MatchStatus::Unmatched);
    a.release_year = Some(2020);
    a.playtime = playtime(Some(10), Some(100));
    a.added_at = 50;
    let mut b = row("steam", "2", "Bravo", MatchStatus::Unmatched);
    b.release_year = Some(2010);
    b.playtime = playtime(Some(100), Some(200));
    b.added_at = 10;
    let mut c = row("steam", "3", "Charlie", MatchStatus::Unmatched);
    c.release_year = None;
    c.playtime = playtime(None, None);
    c.added_at = 5;
    let rows = vec![a, b, c];

    let sorted = |key, direction| {
        let mut rows = rows.clone();
        sort(&mut rows, Sort { key, direction });
        rows.into_iter().map(|row| row.key).collect::<Vec<_>>()
    };
    assert_eq!(
        sorted(SortKey::ReleaseYear, SortDirection::Descending),
        ["steam:1", "steam:2", "steam:3"]
    );
    assert_eq!(
        sorted(SortKey::ReleaseYear, SortDirection::Ascending),
        ["steam:2", "steam:1", "steam:3"],
        "unknown stays last ascending"
    );
    assert_eq!(
        sorted(SortKey::Playtime, SortDirection::Descending),
        ["steam:2", "steam:1", "steam:3"]
    );
    assert_eq!(
        sorted(SortKey::Playtime, SortDirection::Ascending),
        ["steam:1", "steam:2", "steam:3"]
    );
    assert_eq!(
        sorted(SortKey::LastPlayed, SortDirection::Descending),
        ["steam:2", "steam:1", "steam:3"]
    );
    assert_eq!(
        sorted(SortKey::LastPlayed, SortDirection::Ascending),
        ["steam:1", "steam:2", "steam:3"]
    );
    assert_eq!(
        sorted(SortKey::DateAdded, SortDirection::Descending),
        ["steam:1", "steam:2", "steam:3"]
    );
    assert_eq!(
        sorted(SortKey::DateAdded, SortDirection::Ascending),
        ["steam:3", "steam:2", "steam:1"]
    );
}

#[test]
fn equal_new_key_values_keep_the_title_tie_break_in_both_directions() {
    let mut rows = vec![
        row("steam", "9", "Zulu", MatchStatus::Unmatched),
        row("steam", "1", "Alpha", MatchStatus::Unmatched),
    ];
    for row in &mut rows {
        row.playtime = playtime(Some(60), Some(10));
    }
    for direction in [SortDirection::Ascending, SortDirection::Descending] {
        rows.reverse();
        sort(
            &mut rows,
            Sort {
                key: SortKey::Playtime,
                direction,
            },
        );
        assert_eq!(keys(&rows), ["steam:1", "steam:9"], "{direction:?}");
    }
}

fn facet_fixture() -> Vec<Row> {
    let mut rows = vec![
        row("steam", "1", "Alpha", matched(1)),
        row("steam", "2", "Bravo", MatchStatus::Unmatched),
        row(
            "shop",
            "3",
            "Charlie",
            MatchStatus::Ambiguous { candidates: vec![] },
        ),
        row("shop", "4", "Delta", MatchStatus::NoGame),
        row("steam", "5", "Echo", matched(5)),
    ];
    rows[0].installed = true;
    rows[0].is_favorite = true;
    rows[0].running = true;
    rows[0].playtime = playtime(Some(12 * 3600), Some(1_000));
    rows[0].release_year = Some(2021);
    rows[0].added_at = 500;
    rows[0].gamenode = Some(gamenode(Some(EntryStatus::Playing), &["backlog"]));
    rows[1].owned = true;
    rows[1].playtime = playtime(Some(0), None);
    rows[1].release_year = Some(1995);
    rows[1].added_at = 100;
    rows[1].gamenode = Some(gamenode(None, &[]));
    rows[2].shared = true;
    rows[2].playtime = playtime(None, Some(900));
    rows[2].release_year = None;
    rows[2].added_at = 900;
    rows[2].is_favorite = true;
    rows[3].playtime = playtime(Some(101 * 3600), Some(1));
    rows[3].release_year = Some(1985);
    rows[3].added_at = 200;
    rows[3].gamenode = Some(gamenode(Some(EntryStatus::Finished), &[]));
    rows[4].playtime = playtime(Some(10 * 3600), Some(2));
    rows[4].release_year = Some(2024);
    rows[4].added_at = 300;
    rows[4].gamenode = Some(gamenode(Some(EntryStatus::Planned), &[]));
    rows
}

#[test]
fn every_facet_option_count_equals_filtering_with_that_option_alone() {
    const NOW: i64 = 1_000;
    let rows = facet_fixture();
    let scope = LibraryScope::All;
    let base = filters(|f| {
        f.availability.owned = true;
        f.stores = vec!["steam".to_owned(), "shop".to_owned()];
        f.match_states = vec![MatchFilterValue::Matched, MatchFilterValue::Unmatched];
        f.gamenode_status = vec![
            GameNodeStatusFilter::Playing,
            GameNodeStatusFilter::Finished,
            GameNodeStatusFilter::Planned,
            GameNodeStatusFilter::NotInLibrary,
        ];
        f.playtime = PlaytimeThreshold::AtLeast10h;
        f.played = PlayedFilter::Played;
        f.last_played = LastPlayedFilter::Last90Days;
        f.added = AddedFilter::Last90Days;
        f.running_only = true;
    });
    let p = prepared(&rows, &scope, &base, "", NOW);
    let counts = p.filter_counts(&providers(), true);

    let expected = |change: &dyn Fn(&mut LibraryFilters)| {
        let mut filters = base.clone();
        change(&mut filters);
        filter(&rows, &scope, &filters, "", NOW).len()
    };

    assert_eq!(
        counts.availability.installed,
        expected(&|f| f.availability = AvailabilityFilter {
            installed: true,
            ..Default::default()
        })
    );
    assert_eq!(
        counts.availability.owned,
        expected(&|f| f.availability = AvailabilityFilter {
            owned: true,
            ..Default::default()
        })
    );
    assert_eq!(
        counts.availability.shared,
        expected(&|f| f.availability = AvailabilityFilter {
            shared: true,
            ..Default::default()
        })
    );
    for (count, state) in [
        (counts.match_states.matched, MatchFilterValue::Matched),
        (counts.match_states.ambiguous, MatchFilterValue::Ambiguous),
        (counts.match_states.unmatched, MatchFilterValue::Unmatched),
        (counts.match_states.no_game, MatchFilterValue::NoGame),
    ] {
        assert_eq!(
            count,
            expected(&|f| f.match_states = vec![state]),
            "{state:?}"
        );
    }
    for (count, status) in [
        (
            counts.gamenode_status.playing,
            GameNodeStatusFilter::Playing,
        ),
        (
            counts.gamenode_status.finished,
            GameNodeStatusFilter::Finished,
        ),
        (
            counts.gamenode_status.planned,
            GameNodeStatusFilter::Planned,
        ),
        (
            counts.gamenode_status.ongoing,
            GameNodeStatusFilter::Ongoing,
        ),
        (
            counts.gamenode_status.dropped,
            GameNodeStatusFilter::Dropped,
        ),
        (
            counts.gamenode_status.in_library,
            GameNodeStatusFilter::InLibrary,
        ),
        (
            counts.gamenode_status.not_in_library,
            GameNodeStatusFilter::NotInLibrary,
        ),
    ] {
        assert_eq!(
            count,
            expected(&|f| f.gamenode_status = vec![status]),
            "{status:?}"
        );
    }
    assert_eq!(
        counts.playtime.at_least10h,
        expected(&|f| f.playtime = PlaytimeThreshold::AtLeast10h)
    );
    assert_eq!(
        counts.playtime.at_least100h,
        expected(&|f| f.playtime = PlaytimeThreshold::AtLeast100h)
    );
    assert_eq!(
        counts.played.played,
        expected(&|f| f.played = PlayedFilter::Played)
    );
    assert_eq!(
        counts.played.unplayed,
        expected(&|f| f.played = PlayedFilter::Unplayed)
    );
    for (count, window) in [
        (counts.last_played.last7_days, LastPlayedFilter::Last7Days),
        (counts.last_played.last30_days, LastPlayedFilter::Last30Days),
        (counts.last_played.last90_days, LastPlayedFilter::Last90Days),
        (counts.last_played.last_year, LastPlayedFilter::LastYear),
        (
            counts.last_played.older_than_year,
            LastPlayedFilter::OlderThanYear,
        ),
    ] {
        assert_eq!(count, expected(&|f| f.last_played = window), "{window:?}");
    }
    for (count, window) in [
        (counts.added.last7_days, AddedFilter::Last7Days),
        (counts.added.last30_days, AddedFilter::Last30Days),
        (counts.added.last90_days, AddedFilter::Last90Days),
        (counts.added.last_year, AddedFilter::LastYear),
        (counts.added.older_than_year, AddedFilter::OlderThanYear),
    ] {
        assert_eq!(count, expected(&|f| f.added = window), "{window:?}");
    }
    assert_eq!(
        counts.favorites_only,
        expected(&|f| f.favorites_only = true)
    );
    assert_eq!(counts.running_only, expected(&|f| f.running_only = true));
    for option in &counts.release {
        let value = option.value;
        assert_eq!(option.count, expected(&|f| f.release = value), "{value:?}");
    }
}

#[test]
fn release_options_list_only_populated_decades_then_earlier_and_unknown() {
    const NOW: i64 = 1_000;
    let rows = facet_fixture();
    let default_filters = LibraryFilters::default();
    let p = prepared(&rows, &LibraryScope::All, &default_filters, "", NOW);
    let counts = p.filter_counts(&providers(), true);
    let values: Vec<ReleaseFilter> = counts.release.iter().map(|count| count.value).collect();
    assert_eq!(
        values,
        [
            ReleaseFilter::Decade(2020),
            ReleaseFilter::Decade(1990),
            ReleaseFilter::Earlier,
            ReleaseFilter::Unknown,
        ]
    );
    assert_eq!(
        counts
            .release
            .iter()
            .find(|count| count.value == ReleaseFilter::Decade(2020))
            .unwrap()
            .count,
        2
    );

    // A library without 2000s games never offers that decade.
    let mut sparse = facet_fixture();
    for row in &mut sparse {
        row.release_year = Some(2021);
    }
    let sparse_filters = LibraryFilters::default();
    let sparse = prepared(&sparse, &LibraryScope::All, &sparse_filters, "", NOW);
    assert_eq!(
        sparse
            .filter_counts(&providers(), true)
            .release
            .into_iter()
            .map(|count| count.value)
            .collect::<Vec<_>>(),
        [ReleaseFilter::Decade(2020)]
    );
}

#[test]
fn store_counts_ignore_the_store_facet_and_the_scope_and_sum_to_all() {
    const NOW: i64 = 1_000;
    let rows = facet_fixture();
    let base = filters(|f| {
        f.stores = vec!["steam".to_owned()];
        f.favorites_only = true;
    });
    let p = prepared(&rows, &LibraryScope::Installed, &base, "", NOW);
    let counts = p.filter_counts(&providers(), true);

    let all_scope = LibraryScope::All;
    let mut without_stores = base.clone();
    without_stores.stores.clear();
    let expected_all = filter(&rows, &all_scope, &without_stores, "", NOW).len();
    assert_eq!(
        counts.stores.iter().map(|store| store.count).sum::<usize>(),
        expected_all
    );
    for store in &counts.stores {
        let expected = filter(&rows, &all_scope, &without_stores, "", NOW)
            .iter()
            .filter(|row| row.provider == store.id)
            .count();
        assert_eq!(store.count, expected, "{}", store.id);
    }
    assert_eq!(p.scope_counts().all, expected_all);
    assert_eq!(
        counts.stores.iter().map(|store| store.count).sum::<usize>(),
        p.scope_counts().all
    );

    let favorites = p.scope_counts().favorites;
    let mut with_favorites = without_stores.clone();
    with_favorites.favorites_only = true;
    assert_eq!(
        favorites,
        filter(&rows, &all_scope, &with_favorites, "", NOW).len()
    );
}

#[test]
fn scope_counts_keep_every_facet_except_stores() {
    const NOW: i64 = 1_000;
    let rows = facet_fixture();
    let base = filters(|f| {
        f.stores = vec!["steam".to_owned()];
        f.favorites_only = true;
        f.availability.installed = true;
    });
    let p = prepared(&rows, &LibraryScope::All, &base, "", NOW);
    let counts = p.scope_counts();
    let mut without_stores = base.clone();
    without_stores.stores.clear();
    assert_eq!(
        counts.all,
        filter(&rows, &LibraryScope::All, &without_stores, "", NOW).len()
    );
    assert_eq!(
        counts.installed,
        filter(&rows, &LibraryScope::Installed, &without_stores, "", NOW).len()
    );
    assert_eq!(
        counts.favorites,
        filter(&rows, &LibraryScope::Favorites, &without_stores, "", NOW).len()
    );
    assert_eq!(
        counts.recent,
        filter(&rows, &LibraryScope::Recent, &without_stores, "", NOW).len()
    );
    assert_eq!(
        counts.running,
        filter(&rows, &LibraryScope::Running, &without_stores, "", NOW).len()
    );
}

#[test]
fn hidden_rows_leave_every_count_until_show_hidden_is_on() {
    const NOW: i64 = 1_000;
    let mut rows = facet_fixture();
    rows[0].hidden = true;
    for row in rows.iter_mut().skip(1) {
        row.installed = false;
    }
    let hidden_key = rows[0].key.clone();
    let default = LibraryFilters::default();
    let p = prepared(&rows, &LibraryScope::All, &default, "", NOW);
    let counts = p.filter_counts(&providers(), true);
    assert!(
        !filter(&rows, &LibraryScope::All, &default, "", NOW)
            .iter()
            .any(|row| row.key == hidden_key)
    );
    assert_eq!(counts.show_hidden, 1);
    assert_eq!(counts.availability.installed, 0);
    assert_eq!(
        counts.stores.iter().map(|store| store.count).sum::<usize>(),
        4,
        "hidden rows are excluded from store counts too"
    );
    assert_eq!(p.scope_counts().installed, 0);
    assert_eq!(p.total(), 4);

    let show_hidden = filters(|f| f.show_hidden = true);
    let p = prepared(&rows, &LibraryScope::All, &show_hidden, "", NOW);
    assert!(
        filter(&rows, &LibraryScope::All, &show_hidden, "", NOW)
            .iter()
            .any(|row| row.key == hidden_key)
    );
    let counts = p.filter_counts(&providers(), true);
    assert_eq!(counts.availability.installed, 1);
    assert_eq!(counts.show_hidden, 1);
    assert_eq!(
        counts.stores.iter().map(|store| store.count).sum::<usize>(),
        5
    );
    assert_eq!(p.scope_counts().installed, 1);
    assert_eq!(p.total(), 5);
}

#[test]
fn the_status_facet_is_unavailable_until_the_account_library_is_read() {
    const NOW: i64 = 1_000;
    let rows = facet_fixture();
    let default_filters = LibraryFilters::default();
    let p = prepared(&rows, &LibraryScope::All, &default_filters, "", NOW);
    assert!(
        !p.filter_counts(&providers(), false)
            .gamenode_status
            .available
    );
    assert!(
        p.filter_counts(&providers(), true)
            .gamenode_status
            .available
    );
}

#[test]
fn collection_scope_and_counts_use_the_cached_account_library() {
    let mut rows = facet_fixture();
    rows[0].gamenode = Some(gamenode(Some(EntryStatus::Playing), &["backlog", "coop"]));
    rows[1].gamenode = Some(gamenode(None, &["backlog"]));
    assert_eq!(
        keys(&filter(
            &rows,
            &LibraryScope::Collection("backlog".to_owned()),
            &LibraryFilters::default(),
            "",
            0
        )),
        ["steam:1", "steam:2"]
    );

    let cache = LibraryCache {
        entries: vec![stored("steam", "1", "Alpha", true)],
        account_library: Some(AccountLibrary {
            entries: std::collections::BTreeMap::from([(
                1,
                AccountEntry {
                    id: "entry-1".to_owned(),
                    game_id: 1,
                    status: Some(EntryStatus::Playing),
                    collection_ids: vec!["backlog".to_owned(), "coop".to_owned()],
                },
            )]),
            collections: vec![
                Collection {
                    id: "backlog".to_owned(),
                    name: "Backlog".to_owned(),
                    default_status: None,
                },
                Collection {
                    id: "coop".to_owned(),
                    name: "Coop".to_owned(),
                    default_status: None,
                },
            ],
            read_at: 5,
        }),
        auto_matches: vec![AutoMatch {
            category: 1,
            uid: "1".to_owned(),
            game_ids: vec![1],
            checked_at: 1,
        }],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    finish_scan(&mut machine, vec![record("1", "Alpha", "/1")], 9);
    let counts = machine.collection_counts();
    assert_eq!(
        counts,
        [
            CollectionCount {
                id: "backlog".to_owned(),
                name: "Backlog".to_owned(),
                count: 1,
            },
            CollectionCount {
                id: "coop".to_owned(),
                name: "Coop".to_owned(),
                count: 1,
            },
        ]
    );
    assert_eq!(
        machine.view().unwrap().collections,
        counts,
        "the view carries the same collection counts the sidebar shows"
    );
    let row = &machine.visible_rows("")[0];
    assert_eq!(row.gamenode.as_ref().unwrap().entry_id, "entry-1");
    assert_eq!(
        row.gamenode.as_ref().unwrap().collection_ids,
        ["backlog", "coop"]
    );
}

#[test]
fn saved_view_counts_use_each_views_own_payload() {
    let cache = LibraryCache {
        entries: vec![
            stored("steam", "1", "Alpha", true),
            stored("steam", "2", "Beta", true),
        ],
        auto_matches: vec![
            AutoMatch {
                category: 1,
                uid: "1".to_owned(),
                game_ids: vec![1],
                checked_at: 1,
            },
            AutoMatch {
                category: 1,
                uid: "2".to_owned(),
                game_ids: vec![2],
                checked_at: 1,
            },
        ],
        games: vec![
            GameRecord {
                game_id: 1,
                name: "Alpha".to_owned(),
                release_year: Some(2021),
                cover_url: None,
                cover_file: None,
                cover_failed_at: None,
            },
            GameRecord {
                game_id: 2,
                name: "Beta".to_owned(),
                release_year: Some(2010),
                cover_url: None,
                cover_file: None,
                cover_failed_at: None,
            },
        ],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    finish_scan(
        &mut machine,
        vec![record("1", "Alpha", "/1"), record("2", "Beta", "/2")],
        9,
    );
    machine.saved_views = vec![
        SavedView {
            id: 1,
            name: "2020s".to_owned(),
            payload: SavedViewPayload {
                filters: filters(|f| f.release = ReleaseFilter::Decade(2020)),
                ..SavedViewPayload::default()
            },
            position: 0,
            created_at: 1,
        },
        SavedView {
            id: 2,
            name: "Empty".to_owned(),
            payload: SavedViewPayload {
                filters: filters(|f| f.stores = vec!["gone".to_owned()]),
                ..SavedViewPayload::default()
            },
            position: 1,
            created_at: 1,
        },
    ];
    let counts = machine.saved_view_counts();
    assert_eq!(counts[0].name, "2020s");
    assert_eq!(counts[0].count, 1);
    assert!(!counts[0].active);
    assert_eq!(counts[1].count, 0);
}

#[test]
fn group_orders_are_fixed_and_empty_groups_are_omitted() {
    let mut rows = vec![
        row("steam", "1", "A", MatchStatus::Unmatched),
        row("gog", "2", "B", MatchStatus::NoGame),
        row(
            "epic",
            "3",
            "C",
            MatchStatus::Ambiguous { candidates: vec![] },
        ),
        row("steam", "4", "D", matched(4)),
    ];
    rows[0].release_year = Some(2021);
    rows[1].release_year = Some(1985);
    rows[2].release_year = None;
    rows[3].release_year = Some(2011);
    rows[2].installed = false;
    rows[2].gamenode = Some(gamenode(Some(EntryStatus::Planned), &[]));
    rows[3].gamenode = Some(gamenode(Some(EntryStatus::Playing), &[]));
    rows[1].gamenode = Some(gamenode(None, &[]));
    rows[0].gamenode = Some(gamenode(Some(EntryStatus::Finished), &[]));
    let providers = providers();
    let empty = BTreeSet::new();

    let (grouped, groups) = group_rows(
        {
            let mut sorted = rows.clone();
            sort(&mut sorted, Sort::default());
            sorted
        },
        GroupBy::Store,
        &empty,
        &providers,
    );
    assert_eq!(
        groups
            .iter()
            .map(|group| group.id.as_str())
            .collect::<Vec<_>>(),
        ["steam", "gog", "epic"]
    );
    assert_eq!(
        grouped
            .iter()
            .map(|row| row.key.as_str())
            .collect::<Vec<_>>(),
        ["steam:1", "steam:4", "gog:2", "epic:3"]
    );

    let mut sorted = rows.clone();
    sort(&mut sorted, Sort::default());
    let (_, groups) = group_rows(sorted, GroupBy::MatchState, &empty, &providers);
    assert_eq!(
        groups
            .iter()
            .map(|group| group.id.as_str())
            .collect::<Vec<_>>(),
        ["matched", "ambiguous", "unmatched", "noGame"]
    );

    let mut sorted = rows.clone();
    sort(&mut sorted, Sort::default());
    let (_, groups) = group_rows(sorted, GroupBy::Installation, &empty, &providers);
    assert_eq!(
        groups
            .iter()
            .map(|group| group.id.as_str())
            .collect::<Vec<_>>(),
        ["installed", "notInstalled"]
    );

    let mut sorted = rows.clone();
    sort(&mut sorted, Sort::default());
    let (_, groups) = group_rows(sorted, GroupBy::GameNodeStatus, &empty, &providers);
    assert_eq!(
        groups
            .iter()
            .map(|group| group.id.as_str())
            .collect::<Vec<_>>(),
        ["playing", "finished", "planned", "inLibrary"]
    );

    let mut sorted = rows.clone();
    sort(&mut sorted, Sort::default());
    let (_, groups) = group_rows(sorted, GroupBy::ReleaseDecade, &empty, &providers);
    assert_eq!(
        groups
            .iter()
            .map(|group| group.id.as_str())
            .collect::<Vec<_>>(),
        ["2020", "2010", "earlier", "unknown"]
    );

    // Empty groups are omitted: no dropped or planned GameNode status remains once no row has it.
    let mut sorted = rows.clone();
    sort(&mut sorted, Sort::default());
    for row in &mut sorted {
        row.gamenode = Some(gamenode(Some(EntryStatus::Playing), &[]));
    }
    let (_, groups) = group_rows(sorted, GroupBy::GameNodeStatus, &empty, &providers);
    assert_eq!(
        groups
            .iter()
            .map(|group| group.id.as_str())
            .collect::<Vec<_>>(),
        ["playing"]
    );
}

#[test]
fn collapsed_groups_keep_headers_and_counts_but_lose_their_rows() {
    let rows = vec![
        row("steam", "1", "A", MatchStatus::Unmatched),
        row("steam", "2", "B", MatchStatus::Unmatched),
        row("gog", "3", "C", MatchStatus::Unmatched),
    ];
    let providers = providers();
    let collapsed = BTreeSet::from(["steam".to_owned()]);
    let (grouped, groups) = group_rows(rows, GroupBy::Store, &collapsed, &providers);
    assert_eq!(
        grouped
            .iter()
            .map(|row| row.key.as_str())
            .collect::<Vec<_>>(),
        ["gog:3"]
    );
    assert_eq!(groups[0].id, "steam");
    assert_eq!(groups[0].count, 2);
    assert!(groups[0].collapsed);
    assert_eq!(groups[1].first_index, 0);
}

// ---- through the machine ----

fn library_of(count: usize) -> LibraryMachine {
    let (mut machine, _) = loaded(false, LibraryCache::default());
    let records = (0..count)
        .map(|i| record(&format!("{i}"), &format!("Game {i}"), "/g"))
        .collect();
    finish_scan(&mut machine, records, 1);
    machine
}

#[test]
fn the_row_carries_hidden_and_cached_game_node_state() {
    let cache = LibraryCache {
        entries: vec![stored("steam", "1", "Alpha", true)],
        auto_matches: vec![AutoMatch {
            category: 1,
            uid: "1".to_owned(),
            game_ids: vec![7],
            checked_at: 1,
        }],
        hidden: vec!["steam:1".to_owned()],
        account_library: Some(AccountLibrary {
            entries: std::collections::BTreeMap::from([(
                7,
                AccountEntry {
                    id: "entry-7".to_owned(),
                    game_id: 7,
                    status: Some(EntryStatus::Finished),
                    collection_ids: vec!["coop".to_owned()],
                },
            )]),
            collections: vec![],
            read_at: 1,
        }),
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    finish_scan(&mut machine, vec![record("1", "Alpha", "/1")], 2);
    let rows = machine.all_rows();
    assert!(rows[0].hidden);
    let gamenode = rows[0].gamenode.as_ref().unwrap();
    assert_eq!(gamenode.status, Some(EntryStatus::Finished));
    assert_eq!(gamenode.entry_id, "entry-7");
    assert_eq!(gamenode.collection_ids, ["coop"]);
    assert!(machine.visible_rows("").is_empty());
    machine.layout.filters.show_hidden = true;
    let view = machine.view().unwrap();
    assert_eq!(view.rows.len(), 1);
    assert_eq!(view.filter_counts.show_hidden, 1);
    assert!(view.filter_counts.gamenode_status.available);
    assert_eq!(view.filter_counts.gamenode_status.finished, 1);
}

#[test]
fn group_inputs_persist_the_layout_and_remember_each_grouping() {
    let mut machine = library_of(6);
    let effects = machine.handle(Input::SetGroup(GroupBy::Store));
    let [Effect::PersistLayout(layout)] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(layout.group, GroupBy::Store);

    let effects = machine.handle(Input::ToggleGroup {
        id: "steam".to_owned(),
    });
    let [Effect::PersistLayout(layout)] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert!(layout.collapsed_groups[&GroupBy::Store].contains("steam"));
    assert!(machine.visible_rows("").len() < 6);

    // Another grouping starts expanded and keeps its own collapsed set.
    machine.handle(Input::SetGroup(GroupBy::Installation));
    assert_eq!(machine.visible_rows("").len(), 6);
    let effects = machine.handle(Input::SetAllGroupsCollapsed(true));
    let [Effect::PersistLayout(layout)] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert!(layout.collapsed_groups[&GroupBy::Installation].contains("installed"));
    assert!(machine.visible_rows("").is_empty());

    machine.handle(Input::SetAllGroupsCollapsed(false));
    assert_eq!(machine.visible_rows("").len(), 6);
    machine.handle(Input::SetGroup(GroupBy::Store));
    // The store grouping still remembers its own collapsed Steam group.
    assert!(machine.layout.collapsed_groups[&GroupBy::Store].contains("steam"));
}

#[test]
fn collapsing_a_group_removes_its_selected_rows_from_the_selection() {
    let mut machine = library_of(4);
    machine.handle(Input::SetGroup(GroupBy::Installation));
    machine.handle(Input::Select {
        key: "steam:1".to_owned(),
        mode: crate::library::SelectMode::Replace,
    });
    machine.handle(Input::Select {
        key: "steam:2".to_owned(),
        mode: crate::library::SelectMode::Toggle,
    });
    assert_eq!(machine.selected_keys(), ["steam:1", "steam:2"]);
    machine.handle(Input::ToggleGroup {
        id: "installed".to_owned(),
    });
    assert!(machine.selected_keys().is_empty());
    machine.handle(Input::ToggleGroup {
        id: "installed".to_owned(),
    });
    assert!(
        machine.selected_keys().is_empty(),
        "expanding never brings pruned rows back"
    );
}

#[test]
fn select_all_uses_expanded_rows_only_and_clear_empties_the_selection() {
    let mut machine = library_of(6);
    machine.handle(Input::SetGroup(GroupBy::Installation));
    machine.handle(Input::SetAllGroupsCollapsed(true));
    machine.handle(Input::SelectAll);
    assert!(machine.selected_keys().is_empty());

    machine.handle(Input::SetAllGroupsCollapsed(false));
    machine.handle(Input::SelectAll);
    assert_eq!(machine.selected_keys().len(), 6);
    machine.handle(Input::ClearSelection);
    assert!(machine.selected_keys().is_empty());
    assert!(machine.focused_key().is_none());
}

#[test]
fn select_all_skips_collapsed_and_hidden_rows() {
    let mut machine = library_of(6);
    machine.layout.filters.show_hidden = false;
    machine.hidden.insert("steam:3".to_owned());
    machine.handle(Input::SetGroup(GroupBy::Installation));
    machine.handle(Input::SelectAll);
    let selected = machine.selected_keys();
    assert_eq!(selected.len(), 5);
    assert!(!selected.contains(&"steam:3".to_owned()));

    // Every fixture row is installed, so collapsing the group removes all visible rows and the
    // selected rows are pruned rather than kept behind the collapsed header.
    machine.handle(Input::ToggleGroup {
        id: "installed".to_owned(),
    });
    assert!(machine.visible_rows("").is_empty());
    machine.handle(Input::SelectAll);
    assert!(machine.selected_keys().is_empty());

    machine.handle(Input::ToggleGroup {
        id: "installed".to_owned(),
    });
    machine.handle(Input::SelectAll);
    assert_eq!(machine.selected_keys().len(), 5, "the hidden row stays out");
    assert!(
        !machine
            .visible_rows("")
            .iter()
            .any(|row| row.key == "steam:3")
    );
}

#[test]
fn scope_recent_and_collection_go_through_the_machine() {
    let cache = LibraryCache {
        entries: vec![
            stored("steam", "1", "Alpha", true),
            stored("steam", "2", "Bravo", true),
        ],
        account_library: Some(AccountLibrary {
            entries: std::collections::BTreeMap::from([(
                2,
                AccountEntry {
                    id: "entry-2".to_owned(),
                    game_id: 2,
                    status: Some(EntryStatus::Playing),
                    collection_ids: vec!["coop".to_owned()],
                },
            )]),
            collections: vec![],
            read_at: 1,
        }),
        auto_matches: vec![AutoMatch {
            category: 1,
            uid: "2".to_owned(),
            game_ids: vec![2],
            checked_at: 1,
        }],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    finish_scan(
        &mut machine,
        vec![record("1", "Alpha", "/1"), record("2", "Bravo", "/2")],
        RECENT_DAYS * DAY_SECONDS + 1,
    );
    machine.now = RECENT_DAYS * DAY_SECONDS + 1;
    machine.set_playtime(std::collections::BTreeMap::from([(
        "steam:1".to_owned(),
        playtime(Some(60), Some(1)),
    )]));
    machine.handle(Input::SetScope(LibraryScope::Recent));
    assert_eq!(
        machine
            .visible_rows("")
            .iter()
            .map(|row| row.key.as_str())
            .collect::<Vec<_>>(),
        ["steam:1"]
    );
    machine.handle(Input::SetScope(LibraryScope::Collection("coop".to_owned())));
    assert_eq!(
        machine
            .visible_rows("")
            .iter()
            .map(|row| row.key.as_str())
            .collect::<Vec<_>>(),
        ["steam:2"]
    );
    assert_eq!(
        machine.query("").title_source.unwrap(),
        TitleSource {
            kind: TitleSourceKind::Collection,
            id: "coop".to_owned(),
        }
    );
    machine.handle(Input::SetScope(LibraryScope::SavedView(7)));
    assert_eq!(
        machine.query("").title_source.unwrap(),
        TitleSource {
            kind: TitleSourceKind::SavedView,
            id: "7".to_owned(),
        }
    );
}

#[test]
fn provider_counts_follow_every_facet_except_the_sidebar_and_stay_raw_for_facts() {
    let cache = LibraryCache {
        entries: vec![
            stored("steam", "1", "Favorite", true),
            stored_with("steam", "2", "Owned", false, true, false),
        ],
        games: vec![GameRecord {
            game_id: 7,
            name: "Favorite".to_owned(),
            release_year: Some(2011),
            cover_url: None,
            cover_file: None,
            cover_failed_at: None,
        }],
        favorites: vec![crate::library::store::FavoriteRecord {
            account_id: "u1".to_owned(),
            game_id: 7,
            is_favorite: true,
            pending: false,
            updated_at: 1,
        }],
        auto_matches: vec![AutoMatch {
            category: 1,
            uid: "1".to_owned(),
            game_ids: vec![7],
            checked_at: 1,
        }],
        ..LibraryCache::default()
    };
    let mut machine = loaded(false, cache).0;
    finish_scan(&mut machine, vec![record("1", "Favorite", "/1")], 9);
    machine.handle(Input::SetFilters(LibraryFilters {
        favorites_only: true,
        ..LibraryFilters::default()
    }));

    let steam = machine
        .provider_counts()
        .into_iter()
        .find(|count| count.id == "steam")
        .unwrap();
    assert_eq!(steam.count, 1, "owned-only rows are hidden by the filter");
    assert_eq!(
        steam.installed_count, 1,
        "the subline fact stays raw, not filtered"
    );
    assert_eq!(steam.owned_count, 1);
    // Selecting a store does not change its count.
    machine.handle(Input::SetFilter(
        crate::library::SidebarSelection::Provider("steam".to_owned()),
    ));
    let steam = machine
        .provider_counts()
        .into_iter()
        .find(|count| count.id == "steam")
        .unwrap();
    assert_eq!(steam.count, 1);
}

fn override_cache(display_overrides: Vec<crate::library::DisplayOverride>) -> LibraryCache {
    LibraryCache {
        entries: vec![
            stored("steam", "620", "portal 2", true),
            stored("steam", "70", "Half-Life", true),
        ],
        auto_matches: vec![AutoMatch {
            category: 1,
            uid: "620".to_owned(),
            game_ids: vec![7],
            checked_at: 1,
        }],
        games: vec![GameRecord {
            game_id: 7,
            name: "Portal 2".to_owned(),
            release_year: None,
            cover_url: None,
            cover_file: None,
            cover_failed_at: None,
        }],
        display_overrides,
        ..LibraryCache::default()
    }
}

fn titles(machine: &mut LibraryMachine) -> BTreeMap<String, String> {
    machine
        .all_rows()
        .into_iter()
        .map(|row| (row.key, row.title))
        .collect()
}

fn scan_both(machine: &mut LibraryMachine) {
    finish_scan(
        machine,
        vec![
            record("620", "portal 2", "/p"),
            record("70", "Half-Life", "/h"),
        ],
        9,
    );
}

fn stored_override(machine: &mut LibraryMachine, key: &str, name: Option<&str>, revision: u64) {
    machine.handle(Input::DisplayNameStored {
        key: key.to_owned(),
        display_name: name.map(str::to_owned),
        revision,
    });
}

#[test]
fn a_plugin_display_name_wins_over_the_matched_and_detected_titles_until_cleared() {
    let (mut machine, _) = loaded(false, override_cache(vec![]));
    scan_both(&mut machine);
    assert_eq!(
        titles(&mut machine)["steam:620"],
        "Portal 2",
        "matched name"
    );
    assert_eq!(
        titles(&mut machine)["steam:70"],
        "Half-Life",
        "detected name"
    );

    stored_override(&mut machine, "steam:620", Some("Portal Two"), 1);
    stored_override(&mut machine, "steam:70", Some("HL"), 1);
    let rows = machine.all_rows();
    let portal = rows.iter().find(|row| row.key == "steam:620").unwrap();
    // The facts stay separate from the override.
    assert_eq!(portal.title, "Portal Two");
    assert_eq!(portal.matched_name.as_deref(), Some("Portal 2"));
    assert_eq!(portal.detected_name, "portal 2");
    assert_eq!(titles(&mut machine)["steam:70"], "HL");

    // Clearing restores each entry's previous title and the revision keeps counting.
    stored_override(&mut machine, "steam:620", None, 2);
    stored_override(&mut machine, "steam:70", None, 2);
    assert_eq!(titles(&mut machine)["steam:620"], "Portal 2");
    assert_eq!(titles(&mut machine)["steam:70"], "Half-Life");
    let cleared = machine.display_override("steam:620").unwrap();
    assert_eq!(
        (cleared.display_name.as_deref(), cleared.revision),
        (None, 2)
    );
}

#[test]
fn a_display_name_for_an_unknown_entry_is_ignored() {
    let (mut machine, _) = loaded(false, override_cache(vec![]));
    scan_both(&mut machine);
    stored_override(&mut machine, "steam:nope", Some("Ghost"), 1);
    assert!(machine.display_override("steam:nope").is_none());
    assert_eq!(titles(&mut machine).len(), 2);
}

#[test]
fn stored_display_names_load_with_their_revision() {
    let (mut machine, _) = loaded(
        false,
        override_cache(vec![
            crate::library::DisplayOverride {
                entry_key: "steam:620".to_owned(),
                display_name: Some("Portal Two".to_owned()),
                revision: 4,
            },
            // Cleared earlier: no name, but its revision is kept.
            crate::library::DisplayOverride {
                entry_key: "steam:70".to_owned(),
                display_name: None,
                revision: 2,
            },
        ]),
    );
    scan_both(&mut machine);
    assert_eq!(titles(&mut machine)["steam:620"], "Portal Two");
    assert_eq!(titles(&mut machine)["steam:70"], "Half-Life");
    assert_eq!(machine.display_override("steam:620").unwrap().revision, 4);
    assert_eq!(machine.display_override("steam:70").unwrap().revision, 2);
}

fn covers_cache(artwork_overrides: Vec<crate::library::ArtworkOverride>) -> LibraryCache {
    let mut cache = override_cache(vec![]);
    cache.games[0].cover_file = Some("7.jpg".to_owned());
    cache.artwork_overrides = artwork_overrides;
    cache
}

fn covers(machine: &mut LibraryMachine) -> BTreeMap<String, Option<String>> {
    machine
        .all_rows()
        .into_iter()
        .map(|row| (row.key, row.cover_file))
        .collect()
}

fn stored_cover(machine: &mut LibraryMachine, key: &str, file: Option<&str>, revision: u64) {
    machine.handle(Input::ArtworkStored {
        key: key.to_owned(),
        file: file.map(str::to_owned),
        revision,
    });
}

#[test]
fn a_plugin_cover_wins_over_the_matched_games_cover_until_cleared() {
    let (mut machine, _) = loaded(false, covers_cache(vec![]));
    scan_both(&mut machine);
    assert_eq!(covers(&mut machine)["steam:620"].as_deref(), Some("7.jpg"));
    assert_eq!(covers(&mut machine)["steam:70"], None, "no matched game");

    // The override replaces the game's cover for this entry, and also gives an unmatched entry
    // a cover; the game record is untouched.
    stored_cover(&mut machine, "steam:620", Some("plugin-a-b.jpg"), 1);
    stored_cover(&mut machine, "steam:70", Some("plugin-c-d.png"), 1);
    assert_eq!(
        covers(&mut machine)["steam:620"].as_deref(),
        Some("plugin-a-b.jpg")
    );
    assert_eq!(
        covers(&mut machine)["steam:70"].as_deref(),
        Some("plugin-c-d.png")
    );
    assert_eq!(
        machine.game(7).unwrap().cover_file.as_deref(),
        Some("7.jpg")
    );
    assert_eq!(
        machine.entry_cover_file("steam:620").as_deref(),
        Some("plugin-a-b.jpg")
    );

    // Clearing restores each entry's own cover and the revision keeps counting.
    stored_cover(&mut machine, "steam:620", None, 2);
    stored_cover(&mut machine, "steam:70", None, 2);
    assert_eq!(covers(&mut machine)["steam:620"].as_deref(), Some("7.jpg"));
    assert_eq!(covers(&mut machine)["steam:70"], None);
    assert_eq!(
        machine.entry_cover_file("steam:620").as_deref(),
        Some("7.jpg")
    );
    assert_eq!(machine.artwork_override("steam:620").unwrap().revision, 2);
}

#[test]
fn a_cover_for_an_unknown_entry_is_ignored() {
    let (mut machine, _) = loaded(false, covers_cache(vec![]));
    scan_both(&mut machine);
    stored_cover(&mut machine, "steam:nope", Some("plugin-x-y.jpg"), 1);
    assert!(machine.artwork_override("steam:nope").is_none());
    assert_eq!(machine.entry_cover_file("steam:nope"), None);
}

#[test]
fn stored_covers_load_with_their_revision() {
    let (mut machine, _) = loaded(
        false,
        covers_cache(vec![
            crate::library::ArtworkOverride {
                entry_key: "steam:620".to_owned(),
                file: Some("plugin-a-b.jpg".to_owned()),
                revision: 4,
            },
            // Cleared earlier: no file, but its revision is kept.
            crate::library::ArtworkOverride {
                entry_key: "steam:70".to_owned(),
                file: None,
                revision: 2,
            },
        ]),
    );
    scan_both(&mut machine);
    assert_eq!(
        covers(&mut machine)["steam:620"].as_deref(),
        Some("plugin-a-b.jpg")
    );
    assert_eq!(covers(&mut machine)["steam:70"], None);
    assert_eq!(machine.artwork_override("steam:620").unwrap().revision, 4);
    assert_eq!(machine.artwork_override("steam:70").unwrap().revision, 2);
}
