//! Bounded, read-only discovery of installed packages that own game desktop entries.
//!
//! Two backends produce the same provider-local records: `pacman -Qqo` on Arch-family systems
//! and PackageKit's installed-file search elsewhere. Both backends are queried with argument-based
//! calls, bounded by a timeout, a candidate count and a record count. A backend that is missing or
//! fails returns a typed [`ScanFailure`]; a successful query with no owners returns an empty list.

use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    time::Duration,
};

use async_trait::async_trait;
use futures_util::StreamExt;
use zbus::{Connection, Proxy, zvariant::OwnedObjectPath};

const OS_RELEASE: &str = "/etc/os-release";
const PACMAN: &str = "pacman";
/// Force the C locale so the unowned-file result is a stable, non-localized message.
const PACMAN_LOCALE: &str = "C";
const DESKTOP_DIRECTORIES: [&str; 2] = ["/usr/share/applications", "/usr/local/share/applications"];

const PACKAGEKIT_SERVICE: &str = "org.freedesktop.PackageKit";
const PACKAGEKIT_PATH: &str = "/org/freedesktop/PackageKit";
const PACKAGEKIT_INTERFACE: &str = "org.freedesktop.PackageKit";
const TRANSACTION_INTERFACE: &str = "org.freedesktop.PackageKit.Transaction";
/// `PkFilterEnum::Installed` is enum position 2 in the PackageKit bitfield.
const FILTER_INSTALLED: u64 = 1 << 2;
/// `PkExitEnum::Success`.
const PACKAGEKIT_EXIT_SUCCESS: u32 = 1;

/// Maximum records reported for one scan. The host also enforces its own limit.
pub(super) const MAX_RECORDS: usize = 4_096;
/// Maximum desktop entries inspected for one scan.
const MAX_DESKTOP_FILES: usize = 4_096;
/// Maximum files per PackageKit installed-file search.
const DESKTOP_SEARCH_BATCH: usize = 128;
/// Maximum D-Bus transaction messages consumed for one search.
const MAX_PACKAGEKIT_MESSAGES: usize = 65_536;
/// Per-file pacman ownership query budget.
const PACMAN_QUERY_TIMEOUT: Duration = Duration::from_secs(5);
/// Per-batch PackageKit transaction budget.
const PACKAGEKIT_QUERY_TIMEOUT: Duration = Duration::from_secs(20);

/// A typed reason why a package scan produced no result.
///
/// Every variant is a failure; an empty successful scan is `Ok(Vec::new())` instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ScanFailure {
    DesktopMetadata,
    PacmanUnavailable,
    PacmanFailed,
    PacmanTimeout,
    PacmanMalformed,
    PackageKitUnavailable,
    PackageKitFailed,
    PackageKitTimeout,
}

