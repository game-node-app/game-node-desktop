use super::*;
use crate::library::machine::tests::{finish_scan, loaded, loaded_with, record, stored};
use crate::library::matching::{MatchSource, MatchStatus};
use crate::library::store::{ChosenMatch, LibraryCache};
use crate::library::{Input, entry_key};
use crate::provider::Platform;
use crate::provider::Record;
use crate::provider::registry::{self, tests::STEAM_MANIFEST};

const NOW: i64 = 1_000_000;

fn summary(id: u64, name: &str) -> GameSummary {
    GameSummary {
        id,
        name: name.to_owned(),
        release_year: Some(2011),
        cover_url: Some(format!(
            "//images.igdb.com/igdb/image/upload/t_thumb/{id}.jpg"
        )),
    }
}

fn found(uid: &str, game: GameSummary) -> ExternalMatch {
    ExternalMatch {
        uid: uid.to_owned(),
        game,
    }
}

fn lookups(effects: &[Effect]) -> Vec<(u64, u32, Vec<String>)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Lookup { op, category, uids } => Some((*op, *category, uids.clone())),
            _ => None,
        })
        .collect()
}

fn persisted(effects: &[Effect]) -> Option<(&[GameRecord], &[AutoMatch])> {
    effects.iter().find_map(|effect| match effect {
        Effect::PersistMatches { games, matches } => Some((games.as_slice(), matches.as_slice())),
        _ => None,
    })
}

/// A verified machine with Portal 2 (620) and Dota 2 (570) installed and the scan finished.
/// Returns the lookup the end of the scan started.
fn verified() -> (LibraryMachine, (u64, u32, Vec<String>)) {
    let mut cache = LibraryCache::default();
    cache.entries.push(stored("steam", "620", "Portal 2", true));
    cache.entries.push(stored("steam", "570", "Dota 2", true));
    let (mut machine, _) = loaded(false, cache);
    machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: NOW,
    });
    let op = machine.scan.as_ref().expect("scan running").op;
    let effects = machine.handle(Input::ProviderScanned {
        op,
        provider: "steam".to_owned(),
        result: Ok(vec![
            record("620", "Portal 2", "/games/Portal 2"),
            record("570", "Dota 2", "/games/Dota 2"),
        ]),
        now: NOW,
    });
    let mut asked = lookups(&effects);
    assert_eq!(asked.len(), 1, "{effects:?}");
    (machine, asked.remove(0))
}

fn status(machine: &LibraryMachine, id: &str) -> MatchStatus {
    machine.match_status(&machine.entries[&entry_key("steam", id)])
}

fn done(
    machine: &mut LibraryMachine,
    ask: &(u64, u32, Vec<String>),
    result: Result<Vec<ExternalMatch>, ApiError>,
    now: i64,
) -> Vec<Effect> {
    machine.handle(Input::LookupDone {
        op: ask.0,
        category: ask.1,
        result,
        now,
    })
}

#[test]
fn a_finished_scan_looks_up_every_unchecked_id_once() {
    let (_, ask) = verified();
    assert_eq!(ask.1, 1, "steam's external category");
    assert_eq!(ask.2, ["570", "620"]);
}

#[test]
fn nothing_is_looked_up_without_a_verified_session() {
    for access in [Access::Unverified, Access::Offline] {
        let mut cache = LibraryCache::default();
        cache.entries.push(stored("steam", "620", "Portal 2", true));
        let (mut machine, _) = loaded(false, cache);
        machine.handle(Input::SetAccess { access, now: NOW });
        let op = machine.scan.as_ref().unwrap().op;
        let effects = machine.handle(Input::ProviderScanned {
            op,
            provider: "steam".to_owned(),
            result: Ok(vec![record("620", "Portal 2", "/games/Portal 2")]),
            now: NOW,
        });
        assert!(lookups(&effects).is_empty(), "{access:?}");
        // Becoming offline again or staying unverified later changes nothing either.
        let effects = machine.handle(Input::SetAccess { access, now: NOW });
        assert!(lookups(&effects).is_empty(), "{access:?}");
    }
}

