use std::sync::Arc;

use gamenode_core::Provider;
use gamenode_core::auth::{AuthMachine, Input};
use gamenode_core::credential::StorageMethod;
use gamenode_core::library::{
    CoverSize, DetailView, EntryStatus, Input as LibraryInput, LibraryFilters, MatchPlatformFilter,
    MatchSource, MatchStatus, Membership, Row, SelectMode, SidebarSelection, Sort, SortDirection,
    SortKey, ViewMode,
};
use gamenode_core::library::{FilterCounts, RowGroup, ScopeCounts, TitleSource, TitleSourceKind};
use gamenode_core::library::{GroupBy, LibraryScope, RowGameNode};
use gamenode_core::notice::Notice;
use gamenode_core::playtime::{PlaytimeSource, PlaytimeView};
use gamenode_core::settings::{Settings, WindowGeometry};
use gamenode_core::snapshot::{Snapshot, TrackingView};

#[test]
fn test_snapshot_roundtrip_serialization() {
    let machine = AuthMachine::new(Box::new(|| "test-nonce".to_string()));
    let settings = Settings {
        window: Some(WindowGeometry {
            width: 1280,
            height: 720,
            x: Some(100),
            y: Some(50),
            maximized: false,
        }),
        ..Settings::default()
    };

    let mut snapshot = Snapshot::new(&machine, StorageMethod::SecretService, settings, None, None);
    snapshot.tracking = TrackingView {
        title: "Test Game".into(),
        available: true,
        running: ["game_1".into()].into_iter().collect(),
        session_started_at: [("game_1".into(), 1_700_000_000)].into_iter().collect(),
    };
    snapshot.notice = Some(Notice::SignInUnavailable);

    // Serialize to JSON string
    let json_str = serde_json::to_string(&snapshot).expect("failed to serialize Snapshot");

    // Verify key fields are in camelCase
    let value: serde_json::Value = serde_json::from_str(&json_str).expect("failed to parse JSON");
    assert!(value.get("localGames").is_some());
    assert!(value.get("playtimeError").is_some());
    assert!(value.get("playtimeSource").is_some());
    assert!(value.get("auth").is_some());
    assert!(value.get("storage").is_some());
    assert!(value.get("settings").is_some());
    assert!(value.get("preferences").is_some());
    assert!(value.get("tools").is_some());
    assert_eq!(
        value
            .get("tracking")
            .and_then(|tracking| tracking.get("sessionStartedAt"))
            .and_then(|started| started.get("game_1")),
        Some(&serde_json::json!(1_700_000_000))
    );

    // Check settings.window camelCase
    let window = value.get("settings").unwrap().get("window").unwrap();
    assert_eq!(window.get("width"), Some(&serde_json::json!(1280)));
    assert_eq!(window.get("maximized"), Some(&serde_json::json!(false)));

    // Deserialize back and assert equality
    let deserialized: Snapshot =
        serde_json::from_str(&json_str).expect("failed to deserialize Snapshot");
    assert_eq!(snapshot, deserialized);
}

