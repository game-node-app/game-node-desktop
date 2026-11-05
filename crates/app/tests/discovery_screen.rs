//! The watched-folder buttons must dispatch even before a path has been entered.
use gamenode_app::{AppWindow, LocalGames, SettingsState, SettingsTopic};
use slint::platform::software_renderer::{
    MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType,
};
use slint::platform::{Platform, PlatformError, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, PhysicalSize};
use std::{cell::Cell, rc::Rc};

struct Headless(Rc<MinimalSoftwareWindow>);
impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.0.clone())
    }
}

#[test]
fn watch_and_rescan_buttons_dispatch() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
    window.set_size(PhysicalSize::new(1100, 720));
    slint::platform::set_platform(Box::new(Headless(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    ui.show().unwrap();
    ui.global::<LocalGames>().set_supported(true);
    ui.global::<SettingsState>().set_open(true);
    ui.global::<SettingsState>()
        .set_topic(SettingsTopic::Stores);
    let picked = Rc::new(Cell::new(0));
    let observed = picked.clone();
    ui.global::<LocalGames>()
        .on_watch_folder_pick(move || observed.set(observed.get() + 1));
    let rescanned = Rc::new(Cell::new(0));
    let observed = rescanned.clone();
    ui.global::<LocalGames>()
        .on_discovery_rescan(move || observed.set(observed.get() + 1));
    let mut buffer = vec![PremultipliedRgbaColor::default(); 1100 * 720];
    let mut draw = || {
        slint::platform::update_timers_and_animations();
        window.request_redraw();
        window.draw_if_needed(|renderer| {
            renderer.render(&mut buffer, 1100);
        });
    };
    draw();
    ui.window().dispatch_event(WindowEvent::PointerScrolled {
        position: LogicalPosition::new(800., 400.),
        delta_x: 0.,
        delta_y: -1000.,
    });
    draw();
    for y in (120..680).step_by(8) {
        let position = LogicalPosition::new(900., y as f32);
        ui.window()
            .dispatch_event(WindowEvent::PointerMoved { position });
        ui.window().dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        ui.window().dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
        if picked.get() > 0 && rescanned.get() > 0 {
            break;
        }
    }
    assert!(
        picked.get() > 0,
        "empty watch field must open the folder picker"
    );
    assert!(
        rescanned.get() > 0,
        "rescan must dispatch a discovery request"
    );
}
