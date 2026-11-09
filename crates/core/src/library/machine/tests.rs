use super::*;
use crate::library::store::{CatalogError, CatalogRecord};
use crate::provider::registry::{self, tests::STEAM_MANIFEST};
use crate::provider::{Platform, Record};

pub(crate) const SHOP_MANIFEST: &str = r#"
id = "shop"
name = "Shop"
platforms = ["windows", "linux"]
source = "shop"
scan = { native = "manual" }
"#;

pub(crate) fn providers(with_shop: bool) -> Vec<Provider> {
    let mut sources = vec![("steam.toml", STEAM_MANIFEST)];
    if with_shop {
        sources.push(("shop.toml", SHOP_MANIFEST));
    }
    let loaded = registry::tests::load(&sources, Platform::Linux);
    assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
    loaded.providers
}

pub(crate) fn account(id: &str) -> LastAccount {
    LastAccount {
        id: id.to_owned(),
        name: format!("User {id}"),
    }
}

pub(crate) fn record(id: &str, name: &str, dir: &str) -> Record {
    Record {
        id: id.to_owned(),
        name: Some(name.to_owned()),
        install_dir: dir.to_owned(),
        fields: Default::default(),
    }
}

pub(crate) fn stored(provider: &str, id: &str, name: &str, installed: bool) -> StoredEntry {
    stored_with(provider, id, name, installed, false, false)
}

pub(crate) fn stored_with(
    provider: &str,
    id: &str,
    name: &str,
    installed: bool,
    owned: bool,
    family_shared: bool,
) -> StoredEntry {
    StoredEntry {
        key: entry_key(provider, id),
        provider: provider.to_owned(),
        store_id: id.to_owned(),
        name: Some(name.to_owned()),
        install_dir: format!("/games/{name}"),
        fields: Default::default(),
        installed,
        owned,
        family_shared,
        first_seen: 100,
        last_seen: 100,
    }
}

/// The op of the first `LoadCache` in `effects`.
fn load_op(effects: &[Effect]) -> u64 {
    effects
        .iter()
        .find_map(|effect| match effect {
            Effect::LoadCache { op, .. } => Some(*op),
            _ => None,
        })
        .expect("a LoadCache effect")
}

pub(crate) fn scan_ops(effects: &[Effect]) -> Vec<(u64, String)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::RunScan { op, provider } => Some((*op, provider.clone())),
            _ => None,
        })
        .collect()
}

pub(crate) fn catalog_ops(effects: &[Effect]) -> Vec<(u64, String)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::SyncCatalog { op, provider } => Some((*op, provider.clone())),
            _ => None,
        })
        .collect()
}

pub(crate) fn catalog_record(id: &str, name: &str) -> CatalogRecord {
    CatalogRecord {
        id: id.to_owned(),
        name: name.to_owned(),
    }
}

/// A machine for account "a" whose cache has been loaded, with the startup scan still running.
pub(crate) fn loaded(with_shop: bool, cache: LibraryCache) -> (LibraryMachine, Vec<Effect>) {
    let mut machine = LibraryMachine::new(providers(with_shop), LibraryLayout::default());
    let effects = machine.handle(Input::AccountChanged(Some(account("a"))));
    let op = load_op(&effects);
    let effects = machine.handle(Input::Loaded {
        op,
        result: Ok(cache),
    });
    (machine, effects)
}

/// Delivers one provider's scan result for the scan with `op`.
pub(crate) fn scanned(
    machine: &mut LibraryMachine,
    op: u64,
    provider: &str,
    result: Result<Vec<Record>, &str>,
    now: i64,
) -> Vec<Effect> {
    machine.handle(Input::ProviderScanned {
        op,
        provider: provider.to_owned(),
        result: result.map_err(str::to_owned),
        now,
    })
}

/// Runs a whole steam-only scan to completion.
pub(crate) fn finish_scan(machine: &mut LibraryMachine, records: Vec<Record>, now: i64) {
    let op = machine.scan.as_ref().expect("a scan is running").op;
    scanned(machine, op, "steam", Ok(records), now);
}

#[test]
fn an_account_loads_its_cache_and_then_scans() {
    let mut machine = LibraryMachine::new(providers(false), LibraryLayout::default());
    assert!(!machine.is_active());

    let effects = machine.handle(Input::AccountChanged(Some(account("a"))));
    assert_eq!(
        effects,
        [Effect::LoadCache {
            op: 1,
            account_id: Some("a".to_owned())
        }]
    );
    assert!(machine.is_active() && !machine.scan_running());

    // A scan request while the cache is loading is covered by the scan that follows the load.
    assert!(machine.handle(Input::ScanRequested).is_empty());

    let effects = machine.handle(Input::Loaded {
        op: 1,
        result: Ok(LibraryCache::default()),
    });
    assert_eq!(scan_ops(&effects), [(2, "steam".to_owned())]);
    assert!(machine.scan_running());
}

