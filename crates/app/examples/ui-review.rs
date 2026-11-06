//! Isolated native UI review and interactive hot-reload screen server.
//! No production database, credentials, or network access.
//!
//! Modes:
//! 1. One-shot snapshot (backward-compatible):
//!    `cargo run -p gamenode-app --example ui-review -- <output.png|output.ppm> [flags...]`
//! 2. Interactive live screen server (for T3 Code preview, browser, and agent CLI):
//!    `cargo run -p gamenode-app --example ui-review -- serve [--port 4777] [flags...]`

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gamenode_app::{
    AccessKind, AddError, AppWindow, BackupRow, CollectionItem, CollectionsKind, DiscoveryRow,
    EmptyKind, GameItem, LaunchNote, LibraryState, LocalGames, MatchKind, OptionItem, Outcome,
    ProviderItem, SearchError, SettingsState, SettingsTopic, SortKind, SourceKind, Standing,
    StatusKind, SummaryItem, TargetItem, ToastKind, ToolCatalogRow, ToolTargetRow, ToolVersionRow,
    ToolsState, ViewKind, Why,
};
use image::{ColorType, ImageEncoder, codecs::png::PngEncoder};
use serde_json::{Value, json};
use slint::platform::software_renderer::{
    MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType,
};
use slint::platform::{
    Key, Platform, PlatformError, PointerEventButton, WindowAdapter, WindowEvent,
};
use slint::{
    ComponentHandle, LogicalPosition, Model, ModelRc, PhysicalSize, SharedString, VecModel,
};

const DEFAULT_WIDTH: u32 = 1100;
const DEFAULT_HEIGHT: u32 = 720;
const DEFAULT_PORT: u16 = 4777;

struct HeadlessPlatform {
    window: Rc<MinimalSoftwareWindow>,
    started: Instant,
}

impl Platform for HeadlessPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> Duration {
        self.started.elapsed()
    }
}

#[derive(Clone)]
struct SeedGame {
    key: String,
    title: String,
    detected: String,
    matched: String,
    provider_id: String,
    provider_name: String,
    dir: String,
    year: String,
    kind: MatchKind,
    source: SourceKind,
    standing: Standing,
    standing_status: StatusKind,
    hours: i32,
    minutes: i32,
    from_store: bool,
    running: bool,
    executable: String,
    working_dir: String,
    arguments: String,
    gameplay: String,
    launchers: String,
    evidence: String,
    available: bool,
}

struct DemoData {
    games: Vec<SeedGame>,
    selected_key: String,
    filter_provider: String,
    search_query: String,
    sort_key: SortKind,
    descending: bool,
    view_kind: ViewKind,
    detail_open: bool,
    preset: String,
    locale: String,
    width: u32,
    height: u32,
    frame: u64,
    build_id: u64,
    events: Vec<String>,
    pointer_pos: (f32, f32),
}

impl DemoData {
    fn new(build_id: u64) -> Self {
        let games = vec![
            SeedGame {
                key: "steam:620".into(),
                title: "Portal 2".into(),
                detected: "Portal 2".into(),
                matched: "Portal 2".into(),
                provider_id: "steam".into(),
                provider_name: "Steam".into(),
                dir: "C:\\Program Files (x86)\\Steam\\steamapps\\common\\Portal 2".into(),
                year: "2011".into(),
                kind: MatchKind::Matched,
                source: SourceKind::Chosen,
                standing: Standing::InLibrary,
                standing_status: StatusKind::Playing,
                hours: 18,
                minutes: 42,
                from_store: true,
                running: false,
                executable: String::new(),
                working_dir: String::new(),
                arguments: String::new(),
                gameplay: String::new(),
                launchers: String::new(),
                evidence: String::new(),
                available: true,
            },
            SeedGame {
                key: "manual:celeste".into(),
                title: "Celeste".into(),
                detected: "Celeste".into(),
                matched: "Celeste".into(),
                provider_id: "manual".into(),
                provider_name: "DRM-free".into(),
                dir: "D:\\Games\\Celeste".into(),
                year: "2018".into(),
                kind: MatchKind::Matched,
                source: SourceKind::Automatic,
                standing: Standing::InLibrary,
                standing_status: StatusKind::Playing,
                hours: 9,
                minutes: 15,
                from_store: false,
                running: false,
                executable: "D:\\Games\\Celeste\\Celeste.exe".into(),
                working_dir: "D:\\Games\\Celeste".into(),
                arguments: "--fullscreen".into(),
                gameplay: "D:\\Games\\Celeste\\Celeste.exe".into(),
                launchers: String::new(),
                evidence: "PE ProductName: Celeste · Manifest installDir: Celeste".into(),
                available: true,
            },
            SeedGame {
                key: "steam:546560".into(),
                title: "Half-Life: Alyx".into(),
                detected: "Half-Life: Alyx".into(),
                matched: "Half-Life: Alyx".into(),
                provider_id: "steam".into(),
                provider_name: "Steam".into(),
                dir: "C:\\Program Files (x86)\\Steam\\steamapps\\common\\Half-Life Alyx".into(),
                year: "2020".into(),
                kind: MatchKind::Ambiguous,
                source: SourceKind::None,
                standing: Standing::NotChecked,
                standing_status: StatusKind::Unknown,
                hours: 4,
                minutes: 10,
                from_store: true,
                running: false,
                executable: String::new(),
                working_dir: String::new(),
                arguments: String::new(),
                gameplay: String::new(),
                launchers: String::new(),
                evidence: String::new(),
                available: true,
            },
            SeedGame {
                key: "manual:disco".into(),
                title: "Disco Elysium".into(),
                detected: "Disco Elysium - The Final Cut".into(),
                matched: "Disco Elysium".into(),
                provider_id: "manual".into(),
                provider_name: "DRM-free".into(),
                dir: "F:\\GOG Games\\Disco Elysium".into(),
                year: "2019".into(),
                kind: MatchKind::Matched,
                source: SourceKind::Automatic,
                standing: Standing::NotInLibrary,
                standing_status: StatusKind::Unknown,
                hours: 2,
                minutes: 5,
                from_store: false,
                running: false,
                executable: "F:\\GOG Games\\Disco Elysium\\disco.exe".into(),
                working_dir: "F:\\GOG Games\\Disco Elysium".into(),
                arguments: String::new(),
                gameplay: "F:\\GOG Games\\Disco Elysium\\disco.exe".into(),
                launchers: "F:\\GOG Games\\Disco Elysium\\GalaxyClient.exe".into(),
                evidence: "PEFileDescription: Disco Elysium".into(),
                available: false,
            },
            SeedGame {
                key: "steam:999".into(),
                title: "Some Obscure SDK Tool".into(),
                detected: "Some Obscure SDK Tool".into(),
                matched: String::new(),
                provider_id: "steam".into(),
                provider_name: "Steam".into(),
                dir: "C:\\Program Files (x86)\\Steam\\steamapps\\common\\SDKTool".into(),
                year: String::new(),
                kind: MatchKind::Unmatched,
                source: SourceKind::None,
                standing: Standing::NotChecked,
                standing_status: StatusKind::Unknown,
                hours: 0,
                minutes: 25,
                from_store: true,
                running: false,
                executable: String::new(),
                working_dir: String::new(),
                arguments: String::new(),
                gameplay: String::new(),
                launchers: String::new(),
                evidence: String::new(),
                available: true,
            },
        ];

        Self {
            games,
            selected_key: "steam:620".into(),
            filter_provider: String::new(),
            search_query: String::new(),
            sort_key: SortKind::Title,
            descending: false,
            view_kind: ViewKind::Grid,
            detail_open: true,
            preset: "library".into(),
            locale: "en".into(),
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            frame: 1,
            build_id,
            events: Vec::new(),
            pointer_pos: (300.0, 200.0),
        }
    }

