//! Headless checks of the library screens: keyboard and pointer input reach the `LibraryState`
//! callbacks, dialogs close on Escape, and the screens can be rendered for visual review.
//!
//! Slint allows one platform per process and keeps windows on the thread that made them, so
//! everything runs in the single test below. Set `GAMENODE_SHOTS=<dir>` to also write one PPM
//! screenshot per step.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gamenode_app::{
    AccessKind, AddError, AppWindow, CollectionItem, CollectionsKind, EmptyKind, GameItem,
    LibraryState, MatchKind, OptionItem, ProviderItem, SettingsState, SettingsTopic, StatusKind,
    SummaryItem, TargetItem, ViewKind, Why,
};
use slint::platform::software_renderer::{
    MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType,
};
use slint::platform::{Key, Platform, PlatformError, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, ModelRc, PhysicalSize, SharedString, VecModel};

const WIDTH: u32 = 1100;
const HEIGHT: u32 = 700;

struct Headless(Rc<MinimalSoftwareWindow>, std::time::Instant);

impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.0.clone())
    }
    fn duration_since_start(&self) -> Duration {
        self.1.elapsed()
    }
}

type Log = Rc<RefCell<Vec<String>>>;

fn key(ui: &AppWindow, key: Key) {
    press(ui, key.into());
}

fn click(ui: &AppWindow, x: f32, y: f32) {
    let position = LogicalPosition::new(x, y);
    ui.window()
        .dispatch_event(WindowEvent::PointerMoved { position });
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: slint::platform::PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: slint::platform::PointerEventButton::Left,
    });
    slint::platform::update_timers_and_animations();
}

fn press(ui: &AppWindow, text: SharedString) {
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

fn chord(ui: &AppWindow, modifier: Key, text: SharedString) {
    let modifier: SharedString = modifier.into();
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: modifier.clone(),
    });
    press(ui, text);
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: modifier });
}

fn take(log: &Log) -> Vec<String> {
    std::mem::take(&mut *log.borrow_mut())
}

fn game(key: &str, title: &str, provider: &str, kind: MatchKind) -> GameItem {
    GameItem {
        playtime_known: true,
        playtime_hours: 12,
        playtime_minutes: 34,
        playtime_seconds: 0,
        running: false,
        key: key.into(),
        title: title.into(),
        provider: provider.into(),
        cover: Default::default(),
        has_cover: false,
        kind,
        selected: false,
        focused: false,
    }
}

struct Shots {
    dir: Option<std::path::PathBuf>,
    window: Rc<MinimalSoftwareWindow>,
}

impl Shots {
    fn take(&self, name: &str) {
        // Let transitions finish so the picture shows the settled state.
        std::thread::sleep(Duration::from_millis(400));
        slint::platform::update_timers_and_animations();
        let mut buffer = vec![PremultipliedRgbaColor::default(); (WIDTH * HEIGHT) as usize];
        self.window.request_redraw();
        let drawn = self.window.draw_if_needed(|renderer| {
            renderer.render(&mut buffer, WIDTH as usize);
        });
        assert!(drawn, "{name}: nothing to draw");
        let Some(dir) = &self.dir else { return };
        let mut bytes = format!("P6\n{WIDTH} {HEIGHT}\n255\n").into_bytes();
        for pixel in &buffer {
            bytes.extend_from_slice(&[pixel.red, pixel.green, pixel.blue]);
        }
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(format!("{name}.ppm")), bytes).unwrap();
    }
}

