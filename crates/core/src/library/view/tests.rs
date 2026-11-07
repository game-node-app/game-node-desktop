use super::*;
use crate::library::layout::{
    AvailabilityFilter, LibraryFilters, SidebarSelection, Sort, SortDirection, SortKey, ViewMode,
};
use crate::library::machine::tests::{finish_scan, loaded, record, stored, stored_with};
use crate::library::machine::{Effect, Input};
use crate::library::selection::{Direction, SelectMode};
use crate::library::store::{AutoMatch, GameRecord, LibraryCache};

fn installed_filter() -> LibraryFilters {
    LibraryFilters {
        availability: AvailabilityFilter {
            installed: true,
            ..AvailabilityFilter::default()
        },
        ..LibraryFilters::default()
    }
}

fn library() -> LibraryMachine {
    let cache = LibraryCache {
        entries: vec![stored("steam", "620", "portal 2", true)],
        auto_matches: vec![AutoMatch {
            category: 1,
            uid: "620".to_owned(),
            game_ids: vec![7],
            checked_at: 1,
        }],
        games: vec![GameRecord {
            game_id: 7,
            name: "Portal 2".to_owned(),
            release_year: Some(2011),
            cover_url: None,
            cover_file: None,
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
            record("240", "Counter-Strike", "/c"),
        ],
        5,
    );
    machine
}

fn titles(view: &LibraryView) -> Vec<&str> {
    view.rows.iter().map(|row| row.title.as_str()).collect()
}

#[test]
fn there_is_no_view_without_an_account() {
    let mut machine = LibraryMachine::new(
        crate::library::machine::tests::providers(false),
        Default::default(),
    );
    assert_eq!(machine.view(), None);
}

#[test]
fn the_view_reflects_filters_search_counts_and_the_selection() {
    let mut machine = library();
    let view = machine.view().unwrap();
    assert_eq!(titles(&view), ["Counter-Strike", "Half-Life", "Portal 2"]);
    assert_eq!(view.total, 3);
    assert_eq!(view.providers[0].count, 3);
    assert!(!view.scanning && view.empty.is_none());

    machine.handle(Input::SetSearch("life".to_owned()));
    let view = machine.view().unwrap();
    assert_eq!(titles(&view), ["Half-Life"]);
    assert_eq!(view.search, "life");
    assert_eq!(view.total, 3, "the count does not follow the search");

    machine.handle(Input::Select {
        key: "steam:70".to_owned(),
        mode: SelectMode::Replace,
    });
    let view = machine.view().unwrap();
    assert_eq!(view.selected, ["steam:70"]);
    assert_eq!(view.focused.as_deref(), Some("steam:70"));
    assert!(view.actions.play.is_available());
    assert_eq!(view.detail.as_ref().unwrap().install_dir, "/h");
}

#[test]
fn the_detail_shows_the_matched_game_and_where_the_match_came_from() {
    let mut machine = library();
    machine.handle(Input::Select {
        key: "steam:620".to_owned(),
        mode: SelectMode::Replace,
    });
    let detail = machine.view().unwrap().detail.unwrap();
    assert_eq!(detail.row.title, "Portal 2");
    assert_eq!(detail.row.detected_name, "portal 2");
    assert_eq!(detail.release_year, Some(2011));
    assert!(matches!(
        detail.row.status,
        MatchStatus::Matched {
            game_id: 7,
            source: crate::library::matching::MatchSource::Automatic
        }
    ));

    // No detail for several selected games.
    machine.handle(Input::Select {
        key: "steam:70".to_owned(),
        mode: SelectMode::Toggle,
    });
    assert_eq!(machine.view().unwrap().detail, None);
}

#[test]
fn unchanged_state_keeps_the_generation_and_the_rows() {
    let mut machine = library();
    let first = machine.view().unwrap();
    let again = machine.view().unwrap();
    assert_eq!(first, again);
    assert_eq!(first.generation, again.generation);
    assert!(Arc::ptr_eq(&first.rows, &again.rows));

    // Inputs that change nothing the view shows do not bump it either.
    machine.handle(Input::SetColumns(1));
    machine.handle(Input::SetView(ViewMode::Grid));
    machine.handle(Input::SetSearch(String::new()));
    machine.handle(Input::Select {
        key: "nothing".to_owned(),
        mode: SelectMode::Replace,
    });
    assert_eq!(machine.view().unwrap().generation, first.generation);
}

#[test]
fn a_change_bumps_the_generation_and_only_changed_rows_get_new_arcs() {
    let mut machine = library();
    let first = machine.view().unwrap();

    machine.handle(Input::Select {
        key: "steam:70".to_owned(),
        mode: SelectMode::Replace,
    });
    let selected = machine.view().unwrap();
    assert!(selected.generation > first.generation);
    // Selection is not part of a row, so every row is shared with the earlier view.
    assert!(Arc::ptr_eq(&first.rows, &selected.rows));

    // A rename of one game changes just that row.
    machine.entries.get_mut("steam:240").unwrap().name = Some("Counter-Strike 1.6".to_owned());
    let renamed = machine.view().unwrap();
    assert!(renamed.generation > selected.generation);
    for (before, after) in selected.rows.iter().zip(renamed.rows.iter()) {
        assert_eq!(
            Arc::ptr_eq(before, after),
            before.key != "steam:240",
            "{}",
            before.key
        );
    }
}

#[test]
fn layout_commands_change_the_view_and_ask_for_the_layout_to_be_saved() {
    let mut machine = library();
    let first = machine.view().unwrap();

    let effects = machine.handle(Input::SetView(ViewMode::List));
    let [Effect::PersistLayout(layout)] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(layout.view, ViewMode::List);

    machine.handle(Input::SetSort(Sort {
        key: SortKey::Title,
        direction: SortDirection::Descending,
    }));
    machine.handle(Input::SetFilter(SidebarSelection::Provider(
        "steam".to_owned(),
    )));
    machine.handle(Input::SetDetailOpen(false));
    let view = machine.view().unwrap();
    assert!(view.generation > first.generation);
    assert_eq!(view.layout.view, ViewMode::List);
    assert_eq!(titles(&view), ["Portal 2", "Half-Life", "Counter-Strike"]);
    assert!(!view.layout.detail_open);

    // Setting what is already set saves nothing.
    assert!(machine.handle(Input::SetView(ViewMode::List)).is_empty());
}

#[test]
fn pane_widths_are_kept_within_a_usable_range() {
    let mut machine = library();
    machine.handle(Input::SetSidebarWidth(5));
    machine.handle(Input::SetDetailWidth(5000));
    let layout = machine.view().unwrap().layout;
    assert_eq!(
        layout.sidebar_width,
        *crate::library::layout::SIDEBAR_WIDTH_RANGE.start()
    );
    assert_eq!(
        layout.detail_width,
        *crate::library::layout::DETAIL_WIDTH_RANGE.end()
    );
}

#[test]
fn search_text_is_not_part_of_the_persisted_layout() {
    let mut machine = library();
    // Typing never asks for the layout to be saved...
    assert!(
        machine
            .handle(Input::SetSearch("portal".to_owned()))
            .is_empty()
    );
    // ...and a layout saved afterwards has no trace of it.
    let effects = machine.handle(Input::SetView(ViewMode::List));
    let [Effect::PersistLayout(layout)] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    let saved = serde_json::to_string(layout).unwrap();
    assert!(
        !saved.contains("portal") && !saved.contains("search"),
        "{saved}"
    );
    // A machine started from that layout begins with an empty search.
    let mut restarted = LibraryMachine::new(
        crate::library::machine::tests::providers(false),
        layout.clone(),
    );
    restarted.handle(Input::AccountChanged(Some(
        crate::library::machine::tests::account("a"),
    )));
    assert_eq!(restarted.view().unwrap().search, "");
}

#[test]
fn a_saved_provider_filter_that_no_longer_exists_shows_all_games() {
    let mut machine = library();
    machine.handle(Input::SetFilter(SidebarSelection::Provider(
        "gone".to_owned(),
    )));
    let view = machine.view().unwrap();
    assert_eq!(view.layout.sidebar, SidebarSelection::All);
    assert_eq!(view.rows.len(), 3);
}

#[test]
fn empty_states_tell_apart_no_games_from_nothing_matching() {
    // A scan that found nothing.
    let (mut machine, _) = loaded(false, LibraryCache::default());
    assert_eq!(machine.view().unwrap().empty, None, "still scanning");
    finish_scan(&mut machine, vec![], 3);
    let view = machine.view().unwrap();
    assert_eq!(
        view.empty,
        Some(EmptyState::NoGames {
            stores: vec!["Steam".to_owned()]
        })
    );

    // Games exist but the search hides them all.
    let mut machine = library();
    machine.handle(Input::SetSearch("zzz".to_owned()));
    assert_eq!(
        machine.view().unwrap().empty,
        Some(EmptyState::NothingMatches)
    );
    machine.handle(Input::SetSearch(String::new()));
    assert_eq!(machine.view().unwrap().empty, None);
}

#[test]
fn scan_state_and_failures_are_in_the_view() {
    let (mut machine, effects) = loaded(false, LibraryCache::default());
    assert!(machine.view().unwrap().scanning);
    let (op, _) = crate::library::machine::tests::scan_ops(&effects)[0].clone();
    crate::library::machine::tests::scanned(&mut machine, op, "steam", Err("no access"), 4);
    let view = machine.view().unwrap();
    assert!(!view.scanning);
    assert_eq!(view.failed_providers, ["Steam"]);
    // A failed scan is not "no games found": the previous (here empty) list is kept, and the
    // failure is what the UI reports.
}

#[test]
fn keyboard_movement_and_enter_open_the_details() {
    let mut machine = library();
    machine.handle(Input::SetDetailOpen(false));
    machine.handle(Input::Move {
        direction: Direction::Down,
        extend: false,
    });
    machine.handle(Input::Move {
        direction: Direction::Down,
        extend: false,
    });
    machine.handle(Input::OpenDetails);
    let view = machine.view().unwrap();
    assert_eq!(view.selected, ["steam:70"]);
    assert!(view.layout.detail_open);
    assert_eq!(view.detail.unwrap().row.title, "Half-Life");
}

#[test]
fn the_view_separates_the_visible_total_from_the_installed_total() {
    let cache = LibraryCache {
        entries: vec![
            stored("steam", "620", "Portal 2", true),
            stored_with("steam", "70", "Half-Life", false, true, false),
            stored_with("steam", "400", "Portal", false, false, true),
        ],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    finish_scan(&mut machine, vec![record("620", "Portal 2", "/p")], 5);

    let view = machine.view().unwrap();
    assert_eq!(view.total, 3);
    assert_eq!(view.installed_total, 1);
    let count = &view.providers[0];
    assert_eq!(
        (
            count.count,
            count.installed_count,
            count.owned_count,
            count.family_count
        ),
        (3, 1, 1, 1)
    );
}

#[test]
fn the_installed_filter_is_persisted_and_saved_before_this_field_existed() {
    let mut machine = library();
    assert!(machine.view().unwrap().layout.filters.is_default());

    let effects = machine.handle(Input::SetFilters(installed_filter()));
    let [Effect::PersistLayout(layout)] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert!(layout.filters.availability.installed);
    let view = machine.view().unwrap();
    assert!(view.layout.filters.availability.installed);
    assert_eq!(view.rows.len(), 3);
    assert!(
        machine
            .handle(Input::SetFilters(installed_filter()))
            .is_empty()
    );
}

#[test]
fn no_installed_is_its_own_empty_state_and_names_the_selected_provider() {
    let cache = LibraryCache {
        entries: vec![stored_with("steam", "70", "Half-Life", false, true, false)],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    finish_scan(&mut machine, vec![], 5);
    machine.handle(Input::SetFilters(installed_filter()));

    let view = machine.view().unwrap();
    assert_eq!(view.total, 1);
    assert_eq!(view.installed_total, 0);
    assert!(view.rows.is_empty());
    assert_eq!(view.empty, Some(EmptyState::NoInstalled { provider: None }));
    assert_eq!(
        serde_json::to_value(&view.empty).unwrap(),
        serde_json::json!({ "noInstalled": { "provider": null } })
    );

    machine.handle(Input::SetFilter(SidebarSelection::Provider(
        "steam".to_owned(),
    )));
    assert_eq!(
        machine.view().unwrap().empty,
        Some(EmptyState::NoInstalled {
            provider: Some("steam".to_owned())
        })
    );

    // Once something is installed the state disappears; with the filter off it never appears.
    machine.handle(Input::ScanRequested);
    finish_scan(&mut machine, vec![record("70", "Half-Life", "/h")], 6);
    machine.handle(Input::SetFilters(LibraryFilters::default()));
    let view = machine.view().unwrap();
    assert_eq!(view.installed_total, 1);
    assert_eq!(view.empty, None);
}

#[test]
fn no_games_still_wins_over_the_installed_filter() {
    let (mut machine, _) = loaded(false, LibraryCache::default());
    finish_scan(&mut machine, vec![], 5);
    machine.handle(Input::SetFilters(installed_filter()));
    assert_eq!(
        machine.view().unwrap().empty,
        Some(EmptyState::NoGames {
            stores: vec!["Steam".to_owned()]
        })
    );
}

#[test]
fn no_installed_is_scoped_to_the_selected_provider() {
    let cache = LibraryCache {
        entries: vec![
            stored_with("steam", "1", "Owned", false, true, false),
            stored_with("shop", "2", "Installed", false, false, false),
        ],
        ..LibraryCache::default()
    };
    let (mut machine, effects) = loaded(true, cache);
    for (op, provider) in crate::library::machine::tests::scan_ops(&effects) {
        let records = if provider == "shop" {
            vec![record("2", "Installed", "/i")]
        } else {
            vec![]
        };
        crate::library::machine::tests::scanned(&mut machine, op, &provider, Ok(records), 5);
    }

    machine.handle(Input::SetFilters(installed_filter()));
    machine.handle(Input::SetFilter(SidebarSelection::Provider(
        "steam".to_owned(),
    )));
    let view = machine.view().unwrap();
    assert_eq!(view.total, 2);
    assert_eq!(
        view.empty,
        Some(EmptyState::NoInstalled {
            provider: Some("steam".to_owned())
        }),
        "another provider has an installed game, but not the selected one"
    );
    let steam = view
        .providers
        .iter()
        .find(|count| count.id == "steam")
        .unwrap();
    assert_eq!((steam.count, steam.installed_count), (0, 0));
    let shop = view
        .providers
        .iter()
        .find(|count| count.id == "shop")
        .unwrap();
    assert_eq!((shop.count, shop.installed_count), (1, 1));

    // Without the provider selection the installed game keeps the view out of that state.
    machine.handle(Input::SetFilter(SidebarSelection::All));
    assert_eq!(machine.view().unwrap().empty, None);
}

#[test]
fn filters_that_hide_every_row_show_nothing_matches() {
    let mut machine = library();
    machine.handle(Input::SetFilters(LibraryFilters {
        favorites_only: true,
        ..LibraryFilters::default()
    }));
    let view = machine.view().unwrap();
    assert_eq!(view.total, 3);
    assert!(view.rows.is_empty());
    assert_eq!(view.empty, Some(EmptyState::NothingMatches));
    // The sidebar count follows the facet even though the view is empty.
    assert_eq!(
        view.providers
            .iter()
            .map(|count| count.count)
            .sum::<usize>(),
        0
    );
}
