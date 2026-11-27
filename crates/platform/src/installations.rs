//! Worker-only executable inspection and custom launch adapters.
#[cfg(any(windows, target_os = "linux", test))]
use gamenode_core::drm_free::LaunchEnvironmentDelta;
use gamenode_core::drm_free::{
    CustomLaunchCommand, Evidence, InstallationHost, LaunchEnvironment, LocalError, Registration,
};

/// Adds a port's provenance environment to the command that launches it (design D7).
///
/// The variables are added to the inherited environment; the launcher's own variables stay.
#[cfg(any(windows, target_os = "linux", test))]
fn apply_environment(command: &mut std::process::Command, environment: &LaunchEnvironment) {
    command.envs(environment);
}

#[cfg(any(windows, target_os = "linux", test))]
fn apply_environment_delta(
    command: &mut std::process::Command,
    environment: &LaunchEnvironmentDelta,
) -> Result<(), LocalError> {
    if !environment.validate() {
        return Err(LocalError::InvalidTarget);
    }
    command.env_clear();
    for (key, value) in std::env::vars_os().filter(|(key, _)| safe_game_environment_key(key)) {
        command.env(key, value);
    }
    command.envs(&environment.set);
    for key in &environment.unset {
        command.env_remove(key);
    }
    Ok(())
}

#[cfg(any(windows, target_os = "linux", test))]
fn safe_game_environment_key(key: &std::ffi::OsStr) -> bool {
    let key = key.to_string_lossy();
    #[cfg(windows)]
    {
        matches!(
            key.to_ascii_uppercase().as_str(),
            "PATH"
                | "PATHEXT"
                | "SYSTEMROOT"
                | "WINDIR"
                | "TEMP"
                | "TMP"
                | "USERPROFILE"
                | "HOMEDRIVE"
                | "HOMEPATH"
                | "APPDATA"
                | "LOCALAPPDATA"
                | "PROGRAMDATA"
                | "COMSPEC"
                | "NUMBER_OF_PROCESSORS"
                | "PROCESSOR_ARCHITECTURE"
                | "PROCESSOR_IDENTIFIER"
                | "PROCESSOR_LEVEL"
                | "PROCESSOR_REVISION"
        )
    }
    #[cfg(not(windows))]
    {
        key == "PATH"
            || key == "HOME"
            || key == "USER"
            || key == "LOGNAME"
            || key == "LANG"
            || key == "DISPLAY"
            || key == "WAYLAND_DISPLAY"
            || key == "XAUTHORITY"
            || key.starts_with("QT_")
            || key.starts_with("LC_")
            || key.starts_with("XDG_")
            || key == "DBUS_SESSION_BUS_ADDRESS"
    }
}

#[cfg(any(windows, target_os = "linux", test))]
fn custom_command(
    executable: &str,
    arguments: &[String],
    working_directory: &str,
    environment: &LaunchEnvironmentDelta,
) -> Result<std::process::Command, LocalError> {
    if executable.is_empty()
        || executable.contains('\0')
        || working_directory.is_empty()
        || working_directory.contains('\0')
        || arguments.iter().any(|argument| argument.contains('\0'))
    {
        return Err(LocalError::InvalidTarget);
    }
    let mut command = std::process::Command::new(executable);
    command.args(arguments).current_dir(working_directory);
    apply_environment_delta(&mut command, environment)?;
    Ok(command)
}
#[derive(Default)]
pub struct WindowsInstallations;

/// Linux executable inspection and direct launch, including explicit wrapper recipes such as
/// Proton. The launch plan is already resolved and reviewed by the host before this adapter runs.
#[derive(Default)]
pub struct LinuxInstallations;

#[cfg(target_os = "linux")]
fn linux_canonical(path: &str) -> Result<std::path::PathBuf, LocalError> {
    std::fs::canonicalize(path).map_err(|_| LocalError::InvalidTarget)
}

#[cfg(target_os = "linux")]
pub(crate) fn linux_identity(path: &std::path::Path) -> Result<String, LocalError> {
    use std::os::unix::{fs::MetadataExt, fs::PermissionsExt};
    let metadata = std::fs::metadata(path).map_err(|_| LocalError::InvalidTarget)?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(LocalError::InvalidTarget);
    }
    Ok(format!(
        "linux:{}:{}:{}:{}:{}:{}:{}:{}",
        metadata.dev(),
        metadata.ino(),
        metadata.mode() & 0o7777,
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.ctime(),
        metadata.ctime_nsec(),
    ))
}

#[cfg(target_os = "linux")]
impl LinuxInstallations {
    fn spawn_custom(&self, request: &CustomLaunchCommand) -> Result<(), LocalError> {
        let executable = linux_canonical(&request.executable)?;
        if linux_identity(&executable)? != request.file_identity {
            return Err(LocalError::InvalidTarget);
        }
        let working = linux_canonical(&request.working_directory)?;
        if !working.is_dir() {
            return Err(LocalError::InvalidTarget);
        }
        let mut command = custom_command(
            &executable.to_string_lossy(),
            &request.arguments,
            &working.to_string_lossy(),
            &request.environment,
        )?;
        command
            .spawn()
            .map_err(|error| LocalError::Failed(error.to_string()))?;
        Ok(())
    }
}

