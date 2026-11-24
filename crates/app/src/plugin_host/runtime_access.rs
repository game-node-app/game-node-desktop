//! Host-owned approval of one exact emulator executable.
//!
//! A plugin never names a path. It asks [`RuntimesRequestAccess`](crate::plugin_host::resources)
//! with text hints; the host matches its own installed-program inventory, shows a picker, valides
//! the program the user chose and records a durable [`RuntimeApproval`]. The runtime record points
//! at the approval, and every launch rechecks the approval's ownership, identity, size and
//! digest. None of the refusal messages or logs carries a path.
//!
//! The executable is a single file, not an approved folder: this approval grants no file access
//! and no traversal beside the program.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use gamenode_platform::plugin_files::{self, Reserved};
use gamenode_platform::plugin_storage::RuntimeApproval;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;
use tonic::Status;

/// The most a suggested executable may be; larger files are refused instead of hashed.
pub const MAX_EXECUTABLE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
/// The most suggestions the picker shows for one request.
pub const MAX_SUGGESTIONS: usize = 32;
/// The largest text hint, in bytes.
const MAX_HINT_BYTES: usize = 256;
const MAX_HINTS: usize = 32;

/// One host-owned installed program. Only the official PackageKit provider contributes entries
/// today; a third-party provider's records never become candidates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledProgram {
    pub display_name: String,
    pub executable: String,
    pub desktop_file: String,
    pub package_id: String,
    pub package_manager: String,
}

/// A program the picker offers, with the facts the user reviews before approving it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeExecutableCandidate {
    pub display_name: String,
    pub package_manager: String,
    pub package_id: String,
    /// The package version when the package id carries one; empty otherwise.
    pub version: String,
    /// The exact path that would be approved.
    pub path: String,
}

/// The text hints an extension declared. They are matched against host inventory and are never
/// paths.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuntimeHints {
    pub executable_names: Vec<String>,
    pub desktop_ids: Vec<String>,
}

impl RuntimeHints {
    pub fn validate(&self) -> bool {
        let valid = |value: &str| {
            !value.is_empty()
                && value.len() <= MAX_HINT_BYTES
                && !value
                    .chars()
                    .any(|character| character.is_control() || matches!(character, '/' | '\\'))
        };
        self.executable_names.len() + self.desktop_ids.len() <= MAX_HINTS
            && self
                .executable_names
                .iter()
                .chain(&self.desktop_ids)
                .all(|hint| valid(hint))
    }

    pub fn is_empty(&self) -> bool {
        self.executable_names.is_empty() && self.desktop_ids.is_empty()
    }
}

/// What the host asks the user to choose. The candidate paths are host-owned and intentionally
/// shown; nothing else in the request is a path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeAccessRequest {
    pub plugin_id: String,
    pub publisher_fingerprint: String,
    pub runtime_name: String,
    pub runtime_version: String,
    pub platform: String,
    pub supported_formats: Vec<String>,
    pub hints: RuntimeHints,
    pub candidates: Vec<RuntimeExecutableCandidate>,
}

/// Plain event data for the runtime picker. Paths are shown because the user approves one exact
/// program; they never travel back to the plugin or into logs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSelectionPrompt {
    pub id: String,
    pub plugin_id: String,
    pub publisher_fingerprint: String,
    pub runtime_name: String,
    pub runtime_version: String,
    pub platform: String,
    pub supported_formats: Vec<String>,
    pub hints: Vec<String>,
    pub candidates: Vec<RuntimeExecutableCandidate>,
}

/// The user's side of the runtime approval. The host calls it off the UI thread and a call that
/// takes longer than [`APPROVAL_TIMEOUT`](super::files::APPROVAL_TIMEOUT) counts as declined.
#[async_trait]
pub trait RuntimeApprover: Send + Sync {
    /// Lets the user choose one executable. `None` is a refusal.
    async fn choose_executable(
        &self,
        request: RuntimeAccessRequest,
    ) -> Result<Option<PathBuf>, Status>;
}

/// The window event that opens the runtime picker.
pub const RUNTIME_SELECT_EVENT: &str = "plugin:runtime-approval-select";
/// The window event that closes a runtime prompt that was withdrawn or timed out.
pub const RUNTIME_CLOSED_EVENT: &str = "plugin:runtime-approval-closed";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeApprovalSignal {
    Select(RuntimeSelectionPrompt),
    Closed(String),
}

