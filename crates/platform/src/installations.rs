//! Worker-only executable inspection and direct Windows launch.
use gamenode_core::drm_free::{Evidence, InstallationHost, LocalError, Registration};
#[derive(Default)]
pub struct WindowsInstallations;
#[cfg(windows)]
fn canonical(path: &str) -> Result<std::path::PathBuf, LocalError> {
    std::fs::canonicalize(path)
        .map(|p| {
            let text = p.to_string_lossy();
            if let Some(unc) = text.strip_prefix("\\\\?\\UNC\\") {
                std::path::PathBuf::from(format!("\\\\{unc}"))
            } else {
                std::path::PathBuf::from(text.strip_prefix("\\\\?\\").unwrap_or(&text))
            }
        })
        .map_err(|_| LocalError::InvalidTarget)
}
#[cfg(windows)]
fn identity(path: &std::path::Path) -> Result<String, LocalError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let file = std::fs::File::open(path).map_err(|_| LocalError::InvalidTarget)?;
    let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // SAFETY: valid owned file handle and writable output retained for the call.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0 {
        return Err(LocalError::InvalidTarget);
    }
    // SAFETY: successful API initialized the output.
    let info = unsafe { info.assume_init() };
    Ok(format!(
        "{}:{}:{}",
        info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow
    ))
}
#[cfg(windows)]
fn validate(path: &std::path::Path) -> Result<(), LocalError> {
    use std::io::{Read, Seek, SeekFrom};
    if !path.is_file()
        || !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
    {
        return Err(LocalError::InvalidTarget);
    }
    let mut file = std::fs::File::open(path).map_err(|_| LocalError::InvalidTarget)?;
    let mut dos = [0; 64];
    file.read_exact(&mut dos)
        .map_err(|_| LocalError::InvalidTarget)?;
    let offset = u32::from_le_bytes(dos[60..64].try_into().unwrap());
    if &dos[..2] != b"MZ" || offset > 16 * 1024 * 1024 {
        return Err(LocalError::InvalidTarget);
    }
    file.seek(SeekFrom::Start(u64::from(offset)))
        .map_err(|_| LocalError::InvalidTarget)?;
    let mut signature = [0; 4];
    file.read_exact(&mut signature)
        .map_err(|_| LocalError::InvalidTarget)?;
    if signature != *b"PE\0\0" {
        return Err(LocalError::InvalidTarget);
    }
    Ok(())
}
impl InstallationHost for WindowsInstallations {
    fn inspect(&self, executable: &str) -> Result<Registration, LocalError> {
        #[cfg(not(windows))]
        {
            let _ = executable;
            Err(LocalError::Unsupported)
        }
        #[cfg(windows)]
        {
            let path = canonical(executable)?;
            validate(&path)?;
            let root = path
                .parent()
                .ok_or(LocalError::InvalidTarget)?
                .to_string_lossy()
                .into_owned();
            let title = path
                .file_stem()
                .ok_or(LocalError::InvalidTarget)?
                .to_string_lossy()
                .into_owned();
            let file_identity = identity(&path)?;
            let executable = path.to_string_lossy().into_owned();
            Ok(Registration {
                id: crate::random::session_id(),
                title,
                executable: executable.clone(),
                file_identity,
                root: root.clone(),
                working_directory: root,
                arguments: vec![],
                gameplay: vec![executable],
                launchers: vec![],
                revision: 1,
                removed: false,
                available: true,
            })
        }
    }
    fn launch(&self, registration: &Registration) -> Result<(), LocalError> {
        #[cfg(not(windows))]
        {
            let _ = registration;
            Err(LocalError::Unsupported)
        }
        #[cfg(windows)]
        {
            if registration.removed {
                return Err(LocalError::InvalidTarget);
            }
            let path = canonical(&registration.executable)?;
            validate(&path)?;
            let root = canonical(&registration.root)?;
            let working = canonical(&registration.working_directory)?;
            let normalize = |p: &std::path::Path| p.to_string_lossy().to_lowercase();
            let root = normalize(&root);
            if !normalize(&path).starts_with(&(root.trim_end_matches('\\').to_owned() + "\\"))
                || !working.is_dir()
                || identity(&path)? != registration.file_identity
            {
                return Err(LocalError::InvalidTarget);
            }
            std::process::Command::new(path)
                .args(&registration.arguments)
                .current_dir(working)
                .spawn()
                .map_err(|e| LocalError::Failed(e.to_string()))?;
            Ok(())
        }
    }
    fn evidence(&self, registration: &Registration) -> Result<Vec<Evidence>, LocalError> {
        #[cfg(not(windows))]
        {
            let _ = registration;
            Err(LocalError::Unsupported)
        }
        #[cfg(windows)]
        {
            let mut values = pe_evidence(&registration.executable)?;
            values.push(Evidence {
                kind: gamenode_core::drm_free::EvidenceKind::Executable,
                value: std::path::Path::new(&registration.executable)
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                strong: false,
            });
            if let Some(name) = std::path::Path::new(&registration.root).file_name() {
                values.push(Evidence {
                    kind: gamenode_core::drm_free::EvidenceKind::Folder,
                    value: name.to_string_lossy().into_owned(),
                    strong: false,
                });
            }
            Ok(values)
        }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    #[cfg(not(windows))]
    fn unsupported_on_linux() {
        use gamenode_core::drm_free::InstallationHost;
        assert_eq!(
            super::WindowsInstallations.inspect("game.exe"),
            Err(super::LocalError::Unsupported)
        );
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    #[test]
    fn inspect_alias_identity_and_validate_before_launch() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("jogo com espacos.exe");
        let system = std::env::var_os("SystemRoot").unwrap();
        std::fs::copy(
            std::path::Path::new(&system).join("System32/cmd.exe"),
            &path,
        )
        .unwrap();
        let host = WindowsInstallations;
        let mut registration = host.inspect(path.to_str().unwrap()).unwrap();
        let alias = directory.path().join("alias.exe");
        std::fs::hard_link(&path, &alias).unwrap();
        assert_eq!(
            registration.file_identity,
            host.inspect(alias.to_str().unwrap()).unwrap().file_identity
        );
        registration.arguments = vec!["/C".into(), "exit".into(), "0".into()];
        host.launch(&registration).unwrap();
        registration.root = directory
            .path()
            .join("other")
            .to_string_lossy()
            .into_owned();
        assert!(host.launch(&registration).is_err());
        std::fs::write(&path, b"not a PE").unwrap();
        assert!(host.inspect(path.to_str().unwrap()).is_err());
    }
}

/// Native picker on a worker; cancellation never creates a registration.
pub fn pick_executable() -> Result<Option<String>, LocalError> {
    #[cfg(not(windows))]
    {
        Err(LocalError::Unsupported)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Controls::Dialogs::{
            CommDlgExtendedError, GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR,
            OFN_PATHMUSTEXIST, OPENFILENAMEW,
        };
        let mut path = vec![0u16; 32768];
        let filter: Vec<u16> = "Windows executable\0*.exe\0\0".encode_utf16().collect();
        let mut data: OPENFILENAMEW = unsafe { std::mem::zeroed() };
        data.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        data.lpstrFile = path.as_mut_ptr();
        data.nMaxFile = path.len() as u32;
        data.lpstrFilter = filter.as_ptr();
        data.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
        // SAFETY: buffers and structure remain valid for the modal dialog call.
        if unsafe { GetOpenFileNameW(&mut data) } == 0 {
            if unsafe { CommDlgExtendedError() } == 0 {
                return Ok(None);
            }
            return Err(LocalError::Failed("file picker failed".into()));
        }
        let length = path.iter().position(|c| *c == 0).unwrap_or(path.len());
        Ok(Some(String::from_utf16_lossy(&path[..length])))
    }
}

#[cfg(windows)]
fn pe_evidence(path: &str) -> Result<Vec<Evidence>, LocalError> {
    use gamenode_core::drm_free::EvidenceKind;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let path = wide(path);
    let mut ignored = 0;
    // SAFETY: valid NUL-terminated path; API output is bounded before allocation.
    let length = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), &mut ignored) };
    if length == 0 {
        return Ok(vec![]);
    }
    if length > 4 * 1024 * 1024 {
        return Err(LocalError::Failed(
            "executable metadata exceeds limit".into(),
        ));
    }
    let mut data = vec![0u8; length as usize];
    if unsafe { GetFileVersionInfoW(path.as_ptr(), 0, length, data.as_mut_ptr().cast()) } == 0 {
        return Ok(vec![]);
    }
    let query = |key: &str| -> Option<(*const u8, usize)> {
        let key = wide(key);
        let mut ptr = std::ptr::null_mut();
        let mut len = 0;
        if unsafe { VerQueryValueW(data.as_ptr().cast(), key.as_ptr(), &mut ptr, &mut len) } == 0
            || ptr.is_null()
        {
            return None;
        }
        let start = ptr as usize;
        let base = data.as_ptr() as usize;
        if start < base || start >= base + data.len() {
            return None;
        }
        Some((ptr.cast(), len as usize))
    };
    let mut translations = vec![(0x0409u16, 0x04b0u16)];
    if let Some((ptr, len)) = query("\\VarFileInfo\\Translation") {
        if len <= 4096 && ptr as usize + len <= data.as_ptr() as usize + data.len() {
            // SAFETY: output lies inside the bounded version-info buffer; copy unaligned words.
            let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
            for c in bytes.chunks_exact(4) {
                translations.push((
                    u16::from_le_bytes([c[0], c[1]]),
                    u16::from_le_bytes([c[2], c[3]]),
                ));
            }
        }
    }
    let mut evidence = Vec::new();
    for (name, kind) in [
        ("ProductName", EvidenceKind::Product),
        ("FileDescription", EvidenceKind::Description),
        ("OriginalFilename", EvidenceKind::Executable),
    ] {
        for (language, codepage) in &translations {
            if let Some((ptr, len)) = query(&format!(
                "\\StringFileInfo\\{language:04x}{codepage:04x}\\{name}"
            )) {
                if len > 4096 || ptr as usize + len * 2 > data.as_ptr() as usize + data.len() {
                    continue;
                }
                let bytes = unsafe { std::slice::from_raw_parts(ptr, len * 2) };
                let words: Vec<_> = bytes
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .take_while(|c| *c != 0)
                    .collect();
                let value = String::from_utf16_lossy(&words);
                if !value.trim().is_empty() {
                    evidence.push(Evidence {
                        kind: kind.clone(),
                        value,
                        strong: false,
                    });
                    break;
                }
            }
        }
    }
    Ok(evidence)
}