    fn push_event(&mut self, event: impl Into<String>) {
        self.events.push(event.into());
        if self.events.len() > 25 {
            self.events.remove(0);
        }
    }
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn setup_base_window(ui: &AppWindow) {
    let notices = concat!(
        include_str!("../../../LICENSE"),
        "\n\n",
        include_str!("../../../licenses/README.md"),
        "\n\n",
        include_str!("../../../licenses/Roboto-OFL-1.1.txt"),
        "\n\n",
        include_str!("../../../licenses/THIRD-PARTY-NOTICES.txt"),
    );
    let characters: Vec<char> = notices.chars().collect();
    let pages: Vec<SharedString> = characters
        .chunks(2400)
        .map(|chunk| chunk.iter().collect::<String>().into())
        .collect();
    ui.set_license_page_count(pages.len() as i32);
    if let Some(first) = pages.first() {
        ui.set_licenses(first.clone());
    }
    let weak = ui.as_weak();
    ui.on_license_page_changed(move |page| {
        if let Some(ui) = weak.upgrade()
            && let Some(text) = pages.get(page as usize)
        {
            ui.set_license_page(page);
            ui.set_licenses(text.clone());
        }
    });

    ui.set_version("0.1.0-dev".into());
    ui.set_identity("cassi".into());
    ui.set_active_software(true);
    ui.set_fallback_renderer(false);
    ui.set_background_supported(true);
    ui.set_storage("Session storage: Windows Credential Manager".into());

    let local = ui.global::<LocalGames>();
    local.set_supported(true);
    local.set_automatic(true);
    local.set_game_automatic(true);
    local.set_save_status(2);
    local.set_job_status(4);
    local.set_destination("C:\\Users\\cassi\\AppData\\Local\\GameNode\\Desktop\\backups".into());
    local.set_profile_title("Celeste".into());
    local.set_save_paths(ModelRc::from(Rc::new(VecModel::from(vec![
        "D:\\Games\\Celeste\\Saves\\0.celeste".into(),
        "D:\\Games\\Celeste\\Saves\\settings.celeste".into(),
    ]))));
    local.set_save_registry(ModelRc::from(Rc::new(VecModel::from(vec![
        "HKEY_CURRENT_USER\\Software\\MattMakesGames\\Celeste".into(),
    ]))));
    local.set_history(ModelRc::from(Rc::new(VecModel::from(vec![
        BackupRow {
            key: "backup-v3".into(),
            label: "2026-11-05 11:30".into(),
            size: "47.0 KB".into(),
            destination: "C:\\Users\\cassi\\AppData\\Local\\GameNode\\Desktop\\backups".into(),
        },
        BackupRow {
            key: "backup-v2".into(),
            label: "2026-11-04 22:15".into(),
            size: "46.8 KB".into(),
            destination: "C:\\Users\\cassi\\AppData\\Local\\GameNode\\Desktop\\backups".into(),
        },
        BackupRow {
            key: "backup-v1".into(),
            label: "2026-11-03 19:04".into(),
            size: "43.2 KB".into(),
            destination: "C:\\Users\\cassi\\AppData\\Local\\GameNode\\Desktop\\backups".into(),
        },
    ]))));
    local.set_watches(ModelRc::from(Rc::new(VecModel::from(vec![DiscoveryRow {
        fingerprint: "watch-1".into(),
        title: String::new().into(),
        folder: "D:\\Games".into(),
        executable: String::new().into(),
        targets: Default::default(),
        picked: false,
        resolved: true,
    }]))));
    local.set_candidates(ModelRc::from(Rc::new(VecModel::from(vec![
        DiscoveryRow {
            fingerprint: "cand-hades".into(),
            title: "Hades".into(),
            folder: "D:\\Games\\Hades".into(),
            executable: "D:\\Games\\Hades\\x64\\Hades.exe".into(),
            targets: Default::default(),
            picked: true,
            resolved: true,
        },
        DiscoveryRow {
            fingerprint: "cand-hollow".into(),
            title: "Hollow Knight".into(),
            folder: "D:\\Games\\Hollow Knight".into(),
            executable: "D:\\Games\\Hollow Knight\\hollow_knight.exe".into(),
            targets: ModelRc::from(Rc::new(VecModel::from(vec![
                "D:\\Games\\Hollow Knight\\hollow_knight.exe".into(),
                "D:\\Games\\Hollow Knight\\hollow_knight_64.exe".into(),
            ]))),
            picked: false,
            resolved: false,
        },
    ]))));
}

fn sync_library_view(ui: &AppWindow, data: &DemoData) {
    let state = ui.global::<LibraryState>();
    let local = ui.global::<LocalGames>();

    let mut filtered: Vec<&SeedGame> = data
        .games
        .iter()
        .filter(|g| {
            (data.filter_provider.is_empty() || g.provider_id == data.filter_provider)
                && (data.search_query.is_empty()
                    || g.title
                        .to_ascii_lowercase()
                        .contains(&data.search_query.to_ascii_lowercase()))
        })
        .collect();

    filtered.sort_by(|a, b| {
        let ord = match data.sort_key {
            SortKind::Title => a.title.cmp(&b.title),
            SortKind::Provider => a
                .provider_name
                .cmp(&b.provider_name)
                .then_with(|| a.title.cmp(&b.title)),
            SortKind::MatchStatus => (a.kind as i32)
                .cmp(&(b.kind as i32))
                .then_with(|| a.title.cmp(&b.title)),
        };
        if data.descending { ord.reverse() } else { ord }
    });

    let mut focused_index = -1;
    let items: Vec<GameItem> = filtered
        .iter()
        .enumerate()
        .map(|(idx, g)| {
            let selected = g.key == data.selected_key;
            if selected {
                focused_index = idx as i32;
            }
            GameItem {
                playtime_known: true,
                playtime_hours: g.hours,
                playtime_minutes: g.minutes,
                playtime_seconds: 0,
                running: g.running,
                key: g.key.clone().into(),
                title: g.title.clone().into(),
                provider: g.provider_name.clone().into(),
                cover: Default::default(),
                has_cover: false,
                kind: g.kind,
                selected,
                focused: selected,
            }
        })
        .collect();

    let steam_count = data
        .games
        .iter()
        .filter(|g| g.provider_id == "steam")
        .count() as i32;
    let manual_count = data
        .games
        .iter()
        .filter(|g| g.provider_id == "manual")
        .count() as i32;

    let existing = state.get_items();
    if existing.row_count() == items.len() {
        for (i, item) in items.into_iter().enumerate() {
            if existing.row_data(i).as_ref() != Some(&item) {
                existing.set_row_data(i, item);
            }
        }
    } else {
        state.set_items(ModelRc::from(Rc::new(VecModel::from(items))));
    }
    state.set_providers(ModelRc::from(Rc::new(VecModel::from(vec![
        ProviderItem {
            id: "steam".into(),
            name: "Steam".into(),
            count: steam_count,
            selected: data.filter_provider == "steam",
            failed: false,
        },
        ProviderItem {
            id: "manual".into(),
            name: "DRM-free".into(),
            count: manual_count,
            selected: data.filter_provider == "manual",
            failed: false,
        },
    ]))));
    state.set_total(data.games.len() as i32);
    state.set_all_selected(data.filter_provider.is_empty());
    state.set_focused_index(focused_index);
    state.set_view(data.view_kind);
    state.set_sort(data.sort_key);
    state.set_descending(data.descending);
    state.set_detail_open(data.detail_open);

    if data.games.is_empty() {
        state.set_empty(EmptyKind::NoGames);
        state.set_stores("Steam, DRM-free".into());
    } else if filtered.is_empty() {
        state.set_empty(EmptyKind::NothingMatches);
    } else {
        state.set_empty(EmptyKind::None);
    }

    if let Some(selected) = data.games.iter().find(|g| g.key == data.selected_key) {
        state.set_selected_count(1);
        state.set_detail_valid(true);
        state.set_detail_running(selected.running);
        state.set_detail_playtime_known(true);
        state.set_detail_playtime_hours(selected.hours);
        state.set_detail_playtime_minutes(selected.minutes);
        state.set_detail_playtime_seconds(0);
        state.set_detail_playtime_from_store(selected.from_store);
        state.set_detail_title(selected.title.clone().into());
        state.set_detail_detected(selected.detected.clone().into());
        state.set_detail_matched(selected.matched.clone().into());
        state.set_detail_provider(selected.provider_name.clone().into());
        state.set_detail_dir(selected.dir.clone().into());
        state.set_detail_year(selected.year.clone().into());
        state.set_detail_kind(selected.kind);
        state.set_detail_source(selected.source);
        state.set_detail_standing(selected.standing);
        state.set_detail_standing_status(selected.standing_status);

        state.set_play_why(if !selected.available {
            Why::MissingField
        } else if selected.running {
            Why::Running
        } else {
            Why::Available
        });
        state.set_install_why(if selected.provider_id == "manual" {
            Why::NoInstallRule
        } else {
            Why::Available
        });
        state.set_match_why(Why::Available);
        state.set_change_match(selected.kind == MatchKind::Matched);

        if selected.standing == Standing::InLibrary {
            state.set_edit_existing(true);
            state.set_add_why(Why::Available);
            state.set_add_existing_status(selected.standing_status);
        } else if selected.kind == MatchKind::Matched {
            state.set_edit_existing(false);
            state.set_add_why(Why::Available);
            state.set_add_existing_status(StatusKind::Unknown);
        } else {
            state.set_edit_existing(false);
            state.set_add_why(Why::NotMatched);
            state.set_add_existing_status(StatusKind::Unknown);
        }

        if let Some(local_id) = selected.key.strip_prefix("manual:") {
            local.set_selected_id(local_id.into());
            local.set_title(selected.title.clone().into());
            local.set_executable(selected.executable.clone().into());
            local.set_folder(selected.dir.clone().into());
            local.set_working_directory(selected.working_dir.clone().into());
            local.set_arguments(selected.arguments.clone().into());
            local.set_gameplay(selected.gameplay.clone().into());
            local.set_launchers(selected.launchers.clone().into());
            local.set_match_evidence(selected.evidence.clone().into());
            if selected.key == "manual:celeste" {
                local.set_candidate_targets(ModelRc::from(Rc::new(VecModel::from(vec![
                    "D:\\Games\\Celeste\\Celeste.exe".into(),
                    "D:\\Games\\Celeste\\Celeste-x86.exe".into(),
                ]))));
            } else {
                local.set_candidate_targets(Default::default());
            }
            if !selected.available {
                local.set_error(
                    "Executable not found at the registered path. Use Locate executable to repair."
                        .into(),
                );
            } else {
                local.set_error("".into());
            }
        } else {
            local.set_selected_id("".into());
            local.set_match_evidence("".into());
            local.set_error("".into());
            local.set_candidate_targets(Default::default());
        }
    } else {
        state.set_selected_count(0);
        state.set_detail_valid(false);
        state.set_play_why(Why::NothingSelected);
        state.set_install_why(Why::NothingSelected);
        state.set_match_why(Why::NothingSelected);
        state.set_add_why(Why::NothingSelected);
        local.set_selected_id("".into());
        local.set_candidate_targets(Default::default());
    }
}

fn wire_callbacks(ui: &AppWindow, data: &Rc<RefCell<DemoData>>) {
    let state = ui.global::<LibraryState>();
    let local = ui.global::<LocalGames>();

    {
        let weak = ui.as_weak();
        let data = data.clone();
        state.on_select(move |key, ctrl, shift| {
            let mut d = data.borrow_mut();
            d.push_event(format!("select {key} ctrl={ctrl} shift={shift}"));
            d.selected_key = key.to_string();
            if let Some(ui) = weak.upgrade() {
                sync_library_view(&ui, &d);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        state.on_activate(move |key| {
            let mut d = data.borrow_mut();
            if !key.is_empty() {
                d.selected_key = key.to_string();
            }
            let target = d.selected_key.clone();
            d.push_event(format!("activate {target}"));
            let playable = d
                .games
                .iter()
                .find(|g| g.key == target)
                .is_some_and(|g| g.available && !g.running);
            if !playable {
                d.detail_open = true;
            }
            if let Some(ui) = weak.upgrade() {
                sync_library_view(&ui, &d);
                if playable {
                    ui.global::<LibraryState>()
                        .set_launch_note(LaunchNote::OpeningPlay);
                }
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        state.on_move(move |dir, extend| {
            let mut d = data.borrow_mut();
            d.push_event(format!("move {dir} extend={extend}"));
            let keys: Vec<String> = d.games.iter().map(|g| g.key.clone()).collect();
            if !keys.is_empty() {
                let current = keys.iter().position(|k| k == &d.selected_key).unwrap_or(0);
                let next = match dir {
                    0 | 2 => current.saturating_sub(1),
                    1 | 3 => (current + 1).min(keys.len() - 1),
                    4 => 0,
                    5 => keys.len() - 1,
                    _ => current,
                };
                d.selected_key = keys[next].clone();
            }
            if let Some(ui) = weak.upgrade() {
                sync_library_view(&ui, &d);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        state.on_set_view(move |view| {
            let mut d = data.borrow_mut();
            d.view_kind = view;
            d.push_event(format!(
                "view {}",
                if view == ViewKind::List {
                    "list"
                } else {
                    "grid"
                }
            ));
            if let Some(ui) = weak.upgrade() {
                sync_library_view(&ui, &d);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        state.on_set_filter(move |provider| {
            let mut d = data.borrow_mut();
            d.filter_provider = provider.to_string();
            d.push_event(format!("filter {provider}"));
            if let Some(ui) = weak.upgrade() {
                sync_library_view(&ui, &d);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        state.on_set_search(move |query| {
            let mut d = data.borrow_mut();
            d.search_query = query.to_string();
            d.push_event(format!("search {query}"));
            if let Some(ui) = weak.upgrade() {
                sync_library_view(&ui, &d);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        state.on_set_sort(move |key, desc| {
            let mut d = data.borrow_mut();
            d.sort_key = key;
            d.descending = desc;
            d.push_event(format!("sort desc={desc}"));
            if let Some(ui) = weak.upgrade() {
                sync_library_view(&ui, &d);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        state.on_set_detail_open(move |open| {
            let mut d = data.borrow_mut();
            d.detail_open = open;
            d.push_event(format!("detail-open {open}"));
            if let Some(ui) = weak.upgrade() {
                ui.global::<LibraryState>().set_detail_open(open);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        state.on_open_details(move || {
            let mut d = data.borrow_mut();
            d.detail_open = true;
            d.push_event("open-details");
            if let Some(ui) = weak.upgrade() {
                ui.global::<LibraryState>().set_detail_open(true);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        state.on_play(move || {
            data.borrow_mut().push_event("play");
            if let Some(ui) = weak.upgrade() {
                ui.global::<LibraryState>()
                    .set_launch_note(LaunchNote::OpeningPlay);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        state.on_install(move || {
            data.borrow_mut().push_event("install");
            if let Some(ui) = weak.upgrade() {
                ui.global::<LibraryState>()
                    .set_launch_note(LaunchNote::NoHandler);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        state.on_dismiss_launch(move || {
            data.borrow_mut().push_event("dismiss-launch");
            if let Some(ui) = weak.upgrade() {
                ui.global::<LibraryState>()
                    .set_launch_note(LaunchNote::None);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        state.on_open_match(move || {
            let mut d = data.borrow_mut();
            d.push_event("open-match");
            if let Some(ui) = weak.upgrade() {
                open_match_dialog(&ui, &d);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        state.on_close_match(move || {
            data.borrow_mut().push_event("close-match");
            if let Some(ui) = weak.upgrade() {
                ui.global::<LibraryState>().set_match_open(false);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        state.on_pick_match(move |id| {
            let mut d = data.borrow_mut();
            d.push_event(format!("pick-match {id}"));
            let selected_key = d.selected_key.clone();
            if let Some(g) = d.games.iter_mut().find(|g| g.key == selected_key) {
                g.kind = MatchKind::Matched;
                g.source = SourceKind::Chosen;
                g.matched = g.title.clone();
            }
            if let Some(ui) = weak.upgrade() {
                ui.global::<LibraryState>().set_match_open(false);
                sync_library_view(&ui, &d);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        state.on_mark_no_game(move || {
            let mut d = data.borrow_mut();
            d.push_event("mark-no-game");
            let selected_key = d.selected_key.clone();
            if let Some(g) = d.games.iter_mut().find(|g| g.key == selected_key) {
                g.kind = MatchKind::NoGame;
                g.source = SourceKind::Chosen;
            }
            if let Some(ui) = weak.upgrade() {
                ui.global::<LibraryState>().set_match_open(false);
                sync_library_view(&ui, &d);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        state.on_open_add(move || {
            let mut d = data.borrow_mut();
            d.push_event("open-add");
            if let Some(ui) = weak.upgrade() {
                open_add_dialog(&ui, &d, false);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        state.on_close_add(move || {
            data.borrow_mut().push_event("close-add");
            if let Some(ui) = weak.upgrade() {
                let s = ui.global::<LibraryState>();
                s.set_add_open(false);
                s.set_add_has_summary(false);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        state.on_add_confirm(move || {
            let mut d = data.borrow_mut();
            d.push_event("add-confirm");
            if let Some(ui) = weak.upgrade() {
                open_add_dialog(&ui, &d, true);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        local.on_save(
            move |title, executable, folder, working, arguments, gameplay, launchers| {
                let mut d = data.borrow_mut();
                d.push_event(format!("local-save title={title}"));
                let selected_key = d.selected_key.clone();
                if let Some(g) = d.games.iter_mut().find(|g| g.key == selected_key) {
                    g.title = title.to_string();
                    g.detected = title.to_string();
                    g.executable = executable.to_string();
                    g.dir = folder.to_string();
                    g.working_dir = working.to_string();
                    g.arguments = arguments.to_string();
                    g.gameplay = gameplay.to_string();
                    g.launchers = launchers.to_string();
                }
                if let Some(ui) = weak.upgrade() {
                    sync_library_view(&ui, &d);
                }
            },
        );
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        local.on_backup_now(move || {
            data.borrow_mut().push_event("backup-now");
            if let Some(ui) = weak.upgrade() {
                let l = ui.global::<LocalGames>();
                l.set_job_status(4);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        local.on_restore_preview(move |key| {
            data.borrow_mut().push_event(format!("restore-preview {key}"));
            if let Some(ui) = weak.upgrade() {
                let l = ui.global::<LocalGames>();
                l.set_has_restore(true);
                l.set_restore_targets(
                    "D:\\Games\\Celeste\\Saves\\0.celeste\nD:\\Games\\Celeste\\Saves\\settings.celeste"
                        .into(),
                );
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        local.on_restore_confirm(move || {
            data.borrow_mut().push_event("restore-confirm");
            if let Some(ui) = weak.upgrade() {
                let l = ui.global::<LocalGames>();
                l.set_has_restore(false);
                l.set_restore_targets("".into());
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        local.on_restore_cancel(move || {
            data.borrow_mut().push_event("restore-cancel");
            if let Some(ui) = weak.upgrade() {
                let l = ui.global::<LocalGames>();
                l.set_has_restore(false);
                l.set_restore_targets("".into());
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        local.on_dismiss_discovered(move || {
            data.borrow_mut().push_event("dismiss-discovered");
            if let Some(ui) = weak.upgrade() {
                ui.global::<LocalGames>().set_discovered_notice("".into());
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        local.on_claim_cancel(move || {
            data.borrow_mut().push_event("claim-cancel");
            if let Some(ui) = weak.upgrade() {
                ui.global::<LocalGames>().set_claim_open(false);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        local.on_claim_resolve(move |transfer| {
            data.borrow_mut()
                .push_event(format!("claim-resolve transfer={transfer}"));
            if let Some(ui) = weak.upgrade() {
                ui.global::<LocalGames>().set_claim_open(false);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        local.on_remove(move |allow_rediscovery| {
            let mut d = data.borrow_mut();
            d.push_event(format!(
                "local-remove allow_rediscovery={allow_rediscovery}"
            ));
            let selected_key = d.selected_key.clone();
            d.games.retain(|g| g.key != selected_key);
            if let Some(first) = d.games.first() {
                d.selected_key = first.key.clone();
            } else {
                d.selected_key.clear();
            }
            if let Some(ui) = weak.upgrade() {
                ui.global::<LocalGames>().set_remove_open(false);
                sync_library_view(&ui, &d);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        ui.on_sync_dismissed(move || {
            data.borrow_mut().push_event("sync-dismissed");
            if let Some(ui) = weak.upgrade() {
                ui.set_sync_explanation(false);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        state.on_open_tools(move || {
            let mut d = data.borrow_mut();
            d.push_event("open-tools");
            if let Some(ui) = weak.upgrade() {
                open_tools_for_selected(&ui, &d);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        let tools = ui.global::<ToolsState>();
        tools.on_open_for_selected(move || {
            let mut d = data.borrow_mut();
            d.push_event("open-for-selected");
            if let Some(ui) = weak.upgrade() {
                open_tools_for_selected(&ui, &d);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        let tools = ui.global::<ToolsState>();
        tools.on_close(move || {
            data.borrow_mut().push_event("close-tools");
            if let Some(ui) = weak.upgrade() {
                ui.global::<ToolsState>().set_open(false);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        let tools = ui.global::<ToolsState>();
        tools.on_select_channel(move |channel| {
            let mut d = data.borrow_mut();
            d.push_event(format!("select-channel {channel}"));
            if let Some(ui) = weak.upgrade() {
                let t = ui.global::<ToolsState>();
                t.set_channel_index(channel);
                let versions = if channel == 1 {
                    vec![
                        ToolVersionRow {
                            version: "nightly-2024.10.25".into(),
                            label: "nightly-2024.10.25".into(),
                            published_at: "2024-10-25".into(),
                            cached: false,
                        },
                        ToolVersionRow {
                            version: "nightly-2024.10.24".into(),
                            label: "nightly-2024.10.24".into(),
                            published_at: "2024-10-24".into(),
                            cached: false,
                        },
                    ]
                } else {
                    vec![
                        ToolVersionRow {
                            version: "v0.7.9".into(),
                            label: "v0.7.9".into(),
                            published_at: "2024-10-15".into(),
                            cached: true,
                        },
                        ToolVersionRow {
                            version: "v0.7.8".into(),
                            label: "v0.7.8".into(),
                            published_at: "2024-08-01".into(),
                            cached: false,
                        },
                    ]
                };
                t.set_versions(ModelRc::from(Rc::new(VecModel::from(versions))));
                if channel == 1 {
                    t.set_selected_version("nightly-2024.10.25".into());
                } else {
                    t.set_selected_version("v0.7.9".into());
                }
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        let tools = ui.global::<ToolsState>();
        tools.on_refresh_versions(move || {
            let mut d = data.borrow_mut();
            d.push_event("refresh-versions");
            if let Some(ui) = weak.upgrade() {
                ui.global::<ToolsState>()
                    .set_status_message("Versions refreshed".into());
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        let tools = ui.global::<ToolsState>();
        tools.on_select_version(move |version| {
            let mut d = data.borrow_mut();
            d.push_event(format!("select-version {version}"));
            if let Some(ui) = weak.upgrade() {
                ui.global::<ToolsState>().set_selected_version(version);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        let tools = ui.global::<ToolsState>();
        tools.on_select_target_exe(move |target| {
            let mut d = data.borrow_mut();
            d.push_event(format!("select-target-exe {target}"));
            if let Some(ui) = weak.upgrade() {
                ui.global::<ToolsState>().set_selected_target_exe(target);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        let tools = ui.global::<ToolsState>();
        tools.on_select_proxy(move |proxy| {
            let mut d = data.borrow_mut();
            d.push_event(format!("select-proxy {proxy}"));
            if let Some(ui) = weak.upgrade() {
                ui.global::<ToolsState>().set_selected_proxy(proxy);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        let tools = ui.global::<ToolsState>();
        tools.on_toggle_anticheat_confirmed(move |confirmed| {
            let mut d = data.borrow_mut();
            d.push_event(format!("toggle-anticheat-confirmed {confirmed}"));
            if let Some(ui) = weak.upgrade() {
                ui.global::<ToolsState>().set_anticheat_confirmed(confirmed);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        let tools = ui.global::<ToolsState>();
        tools.on_toggle_reset_config(move |reset| {
            let mut d = data.borrow_mut();
            d.push_event(format!("toggle-reset-config {reset}"));
            if let Some(ui) = weak.upgrade() {
                ui.global::<ToolsState>().set_reset_config(reset);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        let tools = ui.global::<ToolsState>();
        tools.on_install_tool(move || {
            let mut d = data.borrow_mut();
            d.push_event("install-tool");
            if let Some(ui) = weak.upgrade() {
                let t = ui.global::<ToolsState>();
                t.set_installed(true);
                let ver = t.get_selected_version();
                let proxy = t.get_selected_proxy();
                t.set_installed_version(ver.clone());
                t.set_installed_proxy(proxy.clone());
                t.set_installed_channel(if t.get_channel_index() == 1 { "Nightly".into() } else { "Stable".into() });
                ui.global::<LibraryState>().set_detail_tools(format!("OptiScaler {} \u{00b7} {}", ver, proxy).into());
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        let tools = ui.global::<ToolsState>();
        tools.on_update_tool(move || {
            let mut d = data.borrow_mut();
            d.push_event("update-tool");
            if let Some(ui) = weak.upgrade() {
                let t = ui.global::<ToolsState>();
                t.set_installed(true);
                let ver = t.get_selected_version();
                let proxy = t.get_selected_proxy();
                t.set_installed_version(ver.clone());
                t.set_installed_proxy(proxy.clone());
                t.set_installed_channel(if t.get_channel_index() == 1 { "Nightly".into() } else { "Stable".into() });
                ui.global::<LibraryState>().set_detail_tools(format!("OptiScaler {} \u{00b7} {}", ver, proxy).into());
            }
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        let tools = ui.global::<ToolsState>();
        tools.on_uninstall_tool(move || {
            let mut d = data.borrow_mut();
            d.push_event("uninstall-tool");
            if let Some(ui) = weak.upgrade() {
                let t = ui.global::<ToolsState>();
                t.set_installed(false);
                t.set_installed_version("".into());
                ui.global::<LibraryState>().set_detail_tools("".into());
            }
        });
    }
    {
        let data = data.clone();
        let tools = ui.global::<ToolsState>();
        tools.on_open_source(move |url| {
            data.borrow_mut().push_event(format!("open-source {url}"));
        });
    }
    {
        let weak = ui.as_weak();
        let data = data.clone();
        let tools = ui.global::<ToolsState>();
        tools.on_clear_cache(move || {
            let mut d = data.borrow_mut();
            d.push_event("clear-cache");
            if let Some(ui) = weak.upgrade() {
                let t = ui.global::<ToolsState>();
                t.set_cache_archive_count(0);
                t.set_cache_size_text("0 B".into());
            }
        });
    }
}

fn open_tools_for_selected(ui: &AppWindow, data: &DemoData) {
    let tools = ui.global::<ToolsState>();
    let selected = data
        .games
        .iter()
        .find(|g| g.key == data.selected_key)
        .unwrap_or(&data.games[0]);
    tools.set_entry_key(selected.key.clone().into());
    tools.set_game_title(selected.title.clone().into());
    tools.set_install_dir(selected.dir.clone().into());
    tools.set_game_running(selected.running);
    tools.set_busy(false);
    tools.set_status_message("".into());
    tools.set_error_message("".into());

    let versions = vec![
        ToolVersionRow {
            version: "v0.7.9".into(),
            label: "v0.7.9".into(),
            published_at: "2024-10-15".into(),
            cached: true,
        },
        ToolVersionRow {
            version: "v0.7.8".into(),
            label: "v0.7.8".into(),
            published_at: "2024-08-01".into(),
            cached: false,
        },
    ];
    tools.set_versions(ModelRc::from(Rc::new(VecModel::from(versions))));
    tools.set_channel_index(0);
    tools.set_selected_version("v0.7.9".into());

    let targets = vec![
        ToolTargetRow {
            relative_exe: "bin/x64/Cyberpunk2077.exe".into(),
            label: "bin/x64/Cyberpunk2077.exe".into(),
            architecture: "x86_64".into(),
            preferred: true,
        },
        ToolTargetRow {
            relative_exe: "Cyberpunk2077.exe".into(),
            label: "Cyberpunk2077.exe".into(),
            architecture: "x86_64".into(),
            preferred: false,
        },
    ];
    tools.set_targets(ModelRc::from(Rc::new(VecModel::from(targets))));
    tools.set_selected_target_exe("bin/x64/Cyberpunk2077.exe".into());
    tools.set_selected_proxy("dxgi.dll".into());
    tools.set_reset_config(false);

    tools.set_open(true);
}

fn open_match_dialog(ui: &AppWindow, data: &DemoData) {
    let state = ui.global::<LibraryState>();
    let selected = data
        .games
        .iter()
        .find(|g| g.key == data.selected_key)
        .unwrap_or(&data.games[0]);
    state.set_match_detected(selected.detected.clone().into());
    state.set_match_provider(selected.provider_name.clone().into());
    state.set_match_query(selected.title.clone().into());
    state.set_match_searching(false);
    state.set_match_no_results(false);
    state.set_match_error(SearchError::None);
    state.set_match_blocked(Why::Available);
    state.set_match_can_clear(selected.kind == MatchKind::Matched);
    state.set_match_candidates(ModelRc::from(Rc::new(VecModel::from(vec![
        OptionItem {
            id: "101".into(),
            name: selected.title.clone().into(),
            year: if selected.year.is_empty() {
                "2020".into()
            } else {
                selected.year.clone().into()
            },
            cover: Default::default(),
            has_cover: false,
            current: selected.kind == MatchKind::Matched,
        },
        OptionItem {
            id: "102".into(),
            name: format!("{} — Soundtrack Edition", selected.title).into(),
            year: "2021".into(),
            cover: Default::default(),
            has_cover: false,
            current: false,
        },
    ]))));
    state.set_match_results(ModelRc::from(Rc::new(VecModel::from(vec![OptionItem {
        id: "103".into(),
        name: "Half-Life 2".into(),
        year: "2004".into(),
        cover: Default::default(),
        has_cover: false,
        current: false,
    }]))));
    state.set_match_open(true);
}

fn open_add_dialog(ui: &AppWindow, data: &DemoData, summary: bool) {
    let state = ui.global::<LibraryState>();
    let selected = data
        .games
        .iter()
        .find(|g| g.key == data.selected_key)
        .unwrap_or(&data.games[0]);
    state.set_add_targets(ModelRc::from(Rc::new(VecModel::from(vec![
        TargetItem {
            title: selected.title.clone().into(),
            matched: true,
        },
        TargetItem {
            title: "Some Obscure SDK Tool".into(),
            matched: false,
        },
    ]))));
    state.set_add_status(StatusKind::Playing);
    state.set_add_applied_status(StatusKind::Playing);
    state.set_add_collections_kind(CollectionsKind::Loaded);
    state.set_add_collections(ModelRc::from(Rc::new(VecModel::from(vec![
        CollectionItem {
            id: "col-1".into(),
            name: "Favorites".into(),
            picked: true,
            forces: StatusKind::Playing,
        },
        CollectionItem {
            id: "col-2".into(),
            name: "Backlog".into(),
            picked: false,
            forces: StatusKind::Planned,
        },
    ]))));
    state.set_add_can_confirm(true);
    state.set_add_error(AddError::None);
    state.set_add_has_summary(summary);
    if summary {
        state.set_add_summary(ModelRc::from(Rc::new(VecModel::from(vec![
            SummaryItem {
                title: selected.title.clone().into(),
                outcome: Outcome::Added,
                status: StatusKind::Playing,
            },
            SummaryItem {
                title: "Some Obscure SDK Tool".into(),
                outcome: Outcome::Skipped,
                status: StatusKind::Unknown,
            },
        ]))));
        state.set_add_summary_added(1);
        state.set_add_summary_skipped(1);
    }
    state.set_add_open(true);
}

fn reset_overlays(ui: &AppWindow) {
    ui.set_settings_opener(2);
    ui.set_settings_open(false);
    ui.set_sync_explanation(false);
    ui.set_busy(false);
    ui.set_can_retry(false);
    ui.set_status("".into());

    let state = ui.global::<LibraryState>();
    state.set_match_open(false);
    state.set_add_open(false);
    state.set_add_has_summary(false);
    state.set_scanning(false);
    state.set_failed_stores("".into());
    state.set_access(AccessKind::Verified);
    state.set_launch_note(LaunchNote::None);
    state.set_detail_tools("".into());

    let local = ui.global::<LocalGames>();
    local.set_editing(false);
    local.set_protection_open(false);
    local.set_claim_open(false);
    local.set_remove_open(false);
    local.set_has_restore(false);
    local.set_restore_targets("".into());
    local.set_discovered_notice("".into());

    let settings = ui.global::<SettingsState>();
    settings.set_open(false);
    settings.set_dirty(false);
    settings.set_alert_open(false);
    settings.set_toast_open(false);

    let tools = ui.global::<ToolsState>();
    tools.set_open(false);
    tools.set_busy(false);
    tools.set_game_running(false);
    tools.set_error_message("".into());
    tools.set_status_message("".into());
    tools.set_anticheat_detected(false);
    tools.set_anticheat_confirmed(false);
    tools.set_installed(false);
    tools.set_installed_version("".into());
}

fn apply_preset(ui: &AppWindow, data: &mut DemoData, preset: &str) {
    data.preset = preset.to_owned();
    reset_overlays(ui);

    // Ensure default library games exist if we previously cleared them for `empty`
    if data.games.is_empty() && preset != "empty" {
        let fresh = DemoData::new(data.build_id);
        data.games = fresh.games;
    }

    ui.set_signed_in(preset != "signed-out" && preset != "settings-account-signed-out");
    let state = ui.global::<LibraryState>();
    let local = ui.global::<LocalGames>();
    state.set_active(preset != "signed-out" && preset != "settings-account-signed-out");
    data.search_query.clear();
    state.set_search("".into());
    data.filter_provider.clear();

    match preset {
        "signed-out" => {
            state.set_active(false);
        }
        "library" | "grid" | "details" => {
            data.view_kind = ViewKind::Grid;
            data.selected_key = "steam:620".into();
            data.detail_open = true;
            sync_library_view(ui, data);
        }
        "list" => {
            data.view_kind = ViewKind::List;
            data.selected_key = "steam:620".into();
            data.detail_open = true;
            sync_library_view(ui, data);
        }
        "drm-detail" => {
            data.view_kind = ViewKind::Grid;
            data.selected_key = "manual:celeste".into();
            data.detail_open = true;
            sync_library_view(ui, data);
        }
        "drm-missing" => {
            data.view_kind = ViewKind::Grid;
            data.selected_key = "manual:disco".into();
            data.detail_open = true;
            sync_library_view(ui, data);
        }
        "settings" | "settings-account" => {
            sync_library_view(ui, data);
            ui.set_signed_in(true);
            ui.set_identity("cassi@example.com".into());
            let settings = ui.global::<SettingsState>();
            settings.set_open(true);
            settings.set_topic(SettingsTopic::Account);
        }
        "settings-account-signed-out" => {
            sync_library_view(ui, data);
            ui.set_signed_in(false);
            let settings = ui.global::<SettingsState>();
            settings.set_open(true);
            settings.set_topic(SettingsTopic::Account);
        }
        "settings-stores" => {
            sync_library_view(ui, data);
            let settings = ui.global::<SettingsState>();
            settings.set_open(true);
            settings.set_topic(SettingsTopic::Stores);
            local.set_supported(true);
        }
        "settings-stores-failed" => {
            sync_library_view(ui, data);
            let settings = ui.global::<SettingsState>();
            settings.set_open(true);
            settings.set_topic(SettingsTopic::Stores);
            local.set_supported(true);
            state.set_failed_stores("Steam".into());
            let providers: Vec<ProviderItem> = vec![
                ProviderItem {
                    id: "steam".into(),
                    name: "Steam".into(),
                    count: 4,
                    selected: false,
                    failed: true,
                },
                ProviderItem {
                    id: "manual".into(),
                    name: "DRM-free".into(),
                    count: 2,
                    selected: false,
                    failed: false,
                },
            ];
            state.set_providers(ModelRc::new(VecModel::from(providers)));
        }
        "settings-backups" => {
            sync_library_view(ui, data);
            let settings = ui.global::<SettingsState>();
            settings.set_open(true);
            settings.set_topic(SettingsTopic::Backups);
            local.set_supported(true);
            local.set_destination("D:\\Games\\Backups".into());
            local.set_automatic(true);
        }
        "settings-backups-dirty" => {
            sync_library_view(ui, data);
            let settings = ui.global::<SettingsState>();
            settings.set_open(true);
            settings.set_topic(SettingsTopic::Backups);
            local.set_supported(true);
            local.set_destination("D:\\Games\\Backups".into());
            settings.set_destination_edit("E:\\External\\Backups".into());
            settings.set_dirty(true);
        }
        "settings-backups-unsupported" => {
            sync_library_view(ui, data);
            let settings = ui.global::<SettingsState>();
            settings.set_open(true);
            settings.set_topic(SettingsTopic::Backups);
            local.set_supported(false);
        }
        "settings-advanced-restart" => {
            sync_library_view(ui, data);
            let settings = ui.global::<SettingsState>();
            settings.set_open(true);
            settings.set_topic(SettingsTopic::Advanced);
            ui.set_software(true);
            ui.set_active_software(false);
        }
        "settings-about" => {
            sync_library_view(ui, data);
            let settings = ui.global::<SettingsState>();
            settings.set_open(true);
            settings.set_topic(SettingsTopic::About);
            ui.set_version("0.1.0".into());
        }
        "settings-alert" => {
            sync_library_view(ui, data);
            let settings = ui.global::<SettingsState>();
            settings.set_open(true);
            settings.set_topic(SettingsTopic::Backups);
            settings.set_dirty(true);
            settings.set_alert_open(true);
        }
        "settings-toast" => {
            sync_library_view(ui, data);
            let settings = ui.global::<SettingsState>();
            settings.set_open(true);
            settings.set_topic(SettingsTopic::Backups);
            settings.invoke_trigger_toast(ToastKind::Success, "Backup destination saved".into());
        }
        "settings-compact" => {
            sync_library_view(ui, data);
            let settings = ui.global::<SettingsState>();
            settings.set_open(true);
            settings.set_topic(SettingsTopic::Account);
            data.width = 640;
            data.height = 420;
        }
        "protection" => {
            data.selected_key = "manual:celeste".into();
            sync_library_view(ui, data);
            local.set_protection_open(true);
        }
        "restore" | "restore-preview" => {
            data.selected_key = "manual:celeste".into();
            sync_library_view(ui, data);
            local.set_has_restore(true);
            local.set_restore_targets(
                "D:\\Games\\Celeste\\Saves\\0.celeste\nD:\\Games\\Celeste\\Saves\\settings.celeste"
                    .into(),
            );
            local.set_protection_open(true);
        }
        "discovery" => {
            data.selected_key = "manual:celeste".into();
            data.detail_open = true;
            sync_library_view(ui, data);
            local.set_discovered_notice("Celeste, Hollow Knight".into());
        }
        "editor" => {
            data.selected_key = "manual:celeste".into();
            sync_library_view(ui, data);
            local.set_editing(true);
        }
        "claim" | "claim-conflict" => {
            data.selected_key = "manual:celeste".into();
            sync_library_view(ui, data);
            local.set_claim_title("Celeste (Steam)".into());
            local.set_claim_open(true);
        }
        "remove" | "remove-game" => {
            data.selected_key = "manual:celeste".into();
            sync_library_view(ui, data);
            local.set_remove_open(true);
        }
        "match" | "match-dialog" => {
            data.selected_key = "steam:546560".into();
            sync_library_view(ui, data);
            open_match_dialog(ui, data);
        }
        "add" | "add-dialog" => {
            data.selected_key = "steam:620".into();
            sync_library_view(ui, data);
            open_add_dialog(ui, data, false);
        }
        "add-summary" => {
            data.selected_key = "steam:620".into();
            sync_library_view(ui, data);
            open_add_dialog(ui, data, true);
        }
        "sync-explanation" => {
            sync_library_view(ui, data);
            ui.set_sync_explanation(true);
        }
        "banner" => {
            sync_library_view(ui, data);
            state.set_scanning(true);
            state.set_failed_stores("Steam".into());
            state.set_access(AccessKind::Unverified);
            local.set_discovered_notice("Celeste, Hollow Knight".into());
            ui.set_can_retry(true);
        }
        "empty" => {
            data.games.clear();
            sync_library_view(ui, data);
        }
        "nothing-matches" => {
            data.search_query = "zzzz-no-match".into();
            state.set_search("zzzz-no-match".into());
            sync_library_view(ui, data);
        }
        "tools" => {
            data.selected_key = "steam:620".into();
            data.detail_open = true;
            sync_library_view(ui, data);
            open_tools_for_selected(ui, data);
            let tools = ui.global::<ToolsState>();
            tools.set_installed(false);
            tools.set_installed_version("".into());
            tools.set_installed_channel("Stable".into());
            tools.set_installed_proxy("dxgi.dll".into());
            tools.set_anticheat_detected(false);
            tools.set_anticheat_systems("".into());
            tools.set_anticheat_evidence("".into());
            tools.set_anticheat_confirmed(false);
            tools.set_open(true);
        }
        "tools-anticheat" => {
            data.selected_key = "steam:620".into();
            data.detail_open = true;
            sync_library_view(ui, data);
            open_tools_for_selected(ui, data);
            let tools = ui.global::<ToolsState>();
            tools.set_installed(false);
            tools.set_installed_version("".into());
            tools.set_installed_channel("Stable".into());
            tools.set_installed_proxy("dxgi.dll".into());
            tools.set_anticheat_detected(true);
            tools.set_anticheat_systems("EasyAntiCheat".into());
            tools.set_anticheat_evidence(
                "EasyAntiCheat/EasyAntiCheat_EOS_Setup.exe, EasyAntiCheat_x64.dll".into(),
            );
            tools.set_anticheat_confirmed(false);
            tools.set_open(true);
        }
        "tools-installed" => {
            data.selected_key = "steam:620".into();
            data.detail_open = true;
            sync_library_view(ui, data);
            open_tools_for_selected(ui, data);
            let tools = ui.global::<ToolsState>();
            tools.set_installed(true);
            tools.set_installed_version("v0.7.9".into());
            tools.set_installed_channel("Stable".into());
            tools.set_installed_proxy("dxgi.dll".into());
            tools.set_anticheat_detected(false);
            tools.set_anticheat_systems("".into());
            tools.set_anticheat_evidence("".into());
            tools.set_anticheat_confirmed(false);
            ui.global::<LibraryState>()
                .set_detail_tools("OptiScaler v0.7.9 · dxgi.dll".into());
            tools.set_open(true);
        }
        "settings-tools" => {
            sync_library_view(ui, data);
            let settings = ui.global::<SettingsState>();
            ui.set_settings_open(true);
            settings.set_open(true);
            settings.set_topic(SettingsTopic::Tools);
            let tools = ui.global::<ToolsState>();
            tools.set_cache_size_text("68.4 MB".into());
            tools.set_cache_archive_count(2);
            let catalog = vec![ToolCatalogRow {
                id: "optiscaler".into(),
                name: "OptiScaler".into(),
                summary: "Upscaler & frame generation middleware (DLSS / XeSS / FSR 2/3/4)".into(),
                license: "GPL-3.0".into(),
                source_url: "https://github.com/optiscaler/OptiScaler".into(),
                installed: false,
                installed_version: "".into(),
                installed_channel: "stable".into(),
                installed_proxy: "dxgi.dll".into(),
            }];
            tools.set_catalog(ModelRc::from(Rc::new(VecModel::from(catalog))));
        }
        _ => {
            sync_library_view(ui, data);
        }
    }
    slint::platform::update_timers_and_animations();
}

fn apply_locale(locale: &str, data: &mut DemoData) {
    let tag = if locale.to_ascii_lowercase().starts_with("pt") {
        "pt_BR"
    } else {
        ""
    };
    let _ = slint::select_bundled_translation(tag);
    data.locale = if tag.is_empty() {
        "en".into()
    } else {
        "pt_BR".into()
    };
}

fn key_from_name(name: &str) -> SharedString {
    match name.to_ascii_lowercase().as_str() {
        "escape" | "esc" => Key::Escape.into(),
        "return" | "enter" => Key::Return.into(),
        "tab" => Key::Tab.into(),
        "backspace" => Key::Backspace.into(),
        "delete" | "del" => Key::Delete.into(),
        "up" | "uparrow" | "arrowup" => Key::UpArrow.into(),
        "down" | "downarrow" | "arrowdown" => Key::DownArrow.into(),
        "left" | "leftarrow" | "arrowleft" => Key::LeftArrow.into(),
        "right" | "rightarrow" | "arrowright" => Key::RightArrow.into(),
        "home" => Key::Home.into(),
        "end" => Key::End.into(),
        "pageup" => Key::PageUp.into(),
        "pagedown" => Key::PageDown.into(),
        "f5" => Key::F5.into(),
        "space" => " ".into(),
        _ => name.into(),
    }
}

fn dispatch_key(ui: &AppWindow, key_name: &str, ctrl: bool, shift: bool, alt: bool) {
    let key_text = key_from_name(key_name);
    if ctrl {
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Control.into(),
        });
    }
    if shift {
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Shift.into(),
        });
    }
    if alt {
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Alt.into(),
        });
    }
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: key_text.clone(),
    });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: key_text });
    if alt {
        ui.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::Alt.into(),
        });
    }
    if shift {
        ui.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::Shift.into(),
        });
    }
    if ctrl {
        ui.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::Control.into(),
        });
    }
}

fn render_png_bytes(
    window: &MinimalSoftwareWindow,
    pixels: &mut Vec<PremultipliedRgbaColor>,
    width: u32,
    height: u32,
) -> Vec<u8> {
    slint::platform::update_timers_and_animations();
    pixels.resize((width * height) as usize, PremultipliedRgbaColor::default());
    window.request_redraw();
    window.draw_if_needed(|renderer| {
        renderer.render(pixels, width as usize);
    });
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    for p in pixels.iter() {
        rgb.extend_from_slice(&[p.red, p.green, p.blue]);
    }
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(&rgb, width, height, ColorType::Rgb8.into())
        .expect("encode PNG");
    png
}

fn write_snapshot_file(
    path: &Path,
    window: &MinimalSoftwareWindow,
    pixels: &mut Vec<PremultipliedRgbaColor>,
    width: u32,
    height: u32,
) -> std::io::Result<Vec<u8>> {
    let png = render_png_bytes(window, pixels, width, height);
    if path.extension().is_some_and(|ext| ext == "ppm") {
        let mut out = format!("P6\n{width} {height}\n255\n").into_bytes();
        for p in pixels.iter() {
            out.extend_from_slice(&[p.red, p.green, p.blue]);
        }
        std::fs::write(path, &out)?;
    } else {
        std::fs::write(path, &png)?;
    }
    Ok(png)
}

fn build_state_json(ui: &AppWindow, data: &DemoData) -> Value {
    let state = ui.global::<LibraryState>();
    let local = ui.global::<LocalGames>();

    let mut modals = Vec::new();
    if ui.get_sync_explanation() {
        modals.push("SyncExplanation");
    }
    if state.get_match_open() {
        modals.push("MatchDialog");
    }
    if state.get_add_open() {
        modals.push(if state.get_add_has_summary() {
            "AddDialog (Summary)"
        } else {
            "AddDialog"
        });
    }
    if local.get_editing() {
        modals.push("LocalGameEditor");
    }
    if local.get_protection_open() {
        modals.push(if local.get_has_restore() {
            "SaveProtection (RestorePreview)"
        } else {
            "SaveProtection"
        });
    }
    if local.get_claim_open() {
        modals.push("ClaimConflict");
    }
    if local.get_remove_open() {
        modals.push("RemoveLocalGame");
    }
    let tools = ui.global::<ToolsState>();
    if tools.get_open() {
        modals.push("tools");
    }

    let base_screen = if ui.get_settings_open() {
        "Settings / About"
    } else if state.get_active() {
        if state.get_view() == ViewKind::List {
            "Library (List)"
        } else {
            "Library (Grid)"
        }
    } else {
        "Welcome / Sign-in"
    };

    let screen_summary = if modals.is_empty() {
        base_screen.to_owned()
    } else {
        format!("{base_screen} + {}", modals.join(" + "))
    };

    json!({
        "build_id": data.build_id,
        "frame": data.frame,
        "preset": data.preset,
        "locale": data.locale,
        "width": data.width,
        "height": data.height,
        "screen_summary": screen_summary,
        "active_modals": modals,
        "window": {
            "signed_in": ui.get_signed_in(),
            "settings_open": ui.get_settings_open(),
            "sync_explanation": ui.get_sync_explanation(),
            "software": ui.get_software(),
            "sync_enabled": ui.get_sync_enabled(),
            "store_playtime": ui.get_store_playtime(),
            "identity": ui.get_identity().as_str(),
            "version": ui.get_version().as_str(),
            "status": ui.get_status().as_str(),
        },
        "library": {
            "active": state.get_active(),
            "view": if state.get_view() == ViewKind::List { "list" } else { "grid" },
            "total": state.get_total(),
            "items_count": state.get_items().row_count(),
            "selected_key": data.selected_key,
            "selected_count": state.get_selected_count(),
            "focused_index": state.get_focused_index(),
            "search": state.get_search().as_str(),
            "filter_provider": data.filter_provider,
            "detail_open": state.get_detail_open(),
            "detail_valid": state.get_detail_valid(),
            "detail_title": state.get_detail_title().as_str(),
            "detail_provider": state.get_detail_provider().as_str(),
            "detail_dir": state.get_detail_dir().as_str(),
            "detail_year": state.get_detail_year().as_str(),
            "match_open": state.get_match_open(),
            "add_open": state.get_add_open(),
            "add_has_summary": state.get_add_has_summary(),
            "scanning": state.get_scanning(),
            "failed_stores": state.get_failed_stores().as_str(),
        },
        "local_games": {
            "supported": local.get_supported(),
            "selected_id": local.get_selected_id().as_str(),
            "title": local.get_title().as_str(),
            "executable": local.get_executable().as_str(),
            "folder": local.get_folder().as_str(),
            "arguments": local.get_arguments().as_str(),
            "match_evidence": local.get_match_evidence().as_str(),
            "editing": local.get_editing(),
            "protection_open": local.get_protection_open(),
            "claim_open": local.get_claim_open(),
            "claim_title": local.get_claim_title().as_str(),
            "remove_open": local.get_remove_open(),
            "automatic": local.get_automatic(),
            "game_automatic": local.get_game_automatic(),
            "destination": local.get_destination().as_str(),
            "save_status": local.get_save_status(),
            "job_status": local.get_job_status(),
            "profile_title": local.get_profile_title().as_str(),
            "save_paths_count": local.get_save_paths().row_count(),
            "discovered_notice": local.get_discovered_notice().as_str(),
            "candidate_targets_count": local.get_candidate_targets().row_count(),
            "has_restore": local.get_has_restore(),
            "restore_targets": local.get_restore_targets().as_str(),
            "history_count": local.get_history().row_count(),
            "candidates_count": local.get_candidates().row_count(),
            "watches_count": local.get_watches().row_count(),
            "error": local.get_error().as_str(),
        },
        "tools": {
            "open": tools.get_open(),
            "installed": tools.get_installed(),
            "installed_version": tools.get_installed_version().as_str(),
            "installed_channel": tools.get_installed_channel().as_str(),
            "installed_proxy": tools.get_installed_proxy().as_str(),
            "selected_version": tools.get_selected_version().as_str(),
            "selected_target_exe": tools.get_selected_target_exe().as_str(),
            "selected_proxy": tools.get_selected_proxy().as_str(),
            "anticheat_detected": tools.get_anticheat_detected(),
            "cache_size_text": tools.get_cache_size_text().as_str(),
            "cache_archive_count": tools.get_cache_archive_count(),
        },
        "events": data.events,
    })
}

fn save_session_file(path: &Path, data: &DemoData) {
    let payload = json!({
        "preset": data.preset,
        "locale": data.locale,
        "width": data.width,
        "height": data.height,
        "selected_key": data.selected_key,
        "filter_provider": data.filter_provider,
        "search_query": data.search_query,
        "detail_open": data.detail_open,
        "view": if data.view_kind == ViewKind::List { "list" } else { "grid" },
    });
    if let Ok(text) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(path, text);
    }
}

fn load_session_file(
    path: &Path,
    ui: &AppWindow,
    window: &MinimalSoftwareWindow,
    data: &mut DemoData,
) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(val) = serde_json::from_str::<Value>(&text) else {
        return;
    };
    if let Some(locale) = val.get("locale").and_then(Value::as_str) {
        apply_locale(locale, data);
    }
    if let (Some(w), Some(h)) = (
        val.get("width").and_then(Value::as_u64),
        val.get("height").and_then(Value::as_u64),
    ) {
        let w = (w as u32).clamp(320, 3840);
        let h = (h as u32).clamp(240, 2160);
        data.width = w;
        data.height = h;
        window.set_size(PhysicalSize::new(w, h));
    }
    if let Some(preset) = val.get("preset").and_then(Value::as_str) {
        apply_preset(ui, data, preset);
    }
}

fn parse_query(query: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        map.insert(url_decode(k), url_decode(v));
    }
    map
}

fn url_decode(input: &str) -> String {
    let mut bytes = Vec::with_capacity(input.len());
    let raw = input.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'+' {
            bytes.push(b' ');
            i += 1;
        } else if raw[i] == b'%'
            && i + 2 < raw.len()
            && let Ok(hex) = std::str::from_utf8(&raw[i + 1..i + 3])
            && let Ok(val) = u8::from_str_radix(hex, 16)
        {
            bytes.push(val);
            i += 3;
        } else {
            bytes.push(raw[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn apply_query_actions(
    params: &HashMap<String, String>,
    ui: &AppWindow,
    window: &MinimalSoftwareWindow,
    data: &Rc<RefCell<DemoData>>,
) {
    if let Some(locale) = params.get("locale") {
        let mut d = data.borrow_mut();
        apply_locale(locale, &mut d);
        let loc = d.locale.clone();
        d.push_event(format!("locale={loc}"));
    }
    if let Some(size) = params.get("resize")
        && let Some((w, h)) = size.split_once('x')
        && let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>())
    {
        let w = w.clamp(320, 3840);
        let h = h.clamp(240, 2160);
        {
            let mut d = data.borrow_mut();
            d.width = w;
            d.height = h;
            d.push_event(format!("resize={w}x{h}"));
        }
        window.set_size(PhysicalSize::new(w, h));
    }
    if let Some(preset) = params.get("preset") {
        let mut d = data.borrow_mut();
        apply_preset(ui, &mut d, preset);
        d.push_event(format!("preset={preset}"));
    }
    if let Some(coords) = params.get("move")
        && let Some((x, y)) = coords.split_once(',')
        && let (Ok(x), Ok(y)) = (x.parse::<f32>(), y.parse::<f32>())
    {
        data.borrow_mut().pointer_pos = (x, y);
        ui.window().dispatch_event(WindowEvent::PointerMoved {
            position: LogicalPosition::new(x, y),
        });
        data.borrow_mut()
            .push_event(format!("move=({x:.0},{y:.0})"));
    }
    if let Some(coords) = params.get("click")
        && let Some((x, y)) = coords.split_once(',')
        && let (Ok(x), Ok(y)) = (x.parse::<f32>(), y.parse::<f32>())
    {
        let button = match params.get("button").map(String::as_str) {
            Some("right") => PointerEventButton::Right,
            Some("middle") => PointerEventButton::Middle,
            _ => PointerEventButton::Left,
        };
        let ctrl = params.get("ctrl").is_some_and(|v| v == "true" || v == "1");
        let shift = params.get("shift").is_some_and(|v| v == "true" || v == "1");
        if ctrl {
            ui.window().dispatch_event(WindowEvent::KeyPressed {
                text: Key::Control.into(),
            });
        }
        if shift {
            ui.window().dispatch_event(WindowEvent::KeyPressed {
                text: Key::Shift.into(),
            });
        }
        data.borrow_mut().pointer_pos = (x, y);
        let position = LogicalPosition::new(x, y);
        ui.window()
            .dispatch_event(WindowEvent::PointerMoved { position });
        ui.window()
            .dispatch_event(WindowEvent::PointerPressed { position, button });
        ui.window()
            .dispatch_event(WindowEvent::PointerReleased { position, button });
        slint::platform::update_timers_and_animations();
        if shift {
            ui.window().dispatch_event(WindowEvent::KeyReleased {
                text: Key::Shift.into(),
            });
        }
        if ctrl {
            ui.window().dispatch_event(WindowEvent::KeyReleased {
                text: Key::Control.into(),
            });
        }
        data.borrow_mut()
            .push_event(format!("click=({x:.0},{y:.0})"));
    }
    if let Some(coords) = params.get("dblclick")
        && let Some((x, y)) = coords.split_once(',')
        && let (Ok(x), Ok(y)) = (x.parse::<f32>(), y.parse::<f32>())
    {
        data.borrow_mut().pointer_pos = (x, y);
        let position = LogicalPosition::new(x, y);
        let button = PointerEventButton::Left;
        for _ in 0..2 {
            ui.window()
                .dispatch_event(WindowEvent::PointerMoved { position });
            ui.window()
                .dispatch_event(WindowEvent::PointerPressed { position, button });
            ui.window()
                .dispatch_event(WindowEvent::PointerReleased { position, button });
            slint::platform::update_timers_and_animations();
        }
        data.borrow_mut()
            .push_event(format!("dblclick=({x:.0},{y:.0})"));
    }
    if let Some(coords) = params.get("drag") {
        let parts: Vec<f32> = coords
            .split(',')
            .filter_map(|s| s.parse::<f32>().ok())
            .collect();
        if parts.len() == 4 {
            let (x1, y1, x2, y2) = (parts[0], parts[1], parts[2], parts[3]);
            let p1 = LogicalPosition::new(x1, y1);
            let p2 = LogicalPosition::new(x2, y2);
            ui.window()
                .dispatch_event(WindowEvent::PointerMoved { position: p1 });
            ui.window().dispatch_event(WindowEvent::PointerPressed {
                position: p1,
                button: PointerEventButton::Left,
            });
            ui.window()
                .dispatch_event(WindowEvent::PointerMoved { position: p2 });
            ui.window().dispatch_event(WindowEvent::PointerReleased {
                position: p2,
                button: PointerEventButton::Left,
            });
            slint::platform::update_timers_and_animations();
            let mut d = data.borrow_mut();
            d.pointer_pos = (x2, y2);
            d.push_event(format!("drag=({x1:.0},{y1:.0})->({x2:.0},{y2:.0})"));
        }
    }
    if let Some(scroll) = params.get("scroll") {
        let parts: Vec<f32> = scroll
            .split(',')
            .filter_map(|s| s.parse::<f32>().ok())
            .collect();
        if parts.len() == 4 {
            let (x, y, dx, dy) = (parts[0], parts[1], parts[2], parts[3]);
            ui.window().dispatch_event(WindowEvent::PointerScrolled {
                position: LogicalPosition::new(x, y),
                delta_x: dx,
                delta_y: dy,
            });
            slint::platform::update_timers_and_animations();
            data.borrow_mut()
                .push_event(format!("scroll=({dx:.0},{dy:.0}) at ({x:.0},{y:.0})"));
        }
    }
    if let Some(key_name) = params.get("key") {
        let ctrl = params.get("ctrl").is_some_and(|v| v == "true" || v == "1");
        let shift = params.get("shift").is_some_and(|v| v == "true" || v == "1");
        let alt = params.get("alt").is_some_and(|v| v == "true" || v == "1");
        dispatch_key(ui, key_name, ctrl, shift, alt);
        slint::platform::update_timers_and_animations();
        data.borrow_mut().push_event(format!("key={key_name}"));
    }
    if let Some(text) = params.get("text") {
        for ch in text.chars() {
            let s: SharedString = ch.to_string().into();
            ui.window()
                .dispatch_event(WindowEvent::KeyPressed { text: s.clone() });
            ui.window()
                .dispatch_event(WindowEvent::KeyReleased { text: s });
        }
        slint::platform::update_timers_and_animations();
        data.borrow_mut().push_event(format!("text={text:?}"));
    }
    data.borrow_mut().frame += 1;
}

fn respond(mut stream: TcpStream, status: &str, content_type: &str, body: &[u8]) {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

const STUDIO_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>GameNode Live Screen Studio</title>
<style>
  :root {
    --bg: #0f1115;
    --panel: #171a21;
    --border: #2a2f3a;
    --text: #e6e9ef;
    --muted: #939ab0;
    --accent: #6366f1;
    --ok: #22c55e;
  }
  * { box-sizing: border-box; }
  body {
    margin: 0;
    background: var(--bg);
    color: var(--text);
    font-family: system-ui, -apple-system, sans-serif;
    font-size: 13px;
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow: hidden;
  }
  header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 14px;
    background: var(--panel);
    border-bottom: 1px solid var(--border);
    flex-wrap: wrap;
  }
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 3px 9px;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 600;
    background: rgba(34, 197, 94, 0.14);
    color: var(--ok);
    border: 1px solid rgba(34, 197, 94, 0.35);
  }
  .badge.reloading {
    background: rgba(245, 158, 11, 0.16);
    color: #fbbf24;
    border-color: rgba(245, 158, 11, 0.4);
  }
  .summary {
    font-weight: 600;
    color: #fff;
  }
  .coords {
    margin-left: auto;
    color: var(--muted);
    font-family: monospace;
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
    padding: 7px 14px;
    background: #13161c;
    border-bottom: 1px solid var(--border);
    align-items: center;
  }
  .group-label {
    color: var(--muted);
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin: 0 4px 0 6px;
  }
  button {
    background: #202531;
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 4px 8px;
    font-size: 12px;
    cursor: pointer;
  }
  button:hover {
    background: #2b3242;
    border-color: #434c63;
  }
  button.active {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }
  main {
    display: flex;
    flex: 1;
    min-height: 0;
  }
  .stage {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 10px;
    overflow: auto;
    background: #0a0c0f;
    min-width: 0;
  }
  .screen-wrap {
    position: relative;
    border: 1px solid var(--border);
    box-shadow: 0 12px 36px rgba(0,0,0,0.6);
    background: #161616;
    line-height: 0;
    max-width: 100%;
  }
  #screen {
    display: block;
    max-width: 100%;
    max-height: calc(100vh - 170px);
    width: auto;
    height: auto;
    cursor: crosshair;
    user-select: none;
    -webkit-user-drag: none;
  }
  aside {
    width: 300px;
    background: var(--panel);
    border-left: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    overflow: auto;
    padding: 10px;
    gap: 8px;
  }
  aside h3 {
    margin: 0;
    font-size: 12px;
    text-transform: uppercase;
    color: var(--muted);
  }
  pre {
    margin: 0;
    padding: 8px;
    background: #0d0f14;
    border: 1px solid var(--border);
    border-radius: 6px;
    font-size: 11px;
    color: #cbd5e1;
    overflow: auto;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .input-row {
    display: flex;
    gap: 5px;
  }
  input[type="text"] {
    flex: 1;
    background: #0d0f14;
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 4px 7px;
    font-size: 12px;
  }
</style>
</head>
<body>
<header>
  <span id="status-badge" class="badge">Live Screen + Hot Reload</span>
  <span id="screen-summary" class="summary">Loading…</span>
  <span id="meta-info" style="color:var(--muted)"></span>
  <span id="cursor-coords" class="coords">(0, 0)</span>
</header>
<div class="toolbar" role="toolbar" aria-label="Screen Presets">
  <span class="group-label">Screens:</span>
  <button onclick="act({preset:'signed-out'})">Signed Out</button>
  <button onclick="act({preset:'library'})">Library Grid</button>
  <button onclick="act({preset:'list'})">Library List</button>
  <button onclick="act({preset:'drm-detail'})">DRM-Free Detail</button>
  <button onclick="act({preset:'drm-missing'})">DRM-Free Missing</button>
  <button onclick="act({preset:'settings'})">Settings</button>
  <span class="group-label">Modals:</span>
  <button onclick="act({preset:'tools'})">Tools Modal</button>
  <button onclick="act({preset:'tools-anticheat'})">Tools Anti-Cheat</button>
  <button onclick="act({preset:'tools-installed'})">Tools Installed</button>
  <button onclick="act({preset:'settings-tools'})">Settings Tools</button>
  <button onclick="act({preset:'protection'})">Save Protection</button>
  <button onclick="act({preset:'restore'})">Restore Preview</button>
  <button onclick="act({preset:'discovery'})">Watched / Discovery</button>
  <button onclick="act({preset:'editor'})">Game Editor</button>
  <button onclick="act({preset:'claim'})">Claim Conflict</button>
  <button onclick="act({preset:'match'})">Match Dialog</button>
  <button onclick="act({preset:'add'})">Add Dialog</button>
  <button onclick="act({preset:'add-summary'})">Add Summary</button>
  <button onclick="act({preset:'sync-explanation'})">Sync Explain</button>
  <span class="group-label">States:</span>
  <button onclick="act({preset:'banner'})">Banner / Unverified</button>
  <button onclick="act({preset:'empty'})">Empty Library</button>
  <button onclick="act({preset:'nothing-matches'})">No Search Match</button>
</div>
<div class="toolbar" role="toolbar" aria-label="Locale, Size & Keys">
  <span class="group-label">Locale:</span>
  <button onclick="act({locale:'en'})">EN</button>
  <button onclick="act({locale:'pt_BR'})">pt_BR</button>
  <span class="group-label">Size:</span>
  <button onclick="act({resize:'1100x720'})">1100×720</button>
  <button onclick="act({resize:'640x420'})">640×420 (Compact)</button>
  <button onclick="act({resize:'1440x900'})">1440×900</button>
  <span class="group-label">Keys:</span>
  <button onclick="act({key:'Escape'})">Esc</button>
  <button onclick="act({key:'Tab'})">Tab</button>
  <button onclick="act({key:'Tab',shift:'true'})">Shift+Tab</button>
  <button onclick="act({key:'Return'})">Enter</button>
  <button onclick="act({key:'UpArrow'})">↑</button>
  <button onclick="act({key:'DownArrow'})">↓</button>
  <button onclick="act({key:'LeftArrow'})">←</button>
  <button onclick="act({key:'RightArrow'})">→</button>
  <button onclick="act({key:'F5'})">F5</button>
  <button onclick="act({scroll:'550,360,0,-160'})">Scroll ↓</button>
  <button onclick="act({scroll:'550,360,0,160'})">Scroll ↑</button>
</div>
<main>
  <div class="stage">
    <div class="screen-wrap">
      <img id="screen" src="/screen.png" alt="GameNode Live Screen" />
    </div>
  </div>
  <aside>
    <h3>Send Text to Focused Field</h3>
    <div class="input-row">
      <input id="type-input" type="text" placeholder="Type text..." onkeydown="if(event.key==='Enter'){sendText();event.stopPropagation();}" />
      <button onclick="sendText()">Type</button>
    </div>
    <h3>Live UI State</h3>
    <pre id="state-json" aria-live="polite">Loading…</pre>
  </aside>
</main>
<script>
let lastBuildId = null;
let lastFrame = null;
const img = document.getElementById('screen');
const badge = document.getElementById('status-badge');

function coordsFromEvent(e) {
  const r = img.getBoundingClientRect();
  const scaleX = (img.naturalWidth || r.width) / r.width;
  const scaleY = (img.naturalHeight || r.height) / r.height;
  const x = Math.round((e.clientX - r.left) * scaleX);
  const y = Math.round((e.clientY - r.top) * scaleY);
  return { x, y };
}

img.addEventListener('mousemove', (e) => {
  const { x, y } = coordsFromEvent(e);
  document.getElementById('cursor-coords').textContent = `(${x}, ${y})`;
});

img.addEventListener('click', (e) => {
  const { x, y } = coordsFromEvent(e);
  act({ click: `${x},${y}`, ctrl: e.ctrlKey, shift: e.shiftKey });
});

img.addEventListener('dblclick', (e) => {
  const { x, y } = coordsFromEvent(e);
  act({ dblclick: `${x},${y}` });
});

img.addEventListener('contextmenu', (e) => {
  e.preventDefault();
  const { x, y } = coordsFromEvent(e);
  act({ click: `${x},${y}`, button: 'right' });
});

img.addEventListener('wheel', (e) => {
  e.preventDefault();
  const { x, y } = coordsFromEvent(e);
  act({ scroll: `${x},${y},${Math.round(-e.deltaX)},${Math.round(-e.deltaY)}` });
}, { passive: false });

window.addEventListener('keydown', (e) => {
  if (document.activeElement && document.activeElement.tagName === 'INPUT') return;
  const map = {
    Escape: 'Escape', Enter: 'Return', Tab: 'Tab', Backspace: 'Backspace',
    Delete: 'Delete', ArrowUp: 'UpArrow', ArrowDown: 'DownArrow',
    ArrowLeft: 'LeftArrow', ArrowRight: 'RightArrow', Home: 'Home',
    End: 'End', F5: 'F5'
  };
  const key = map[e.key] || (e.key.length === 1 ? e.key : null);
  if (key) {
    e.preventDefault();
    act({ key, ctrl: e.ctrlKey, shift: e.shiftKey, alt: e.altKey });
  }
});

function sendText() {
  const input = document.getElementById('type-input');
  if (input.value) {
    act({ text: input.value });
    input.value = '';
  }
}

async function act(params) {
  const qs = new URLSearchParams(params).toString();
  try {
    const res = await fetch('/api/action?' + qs);
    const state = await res.json();
    updateUI(state, true);
  } catch (err) {
    badge.textContent = 'Rebuilding / Reloading…';
    badge.className = 'badge reloading';
  }
}

function updateUI(state, forceImage) {
  const changed = forceImage || state.build_id !== lastBuildId || state.frame !== lastFrame;
  lastBuildId = state.build_id;
  lastFrame = state.frame;
  badge.textContent = 'Live Screen + Hot Reload';
  badge.className = 'badge';
  document.getElementById('screen-summary').textContent = state.screen_summary;
  document.getElementById('meta-info').textContent =
    `${state.width}×${state.height} · ${state.locale} · preset=${state.preset} · frame #${state.frame}`;
  document.getElementById('state-json').textContent = JSON.stringify(state, null, 2);
  if (changed) {
    img.src = `/screen.png?b=${state.build_id}&f=${state.frame}&t=${Date.now()}`;
  }
}

async function poll() {
  try {
    const res = await fetch('/api/state');
    const state = await res.json();
    updateUI(state, false);
  } catch (err) {
    badge.textContent = 'Hot-Reloading UI…';
    badge.className = 'badge reloading';
  }
}

setInterval(poll, 450);
poll();
</script>
</body>
</html>
"#;

fn run_server(
    port: u16,
    state_file: Option<PathBuf>,
    shot_file: Option<PathBuf>,
    initial_flags: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    window.set_size(PhysicalSize::new(DEFAULT_WIDTH, DEFAULT_HEIGHT));
    slint::platform::set_platform(Box::new(HeadlessPlatform {
        window: window.clone(),
        started: Instant::now(),
    }))?;

    let build_id = unix_millis();
    let data = Rc::new(RefCell::new(DemoData::new(build_id)));
    let ui = AppWindow::new()?;
    setup_base_window(&ui);
    wire_callbacks(&ui, &data);
    ui.show()?;
    ui.window()
        .dispatch_event(WindowEvent::WindowActiveChanged(true));

    {
        let mut d = data.borrow_mut();
        apply_preset(&ui, &mut d, "library");
        if let Some(sf) = &state_file {
            load_session_file(sf, &ui, &window, &mut d);
        }
        apply_cli_flags(&ui, &window, &mut d, initial_flags);
        if let Some(sf) = &state_file {
            save_session_file(sf, &d);
        }
    }

    let mut pixels = Vec::new();
    if let Some(shot) = &shot_file {
        let d = data.borrow();
        let _ = write_snapshot_file(shot, &window, &mut pixels, d.width, d.height);
    }

    let listener = TcpListener::bind(("127.0.0.1", port))?;
    eprintln!("GameNode Live Screen Studio listening on http://127.0.0.1:{port}");

    for stream in listener.incoming() {
        let Ok(mut stream) = stream else {
            continue;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut buf = [0u8; 8192];
        let Ok(n) = stream.read(&mut buf) else {
            continue;
        };
        if n == 0 {
            continue;
        }
        let req = String::from_utf8_lossy(&buf[..n]);
        let first_line = req.lines().next().unwrap_or("");
        let mut parts = first_line.split_whitespace();
        let _method = parts.next().unwrap_or("GET");
        let target = parts.next().unwrap_or("/");
        let (path, query) = target.split_once('?').unwrap_or((target, ""));

        match path {
            "/" | "/index.html" => {
                respond(
                    stream,
                    "200 OK",
                    "text/html; charset=utf-8",
                    STUDIO_HTML.as_bytes(),
                );
            }
            "/screen.png" => {
                let (w, h) = {
                    let d = data.borrow();
                    (d.width, d.height)
                };
                let png = render_png_bytes(&window, &mut pixels, w, h);
                if let Some(shot) = &shot_file {
                    let _ = std::fs::write(shot, &png);
                }
                respond(stream, "200 OK", "image/png", &png);
            }
            "/api/state" => {
                let json = {
                    let d = data.borrow();
                    build_state_json(&ui, &d)
                };
                let body = serde_json::to_vec_pretty(&json)?;
                respond(stream, "200 OK", "application/json", &body);
            }
            "/api/action" => {
                let params = parse_query(query);
                apply_query_actions(&params, &ui, &window, &data);
                if let Some(sf) = &state_file {
                    let d = data.borrow();
                    save_session_file(sf, &d);
                }
                let (w, h) = {
                    let d = data.borrow();
                    (d.width, d.height)
                };
                let png = render_png_bytes(&window, &mut pixels, w, h);
                if let Some(shot) = &shot_file {
                    let _ = std::fs::write(shot, &png);
                }
                if let Some(custom_shot) = params.get("shot") {
                    let _ = write_snapshot_file(Path::new(custom_shot), &window, &mut pixels, w, h);
                }
                let json = {
                    let d = data.borrow();
                    build_state_json(&ui, &d)
                };
                let body = serde_json::to_vec_pretty(&json)?;
                respond(stream, "200 OK", "application/json", &body);
            }
            _ => {
                respond(stream, "404 Not Found", "text/plain", b"Not Found");
            }
        }
    }
    Ok(())
}

fn apply_cli_flags(
    ui: &AppWindow,
    window: &MinimalSoftwareWindow,
    data: &mut DemoData,
    flags: &[String],
) {
    for flag in flags {
        match flag.as_str() {
            "compact" => {
                data.width = 640;
                data.height = 420;
                window.set_size(PhysicalSize::new(640, 420));
            }
            "maximized" | "wide" => {
                data.width = 1440;
                data.height = 900;
                window.set_size(PhysicalSize::new(1440, 900));
            }
            "pt_BR" => apply_locale("pt_BR", data),
            "en" => apply_locale("en", data),
            preset @ ("signed-out"
            | "library"
            | "grid"
            | "list"
            | "details"
            | "drm-detail"
            | "drm-missing"
            | "settings"
            | "settings-account"
            | "settings-account-signed-out"
            | "settings-stores"
            | "settings-stores-failed"
            | "settings-backups"
            | "settings-backups-dirty"
            | "settings-backups-unsupported"
            | "settings-advanced-restart"
            | "settings-about"
            | "settings-alert"
            | "settings-toast"
            | "settings-compact"
            | "settings-tools"
            | "tools"
            | "tools-anticheat"
            | "tools-installed"
            | "protection"
            | "restore"
            | "restore-preview"
            | "discovery"
            | "editor"
            | "claim"
            | "claim-conflict"
            | "remove"
            | "remove-game"
            | "match"
            | "match-dialog"
            | "add"
            | "add-dialog"
            | "add-summary"
            | "sync-explanation"
            | "banner"
            | "empty"
            | "nothing-matches") => {
                apply_preset(ui, data, preset);
            }
            other => {
                if let Some((w, h)) = other.split_once('x')
                    && let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>())
                {
                    let w = w.clamp(320, 3840);
                    let h = h.clamp(240, 2160);
                    data.width = w;
                    data.height = h;
                    window.set_size(PhysicalSize::new(w, h));
                }
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "serve") {
        let mut port = DEFAULT_PORT;
        let mut state_file: Option<PathBuf> = Some(PathBuf::from("/tmp/gamenode-ui-state.json"));
        let mut shot_file: Option<PathBuf> = Some(PathBuf::from("/tmp/gamenode-screen.png"));
        let mut flags = Vec::new();
        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--port" if i + 1 < args.len() => {
                    port = args[i + 1].parse().unwrap_or(DEFAULT_PORT);
                    i += 2;
                }
                "--state-file" if i + 1 < args.len() => {
                    state_file = Some(PathBuf::from(&args[i + 1]));
                    i += 2;
                }
                "--no-state-file" => {
                    state_file = None;
                    i += 1;
                }
                "--shot-file" if i + 1 < args.len() => {
                    shot_file = Some(PathBuf::from(&args[i + 1]));
                    i += 2;
                }
                other => {
                    flags.push(other.to_owned());
                    i += 1;
                }
            }
        }
        return run_server(port, state_file, shot_file, &flags);
    }

    // One-shot snapshot mode: `ui-review <output.png|output.ppm> [flags...]`
    let output = PathBuf::from(args.first().expect("output PNG/PPM path or `serve`"));
    let flags = &args[1..];

    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    window.set_size(PhysicalSize::new(DEFAULT_WIDTH, DEFAULT_HEIGHT));
    slint::platform::set_platform(Box::new(HeadlessPlatform {
        window: window.clone(),
        started: Instant::now(),
    }))?;

    let data = Rc::new(RefCell::new(DemoData::new(unix_millis())));
    let ui = AppWindow::new()?;
    setup_base_window(&ui);
    wire_callbacks(&ui, &data);
    ui.show()?;
    ui.window()
        .dispatch_event(WindowEvent::WindowActiveChanged(true));

    {
        let mut d = data.borrow_mut();
        // Default to signed-out unless `library` or another preset is given, matching legacy behavior
        apply_preset(&ui, &mut d, "signed-out");
        apply_cli_flags(&ui, &window, &mut d, flags);
    }

    let mut pixels = Vec::new();
    let (w, h) = {
        let d = data.borrow();
        (d.width, d.height)
    };
    write_snapshot_file(&output, &window, &mut pixels, w, h)?;
    Ok(())
}