impl ScanFailure {
    /// Stable machine-readable code for the plugin log and the host failure path.
    pub(super) fn code(self) -> &'static str {
        match self {
            Self::DesktopMetadata => "desktop_metadata_failed",
            Self::PacmanUnavailable => "pacman_unavailable",
            Self::PacmanFailed => "pacman_query_failed",
            Self::PacmanTimeout => "pacman_query_timeout",
            Self::PacmanMalformed => "pacman_invalid_output",
            Self::PackageKitUnavailable => "packagekit_unavailable",
            Self::PackageKitFailed => "packagekit_query_failed",
            Self::PackageKitTimeout => "packagekit_query_timeout",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PackageBackend {
    Pacman,
    PackageKit,
}

impl PackageBackend {
    /// Reads the distribution identity from an `/etc/os-release` body.
    pub(super) fn for_os_release(os_release: &str) -> Self {
        let mut id = "";
        let mut id_like = "";
        for line in os_release.lines() {
            if let Some((key, value)) = line.split_once('=') {
                match key {
                    "ID" => id = value.trim_matches('"').trim_matches('\''),
                    "ID_LIKE" => id_like = value.trim_matches('"').trim_matches('\''),
                    _ => {}
                }
            }
        }
        if id.eq_ignore_ascii_case("arch")
            || id_like
                .split_ascii_whitespace()
                .any(|family| family.eq_ignore_ascii_case("arch"))
        {
            Self::Pacman
        } else {
            Self::PackageKit
        }
    }
}

/// One candidate desktop entry: a visible Application in the Game category whose program exists.
#[derive(Clone, Debug)]
pub(super) struct DesktopEntry {
    pub(super) path: PathBuf,
    pub(super) name: Option<String>,
    pub(super) exec: String,
    pub(super) binary: Option<PathBuf>,
}

/// A provider-local package record. The package name is the stable identity.
#[derive(Clone, Debug)]
pub(super) struct PackageCandidate {
    pub(super) id: String,
    pub(super) name: Option<String>,
    pub(super) install_dir: String,
    pub(super) fields: BTreeMap<String, String>,
}

/// Scans the live system with the backend selected from `/etc/os-release`.
pub(super) async fn discover() -> Result<Vec<PackageCandidate>, ScanFailure> {
    let (os_release, entries) = tokio::task::spawn_blocking(read_system_inputs)
        .await
        .map_err(|_| ScanFailure::DesktopMetadata)?;
    match PackageBackend::for_os_release(&os_release) {
        PackageBackend::Pacman => scan_pacman(&SystemPacman, &entries, PACMAN_QUERY_TIMEOUT).await,
        PackageBackend::PackageKit => {
            let client = SystemPackageKit::connect().await?;
            scan_packagekit(&client, &entries, PACKAGEKIT_QUERY_TIMEOUT).await
        }
    }
}

/// Reads `/etc/os-release` and the bounded desktop-entry set on a blocking worker.
fn read_system_inputs() -> (String, Vec<DesktopEntry>) {
    let os_release = fs::read_to_string(OS_RELEASE).unwrap_or_default();
    (os_release, read_game_desktop_entries())
}

/// Queries pacman ownership for every candidate entry.
pub(super) async fn scan_pacman(
    runner: &dyn PacmanRunner,
    entries: &[DesktopEntry],
    timeout: Duration,
) -> Result<Vec<PackageCandidate>, ScanFailure> {
    let mut records = BTreeMap::<String, PackageCandidate>::new();
    for entry in entries {
        let owner = tokio::time::timeout(timeout, runner.owner(&entry.path))
            .await
            .map_err(|_| ScanFailure::PacmanTimeout)??;
        let Some(package_id) = owner else { continue };
        let Some(candidate) = candidate_from_desktop(&package_id, entry, "pacman") else {
            continue;
        };
        records.entry(candidate.id.clone()).or_insert(candidate);
        if records.len() >= MAX_RECORDS {
            break;
        }
    }
    Ok(records.into_values().collect())
}

/// One `pacman -Qqo` ownership query.
#[async_trait]
pub(super) trait PacmanRunner: Send + Sync {
    /// Returns the owning package, `None` when no installed package owns the file, or a typed
    /// failure when pacman is missing or the query failed.
    async fn owner(&self, path: &Path) -> Result<Option<String>, ScanFailure>;
}

pub(super) struct SystemPacman;

#[async_trait]
impl PacmanRunner for SystemPacman {
    async fn owner(&self, path: &Path) -> Result<Option<String>, ScanFailure> {
        // Fixed executable, argument-based arguments and a fixed locale: no shell is involved and
        // the desktop file path is never interpreted as syntax.
        let output = tokio::process::Command::new(PACMAN)
            .arg("-Qqo")
            .arg("--")
            .arg(path)
            .env("LC_ALL", PACMAN_LOCALE)
            .env("LANG", PACMAN_LOCALE)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|_| ScanFailure::PacmanUnavailable)?;
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return if error.contains("No package owns") {
                Ok(None)
            } else {
                Err(ScanFailure::PacmanFailed)
            };
        }
        parse_package_owner(&output.stdout)
    }
}

/// Parses a single package name from `pacman -Qqo` output.
pub(super) fn parse_package_owner(output: &[u8]) -> Result<Option<String>, ScanFailure> {
    let text = String::from_utf8(output.to_vec()).map_err(|_| ScanFailure::PacmanMalformed)?;
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let Some(package_id) = lines.next() else {
        return Ok(None);
    };
    if lines.next().is_some() {
        return Ok(None);
    }
    let package_id = package_id.trim();
    if package_id.len() > 256 || package_id.chars().any(char::is_control) {
        return Ok(None);
    }
    Ok(Some(package_id.to_owned()))
}

