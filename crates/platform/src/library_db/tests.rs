use super::*;
use crate::db::{SETTING_KEYS, SqliteSettingsStore};
use gamenode_core::library::{SidebarSelection, SortDirection, SortKey, ViewMode};
use gamenode_core::ports::SettingsStore;
use gamenode_core::{RendererPreference, Settings};
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
            last_error: None
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
        sidebar_width: 180,
        detail_width: 420,
        detail_open: false,
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
    let saved = Settings {
        renderer: RendererPreference::Software,
        window: None,
    };
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
        "session",
    ] {
        assert!(
            !schema.to_lowercase().contains(word),
            "schema mentions {word}"
        );
    }
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