pub type RuntimeApprovalEmit = Arc<dyn Fn(RuntimeApprovalSignal) -> bool + Send + Sync>;

struct ForgetRuntimePrompt<'a> {
    bridge: &'a RuntimeApprovalBridge,
    id: String,
}

impl Drop for ForgetRuntimePrompt<'_> {
    fn drop(&mut self) {
        let removed = self
            .bridge
            .waiting
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.id)
            .is_some();
        if removed {
            (self.bridge.emit)(RuntimeApprovalSignal::Closed(self.id.clone()));
        }
    }
}

/// Bridges runtime approval requests to the React window without making the host wait on the UI
/// thread. The call site owns the timeout.
pub struct RuntimeApprovalBridge {
    emit: RuntimeApprovalEmit,
    waiting: Mutex<BTreeMap<String, oneshot::Sender<Option<String>>>>,
}

impl RuntimeApprovalBridge {
    pub fn new(emit: RuntimeApprovalEmit) -> Self {
        Self {
            emit,
            waiting: Mutex::default(),
        }
    }

    fn id() -> Result<String, Status> {
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| Status::internal("no source of randomness"))?;
        Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
    }

    /// Answers a pending runtime prompt. A repeated or unknown answer is ignored.
    pub fn answer(&self, id: &str, selected: Option<String>) -> bool {
        let sender = self
            .waiting
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(id);
        sender.is_some_and(|sender| sender.send(selected).is_ok())
    }

    async fn choose(&self, request: RuntimeAccessRequest) -> Result<Option<String>, Status> {
        let id = Self::id()?;
        let (send, receive) = oneshot::channel();
        self.waiting
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(id.clone(), send);
        let _forget = ForgetRuntimePrompt {
            bridge: self,
            id: id.clone(),
        };
        let mut hints = request.hints.executable_names.clone();
        hints.extend(request.hints.desktop_ids.iter().cloned());
        let shown = (self.emit)(RuntimeApprovalSignal::Select(RuntimeSelectionPrompt {
            id,
            plugin_id: request.plugin_id,
            publisher_fingerprint: request.publisher_fingerprint,
            runtime_name: request.runtime_name,
            runtime_version: request.runtime_version,
            platform: request.platform,
            supported_formats: request.supported_formats,
            hints,
            candidates: request.candidates,
        }));
        if !shown {
            return Err(Status::unavailable("there is no window to ask the user in"));
        }
        receive
            .await
            .map_err(|_| Status::unavailable("the runtime selection was closed"))
    }
}

#[async_trait]
impl RuntimeApprover for RuntimeApprovalBridge {
    async fn choose_executable(
        &self,
        request: RuntimeAccessRequest,
    ) -> Result<Option<PathBuf>, Status> {
        Ok(self.choose(request).await?.map(PathBuf::from))
    }
}

/// A validated executable the host may approve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InspectedExecutable {
    /// The canonical, valid-Unicode path stored in the approval.
    pub path: String,
    pub volume: u64,
    pub node: u64,
    pub size: u64,
    /// Lowercase hexadecimal SHA-256.
    pub digest: String,
}

/// Extracts the picker inventory from stored library entries. Only the official PackageKit
/// provider contributes; a third-party provider's record never becomes a candidate, even when its
/// fields contain something named `executable`.
pub fn installed_programs_from_entries(
    entries: &[gamenode_core::library::StoredEntry],
) -> Vec<InstalledProgram> {
    entries
        .iter()
        .filter(|entry| entry.installed && entry.provider == "packagekit")
        .filter_map(|entry| {
            let executable = entry.fields.get("executable")?.trim();
            if executable.is_empty() {
                return None;
            }
            Some(InstalledProgram {
                display_name: entry
                    .name
                    .clone()
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or_else(|| entry.store_id.clone()),
                executable: executable.to_owned(),
                desktop_file: entry
                    .fields
                    .get("desktop_file")
                    .map(|value| value.trim().to_owned())
                    .unwrap_or_default(),
                package_id: entry
                    .fields
                    .get("package_id")
                    .map(|value| value.trim().to_owned())
                    .unwrap_or_default(),
                package_manager: entry
                    .fields
                    .get("package_manager")
                    .map(|value| value.trim().to_owned())
                    .unwrap_or_default(),
            })
        })
        .collect()
}