/// Queries PackageKit installed-file ownership for every candidate entry.
pub(super) async fn scan_packagekit(
    query: &dyn InstalledFileQuery,
    entries: &[DesktopEntry],
    timeout: Duration,
) -> Result<Vec<PackageCandidate>, ScanFailure> {
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let desktop_paths = entries
        .iter()
        .map(|entry| entry.path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let rows = search_installed_files(query, &desktop_paths, timeout).await?;
    Ok(records_from_rows(entries, rows, "packagekit"))
}

/// One PackageKit installed-file search.
#[async_trait]
pub(super) trait InstalledFileQuery: Send + Sync {
    async fn search_installed_files(
        &self,
        filter: u64,
        files: Vec<String>,
    ) -> Result<Vec<PackageRow>, ScanFailure>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PackageRow {
    pub(super) package_id: String,
    pub(super) summary: String,
}

/// Runs bounded installed-filter searches in batches, refusing more than [`MAX_RECORDS`] rows.
pub(super) async fn search_installed_files(
    query: &dyn InstalledFileQuery,
    desktop_paths: &[String],
    timeout: Duration,
) -> Result<Vec<PackageRow>, ScanFailure> {
    let mut rows = Vec::new();
    for batch in desktop_paths.chunks(DESKTOP_SEARCH_BATCH) {
        let found = tokio::time::timeout(
            timeout,
            query.search_installed_files(FILTER_INSTALLED, batch.to_vec()),
        )
        .await
        .map_err(|_| ScanFailure::PackageKitTimeout)??;
        rows.extend(found);
        if rows.len() >= MAX_RECORDS {
            rows.truncate(MAX_RECORDS);
            break;
        }
    }
    Ok(rows)
}

pub(super) struct SystemPackageKit {
    connection: Connection,
    timeout: Duration,
}

impl SystemPackageKit {
    pub(super) async fn connect() -> Result<Self, ScanFailure> {
        let connection = Connection::system()
            .await
            .map_err(|_| ScanFailure::PackageKitUnavailable)?;
        Ok(Self {
            connection,
            timeout: PACKAGEKIT_QUERY_TIMEOUT,
        })
    }

    async fn create_transaction(&self) -> Result<OwnedObjectPath, ScanFailure> {
        let root = Proxy::new(
            &self.connection,
            PACKAGEKIT_SERVICE,
            PACKAGEKIT_PATH,
            PACKAGEKIT_INTERFACE,
        )
        .await
        .map_err(|_| ScanFailure::PackageKitFailed)?;
        root.call("CreateTransaction", &())
            .await
            .map_err(|_| ScanFailure::PackageKitFailed)
    }

    async fn transaction_call(
        &self,
        transaction: &OwnedObjectPath,
        method: &'static str,
    ) -> Result<(), ScanFailure> {
        let proxy = Proxy::new(
            &self.connection,
            PACKAGEKIT_SERVICE,
            transaction.clone(),
            TRANSACTION_INTERFACE,
        )
        .await
        .map_err(|_| ScanFailure::PackageKitFailed)?;
        proxy
            .call::<_, _, ()>(method, &())
            .await
            .map_err(|_| ScanFailure::PackageKitFailed)
    }

    async fn search(
        &self,
        transaction: &OwnedObjectPath,
        filter: u64,
        files: Vec<String>,
    ) -> Result<Vec<PackageRow>, ScanFailure> {
        let proxy = Proxy::new(
            &self.connection,
            PACKAGEKIT_SERVICE,
            transaction.clone(),
            TRANSACTION_INTERFACE,
        )
        .await
        .map_err(|_| ScanFailure::PackageKitFailed)?;
        let mut packages = proxy
            .receive_signal("Package")
            .await
            .map_err(|_| ScanFailure::PackageKitFailed)?;
        let mut finished = proxy
            .receive_signal("Finished")
            .await
            .map_err(|_| ScanFailure::PackageKitFailed)?;
        proxy
            .call_method("SearchFiles", &(filter, files))
            .await
            .map_err(|_| ScanFailure::PackageKitFailed)?;
        let mut rows = Vec::new();
        let mut messages = 0usize;
        loop {
            // A signal flood is a broken or hostile backend, not a truncated success.
            if messages >= MAX_PACKAGEKIT_MESSAGES {
                return Err(ScanFailure::PackageKitFailed);
            }
            messages += 1;
            // Stop collecting at the record bound; the transaction is dropped with the proxy.
            if rows.len() >= MAX_RECORDS {
                break;
            }
            tokio::select! {
                biased;
                message = packages.next() => {
                    // Only a `Finished` signal completes a PackageKit transaction.
                    let Some(message) = message else {
                        return Err(ScanFailure::PackageKitFailed);
                    };
                    if let Ok((_, package_id, summary)) =
                        message.body().deserialize::<(u32, String, String)>()
                        && package_id.contains(';')
                    {
                        rows.push(PackageRow { package_id, summary });
                    }
                }
                message = finished.next() => {
                    let Some(message) = message else {
                        return Err(ScanFailure::PackageKitFailed);
                    };
                    let (exit, _runtime) = message
                        .body()
                        .deserialize::<(u32, u32)>()
                        .map_err(|_| ScanFailure::PackageKitFailed)?;
                    if exit != PACKAGEKIT_EXIT_SUCCESS {
                        return Err(ScanFailure::PackageKitFailed);
                    }
                    break;
                }
            }
        }
        Ok(rows)
    }
}

#[async_trait]
impl InstalledFileQuery for SystemPackageKit {
    async fn search_installed_files(
        &self,
        filter: u64,
        files: Vec<String>,
    ) -> Result<Vec<PackageRow>, ScanFailure> {
        let transaction = self.create_transaction().await?;
        match tokio::time::timeout(self.timeout, self.search(&transaction, filter, files)).await {
            Ok(result) => result,
            Err(_) => {
                // The transaction is still open in PackageKit; cancel it on a best-effort basis
                // so a stalled backend does not leave an idle transaction behind.
                let _ = self.transaction_call(&transaction, "Cancel").await;
                Err(ScanFailure::PackageKitTimeout)
            }
        }
    }
}

/// Maps PackageKit rows to provider records, keeping the first record per package name.
fn records_from_rows(
    entries: &[DesktopEntry],
    rows: Vec<PackageRow>,
    package_manager: &str,
) -> Vec<PackageCandidate> {
    let mut records = BTreeMap::<String, PackageCandidate>::new();
    for row in rows {
        let Some(package_name) = normalized_package_name(&row.package_id) else {
            continue;
        };
        for entry in entries
            .iter()
            .filter(|entry| package_id_has_desktop(package_name, &entry.path, &entry.exec))
        {
            let Some(binary) = entry.binary.as_ref() else {
                continue;
            };
            records
                .entry(package_name.to_owned())
                .or_insert_with(|| PackageCandidate {
                    id: package_name.to_owned(),
                    name: entry
                        .name
                        .clone()
                        .or_else(|| (!row.summary.is_empty()).then(|| row.summary.clone())),
                    install_dir: parent_dir(binary),
                    fields: BTreeMap::from([
                        ("package_id".to_owned(), row.package_id.clone()),
                        ("package_manager".to_owned(), package_manager.to_owned()),
                        (
                            "desktop_file".to_owned(),
                            entry.path.to_string_lossy().into_owned(),
                        ),
                        (
                            "executable".to_owned(),
                            binary.to_string_lossy().into_owned(),
                        ),
                    ]),
                });
        }
        if records.len() >= MAX_RECORDS {
            break;
        }
    }
    records.into_values().collect()
}

/// Normalizes a PackageKit package id such as `supertux;0.6.3-4;x86_64;installed`.
pub(super) fn normalized_package_name(package_id: &str) -> Option<&str> {
    let name = package_id.split(';').next()?.trim();
    (!name.is_empty()
        && name.len() <= 256
        && !name.chars().any(char::is_control)
        && !name.contains('/'))
    .then_some(name)
}

/// Builds a record from a pacman owner and its desktop entry without executing the entry.
pub(super) fn candidate_from_desktop(
    package_id: &str,
    entry: &DesktopEntry,
    package_manager: &str,
) -> Option<PackageCandidate> {
    let binary = entry.binary.as_ref()?;
    Some(PackageCandidate {
        id: package_id.to_owned(),
        name: entry.name.clone(),
        install_dir: parent_dir(binary),
        fields: BTreeMap::from([
            ("package_id".to_owned(), package_id.to_owned()),
            (
                "desktop_file".to_owned(),
                entry.path.to_string_lossy().into_owned(),
            ),
            (
                "executable".to_owned(),
                binary.to_string_lossy().into_owned(),
            ),
            ("package_manager".to_owned(), package_manager.to_owned()),
        ]),
    })
}

fn parent_dir(binary: &Path) -> String {
    binary
        .parent()
        .unwrap_or(binary)
        .to_string_lossy()
        .into_owned()
}

/// Reads bounded visible Game desktop entries from the system application directories.
fn read_game_desktop_entries() -> Vec<DesktopEntry> {
    let mut entries = Vec::new();
    for directory in DESKTOP_DIRECTORIES {
        let Ok(files) = fs::read_dir(directory) else {
            continue;
        };
        for file in files
            .flatten()
            .take(MAX_DESKTOP_FILES.saturating_sub(entries.len()))
        {
            if !file
                .path()
                .extension()
                .is_some_and(|extension| extension == "desktop")
            {
                continue;
            }
            let Ok(text) = fs::read_to_string(file.path()) else {
                continue;
            };
            let Some(entry) = parse_game_desktop_entry(file.path(), &text) else {
                continue;
            };
            // An entry whose program does not exist is never a game candidate.
            if entry.binary.is_none() {
                continue;
            }
            entries.push(entry);
            if entries.len() >= MAX_DESKTOP_FILES {
                break;
            }
        }
    }
    entries
}

/// Parses one desktop entry, keeping only visible Game applications with a resolvable program.
pub(super) fn parse_game_desktop_entry(path: PathBuf, text: &str) -> Option<DesktopEntry> {
    let mut in_entry = false;
    let mut entry_type = None;
    let mut name = None;
    let mut exec = None;
    let mut categories = String::new();
    let mut no_display = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            match key {
                "Type" => entry_type = Some(value),
                "Name" => name = Some(value.to_owned()),
                "Exec" => exec = Some(value.to_owned()),
                "Categories" => categories = value.to_owned(),
                "NoDisplay" => no_display = value.eq_ignore_ascii_case("true"),
                _ => {}
            }
        }
    }
    let is_game = categories
        .split(';')
        .any(|category| category.eq_ignore_ascii_case("game"));
    if entry_type != Some("Application") || !is_game || no_display {
        return None;
    }
    let exec = exec?;
    let binary = resolve_exec(&exec);
    Some(DesktopEntry {
        path,
        name,
        exec,
        binary,
    })
}

