//! Durable, non-secret foundation settings.

use serde::{Deserialize, Serialize};

/// Window placement in logical pixels. `x`/`y` are absent where the window system does not allow
/// applications to read or set positions (Wayland).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowGeometry {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

/// Foundation settings. Never holds credentials or callback URLs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub window: Option<WindowGeometry>,
}
