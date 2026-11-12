use super::*;
use crate::db::{SETTING_KEYS, SqliteSettingsStore};
use gamenode_core::Settings;
use gamenode_core::library::{SidebarSelection, SortDirection, SortKey, ViewMode};
use gamenode_core::ports::SettingsStore;
use tempfile::TempDir;

fn open(dir: &TempDir) -> SqliteSettingsStore {
    SqliteSettingsStore::open(&dir.path().join("test.sqlite3")).unwrap()
}

fn record(id: &str, name: &str) -> Record {
    Record {
        id: id.to_owned(),
        name: Some(name.to_owned()),
        install_dir: format!("/games/{name}"),
        fields: BTreeMap::from([("state_flags".to_owned(), "4".to_owned())]),
    }
}

fn keys(cache: &LibraryCache) -> Vec<(&str, bool)> {
    cache
        .entries
        .iter()
        .map(|e| (e.key.as_str(), e.installed))
        .collect()
}

fn game(id: u64, cover: Option<&str>) -> GameRecord {
    GameRecord {
        game_id: id,
        name: format!("Game {id}"),
        release_year: Some(2011),
        cover_url: cover.map(str::to_owned),
        cover_file: None,
        cover_failed_at: None,
    }
}

#[test]
fn a_scan_is_saved_and_loaded_with_its_fields() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir).library_store();
    store
        .save_scan("steam", &[record("620", "Portal 2")], 100)
        .unwrap();

    let cache = store.load_cache(None).unwrap();
    assert_eq!(cache.entries.len(), 1);
    let entry = &cache.entries[0];
    assert_eq!(entry.key, "steam:620");
    assert_eq!(
        (entry.provider.as_str(), entry.store_id.as_str()),
        ("steam", "620")
    );
    assert_eq!(entry.name.as_deref(), Some("Portal 2"));
    assert_eq!(entry.install_dir, "/games/Portal 2");
    assert_eq!(entry.fields["state_flags"], "4");
    assert!(entry.installed);
    assert_eq!((entry.first_seen, entry.last_seen), (100, 100));
    assert_eq!(entry.record(), record("620", "Portal 2"));
    assert_eq!(
        cache.scans,
        [ProviderScanState {
            provider: "steam".into(),
            last_success_at: Some(100),
            last_error: None,
            last_catalog_at: None,
            catalog_error: None,
            last_shared_at: None,
            shared_error: None,
        }]
    );
}

#[test]
fn a_rescan_updates_entries_adds_new_ones_and_keeps_first_seen() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir).library_store();
    store
        .save_scan("steam", &[record("620", "Portal 2")], 100)
        .unwrap();

    let mut moved = record("620", "Portal 2");
    moved.install_dir = "/other/Portal 2".into();
    store
        .save_scan("steam", &[moved, record("570", "Dota 2")], 200)
        .unwrap();

    let cache = store.load_cache(None).unwrap();
    assert_eq!(keys(&cache), [("steam:570", true), ("steam:620", true)]);
    let portal = cache.entries.iter().find(|e| e.key == "steam:620").unwrap();
    assert_eq!(portal.install_dir, "/other/Portal 2");
    assert_eq!((portal.first_seen, portal.last_seen), (100, 200));
    assert_eq!(cache.scans[0].last_success_at, Some(200));
}

#[test]
fn an_uninstalled_game_is_kept_with_its_matches_and_returns_with_them() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir).library_store();
    store
        .save_scan("steam", &[record("620", "Portal 2")], 100)
        .unwrap();
    store
        .save_auto_matches(&[AutoMatch {
            category: 1,
            uid: "620".into(),
            game_ids: vec![72],
            checked_at: 100,
        }])
        .unwrap();
    store
        .save_chosen_match(
            "acct",
            &ChosenMatch {
                entry_key: "steam:620".into(),
                game_id: Some(72),
                chosen_at: 110,
            },
        )
        .unwrap();
    store
        .save_games(&[game(
            72,
            Some("//images.igdb.com/igdb/image/upload/t_thumb/co1.jpg"),
        )])
        .unwrap();
    store.save_cover_ready(72, "72.jpg").unwrap();

    // Steam no longer lists it.
    store.save_scan("steam", &[], 200).unwrap();
    let cache = store.load_cache(Some("acct")).unwrap();
    assert_eq!(keys(&cache), [("steam:620", false)]);
    assert_eq!(cache.auto_matches.len(), 1);
    assert_eq!(cache.chosen_matches.len(), 1);
    assert_eq!(cache.games[0].cover_file.as_deref(), Some("72.jpg"));

    // Reinstalled: same entry, same match and cover.
    store
        .save_scan("steam", &[record("620", "Portal 2")], 300)
        .unwrap();
    let cache = store.load_cache(Some("acct")).unwrap();
    assert_eq!(keys(&cache), [("steam:620", true)]);
    assert_eq!(cache.entries[0].first_seen, 100);
    assert_eq!(cache.chosen_matches[0].game_id, Some(72));
    assert_eq!(cache.games[0].cover_file.as_deref(), Some("72.jpg"));
}

