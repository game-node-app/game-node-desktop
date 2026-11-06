use super::*;
use crate::library::machine::tests::{finish_scan, loaded, record, stored};
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
        status,
        cover_file: None,
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
        keys(&filter(&rows, &SidebarSelection::All, "")),
        keys(&rows)
    );
    assert_eq!(keys(&filter(&rows, &steam, "")), ["steam:3", "steam:2"]);
    assert_eq!(
        keys(&filter(&rows, &SidebarSelection::All, "PORT")),
        ["steam:3", "shop:1"]
    );
    // Provider filter and search combine.
    assert_eq!(keys(&filter(&rows, &steam, " port ")), ["steam:3"]);
    // Blank search (only spaces) keeps everything.
    assert_eq!(filter(&rows, &SidebarSelection::All, "   ").len(), 4);
}

#[test]
fn search_covers_the_matched_title_as_well_as_the_detected_one() {
    let mut rows = sample();
    rows[0].matched_name = Some("Portal 2: Aperture".to_owned());
    rows[0].title = "Portal 2: Aperture".to_owned();
    let all = SidebarSelection::All;

    assert_eq!(keys(&filter(&rows, &all, "aperture")), ["steam:3"]);
    // The detected title still finds it.
    assert_eq!(keys(&filter(&rows, &all, "portal 2")), ["steam:3"]);
}

#[test]
fn nothing_matches_gives_an_empty_list_not_an_error() {
    let rows = sample();
    assert!(filter(&rows, &SidebarSelection::All, "zzz").is_empty());
    assert!(filter(&rows, &SidebarSelection::Provider("nowhere".to_owned()), "").is_empty());
    assert!(filter(&[], &SidebarSelection::All, "").is_empty());
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
fn counts_installed_games_per_provider_and_keeps_empty_ones() {
    let rows = sample();
    let counted = counts(
        &rows,
        [("steam", "Steam"), ("shop", "Shop"), ("gog", "GOG")],
    );
    let pairs: Vec<_> = counted.iter().map(|c| (c.id.as_str(), c.count)).collect();
    assert_eq!(pairs, [("steam", 2), ("shop", 2), ("gog", 0)]);
    assert_eq!(counted[0].name, "Steam");
}

#[test]
fn provider_counts_reach_the_frontend_with_the_keys_it_reads() {
    // The UI's `ProviderCount` is `{ id, name, count }`; a sidebar button without `id` cannot
    // send its filter, so the wire shape is pinned here.
    let counted = counts(&sample(), [("steam", "Steam")]);
    let json = serde_json::to_value(&counted[0]).unwrap();
    assert_eq!(
        json,
        serde_json::json!({ "id": "steam", "name": "Steam", "count": 2 })
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
