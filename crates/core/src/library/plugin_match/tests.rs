use super::*;
use crate::library::machine::tests::{loaded, record, stored};
use crate::library::store::{AutoMatch, GameRecord, LibraryCache};
use crate::library::{Access, Input, SelectMode, entry_key};

const NOW: i64 = 5_000;

fn game(id: u64, name: &str) -> GameSummary {
    GameSummary {
        id,
        name: name.to_owned(),
        release_year: Some(2011),
        platforms: vec![],
        cover_url: None,
    }
}

fn known(id: u64, name: &str) -> GameRecord {
    GameRecord {
        game_id: id,
        name: name.to_owned(),
        release_year: Some(2011),
        platforms: vec![],
        cover_url: None,
        cover_file: None,
        cover_failed_at: None,
    }
}

/// A verified library: Portal 2 (620, automatically matched to game 7), Dota 2 (570, unknown to
/// GameNode). Game 7 is known; game 40 is not.
fn library() -> LibraryMachine {
    let mut cache = LibraryCache::default();
    for (id, name) in [("620", "Portal 2"), ("570", "Dota 2")] {
        cache.entries.push(stored("steam", id, name, true));
    }
    for (uid, ids) in [("620", vec![7]), ("570", vec![])] {
        cache.auto_matches.push(AutoMatch {
            category: 1,
            uid: uid.to_owned(),
            game_ids: ids,
            checked_at: NOW,
        });
    }
    cache.games = vec![known(7, "Portal 2")];
    let (mut machine, _) = loaded(false, cache);
    let op = machine.scan.as_ref().unwrap().op;
    machine.handle(Input::ProviderScanned {
        targets: Default::default(),
        op,
        provider: "steam".to_owned(),
        result: Ok(vec![
            record("620", "Portal 2", "/g"),
            record("570", "Dota 2", "/g"),
        ]),
        now: NOW,
    });
    machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: NOW,
    });
    machine
}

fn assign(machine: &mut LibraryMachine, id: &str, game_id: u64) -> Vec<Effect> {
    machine.handle(Input::AssignPluginMatch {
        key: entry_key("steam", id),
        game_id,
        now: NOW + 1,
    })
}

fn persisted_choices(effects: &[Effect]) -> Vec<ChosenMatch> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::PersistChoice { choice, .. } => Some(choice.clone()),
            _ => None,
        })
        .collect()
}

fn matched(machine: &LibraryMachine, id: &str) -> Option<u64> {
    machine
        .match_status(machine.entry(&entry_key("steam", id)).unwrap())
        .game_id()
}

#[test]
fn the_plan_names_why_an_assignment_is_not_possible() {
    let mut machine = library();
    let key = entry_key("steam", "570");
    assert_eq!(machine.plan_plugin_match(&key, 7), PluginMatch::Assign);
    assert_eq!(
        machine.plan_plugin_match("steam:nope", 7),
        PluginMatch::UnknownEntry
    );
    assert_eq!(
        machine.plan_plugin_match(&key, 40),
        PluginMatch::UnknownGame
    );
    machine.account = None;
    assert_eq!(machine.plan_plugin_match(&key, 7), PluginMatch::NoAccount);
}

#[test]
fn a_game_a_search_offered_can_be_assigned_and_is_stored_with_the_choice() {
    let mut machine = library();
    assert!(assign(&mut machine, "570", 40).is_empty());
    assert_eq!(matched(&machine, "570"), None);

    machine.handle(Input::CatalogSearched {
        games: vec![game(40, "Defense of the Ancients")],
    });
    let effects = assign(&mut machine, "570", 40);

    let [choice] = persisted_choices(&effects).try_into().unwrap();
    assert_eq!(choice.entry_key, entry_key("steam", "570"));
    assert_eq!(choice.game_id, Some(40));
    assert_eq!(choice.chosen_at, NOW + 1);
    assert_eq!(choice.source, ChoiceSource::Plugin);
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::PersistMatches { games, .. } if games.iter().any(|g| g.game_id == 40)
    )));
    assert_eq!(matched(&machine, "570"), Some(40));
    assert_eq!(machine.game(40).unwrap().name, "Defense of the Ancients");
}

#[test]
fn a_known_game_needs_no_search() {
    let mut machine = library();
    let effects = assign(&mut machine, "570", 7);
    assert_eq!(persisted_choices(&effects).len(), 1);
    assert_eq!(matched(&machine, "570"), Some(7));
}