#[test]
fn a_session_verified_after_the_scan_runs_the_lookup_then() {
    let mut cache = LibraryCache::default();
    cache.entries.push(stored("steam", "620", "Portal 2", true));
    let (mut machine, _) = loaded(false, cache);
    finish_scan(&mut machine, vec![record("620", "Portal 2", "/g")], NOW);

    let effects = machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: NOW,
    });
    assert_eq!(lookups(&effects).len(), 1);
    // Verifying again while the lookup is out does not ask twice.
    let effects = machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: NOW,
    });
    assert!(lookups(&effects).is_empty());
}

#[test]
fn an_answer_is_stored_for_every_asked_id_including_unknown_ones() {
    let (mut machine, ask) = verified();
    let effects = done(
        &mut machine,
        &ask,
        Ok(vec![found("620", summary(7, "Portal 2"))]),
        NOW + 5,
    );

    let (games, matches) = persisted(&effects).expect("the answer is persisted");
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].game_id, 7);
    assert_eq!(games[0].release_year, Some(2011));
    assert_eq!(matches.len(), 2, "570 is stored as known to have no game");
    let dota = matches.iter().find(|m| m.uid == "570").unwrap();
    assert!(dota.game_ids.is_empty());
    assert!(
        matches
            .iter()
            .all(|m| m.checked_at == NOW + 5 && m.category == 1)
    );

    assert_eq!(
        status(&machine, "620"),
        MatchStatus::Matched {
            game_id: 7,
            source: MatchSource::Automatic
        }
    );
    assert_eq!(status(&machine, "570"), MatchStatus::Unmatched);
    // Nothing is left to ask about, so the answer does not start another lookup.
    assert!(lookups(&effects).is_empty());
}

#[test]
fn an_id_with_several_games_is_ambiguous_and_keeps_its_candidates() {
    let (mut machine, ask) = verified();
    done(
        &mut machine,
        &ask,
        Ok(vec![
            found("620", summary(7, "Portal 2")),
            found("620", summary(9, "Portal 2: Remaster")),
            found("620", summary(7, "Portal 2")),
        ]),
        NOW,
    );
    assert_eq!(
        status(&machine, "620"),
        MatchStatus::Ambiguous {
            candidates: vec![7, 9]
        }
    );
    assert!(
        machine.games.contains_key(&9),
        "candidates can be shown by name"
    );
}

#[test]
fn a_failed_lookup_changes_nothing_and_the_next_trigger_asks_again() {
    for error in [ApiError::Rejected, ApiError::Failed] {
        let (mut machine, ask) = verified();
        // An earlier answer exists and must survive the failure untouched.
        let before = machine.auto_matches.clone();
        let effects = done(&mut machine, &ask, Err(error), NOW + 10);
        assert!(persisted(&effects).is_none(), "{error:?}");
        assert!(machine.games.is_empty(), "{error:?}");
        assert_eq!(machine.auto_matches, before, "{error:?}");
        assert_eq!(status(&machine, "620"), MatchStatus::Unmatched);

        let effects = machine.handle(Input::SetAccess {
            access: Access::Verified,
            now: NOW + 20,
        });
        assert_eq!(lookups(&effects).len(), 1, "{error:?}: asked again");
    }
}

#[test]
fn a_failed_refresh_keeps_the_previous_answer() {
    let (mut machine, ask) = verified();
    done(
        &mut machine,
        &ask,
        Ok(vec![found("620", summary(7, "Portal 2"))]),
        NOW,
    );

    // A day later the answer is stale; the refresh fails; the old match still shows.
    let later = NOW + LOOKUP_MAX_AGE_SECS + 1;
    let effects = machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: later,
    });
    let ask = lookups(&effects).remove(0);
    assert_eq!(ask.2, ["570", "620"]);
    let effects = done(&mut machine, &ask, Err(ApiError::Failed), later);
    assert!(persisted(&effects).is_none());
    assert_eq!(
        status(&machine, "620"),
        MatchStatus::Matched {
            game_id: 7,
            source: MatchSource::Automatic
        }
    );
}