/// Matches host inventory against the extension's hints. Vendored package ids never contribute:
/// callers pass only entries from the official inventory reader.
pub fn suggestions(
    programs: &[InstalledProgram],
    hints: &RuntimeHints,
) -> Vec<RuntimeExecutableCandidate> {
    if hints.is_empty() {
        return Vec::new();
    }
    let mut by_path: BTreeMap<String, (bool, RuntimeExecutableCandidate)> = BTreeMap::new();
    for program in programs {
        if !valid_inventory_path(&program.executable) {
            continue;
        }
        let executable = Path::new(&program.executable);
        let executable_name = executable
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let executable_stem = executable
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let desktop = Path::new(&program.desktop_file);
        let desktop_name = desktop
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let desktop_stem = desktop
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();

        let executable_match = hints.executable_names.iter().any(|hint| {
            component_matches(hint, &executable_name) || component_matches(hint, &executable_stem)
        });
        let desktop_match = hints.desktop_ids.iter().any(|hint| {
            component_matches(hint, &desktop_name) || component_matches(hint, &desktop_stem)
        });
        if !executable_match && !desktop_match {
            continue;
        }
        let display_name = program
            .display_name
            .trim()
            .chars()
            .take(256)
            .collect::<String>();
        let display_name = if display_name.is_empty() {
            executable_stem.clone()
        } else {
            display_name
        };
        let candidate = RuntimeExecutableCandidate {
            display_name,
            package_manager: plain_inventory_text(&program.package_manager),
            package_id: plain_inventory_text(&program.package_id),
            version: package_version(&program.package_id),
            path: program.executable.trim().to_owned(),
        };
        by_path
            .entry(candidate.path.clone())
            .and_modify(|(existing, _)| *existing |= executable_match)
            .or_insert((executable_match, candidate));
    }
    let mut candidates: Vec<_> = by_path.into_values().collect();
    candidates.sort_by(|(left_exact, left), (right_exact, right)| {
        right_exact
            .cmp(left_exact)
            .then_with(|| {
                left.display_name
                    .to_lowercase()
                    .cmp(&right.display_name.to_lowercase())
            })
            .then_with(|| left.path.cmp(&right.path))
    });
    candidates
        .into_iter()
        .take(MAX_SUGGESTIONS)
        .map(|(_, candidate)| candidate)
        .collect()
}

fn component_matches(hint: &str, component: &str) -> bool {
    let hint = hint.trim();
    !hint.is_empty() && component.eq_ignore_ascii_case(hint)
}

fn plain_inventory_text(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(256)
        .collect()
}

/// `name;version;arch;repository` from PackageKit, or just a package name.
fn package_version(package_id: &str) -> String {
    package_id
        .split(';')
        .nth(1)
        .map(plain_inventory_text)
        .unwrap_or_default()
}

fn valid_inventory_path(path: &str) -> bool {
    !path.trim().is_empty()
        && path.len() <= 4096
        && !path
            .chars()
            .any(|character| character == '\0' || character.is_control())
}

/// Validates a path the user chose and records what will be approved.
///
/// Blocking: it canonicalizes and hashes the file. The final component may not be a link; a
/// linked parent is resolved by the user's choice, and the stored canonical path is what is
/// rechecked later.
pub fn inspect_executable(
    chosen: &Path,
    reserved: &Reserved,
) -> Result<InspectedExecutable, Status> {
    let link_metadata = std::fs::symlink_metadata(chosen)
        .map_err(|_| Status::failed_precondition("the selected program does not exist"))?;
    if link_metadata.file_type().is_symlink() {
        return Err(Status::permission_denied(
            "the selected program may not be a link",
        ));
    }
    if !link_metadata.is_file() {
        return Err(Status::failed_precondition(
            "the selected program is not a regular file",
        ));
    }
    let canonical = std::fs::canonicalize(chosen)
        .map_err(|_| Status::failed_precondition("the selected program does not exist"))?;
    let metadata = std::fs::metadata(&canonical)
        .map_err(|_| Status::failed_precondition("the selected program does not exist"))?;
    if !metadata.is_file() {
        return Err(Status::failed_precondition(
            "the selected program is not a regular file",
        ));
    }
    if reserved.is_reserved(&canonical) {
        return Err(Status::permission_denied(
            "that program cannot be approved for a plugin",
        ));
    }
    if !is_executable(&canonical, &metadata) {
        return Err(Status::failed_precondition(
            "the selected program is not executable",
        ));
    }
    if metadata.len() > MAX_EXECUTABLE_BYTES {
        return Err(Status::resource_exhausted(
            "the selected program is too large to approve",
        ));
    }
    let identity = plugin_files::identity_of(&canonical)
        .map_err(|_| Status::failed_precondition("the selected program cannot be inspected"))?;
    let digest = hash_file(&canonical)
        .map_err(|_| Status::failed_precondition("the selected program cannot be read"))?;
    let path = canonical
        .to_str()
        .ok_or_else(|| {
            Status::failed_precondition("the selected program path is not valid Unicode")
        })?
        .to_owned();
    Ok(InspectedExecutable {
        path,
        volume: identity.volume,
        node: identity.node,
        size: metadata.len(),
        digest,
    })
}

