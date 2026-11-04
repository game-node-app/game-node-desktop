//! Display bounds used to restore reachable window geometry.

use gamenode_core::geometry::Display;

/// Whether the current window system allows the application to restore a position.
pub fn can_position_windows() -> bool {
    #[cfg(target_os = "windows")]
    {
        true
    }
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_some()
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        false
    }
}

/// Returns monitor rectangles in physical pixels. Convert them to the window's logical
/// coordinate space before passing them to `gamenode_core::geometry::plan_window`.
pub fn available_displays() -> Vec<Display> {
    #[cfg(target_os = "windows")]
    {
        windows::available_displays()
    }
    #[cfg(target_os = "linux")]
    {
        if can_position_windows() {
            x11::available_displays()
        } else {
            Vec::new()
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "linux")]
mod x11 {
    use super::Display;
    use std::process::Command;

    pub(super) fn available_displays() -> Vec<Display> {
        let Ok(output) = Command::new("xrandr").arg("--query").output() else {
            return Vec::new();
        };
        if !output.status.success() {
            return Vec::new();
        }
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(parse_connected_monitor)
            .collect()
    }

    fn parse_connected_monitor(line: &str) -> Option<Display> {
        let (_, rest) = line.split_once(" connected")?;
        if rest.starts_with(" disconnected") {
            return None;
        }
        rest.split_ascii_whitespace().find_map(parse_geometry)
    }

    fn parse_geometry(value: &str) -> Option<Display> {
        let (width, dimensions) = value.split_once('x')?;
        let width = width.parse().ok()?;
        let offset_start = dimensions.find(['+', '-'])?;
        let height = dimensions[..offset_start].parse().ok()?;
        let offsets = &dimensions[offset_start..];
        let (x, rest) = signed_integer(offsets)?;
        let (y, _) = signed_integer(rest)?;
        Some(Display {
            x,
            y,
            width,
            height,
        })
    }

    fn signed_integer(input: &str) -> Option<(i32, &str)> {
        if input.len() < 2 {
            return None;
        }
        let end = input[1..]
            .find(['+', '-'])
            .map_or(input.len(), |index| index + 1);
        Some((input[..end].parse().ok()?, &input[end..]))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn parses_connected_xrandr_monitor_geometry() {
            assert_eq!(
                parse_connected_monitor("DP-1 connected primary 2560x1440+0+0 (normal)").unwrap(),
                Display {
                    x: 0,
                    y: 0,
                    width: 2560,
                    height: 1440,
                }
            );
            assert_eq!(
                parse_connected_monitor("HDMI-1 connected 1920x1080-1920+120 (normal)").unwrap(),
                Display {
                    x: -1920,
                    y: 120,
                    width: 1920,
                    height: 1080,
                }
            );
        }

        #[test]
        fn ignores_disconnected_outputs_and_mode_lines() {
            assert!(parse_connected_monitor("DP-2 disconnected (normal)").is_none());
            assert!(parse_connected_monitor("   1920x1080 60.00*+").is_none());
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::Display;
    use std::mem::size_of;
    use windows_sys::Win32::Foundation::{LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
    };

    pub(super) fn available_displays() -> Vec<Display> {
        let mut displays = Vec::new();
        // EnumDisplayMonitors invokes collect synchronously with this vector as callback state.
        unsafe {
            EnumDisplayMonitors(
                std::ptr::null_mut(),
                std::ptr::null(),
                Some(collect),
                &mut displays as *mut Vec<Display> as LPARAM,
            );
        }
        displays
    }

    unsafe extern "system" fn collect(
        monitor: HMONITOR,
        _dc: HDC,
        _rect: *mut RECT,
        data: LPARAM,
    ) -> windows_sys::core::BOOL {
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: EnumDisplayMonitors supplies a live monitor handle and our callback state points
        // to the Vec allocated by available_displays for the duration of enumeration.
        if unsafe { GetMonitorInfoW(monitor, &mut info) } != 0 {
            let area = info.rcWork;
            // SAFETY: data was created from the mutable Vec passed to EnumDisplayMonitors above.
            unsafe {
                (&mut *(data as *mut Vec<Display>)).push(Display {
                    x: area.left,
                    y: area.top,
                    width: area.right.saturating_sub(area.left) as u32,
                    height: area.bottom.saturating_sub(area.top) as u32,
                });
            }
        }
        1
    }
}