#[test]
fn a_failed_scan_keeps_the_previous_entries_and_only_marks_that_provider() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir).library_store();
    store
        .save_scan("steam", &[record("620", "Portal 2")], 100)
        .unwrap();
    store
        .save_scan("gog", &[record("g1", "Witcher")], 100)
        .unwrap();

    store
        .save_scan_failure("steam", "the scan failed: boom")
        .unwrap();
    let cache = store.load_cache(None).unwrap();
    assert_eq!(keys(&cache), [("gog:g1", true), ("steam:620", true)]);
    let steam = cache.scans.iter().find(|s| s.provider == "steam").unwrap();
    assert_eq!(steam.last_success_at, Some(100), "the last success is kept");
    assert_eq!(steam.last_error.as_deref(), Some("the scan failed: boom"));
    let gog = cache.scans.iter().find(|s| s.provider == "gog").unwrap();
    assert_eq!(gog.last_error, None);

    store
        .save_scan("steam", &[record("620", "Portal 2")], 200)
        .unwrap();
    let steam = store
        .load_cache(None)
        .unwrap()
        .scans
        .into_iter()
        .find(|s| s.provider == "steam")
        .unwrap();
    assert_eq!((steam.last_success_at, steam.last_error), (Some(200), None));
}

#[test]
fn a_provider_that_never_succeeded_can_have_a_failure_recorded() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir).library_store();
    store.save_scan_failure("steam", "nope").unwrap();
    let cache = store.load_cache(None).unwrap();
    assert!(cache.entries.is_empty());
    assert_eq!(cache.scans[0].last_success_at, None);
}

#[test]
fn a_scan_that_fails_part_way_changes_nothing() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir).library_store();
    store
        .save_scan("steam", &[record("620", "Portal 2")], 100)
        .unwrap();
    // Make the second insert fail, after the first has already been written.
    store
        .conn()
        .execute_batch(
            "CREATE TRIGGER boom BEFORE INSERT ON library_entries WHEN NEW.store_id = 'bad'
             BEGIN SELECT RAISE(ABORT, 'boom'); END;",
        )
        .unwrap();
    let result = store.save_scan(
        "steam",
        &[record("570", "Dota 2"), record("bad", "Bad")],
        200,
    );
    assert!(result.is_err());

    let cache = store.load_cache(None).unwrap();
    assert_eq!(
        keys(&cache),
        [("steam:620", true)],
        "rolled back as one transaction"
    );
    assert_eq!(cache.entries[0].last_seen, 100);
    assert_eq!(cache.scans[0].last_success_at, Some(100));
}

#[test]
fn chosen_matches_are_separate_per_account() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir).library_store();
    let choice = |game_id| ChosenMatch {
        entry_key: "steam:620".into(),
        game_id,
        chosen_at: 5,
    };
    store.save_chosen_match("alice", &choice(Some(72))).unwrap();
    store.save_chosen_match("bob", &choice(None)).unwrap();

    assert_eq!(
        store.load_cache(Some("alice")).unwrap().chosen_matches,
        [choice(Some(72))]
    );
    assert_eq!(
        store.load_cache(Some("bob")).unwrap().chosen_matches,
        [choice(None)],
        "\"no game\" is a choice, distinct from none"
    );
    assert!(
        store
            .load_cache(Some("carol"))
            .unwrap()
            .chosen_matches
            .is_empty()
    );
    assert!(store.load_cache(None).unwrap().chosen_matches.is_empty());

    // Changing a choice replaces it for that account only.
    store
        .save_chosen_match(
            "alice",
            &ChosenMatch {
                chosen_at: 9,
                ..choice(Some(99))
            },
        )
        .unwrap();
    assert_eq!(
        store.load_cache(Some("alice")).unwrap().chosen_matches[0].game_id,
        Some(99)
    );
    assert_eq!(
        store.load_cache(Some("bob")).unwrap().chosen_matches[0].game_id,
        None
    );
}

#[test]
fn automatic_matches_are_replaced_and_unreadable_rows_are_dropped() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir).library_store();
    let auto = |ids: Vec<u64>, at| AutoMatch {
        category: 1,
        uid: "620".into(),
        game_ids: ids,
        checked_at: at,
    };
    store.save_auto_matches(&[auto(vec![], 1)]).unwrap();
    assert_eq!(
        store.load_cache(None).unwrap().auto_matches,
        [auto(vec![], 1)]
    );
    store.save_auto_matches(&[auto(vec![72, 73], 2)]).unwrap();
    assert_eq!(
        store.load_cache(None).unwrap().auto_matches,
        [auto(vec![72, 73], 2)]
    );

    store
        .conn()
        .execute("UPDATE auto_matches SET game_ids_json = 'garbage'", [])
        .unwrap();
    assert!(store.load_cache(None).unwrap().auto_matches.is_empty());
}