#[test]
fn test_detail_view_serialization_and_artwork_omission() {
    let row = Arc::new(Row {
        playtime: PlaytimeView {
            seconds: Some(3600),
            source: PlaytimeSource::Store,
            last_played_at: Some(1_700_000_000),
        },
        running: false,
        session_started_at: None,
        key: "steam:1091500".into(),
        title: "Cyberpunk 2077".into(),
        detected_name: "Cyberpunk 2077".into(),
        matched_name: Some("Cyberpunk 2077".into()),
        provider: "steam".into(),
        provider_name: "Steam".into(),
        installed: true,
        owned: true,
        shared: false,
        status: MatchStatus::Matched {
            game_id: 1091500,
            source: MatchSource::Chosen,
        },
        cover_file: Some("1091500.jpg".into()),
        game_cover_file: Some("1091500.jpg".into()),
        release_year: Some(2020),
        added_at: 1_700_000_000,
        is_favorite: false,
        hidden: false,
        gamenode: None,
    });

    let raw_bytes: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    let artwork_arc: Arc<[u8]> = Arc::from(raw_bytes);

    let detail = DetailView {
        running: false,
        row,
        detected_target: None,
        install_dir: "C:\\Games\\Cyberpunk 2077".into(),
        release_year: Some(2020),
        has_artwork: true,
        artwork: Some(artwork_arc),
        membership: Membership::InLibrary(Some(EntryStatus::Playing)),
        achievement_backend: false,
        achievements: None,
        suggestions: Vec::new(),
    };

    let json_val = serde_json::to_value(&detail).expect("serialize DetailView");

    // Verify camelCase fields
    assert_eq!(
        json_val.get("installDir"),
        Some(&serde_json::json!("C:\\Games\\Cyberpunk 2077"))
    );
    assert_eq!(json_val.get("releaseYear"), Some(&serde_json::json!(2020)));
    assert_eq!(json_val.get("hasArtwork"), Some(&serde_json::json!(true)));
    let row_json = json_val.get("row").unwrap();
    assert_eq!(row_json.get("hidden"), Some(&serde_json::json!(false)));
    assert_eq!(
        row_json.get("sessionStartedAt"),
        Some(&serde_json::Value::Null)
    );
    assert_eq!(row_json.get("gamenode"), Some(&serde_json::Value::Null));

    // CRITICAL: Verify that the raw artwork bytes are NOT serialized in JSON
    assert!(
        json_val.get("artwork").is_none(),
        "Raw artwork bytes must be skipped in JSON serialization!"
    );

    // Deserializing DetailView round-trips with artwork: None and has_artwork: true
    let json_str = serde_json::to_string(&detail).expect("serialize DetailView to string");
    let deserialized: DetailView = serde_json::from_str(&json_str).expect("deserialize DetailView");
    assert!(deserialized.has_artwork);
    assert_eq!(deserialized.artwork, None);
    assert_eq!(deserialized.install_dir, detail.install_dir);
    assert_eq!(deserialized.release_year, detail.release_year);

    // Verify set_artwork helper updates has_artwork
    let mut updated_detail = detail.clone();
    updated_detail.set_artwork(None);
    assert!(!updated_detail.has_artwork);
    assert_eq!(updated_detail.artwork, None);
}

#[test]
fn test_auth_input_serialization() {
    let inputs = vec![
        Input::Start,
        Input::StartSignIn(Provider::Discord),
        Input::CancelSignIn,
        Input::BrowserOpened {
            op: 42,
            url: "https://server.test/sign-in".into(),
        },
        Input::ReopenBrowser,
        Input::BrowserReopenFailed {
            url: "https://server.test/sign-in".into(),
        },
        Input::CallbackReceived("app.gamenode://auth?code=xyz".into()),
        Input::RetryRestore,
        Input::SignOut,
        Input::Tick,
        Input::SessionRejected,
    ];

    for input in &inputs {
        let json_str = serde_json::to_string(input).expect("serialize auth Input");
        let deserialized: Input = serde_json::from_str(&json_str).expect("deserialize auth Input");
        assert_eq!(input, &deserialized);
    }

    // Verify camelCase representation
    let browser_opened = Input::BrowserOpened {
        op: 99,
        url: "https://server.test/sign-in".into(),
    };
    let json_val = serde_json::to_value(&browser_opened).unwrap();
    assert_eq!(
        json_val.get("browserOpened"),
        Some(&serde_json::json!({
            "op": 99,
            "url": "https://server.test/sign-in"
        }))
    );

    let start_sign_in = Input::StartSignIn(Provider::Google);
    let json_val = serde_json::to_value(&start_sign_in).unwrap();
    assert_eq!(
        json_val.get("startSignIn"),
        Some(&serde_json::json!("google"))
    );
}