#[test]
fn an_unreadable_cache_still_scans() {
    let mut machine = LibraryMachine::new(providers(false), LibraryLayout::default());
    machine.handle(Input::AccountChanged(Some(account("a"))));
    let effects = machine.handle(Input::Loaded {
        op: 1,
        result: Err(()),
    });
    assert_eq!(scan_ops(&effects).len(), 1);
}

#[test]
fn repeated_rescan_requests_cause_at_most_one_more_scan() {
    let (mut machine, effects) = loaded(false, LibraryCache::default());
    let (first_op, _) = scan_ops(&effects)[0].clone();

    for _ in 0..5 {
        assert!(machine.handle(Input::ScanRequested).is_empty());
    }
    assert!(machine.scan_running());

    // The running scan ends: exactly one follow-up scan starts.
    let effects = scanned(&mut machine, first_op, "steam", Ok(vec![]), 10);
    let follow_ups = scan_ops(&effects);
    assert_eq!(follow_ups.len(), 1);
    assert!(follow_ups[0].0 > first_op);
    assert!(machine.scan_running());

    // That one ends with nothing queued behind it.
    let effects = scanned(&mut machine, follow_ups[0].0, "steam", Ok(vec![]), 11);
    assert!(scan_ops(&effects).is_empty());
    assert!(!machine.scan_running());

    // And an idle request starts a scan straight away.
    assert_eq!(scan_ops(&machine.handle(Input::ScanRequested)).len(), 1);
}

#[test]
fn results_of_an_older_scan_or_for_an_unknown_provider_are_ignored() {
    let (mut machine, effects) = loaded(false, LibraryCache::default());
    let (op, _) = scan_ops(&effects)[0].clone();

    assert!(
        scanned(
            &mut machine,
            op + 7,
            "steam",
            Ok(vec![record("1", "A", "/a")]),
            5
        )
        .is_empty()
    );
    assert!(
        scanned(
            &mut machine,
            op,
            "nope",
            Ok(vec![record("1", "A", "/a")]),
            5
        )
        .is_empty()
    );
    assert_eq!(machine.installed_entries().count(), 0);
    assert!(machine.scan_running());

    // A result is taken once: a repeated delivery changes nothing.
    scanned(
        &mut machine,
        op,
        "steam",
        Ok(vec![record("1", "A", "/a")]),
        5,
    );
    assert!(scanned(&mut machine, op, "steam", Ok(vec![]), 6).is_empty());
    assert_eq!(machine.installed_entries().count(), 1);
}

#[test]
fn a_scan_adds_entries_and_persists_the_outcome() {
    let (mut machine, effects) = loaded(false, LibraryCache::default());
    let (op, _) = scan_ops(&effects)[0].clone();
    let records = vec![record("620", "Portal 2", "/g/Portal 2")];

    let effects = scanned(&mut machine, op, "steam", Ok(records.clone()), 50);
    assert_eq!(
        effects,
        [Effect::PersistScan {
            provider: "steam".to_owned(),
            outcome: ScanOutcome::Scanned(records),
            now: 50
        }]
    );
    let entry = machine.entry("steam:620").unwrap();
    assert!(entry.installed);
    assert_eq!((entry.first_seen, entry.last_seen), (50, 50));
    assert_eq!(
        machine.scan_state("steam").unwrap().last_success_at,
        Some(50)
    );
}

#[test]
fn an_uninstalled_game_leaves_the_view_but_is_kept() {
    let (mut machine, _) = loaded(false, LibraryCache::default());
    finish_scan(
        &mut machine,
        vec![record("1", "One", "/1"), record("2", "Two", "/2")],
        10,
    );
    machine.handle(Input::ScanRequested);
    finish_scan(&mut machine, vec![record("1", "One", "/1")], 20);

    let installed: Vec<_> = machine
        .installed_entries()
        .map(|e| e.key.as_str())
        .collect();
    assert_eq!(installed, ["steam:1"]);
    let gone = machine.entry("steam:2").expect("never deleted");
    assert!(!gone.installed);
    assert_eq!(gone.last_seen, 10);
}

