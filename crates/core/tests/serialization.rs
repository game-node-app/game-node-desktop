use std::sync::Arc;

use gamenode_core::Provider;
use gamenode_core::auth::{AuthMachine, Input};
use gamenode_core::credential::StorageMethod;
use gamenode_core::library::{
    DetailView, EntryStatus, Input as LibraryInput, MatchSource, MatchStatus, Membership, Row,
    SelectMode, SidebarSelection, Sort, SortDirection, SortKey, ViewMode,
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
        },
        running: false,
        key: "steam:1091500".into(),
        title: "Cyberpunk 2077".into(),
        detected_name: "Cyberpunk 2077".into(),
        matched_name: Some("Cyberpunk 2077".into()),
        provider: "steam".into(),
        provider_name: "Steam".into(),
        status: MatchStatus::Matched {
            game_id: 1091500,
            source: MatchSource::Chosen,
        },
        cover_file: Some("1091500.jpg".into()),
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