#[test]
fn assigning_the_game_an_explicit_choice_already_names_changes_nothing() {
    let mut machine = library();
    assert!(!assign(&mut machine, "570", 7).is_empty());
    assert_eq!(
        machine.plan_plugin_match(&entry_key("steam", "570"), 7),
        PluginMatch::Already
    );
    assert!(assign(&mut machine, "570", 7).is_empty());
}

#[test]
fn an_automatic_match_is_pinned_by_an_assignment_of_the_same_game() {
    let mut machine = library();
    assert_eq!(matched(&machine, "620"), Some(7));
    let effects = assign(&mut machine, "620", 7);
    assert_eq!(persisted_choices(&effects)[0].source, ChoiceSource::Plugin);
}

#[test]
fn a_plugin_replaces_its_own_choice_but_never_the_users() {
    let mut machine = library();
    machine.handle(Input::CatalogSearched {
        games: vec![game(40, "Other")],
    });
    assign(&mut machine, "570", 7);
    assert!(!assign(&mut machine, "570", 40).is_empty());
    assert_eq!(matched(&machine, "570"), Some(40));

    // The user picks a different game in the dialog...
    machine.chosen.insert(
        entry_key("steam", "570"),
        ChosenMatch {
            entry_key: entry_key("steam", "570"),
            game_id: Some(7),
            chosen_at: NOW + 2,
            source: ChoiceSource::User,
        },
    );
    assert_eq!(
        machine.plan_plugin_match(&entry_key("steam", "570"), 40),
        PluginMatch::UserChoice
    );
    assert!(assign(&mut machine, "570", 40).is_empty());
    assert_eq!(matched(&machine, "570"), Some(7));

    // ...and "no game" is a choice too.
    machine
        .chosen
        .get_mut(&entry_key("steam", "570"))
        .unwrap()
        .game_id = None;
    assert_eq!(
        machine.plan_plugin_match(&entry_key("steam", "570"), 7),
        PluginMatch::UserChoice
    );
}

#[test]
fn the_user_picking_the_plugins_game_makes_it_theirs() {
    let mut machine = library();
    let key = entry_key("steam", "570");
    machine.handle(Input::CatalogSearched {
        games: vec![game(40, "Defense of the Ancients")],
    });
    assign(&mut machine, "570", 40);
    assert_eq!(
        machine.chosen_match(&key).unwrap().source,
        ChoiceSource::Plugin
    );

    machine.handle(Input::Select {
        key: key.clone(),
        mode: SelectMode::Replace,
    });
    let effects = machine.handle(Input::OpenMatch);
    let op = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::SearchGames { op, .. } => Some(*op),
            _ => None,
        })
        .expect("opening searches the detected title");
    machine.handle(Input::MatchSearched {
        op,
        result: Ok(vec![game(40, "Defense of the Ancients")]),
    });
    let effects = machine.handle(Input::PickMatch {
        game_id: 40,
        now: NOW + 3,
    });

    assert_eq!(persisted_choices(&effects)[0].source, ChoiceSource::User);
    assert_eq!(
        machine.chosen_match(&key).unwrap().source,
        ChoiceSource::User
    );
    assert_eq!(machine.plan_plugin_match(&key, 40), PluginMatch::Already);
    machine.handle(Input::CatalogSearched {
        games: vec![game(41, "Other")],
    });
    assert_eq!(machine.plan_plugin_match(&key, 41), PluginMatch::UserChoice);
}

#[test]
fn offers_are_bounded_and_the_oldest_drop_first() {
    let mut machine = library();
    let games: Vec<_> = (0..MAX_OFFERED_GAMES as u64 + 5)
        .map(|id| game(1000 + id, "Game"))
        .collect();
    machine.handle(Input::CatalogSearched { games });
    assert_eq!(machine.offered.len(), MAX_OFFERED_GAMES);
    let key = entry_key("steam", "570");
    assert_eq!(
        machine.plan_plugin_match(&key, 1000),
        PluginMatch::UnknownGame
    );
    assert_eq!(
        machine.plan_plugin_match(&key, 1004),
        PluginMatch::UnknownGame
    );
    assert_eq!(machine.plan_plugin_match(&key, 1005), PluginMatch::Assign);
}
