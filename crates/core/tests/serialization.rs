use std::sync::Arc;

use gamenode_core::Provider;
use gamenode_core::auth::{AuthMachine, Input};
use gamenode_core::credential::StorageMethod;
use gamenode_core::library::{
    DetailView, EntryStatus, Input as LibraryInput, LibraryFilters, MatchSource, MatchStatus,
    Membership, Row, SelectMode, SidebarSelection, Sort, SortDirection, SortKey, ViewMode,
};
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
    };

    let mut snapshot = Snapshot::new(&machine, StorageMethod::SecretService, settings, None, None);
    snapshot.tracking = TrackingView {
        title: "Test Game".into(),
        available: true,
        running: ["game_1".into()].into_iter().collect(),
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
    assert!(value.get("tools").is_some());

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
        release_year: Some(2020),
        added_at: 1_700_000_000,
        is_favorite: false,
    });

    let raw_bytes: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    let artwork_arc: Arc<[u8]> = Arc::from(raw_bytes);

    let detail = DetailView {
        running: false,
        row,
        install_dir: "C:\\Games\\Cyberpunk 2077".into(),
        release_year: Some(2020),
        has_artwork: true,
        artwork: Some(artwork_arc),
        membership: Membership::InLibrary(Some(EntryStatus::Playing)),
        achievement_backend: false,
        achievements: None,
    };

    let json_val = serde_json::to_value(&detail).expect("serialize DetailView");

    // Verify camelCase fields
    assert_eq!(
        json_val.get("installDir"),
        Some(&serde_json::json!("C:\\Games\\Cyberpunk 2077"))
    );
    assert_eq!(json_val.get("releaseYear"), Some(&serde_json::json!(2020)));
    assert_eq!(json_val.get("hasArtwork"), Some(&serde_json::json!(true)));

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
        Input::BrowserOpened { op: 42 },
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
    let browser_opened = Input::BrowserOpened { op: 99 };
    let json_val = serde_json::to_value(&browser_opened).unwrap();
    assert_eq!(
        json_val.get("browserOpened"),
        Some(&serde_json::json!({ "op": 99 }))
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
        LibraryInput::SetSort(Sort {
            key: SortKey::Title,
            direction: SortDirection::Ascending,
        }),
        LibraryInput::OpenMatch,
        LibraryInput::CloseMatch,
        LibraryInput::ScanRequested,
        LibraryInput::RequestCatalog,
        LibraryInput::SetFilters(LibraryFilters::default()),
        LibraryInput::SetSidebarCollapsed(true),
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
    let filters =
        serde_json::to_value(LibraryInput::SetFilters(LibraryFilters::default())).unwrap();
    assert_eq!(
        filters.get("setFilters"),
        Some(&serde_json::json!({
            "availability": { "installed": false, "owned": false, "shared": false },
            "runningOnly": false,
            "matchState": "any",
            "playtime": "any",
            "lastPlayed": "any",
            "release": "any",
            "added": "any",
            "favoritesOnly": false
        }))
    );
    assert_eq!(
        serde_json::to_value(LibraryInput::RequestCatalog).unwrap(),
        serde_json::json!("requestCatalog")
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
    assert_eq!(layout.sidebar, SidebarSelection::Provider("steam".into()));
    assert!(layout.effective_filters().availability.installed);
    let json = serde_json::to_value(layout.normalized()).unwrap();
    assert_eq!(
        json.get("filter"),
        Some(&serde_json::json!({ "provider": "steam" }))
    );
    assert!(json.get("sidebar").is_none());
    assert!(json.get("installedOnly").is_none());
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