impl InstallationHost for LinuxInstallations {
    fn inspect(&self, executable: &str) -> Result<Registration, LocalError> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = executable;
            Err(LocalError::Unsupported)
        }
        #[cfg(target_os = "linux")]
        {
            let path = linux_canonical(executable)?;
            let file_identity = linux_identity(&path)?;
            let root = path
                .parent()
                .ok_or(LocalError::InvalidTarget)?
                .to_string_lossy()
                .into_owned();
            let executable = path.to_string_lossy().into_owned();
            let title = path
                .file_stem()
                .ok_or(LocalError::InvalidTarget)?
                .to_string_lossy()
                .into_owned();
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
                rediscovery_allowed: false,
                transferred_from: None,
                custom_cover: None,
                available: true,
            })
        }
    }

    fn validate_registration(
        &self,
        registration: &Registration,
    ) -> Result<Registration, LocalError> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = registration;
            Err(LocalError::Unsupported)
        }
        #[cfg(target_os = "linux")]
        {
            let executable = linux_canonical(&registration.executable)?;
            let root = linux_canonical(&registration.root)?;
            let working = linux_canonical(&registration.working_directory)?;
            if registration.removed
                || !root.is_dir()
                || !working.is_dir()
                || !executable.starts_with(&root)
                || linux_identity(&executable)? != registration.file_identity
            {
                return Err(LocalError::InvalidTarget);
            }
            let mut result = registration.clone();
            result.executable = executable.to_string_lossy().into_owned();
            result.root = root.to_string_lossy().into_owned();
            result.working_directory = working.to_string_lossy().into_owned();
            Ok(result)
        }
    }

    fn launch(&self, registration: &Registration) -> Result<(), LocalError> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = registration;
            Err(LocalError::Unsupported)
        }
        #[cfg(target_os = "linux")]
        {
            let registration = self.validate_registration(registration)?;
            let mut command = std::process::Command::new(&registration.executable);
            command
                .args(&registration.arguments)
                .current_dir(&registration.working_directory);
            command
                .spawn()
                .map_err(|error| LocalError::Failed(error.to_string()))?;
            Ok(())
        }
    }

    fn launch_with_environment(
        &self,
        registration: &Registration,
        environment: &LaunchEnvironment,
    ) -> Result<(), LocalError> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (registration, environment);
            Err(LocalError::Unsupported)
        }
        #[cfg(target_os = "linux")]
        {
            let registration = self.validate_registration(registration)?;
            let mut command = std::process::Command::new(&registration.executable);
            command
                .args(&registration.arguments)
                .current_dir(&registration.working_directory);
            apply_environment(&mut command, environment);
            command
                .spawn()
                .map_err(|error| LocalError::Failed(error.to_string()))?;
            Ok(())
        }
    }

    fn launch_custom(&self, request: &CustomLaunchCommand) -> Result<(), LocalError> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = request;
            Err(LocalError::Unsupported)
        }
        #[cfg(target_os = "linux")]
        {
            self.spawn_custom(request)
        }
    }

    fn evidence(&self, registration: &Registration) -> Result<Vec<Evidence>, LocalError> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = registration;
            Err(LocalError::Unsupported)
        }
        #[cfg(target_os = "linux")]
        {
            Ok(vec![Evidence {
                kind: gamenode_core::drm_free::EvidenceKind::Executable,
                value: std::path::Path::new(&registration.executable)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                strong: false,
            }])
        }
    }
}
#[cfg(windows)]
pub(crate) fn canonical(path: &str) -> Result<std::path::PathBuf, LocalError> {
    std::fs::canonicalize(path)
        .map(|p| {
            std::path::PathBuf::from(crate::host::strip_verbatim_prefix(
                p.to_string_lossy().into_owned(),
            ))
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
    let mut coff = [0u8; 20];
    file.read_exact(&mut coff)
        .map_err(|_| LocalError::InvalidTarget)?;
    let machine = u16::from_le_bytes(coff[..2].try_into().unwrap());
    let sections = u16::from_le_bytes(coff[2..4].try_into().unwrap());
    let optional_size = u16::from_le_bytes(coff[16..18].try_into().unwrap());
    let flags = u16::from_le_bytes(coff[18..20].try_into().unwrap());
    if ![0x14c, 0x8664].contains(&machine)
        || sections == 0
        || sections > 96
        || flags & 2 == 0
        || flags & 0x2000 != 0
        || !(96..=4096).contains(&optional_size)
    {
        return Err(LocalError::InvalidTarget);
    }
    let mut optional = vec![0u8; usize::from(optional_size)];
    file.read_exact(&mut optional)
        .map_err(|_| LocalError::InvalidTarget)?;
    let magic = u16::from_le_bytes(optional[..2].try_into().unwrap());
    let subsystem = u16::from_le_bytes(optional[68..70].try_into().unwrap());
    if ![0x10b, 0x20b].contains(&magic)
        || ![2, 3].contains(&subsystem)
        || file
            .metadata()
            .map_err(|_| LocalError::InvalidTarget)?
            .len()
            < u64::from(offset) + 24 + u64::from(optional_size) + u64::from(sections) * 40
    {
        return Err(LocalError::InvalidTarget);
    }
    Ok(())
}
impl WindowsInstallations {
    /// Starts the registration with the environment its port provenance records.
    #[cfg(windows)]
    fn spawn(
        &self,
        registration: &Registration,
        environment: &LaunchEnvironment,
    ) -> Result<(), LocalError> {
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
        let mut command = std::process::Command::new(path);
        command.args(&registration.arguments).current_dir(working);
        apply_environment(&mut command, environment);
        command
            .spawn()
            .map_err(|e| LocalError::Failed(e.to_string()))?;
        Ok(())
    }

    /// Starts a fully resolved custom executable after repeating identity checks on the worker.
    #[cfg(windows)]
    fn spawn_custom(&self, request: &CustomLaunchCommand) -> Result<(), LocalError> {
        let path = canonical(&request.executable)?;
        validate(&path)?;
        if identity(&path)? != request.file_identity {
            return Err(LocalError::InvalidTarget);
        }
        let working = canonical(&request.working_directory)?;
        if !working.is_dir() {
            return Err(LocalError::InvalidTarget);
        }
        let mut command = custom_command(
            &path.to_string_lossy(),
            &request.arguments,
            &working.to_string_lossy(),
            &request.environment,
        )?;
        command
            .spawn()
            .map_err(|error| LocalError::Failed(error.to_string()))?;
        Ok(())
    }
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
                title: title.clone(),
                executable: executable.clone(),
                file_identity,
                root: root.clone(),
                working_directory: root,
                arguments: vec![],
                gameplay: if gamenode_core::drm_free::identity::launcher(&title)
                    || gamenode_core::drm_free::identity::maintenance(&title)
                {
                    vec![]
                } else {
                    vec![executable.clone()]
                },
                launchers: if gamenode_core::drm_free::identity::launcher(&title) {
                    vec![executable]
                } else {
                    vec![]
                },
                revision: 1,
                removed: false,
                rediscovery_allowed: false,
                transferred_from: None,
                custom_cover: None,
                available: true,
            })
        }
    }
    fn validate_registration(
        &self,
        registration: &Registration,
    ) -> Result<Registration, LocalError> {
        #[cfg(not(windows))]
        {
            let _ = registration;
            Err(LocalError::Unsupported)
        }
        #[cfg(windows)]
        {
            let mut value = registration.clone();
            let root = canonical(&value.root)?;
            let working = canonical(&value.working_directory)?;
            if !root.is_dir() || !working.is_dir() {
                return Err(LocalError::InvalidTarget);
            }
            let prefix = root
                .to_string_lossy()
                .to_lowercase()
                .trim_end_matches('\\')
                .to_owned()
                + "\\";
            let executable = canonical(&value.executable)?;
            validate(&executable)?;
            if !executable
                .to_string_lossy()
                .to_lowercase()
                .starts_with(&prefix)
                || identity(&executable)? != value.file_identity
            {
                return Err(LocalError::InvalidTarget);
            }
            for paths in [&mut value.gameplay, &mut value.launchers] {
                for path in paths.iter_mut() {
                    let canonical = canonical(path)?;
                    validate(&canonical)?;
                    if !canonical
                        .to_string_lossy()
                        .to_lowercase()
                        .starts_with(&prefix)
                    {
                        return Err(LocalError::InvalidTarget);
                    }
                    *path = canonical.to_string_lossy().into_owned();
                }
                paths.sort();
                paths.dedup();
            }
            if value.gameplay.iter().any(|path| {
                value.launchers.contains(path)
                    || gamenode_core::drm_free::identity::maintenance(
                        std::path::Path::new(path)
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .as_ref(),
                    )
            }) {
                return Err(LocalError::InvalidTarget);
            }
            value.root = root.to_string_lossy().into_owned();
            value.working_directory = working.to_string_lossy().into_owned();
            value.executable = executable.to_string_lossy().into_owned();
            Ok(value)
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
            self.spawn(registration, &LaunchEnvironment::new())
        }
    }
    fn launch_with_environment(
        &self,
        registration: &Registration,
        environment: &LaunchEnvironment,
    ) -> Result<(), LocalError> {
        #[cfg(not(windows))]
        {
            let _ = (registration, environment);
            Err(LocalError::Unsupported)
        }
        #[cfg(windows)]
        {
            self.spawn(registration, environment)
        }
    }
    fn launch_custom(&self, request: &CustomLaunchCommand) -> Result<(), LocalError> {
        #[cfg(not(windows))]
        {
            let _ = request;
            Err(LocalError::Unsupported)
        }
        #[cfg(windows)]
        {
            self.spawn_custom(request)
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
/// Native picker on a worker; cancellation never creates a registration.
pub fn pick_executable() -> Result<Option<String>, LocalError> {
    pick_file(true)
}

/// Opens a Windows executable picker that starts in `folder` (a port install folder).
///
/// The port's follow-up picker uses it when none of the recipe's executables exists.
pub fn pick_executable_in(folder: &str) -> Result<Option<String>, LocalError> {
    pick_file_starting_at(true, None, Some(folder))
}

/// Opens a folder picker with an optional initial selection.
///
/// This is the parent-folder picker of a PC port install. Unlike [`pick_save_location`], it can
/// start at the folder the user chose last time.
pub fn pick_folder_starting_at(initial: Option<&str>) -> Result<Option<String>, LocalError> {
    pick_folder(initial)
}

/// Opens a file picker limited to the supplied extensions (`["iso", ".bin"]` both work).
///
/// An empty list offers every file. This is the guided-setup picker for a pot's supplied files
/// and console packages.
pub fn pick_file_with_extensions(
    extensions: &[String],
    initial: Option<&str>,
) -> Result<Option<String>, LocalError> {
    pick_file_starting_at(false, Some(extensions), initial)
}

/// Native pickers used by the PC port installer.
///
/// Windows uses the system dialogs through [`PlatformFilePicker`]. Linux injects a Tauri dialog
/// picker from the app, because the platform crate has no window handle (task 2.7).
pub trait PortFilePicker: Send + Sync {
    /// A folder, optionally starting at `initial`.
    fn folder(&self, initial: Option<&str>) -> Result<Option<String>, LocalError>;
    /// A file limited to `extensions` (empty accepts any file).
    fn file(
        &self,
        extensions: &[String],
        initial: Option<&str>,
    ) -> Result<Option<String>, LocalError>;
    /// An executable inside the port's install folder.
    fn executable(&self, initial: Option<&str>) -> Result<Option<String>, LocalError>;
}

/// The system dialogs. Only Windows implements them; other platforms report `Unsupported`.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlatformFilePicker;

impl PortFilePicker for PlatformFilePicker {
    fn folder(&self, initial: Option<&str>) -> Result<Option<String>, LocalError> {
        pick_folder_starting_at(initial)
    }

    fn file(
        &self,
        extensions: &[String],
        initial: Option<&str>,
    ) -> Result<Option<String>, LocalError> {
        pick_file_with_extensions(extensions, initial)
    }

    fn executable(&self, initial: Option<&str>) -> Result<Option<String>, LocalError> {
        match initial.filter(|folder| !folder.trim().is_empty()) {
            Some(folder) => pick_executable_in(folder),
            None => pick_executable(),
        }
    }
}

fn pick_file(executable: bool) -> Result<Option<String>, LocalError> {
    pick_file_starting_at(executable, None, None)
}

fn pick_file_starting_at(
    executable: bool,
    extensions: Option<&[String]>,
    initial: Option<&str>,
) -> Result<Option<String>, LocalError> {
    #[cfg(not(windows))]
    {
        let _ = (executable, extensions, initial);
        Err(LocalError::Unsupported)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Controls::Dialogs::{
            CommDlgExtendedError, GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR,
            OFN_PATHMUSTEXIST, OPENFILENAMEW,
        };
        /// Builds the double-NUL-terminated filter list `GetOpenFileNameW` expects.
        fn filter_list(executable: bool, extensions: Option<&[String]>) -> Vec<u16> {
            let mut text = String::new();
            if executable {
                text.push_str("Windows executable\0*.exe\0");
            } else if let Some(extensions) = extensions.filter(|list| !list.is_empty()) {
                text.push_str("Supported files\0");
                for (index, extension) in extensions.iter().enumerate() {
                    let extension = extension.trim().trim_start_matches('.');
                    if extension.is_empty() {
                        continue;
                    }
                    if index > 0 {
                        text.push(';');
                    }
                    text.push('*');
                    text.push('.');
                    text.push_str(extension);
                }
                text.push_str("\0");
            }
            text.push_str("All files\0*.*\0\0");
            text.encode_utf16().collect()
        }

        let mut path = vec![0u16; 32768];
        let filter = filter_list(executable, extensions);
        let initial_dir: Vec<u16> = initial
            .filter(|folder| !folder.is_empty())
            .map(|folder| folder.encode_utf16().chain(std::iter::once(0)).collect())
            .unwrap_or_default();
        let mut data: OPENFILENAMEW = unsafe { std::mem::zeroed() };
        data.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        data.lpstrFilter = filter.as_ptr();
        data.lpstrFile = path.as_mut_ptr();
        data.nMaxFile = path.len() as u32;
        if !initial_dir.is_empty() {
            data.lpstrInitialDir = initial_dir.as_ptr();
        }
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
    if let Some((ptr, len)) = query("\\VarFileInfo\\Translation").filter(|&(ptr, len)| {
        len <= 4096 && ptr as usize + len <= data.as_ptr() as usize + data.len()
    }) {
        // SAFETY: output lies inside the bounded version-info buffer; copy unaligned words.
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
        for c in bytes.as_chunks::<4>().0 {
            translations.push((
                u16::from_le_bytes([c[0], c[1]]),
                u16::from_le_bytes([c[2], c[3]]),
            ));
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
                    .as_chunks::<2>()
                    .0
                    .iter()
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

pub fn pick_save_location(folder: bool) -> Result<Option<String>, LocalError> {
    if !folder {
        return pick_file(false);
    }
    pick_folder(None)
}

#[cfg(test)]
mod custom_launch_tests {
    use super::*;

    #[test]
    fn spawned_fixture_receives_literal_arguments_cwd_and_environment_delta() {
        let directory = tempfile::tempdir().unwrap();
        let interpreter = std::process::Command::new("python3")
            .args(["-c", "import sys; print(sys.executable)"])
            .output()
            .expect("Python is available to the repository tooling");
        assert!(interpreter.status.success());
        let interpreter = String::from_utf8(interpreter.stdout).unwrap();
        let interpreter = interpreter.trim();
        let script = "import json,os,sys; print(json.dumps({'executable':sys.executable, 'argv':sys.argv[1:], 'cwd':os.getcwd(), 'set':os.environ.get('GAMENODE_TEST_SET'), 'unset':'PATH' not in os.environ}))";
        let arguments = vec![
            "-c".into(),
            script.into(),
            "space and & literal".into(),
            "$(not-a-shell)".into(),
        ];
        let environment = LaunchEnvironmentDelta {
            set: LaunchEnvironment::from([("GAMENODE_TEST_SET".into(), "one value".into())]),
            unset: ["PATH".into()].into(),
        };
        let mut command = custom_command(
            interpreter,
            &arguments,
            &directory.path().to_string_lossy(),
            &environment,
        )
        .unwrap();
        // Process creation and waiting happen on a worker, matching the production contract.
        let output = std::thread::spawn(move || command.output())
            .join()
            .expect("launch worker should finish")
            .expect("fixture process should start without a shell");

        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["executable"], interpreter);
        assert_eq!(
            result["argv"],
            serde_json::json!(["space and & literal", "$(not-a-shell)"])
        );
        assert_eq!(result["cwd"], directory.path().to_string_lossy().as_ref());
        assert_eq!(result["set"], "one value");
        assert_eq!(result["unset"], true);
    }

    #[test]
    fn invalid_environment_deltas_are_rejected_before_process_creation() {
        let invalid_name = LaunchEnvironmentDelta {
            set: LaunchEnvironment::from([("BAD=KEY".into(), "value".into())]),
            unset: Default::default(),
        };
        assert!(!invalid_name.validate());
        assert!(custom_command("python3", &[], ".", &invalid_name).is_err());

        let credential_name = LaunchEnvironmentDelta {
            set: LaunchEnvironment::from([("STEAM_SESSION".into(), "value".into())]),
            unset: Default::default(),
        };
        assert!(!credential_name.validate());
    }

    #[test]
    fn inherited_custom_launch_environment_is_restricted_to_host_allowlisted_keys() {
        assert!(safe_game_environment_key(std::ffi::OsStr::new("PATH")));
        #[cfg(not(windows))]
        assert!(safe_game_environment_key(std::ffi::OsStr::new("HOME")));
        #[cfg(not(windows))]
        {
            assert!(safe_game_environment_key(std::ffi::OsStr::new(
                "XAUTHORITY"
            )));
            assert!(safe_game_environment_key(std::ffi::OsStr::new(
                "QT_QPA_PLATFORM"
            )));
        }
        assert!(!safe_game_environment_key(std::ffi::OsStr::new(
            "STEAM_SESSION"
        )));
        assert!(!safe_game_environment_key(std::ffi::OsStr::new(
            "TAURI_PLUGIN_TOKEN"
        )));
        assert!(!safe_game_environment_key(std::ffi::OsStr::new("RUST_LOG")));
    }
}

fn pick_folder(initial: Option<&str>) -> Result<Option<String>, LocalError> {
    #[cfg(not(windows))]
    {
        let _ = initial;
        Err(LocalError::Unsupported)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::{HWND, LPARAM},
            System::Com::{
                COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize,
            },
            UI::Shell::{
                BFFM_INITIALIZED, BFFM_SETSELECTIONW, BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS,
                BROWSEINFOW, SHBrowseForFolderW, SHGetPathFromIDListW,
            },
            UI::WindowsAndMessaging::SendMessageW,
        };
        // SAFETY: sends the selected folder to the dialog during initialization; both pointers
        // stay valid for the duration of the modal call.
        unsafe extern "system" fn select_initial(
            hwnd: HWND,
            message: u32,
            _lparam: LPARAM,
            data: LPARAM,
        ) -> i32 {
            if message == BFFM_INITIALIZED && data != 0 {
                unsafe {
                    SendMessageW(hwnd, BFFM_SETSELECTIONW, 1, data);
                }
            }
            0
        }

        // SAFETY: the picker owns this worker's COM apartment until the dialog closes.
        let result = unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
        if result < 0 {
            return Err(LocalError::Failed(
                "folder picker initialization failed".into(),
            ));
        }
        struct Apartment;
        impl Drop for Apartment {
            fn drop(&mut self) {
                unsafe {
                    CoUninitialize();
                }
            }
        }
        let _apartment = Apartment;
        let initial: Vec<u16> = initial
            .filter(|folder| !folder.is_empty())
            .map(|folder| folder.encode_utf16().chain(std::iter::once(0)).collect())
            .unwrap_or_default();
        let options = BROWSEINFOW {
            ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
            lpfn: (!initial.is_empty()).then_some(select_initial as _),
            lParam: if initial.is_empty() {
                0
            } else {
                initial.as_ptr() as isize
            },
            ..Default::default()
        };
        // SAFETY: options live through the modal native call; the returned item list is freed below.
        let item = unsafe { SHBrowseForFolderW(&options) };
        if item.is_null() {
            return Ok(None);
        }
        let mut path = [0u16; 260];
        let success = unsafe { SHGetPathFromIDListW(item, path.as_mut_ptr()) };
        unsafe {
            CoTaskMemFree(item.cast());
        }
        if success == 0 {
            return Err(LocalError::InvalidTarget);
        }
        let length = path.iter().position(|c| *c == 0).unwrap_or(path.len());
        Ok(Some(String::from_utf16_lossy(&path[..length])))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(not(windows))]
    fn unsupported_on_linux() {
        use gamenode_core::drm_free::{
            CustomLaunchCommand, InstallationHost, LaunchEnvironmentDelta, Registration,
        };
        let host = super::WindowsInstallations;
        let registration = Registration {
            id: "test".into(),
            title: "test".into(),
            executable: "game.exe".into(),
            file_identity: "1:2:3".into(),
            root: "/game".into(),
            working_directory: "/game".into(),
            arguments: vec![],
            gameplay: vec![],
            launchers: vec![],
            revision: 1,
            removed: false,
            rediscovery_allowed: false,
            transferred_from: None,
            custom_cover: None,
            available: true,
        };
        assert_eq!(
            host.inspect("game.exe"),
            Err(super::LocalError::Unsupported)
        );
        assert_eq!(
            host.validate_registration(&registration),
            Err(super::LocalError::Unsupported)
        );
        assert_eq!(
            host.launch(&registration),
            Err(super::LocalError::Unsupported)
        );
        assert_eq!(
            host.launch_with_environment(
                &registration,
                &gamenode_core::drm_free::LaunchEnvironment::new()
            ),
            Err(super::LocalError::Unsupported)
        );
        assert_eq!(
            host.launch_custom(&CustomLaunchCommand {
                executable: "game.exe".into(),
                file_identity: "1:2:3".into(),
                arguments: vec!["--profile".into()],
                working_directory: "/game".into(),
                environment: LaunchEnvironmentDelta::default(),
            }),
            Err(super::LocalError::Unsupported)
        );
        assert_eq!(
            host.evidence(&registration),
            Err(super::LocalError::Unsupported)
        );
        assert_eq!(
            super::pick_executable(),
            Err(super::LocalError::Unsupported)
        );
        assert_eq!(
            super::pick_save_location(true),
            Err(super::LocalError::Unsupported)
        );
        assert_eq!(
            super::pick_save_location(false),
            Err(super::LocalError::Unsupported)
        );
        assert_eq!(
            super::pick_folder_starting_at(Some("D:/Ports")),
            Err(super::LocalError::Unsupported)
        );
        assert_eq!(
            super::pick_file_with_extensions(&["pkg".into()], None),
            Err(super::LocalError::Unsupported)
        );
        assert_eq!(
            super::pick_executable_in("D:/Ports/Bloodborne"),
            Err(super::LocalError::Unsupported)
        );
    }

    #[test]
    #[cfg(not(windows))]
    fn a_port_environment_is_applied_to_the_launch_command() {
        use gamenode_core::drm_free::LaunchEnvironment;
        let mut command = std::process::Command::new("game.exe");
        let environment =
            LaunchEnvironment::from([("BLOODBORNE_PATH".to_string(), "D:/dump.iso".to_string())]);
        super::apply_environment(&mut command, &environment);

        let variables: Vec<(String, Option<String>)> = command
            .get_envs()
            .map(|(name, value)| {
                (
                    name.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect();
        assert_eq!(
            variables,
            vec![(
                "BLOODBORNE_PATH".to_string(),
                Some("D:/dump.iso".to_string())
            )]
        );
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    #[test]
    fn inspect_alias_identity_and_validate_before_launch() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("jogo & teste com espaços 日本語.exe");
        let system = std::env::var_os("SystemRoot").unwrap();
        std::fs::copy(
            std::path::Path::new(&system).join("System32/cmd.exe"),
            &path,
        )
        .unwrap();
        let host = WindowsInstallations;
        let dll = directory.path().join("disguised.exe");
        std::fs::copy(
            std::path::Path::new(&system).join("System32/kernel32.dll"),
            &dll,
        )
        .unwrap();
        assert!(host.inspect(dll.to_str().unwrap()).is_err());
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
        let invalid = directory.path().join("invalid.exe");
        std::fs::write(&invalid, b"not a PE").unwrap();
        assert!(host.inspect(invalid.to_str().unwrap()).is_err());
        // Launch returns promptly; wait only in the fixture for Windows to release the image.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match std::fs::write(&path, b"not a PE") {
                Ok(()) => break,
                Err(error)
                    if error.raw_os_error() == Some(32) && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(20))
                }
                Err(error) => panic!("{error}"),
            }
        }
        assert!(host.inspect(path.to_str().unwrap()).is_err());
    }
    #[test]
    fn replaced_junction_cannot_launch_even_with_the_same_file_identity() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("game");
        let bin = root.join("bin");
        let outside = fixture.path().join("outside");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir(&outside).unwrap();
        let system = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/cmd.exe");
        let executable = bin.join("game.exe");
        std::fs::copy(&system, &executable).unwrap();
        let host = WindowsInstallations;
        let mut registration = host.inspect(executable.to_str().unwrap()).unwrap();
        registration.root = root.to_string_lossy().into_owned();
        assert!(host.validate_registration(&registration).is_ok());
        let old = root.join("old-bin");
        std::fs::rename(&bin, &old).unwrap();
        std::fs::hard_link(old.join("game.exe"), outside.join("game.exe")).unwrap();
        let script = fixture.path().join("junction.ps1");
        std::fs::write(&script, b"param([string]$Link,[string]$Target)\nNew-Item -ItemType Junction -Path $Link -Target $Target -ErrorAction Stop | Out-Null\n").unwrap();
        let powershell = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let output = std::process::Command::new(powershell)
            .current_dir(fixture.path())
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&script)
            .arg(&bin)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            format!(
                "stdout={} stderr={} bin={} outside={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
                bin.display(),
                outside.display()
            )
        );
        assert_eq!(
            identity(&canonical(executable.to_str().unwrap()).unwrap()).unwrap(),
            registration.file_identity
        );
        assert_eq!(host.launch(&registration), Err(LocalError::InvalidTarget));
        assert!(host.validate_registration(&registration).is_err());
        std::fs::remove_dir(&bin).unwrap();
    }
    #[test]
    fn failed_process_start_returns_failed_error() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("bad.exe");
        let mut data = vec![0u8; 512];
        data[0..2].copy_from_slice(b"MZ");
        data[60..64].copy_from_slice(&64u32.to_le_bytes());
        data[64..68].copy_from_slice(b"PE\0\0");
        data[68..70].copy_from_slice(&0x8664u16.to_le_bytes());
        data[70..72].copy_from_slice(&1u16.to_le_bytes());
        data[84..86].copy_from_slice(&240u16.to_le_bytes());
        data[86..88].copy_from_slice(&2u16.to_le_bytes());
        data[88..90].copy_from_slice(&0x20bu16.to_le_bytes());
        data[156..158].copy_from_slice(&2u16.to_le_bytes());
        std::fs::write(&path, &data).unwrap();
        let host = WindowsInstallations;
        let registration = host.inspect(path.to_str().unwrap()).unwrap();
        assert!(matches!(
            host.launch(&registration),
            Err(LocalError::Failed(_))
        ));
    }
    #[test]
    fn evidence_inspects_pe_metadata_without_executing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cmd.exe");
        let system = std::env::var_os("SystemRoot").unwrap();
        std::fs::copy(
            std::path::Path::new(&system).join("System32/cmd.exe"),
            &path,
        )
        .unwrap();
        let host = WindowsInstallations;
        let registration = host.inspect(path.to_str().unwrap()).unwrap();
        let evidence = host.evidence(&registration).unwrap();
        assert!(!evidence.is_empty());
        assert!(
            evidence
                .iter()
                .any(|e| e.kind == gamenode_core::drm_free::EvidenceKind::Executable)
        );
    }
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::*;
    use gamenode_core::drm_free::LaunchEnvironmentDelta;
    use std::{collections::BTreeSet, os::unix::fs::PermissionsExt};

    fn executable(directory: &std::path::Path, name: &str) -> std::path::PathBuf {
        let path = directory.join(name);
        std::fs::write(
            &path,
            "#!/bin/sh\nprintf '%s\\n' \"$PWD\" \"$@\" \"$PLAN_MARKER\" > \"$CAPTURE\"\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
        path
    }

    fn wait_for_capture(path: &std::path::Path) -> String {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            if let Ok(result) = std::fs::read_to_string(path) {
                return result;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "launch fixture timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn direct_linux_launch_keeps_approved_identity_and_reviewed_fields() {
        let directory = tempfile::tempdir().unwrap();
        let working = directory.path().join("game folder");
        std::fs::create_dir(&working).unwrap();
        let game = executable(&working, "game");
        let capture = directory.path().join("direct.out");
        let host = LinuxInstallations;
        let approved = host.inspect(game.to_str().unwrap()).unwrap();
        let request = CustomLaunchCommand {
            executable: approved.executable.clone(),
            file_identity: approved.file_identity.clone(),
            arguments: vec!["--profile".into(), "two words".into(), "semi;colon".into()],
            working_directory: working.to_string_lossy().into_owned(),
            environment: LaunchEnvironmentDelta {
                set: [
                    ("CAPTURE".into(), capture.to_string_lossy().into_owned()),
                    ("PLAN_MARKER".into(), "reviewed".into()),
                ]
                .into(),
                unset: BTreeSet::new(),
            },
        };
        host.launch_custom(&request).unwrap();
        assert_eq!(
            wait_for_capture(&capture),
            format!(
                "{}\n--profile\ntwo words\nsemi;colon\nreviewed\n",
                working.display()
            )
        );

        let mut stale = request;
        stale.file_identity.push_str(":stale");
        assert_eq!(host.launch_custom(&stale), Err(LocalError::InvalidTarget));
    }

    #[test]
    fn proton_style_wrapper_receives_the_exact_reviewed_game_arguments_and_context() {
        let directory = tempfile::tempdir().unwrap();
        let working = directory.path().join("prefix");
        std::fs::create_dir(&working).unwrap();
        let wrapper = executable(&working, "approved-proton-wrapper");
        let capture = directory.path().join("wrapper.out");
        let host = LinuxInstallations;
        let approved = host.inspect(wrapper.to_str().unwrap()).unwrap();
        let request = CustomLaunchCommand {
            executable: approved.executable,
            file_identity: approved.file_identity,
            arguments: vec![
                "run".into(),
                "--prefix".into(),
                "/approved/prefix with spaces".into(),
                "--".into(),
                "Z:/Games/Example/game.exe".into(),
                "--name".into(),
                "value with spaces".into(),
            ],
            working_directory: working.to_string_lossy().into_owned(),
            environment: LaunchEnvironmentDelta {
                set: [
                    ("CAPTURE".into(), capture.to_string_lossy().into_owned()),
                    ("PLAN_MARKER".into(), "proton-reviewed".into()),
                ]
                .into(),
                unset: BTreeSet::new(),
            },
        };
        host.launch_custom(&request).unwrap();
        assert_eq!(
            wait_for_capture(&capture),
            format!(
                "{}\nrun\n--prefix\n/approved/prefix with spaces\n--\nZ:/Games/Example/game.exe\n--name\nvalue with spaces\nproton-reviewed\n",
                working.display()
            )
        );
    }

    #[test]
    fn linux_custom_launch_rejects_non_executable_and_missing_working_directory() {
        let directory = tempfile::tempdir().unwrap();
        let non_executable = directory.path().join("not-executable");
        std::fs::write(&non_executable, "data").unwrap();
        assert_eq!(
            LinuxInstallations.inspect(non_executable.to_str().unwrap()),
            Err(LocalError::InvalidTarget)
        );

        let game = executable(directory.path(), "game");
        let approved = LinuxInstallations.inspect(game.to_str().unwrap()).unwrap();
        let request = CustomLaunchCommand {
            executable: approved.executable,
            file_identity: approved.file_identity,
            arguments: vec![],
            working_directory: directory
                .path()
                .join("missing")
                .to_string_lossy()
                .into_owned(),
            environment: LaunchEnvironmentDelta::default(),
        };
        assert_eq!(
            LinuxInstallations.launch_custom(&request),
            Err(LocalError::InvalidTarget)
        );
    }

    #[test]
    fn missing_wrapper_and_unsupported_environment_fields_fail_before_spawn() {
        let directory = tempfile::tempdir().unwrap();
        let game = executable(directory.path(), "game");
        let approved = LinuxInstallations.inspect(game.to_str().unwrap()).unwrap();
        let missing_output = directory.path().join("should-not-exist.out");
        let request = CustomLaunchCommand {
            executable: directory
                .path()
                .join("removed-wrapper")
                .to_string_lossy()
                .into_owned(),
            file_identity: approved.file_identity.clone(),
            arguments: vec!["run".into()],
            working_directory: directory.path().to_string_lossy().into_owned(),
            environment: LaunchEnvironmentDelta {
                set: [(
                    "CAPTURE".into(),
                    missing_output.to_string_lossy().into_owned(),
                )]
                .into(),
                unset: BTreeSet::new(),
            },
        };
        assert_eq!(
            LinuxInstallations.launch_custom(&request),
            Err(LocalError::InvalidTarget)
        );
        assert!(!missing_output.exists());

        let unsupported_environment = CustomLaunchCommand {
            executable: approved.executable,
            file_identity: approved.file_identity,
            arguments: vec![],
            working_directory: directory.path().to_string_lossy().into_owned(),
            environment: LaunchEnvironmentDelta {
                set: [("STEAM_SESSION".into(), "must-not-leak".into())].into(),
                unset: BTreeSet::new(),
            },
        };
        assert_eq!(
            LinuxInstallations.launch_custom(&unsupported_environment),
            Err(LocalError::InvalidTarget)
        );
        assert!(!missing_output.exists());
    }
}
