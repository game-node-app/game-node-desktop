//! Per-user installed-build autostart. Windows owns the user's startup-enabled preference.
use std::path::Path;
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegSetValueExW,
};
const RUN: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.encode_wide().chain(Some(0)).collect()
}
fn register_at(exe: &Path, key: &str) -> std::io::Result<bool> {
    if !exe
        .parent()
        .is_some_and(|p| p.join(".gamenode-desktop-install.json").is_file())
    {
        return Ok(false);
    }
    let key = wide(std::ffi::OsStr::new(key));
    let name = wide(std::ffi::OsStr::new("GameNode Desktop"));
    let mut command = std::ffi::OsString::from("\"");
    command.push(exe);
    command.push("\" --background");
    let command = wide(&command);
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
pub fn register() -> std::io::Result<bool> {
    register_at(&std::env::current_exe()?, RUN)
}
#[cfg(test)]
mod tests {
    use super::*;
    use gamenode_core::ports::RootResolver;
    use windows_sys::Win32::System::Registry::{RegDeleteKeyW, RegDeleteTreeW};
    #[test]
    fn only_installed_builds_write_the_quoted_executable_and_background_flag() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("Game Node.exe");
        let key = format!(
            "Software\\GameNodeDesktopTests\\autostart-{}",
            crate::random::nonce()
        );
        assert!(!register_at(&exe, &key).unwrap());
        assert!(
            crate::host::SystemRoots::new()
                .registry_string(&key, "GameNode Desktop")
                .is_none()
        );
        std::fs::write(dir.path().join(".gamenode-desktop-install.json"), "{}").unwrap();
        assert!(register_at(&exe, &key).unwrap());
        let expected = format!("\"{}\" --background", exe.display());
        assert_eq!(
            crate::host::SystemRoots::new()
                .registry_string(&key, "GameNode Desktop")
                .as_deref(),
            Some(expected.as_str())
        );
        assert!(register_at(&exe, &key).unwrap());
        unsafe {
            let key = wide(std::ffi::OsStr::new(&key));
            RegDeleteTreeW(HKEY_CURRENT_USER, key.as_ptr());
            RegDeleteKeyW(HKEY_CURRENT_USER, key.as_ptr());
        }
    }
}