#[test]
fn game_metadata_keeps_cover_state_until_the_cover_url_changes() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir).library_store();
    let url = "//images.igdb.com/igdb/image/upload/t_thumb/co1.jpg";
    store.save_games(&[game(72, Some(url))]).unwrap();
    store.save_cover_failed(72, 50).unwrap();
    let cache = store.load_cache(None).unwrap();
    assert_eq!(cache.games[0].cover_failed_at, Some(50));
    assert_eq!(cache.games[0].cover_file, None);

    store.save_cover_ready(72, "72.jpg").unwrap();
    let cache = store.load_cache(None).unwrap();
    assert_eq!(cache.games[0].cover_file.as_deref(), Some("72.jpg"));
    assert_eq!(
        cache.games[0].cover_failed_at, None,
        "success clears the failure"
    );

    // Same cover URL: only the name moves on, the cached cover stays.
    let renamed = GameRecord {
        name: "Portal Two".into(),
        ..game(72, Some(url))
    };
    store.save_games(&[renamed]).unwrap();
    let kept = &store.load_cache(None).unwrap().games[0];
    assert_eq!(
        (kept.name.as_str(), kept.cover_file.as_deref()),
        ("Portal Two", Some("72.jpg"))
    );

    // A different cover invalidates the cached file.
    store
        .save_games(&[game(
            72,
            Some("//images.igdb.com/igdb/image/upload/t_thumb/co2.jpg"),
        )])
        .unwrap();
    let changed = &store.load_cache(None).unwrap().games[0];
    assert_eq!(
        (changed.cover_file.as_deref(), changed.cover_failed_at),
        (None, None)
    );
}

#[test]
fn the_layout_survives_reopening() {
    let dir = TempDir::new().unwrap();
    assert_eq!(
        open(&dir).library_store().load_layout().unwrap(),
        LibraryLayout::default()
    );

    let layout = LibraryLayout {
        view: ViewMode::List,
        sidebar: SidebarSelection::Provider("steam".into()),
        sort: gamenode_core::library::Sort {
            key: SortKey::Provider,
            direction: SortDirection::Descending,
        },
        filters: gamenode_core::library::LibraryFilters {
            availability: gamenode_core::library::AvailabilityFilter {
                installed: true,
                ..Default::default()
            },
            ..Default::default()
        },
        sidebar_width: 180,
        detail_width: 420,
        detail_open: false,
        sidebar_collapsed: true,
        ..LibraryLayout::default()
    };
    open(&dir).library_store().save_layout(&layout).unwrap();
    assert_eq!(open(&dir).library_store().load_layout().unwrap(), layout);
}

#[test]
fn the_saved_layout_has_no_search_text_and_an_unreadable_one_falls_back() {
    let dir = TempDir::new().unwrap();
    let settings = open(&dir);
    let store = settings.library_store();
    store.save_layout(&LibraryLayout::default()).unwrap();
    let json: String = settings
        .conn()
        .query_row(
            "SELECT value FROM settings WHERE key = 'library.layout'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        !json.contains("search") && !json.contains("query"),
        "{json}"
    );

    // A newer build's extra field is ignored; a damaged value is not fatal.
    settings
        .conn()
        .execute("UPDATE settings SET value = '{\"view\":\"list\",\"later\":1}' WHERE key = 'library.layout'", [])
        .unwrap();
    assert_eq!(store.load_layout().unwrap().view, ViewMode::List);
    settings
        .conn()
        .execute(
            "UPDATE settings SET value = 'not json' WHERE key = 'library.layout'",
            [],
        )
        .unwrap();
    assert_eq!(store.load_layout().unwrap(), LibraryLayout::default());
}