/// Associates a normalized package name with a desktop entry by file stem or program name.
fn package_id_has_desktop(package_name: &str, desktop_path: &Path, exec: &str) -> bool {
    let desktop_stem = desktop_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let executable = exec_program(exec).unwrap_or_default();
    let executable = Path::new(&executable)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    desktop_stem == package_name
        || desktop_stem
            .strip_prefix(package_name)
            .is_some_and(|suffix| {
                suffix.starts_with('-') || suffix.chars().all(|ch| ch.is_ascii_digit())
            })
        || executable == package_name
        || executable
            .strip_prefix(package_name)
            .is_some_and(|suffix| suffix.chars().all(|ch| ch.is_ascii_digit()))
}

/// Tokenizes only the program of a desktop `Exec` value.
///
/// Shell syntax is never interpreted and field codes are rejected, so a desktop file cannot
/// influence a command line or an executable action.
pub(super) fn exec_program(exec: &str) -> Option<String> {
    if exec.len() > 8_192 {
        return None;
    }
    let mut chars = exec.trim_start().chars();
    let mut program = String::new();
    let mut quoted = false;
    while let Some(ch) = chars.next() {
        match ch {
            '"' => quoted = !quoted,
            '\\' => {
                let escaped = chars.next()?;
                if quoted && !matches!(escaped, '"' | '`' | '$' | '\\') {
                    return None;
                }
                program.push(escaped);
            }
            ch if ch.is_whitespace() && !quoted => break,
            '%' => return None,
            ch => program.push(ch),
        }
    }
    (!quoted && !program.is_empty()).then_some(program)
}

/// Resolves the program to an existing file without ever running it.
pub(super) fn resolve_exec(exec: &str) -> Option<PathBuf> {
    let executable = exec_program(exec)?;
    let candidate = PathBuf::from(&executable);
    if candidate.is_absolute() {
        return candidate.is_file().then_some(candidate);
    }
    env::var_os("PATH")?
        .to_string_lossy()
        .split(':')
        .map(|directory| PathBuf::from(directory).join(&executable))
        .find(|path| path.is_file())
}
