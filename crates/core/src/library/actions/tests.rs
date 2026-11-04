use super::*;
use crate::library::machine::tests::{finish_scan, loaded, record, stored};
use crate::library::machine::{Input, LibraryMachine};
use crate::library::selection::SelectMode;
use crate::library::store::{AutoMatch, ChosenMatch, LibraryCache};
use crate::provider::Platform;
use crate::provider::registry;

fn auto(uid: &str, ids: &[u64]) -> AutoMatch {
    AutoMatch {
        category: 1,
        uid: uid.to_owned(),
        game_ids: ids.to_vec(),
        checked_at: 1,
    }
}

/// Steam entries 620 (matched automatically to game 7), 70 (unmatched) and 240 (matched).
fn library(access: Access) -> LibraryMachine {
    let cache = LibraryCache {
        entries: vec![
            stored("steam", "620", "Portal 2", true),
            stored("steam", "70", "Half-Life", true),
            stored("steam", "240", "Counter-Strike", true),
        ],
        auto_matches: vec![auto("620", &[7]), auto("240", &[9])],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    finish_scan(
        &mut machine,
        vec![
            record("620", "Portal 2", "/p"),
            record("70", "Half-Life", "/h"),
            record("240", "Counter-Strike", "/c"),
        ],
        5,
    );
    machine.handle(Input::SetAccess { access, now: 1_000 });
    machine
}

fn select(machine: &mut LibraryMachine, keys: &[&str]) {
    for (i, key) in keys.iter().enumerate() {
        machine.handle(Input::Select {
            key: (*key).to_owned(),
            mode: if i == 0 {
                SelectMode::Replace
            } else {
                SelectMode::Toggle
            },
        });
    }
}

fn disabled(reason: Reason) -> Availability {
    Availability::Disabled(reason)
}

#[test]
fn nothing_selected_offers_nothing() {
    let machine = library(Access::Verified);
    let actions = machine.actions();
    assert_eq!(actions.selected, 0);
    assert_eq!(actions.play, disabled(Reason::NothingSelected));
    assert_eq!(actions.add_to_gamenode, disabled(Reason::NothingSelected));
}

#[test]
fn an_unmatched_game_can_be_played_and_matched_but_not_added() {
    let mut machine = library(Access::Verified);
    select(&mut machine, &["steam:70"]);
    let actions = machine.actions();

    assert_eq!(actions.selected, 1);
    assert!(actions.play.is_available() && actions.install.is_available());
    assert!(actions.match_game.is_available());
    assert!(!actions.change_match, "it is a first match, not a change");
    assert_eq!(actions.add_to_gamenode, disabled(Reason::NotMatched));
}

#[test]
fn a_game_marked_as_not_on_gamenode_cannot_be_added_but_its_choice_can_change() {
    let mut machine = library(Access::Verified);
    machine.chosen.insert(
        "steam:70".to_owned(),
        ChosenMatch {
            entry_key: "steam:70".to_owned(),
            game_id: None,
            chosen_at: 1,
        },
    );
    select(&mut machine, &["steam:70"]);
    let actions = machine.actions();
    assert_eq!(actions.add_to_gamenode, disabled(Reason::NotMatched));
    assert!(actions.change_match && actions.match_game.is_available());
}

#[test]
fn a_matched_game_is_added_only_once_membership_says_it_is_absent() {
    let mut machine = library(Access::Verified);
    select(&mut machine, &["steam:620"]);
    assert!(machine.actions().change_match);

    // Not read yet, then being read: not addable.
    for membership in [Membership::NotChecked, Membership::Checking] {
        machine.membership.insert(7, membership);
        assert_eq!(
            machine.actions().add_to_gamenode,
            disabled(Reason::CheckingMembership)
        );
    }
    machine.membership.insert(7, Membership::Unavailable);
    assert_eq!(
        machine.actions().add_to_gamenode,
        disabled(Reason::MembershipUnavailable)
    );
    machine
        .membership
        .insert(7, Membership::InLibrary(Some(EntryStatus::Finished)));
    assert_eq!(
        machine.actions().add_to_gamenode,
        disabled(Reason::AlreadyInLibrary(Some(EntryStatus::Finished)))
    );
    machine.membership.insert(7, Membership::NotInLibrary);
    assert!(machine.actions().add_to_gamenode.is_available());
}

#[test]
fn a_multi_selection_offers_no_single_game_actions() {
    let mut machine = library(Access::Verified);
    select(&mut machine, &["steam:620", "steam:70"]);
    let actions = machine.actions();

    assert_eq!(actions.selected, 2);
    for single in [actions.play, actions.install, actions.match_game] {
        assert_eq!(single, disabled(Reason::MultipleSelected));
    }
    // One of them is matched, so adding is offered; each game is checked when confirming.
    assert!(actions.add_to_gamenode.is_available());
}

#[test]
fn adding_a_multi_selection_needs_at_least_one_matched_game() {
    let mut machine = library(Access::Verified);
    machine.chosen.insert(
        "steam:70".to_owned(),
        ChosenMatch {
            entry_key: "steam:70".to_owned(),
            game_id: None,
            chosen_at: 1,
        },
    );
    // 70 is "no game"; add an unmatched one by removing 240's automatic match.
    machine.auto_matches.clear();
    select(&mut machine, &["steam:70", "steam:240"]);
    assert_eq!(
        machine.actions().add_to_gamenode,
        disabled(Reason::NoMatchedGames)
    );
}

#[test]
fn the_cached_library_before_verification_plays_but_cannot_use_the_network() {
    let mut machine = library(Access::Unverified);
    select(&mut machine, &["steam:620"]);
    let actions = machine.actions();

    assert!(actions.play.is_available(), "Play is local");
    assert!(actions.install.is_available(), "Install is local");
    assert_eq!(actions.match_game, disabled(Reason::NeedsVerifiedSession));
    assert_eq!(
        actions.add_to_gamenode,
        disabled(Reason::NeedsVerifiedSession)
    );

    // Even a membership read earlier does not enable Add without a verified session.
    machine.membership.insert(7, Membership::NotInLibrary);
    assert_eq!(
        machine.actions().add_to_gamenode,
        disabled(Reason::NeedsVerifiedSession)
    );

    // Several games: the same explanation.
    select(&mut machine, &["steam:620", "steam:240"]);
    assert_eq!(
        machine.actions().add_to_gamenode,
        disabled(Reason::NeedsVerifiedSession)
    );
}

#[test]
fn offline_with_a_verified_session_says_gamenode_cannot_be_reached() {
    let mut machine = library(Access::Offline);
    select(&mut machine, &["steam:620"]);
    let actions = machine.actions();
    assert!(actions.play.is_available() && actions.install.is_available());
    assert_eq!(actions.match_game, disabled(Reason::Offline));
    assert_eq!(actions.add_to_gamenode, disabled(Reason::Offline));
}

#[test]
fn a_provider_without_rules_or_a_record_missing_a_field_disables_play_and_install() {
    // `shop` has no actions; `deluxe` needs a field the scan never provides.
    let manifests = [
        (
            "shop.toml",
            crate::library::machine::tests::SHOP_MANIFEST.to_owned(),
        ),
        (
            "deluxe.toml",
            r#"
id = "deluxe"
name = "Deluxe"
platforms = ["windows", "linux"]
source = "deluxe"
scan = { native = "steam" }
launch = { uri = "deluxe://run/{launch_id}" }
install = { uri = "deluxe://install/{id}" }
"#
            .to_owned(),
        ),
    ];
    let sources: Vec<(&str, &str)> = manifests.iter().map(|(f, t)| (*f, t.as_str())).collect();
    let loaded_providers = registry::load(&sources, Platform::Linux);
    assert!(loaded_providers.diagnostics.is_empty());

    let mut machine = LibraryMachine::new(loaded_providers.providers, Default::default());
    machine
        .entries
        .insert("shop:1".to_owned(), stored("shop", "1", "Plain", true));
    machine
        .entries
        .insert("deluxe:1".to_owned(), stored("deluxe", "1", "Fancy", true));
    machine.access = Access::Verified;

    select(&mut machine, &["shop:1"]);
    let actions = machine.actions();
    assert_eq!(actions.play, disabled(Reason::NoLaunchRule));
    assert_eq!(actions.install, disabled(Reason::NoInstallRule));

    select(&mut machine, &["deluxe:1"]);
    let actions = machine.actions();
    assert_eq!(actions.play, disabled(Reason::MissingField));
    assert!(actions.install.is_available());
}

#[test]
fn availability_helpers() {
    assert_eq!(Availability::Available.reason(), None);
    assert_eq!(disabled(Reason::Offline).reason(), Some(Reason::Offline));
    assert!(!disabled(Reason::Offline).is_available());
}
