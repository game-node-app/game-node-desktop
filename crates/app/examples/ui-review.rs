//! Isolated native UI review. No database, credentials, or API access.
use gamenode_app::AppWindow;
use slint::ComponentHandle;
use std::io::Write;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .select()?;

    let ui = AppWindow::new()?;
    if args.iter().any(|arg| arg == "maximized") {
        ui.window().set_maximized(true);
    }
    if args.iter().any(|arg| arg == "compact") {
        ui.window().set_size(slint::LogicalSize::new(640., 420.));
    }
    if args.iter().any(|arg| arg == "pt_BR") {
        slint::select_bundled_translation("pt_BR")?;
    }
    if args.iter().any(|arg| arg == "protection") {
        let local = ui.global::<gamenode_app::LocalGames>();
        local.set_supported(true);
        local.set_selected_id("fixture".into());
        local.set_protection_open(true);
        local.set_automatic(true);
        local.set_game_automatic(true);
        local.set_destination(
            "C:\\Users\\Fixture\\AppData\\Local\\GameNode\\Desktop\\backups".into(),
        );
    }
    ui.set_version("0.1.0".into());
    ui.set_active_software(true);
    ui.set_fallback_renderer(false);
    ui.set_storage("Session storage: Windows Credential Manager".into());
    ui.set_settings_open(args.iter().any(|arg| arg == "settings"));
    ui.set_signed_in(args.iter().any(|arg| arg == "library"));
    let weak = ui.as_weak();
    let output = args.get(1).expect("output PPM path").clone();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::SingleShot,
        std::time::Duration::from_secs(2),
        move || {
            let ui = weak.upgrade().unwrap();
            let pixels = ui.window().take_snapshot().unwrap();
            let mut file = std::fs::File::create(&output).unwrap();
            write!(file, "P6\n{} {}\n255\n", pixels.width(), pixels.height()).unwrap();
            let rgb: Vec<u8> = pixels
                .as_slice()
                .iter()
                .flat_map(|pixel| [pixel.r, pixel.g, pixel.b])
                .collect();
            file.write_all(&rgb).unwrap();
            slint::quit_event_loop().unwrap();
        },
    );
    ui.run()?;
    Ok(())
}