/// A window showing a small library, with every `LibraryState` callback recorded in `log`.
fn open(log: &Log) -> AppWindow {
    let ui = AppWindow::new().unwrap();
    ui.set_signed_in(true);
    ui.set_version("0.1.0".into());
    ui.show().unwrap();
    ui.window()
        .dispatch_event(WindowEvent::WindowActiveChanged(true));

    let state = ui.global::<LibraryState>();
    macro_rules! record {
        ($on:ident, $name:literal) => {{
            let log = log.clone();
            state.$on(move || log.borrow_mut().push($name.into()));
        }};
    }
    let note = |log: &Log| {
        let log = log.clone();
        move |line: String| log.borrow_mut().push(line)
    };
    let n = note(log);
    state.on_select(move |k, ctrl, shift| n(format!("select {k} ctrl={ctrl} shift={shift}")));
    let n = note(log);
    state.on_move(move |d, extend| n(format!("move {d} extend={extend}")));
    record!(on_open_details, "open-details");
    record!(on_play, "play");
    record!(on_install, "install");
    record!(on_rescan, "rescan");
    record!(on_open_match, "open-match");
    record!(on_open_add, "open-add");
    record!(on_close_match, "close-match");
    record!(on_close_add, "close-add");
    record!(on_match_search, "match-search");
    record!(on_mark_no_game, "mark-no-game");
    record!(on_add_confirm, "add-confirm");
    let n = note(log);
    state.on_set_search(move |text| n(format!("search {text}")));
    let n = note(log);
    state.on_set_view(move |v| n(format!("view {}", v == ViewKind::List)));
    let n = note(log);
    state.on_set_filter(move |id| n(format!("filter {id}")));
    let n = note(log);
    state.on_pick_match(move |id| n(format!("pick {id}")));

    let items = Rc::new(VecModel::from(vec![
        game("steam:1", "Portal 2", "Steam", MatchKind::Matched),
        game("steam:2", "Half-Life: Alyx", "Steam", MatchKind::Ambiguous),
        game(
            "steam:3",
            "Some Obscure Tool",
            "Steam",
            MatchKind::Unmatched,
        ),
        game("steam:4", "Valheim", "Steam", MatchKind::NoGame),
    ]));
    state.set_items(ModelRc::from(items));
    state.set_providers(ModelRc::from(Rc::new(VecModel::from(vec![ProviderItem {
        id: "steam".into(),
        name: "Steam".into(),
        count: 4,
        selected: false,
        failed: false,
    }]))));
    state.set_total(4);
    state.set_all_selected(true);
    state.set_focused_index(-1);
    state.set_active(true);
    state.set_detail_valid(false);
    state.set_play_why(Why::NothingSelected);
    state.set_install_why(Why::NothingSelected);
    state.set_match_why(Why::NothingSelected);
    state.set_add_why(Why::NothingSelected);
    slint::platform::update_timers_and_animations();
    ui
}