#[test]
fn sign_out_removes_the_account_keys_and_keeps_everything_else() {
    let dir = TempDir::new().unwrap();
    let settings = open(&dir);
    let store = settings.library_store();
    let saved = Settings { window: None };
    settings.save(&saved).unwrap();
    store
        .save_layout(&LibraryLayout {
            detail_open: false,
            ..LibraryLayout::default()
        })
        .unwrap();
    store
        .save_scan("steam", &[record("620", "Portal 2")], 1)
        .unwrap();
    store
        .save_chosen_match(
            "acct",
            &ChosenMatch {
                entry_key: "steam:620".into(),
                game_id: Some(1),
                chosen_at: 1,
            },
        )
        .unwrap();

    assert_eq!(store.load_last_account().unwrap(), None);
    let account = LastAccount {
        id: "acct".into(),
        name: "Ada".into(),
    };
    store.save_last_account(&account).unwrap();
    assert_eq!(store.load_last_account().unwrap(), Some(account.clone()));
    let renamed = LastAccount {
        name: "Ada L.".into(),
        ..account
    };
    store.save_last_account(&renamed).unwrap();
    assert_eq!(store.load_last_account().unwrap(), Some(renamed));

    store.clear_last_account().unwrap();
    assert_eq!(store.load_last_account().unwrap(), None);
    let remaining: Vec<String> = settings
        .conn()
        .prepare("SELECT key FROM settings WHERE key LIKE 'account.%'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(remaining.is_empty());

    assert_eq!(
        settings.load().unwrap(),
        saved,
        "foundation settings are kept"
    );
    assert!(
        !store.load_layout().unwrap().detail_open,
        "the layout is kept"
    );
    let cache = store.load_cache(Some("acct")).unwrap();
    assert_eq!(
        (cache.entries.len(), cache.chosen_matches.len()),
        (1, 1),
        "the cache is kept"
    );

    // Saving foundation settings does not touch the account keys.
    store
        .save_last_account(&LastAccount {
            id: "acct".into(),
            name: "Ada".into(),
        })
        .unwrap();
    settings.save(&Settings::default()).unwrap();
    assert!(store.load_last_account().unwrap().is_some());
}

#[test]
fn a_half_written_account_record_is_treated_as_signed_out() {
    let dir = TempDir::new().unwrap();
    let settings = open(&dir);
    settings
        .conn()
        .execute(
            "INSERT INTO settings VALUES ('account.last_id', 'acct')",
            [],
        )
        .unwrap();
    assert_eq!(settings.library_store().load_last_account().unwrap(), None);
}

#[test]
fn nothing_stored_is_a_credential_or_a_callback_url() {
    let dir = TempDir::new().unwrap();
    let settings = open(&dir);
    let store = settings.library_store();
    store.save_layout(&LibraryLayout::default()).unwrap();
    store
        .save_last_account(&LastAccount {
            id: "acct".into(),
            name: "Ada".into(),
        })
        .unwrap();
    store
        .save_scan("steam", &[record("620", "Portal 2")], 1)
        .unwrap();
    store
        .save_games(&[game(
            72,
            Some("//images.igdb.com/igdb/image/upload/t_thumb/co1.jpg"),
        )])
        .unwrap();

    let conn = settings.conn();
    let keys: Vec<String> = conn
        .prepare("SELECT key FROM settings")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(
        keys.iter().all(|k| SETTING_KEYS.contains(&k.as_str())),
        "{keys:?}"
    );
    let values: Vec<String> = conn
        .prepare("SELECT value FROM settings")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(values.iter().all(|v| !v.contains("://")), "{values:?}");
    drop(conn);

    // The new tables never mention secrets either.
    let schema: String = settings
        .conn()
        .query_row(
            "SELECT group_concat(sql, ' ') FROM sqlite_master",
            [],
            |r| r.get(0),
        )
        .unwrap();
    for word in [
        "token",
        "cookie",
        "secret",
        "password",
        "credential",
        "callback",
        "session_token",
    ] {
        assert!(
            !schema.to_lowercase().contains(word),
            "schema mentions {word}"
        );
    }
}

fn catalog(id: &str, name: &str) -> CatalogRecord {
    CatalogRecord {
        id: id.into(),
        name: name.into(),
        fields: BTreeMap::new(),
    }
}

fn catalog_with(id: &str, name: &str, fields: &[(&str, &str)]) -> CatalogRecord {
    CatalogRecord {
        id: id.into(),
        name: name.into(),
        fields: fields
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect(),
    }
}

fn catalog_update(
    owned: Option<Result<Vec<CatalogRecord>, CatalogError>>,
    shared: Option<Result<Vec<CatalogRecord>, CatalogError>>,
    now: i64,
) -> CatalogUpdate {
    CatalogUpdate { owned, shared, now }
}

#[test]
fn catalog_fields_fill_missing_keys_and_survive_a_restart() {
    let dir = TempDir::new().unwrap();
    {
        let store = open(&dir).library_store();
        store
            .save_scan("steam", &[record("620", "Portal 2")], 100)
            .unwrap();
        store
            .save_catalog(
                "steam",
                &catalog_update(
                    Some(Ok(vec![
                        catalog_with(
                            "620",
                            "Portal 2",
                            &[
                                ("namespace", "catalog-ns"),
                                ("state_flags", "catalog-flags"),
                                ("catalog_item_id", "item-620"),
                            ],
                        ),
                        catalog_with(
                            "730",
                            "Counter-Strike 2",
                            &[("namespace", "catalog-only-ns")],
                        ),
                    ])),
                    None,
                    200,
                ),
            )
            .unwrap();
    }

    // A fresh store on the same file is a restart: fields must come from `fields_json`.
    let cache = open(&dir).library_store().load_cache(None).unwrap();
    let portal = cache.entries.iter().find(|e| e.key == "steam:620").unwrap();
    assert_eq!(
        portal.fields.get("state_flags").map(String::as_str),
        Some("4"),
        "installed discovery fields are never erased by the catalog"
    );
    assert_eq!(
        portal.fields.get("namespace").map(String::as_str),
        Some("catalog-ns"),
        "the catalog fills a key the installed scan did not report"
    );
    assert_eq!(
        portal.fields.get("catalog_item_id").map(String::as_str),
        Some("item-620")
    );

    let cs = cache.entries.iter().find(|e| e.key == "steam:730").unwrap();
    assert_eq!(
        cs.fields.get("namespace").map(String::as_str),
        Some("catalog-only-ns")
    );
    assert_eq!(cs.install_dir, "");
}

#[test]
fn catalog_sync_replaces_its_facts_and_lets_owned_win_over_shared() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir).library_store();
    store
        .save_scan(
            "steam",
            &[record("620", "Portal 2"), record("570", "Dota 2")],
            100,
        )
        .unwrap();

    store
        .save_catalog(
            "steam",
            &catalog_update(
                Some(Ok(vec![
                    catalog("620", "Portal 2 from Steam"),
                    catalog("730", "Counter-Strike 2"),
                ])),
                None,
                200,
            ),
        )
        .unwrap();

    let cache = store.load_cache(None).unwrap();
    let portal = cache.entries.iter().find(|e| e.key == "steam:620").unwrap();
    assert_eq!(
        (
            portal.installed,
            portal.owned,
            portal.shared,
            portal.name.as_deref(),
            portal.first_seen,
            portal.last_seen,
        ),
        (true, true, false, Some("Portal 2"), 100, 200),
        "the installed name and first-seen time survive"
    );
    let cs = cache.entries.iter().find(|e| e.key == "steam:730").unwrap();
    assert_eq!(
        (
            cs.installed,
            cs.owned,
            cs.shared,
            cs.install_dir.as_str(),
            cs.first_seen,
        ),
        (false, true, false, "", 200)
    );
    let dota = cache.entries.iter().find(|e| e.key == "steam:570").unwrap();
    assert!(!dota.owned, "the owned set is replaced, not merged");
    assert_eq!(cache.scans[0].last_catalog_at, Some(200));
    assert_eq!(cache.scans[0].catalog_error, None);

    // A shared sync after owned marks only games that are not owned, and a catalog-only name is
    // kept once it is non-empty.
    store
        .save_catalog(
            "steam",
            &catalog_update(
                None,
                Some(Ok(vec![
                    catalog("620", "Portal 2 shared"),
                    catalog("730", "Counter-Strike 2 shared"),
                    catalog("400", "Shared Game"),
                ])),
                300,
            ),
        )
        .unwrap();
    let cache = store.load_cache(None).unwrap();
    let portal = cache.entries.iter().find(|e| e.key == "steam:620").unwrap();
    assert_eq!(
        (portal.owned, portal.shared),
        (true, false),
        "owned wins over shared"
    );
    let cs = cache.entries.iter().find(|e| e.key == "steam:730").unwrap();
    assert_eq!(
        (cs.owned, cs.shared, cs.name.as_deref()),
        (true, false, Some("Counter-Strike 2")),
        "an existing non-empty catalog name is not replaced"
    );
    let shared = cache.entries.iter().find(|e| e.key == "steam:400").unwrap();
    assert_eq!(
        (shared.installed, shared.owned, shared.shared),
        (false, false, true)
    );
    assert_eq!(shared.name.as_deref(), Some("Shared Game"));
    assert_eq!(cache.scans[0].last_shared_at, Some(300));
    assert_eq!(cache.scans[0].shared_error, None);
}