#[test]
fn ids_checked_within_a_day_are_not_asked_about_again() {
    let (mut machine, ask) = verified();
    done(&mut machine, &ask, Ok(vec![]), NOW);

    let soon = machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: NOW + LOOKUP_MAX_AGE_SECS,
    });
    assert!(lookups(&soon).is_empty());
    let stale = machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: NOW + LOOKUP_MAX_AGE_SECS + 1,
    });
    assert_eq!(lookups(&stale).len(), 1);
}

#[test]
fn a_rescan_that_adds_a_game_asks_only_about_the_new_id() {
    let (mut machine, ask) = verified();
    done(&mut machine, &ask, Ok(vec![]), NOW);

    machine.handle(Input::ScanRequested);
    let op = machine.scan.as_ref().unwrap().op;
    let effects = machine.handle(Input::ProviderScanned {
        op,
        provider: "steam".to_owned(),
        result: Ok(vec![
            record("620", "Portal 2", "/g"),
            record("570", "Dota 2", "/g"),
            record("400", "Portal", "/g"),
        ]),
        now: NOW + 60,
    });
    let asked = lookups(&effects);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].2, ["400"]);
}

#[test]
fn an_uninstalled_game_is_not_looked_up() {
    let mut cache = LibraryCache::default();
    cache.entries.push(stored("steam", "620", "Portal 2", true));
    cache.entries.push(stored("steam", "70", "Half-Life", true));
    let (mut machine, _) = loaded(false, cache);
    machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: NOW,
    });
    let op = machine.scan.as_ref().unwrap().op;
    let effects = machine.handle(Input::ProviderScanned {
        op,
        provider: "steam".to_owned(),
        result: Ok(vec![record("620", "Portal 2", "/g")]),
        now: NOW,
    });
    assert_eq!(lookups(&effects)[0].2, ["620"]);
}

#[test]
fn a_manual_pick_survives_any_lookup_result() {
    for result in [
        vec![found("620", summary(7, "Portal 2"))],
        vec![
            found("620", summary(7, "Portal 2")),
            found("620", summary(9, "Other")),
        ],
        vec![],
    ] {
        let (mut machine, ask) = verified();
        machine.chosen.insert(
            entry_key("steam", "620"),
            ChosenMatch {
                entry_key: entry_key("steam", "620"),
                game_id: Some(42),
                chosen_at: NOW,
            },
        );
        done(&mut machine, &ask, Ok(result), NOW + 1);
        assert_eq!(
            status(&machine, "620"),
            MatchStatus::Matched {
                game_id: 42,
                source: MatchSource::Chosen
            }
        );
    }
}

#[test]
fn no_game_stays_no_game_after_a_lookup_finds_one() {
    let (mut machine, ask) = verified();
    machine.chosen.insert(
        entry_key("steam", "620"),
        ChosenMatch {
            entry_key: entry_key("steam", "620"),
            game_id: None,
            chosen_at: NOW,
        },
    );
    done(
        &mut machine,
        &ask,
        Ok(vec![found("620", summary(7, "Portal 2"))]),
        NOW + 1,
    );
    assert_eq!(status(&machine, "620"), MatchStatus::NoGame);
}

#[test]
fn a_stale_answer_after_the_account_changed_is_ignored() {
    let (mut machine, ask) = verified();
    machine.handle(Input::AccountChanged(None));
    let effects = done(
        &mut machine,
        &ask,
        Ok(vec![found("620", summary(7, "Portal 2"))]),
        NOW,
    );
    assert!(effects.is_empty());
    assert!(machine.games.is_empty() && machine.auto_matches.is_empty());
}

#[test]
fn an_answer_with_the_wrong_op_is_ignored() {
    let (mut machine, ask) = verified();
    let wrong = (ask.0 + 100, ask.1, ask.2.clone());
    assert!(done(&mut machine, &wrong, Ok(vec![]), NOW).is_empty());
    assert!(machine.auto_matches.is_empty());
    // The real answer still lands afterwards.
    assert!(persisted(&done(&mut machine, &ask, Ok(vec![]), NOW)).is_some());
}

