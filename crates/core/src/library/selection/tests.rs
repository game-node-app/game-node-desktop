use super::*;
use crate::library::layout::{SidebarSelection, ViewMode};
use crate::library::machine::Input;
use crate::library::machine::tests::{finish_scan, loaded, record};
use crate::library::store::LibraryCache;

fn keys(count: usize) -> Vec<String> {
    (0..count).map(|i| format!("k{i}")).collect()
}

fn chosen(selection: &Selection, visible: &[String]) -> Vec<String> {
    selection.visible(visible)
}

fn names(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn a_plain_click_selects_only_that_entry() {
    let visible = keys(5);
    let mut selection = Selection::default();
    selection.select("k1", SelectMode::Replace, &visible);
    selection.select("k3", SelectMode::Replace, &visible);
    assert_eq!(chosen(&selection, &visible), names(&["k3"]));
    assert_eq!(selection.focus(&visible), Some("k3"));
}

#[test]
fn ctrl_click_adds_and_removes_entries() {
    let visible = keys(5);
    let mut selection = Selection::default();
    selection.select("k1", SelectMode::Toggle, &visible);
    selection.select("k3", SelectMode::Toggle, &visible);
    assert_eq!(chosen(&selection, &visible), names(&["k1", "k3"]));
    selection.select("k1", SelectMode::Toggle, &visible);
    assert_eq!(chosen(&selection, &visible), names(&["k3"]));
    selection.select("k3", SelectMode::Toggle, &visible);
    assert!(selection.is_empty());
}

#[test]
fn shift_click_selects_the_range_from_the_anchor_in_either_direction() {
    let visible = keys(6);
    let mut selection = Selection::default();
    selection.select("k2", SelectMode::Replace, &visible);
    selection.select("k4", SelectMode::Range, &visible);
    assert_eq!(chosen(&selection, &visible), names(&["k2", "k3", "k4"]));

    // The anchor stays where the plain click put it, so the range can pivot around it.
    selection.select("k0", SelectMode::Range, &visible);
    assert_eq!(chosen(&selection, &visible), names(&["k0", "k1", "k2"]));
    assert_eq!(selection.focus(&visible), Some("k0"));
}

#[test]
fn shift_click_without_an_anchor_selects_just_that_entry() {
    let visible = keys(4);
    let mut selection = Selection::default();
    selection.select("k2", SelectMode::Range, &visible);
    assert_eq!(chosen(&selection, &visible), names(&["k2"]));
}

#[test]
fn ctrl_click_moves_the_anchor() {
    let visible = keys(6);
    let mut selection = Selection::default();
    selection.select("k0", SelectMode::Replace, &visible);
    selection.select("k3", SelectMode::Toggle, &visible);
    selection.select("k5", SelectMode::Range, &visible);
    // The range runs from the Ctrl-clicked entry, replacing the earlier selection.
    assert_eq!(chosen(&selection, &visible), names(&["k3", "k4", "k5"]));
}

#[test]
fn clicking_an_entry_that_is_not_shown_does_nothing() {
    let visible = keys(3);
    let mut selection = Selection::default();
    selection.select("k1", SelectMode::Replace, &visible);
    selection.select("gone", SelectMode::Replace, &visible);
    assert_eq!(chosen(&selection, &visible), names(&["k1"]));
}

#[test]
fn arrows_move_by_one_in_a_row_and_by_the_column_count_between_rows() {
    // 3 columns, 8 items:  k0 k1 k2 / k3 k4 k5 / k6 k7
    let visible = keys(8);
    let mut selection = Selection::default();
    selection.select("k4", SelectMode::Replace, &visible);

    let mut go = |direction, expected: &str| {
        selection.step(direction, false, 3, &visible);
        assert_eq!(selection.focus(&visible), Some(expected), "{direction:?}");
        assert_eq!(chosen(&selection, &visible), names(&[expected]));
    };
    go(Direction::Right, "k5");
    go(Direction::Left, "k4");
    go(Direction::Down, "k7");
    go(Direction::Up, "k4");
    go(Direction::Up, "k1");
    // The top row has nothing above: stay put.
    go(Direction::Up, "k1");
    go(Direction::Left, "k0");
    go(Direction::Left, "k0");
}

#[test]
fn down_into_a_partial_last_row_lands_on_its_last_entry() {
    // 3 columns, 8 items: from k2 (column 3) the last row only has k6 and k7.
    let visible = keys(8);
    let mut selection = Selection::default();
    selection.select("k5", SelectMode::Replace, &visible);
    selection.step(Direction::Down, false, 3, &visible);
    assert_eq!(selection.focus(&visible), Some("k7"));
    // Already in the last row: nowhere further down.
    selection.step(Direction::Down, false, 3, &visible);
    assert_eq!(selection.focus(&visible), Some("k7"));
    // The right edge of the last item does not run past the end.
    selection.step(Direction::Right, false, 3, &visible);
    assert_eq!(selection.focus(&visible), Some("k7"));
}

#[test]
fn the_list_moves_one_entry_at_a_time() {
    let visible = keys(4);
    let mut selection = Selection::default();
    selection.select("k1", SelectMode::Replace, &visible);
    selection.step(Direction::Down, false, 1, &visible);
    assert_eq!(selection.focus(&visible), Some("k2"));
    selection.step(Direction::Up, false, 1, &visible);
    selection.step(Direction::Up, false, 1, &visible);
    assert_eq!(selection.focus(&visible), Some("k0"));
}

#[test]
fn home_and_end_jump_to_the_first_and_last_entry() {
    let visible = keys(8);
    let mut selection = Selection::default();
    selection.select("k4", SelectMode::Replace, &visible);
    selection.step(Direction::End, false, 3, &visible);
    assert_eq!(chosen(&selection, &visible), names(&["k7"]));
    selection.step(Direction::Home, false, 3, &visible);
    assert_eq!(chosen(&selection, &visible), names(&["k0"]));
}

#[test]
fn shift_with_a_movement_extends_from_the_anchor() {
    let visible = keys(8);
    let mut selection = Selection::default();
    selection.select("k1", SelectMode::Replace, &visible);
    selection.step(Direction::Right, true, 3, &visible);
    selection.step(Direction::Right, true, 3, &visible);
    assert_eq!(chosen(&selection, &visible), names(&["k1", "k2", "k3"]));
    // Going back shrinks it again, and passing the anchor flips the side.
    selection.step(Direction::Left, true, 3, &visible);
    assert_eq!(chosen(&selection, &visible), names(&["k1", "k2"]));
    // Focus is on k2; one row down is k5.
    selection.step(Direction::Down, true, 3, &visible);
    assert_eq!(
        chosen(&selection, &visible),
        names(&["k1", "k2", "k3", "k4", "k5"])
    );
    selection.step(Direction::End, true, 3, &visible);
    assert_eq!(selection.visible(&visible).len(), 7);
    selection.step(Direction::Home, true, 3, &visible);
    assert_eq!(chosen(&selection, &visible), names(&["k0", "k1"]));
}

#[test]
fn the_first_key_press_lands_on_the_first_entry() {
    let visible = keys(4);
    let mut selection = Selection::default();
    selection.step(Direction::Down, false, 2, &visible);
    assert_eq!(selection.focus(&visible), Some("k0"));

    let mut selection = Selection::default();
    selection.step(Direction::Up, false, 2, &visible);
    assert_eq!(selection.focus(&visible), Some("k3"));
}

#[test]
fn moving_in_an_empty_list_does_nothing() {
    let mut selection = Selection::default();
    selection.step(Direction::Down, false, 3, &[]);
    assert!(selection.is_empty());
}

#[test]
fn removed_entries_drop_out_of_the_selection() {
    let visible = keys(4);
    let mut selection = Selection::default();
    selection.select("k0", SelectMode::Replace, &visible);
    selection.select("k2", SelectMode::Toggle, &visible);
    selection.retain(|key| key != "k2");
    assert_eq!(chosen(&selection, &visible), names(&["k0"]));
    assert_eq!(selection.focus(&visible), None, "the focused entry is gone");
}

// ---- through the machine ----

fn library_of(count: usize) -> crate::library::machine::LibraryMachine {
    let (mut machine, _) = loaded(false, LibraryCache::default());
    let records = (0..count)
        .map(|i| record(&format!("{i}"), &format!("Game {i}"), "/g"))
        .collect();
    finish_scan(&mut machine, records, 1);
    machine
}

fn click(machine: &mut crate::library::machine::LibraryMachine, key: &str, mode: SelectMode) {
    machine.handle(Input::Select {
        key: key.to_owned(),
        mode,
    });
}

#[test]
fn the_selection_survives_a_view_switch() {
    let mut machine = library_of(6);
    machine.handle(Input::SetColumns(3));
    click(&mut machine, "steam:1", SelectMode::Replace);
    click(&mut machine, "steam:3", SelectMode::Range);
    let before = machine.selected_keys();
    assert_eq!(before, names(&["steam:1", "steam:2", "steam:3"]));

    machine.layout.view = ViewMode::List;
    assert_eq!(machine.selected_keys(), before);
    assert_eq!(machine.focused_key().as_deref(), Some("steam:3"));
    // The list moves by one row, whatever the grid's column count.
    machine.handle(Input::Move {
        direction: Direction::Down,
        extend: false,
    });
    assert_eq!(machine.selected_keys(), names(&["steam:4"]));

    machine.layout.view = ViewMode::Grid;
    machine.handle(Input::Move {
        direction: Direction::Down,
        extend: false,
    });
    // k4 (column 2, row 2 of 3 columns) is already in the last row.
    assert_eq!(machine.selected_keys(), names(&["steam:4"]));
}

#[test]
fn the_selection_survives_a_filter_change_and_hidden_entries_are_not_acted_on() {
    let mut machine = library_of(4);
    click(&mut machine, "steam:0", SelectMode::Replace);
    click(&mut machine, "steam:2", SelectMode::Toggle);
    assert_eq!(machine.selected_keys(), names(&["steam:0", "steam:2"]));

    // A search hides steam:0: only the shown selection counts...
    machine.search = "game 2".to_owned();
    assert_eq!(machine.selected_keys(), names(&["steam:2"]));
    // ...and clearing the search brings it back.
    machine.search.clear();
    assert_eq!(machine.selected_keys(), names(&["steam:0", "steam:2"]));

    // Same for the sidebar.
    machine.layout.sidebar = SidebarSelection::Provider("steam".to_owned());
    assert_eq!(machine.selected_keys().len(), 2);
}

#[test]
fn an_uninstalled_game_leaves_the_selection() {
    let mut machine = library_of(3);
    click(&mut machine, "steam:1", SelectMode::Replace);
    click(&mut machine, "steam:2", SelectMode::Toggle);

    machine.handle(Input::ScanRequested);
    finish_scan(&mut machine, vec![record("2", "Game 2", "/g")], 5);
    assert_eq!(machine.selected_keys(), names(&["steam:2"]));
    // Even when it returns, it does not come back selected.
    machine.handle(Input::ScanRequested);
    finish_scan(
        &mut machine,
        vec![record("1", "Game 1", "/g"), record("2", "Game 2", "/g")],
        6,
    );
    assert_eq!(machine.selected_keys(), names(&["steam:2"]));
}

#[test]
fn signing_out_clears_the_selection() {
    let mut machine = library_of(2);
    click(&mut machine, "steam:0", SelectMode::Replace);
    machine.handle(Input::AccountChanged(None));
    assert!(machine.selected_keys().is_empty());
}
