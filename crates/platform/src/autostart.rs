//! Per-user installed-build autostart. Windows owns the user's startup-enabled preference.
//!
//! GameNode writes only the current user's `Run` value; the startup-enabled switch Windows keeps
//! under `StartupApproved\Run` is never read or written, so a user disabling the entry in Windows'
//! Startup apps settings still wins after every rewrite.
use std::path::Path;
use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW,
};
const RUN: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE_NAME: &str = "GameNode Desktop";
fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.encode_wide().chain(Some(0)).collect()
}

/// The value the `Run` key stores: the quoted executable and `--background` only while the
/// "Start minimized to the tray" preference is on.
fn command_line(exe: &Path, background: bool) -> std::ffi::OsString {
    let mut command = std::ffi::OsString::from("\"");
    command.push(exe);
    command.push("\"");
    if background {
        command.push(" --background");
    }
    command
}

fn is_installed(exe: &Path) -> bool {
    exe.parent()
        .is_some_and(|parent| parent.join(".gamenode-desktop-install.json").is_file())
}

fn register_at(exe: &Path, key: &str, background: bool) -> std::io::Result<bool> {
    if !is_installed(exe) {
        return Ok(false);
    }
    let key = wide(std::ffi::OsStr::new(key));
    let name = wide(std::ffi::OsStr::new(VALUE_NAME));
    let command = wide(&command_line(exe, background));
    let mut handle = std::ptr::null_mut();
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut handle,
            std::ptr::null_mut(),
        )
    };
    if status != 0 {
        return Err(std::io::Error::from_raw_os_error(status as i32));
    }
    let result = unsafe {
        RegSetValueExW(
            handle,
            name.as_ptr(),
            0,
            REG_SZ,
            command.as_ptr().cast(),
            (command.len() * 2) as u32,
        )
    };
    unsafe {
        RegCloseKey(handle);
    }
    if result != 0 {
        return Err(std::io::Error::from_raw_os_error(result as i32));
    }
    Ok(true)
}

fn unregister_at(exe: &Path, key: &str) -> std::io::Result<bool> {
    if !is_installed(exe) {
        return Ok(false);
    }
    let key = wide(std::ffi::OsStr::new(key));
    let name = wide(std::ffi::OsStr::new(VALUE_NAME));
    let mut handle = std::ptr::null_mut();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut handle,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(false);
    }
    if status != 0 {
        return Err(std::io::Error::from_raw_os_error(status as i32));
    }
    let result = unsafe { RegDeleteValueW(handle, name.as_ptr()) };
    unsafe {
        RegCloseKey(handle);
    }
    match result {
        0 => Ok(true),
        ERROR_FILE_NOT_FOUND => Ok(false),
        error => Err(std::io::Error::from_raw_os_error(error as i32)),
    }
}

/// Registers the current executable to start at login. `background` adds `--background` to the
/// value; a build that is not installed keeps returning `Ok(false)` without writing anything.
pub fn register(background: bool) -> std::io::Result<bool> {
    register_at(&std::env::current_exe()?, RUN, background)
}

/// Removes the current user's autostart value. A build that is not installed keeps returning
/// `Ok(false)`; Windows' own `StartupApproved` choice is never touched.
pub fn unregister() -> std::io::Result<bool> {
    unregister_at(&std::env::current_exe()?, RUN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gamenode_core::ports::RootResolver;
    use windows_sys::Win32::System::Registry::{RegDeleteKeyW, RegDeleteTreeW};
    #[test]
    fn the_command_line_carries_the_background_flag_only_when_asked() {
        let exe = Path::new("C:\\Games\\Game Node\\GameNode.exe");
        assert_eq!(
            command_line(exe, true),
            "\"C:\\Games\\Game Node\\GameNode.exe\" --background"
        );
        assert_eq!(
            command_line(exe, false),
            "\"C:\\Games\\Game Node\\GameNode.exe\""
        );
    }

    #[test]
    fn only_installed_builds_write_and_remove_the_quoted_executable() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("Game Node.exe");
        let key = format!(
            "Software\\GameNodeDesktopTests\\autostart-{}",
            crate::random::nonce()
        );
        assert!(!register_at(&exe, &key, true).unwrap());
        assert!(
            crate::host::SystemRoots::new()
                .registry_string(&key, VALUE_NAME)
                .is_none()
        );
        std::fs::write(dir.path().join(".gamenode-desktop-install.json"), "{}").unwrap();
        assert!(register_at(&exe, &key, true).unwrap());
        let expected = format!("\"{}\" --background", exe.display());
        assert_eq!(
            crate::host::SystemRoots::new()
                .registry_string(&key, VALUE_NAME)
                .as_deref(),
            Some(expected.as_str())
        );

        // Starting minimized off rewrites the value without the flag.
        assert!(register_at(&exe, &key, false).unwrap());
        let expected = format!("\"{}\"", exe.display());
        assert_eq!(
            crate::host::SystemRoots::new()
                .registry_string(&key, VALUE_NAME)
                .as_deref(),
            Some(expected.as_str())
        );

        // Turning autostart off removes the value; removing it again reports no change.
        assert!(unregister_at(&exe, &key).unwrap());
        assert!(
            crate::host::SystemRoots::new()
                .registry_string(&key, VALUE_NAME)
                .is_none()
        );
        assert!(!unregister_at(&exe, &key).unwrap());

        std::fs::remove_file(dir.path().join(".gamenode-desktop-install.json")).unwrap();
        assert!(!register_at(&exe, &key, true).unwrap());
        assert!(!unregister_at(&exe, &key).unwrap());
        unsafe {
            let key = wide(std::ffi::OsStr::new(&key));
            RegDeleteTreeW(HKEY_CURRENT_USER, key.as_ptr());
            RegDeleteKeyW(HKEY_CURRENT_USER, key.as_ptr());
        }
    }
}
