//! Window geometry restoration: pure rules, no windowing system.

use crate::settings::WindowGeometry;

pub const DEFAULT_SIZE: (u32, u32) = (1100, 720);
pub const MIN_SIZE: (u32, u32) = (640, 420);
/// A restored window must expose at least this much of its top-left corner to be reachable.
const REACHABLE: (i32, i32) = (120, 40);

/// A display's work area in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Display {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// What to apply to the window at startup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowPlan {
    pub size: (u32, u32),
    /// `None` leaves placement to the window system/compositor.
    pub position: Option<(i32, i32)>,
    pub maximized: bool,
}

/// Plans the startup window from the saved geometry.
///
/// `can_position` is false where the window system forbids applications from placing windows
/// (Wayland): only size and maximized state are restored there. A saved position is used only if
/// it leaves the window reachable on a currently available display; otherwise the window is
/// centered on the first display.
pub fn plan_window(
    saved: Option<WindowGeometry>,
    displays: &[Display],
    can_position: bool,
) -> WindowPlan {
    let Some(saved) = saved else {
        return WindowPlan {
            size: fit(DEFAULT_SIZE, displays),
            position: None,
            maximized: false,
        };
    };
    let size = fit((saved.width, saved.height), displays);
    let position = match (can_position, saved.x, saved.y) {
        (true, Some(x), Some(y)) if !displays.is_empty() => {
            if reachable((x, y), displays) {
                Some((x, y))
            } else {
                Some(center(size, displays[0]))
            }
        }
        _ => None,
    };
    WindowPlan {
        size,
        position,
        maximized: saved.maximized,
    }
}

fn fit((width, height): (u32, u32), displays: &[Display]) -> (u32, u32) {
    let (mut width, mut height) = (width.max(MIN_SIZE.0), height.max(MIN_SIZE.1));
    if let Some(biggest) = displays
        .iter()
        .max_by_key(|d| u64::from(d.width) * u64::from(d.height))
    {
        width = width.min(biggest.width.max(MIN_SIZE.0));
        height = height.min(biggest.height.max(MIN_SIZE.1));
    }
    (width, height)
}

fn reachable((x, y): (i32, i32), displays: &[Display]) -> bool {
    displays.iter().any(|d| {
        let visible_w = (x + REACHABLE.0).min(d.x + d.width as i32) - x.max(d.x);
        let visible_h = (y + REACHABLE.1).min(d.y + d.height as i32) - y.max(d.y);
        visible_w >= REACHABLE.0 && visible_h >= REACHABLE.1
    })
}

fn center(size: (u32, u32), display: Display) -> (i32, i32) {
    (
        display.x + (display.width.saturating_sub(size.0) / 2) as i32,
        display.y + (display.height.saturating_sub(size.1) / 2) as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: Display = Display {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };
    const SIDE: Display = Display {
        x: 1920,
        y: 0,
        width: 1280,
        height: 1024,
    };

    fn saved(x: Option<i32>, y: Option<i32>) -> Option<WindowGeometry> {
        Some(WindowGeometry {
            x,
            y,
            width: 900,
            height: 600,
            maximized: false,
        })
    }

    #[test]
    fn no_saved_geometry_uses_default_size_and_system_placement() {
        let plan = plan_window(None, &[MAIN], true);
        assert_eq!(
            plan,
            WindowPlan {
                size: DEFAULT_SIZE,
                position: None,
                maximized: false
            }
        );
    }

    #[test]
    fn restores_a_reachable_position_including_secondary_displays() {
        assert_eq!(
            plan_window(saved(Some(100), Some(80)), &[MAIN], true).position,
            Some((100, 80))
        );
        assert_eq!(
            plan_window(saved(Some(2000), Some(10)), &[MAIN, SIDE], true).position,
            Some((2000, 10))
        );
    }

    #[test]
    fn unavailable_display_falls_back_to_a_centered_window_on_the_first_display() {
        // Saved on the second display, which is gone.
        let plan = plan_window(saved(Some(2000), Some(10)), &[MAIN], true);
        assert_eq!(plan.position, Some((510, 240)));
        // Mostly off-screen to the left: the corner is not reachable.
        assert_eq!(
            plan_window(saved(Some(-880), Some(10)), &[MAIN], true).position,
            Some((510, 240))
        );
        // Title bar above the screen.
        assert_eq!(
            plan_window(saved(Some(10), Some(-500)), &[MAIN], true).position,
            Some((510, 240))
        );
    }

    #[test]
    fn compositor_controlled_placement_restores_only_size_and_state() {
        let mut g = saved(Some(100), Some(100)).unwrap();
        g.maximized = true;
        let plan = plan_window(Some(g), &[MAIN], false);
        assert_eq!(
            plan,
            WindowPlan {
                size: (900, 600),
                position: None,
                maximized: true
            }
        );
    }

    #[test]
    fn size_is_clamped_to_usable_bounds() {
        let tiny = Some(WindowGeometry {
            x: None,
            y: None,
            width: 10,
            height: 10,
            maximized: false,
        });
        assert_eq!(plan_window(tiny, &[MAIN], true).size, MIN_SIZE);
        let huge = Some(WindowGeometry {
            x: None,
            y: None,
            width: 9000,
            height: 9000,
            maximized: false,
        });
        assert_eq!(plan_window(huge, &[MAIN], true).size, (1920, 1080));
    }

    #[test]
    fn unknown_displays_do_not_invent_a_position() {
        assert_eq!(
            plan_window(saved(Some(5), Some(5)), &[], true).position,
            None
        );
    }
}