#[test]
fn library_screens_react_to_keyboard_and_pointer() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    window.set_size(PhysicalSize::new(WIDTH, HEIGHT));
    slint::platform::set_platform(Box::new(Headless(
        window.clone(),
        std::time::Instant::now(),
    )))
    .unwrap();
    let shots = Shots {
        dir: std::env::var_os("GAMENODE_SHOTS").map(Into::into),
        window: window.clone(),
    };

    let log: Log = Rc::default();
    let ui = open(&log);
    let state = ui.global::<LibraryState>();
    shots.take("01-grid");

    // A click selects; Ctrl and Shift change the mode.
    ui.window().dispatch_event(WindowEvent::PointerMoved {
        position: LogicalPosition::new(300., 200.),
    });
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position: LogicalPosition::new(300., 200.),
        button: slint::platform::PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position: LogicalPosition::new(300., 200.),
        button: slint::platform::PointerEventButton::Left,
    });
    let events = take(&log);
    assert!(
        events
            .iter()
            .any(|e| e.starts_with("select steam:") && e.ends_with("ctrl=false shift=false")),
        "click did not select: {events:?}"
    );

    // Keyboard on the items.
    key(&ui, Key::DownArrow);
    key(&ui, Key::RightArrow);
    key(&ui, Key::Home);
    key(&ui, Key::End);
    key(&ui, Key::UpArrow);
    key(&ui, Key::LeftArrow);
    chord(&ui, Key::Shift, Key::DownArrow.into());
    assert_eq!(
        take(&log),
        [
            "move 1 extend=false",
            "move 3 extend=false",
            "move 4 extend=false",
            "move 5 extend=false",
            "move 0 extend=false",
            "move 2 extend=false",
            "move 1 extend=true",
        ]
    );
    key(&ui, Key::Return);
    assert_eq!(take(&log), ["open-details"]);
    chord(&ui, Key::Control, Key::Return.into());
    assert_eq!(take(&log), ["play"]);
    key(&ui, Key::F5);
    assert_eq!(take(&log), ["rescan"]);

    // Ctrl+F moves to the search box, where typing is a search.
    chord(&ui, Key::Control, "f".into());
    press(&ui, "p".into());
    let events = take(&log);
    assert_eq!(events, ["search p"], "Ctrl+F then typing: {events:?}");
    state.set_search("p".into());
    shots.take("02-search");

    // The list view and the provider filter forward to their callbacks.
    state.set_view(ViewKind::List);
    shots.take("03-list");

    // Dialogs: Escape cancels, and the screen behind stays untouched.
    state.set_match_detected("Half-Life: Alyx".into());
    state.set_match_provider("Steam".into());
    state.set_match_query("Half-Life: Alyx".into());
    state.set_match_candidates(ModelRc::from(Rc::new(VecModel::from(vec![OptionItem {
        id: "11".into(),
        name: "Half-Life: Alyx".into(),
        year: "2020".into(),
        cover: Default::default(),
        has_cover: false,
        current: true,
    }]))));
    state.set_match_results(ModelRc::from(Rc::new(VecModel::from(vec![OptionItem {
        id: "12".into(),
        name: "Half-Life 2".into(),
        year: "2004".into(),
        cover: Default::default(),
        has_cover: false,
        current: false,
    }]))));
    state.set_match_can_clear(true);
    state.set_match_open(true);
    slint::platform::update_timers_and_animations();
    shots.take("04-match");
    take(&log);
    key(&ui, Key::Escape);
    assert_eq!(take(&log), ["close-match"], "Escape in the match dialog");
    state.set_match_open(false);

    state.set_add_targets(ModelRc::from(Rc::new(VecModel::from(vec![
        TargetItem {
            title: "Portal 2".into(),
            matched: true,
        },
        TargetItem {
            title: "Some Obscure Tool".into(),
            matched: false,
        },
    ]))));
    state.set_add_status(StatusKind::Planned);
    state.set_add_collections_kind(CollectionsKind::Loaded);
    state.set_add_collections(ModelRc::from(Rc::new(VecModel::from(vec![
        CollectionItem {
            id: "c1".into(),
            name: "Backlog".into(),
            picked: false,
            forces: StatusKind::Planned,
        },
    ]))));
    state.set_add_can_confirm(true);
    state.set_add_error(AddError::None);
    state.set_add_open(true);
    slint::platform::update_timers_and_animations();
    shots.take("05-add");
    take(&log);
    key(&ui, Key::Escape);
    assert_eq!(take(&log), ["close-add"], "Escape in the add dialog");

    state.set_add_summary(ModelRc::from(Rc::new(VecModel::from(vec![SummaryItem {
        title: "Portal 2".into(),
        outcome: gamenode_app::Outcome::Added,
        status: StatusKind::Planned,
    }]))));
    state.set_add_has_summary(true);
    state.set_add_summary_added(1);
    slint::platform::update_timers_and_animations();
    shots.take("06-add-summary");
    state.set_add_open(false);
    state.set_add_has_summary(false);

    // Banner and empty states.
    state.set_access(AccessKind::Unverified);
    state.set_scanning(true);
    state.set_failed_stores("Steam".into());
    shots.take("07-banner");
    state.set_items(ModelRc::from(Rc::new(VecModel::<GameItem>::default())));
    state.set_empty(EmptyKind::NoGames);
    state.set_stores("Steam".into());
    shots.take("08-empty");
    state.set_empty(EmptyKind::NothingMatches);
    shots.take("09-nothing-matches");

    // Back to a full library with a game selected.
    state.set_empty(EmptyKind::None);
    state.set_access(AccessKind::Verified);
    state.set_scanning(false);
    state.set_failed_stores("".into());
    state.set_view(ViewKind::Grid);
    state.set_items(ModelRc::from(Rc::new(VecModel::from(vec![
        game("steam:1", "Portal 2", "Steam", MatchKind::Matched),
        game("steam:2", "Half-Life: Alyx", "Steam", MatchKind::Ambiguous),
    ]))));
    show_details(&ui);
    state.set_add_why(Why::AlreadyInLibrary);
    shots.take("02b-detail");

    // Every control can be reached with Tab and used with Enter: each stop of the focus order
    // is tried in a fresh window, and between them they must fire all of these.
    show_details(&ui);
    for _ in 0..3 {
        key(&ui, Key::Tab);
    }
    shots.take("10-focus-ring");
    take(&log);
    let mut fired = std::collections::BTreeSet::new();
    for stops in 1..=40 {
        let ui = open(&log);
        show_details(&ui);
        for _ in 0..stops {
            key(&ui, Key::Tab);
        }
        key(&ui, Key::Return);
        for line in take(&log) {
            fired.insert(line.split(' ').next().unwrap_or_default().to_owned());
        }
    }
    for expected in [
        "view",
        "rescan",
        "play",
        "install",
        "open-match",
        "open-add",
        "filter",
        "open-details",
    ] {
        assert!(
            fired.contains(expected),
            "{expected} is not reachable by keyboard: {fired:?}"
        );
    }
    // Moving focus between settings controls must not consume the first click.
    let ui = open(&log);
    ui.set_settings_open(true);
    shots.take("11-settings-account");
    for (y, topic) in [
        (188., SettingsTopic::Stores),
        (222., SettingsTopic::Backups),
        (256., SettingsTopic::Advanced),
        (290., SettingsTopic::About),
        (154., SettingsTopic::Account),
        (188., SettingsTopic::Stores),
    ] {
        click(&ui, 150., y);
        assert_eq!(
            ui.global::<SettingsState>().get_topic(),
            topic,
            "Settings tab must activate on its first click"
        );
    }
    shots.take("12-settings-stores");
    take(&log);
    click(&ui, 360., 285.);
    assert_eq!(take(&log), ["rescan"], "first click on Settings Rescan");
    let source_log = log.clone();
    ui.on_source_changed(move |store| {
        source_log.borrow_mut().push(format!("source {store}"));
    });
    ui.set_store_playtime(true);
    click(&ui, 940., 434.);
    assert_eq!(
        take(&log),
        ["source false"],
        "first click on playtime source"
    );
    click(&ui, 150., 154.);
    ui.set_sync_enabled(true);
    let sync_log = log.clone();
    ui.on_sync_changed(move |enabled| {
        sync_log.borrow_mut().push(format!("sync {enabled}"));
    });
    click(&ui, 960., 319.);
    assert_eq!(take(&log), ["sync false"], "first click on Sync toggle");
    click(&ui, 998., 64.);
    assert!(!ui.get_settings_open(), "first click must close Settings");

    let mut sync_is_keyboard_reachable = false;
    for stops in 1..=40 {
        let ui = open(&log);
        ui.global::<SettingsState>()
            .set_topic(SettingsTopic::Account);
        ui.set_sync_enabled(true);
        ui.set_settings_open(true);
        let sync_log = log.clone();
        ui.on_sync_changed(move |enabled| {
            sync_log.borrow_mut().push(format!("sync {enabled}"));
        });
        take(&log);
        for _ in 0..stops {
            key(&ui, Key::Tab);
        }
        key(&ui, Key::Return);
        if take(&log).iter().any(|line| line == "sync false") {
            sync_is_keyboard_reachable = true;
            break;
        }
    }
    assert!(
        sync_is_keyboard_reachable,
        "Sync setting is not keyboard reachable"
    );

    let mut stores_is_keyboard_reachable = false;
    for stops in 1..=40 {
        let ui = open(&log);
        ui.set_settings_open(true);
        for _ in 0..stops {
            key(&ui, Key::Tab);
        }
        key(&ui, Key::Return);
        if ui.global::<SettingsState>().get_topic() == SettingsTopic::Stores {
            stores_is_keyboard_reachable = true;
            break;
        }
    }
    assert!(
        stores_is_keyboard_reachable,
        "Stores settings are not keyboard reachable"
    );

    // The segmented source control changes with the arrow keys. Start from each opposite value
    // so the callback records a real change rather than re-emitting the selected option.
    for (baseline, key_to_change, expected) in [
        (false, Key::LeftArrow, "source true"),
        (true, Key::RightArrow, "source false"),
    ] {
        let mut source_is_keyboard_reachable = false;
        for stops in 1..=40 {
            let ui = open(&log);
            let settings = ui.global::<SettingsState>();
            settings.set_topic(SettingsTopic::Stores);
            ui.set_store_playtime(baseline);
            ui.set_settings_open(true);
            let source_log = log.clone();
            ui.on_source_changed(move |store| {
                source_log.borrow_mut().push(format!("source {store}"));
            });
            take(&log);
            for _ in 0..stops {
                key(&ui, Key::Tab);
            }
            key(&ui, key_to_change);
            if take(&log).iter().any(|line| line == expected) {
                source_is_keyboard_reachable = true;
                break;
            }
        }
        assert!(
            source_is_keyboard_reachable,
            "Settings control not keyboard reachable: {expected}"
        );
    }

    // The first-use explanation owns keyboard focus, including repeated Tab and Escape.
    let ui = open(&log);
    let dismissed = Rc::new(RefCell::new(0));
    let observed = dismissed.clone();
    ui.on_sync_dismissed(move || {
        *observed.borrow_mut() += 1;
    });
    ui.set_sync_explanation(true);
    take(&log);
    for _ in 0..8 {
        key(&ui, Key::Tab);
    }
    assert!(take(&log).is_empty(), "focus escaped the sync explanation");
    key(&ui, Key::Escape);
    assert_eq!(*dismissed.borrow(), 1);
}

/// The detail pane for one selected, matched game, with every action available.
fn show_details(ui: &AppWindow) {
    let state = ui.global::<LibraryState>();
    state.set_detail_valid(true);
    state.set_detail_playtime_known(true);
    state.set_detail_playtime_hours(12);
    state.set_detail_playtime_minutes(34);
    state.set_detail_playtime_from_store(true);
    state.set_detail_title("Portal 2".into());
    state.set_detail_detected("Portal 2".into());
    state.set_detail_matched("Portal 2".into());
    state.set_detail_provider("Steam".into());
    state.set_detail_dir("C:\\Games\\Steam\\steamapps\\common\\Portal 2".into());
    state.set_detail_year("2011".into());
    state.set_detail_kind(MatchKind::Matched);
    state.set_detail_source(gamenode_app::SourceKind::Chosen);
    state.set_detail_standing(gamenode_app::Standing::InLibrary);
    state.set_detail_standing_status(StatusKind::Playing);
    state.set_selected_count(1);
    state.set_play_why(Why::Available);
    state.set_install_why(Why::Available);
    state.set_match_why(Why::Available);
    state.set_add_why(Why::Available);
    state.set_change_match(true);
    state.set_launch_note(gamenode_app::LaunchNote::NoHandler);
}