#[test]
fn failed_catalog_phases_keep_their_facts_and_only_record_the_error() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir).library_store();
    store
        .save_catalog(
            "steam",
            &catalog_update(
                Some(Ok(vec![catalog("620", "Portal 2")])),
                Some(Ok(vec![catalog("400", "Shared Game")])),
                100,
            ),
        )
        .unwrap();

    store
        .save_catalog(
            "steam",
            &catalog_update(
                Some(Err(CatalogError::Expired)),
                Some(Err(CatalogError::SharedUnavailable)),
                200,
            ),
        )
        .unwrap();

    let cache = store.load_cache(None).unwrap();
    let owned = cache.entries.iter().find(|e| e.key == "steam:620").unwrap();
    assert!(
        owned.owned,
        "a failed owned sync keeps the previous owned set"
    );
    let shared = cache.entries.iter().find(|e| e.key == "steam:400").unwrap();
    assert!(
        shared.shared,
        "a failed shared sync keeps the previous shared set"
    );
    assert_eq!(
        (
            cache.scans[0].last_catalog_at,
            cache.scans[0].catalog_error,
            cache.scans[0].last_shared_at,
            cache.scans[0].shared_error,
        ),
        (
            Some(100),
            Some(CatalogError::Expired),
            Some(100),
            Some(CatalogError::SharedUnavailable)
        ),
        "a failure neither advances its last-success time nor touches the other phase"
    );

    let stored: String = store
        .conn()
        .query_row(
            "SELECT family_error FROM provider_scans WHERE provider = 'steam'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        stored, "sharedUnavailable",
        "the historical family_error column stores the renamed category"
    );

    // A later owned success replaces owned facts, clears its own error and leaves shared alone.
    store
        .save_catalog(
            "steam",
            &catalog_update(Some(Ok(vec![catalog("400", "Now owned")])), None, 300),
        )
        .unwrap();
    let cache = store.load_cache(None).unwrap();
    assert!(
        !cache
            .entries
            .iter()
            .find(|e| e.key == "steam:620")
            .unwrap()
            .owned
    );
    let shared = cache.entries.iter().find(|e| e.key == "steam:400").unwrap();
    assert_eq!((shared.owned, shared.shared), (true, true));
    assert_eq!(
        (
            cache.scans[0].last_catalog_at,
            cache.scans[0].catalog_error,
            cache.scans[0].last_shared_at,
            cache.scans[0].shared_error,
        ),
        (
            Some(300),
            None,
            Some(100),
            Some(CatalogError::SharedUnavailable)
        )
    );

    // A shared success then clears the shared error and applies owned-wins again.
    store
        .save_catalog(
            "steam",
            &catalog_update(
                None,
                Some(Ok(vec![
                    catalog("400", "Now owned"),
                    catalog("730", "Shared"),
                ])),
                400,
            ),
        )
        .unwrap();
    let cache = store.load_cache(None).unwrap();
    let owned = cache.entries.iter().find(|e| e.key == "steam:400").unwrap();
    assert_eq!((owned.owned, owned.shared), (true, false));
    let shared = cache.entries.iter().find(|e| e.key == "steam:730").unwrap();
    assert_eq!((shared.owned, shared.shared), (false, true));
    assert_eq!(
        (cache.scans[0].last_shared_at, cache.scans[0].shared_error,),
        (Some(400), None)
    );
}

