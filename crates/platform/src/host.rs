//! Host file access and root resolution for provider scanners.

use std::io::{Error, ErrorKind, Result};
use std::path::Path;

use gamenode_core::ports::{HostFiles, RootResolver};

/// [`HostFiles`] over `std::fs`.
#[derive(Clone, Copy, Debug, Default)]
pub struct StdHostFiles;

impl HostFiles for StdHostFiles {
    fn read_to_string(&self, path: &str) -> Result<String> {
        std::fs::read_to_string(path)
    }

    fn read_dir(&self, path: &str) -> Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(path)? {
            // A name that is not valid text cannot be a manifest file name; skip it.
            if let Ok(name) = entry?.file_name().into_string() {
                names.push(name);
            }
        }
        Ok(names)
    }

    fn is_dir(&self, path: &str) -> bool {
        Path::new(path).is_dir()
    }

    fn canonicalize(&self, path: &str) -> Result<String> {
        let resolved = std::fs::canonicalize(path)?;
        let text = resolved
            .into_os_string()
            .into_string()
            .map_err(|_| Error::new(ErrorKind::InvalidData, "path is not valid text"))?;
        Ok(strip_verbatim_prefix(text))
    }
}

/// Windows returns `\\?\C:\...` from `canonicalize`; scanners and stored paths use `C:\...`.
pub fn strip_verbatim_prefix(path: String) -> String {
    match path.strip_prefix("\\\\?\\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => rest.to_owned(),
        Some(rest) if rest.starts_with("UNC\\") => format!("\\\\{}", &rest[4..]),
        _ => path,
    }
}

/// The Windows registry hive a [`RootResolver`] key belongs to.
#[cfg(any(test, windows))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hive {
    CurrentUser,
    LocalMachine,
}

/// Splits a registry key into its hive and subkey. `HKCU\`, `HKEY_CURRENT_USER\`, `HKLM\` and
/// `HKEY_LOCAL_MACHINE\` name a hive. A key without one keeps the historical current-user meaning,
/// which the autostart adapter relies on.
#[cfg(any(test, windows))]
fn split_hive(key: &str) -> Option<(Hive, &str)> {
    match key.split_once('\\') {
        Some((hive, subkey)) => {
            if subkey.is_empty() {
                return None;
            }
            match hive.to_ascii_uppercase().as_str() {
                "HKCU" | "HKEY_CURRENT_USER" => Some((Hive::CurrentUser, subkey)),
                "HKLM" | "HKEY_LOCAL_MACHINE" => Some((Hive::LocalMachine, subkey)),
                _ => Some((Hive::CurrentUser, key)),
            }
        }
        None => (!key.is_empty()).then_some((Hive::CurrentUser, key)),
    }
}

/// [`RootResolver`] over the user's home folder and, on Windows, the current-user and local-machine
/// registry hives.
#[derive(Clone, Debug)]
pub struct SystemRoots {
    home: Option<String>,
}

impl SystemRoots {
    pub fn new() -> Self {
        Self {
            home: dirs::home_dir().and_then(|home| home.into_os_string().into_string().ok()),
        }
    }

    /// With a fixed home folder, for tests.
    pub fn with_home(home: impl Into<String>) -> Self {
        Self {
            home: Some(home.into()),
        }
    }
}

impl Default for SystemRoots {
    fn default() -> Self {
        Self::new()
    }
}

impl RootResolver for SystemRoots {
    fn expand_home(&self, path: &str) -> Option<String> {
        match path.strip_prefix('~') {
            None => Some(path.to_owned()),
            Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
                let home = self.home.as_ref()?;
                Some(format!("{}{}", home.trim_end_matches(['/', '\\']), rest))
            }
            // `~name` would be another user's home, which is not supported.
            Some(_) => None,
        }
    }

    fn registry_string(&self, key: &str, value: &str) -> Option<String> {
        registry::read_string(key, value)
    }
}

#[cfg(windows)]
mod registry {
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW,
    };

    use super::{Hive, split_hive};

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub(super) fn read_string(key: &str, value: &str) -> Option<String> {
        if key.contains('\0') || value.contains('\0') {
            return None;
        }
        let (hive, key) = split_hive(key)?;
        let hive = match hive {
            Hive::CurrentUser => HKEY_CURRENT_USER,
            Hive::LocalMachine => HKEY_LOCAL_MACHINE,
        };
        let key = wide(key);
        let value = wide(value);
        let mut bytes: u32 = 0;
        // SAFETY: both strings are NUL-terminated and outlive the call; a null data pointer with
        // a size pointer asks only for the required size.
        let status = unsafe {
            RegGetValueW(
                hive,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut bytes,
            )
        };
        if status != 0 || bytes == 0 || bytes > 64 * 1024 {
            return None;
        }
        let mut buffer = vec![0u16; (bytes as usize).div_ceil(2)];
        let mut capacity = (buffer.len() * 2) as u32;
        // SAFETY: `buffer` is writable for `capacity` bytes and the strings are as above.
        let status = unsafe {
            RegGetValueW(
                hive,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut capacity,
            )
        };
        if status != 0 {
            return None;
        }
        let length = buffer
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(buffer.len());
        String::from_utf16(&buffer[..length]).ok()
    }

    #[cfg(test)]
    pub(super) mod test_support {
        use windows_sys::Win32::System::Registry::{
            HKEY_CURRENT_USER, REG_SZ, RegDeleteTreeW, RegSetKeyValueW,
        };

        use super::wide;

        pub(crate) fn write_string(key: &str, value: &str, data: &str) {
            let (key, value, data) = (wide(key), wide(value), wide(data));
            // SAFETY: NUL-terminated strings that outlive the call; the size covers the data.
            let status = unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    key.as_ptr(),
                    value.as_ptr(),
                    REG_SZ,
                    data.as_ptr().cast(),
                    (data.len() * 2) as u32,
                )
            };
            assert_eq!(status, 0, "could not write the test registry value");
        }

        pub(crate) fn delete_tree(key: &str) {
            let key = wide(key);
            // SAFETY: NUL-terminated string that outlives the call.
            unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, key.as_ptr()) };
        }
    }
}