#[test]
fn a_new_cover_url_drops_the_downloaded_file() {
    let (mut machine, ask) = verified();
    done(
        &mut machine,
        &ask,
        Ok(vec![found("620", summary(7, "Portal 2"))]),
        NOW,
    );
    machine.games.get_mut(&7).unwrap().cover_file = Some("7.jpg".to_owned());

    let later = NOW + LOOKUP_MAX_AGE_SECS + 1;
    let ask = lookups(&machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: later,
    }))
    .remove(0);

    // Same cover: the file is kept.
    done(
        &mut machine,
        &ask,
        Ok(vec![found("620", summary(7, "Portal 2"))]),
        later,
    );
    assert_eq!(machine.games[&7].cover_file.as_deref(), Some("7.jpg"));

    // A different cover: the file no longer matches.
    let later = later + LOOKUP_MAX_AGE_SECS + 1;
    let ask = lookups(&machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: later,
    }))
    .remove(0);
    let mut changed = summary(7, "Portal 2");
    changed.cover_url = Some("//images.igdb.com/igdb/image/upload/t_thumb/new.jpg".to_owned());
    done(&mut machine, &ask, Ok(vec![found("620", changed)]), later);
    assert_eq!(machine.games[&7].cover_file, None);
}

#[test]
fn owned_only_and_family_shared_games_are_looked_up() {
    let mut cache = LibraryCache::default();
    cache
        .entries
        .push(crate::library::machine::tests::stored_with(
            "steam",
            "70",
            "Half-Life",
            false,
            true,
            false,
        ));
    cache
        .entries
        .push(crate::library::machine::tests::stored_with(
            "steam", "400", "Portal", false, false, true,
        ));
    let (mut machine, _) = loaded(false, cache);
    machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: NOW,
    });
    let op = machine.scan.as_ref().unwrap().op;
    let effects = machine.handle(Input::ProviderScanned {
        op,
        provider: "steam".to_owned(),
        result: Ok(vec![]),
        now: NOW,
    });
    let asked = lookups(&effects);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].2, ["400", "70"]);
}

#[test]
fn a_catalog_sync_looks_up_newly_visible_games() {
    use crate::library::store::{CatalogError, CatalogRecord};

    let (mut machine, _) = loaded(false, LibraryCache::default());
    machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: NOW,
    });
    finish_scan(&mut machine, vec![], NOW);
    let op = match machine.handle(Input::RequestCatalog).as_slice() {
        [Effect::SyncCatalog { op, provider }] if provider == "steam" => *op,
        other => panic!("{other:?}"),
    };
    let effects = machine.handle(Input::CatalogSynced {
        op,
        provider: "steam".to_owned(),
        owned: Some(Ok(vec![CatalogRecord {
            id: "70".to_owned(),
            name: "Half-Life".to_owned(),
            fields: Default::default(),
        }])),
        family: Some(Err(CatalogError::FamilyUnavailable)),
        now: NOW + 1,
    });
    assert_eq!(lookups(&effects)[0].2, ["70"]);
}

const NAMESPACE_MANIFEST: &str = r#"
id = "fixture"
name = "Fixture"
platforms = ["windows", "linux"]
source = "fixture"
scan = { native = "manual" }
match = { external_category = 1001, uid_field = "namespace" }
"#;

fn namespaced(id: &str, namespace: &str) -> Record {
    let mut record = record(id, id, "/games/fixture");
    record
        .fields
        .insert("namespace".to_owned(), namespace.to_owned());
    record
}

fn fixture_providers() -> Vec<crate::provider::Provider> {
    let loaded = registry::tests::load(
        &[
            ("steam.toml", STEAM_MANIFEST),
            ("fixture.toml", NAMESPACE_MANIFEST),
        ],
        Platform::Linux,
    );
    assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
    loaded.providers
}