#[test]
fn a_shared_failure_stored_by_an_older_build_is_read_as_shared() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir).library_store();
    store
        .save_catalog(
            "steam",
            &catalog_update(Some(Ok(vec![catalog("400", "Shared Game")])), None, 100),
        )
        .unwrap();

    // A build that still used the family name wrote this row.
    store
        .conn()
        .execute(
            "UPDATE provider_scans SET family_error = 'familyUnavailable' WHERE provider = 'steam'",
            [],
        )
        .unwrap();

    let cache = store.load_cache(None).unwrap();
    assert_eq!(
        cache.scans[0].shared_error,
        Some(CatalogError::SharedUnavailable),
        "the old category name maps onto the shared failure"
    );
    assert!(
        cache.entries.iter().any(|entry| entry.key == "steam:400"),
        "the stored facts are still there"
    );
}

#[test]
fn catalog_names_follow_installed_then_existing_then_catalog_precedence() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir).library_store();

    store
        .save_catalog(
            "steam",
            &catalog_update(
                Some(Ok(vec![catalog("730", "First catalog name")])),
                None,
                100,
            ),
        )
        .unwrap();
    let name = |store: &crate::library_db::SqliteLibraryStore| {
        store
            .load_cache(None)
            .unwrap()
            .entries
            .into_iter()
            .find(|e| e.key == "steam:730")
            .and_then(|e| e.name)
    };
    assert_eq!(name(&store).as_deref(), Some("First catalog name"));

    // An existing non-empty name is kept even when the catalog renames the app.
    store
        .save_catalog(
            "steam",
            &catalog_update(
                Some(Ok(vec![catalog("730", "Second catalog name")])),
                None,
                200,
            ),
        )
        .unwrap();
    assert_eq!(name(&store).as_deref(), Some("First catalog name"));

    // An empty existing name is replaced by the catalog name.
    store
        .conn()
        .execute(
            "UPDATE library_entries SET name = NULL WHERE key = 'steam:730'",
            [],
        )
        .unwrap();
    store
        .save_catalog(
            "steam",
            &catalog_update(
                Some(Ok(vec![catalog("730", "Third catalog name")])),
                None,
                300,
            ),
        )
        .unwrap();
    assert_eq!(name(&store).as_deref(), Some("Third catalog name"));

    // Once installed, the installed scan's name wins over a catalog name.
    store
        .save_scan("steam", &[record("730", "Installed name")], 400)
        .unwrap();
    store
        .save_catalog(
            "steam",
            &catalog_update(
                Some(Ok(vec![catalog("730", "Fourth catalog name")])),
                None,
                500,
            ),
        )
        .unwrap();
    assert_eq!(name(&store).as_deref(), Some("Installed name"));
}

#[test]
fn save_scan_never_changes_owned_or_shared_facts() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir).library_store();
    store
        .save_catalog(
            "steam",
            &catalog_update(
                Some(Ok(vec![catalog("620", "Portal 2")])),
                Some(Ok(vec![catalog("400", "Shared Game")])),
                100,
            ),
        )
        .unwrap();

    store
        .save_scan("steam", &[record("620", "Portal 2")], 200)
        .unwrap();
    let cache = store.load_cache(None).unwrap();
    let portal = cache.entries.iter().find(|e| e.key == "steam:620").unwrap();
    assert_eq!(
        (portal.installed, portal.owned, portal.shared),
        (true, true, false)
    );
    assert!(
        cache
            .entries
            .iter()
            .find(|e| e.key == "steam:400")
            .unwrap()
            .shared,
        "a scan for another entry-set does not clear sharing"
    );

    // A scan that no longer finds an entry only clears installed.
    store.save_scan("steam", &[], 300).unwrap();
    let cache = store.load_cache(None).unwrap();
    let portal = cache.entries.iter().find(|e| e.key == "steam:620").unwrap();
    assert_eq!(
        (portal.installed, portal.owned, portal.shared),
        (false, true, false)
    );
    assert!(
        cache
            .entries
            .iter()
            .find(|e| e.key == "steam:400")
            .unwrap()
            .shared
    );
}

#[test]
fn clear_catalog_keeps_entries_matches_and_covers() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir).library_store();
    store
        .save_scan("steam", &[record("620", "Portal 2")], 100)
        .unwrap();
    store
        .save_auto_matches(&[AutoMatch {
            category: 1,
            uid: "620".into(),
            game_ids: vec![72],
            checked_at: 100,
        }])
        .unwrap();
    store
        .save_chosen_match(
            "acct",
            &ChosenMatch {
                entry_key: "steam:620".into(),
                game_id: Some(72),
                chosen_at: 110,
            },
        )
        .unwrap();
    store
        .save_games(&[game(72, Some("https://images.example/co1.jpg"))])
        .unwrap();
    store.save_cover_ready(72, "72.jpg").unwrap();
    store
        .save_catalog(
            "steam",
            &catalog_update(
                Some(Ok(vec![catalog("620", "Portal 2")])),
                Some(Ok(vec![catalog("400", "Shared Game")])),
                200,
            ),
        )
        .unwrap();

    store.clear_catalog("steam").unwrap();
    let cache = store.load_cache(Some("acct")).unwrap();
    assert_eq!(
        cache
            .entries
            .iter()
            .map(|e| (e.key.as_str(), e.owned, e.shared))
            .collect::<Vec<_>>(),
        [("steam:400", false, false), ("steam:620", false, false)]
    );
    assert_eq!(cache.auto_matches.len(), 1);
    assert_eq!(cache.chosen_matches.len(), 1);
    assert_eq!(cache.games[0].cover_file.as_deref(), Some("72.jpg"));
    assert_eq!(
        (
            cache.scans[0].last_catalog_at,
            cache.scans[0].catalog_error,
            cache.scans[0].last_shared_at,
            cache.scans[0].shared_error,
        ),
        (None, None, None, None)
    );
    assert_eq!(cache.scans[0].last_success_at, Some(100));
}