#[cfg(not(windows))]
mod registry {
    pub(super) fn read_string(_key: &str, _value: &str) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_lists_and_canonicalizes_real_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap().to_owned();
        std::fs::create_dir_all(dir.path().join("steamapps/common/Game")).unwrap();
        std::fs::write(dir.path().join("steamapps/appmanifest_1.acf"), "text").unwrap();

        let files = StdHostFiles;
        let steamapps = Path::new(&root)
            .join("steamapps")
            .to_str()
            .unwrap()
            .to_owned();
        assert!(files.is_dir(&steamapps));
        let mut names = files.read_dir(&steamapps).unwrap();
        names.sort();
        assert_eq!(names, ["appmanifest_1.acf", "common"]);
        let manifest = Path::new(&steamapps).join("appmanifest_1.acf");
        assert_eq!(
            files.read_to_string(manifest.to_str().unwrap()).unwrap(),
            "text"
        );
        assert!(files.canonicalize(&root).is_ok());
        assert!(files.canonicalize(&format!("{root}/missing")).is_err());
        assert!(files.read_dir(&format!("{root}/missing")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn canonicalize_resolves_a_symbolic_link() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("share/Steam");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::create_dir_all(dir.path().join(".steam")).unwrap();
        let link = dir.path().join(".steam/steam");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let files = StdHostFiles;
        assert_eq!(
            files.canonicalize(link.to_str().unwrap()).unwrap(),
            files.canonicalize(target.to_str().unwrap()).unwrap()
        );
    }

    #[test]
    fn expands_home_only_for_a_leading_tilde() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_str().unwrap().to_owned();
        let roots = SystemRoots::with_home(home.clone());
        assert_eq!(
            roots.expand_home("~/.steam/steam"),
            Some(format!("{home}/.steam/steam"))
        );
        assert_eq!(roots.expand_home("~"), Some(home));
        assert_eq!(
            roots.expand_home("/opt/steam"),
            Some("/opt/steam".to_owned())
        );
        assert_eq!(roots.expand_home("~someone/x"), None);
        assert_eq!(SystemRoots { home: None }.expand_home("~/x"), None);
    }

    #[test]
    fn strips_the_windows_verbatim_prefix() {
        assert_eq!(
            strip_verbatim_prefix("\\\\?\\C:\\Steam".to_owned()),
            "C:\\Steam"
        );
        assert_eq!(
            strip_verbatim_prefix("\\\\?\\UNC\\nas\\games".to_owned()),
            "\\\\nas\\games"
        );
        assert_eq!(strip_verbatim_prefix("/home/me".to_owned()), "/home/me");
    }

    #[test]
    fn registry_keys_name_their_hive() {
        assert_eq!(
            split_hive(r"HKCU\Software\Valve\Steam"),
            Some((Hive::CurrentUser, r"Software\Valve\Steam"))
        );
        assert_eq!(
            split_hive(r"HKLM\SOFTWARE\WOW6432Node\Epic Games\EpicGamesLauncher"),
            Some((
                Hive::LocalMachine,
                r"SOFTWARE\WOW6432Node\Epic Games\EpicGamesLauncher"
            ))
        );
        assert_eq!(
            split_hive(r"HKEY_LOCAL_MACHINE\X"),
            Some((Hive::LocalMachine, "X"))
        );
        assert_eq!(
            split_hive(r"Software\Valve\Steam"),
            Some((Hive::CurrentUser, r"Software\Valve\Steam")),
            "a key without a hive keeps the historical current-user meaning"
        );
        assert_eq!(split_hive(r"HKLM\"), None);
        assert_eq!(split_hive(""), None);
    }

    #[cfg(not(windows))]
    #[test]
    fn there_is_no_registry_off_windows() {
        assert_eq!(
            SystemRoots::new().registry_string("Software\\Valve\\Steam", "SteamPath"),
            None
        );
        assert_eq!(
            SystemRoots::new().registry_string(
                r"HKLM\SOFTWARE\WOW6432Node\Epic Games\EpicGamesLauncher",
                "AppDataPath"
            ),
            None
        );
    }

    #[cfg(windows)]
    #[test]
    fn reads_a_value_written_under_a_test_hkcu_key_and_removes_it() {
        let key = format!("Software\\GameNodeDesktopTest\\{}", std::process::id());
        registry::test_support::write_string(&key, "SteamPath", "c:/program files (x86)/steam");

        let roots = SystemRoots::new();
        let found = roots.registry_string(&key, "SteamPath");
        let qualified = roots.registry_string(&format!("HKCU\\{key}"), "SteamPath");
        let missing = roots.registry_string(&key, "Missing");
        let machine_missing =
            roots.registry_string(r"HKLM\Software\GameNodeDesktopTest\Missing", "AppDataPath");
        registry::test_support::delete_tree(&key);

        assert_eq!(found.as_deref(), Some("c:/program files (x86)/steam"));
        assert_eq!(qualified, found);
        assert_eq!(missing, None);
        assert_eq!(machine_missing, None);
        assert_eq!(roots.registry_string(&key, "SteamPath"), None);
    }
}