#[test]
fn a_declared_field_is_the_lookup_uid_and_covers_every_entry_with_it() {
    let (mut machine, _) = loaded_with(fixture_providers(), LibraryCache::default());
    machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: NOW,
    });
    let op = machine.scan.as_ref().expect("scan running").op;

    let effects = machine.handle(Input::ProviderScanned {
        op,
        provider: "steam".to_owned(),
        result: Ok(vec![]),
        now: NOW,
    });
    assert!(lookups(&effects).is_empty(), "no steam entry to ask about");
    let effects = machine.handle(Input::ProviderScanned {
        op,
        provider: "fixture".to_owned(),
        result: Ok(vec![
            namespaced("app1", "ns1"),
            namespaced("app2", "ns1"),
            record("app3", "No namespace", "/games/fixture"),
        ]),
        now: NOW,
    });

    let mut asked = lookups(&effects);
    assert_eq!(asked.len(), 1, "{effects:?}");
    let ask = asked.remove(0);
    assert_eq!(
        (ask.1, ask.2.as_slice()),
        (1001, &["ns1".to_owned()][..]),
        "the field is the uid and one namespace is asked about once"
    );

    let effects = done(
        &mut machine,
        &ask,
        Ok(vec![found("ns1", summary(42, "Shared game"))]),
        NOW + 1,
    );
    let (_, matches) = persisted(&effects).expect("the answer is persisted");
    assert_eq!(matches.len(), 1);
    assert_eq!(
        (matches[0].uid.as_str(), matches[0].game_ids.as_slice()),
        ("ns1", &[42][..])
    );

    let matched = MatchStatus::Matched {
        game_id: 42,
        source: MatchSource::Automatic,
    };
    assert_eq!(
        machine.match_status(&machine.entries["fixture:app1"]),
        matched
    );
    assert_eq!(
        machine.match_status(&machine.entries["fixture:app2"]),
        matched
    );
    assert_eq!(
        machine.match_status(&machine.entries["fixture:app3"]),
        MatchStatus::Unmatched,
        "an entry without the declared field is never looked up"
    );
    assert!(
        machine.entries["fixture:app3"].fields.is_empty(),
        "the missing field is not invented"
    );
}

#[test]
fn an_entry_without_the_declared_field_is_skipped() {
    let (mut machine, _) = loaded_with(fixture_providers(), LibraryCache::default());
    machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: NOW,
    });
    let op = machine.scan.as_ref().expect("scan running").op;
    machine.handle(Input::ProviderScanned {
        op,
        provider: "steam".to_owned(),
        result: Ok(vec![]),
        now: NOW,
    });
    let effects = machine.handle(Input::ProviderScanned {
        op,
        provider: "fixture".to_owned(),
        result: Ok(vec![record("app3", "No namespace", "/games/fixture")]),
        now: NOW,
    });
    assert!(
        lookups(&effects).is_empty(),
        "a declared field that the entry lacks cannot be looked up"
    );
}

#[test]
fn a_second_entry_with_a_known_namespace_does_not_ask_again() {
    let (mut machine, _) = loaded_with(fixture_providers(), LibraryCache::default());
    machine.handle(Input::SetAccess {
        access: Access::Verified,
        now: NOW,
    });
    let op = machine.scan.as_ref().expect("scan running").op;
    machine.handle(Input::ProviderScanned {
        op,
        provider: "steam".to_owned(),
        result: Ok(vec![]),
        now: NOW,
    });
    let effects = machine.handle(Input::ProviderScanned {
        op,
        provider: "fixture".to_owned(),
        result: Ok(vec![namespaced("app1", "ns1")]),
        now: NOW,
    });
    let ask = lookups(&effects).remove(0);
    done(&mut machine, &ask, Ok(vec![]), NOW + 1);

    machine.handle(Input::ScanRequested);
    let op = machine.scan.as_ref().expect("scan running").op;
    machine.handle(Input::ProviderScanned {
        op,
        provider: "steam".to_owned(),
        result: Ok(vec![]),
        now: NOW + 2,
    });
    let effects = machine.handle(Input::ProviderScanned {
        op,
        provider: "fixture".to_owned(),
        result: Ok(vec![namespaced("app1", "ns1"), namespaced("app2", "ns1")]),
        now: NOW + 2,
    });
    assert!(
        lookups(&effects).is_empty(),
        "the cached namespace answer already covers the second entry"
    );
    assert_eq!(
        machine.match_status(&machine.entries["fixture:app2"]),
        MatchStatus::Unmatched,
        "an empty cached answer applies to every entry with that namespace"
    );
}
