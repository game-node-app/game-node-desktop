//! Durable, non-secret foundation settings.

use serde::{Deserialize, Serialize};

/// Which renderer the next start uses. Takes effect on restart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RendererPreference {
    /// GPU rendering through Skia (default).
    #[default]
    Gpu,
    /// Slint's software renderer, for machines where GPU rendering fails.
    Software,
}

impl RendererPreference {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gpu => "gpu",
            Self::Software => "software",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "gpu" => Some(Self::Gpu),
            "software" => Some(Self::Software),
            _ => None,
        }
    }
}

/// Window placement in logical pixels. `x`/`y` are absent where the window system does not allow
/// applications to read or set positions (Wayland).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

/// Foundation settings. Never holds credentials or callback URLs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub renderer: RendererPreference,
    pub window: Option<WindowGeometry>,
}