#[test]
fn store_accounts_are_upserted_and_cleared_per_provider() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir).library_store();
    assert!(store.load_store_accounts().unwrap().is_empty());

    let steam = StoreAccount {
        provider: "steam".into(),
        account_id: "7656119".into(),
        account_name: Some("Ada".into()),
        last_verified_at: Some(100),
    };
    let gog = StoreAccount {
        provider: "gog".into(),
        account_id: "g-1".into(),
        account_name: None,
        last_verified_at: None,
    };
    store.save_store_account(&steam).unwrap();
    store.save_store_account(&gog).unwrap();
    assert_eq!(
        store.load_store_accounts().unwrap(),
        [gog.clone(), steam.clone()],
        "accounts are ordered by provider"
    );

    let renamed = StoreAccount {
        account_id: "7656120".into(),
        account_name: Some("Ada L.".into()),
        last_verified_at: Some(200),
        ..steam
    };
    store.save_store_account(&renamed).unwrap();
    assert_eq!(
        store.load_store_accounts().unwrap(),
        [gog.clone(), renamed.clone()]
    );

    store.clear_store_account("steam").unwrap();
    assert_eq!(store.load_store_accounts().unwrap(), [gog]);
    store.clear_store_account("steam").unwrap(); // already gone
}

fn all_database_text(conn: &Connection) -> String {
    use rusqlite::types::ValueRef;

    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let mut text = String::new();
    let schema: String = conn
        .query_row(
            "SELECT group_concat(sql, ' ') FROM sqlite_master",
            [],
            |row| row.get(0),
        )
        .unwrap();
    text.push_str(&schema);
    for table in tables {
        let mut stmt = conn.prepare(&format!("SELECT * FROM \"{table}\"")).unwrap();
        let columns = stmt.column_count();
        let mut rows = stmt.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            for index in 0..columns {
                match row.get_ref(index).unwrap() {
                    ValueRef::Text(value) | ValueRef::Blob(value) => {
                        text.push_str(&String::from_utf8_lossy(value));
                    }
                    ValueRef::Integer(value) => text.push_str(&value.to_string()),
                    ValueRef::Real(value) => text.push_str(&value.to_string()),
                    ValueRef::Null => {}
                }
                text.push(' ');
            }
        }
    }
    text
}

#[test]
fn catalog_and_account_persistence_never_writes_a_cookie_or_token() {
    let dir = TempDir::new().unwrap();
    let settings = open(&dir);
    let store = settings.library_store();
    store
        .save_store_account(&StoreAccount {
            provider: "steam".into(),
            account_id: "7656119".into(),
            account_name: Some("Ada".into()),
            last_verified_at: Some(100),
        })
        .unwrap();
    store
        .save_catalog(
            "steam",
            &catalog_update(
                Some(Ok(vec![catalog("620", "Portal 2")])),
                Some(Ok(vec![catalog("400", "Shared Game")])),
                100,
            ),
        )
        .unwrap();

    let text = all_database_text(&settings.conn());
    for secret in [
        "steamLoginSecure",
        "sessionid",
        "webapi_token",
        "access_token",
        "Bearer ",
    ] {
        assert!(
            !text.contains(secret),
            "the database contains a credential marker: {secret}"
        );
    }
}

#[test]
fn unreadable_flags_and_error_categories_degrade_without_hiding_installed_games() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir).library_store();
    store
        .save_scan("steam", &[record("620", "Portal 2")], 100)
        .unwrap();
    store
        .save_catalog(
            "steam",
            &catalog_update(Some(Ok(vec![catalog("620", "Portal 2")])), None, 200),
        )
        .unwrap();
    store
        .conn()
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             UPDATE library_entries SET owned = 7, family_shared = -1 WHERE key = 'steam:620';
             UPDATE provider_scans SET catalog_error = 'a raw error message' WHERE provider = 'steam';",
        )
        .unwrap();

    let cache = store.load_cache(None).unwrap();
    let entry = &cache.entries[0];
    assert!(
        entry.installed,
        "the installed fact still makes the game visible"
    );
    assert!(!entry.owned && !entry.shared);
    assert_eq!(cache.scans[0].catalog_error, None);
}