#[test]
fn a_reinstalled_game_returns_with_its_match_and_cover() {
    let choice = ChosenMatch {
        entry_key: "steam:2".to_owned(),
        game_id: Some(42),
        chosen_at: 5,
    };
    let game = GameRecord {
        game_id: 42,
        name: "Two".to_owned(),
        release_year: Some(2011),
        cover_url: Some("https://images.igdb.com/igdb/image/upload/t_cover_big/x.jpg".to_owned()),
        cover_file: Some("42.jpg".to_owned()),
        cover_failed_at: None,
    };
    let cache = LibraryCache {
        entries: vec![stored("steam", "2", "Two", false)],
        chosen_matches: vec![choice.clone()],
        games: vec![game.clone()],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    assert_eq!(
        machine.installed_entries().count(),
        0,
        "uninstalled in the cache"
    );

    finish_scan(&mut machine, vec![record("2", "Two", "/new/Two")], 30);

    let entry = machine.entry("steam:2").unwrap();
    assert!(entry.installed);
    assert_eq!(
        entry.first_seen, 100,
        "the entry is the same one, not a new one"
    );
    assert_eq!(machine.chosen_match("steam:2"), Some(&choice));
    assert_eq!(machine.game(42), Some(&game));
}

#[test]
fn a_moved_install_folder_keeps_the_entry() {
    let cache = LibraryCache {
        entries: vec![stored("steam", "620", "Portal 2", true)],
        chosen_matches: vec![ChosenMatch {
            entry_key: "steam:620".to_owned(),
            game_id: Some(7),
            chosen_at: 1,
        }],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    finish_scan(
        &mut machine,
        vec![record("620", "Portal 2", "/mnt/new/Portal 2")],
        60,
    );

    assert_eq!(machine.installed_entries().count(), 1, "not duplicated");
    let entry = machine.entry("steam:620").unwrap();
    assert_eq!(entry.install_dir, "/mnt/new/Portal 2");
    assert_eq!(entry.first_seen, 100);
    assert_eq!(entry.last_seen, 60);
    assert_eq!(machine.chosen_match("steam:620").unwrap().game_id, Some(7));
}

#[test]
fn a_failed_provider_keeps_its_entries_and_the_others_still_update() {
    let cache = LibraryCache {
        entries: vec![
            stored("steam", "1", "One", true),
            stored("shop", "9", "Nine", true),
        ],
        ..LibraryCache::default()
    };
    let (mut machine, effects) = loaded(true, cache);
    let ops = scan_ops(&effects);
    assert_eq!(ops.len(), 2);
    let op = ops[0].0;

    let failed = scanned(&mut machine, op, "steam", Err("cannot read roots"), 70);
    assert_eq!(
        failed,
        [Effect::PersistScan {
            provider: "steam".to_owned(),
            outcome: ScanOutcome::Failed("cannot read roots".to_owned()),
            now: 70
        }]
    );
    assert!(
        machine.scan_running(),
        "the other provider is still scanning"
    );
    scanned(&mut machine, op, "shop", Ok(vec![]), 70);
    assert!(!machine.scan_running());

    // Steam kept its game and shows the failure; the shop's game is gone from the view.
    let installed: Vec<_> = machine
        .installed_entries()
        .map(|e| e.key.as_str())
        .collect();
    assert_eq!(installed, ["steam:1"]);
    let steam = machine.scan_state("steam").unwrap();
    assert_eq!(steam.last_error.as_deref(), Some("cannot read roots"));
    assert_eq!(steam.last_success_at, None);
    assert_eq!(machine.scan_state("shop").unwrap().last_error, None);

    // A later good scan clears the error.
    machine.handle(Input::ScanRequested);
    let op = machine.scan.as_ref().unwrap().op;
    scanned(
        &mut machine,
        op,
        "steam",
        Ok(vec![record("1", "One", "/1")]),
        80,
    );
    assert_eq!(machine.scan_state("steam").unwrap().last_error, None);
}

#[test]
fn changing_or_clearing_the_account_drops_late_results() {
    let (mut machine, effects) = loaded(false, LibraryCache::default());
    let (scan_op, _) = scan_ops(&effects)[0].clone();

    // Signed out: everything is dropped and a late scan result is ignored.
    assert!(machine.handle(Input::AccountChanged(None)).is_empty());
    assert!(!machine.is_active());
    assert!(
        scanned(
            &mut machine,
            scan_op,
            "steam",
            Ok(vec![record("1", "One", "/1")]),
            9
        )
        .is_empty()
    );
    assert!(machine.entry("steam:1").is_none());

    // Another account loads its own chosen matches; the old account's load result is ignored.
    let first = load_op(&machine.handle(Input::AccountChanged(Some(account("b")))));
    let second = load_op(&machine.handle(Input::AccountChanged(Some(account("c")))));
    assert!(
        machine
            .handle(Input::Loaded {
                op: first,
                result: Ok(LibraryCache::default())
            })
            .is_empty()
    );
    let effects = machine.handle(Input::Loaded {
        op: second,
        result: Ok(LibraryCache::default()),
    });
    assert_eq!(scan_ops(&effects).len(), 1);
    assert_eq!(machine.account().unwrap().id, "c");
}

#[test]
fn the_same_account_again_does_nothing() {
    let (mut machine, _) = loaded(false, LibraryCache::default());
    assert!(
        machine
            .handle(Input::AccountChanged(Some(account("a"))))
            .is_empty()
    );
    assert!(machine.scan_running(), "the running scan was not disturbed");
}

fn artwork_machine() -> LibraryMachine {
    let cache = LibraryCache {
        entries: vec![
            stored("steam", "1", "One", true),
            stored("steam", "2", "Two", true),
        ],
        chosen_matches: vec![
            ChosenMatch {
                entry_key: "steam:1".into(),
                game_id: Some(7),
                chosen_at: 0,
            },
            ChosenMatch {
                entry_key: "steam:2".into(),
                game_id: Some(8),
                chosen_at: 0,
            },
        ],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    machine.access = Access::Verified;
    machine.layout.detail_open = true;
    machine
}

fn select_artwork(machine: &mut LibraryMachine, key: &str) -> Vec<Effect> {
    machine.handle(Input::Select {
        key: key.into(),
        mode: SelectMode::Replace,
    })
}

fn artwork_op(effects: &[Effect]) -> u64 {
    effects
        .iter()
        .find_map(|effect| match effect {
            Effect::FetchArtwork { op, .. } => Some(*op),
            _ => None,
        })
        .expect("artwork request")
}

#[test]
fn artwork_downloads_are_deduplicated_and_late_results_are_cached() {
    let mut machine = artwork_machine();
    let op = artwork_op(&select_artwork(&mut machine, "steam:1"));
    select_artwork(&mut machine, "steam:2");
    assert!(
        !select_artwork(&mut machine, "steam:1")
            .iter()
            .any(|e| matches!(e, Effect::FetchArtwork { .. }))
    );
    select_artwork(&mut machine, "steam:2");
    machine.handle(Input::ArtworkDone {
        op,
        game_id: 7,
        result: Ok(Some(vec![1, 2, 3])),
    });
    assert!(
        machine.artwork.is_none(),
        "a late answer must not replace the selected image"
    );
    assert!(
        !select_artwork(&mut machine, "steam:1")
            .iter()
            .any(|e| matches!(e, Effect::FetchArtwork { .. }))
    );
    assert_eq!(machine.artwork.as_deref(), Some([1, 2, 3].as_slice()));
    machine.reset_data();
    machine.on_artwork_done(op, 7, Ok(Some(vec![4])));
    assert!(
        machine.artwork_cache.is_empty(),
        "answers from the previous account are ignored"
    );
}

#[test]
fn artwork_failures_are_retried_on_reselection_and_hidden_panes_do_not_fetch() {
    let mut machine = artwork_machine();
    machine.layout.detail_open = false;
    assert!(
        select_artwork(&mut machine, "steam:1")
            .iter()
            .all(|e| !matches!(e, Effect::FetchArtwork { .. }))
    );
    let op = artwork_op(&machine.handle(Input::SetDetailOpen(true)));
    machine.handle(Input::ArtworkDone {
        op,
        game_id: 7,
        result: Err(ApiError::Failed),
    });
    select_artwork(&mut machine, "steam:2");
    artwork_op(&select_artwork(&mut machine, "steam:1"));
}

#[test]
fn temporary_removal_hides_entry_and_prunes_selection_until_next_scan() {
    let cache = LibraryCache {
        entries: vec![
            stored("steam", "1", "One", true),
            stored("steam", "2", "Two", true),
        ],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    machine.handle(Input::Select {
        key: "steam:1".into(),
        mode: SelectMode::Replace,
    });
    assert_eq!(machine.selected_keys(), ["steam:1"]);

    let effects = machine.handle(Input::RemoveEntry {
        key: "steam:1".into(),
        allow_rediscovery: true,
    });
    assert!(effects.is_empty());
    assert!(machine.selected_keys().is_empty());
    let installed: Vec<_> = machine
        .installed_entries()
        .map(|e| e.key.as_str())
        .collect();
    assert_eq!(installed, ["steam:2"]);
    assert_eq!(machine.all_entries().count(), 2);

    // Next scan brings it back.
    finish_scan(
        &mut machine,
        vec![record("1", "One", "/1"), record("2", "Two", "/2")],
        200,
    );
    let installed: Vec<_> = machine
        .installed_entries()
        .map(|e| e.key.as_str())
        .collect();
    assert_eq!(installed, ["steam:1", "steam:2"]);
}

#[test]
fn permanent_removal_persists_exclusion_across_scans_and_reset_restores() {
    let cache = LibraryCache {
        entries: vec![
            stored("steam", "1", "One", true),
            stored("steam", "2", "Two", true),
        ],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    machine.handle(Input::Select {
        key: "steam:1".into(),
        mode: SelectMode::Replace,
    });

    let effects = machine.handle(Input::RemoveEntry {
        key: "steam:1".into(),
        allow_rediscovery: false,
    });
    assert_eq!(
        effects,
        [Effect::PersistExcluded(vec!["steam:1".to_owned()])]
    );
    assert!(machine.selected_keys().is_empty());
    let installed: Vec<_> = machine
        .installed_entries()
        .map(|e| e.key.as_str())
        .collect();
    assert_eq!(installed, ["steam:2"]);

    // Subsequent scan still suppresses it.
    finish_scan(
        &mut machine,
        vec![record("1", "One", "/1"), record("2", "Two", "/2")],
        200,
    );
    let installed: Vec<_> = machine
        .installed_entries()
        .map(|e| e.key.as_str())
        .collect();
    assert_eq!(installed, ["steam:2"]);

    // Loading a cache with excluded entries suppresses them too.
    let (reloaded, _) = loaded(
        false,
        LibraryCache {
            entries: vec![
                stored("steam", "1", "One", true),
                stored("steam", "2", "Two", true),
            ],
            excluded: vec!["steam:1".to_owned()],
            ..LibraryCache::default()
        },
    );
    let installed: Vec<_> = reloaded
        .installed_entries()
        .map(|e| e.key.as_str())
        .collect();
    assert_eq!(installed, ["steam:2"]);

    // Resetting exclusions restores the entry immediately and persists the empty list.
    let effects = machine.handle(Input::ResetExcluded);
    assert_eq!(effects, [Effect::PersistExcluded(vec![])]);
    let installed: Vec<_> = machine
        .installed_entries()
        .map(|e| e.key.as_str())
        .collect();
    assert_eq!(installed, ["steam:1", "steam:2"]);
    assert!(machine.handle(Input::ResetExcluded).is_empty());
}

fn ready(cache: LibraryCache) -> LibraryMachine {
    let (mut machine, _) = loaded(false, cache);
    finish_scan(&mut machine, vec![], 1);
    machine
}

fn request_catalog(machine: &mut LibraryMachine) -> (u64, String) {
    let effects = machine.handle(Input::RequestCatalog);
    let ops = catalog_ops(&effects);
    assert_eq!(ops.len(), 1, "{effects:?}");
    ops[0].clone()
}

fn catalog_synced(
    machine: &mut LibraryMachine,
    op: u64,
    owned: Option<Result<Vec<CatalogRecord>, CatalogError>>,
    family: Option<Result<Vec<CatalogRecord>, CatalogError>>,
    now: i64,
) -> Vec<Effect> {
    machine.handle(Input::CatalogSynced {
        op,
        provider: "steam".to_owned(),
        owned,
        family,
        now,
    })
}

#[test]
fn a_catalog_sync_adds_owned_only_entries() {
    let mut machine = ready(LibraryCache::default());
    let (op, provider) = request_catalog(&mut machine);
    assert_eq!(provider, "steam");

    let effects = catalog_synced(
        &mut machine,
        op,
        Some(Ok(vec![catalog_record("620", "Portal 2")])),
        None,
        10,
    );
    assert!(
        effects
            .iter()
            .all(|effect| !matches!(effect, Effect::SyncCatalog { .. }))
    );

    let entry = machine.entry("steam:620").expect("a catalog entry exists");
    assert!(entry.owned && !entry.installed && !entry.family_shared);
    assert_eq!(entry.install_dir, "");
    assert_eq!(entry.name.as_deref(), Some("Portal 2"));
    assert_eq!((entry.first_seen, entry.last_seen), (10, 10));
    assert_eq!(
        machine
            .visible_entries()
            .map(|e| e.key.as_str())
            .collect::<Vec<_>>(),
        ["steam:620"]
    );
    let state = machine.scan_state("steam").unwrap();
    assert_eq!(state.last_catalog_at, Some(10));
    assert_eq!(state.catalog_error, None);
    assert_eq!(state.last_family_at, None);
}

#[test]
fn an_owned_sync_keeps_installed_facts_and_the_installed_name() {
    let cache = LibraryCache {
        entries: vec![stored("steam", "620", "Portal 2", true)],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    finish_scan(&mut machine, vec![record("620", "Portal 2", "/p")], 5);
    let (op, _) = request_catalog(&mut machine);
    catalog_synced(
        &mut machine,
        op,
        Some(Ok(vec![catalog_record("620", "Portal 2 (catalog)")])),
        None,
        20,
    );

    let entry = machine.entry("steam:620").unwrap();
    assert!(entry.installed && entry.owned);
    assert_eq!(entry.install_dir, "/p");
    assert_eq!(entry.name.as_deref(), Some("Portal 2"));
    assert_eq!(entry.first_seen, 100);
    assert_eq!(entry.last_seen, 20);
}

#[test]
fn a_family_sync_marks_a_non_owned_entry_shared() {
    let mut machine = ready(LibraryCache::default());
    let (op, _) = request_catalog(&mut machine);
    catalog_synced(
        &mut machine,
        op,
        None,
        Some(Ok(vec![catalog_record("70", "Half-Life")])),
        10,
    );

    let entry = machine.entry("steam:70").unwrap();
    assert!(entry.family_shared && !entry.owned && !entry.installed);
    let state = machine.scan_state("steam").unwrap();
    assert_eq!(state.last_family_at, Some(10));
    assert_eq!(state.family_error, None);
    assert_eq!(state.last_catalog_at, None);
}

#[test]
fn owned_wins_when_the_same_app_is_also_shared() {
    let mut machine = ready(LibraryCache::default());
    let (op, _) = request_catalog(&mut machine);
    catalog_synced(
        &mut machine,
        op,
        Some(Ok(vec![catalog_record("620", "Portal 2")])),
        Some(Ok(vec![catalog_record("620", "Portal 2")])),
        10,
    );

    let entry = machine.entry("steam:620").unwrap();
    assert!(entry.owned);
    assert!(!entry.family_shared);
    let row = &machine.visible_rows("")[0];
    assert!(row.owned && !row.family_shared);
    assert_eq!(
        machine.scan_state("steam").unwrap().last_family_at,
        Some(10)
    );
}

#[test]
fn a_failed_owned_sync_keeps_the_previous_owned_facts() {
    let mut machine = ready(LibraryCache::default());
    let (op, _) = request_catalog(&mut machine);
    catalog_synced(
        &mut machine,
        op,
        Some(Ok(vec![catalog_record("620", "Portal 2")])),
        None,
        10,
    );

    let (op, _) = request_catalog(&mut machine);
    catalog_synced(
        &mut machine,
        op,
        Some(Err(CatalogError::Unreachable)),
        None,
        30,
    );

    let entry = machine.entry("steam:620").unwrap();
    assert!(entry.owned, "the previous facts are kept");
    let state = machine.scan_state("steam").unwrap();
    assert_eq!(state.catalog_error, Some(CatalogError::Unreachable));
    assert_eq!(
        state.last_catalog_at,
        Some(10),
        "the timestamp does not advance"
    );
}

#[test]
fn a_failed_family_sync_keeps_family_and_does_not_fail_owned() {
    let mut machine = ready(LibraryCache::default());
    let (op, _) = request_catalog(&mut machine);
    catalog_synced(
        &mut machine,
        op,
        None,
        Some(Ok(vec![catalog_record("70", "Half-Life")])),
        10,
    );

    let (op, _) = request_catalog(&mut machine);
    catalog_synced(
        &mut machine,
        op,
        Some(Ok(vec![catalog_record("620", "Portal 2")])),
        Some(Err(CatalogError::FamilyUnavailable)),
        30,
    );

    assert!(machine.entry("steam:70").unwrap().family_shared);
    assert!(machine.entry("steam:620").unwrap().owned);
    let state = machine.scan_state("steam").unwrap();
    assert_eq!(state.family_error, Some(CatalogError::FamilyUnavailable));
    assert_eq!(state.last_family_at, Some(10));
    assert_eq!(state.last_catalog_at, Some(30));
    assert_eq!(state.catalog_error, None);
}

#[test]
fn clearing_the_catalog_keeps_entries_matches_and_covers() {
    let choice = ChosenMatch {
        entry_key: "steam:70".to_owned(),
        game_id: Some(7),
        chosen_at: 3,
    };
    let game = GameRecord {
        game_id: 7,
        name: "Half-Life".to_owned(),
        release_year: Some(1998),
        cover_url: None,
        cover_file: Some("7.jpg".to_owned()),
        cover_failed_at: None,
    };
    let cache = LibraryCache {
        entries: vec![
            stored("steam", "620", "Portal 2", true),
            stored_with("steam", "70", "Half-Life", false, true, false),
        ],
        chosen_matches: vec![choice.clone()],
        games: vec![game.clone()],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    finish_scan(&mut machine, vec![record("620", "Portal 2", "/p")], 5);
    let (op, _) = request_catalog(&mut machine);
    catalog_synced(
        &mut machine,
        op,
        Some(Ok(vec![catalog_record("70", "Half-Life")])),
        Some(Ok(vec![catalog_record("70", "Half-Life")])),
        10,
    );
    assert!(
        machine
            .handle(Input::CatalogCleared {
                provider: "steam".to_owned()
            })
            .is_empty()
    );

    let kept = machine
        .entry("steam:70")
        .expect("entries are never deleted");
    assert!(!kept.owned && !kept.family_shared);
    assert_eq!(machine.chosen_match("steam:70"), Some(&choice));
    assert_eq!(machine.game(7), Some(&game));
    assert!(machine.entry("steam:620").unwrap().installed);
    assert_eq!(
        machine
            .visible_entries()
            .map(|e| e.key.as_str())
            .collect::<Vec<_>>(),
        ["steam:620"]
    );
    let state = machine.scan_state("steam").unwrap();
    assert_eq!(
        (
            state.last_catalog_at,
            state.catalog_error,
            state.last_family_at,
            state.family_error
        ),
        (None, None, None, None)
    );
}

#[test]
fn a_late_or_superseded_catalog_result_is_ignored() {
    let mut machine = ready(LibraryCache::default());
    let (first, _) = request_catalog(&mut machine);
    let (second, _) = request_catalog(&mut machine);
    assert_ne!(first, second);

    catalog_synced(
        &mut machine,
        first,
        Some(Ok(vec![catalog_record("1", "Old")])),
        None,
        10,
    );
    assert!(machine.entry("steam:1").is_none());

    catalog_synced(
        &mut machine,
        second,
        Some(Ok(vec![catalog_record("2", "New")])),
        None,
        11,
    );
    assert!(machine.entry("steam:2").is_some());

    // Signing out after a request makes the answer that arrives later a no-op.
    let (pending, _) = request_catalog(&mut machine);
    machine.handle(Input::CatalogCleared {
        provider: "steam".to_owned(),
    });
    catalog_synced(
        &mut machine,
        pending,
        Some(Ok(vec![catalog_record("3", "Late")])),
        None,
        12,
    );
    assert!(machine.entry("steam:3").is_none());
}

#[test]
fn catalog_records_respect_exclusions_and_drop_unusable_names() {
    let mut machine = ready(LibraryCache::default());
    let (op, _) = request_catalog(&mut machine);
    catalog_synced(
        &mut machine,
        op,
        Some(Ok(vec![
            catalog_record("228980", "Steamworks Common Redistributables"),
            catalog_record("", "No id"),
            catalog_record("1", "   "),
            catalog_record("2", "Two"),
            catalog_record("2", "Two again"),
        ])),
        None,
        10,
    );

    let keys: Vec<_> = machine.visible_entries().map(|e| e.key.as_str()).collect();
    assert_eq!(keys, ["steam:2"]);
    assert_eq!(
        machine.entry("steam:2").unwrap().name.as_deref(),
        Some("Two"),
        "the first usable record wins"
    );
}

#[test]
fn a_scan_does_not_change_owned_or_family_facts() {
    let mut machine = ready(LibraryCache::default());
    let (op, _) = request_catalog(&mut machine);
    catalog_synced(
        &mut machine,
        op,
        Some(Ok(vec![catalog_record("620", "Portal 2")])),
        Some(Ok(vec![catalog_record("70", "Half-Life")])),
        10,
    );

    machine.handle(Input::ScanRequested);
    finish_scan(&mut machine, vec![record("620", "Portal 2", "/p")], 20);

    let installed = machine.entry("steam:620").unwrap();
    assert!(installed.installed && installed.owned);
    assert_eq!(installed.install_dir, "/p");
    let family = machine.entry("steam:70").unwrap();
    assert!(!family.installed && family.family_shared);
}

#[test]
fn a_scan_run_requests_a_catalog_sync_for_catalog_providers() {
    let (_, effects) = loaded(true, LibraryCache::default());
    let catalog = catalog_ops(&effects);
    assert_eq!(catalog.len(), 1, "{effects:?}");
    assert_eq!(
        catalog[0].1, "steam",
        "the shop provider declares no catalog"
    );

    let mut machine = LibraryMachine::new(providers(true), LibraryLayout::default());
    assert!(
        machine.handle(Input::RequestCatalog).is_empty(),
        "not active yet"
    );
}

#[test]
fn catalog_names_fill_empty_names_but_never_replace_known_ones() {
    let cache = LibraryCache {
        entries: vec![
            stored_with("steam", "1", "", false, true, false),
            stored_with("steam", "2", "Known", false, true, false),
            stored_with("steam", "3", "Installed", true, false, false),
        ],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    finish_scan(&mut machine, vec![record("3", "Installed", "/3")], 5);
    let (op, _) = request_catalog(&mut machine);
    catalog_synced(
        &mut machine,
        op,
        Some(Ok(vec![
            catalog_record("1", "Filled"),
            catalog_record("2", "Catalog name"),
            catalog_record("3", "Catalog name"),
        ])),
        None,
        10,
    );

    assert_eq!(
        machine.entry("steam:1").unwrap().name.as_deref(),
        Some("Filled")
    );
    assert_eq!(
        machine.entry("steam:2").unwrap().name.as_deref(),
        Some("Known")
    );
    assert_eq!(
        machine.entry("steam:3").unwrap().name.as_deref(),
        Some("Installed")
    );
}

#[test]
fn a_catalog_sync_leaves_installed_only_entries_alone() {
    let cache = LibraryCache {
        entries: vec![stored("steam", "620", "Portal 2", true)],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    finish_scan(&mut machine, vec![record("620", "Portal 2", "/p")], 5);
    let (op, _) = request_catalog(&mut machine);
    catalog_synced(
        &mut machine,
        op,
        Some(Ok(vec![catalog_record("70", "Half-Life")])),
        Some(Ok(vec![catalog_record("80", "Team Fortress 2")])),
        10,
    );

    let installed = machine.entry("steam:620").unwrap();
    assert!(installed.installed);
    assert!(!installed.owned && !installed.family_shared);
    assert_eq!(
        (installed.name.as_deref(), installed.install_dir.as_str()),
        (Some("Portal 2"), "/p")
    );
}

#[test]
fn a_family_sync_marks_an_installed_entry_shared_but_keeps_its_folder() {
    let cache = LibraryCache {
        entries: vec![stored("steam", "70", "Half-Life", true)],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    finish_scan(&mut machine, vec![record("70", "Half-Life", "/h")], 5);
    let (op, _) = request_catalog(&mut machine);
    catalog_synced(
        &mut machine,
        op,
        None,
        Some(Ok(vec![catalog_record("70", "Half-Life (shared)")])),
        10,
    );

    let entry = machine.entry("steam:70").unwrap();
    assert!(entry.installed && entry.family_shared && !entry.owned);
    assert_eq!(entry.install_dir, "/h");
    assert_eq!(entry.name.as_deref(), Some("Half-Life"));
    assert!(machine.visible_rows("")[0].family_shared);
}

#[test]
fn a_failed_owned_sync_does_not_block_a_family_sync() {
    let mut machine = ready(LibraryCache::default());
    let (op, _) = request_catalog(&mut machine);
    catalog_synced(
        &mut machine,
        op,
        Some(Ok(vec![catalog_record("1", "Owned")])),
        Some(Ok(vec![catalog_record("2", "Shared")])),
        10,
    );

    let (op, _) = request_catalog(&mut machine);
    catalog_synced(
        &mut machine,
        op,
        Some(Err(CatalogError::Server)),
        Some(Ok(vec![catalog_record("3", "Newly shared")])),
        20,
    );

    assert!(machine.entry("steam:1").unwrap().owned);
    assert!(!machine.entry("steam:2").unwrap().family_shared, "replaced");
    assert!(machine.entry("steam:3").unwrap().family_shared);
    let state = machine.scan_state("steam").unwrap();
    assert_eq!(state.catalog_error, Some(CatalogError::Server));
    assert_eq!(state.last_catalog_at, Some(10));
    assert_eq!(state.last_family_at, Some(20));
}