#[test]
fn test_library_input_serialization() {
    let inputs = vec![
        LibraryInput::AccountChanged(None),
        LibraryInput::Select {
            key: "steam:100".into(),
            mode: SelectMode::Replace,
        },
        LibraryInput::Select {
            key: "steam:101".into(),
            mode: SelectMode::Toggle,
        },
        LibraryInput::SetSearch("witcher".into()),
        LibraryInput::SetView(ViewMode::Grid),
        LibraryInput::SetCoverSize(CoverSize::Large),
        LibraryInput::SetSort(Sort {
            key: SortKey::Title,
            direction: SortDirection::Ascending,
        }),
        LibraryInput::OpenMatch,
        LibraryInput::CloseMatch,
        LibraryInput::SetMatchPlatformFilter { enabled: false },
        LibraryInput::ScanRequested,
        LibraryInput::RequestCatalog,
        LibraryInput::SetFilters(LibraryFilters::default()),
        LibraryInput::SaveView {
            name: "Steam 10 h+".into(),
        },
        LibraryInput::RenameView {
            id: 3,
            name: "New name".into(),
        },
        LibraryInput::DeleteView { id: 3 },
        LibraryInput::ApplyView { id: 3 },
        LibraryInput::SetHidden {
            keys: vec!["steam:100".into()],
            hidden: true,
        },
        LibraryInput::SetScope(LibraryScope::Recent),
        LibraryInput::SetGroup(GroupBy::Store),
        LibraryInput::ToggleGroup { id: "steam".into() },
        LibraryInput::SetAllGroupsCollapsed(true),
        LibraryInput::SelectAll,
        LibraryInput::ClearSelection,
        LibraryInput::MarkEntryNoGame {
            key: "manual:7".into(),
            now: 5,
        },
        LibraryInput::SetSidebarCollapsed(true),
        LibraryInput::SetFavorites {
            keys: vec!["steam:100".into(), "steam:101".into()],
            favorite: true,
            now: 5,
        },
        LibraryInput::SetStatus {
            keys: vec!["steam:100".into()],
            status: EntryStatus::Finished,
        },
        LibraryInput::AddToCollection {
            keys: vec!["steam:100".into()],
            collection_id: "backlog".into(),
        },
        LibraryInput::UndoAdd { run: 9 },
        LibraryInput::OpenAdd,
        LibraryInput::HideAdd,
        LibraryInput::CloseAdd,
        LibraryInput::StartNewCollection,
        LibraryInput::SetNewCollectionName("Coop no sofá".into()),
        LibraryInput::CreateCollection,
        LibraryInput::CancelNewCollection,
    ];

    for input in &inputs {
        let json_str = serde_json::to_string(input).expect("serialize library Input");
        let deserialized: LibraryInput =
            serde_json::from_str(&json_str).expect("deserialize library Input");
        assert_eq!(input, &deserialized);
    }

    // Verify camelCase representation
    let search = LibraryInput::SetSearch("cyberpunk".into());
    let json_val = serde_json::to_value(&search).unwrap();
    assert_eq!(
        json_val.get("setSearch"),
        Some(&serde_json::json!("cyberpunk"))
    );
    // Exactly what the UI sends when the sidebar collapses.
    assert_eq!(
        serde_json::to_value(LibraryInput::SetSidebarCollapsed(true)).unwrap(),
        serde_json::json!({ "setSidebarCollapsed": true })
    );
    assert_eq!(
        serde_json::to_value(LibraryInput::SetCoverSize(CoverSize::Large)).unwrap(),
        serde_json::json!({ "setCoverSize": "large" })
    );
    // The add dialog's quick collection field and its background batch.
    assert_eq!(
        serde_json::to_value(LibraryInput::SetNewCollectionName("Coop".into())).unwrap(),
        serde_json::json!({ "setNewCollectionName": "Coop" })
    );
    assert_eq!(
        serde_json::to_value(LibraryInput::CreateCollection).unwrap(),
        serde_json::json!("createCollection")
    );
    assert_eq!(
        serde_json::to_value(LibraryInput::HideAdd).unwrap(),
        serde_json::json!("hideAdd")
    );
    // The organization inputs carry the exact payload the mock host mirrors.
    assert_eq!(
        serde_json::to_value(LibraryInput::SaveView {
            name: "Steam".into()
        })
        .unwrap(),
        serde_json::json!({ "saveView": { "name": "Steam" } })
    );
    assert_eq!(
        serde_json::to_value(LibraryInput::RenameView {
            id: 3,
            name: "New".into()
        })
        .unwrap(),
        serde_json::json!({ "renameView": { "id": 3, "name": "New" } })
    );
    assert_eq!(
        serde_json::to_value(LibraryInput::DeleteView { id: 3 }).unwrap(),
        serde_json::json!({ "deleteView": { "id": 3 } })
    );
    assert_eq!(
        serde_json::to_value(LibraryInput::ApplyView { id: 3 }).unwrap(),
        serde_json::json!({ "applyView": { "id": 3 } })
    );
    assert_eq!(
        serde_json::to_value(LibraryInput::SetHidden {
            keys: vec!["steam:1".into()],
            hidden: true
        })
        .unwrap(),
        serde_json::json!({ "setHidden": { "keys": ["steam:1"], "hidden": true } })
    );
    // The detail pane's direct Not a game action names the entry it acts on.
    assert_eq!(
        serde_json::to_value(LibraryInput::MarkEntryNoGame {
            key: "manual:7".into(),
            now: 5
        })
        .unwrap(),
        serde_json::json!({ "markEntryNoGame": { "key": "manual:7", "now": 5 } })
    );
    // The batch GameNode writes carry the exact payload the mock host mirrors.
    assert_eq!(
        serde_json::to_value(LibraryInput::SetFavorites {
            keys: vec!["steam:1".into()],
            favorite: true,
            now: 7
        })
        .unwrap(),
        serde_json::json!({ "setFavorites": { "keys": ["steam:1"], "favorite": true, "now": 7 } })
    );
    assert_eq!(
        serde_json::to_value(LibraryInput::SetStatus {
            keys: vec!["steam:1".into()],
            status: EntryStatus::Finished
        })
        .unwrap(),
        serde_json::json!({ "setStatus": { "keys": ["steam:1"], "status": "finished" } })
    );
    assert_eq!(
        serde_json::to_value(LibraryInput::AddToCollection {
            keys: vec!["steam:1".into()],
            collection_id: "backlog".into()
        })
        .unwrap(),
        serde_json::json!({ "addToCollection": { "keys": ["steam:1"], "collectionId": "backlog" } })
    );
    assert_eq!(
        serde_json::to_value(LibraryInput::UndoAdd { run: 4 }).unwrap(),
        serde_json::json!({ "undoAdd": { "run": 4 } })
    );
    let filters =
        serde_json::to_value(LibraryInput::SetFilters(LibraryFilters::default())).unwrap();
    assert_eq!(
        filters.get("setFilters"),
        Some(&serde_json::json!({
            "availability": { "installed": false, "owned": false, "shared": false },
            "stores": [],
            "matchStates": [],
            "gamenodeStatus": [],
            "playtime": "any",
            "played": "any",
            "lastPlayed": "any",
            "release": "any",
            "added": "any",
            "favoritesOnly": false,
            "runningOnly": false,
            "showHidden": false
        }))
    );
    assert_eq!(
        serde_json::to_value(LibraryInput::RequestCatalog).unwrap(),
        serde_json::json!("requestCatalog")
    );
    assert_eq!(
        serde_json::to_value(LibraryInput::SetScope(LibraryScope::Recent)).unwrap(),
        serde_json::json!({ "setScope": "recent" })
    );
    assert_eq!(
        serde_json::to_value(LibraryInput::SetGroup(GroupBy::GameNodeStatus)).unwrap(),
        serde_json::json!({ "setGroup": "gameNodeStatus" })
    );
    assert_eq!(
        serde_json::to_value(LibraryInput::ToggleGroup { id: "steam".into() }).unwrap(),
        serde_json::json!({ "toggleGroup": { "id": "steam" } })
    );
    assert_eq!(
        serde_json::to_value(LibraryInput::SetAllGroupsCollapsed(true)).unwrap(),
        serde_json::json!({ "setAllGroupsCollapsed": true })
    );
    assert_eq!(
        serde_json::to_value(LibraryInput::SelectAll).unwrap(),
        serde_json::json!("selectAll")
    );
    assert_eq!(
        serde_json::to_value(LibraryInput::ClearSelection).unwrap(),
        serde_json::json!("clearSelection")
    );
    // Exactly what the match dialog's platform switch sends.
    assert_eq!(
        serde_json::to_value(LibraryInput::SetMatchPlatformFilter { enabled: false }).unwrap(),
        serde_json::json!({ "setMatchPlatformFilter": { "enabled": false } })
    );
}

