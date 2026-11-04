//! The library screens in Brazilian Portuguese: chrome and sentences are translated, while game
//! titles, store names and other data stay exactly as received.
//!
//! One test per file because Slint accepts a single platform per process.
//! `GAMENODE_SHOTS=<dir>` also writes a picture of each screen.

use std::rc::Rc;
use std::time::Duration;

use gamenode_app::{
    AccessKind, AppWindow, GameItem, LaunchNote, LibraryState, LibraryText, MatchKind,
    ProviderItem, SourceKind, Standing, StatusKind, ViewKind, Why,
};
use slint::platform::software_renderer::{
    MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType,
};
use slint::platform::{Platform, PlatformError, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, ModelRc, VecModel};

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

fn game(key: &str, title: &str, kind: MatchKind) -> GameItem {
    GameItem {
        key: key.into(),
        title: title.into(),
        provider: "Steam".into(),
        cover: Default::default(),
        has_cover: false,
        kind,
        selected: false,
        focused: false,
    }
}

fn shot(window: &MinimalSoftwareWindow, name: &str) {
    let Ok(dir) = std::env::var("GAMENODE_SHOTS") else {
        return;
    };
    std::thread::sleep(Duration::from_millis(400));
    slint::platform::update_timers_and_animations();
    let mut buffer = vec![PremultipliedRgbaColor::default(); (WIDTH * HEIGHT) as usize];
    window.request_redraw();
    assert!(window.draw_if_needed(|renderer| {
        renderer.render(&mut buffer, WIDTH as usize);
    }));
    let mut bytes = format!("P6\n{WIDTH} {HEIGHT}\n255\n").into_bytes();
    for pixel in &buffer {
        bytes.extend_from_slice(&[pixel.red, pixel.green, pixel.blue]);
    }
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        std::path::Path::new(&dir).join(format!("{name}.ppm")),
        bytes,
    )
    .unwrap();
}

#[test]
fn portuguese_translates_the_chrome_and_keeps_the_data() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    window.set_size(slint::PhysicalSize::new(WIDTH, HEIGHT));
    slint::platform::set_platform(Box::new(Headless(
        window.clone(),
        std::time::Instant::now(),
    )))
    .unwrap();
    let ui = AppWindow::new().unwrap();
    slint::select_bundled_translation("pt_BR").expect("the pt_BR catalog is bundled");
    ui.set_signed_in(true);
    ui.set_version("0.1.0".into());
    ui.show().unwrap();
    ui.window()
        .dispatch_event(WindowEvent::WindowActiveChanged(true));

    let text = ui.global::<LibraryText>();
    assert_eq!(text.invoke_status(StatusKind::Playing), "Jogando");
    assert_eq!(text.invoke_status(StatusKind::Finished), "Zerado");
    assert_eq!(text.invoke_match_kind(MatchKind::Matched), "Associado");
    assert_eq!(
        text.invoke_match_kind(MatchKind::NoGame),
        "Não está no GameNode"
    );
    assert_eq!(
        text.invoke_source(SourceKind::Chosen),
        "Você escolheu esta correspondência"
    );
    assert_eq!(
        text.invoke_standing(Standing::InLibrary, StatusKind::Planned),
        "Na sua biblioteca do GameNode como Planejado"
    );
    assert_eq!(
        text.invoke_why(Why::NothingSelected, StatusKind::Unknown),
        "Selecione um jogo primeiro."
    );
    assert_eq!(
        text.invoke_why(Why::AlreadyInLibrary, StatusKind::Finished),
        "Já está na sua biblioteca do GameNode como Zerado."
    );
    assert_eq!(
        text.invoke_launch(LaunchNote::NoHandler),
        "O aplicativo da loja não está instalado ou não pode ser aberto."
    );
    assert!(!text.invoke_access(AccessKind::Unverified).is_empty());

    // The screens: titles and store names are data, so they are shown as received.
    let state = ui.global::<LibraryState>();
    let titles = [
        "Portal 2",
        "Half-Life: Alyx",
        "Ünïcode Jogo {} 日本語",
        "Valheim",
    ];
    let kinds = [
        MatchKind::Matched,
        MatchKind::Ambiguous,
        MatchKind::Unmatched,
        MatchKind::NoGame,
    ];
    let items: Vec<_> = titles
        .iter()
        .zip(kinds)
        .enumerate()
        .map(|(i, (title, kind))| game(&format!("steam:{i}"), title, kind))
        .collect();
    state.set_items(ModelRc::from(Rc::new(VecModel::from(items))));
    state.set_providers(ModelRc::from(Rc::new(VecModel::from(vec![ProviderItem {
        id: "steam".into(),
        name: "Steam".into(),
        count: 4,
        selected: false,
    }]))));
    state.set_total(4);
    state.set_all_selected(true);
    state.set_focused_index(-1);
    state.set_active(true);
    state.set_play_why(Why::NothingSelected);
    state.set_install_why(Why::NothingSelected);
    state.set_match_why(Why::NothingSelected);
    state.set_add_why(Why::NothingSelected);
    slint::platform::update_timers_and_animations();
    shot(&window, "pt-01-grid");

    state.set_view(ViewKind::List);
    shot(&window, "pt-02-list");

    state.set_detail_valid(true);
    state.set_detail_title("Portal 2".into());
    state.set_detail_detected("Portal 2".into());
    state.set_detail_matched("Portal 2".into());
    state.set_detail_provider("Steam".into());
    state.set_detail_dir("C:\\Games\\Steam\\steamapps\\common\\Portal 2".into());
    state.set_detail_year("2011".into());
    state.set_detail_kind(MatchKind::Matched);
    state.set_detail_source(SourceKind::Chosen);
    state.set_detail_standing(Standing::InLibrary);
    state.set_detail_standing_status(StatusKind::Playing);
    state.set_selected_count(1);
    state.set_play_why(Why::Available);
    state.set_install_why(Why::NoInstallRule);
    state.set_match_why(Why::Available);
    state.set_add_why(Why::AlreadyInLibrary);
    state.set_add_existing_status(StatusKind::Playing);
    state.set_change_match(true);
    state.set_launch_note(LaunchNote::NoHandler);
    shot(&window, "pt-03-details");

    state.set_match_open(true);
    state.set_match_detected("Half-Life: Alyx".into());
    state.set_match_provider("Steam".into());
    shot(&window, "pt-04-match");
    state.set_match_open(false);

    state.set_add_open(true);
    shot(&window, "pt-05-add");

    // Back to English for anything that runs after in this process.
    slint::select_bundled_translation("").ok();
}