/// Rechecks a durable approval before it is reused or launched. Any changed identity, size or
/// digest invalidates it; the caller must not fall back to another path.
///
/// Blocking: it hashes the file.
pub fn validate_approval(
    approval: &RuntimeApproval,
    reserved: &Reserved,
) -> Result<String, Status> {
    if approval.revoked_at.is_some() {
        return Err(Status::failed_precondition(
            "the approved program is no longer approved",
        ));
    }
    if approval.path.is_empty() || approval.digest.len() != 64 {
        return Err(Status::failed_precondition(
            "the approved program is unavailable; request access again",
        ));
    }
    let path = Path::new(&approval.path);
    let link_metadata = std::fs::symlink_metadata(path)
        .map_err(|_| Status::failed_precondition("the approved program changed or is missing"))?;
    if link_metadata.file_type().is_symlink() || !link_metadata.is_file() {
        return Err(Status::failed_precondition(
            "the approved program changed; request access again",
        ));
    }
    let canonical = std::fs::canonicalize(path)
        .map_err(|_| Status::failed_precondition("the approved program changed or is missing"))?;
    if canonical != path {
        return Err(Status::failed_precondition(
            "the approved program changed; request access again",
        ));
    }
    if reserved.is_reserved(&canonical) {
        return Err(Status::permission_denied(
            "the approved program cannot be started for a plugin",
        ));
    }
    let metadata = std::fs::metadata(&canonical)
        .map_err(|_| Status::failed_precondition("the approved program changed or is missing"))?;
    if !metadata.is_file() || !is_executable(&canonical, &metadata) {
        return Err(Status::failed_precondition(
            "the approved program changed; request access again",
        ));
    }
    if metadata.len() != approval.size {
        return Err(Status::failed_precondition(
            "the approved program changed; request access again",
        ));
    }
    let identity = plugin_files::identity_of(&canonical)
        .map_err(|_| Status::failed_precondition("the approved program cannot be inspected"))?;
    if identity.volume != approval.volume || identity.node != approval.node {
        return Err(Status::failed_precondition(
            "the approved program changed; request access again",
        ));
    }
    let digest = hash_file(&canonical)
        .map_err(|_| Status::failed_precondition("the approved program cannot be read"))?;
    if !digest.eq_ignore_ascii_case(&approval.digest) {
        return Err(Status::failed_precondition(
            "the approved program changed; request access again",
        ));
    }
    Ok(approval.path.clone())
}

fn hash_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(unix)]
fn is_executable(path: &Path, metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.is_file() && metadata.permissions().mode() & 0o111 != 0
}

#[cfg(windows)]
fn is_executable(path: &Path, metadata: &std::fs::Metadata) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            metadata.is_file() && matches!(extension.to_ascii_lowercase().as_str(), "exe" | "com")
        })
}