#[test]
fn sidebar_filter_payloads_from_the_webview_parse() {
    // Exactly what the sidebar sends: "All games", then a provider button.
    let all: LibraryInput = serde_json::from_str(r#"{"setFilter":"all"}"#).unwrap();
    assert_eq!(all, LibraryInput::SetFilter(SidebarSelection::All));
    let steam: LibraryInput =
        serde_json::from_str(r#"{"setFilter":{"provider":"steam"}}"#).unwrap();
    assert_eq!(
        steam,
        LibraryInput::SetFilter(SidebarSelection::Provider("steam".into()))
    );
}

#[test]
fn catalog_errors_keep_their_stored_categories() {
    use gamenode_core::library::{CatalogError, EmptyState, LibraryLayout, SidebarSelection};

    for (error, category) in [
        (CatalogError::Unreachable, "unreachable"),
        (CatalogError::RateLimited, "rateLimited"),
        (CatalogError::Server, "server"),
        (CatalogError::Expired, "expired"),
        (CatalogError::Privacy, "privacy"),
        (CatalogError::Changed, "changed"),
        (CatalogError::InvalidCredential, "invalidCredential"),
        (CatalogError::SharedUnavailable, "sharedUnavailable"),
    ] {
        assert_eq!(error.to_string(), category);
        assert_eq!(error.as_str(), category);
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::Value::String(category.to_owned())
        );
        assert_eq!(category.parse::<CatalogError>(), Ok(error));
    }
    // Stored rows keep parsing after the provider-neutral rename.
    assert_eq!(
        "invalidCookies".parse::<CatalogError>(),
        Ok(CatalogError::InvalidCredential)
    );
    assert_eq!(
        serde_json::from_str::<CatalogError>("\"invalidCookies\"").unwrap(),
        CatalogError::InvalidCredential
    );
    // Rows written by a build that still used the family name parse as the same failure.
    assert_eq!(
        "familyUnavailable".parse::<CatalogError>(),
        Ok(CatalogError::SharedUnavailable)
    );
    assert_eq!("not-a-category".parse::<CatalogError>(), Err(()));

    let empty = EmptyState::NoInstalled {
        provider: Some("steam".into()),
    };
    assert_eq!(
        serde_json::to_value(empty).unwrap(),
        serde_json::json!({ "noInstalled": { "provider": "steam" } })
    );

    let layout: LibraryLayout =
        serde_json::from_str(r#"{"filter":{"provider":"steam"},"installedOnly":true}"#).unwrap();
    assert_eq!(
        layout.legacy_sidebar,
        Some(SidebarSelection::Provider("steam".into()))
    );
    assert!(layout.effective_filters().availability.installed);
    let normalized = layout.normalized();
    assert_eq!(normalized.scope, gamenode_core::library::LibraryScope::All);
    assert_eq!(normalized.filters.stores, ["steam"]);
    let json = serde_json::to_value(normalized).unwrap();
    assert!(json.get("filter").is_none());
    assert!(json.get("sidebar").is_none());
    assert!(json.get("installedOnly").is_none());
    assert_eq!(json.get("scope"), Some(&serde_json::json!("all")));
    assert_eq!(
        json.get("filters").and_then(|f| f.get("stores")),
        Some(&serde_json::json!(["steam"]))
    );
    assert_eq!(
        json.get("filters").and_then(|f| f.get("availability")),
        Some(&serde_json::json!({
            "installed": true,
            "owned": false,
            "shared": false
        }))
    );
}

#[test]
fn query_view_types_serialize_with_the_keys_the_ui_reads() {
    let counts = FilterCounts {
        gamenode_status: gamenode_core::library::GameNodeStatusCounts {
            available: true,
            playing: 2,
            ..Default::default()
        },
        release: vec![gamenode_core::library::ReleaseCount {
            value: gamenode_core::library::ReleaseFilter::Decade(2020),
            count: 3,
        }],
        show_hidden: 1,
        ..Default::default()
    };
    let json = serde_json::to_value(&counts).unwrap();
    assert_eq!(json.get("showHidden"), Some(&serde_json::json!(1)));
    assert_eq!(
        json.get("gamenodeStatus")
            .and_then(|value| value.get("available")),
        Some(&serde_json::json!(true))
    );
    assert_eq!(
        json.get("release"),
        Some(&serde_json::json!([{ "value": { "decade": 2020 }, "count": 3 }]))
    );
    assert!(json.get("matchStates").is_some());
    assert_eq!(
        serde_json::to_value(FilterCounts::default()).unwrap(),
        serde_json::json!({
            "stores": [],
            "availability": { "installed": 0, "owned": 0, "shared": 0 },
            "matchStates": {
                "matched": 0,
                "ambiguous": 0,
                "unmatched": 0,
                "noGame": 0
            },
            "gamenodeStatus": {
                "playing": 0,
                "finished": 0,
                "planned": 0,
                "ongoing": 0,
                "dropped": 0,
                "inLibrary": 0,
                "notInLibrary": 0,
                "available": false
            },
            "playtime": { "any": 0, "atLeast10h": 0, "atLeast100h": 0 },
            "played": { "any": 0, "played": 0, "unplayed": 0 },
            "lastPlayed": {
                "any": 0,
                "last7Days": 0,
                "last30Days": 0,
                "last90Days": 0,
                "lastYear": 0,
                "olderThanYear": 0
            },
            "release": [],
            "added": {
                "any": 0,
                "last7Days": 0,
                "last30Days": 0,
                "last90Days": 0,
                "lastYear": 0,
                "olderThanYear": 0
            },
            "favoritesOnly": 0,
            "runningOnly": 0,
            "showHidden": 0
        })
    );

    assert_eq!(
        serde_json::to_value(ScopeCounts {
            all: 10,
            installed: 4,
            favorites: 2,
            recent: 1,
            running: 1,
        })
        .unwrap(),
        serde_json::json!({
            "all": 10,
            "installed": 4,
            "favorites": 2,
            "recent": 1,
            "running": 1
        })
    );
    assert_eq!(
        serde_json::to_value(RowGroup {
            id: "steam".into(),
            label_key: "Steam".into(),
            count: 3,
            collapsed: true,
            first_index: 2,
        })
        .unwrap(),
        serde_json::json!({
            "id": "steam",
            "labelKey": "Steam",
            "count": 3,
            "collapsed": true,
            "firstIndex": 2
        })
    );
    assert_eq!(
        serde_json::to_value(TitleSource {
            kind: TitleSourceKind::Store,
            id: "steam".into(),
        })
        .unwrap(),
        serde_json::json!({ "kind": "store", "id": "steam" })
    );
    assert_eq!(
        serde_json::to_value(RowGameNode {
            status: Some(EntryStatus::Playing),
            collection_ids: vec!["coop".into()],
            entry_id: "entry-1".into(),
        })
        .unwrap(),
        serde_json::json!({
            "status": "playing",
            "collectionIds": ["coop"],
            "entryId": "entry-1"
        })
    );

    // The match dialog's platform narrowing is camelCase for the UI's chip and counts.
    assert_eq!(
        serde_json::to_value(MatchPlatformFilter {
            platform: "gamecube".into(),
            label: "GameCube".into(),
            aliases: vec!["NGC".into(), "GC".into()],
            enabled: true,
        })
        .unwrap(),
        serde_json::json!({
            "platform": "gamecube",
            "label": "GameCube",
            "aliases": ["NGC", "GC"],
            "enabled": true
        })
    );
}

#[test]
fn stored_entries_and_scan_states_saved_before_the_catalog_still_load() {
    use gamenode_core::library::{ProviderScanState, StoredEntry};

    let entry: StoredEntry = serde_json::from_str(
        r#"{
            "key": "steam:620",
            "provider": "steam",
            "storeId": "620",
            "name": "Portal 2",
            "installDir": "/p",
            "fields": {},
            "installed": true,
            "firstSeen": 1,
            "lastSeen": 2
        }"#,
    )
    .unwrap();
    assert!(!entry.owned && !entry.shared);
    assert_eq!(
        serde_json::to_value(&entry).unwrap().get("owned"),
        Some(&serde_json::json!(false))
    );
    assert_eq!(
        serde_json::to_value(&entry).unwrap().get("shared"),
        Some(&serde_json::json!(false))
    );

    let state: ProviderScanState =
        serde_json::from_str(r#"{"provider":"steam","lastSuccessAt":1,"lastError":null}"#).unwrap();
    assert_eq!(state.last_catalog_at, None);
    assert_eq!(state.catalog_error, None);
    assert_eq!(state.last_shared_at, None);
    assert_eq!(state.shared_error, None);
}
