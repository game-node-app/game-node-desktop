use super::*;
use crate::library::machine::Input;
use crate::library::machine::tests::{finish_scan, loaded, record, stored, stored_with};
use crate::library::matching::MatchSource;
use crate::library::store::{AutoMatch, ChosenMatch, GameRecord, LibraryCache};

fn row(provider: &str, id: &str, title: &str, status: MatchStatus) -> Row {
    Row {
        playtime: Default::default(),
        running: false,
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

#[test]
fn filters_by_provider_and_searches_case_insensitively() {
    let rows = sample();
    let steam = SidebarSelection::Provider("steam".to_owned());

    assert_eq!(
        keys(&filter(
            &rows,
            &SidebarSelection::All,
            &LibraryFilters::default(),
            "",
            0
        )),
        keys(&rows)
    );
    assert_eq!(
        keys(&filter(&rows, &steam, &LibraryFilters::default(), "", 0)),
        ["steam:3", "steam:2"]
    );
    assert_eq!(
        keys(&filter(
            &rows,
            &SidebarSelection::All,
            &LibraryFilters::default(),
            "PORT",
            0
        )),
        ["steam:3", "shop:1"]
    );
    // Provider filter and search combine.
    assert_eq!(
        keys(&filter(
            &rows,
            &steam,
            &LibraryFilters::default(),
            " port ",
            0
        )),
        ["steam:3"]
    );
    // Blank search (only spaces) keeps everything.
    assert_eq!(
        filter(
            &rows,
            &SidebarSelection::All,
            &LibraryFilters::default(),
            "   ",
            0
        )
        .len(),
        4
    );
}

#[test]
fn search_covers_the_matched_title_as_well_as_the_detected_one() {
    let mut rows = sample();
    rows[0].matched_name = Some("Portal 2: Aperture".to_owned());
    rows[0].title = "Portal 2: Aperture".to_owned();
    let all = SidebarSelection::All;

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
            &SidebarSelection::All,
            &LibraryFilters::default(),
            "zzz",
            0
        )
        .is_empty()
    );
    assert!(
        filter(
            &rows,
            &SidebarSelection::Provider("nowhere".to_owned()),
            &LibraryFilters::default(),
            "",
            0
        )
        .is_empty()
    );
    assert!(
        filter(
            &[],
            &SidebarSelection::All,
            &LibraryFilters::default(),
            "",
            0
        )
        .is_empty()
    );
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
        // "Same" and "same" differ only by case; the case-sensitive tie-break puts "Same" first
        // when ascending and last when descending, and the rest follows provider and key.
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
    // The provider order flips; titles inside a provider stay ascending.
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
    // Matched, ambiguous, unmatched, no game.
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
    // The UI's `ProviderCount` has the visible and per-fact counts; a sidebar button without
    // `id` cannot send its filter, so the wire shape is pinned here.
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
fn a_saved_filter_for_a_provider_that_is_gone_falls_back_to_all() {
    let (mut machine, _) = loaded(false, LibraryCache::default());
    finish_scan(&mut machine, vec![record("1", "One", "/1")], 9);
    machine.layout.sidebar = SidebarSelection::Provider("removed".to_owned());
    assert_eq!(machine.effective_sidebar(), SidebarSelection::All);
    assert_eq!(machine.visible_rows("").len(), 1);
}

#[test]
fn the_installed_filter_composes_with_the_provider_and_search() {
    let mut rows = sample();
    rows[0].installed = true;
    rows[1].installed = false;
    rows[2].installed = false;
    rows[3].installed = true;
    let installed = installed_filter();

    assert_eq!(
        keys(&filter(&rows, &SidebarSelection::All, &installed, "", 0)),
        ["steam:3", "shop:4"]
    );
    assert_eq!(
        keys(&filter(
            &rows,
            &SidebarSelection::Provider("steam".to_owned()),
            &installed,
            "",
            0
        )),
        ["steam:3"]
    );
    assert_eq!(
        keys(&filter(
            &rows,
            &SidebarSelection::Provider("shop".to_owned()),
            &installed,
            "zork",
            0
        )),
        ["shop:4"]
    );
    assert!(
        filter(
            &rows,
            &SidebarSelection::Provider("steam".to_owned()),
            &installed,
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

    let all = SidebarSelection::All;
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
    let all = SidebarSelection::All;

    let running = filters(|f| f.running_only = true);
    assert_eq!(keys(&filter(&rows, &all, &running, "", 0)), ["steam:1"]);
    for (state, expected) in [
        (MatchFilter::Matched, "steam:1"),
        (MatchFilter::Ambiguous, "steam:2"),
        (MatchFilter::Unmatched, "steam:3"),
        (MatchFilter::NoGame, "steam:4"),
    ] {
        let by_state = filters(|f| f.match_state = state);
        assert_eq!(keys(&filter(&rows, &all, &by_state, "", 0)), [expected]);
    }
    let favorites = filters(|f| f.favorites_only = true);
    assert_eq!(keys(&filter(&rows, &all, &favorites, "", 0)), ["steam:1"]);
    let everything = filters(|f| {
        f.running_only = true;
        f.match_state = MatchFilter::Matched;
        f.favorites_only = true;
    });
    assert_eq!(keys(&filter(&rows, &all, &everything, "", 0)), ["steam:1"]);
    rows[0].running = false;
    assert!(filter(&rows, &all, &everything, "", 0).is_empty());
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
    let all = SidebarSelection::All;

    let unplayed = filters(|f| f.playtime = PlaytimeFilter::Unplayed);
    assert_eq!(
        keys(&filter(&rows, &all, &unplayed, "", 0)),
        ["steam:2"],
        "unknown playtime is not presented as unplayed"
    );
    let played = filters(|f| f.playtime = PlaytimeFilter::Played);
    assert_eq!(
        keys(&filter(&rows, &all, &played, "", 0)),
        ["steam:3", "steam:4"]
    );
    let ten = filters(|f| f.playtime = PlaytimeFilter::AtLeast10h);
    assert_eq!(
        keys(&filter(&rows, &all, &ten, "", 0)),
        ["steam:3", "steam:4"]
    );
    let hundred = filters(|f| f.playtime = PlaytimeFilter::AtLeast100h);
    assert_eq!(keys(&filter(&rows, &all, &hundred, "", 0)), ["steam:4"]);
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
    let all = SidebarSelection::All;

    let week = filters(|f| f.last_played = LastPlayedFilter::Last7Days);
    assert_eq!(keys(&filter(&rows, &all, &week, "", NOW)), ["steam:1"]);
    let month = filters(|f| f.last_played = LastPlayedFilter::Last30Days);
    assert_eq!(keys(&filter(&rows, &all, &month, "", NOW)), ["steam:1"]);
    let year = filters(|f| f.last_played = LastPlayedFilter::LastYear);
    assert_eq!(
        keys(&filter(&rows, &all, &year, "", NOW)),
        ["steam:1", "steam:2"],
        "the 200-day-old row is inside the year"
    );
    let added_week = filters(|f| f.added = AddedFilter::Last7Days);
    assert_eq!(
        keys(&filter(&rows, &all, &added_week, "", NOW)),
        ["steam:1", "steam:3"]
    );
    let added_month = filters(|f| f.added = AddedFilter::Last30Days);
    assert_eq!(
        keys(&filter(&rows, &all, &added_month, "", NOW)),
        ["steam:1", "steam:3"]
    );
    let added_old = filters(|f| f.added = AddedFilter::LastYear);
    assert_eq!(
        keys(&filter(&rows, &all, &added_old, "", NOW)),
        ["steam:1", "steam:2", "steam:3"],
        "the 200-day-old row is inside the year"
    );
    let added_ninety = filters(|f| f.added = AddedFilter::Last90Days);
    assert_eq!(
        keys(&filter(&rows, &all, &added_ninety, "", NOW)),
        ["steam:1", "steam:3"]
    );
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
    let all = SidebarSelection::All;

    let twenties = filters(|f| f.release = ReleaseFilter::Decade(2020));
    assert_eq!(keys(&filter(&rows, &all, &twenties, "", 0)), ["steam:1"]);
    let nineties = filters(|f| f.release = ReleaseFilter::Decade(1990));
    assert_eq!(keys(&filter(&rows, &all, &nineties, "", 0)), ["steam:2"]);
    let earlier = filters(|f| f.release = ReleaseFilter::Earlier);
    assert_eq!(keys(&filter(&rows, &all, &earlier, "", 0)), ["steam:4"]);
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
        f.release = ReleaseFilter::Decade(2010);
    });
    assert_eq!(
        keys(&filter(&rows, &SidebarSelection::All, &combined, "", 0)),
        ["steam:1"]
    );
    assert_eq!(
        keys(&filter(
            &rows,
            &SidebarSelection::Provider("steam".to_owned()),
            &combined,
            "two",
            0
        )),
        ["steam:1"]
    );
    assert!(filter(&rows, &SidebarSelection::All, &combined, "stories", 0).is_empty());
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
    // Selecting the provider itself must not change its count.
    machine.handle(Input::SetFilter(SidebarSelection::Provider(
        "steam".to_owned(),
    )));
    let steam = machine
        .provider_counts()
        .into_iter()
        .find(|count| count.id == "steam")
        .unwrap();
    assert_eq!(steam.count, 1);
}