#[cfg(not(any(unix, windows)))]
fn is_executable(_path: &Path, metadata: &std::fs::Metadata) -> bool {
    metadata.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(id: &str, name: &str, executable: &str, desktop: &str) -> InstalledProgram {
        InstalledProgram {
            display_name: name.into(),
            executable: executable.into(),
            desktop_file: desktop.into(),
            package_id: id.into(),
            package_manager: "pacman".into(),
        }
    }

    #[test]
    fn hints_match_executable_base_names_and_desktop_ids_without_duplicates() {
        let programs = vec![
            program(
                "dolphin-emu;2503-1;x86_64;extra",
                "Dolphin Emulator",
                "/usr/bin/dolphin-emu",
                "/usr/share/applications/dolphin-emu.desktop",
            ),
            program(
                "dolphin-emu;2503-1;x86_64;extra",
                "Dolphin Emulator (duplicate path)",
                "/usr/bin/dolphin-emu",
                "/usr/share/applications/dolphin-emu.desktop",
            ),
            program(
                "unrelated;1;x86_64;extra",
                "Unrelated",
                "/usr/bin/unrelated",
                "/usr/share/applications/unrelated.desktop",
            ),
        ];
        let suggestions = suggestions(
            &programs,
            &RuntimeHints {
                executable_names: vec!["dolphin-emu".into()],
                desktop_ids: vec!["dolphin-emu.desktop".into()],
            },
        );
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].display_name, "Dolphin Emulator");
        assert_eq!(suggestions[0].path, "/usr/bin/dolphin-emu");
        assert_eq!(suggestions[0].version, "2503-1");
        assert_eq!(suggestions[0].package_manager, "pacman");
    }

    #[test]
    fn desktop_id_hint_matches_and_empty_inventory_yields_no_candidates() {
        let programs = vec![program(
            "dolphin-emu;2503-1;x86_64;extra",
            "Dolphin Emulator",
            "/usr/bin/dolphin-emu",
            "/usr/share/applications/dolphin-emu.desktop",
        )];
        let by_desktop = suggestions(
            &programs,
            &RuntimeHints {
                executable_names: Vec::new(),
                desktop_ids: vec!["dolphin-emu".into()],
            },
        );
        assert_eq!(by_desktop.len(), 1);
        assert!(
            suggestions(
                &[],
                &RuntimeHints {
                    executable_names: vec!["dolphin-emu".into()],
                    desktop_ids: Vec::new(),
                }
            )
            .is_empty()
        );
        assert!(suggestions(&programs, &RuntimeHints::default()).is_empty());
    }

    #[test]
    fn only_official_packagekit_entries_become_inventory() {
        let entry = |provider: &str, executable: &str| gamenode_core::library::StoredEntry {
            key: format!("{provider}:dolphin"),
            provider: provider.into(),
            store_id: "dolphin".into(),
            name: Some("Dolphin Emulator".into()),
            install_dir: "/usr/bin".into(),
            fields: BTreeMap::from([
                ("executable".into(), executable.into()),
                (
                    "desktop_file".into(),
                    "/usr/share/applications/dolphin.desktop".into(),
                ),
                ("package_id".into(), "dolphin;1;x86_64;extra".into()),
                ("package_manager".into(), "pacman".into()),
            ]),
            installed: true,
            target: None,
            owned: false,
            shared: false,
            first_seen: 0,
            last_seen: 0,
        };
        let inventory = installed_programs_from_entries(&[
            entry("packagekit", "/usr/bin/dolphin-emu"),
            entry("third-party", "/tmp/malicious"),
        ]);
        assert_eq!(inventory.len(), 1);
        assert_eq!(inventory[0].executable, "/usr/bin/dolphin-emu");
        assert_eq!(inventory[0].package_manager, "pacman");
    }

    #[cfg(unix)]
    #[test]
    fn approval_rejects_directories_links_and_non_executables() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("folder");
        std::fs::create_dir(&directory).unwrap();
        assert!(inspect_executable(&directory, &Reserved::default()).is_err());

        let plain = temp.path().join("plain");
        std::fs::write(&plain, b"data").unwrap();
        std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(inspect_executable(&plain, &Reserved::default()).is_err());

        let executable = temp.path().join("emulator");
        std::fs::write(&executable, b"data").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(inspect_executable(&executable, &Reserved::default()).is_ok());

        let link = temp.path().join("emulator-link");
        std::os::unix::fs::symlink(&executable, &link).unwrap();
        assert_eq!(
            inspect_executable(&link, &Reserved::default())
                .unwrap_err()
                .code(),
            tonic::Code::PermissionDenied
        );

        let reserved = Reserved::new(std::slice::from_ref(&executable));
        assert!(inspect_executable(&executable, &reserved).is_err());
    }

    #[test]
    fn hints_reject_paths_and_controls() {
        assert!(
            !RuntimeHints {
                executable_names: vec!["/usr/bin/dolphin-emu".into()],
                desktop_ids: Vec::new(),
            }
            .validate()
        );
        assert!(
            !RuntimeHints {
                executable_names: vec!["dolphin\u{0}emu".into()],
                desktop_ids: Vec::new(),
            }
            .validate()
        );
        assert!(
            RuntimeHints {
                executable_names: vec!["dolphin-emu".into()],
                desktop_ids: vec!["dolphin-emu.desktop".into()],
            }
            .validate()
        );
    }
}