#[test]
fn the_local_state_doc_names_every_table_setting_and_folder() {
    const DOC: &str = include_str!("../../../../docs/local-state.md");
    let dir = TempDir::new().unwrap();
    let settings = open(&dir);
    let tables: Vec<String> = settings
        .conn()
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for table in &tables {
        assert!(
            DOC.contains(&format!("`{table}`")),
            "docs/local-state.md lacks table {table}"
        );
    }
    for key in SETTING_KEYS {
        assert!(
            DOC.contains(&format!("`{key}`")),
            "docs/local-state.md lacks setting {key}"
        );
    }
    let covers = crate::paths::DataPaths::at("/data").covers_dir();
    assert_eq!(
        covers,
        std::path::Path::new("/data").join(crate::paths::COVERS_DIR)
    );
    assert!(DOC.contains(&format!("`{}/<game id>.jpg`", crate::paths::COVERS_DIR)));
}

#[test]
fn a_chosen_match_can_be_forgotten_for_one_account_only() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir).library_store();
    let choice = |game_id| ChosenMatch {
        entry_key: "steam:620".to_owned(),
        game_id,
        chosen_at: 5,
    };
    store.save_chosen_match("alice", &choice(Some(72))).unwrap();
    store.save_chosen_match("bob", &choice(None)).unwrap();

    store.delete_chosen_match("alice", "steam:620").unwrap();
    assert!(
        store
            .load_cache(Some("alice"))
            .unwrap()
            .chosen_matches
            .is_empty()
    );
    assert_eq!(
        store.load_cache(Some("bob")).unwrap().chosen_matches.len(),
        1
    );
    // Forgetting what is not there is not an error.
    store.delete_chosen_match("alice", "steam:620").unwrap();
}

#[test]
fn excluded_entries_round_trip_and_unreadable_json_falls_back() {
    let dir = TempDir::new().unwrap();
    let settings = open(&dir);
    let store = settings.library_store();
    assert!(store.load_cache(None).unwrap().excluded.is_empty());

    let excluded = vec!["steam:620".to_owned(), "gog:1".to_owned()];
    store.save_excluded(&excluded).unwrap();
    assert_eq!(store.load_cache(None).unwrap().excluded, excluded);

    settings
        .conn()
        .execute(
            "UPDATE settings SET value = 'not json' WHERE key = 'library.excluded'",
            [],
        )
        .unwrap();
    assert!(store.load_cache(None).unwrap().excluded.is_empty());
}

fn favorite(account: &str, game_id: u64, is_favorite: bool, pending: bool) -> FavoriteRecord {
    FavoriteRecord {
        account_id: account.to_owned(),
        game_id,
        is_favorite,
        pending,
        updated_at: 100,
    }
}

#[test]
fn favorites_round_trip_per_account_with_their_pending_flag() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir).library_store();
    store
        .save_favorite(&favorite("ana", 7, true, false))
        .unwrap();
    store
        .save_favorite(&favorite("ana", 8, true, true))
        .unwrap();
    store
        .save_favorite(&favorite("bo", 9, true, false))
        .unwrap();

    let ana = store.load_cache(Some("ana")).unwrap().favorites;
    assert_eq!(
        ana,
        [
            favorite("ana", 7, true, false),
            FavoriteRecord {
                updated_at: 100,
                pending: true,
                ..favorite("ana", 8, true, false)
            }
        ]
    );
    // Signing out (no account) exposes nothing; another account sees only its own set.
    assert!(store.load_cache(None).unwrap().favorites.is_empty());
    assert_eq!(store.load_cache(Some("bo")).unwrap().favorites.len(), 1);
}

#[test]
fn a_confirmed_removal_deletes_the_row_instead_of_storing_it() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir).library_store();
    store
        .save_favorite(&favorite("ana", 7, true, true))
        .unwrap();
    // Still pending: kept so the change is not forgotten.
    store
        .save_favorite(&favorite("ana", 7, false, true))
        .unwrap();
    assert_eq!(store.load_cache(Some("ana")).unwrap().favorites.len(), 1);
    // Confirmed: absence means "not a favorite and nothing to send".
    store
        .save_favorite(&favorite("ana", 7, false, false))
        .unwrap();
    assert!(store.load_cache(Some("ana")).unwrap().favorites.is_empty());
}

#[test]
fn a_confirmed_favorite_upserts_over_the_pending_change() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir).library_store();
    store
        .save_favorite(&favorite("ana", 7, true, true))
        .unwrap();
    store
        .save_favorite(&favorite("ana", 7, true, false))
        .unwrap();
    let favorites = store.load_cache(Some("ana")).unwrap().favorites;
    assert_eq!(favorites.len(), 1);
    assert!(!favorites[0].pending && favorites[0].is_favorite);
}

#[test]
fn the_favorites_migration_adds_the_table_and_keeps_existing_data() {
    use crate::db::{MIGRATIONS, SqliteSettingsStore, open_with};

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.sqlite3");
    {
        let conn = open_with(&path, &MIGRATIONS[..6]).unwrap();
        conn.execute(
            "INSERT INTO library_entries
                (key, provider, store_id, name, install_dir, fields_json, installed, first_seen, last_seen)
             VALUES ('steam:1', 'steam', '1', 'Old', '/old', '{}', 1, 1, 1)",
            [],
        )
        .unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'favorites'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }

    let store = SqliteSettingsStore::open(&path).unwrap().library_store();
    store
        .save_favorite(&favorite("ana", 7, true, true))
        .unwrap();
    assert_eq!(store.load_cache(Some("ana")).unwrap().entries.len(), 1);
    assert_eq!(store.load_cache(Some("ana")).unwrap().favorites.len(), 1);
}
