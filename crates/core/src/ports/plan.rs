//! Pure install planning for PC ports (design D5).
//!
//! Planning answers every question that can be answered before work starts: which release and asset
//! are installed, where the port goes, which supplied files fill which slot, the arguments the
//! library entry launches with and how much space the target volume needs. Nothing here performs
//! I/O: the host supplies the answers it alone can give (`target_empty`, `free_space`,
//! `library_roots`) and executes the result.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::drm_free::{Registration, resolve_install_path};
use crate::ports::catalog::{
    AcceptedHash, CatalogEntry, CopyRoot, Extractor, FromPackage, Placement, PortAsset,
    PortRelease, SetupKind, WithdrawnRelease, is_safe_relative_path,
};

/// Upstream publishes no extracted size. A game-data archive usually holds at least half its
/// unpacked size, so estimating twice the download keeps the free-space check on the safe side.
pub const EXTRACTED_SIZE_ESTIMATE: u64 = 2;

/// The asset formats the installer handles (design D6): archives it extracts and a bare
/// executable it copies into the folder.
pub const SUPPORTED_FORMATS: [&str; 3] = ["zip", "7z", "exe"];

/// The bytes an install needs before it starts: the download, its extraction and the copies of
/// supplied files.
///
/// The download is cached on the data volume, but the specification counts it against the target
/// volume as well, which is the conservative choice when both live on the same disk.
pub fn space_estimate(asset_bytes: u64, supplied_bytes: u64) -> u64 {
    asset_bytes
        .saturating_mul(1 + EXTRACTED_SIZE_ESTIMATE)
        .saturating_add(supplied_bytes)
}

/// The subfolder name the port is installed into: the recipe's folder name when it has a usable
/// one, else the entry name, else the slug.
pub fn folder_name(entry: &CatalogEntry) -> String {
    [
        entry.launcher.folder_name.as_deref(),
        Some(&entry.name),
        Some(&entry.slug),
    ]
    .into_iter()
    .flatten()
    .map(sanitize_folder_name)
    .find(|name| !name.is_empty())
    .unwrap_or_default()
}

/// Makes a folder name valid on Windows: path separators and reserved characters become `-`,
/// trailing dots and spaces are removed, and reserved device names gain a suffix.
pub fn sanitize_folder_name(name: &str) -> String {
    let mut cleaned: String = name
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                '-'
            } else {
                character
            }
        })
        .collect();
    while cleaned.ends_with('.') || cleaned.ends_with(' ') {
        cleaned.pop();
    }
    let cleaned = cleaned.trim_start().to_string();
    let stem = cleaned.split('.').next().unwrap_or_default().trim();
    let reserved = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if reserved.iter().any(|name| stem.eq_ignore_ascii_case(name)) {
        return format!("{cleaned}-port");
    }
    cleaned
}

/// Joins a folder and one name in the folder's own separator style.
pub fn join_folder(parent: &str, folder: &str) -> String {
    join_relative(parent, folder)
}

/// Joins a relative path to a base folder, converting the relative path's separators to the base's
/// style. Relative paths are validated before they reach this function.
pub fn join_relative(base: &str, relative: &str) -> String {
    let separator = if base.contains('\\') { '\\' } else { '/' };
    let base = base.trim_end_matches(['/', '\\']);
    let parts: Vec<&str> = relative
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() {
        return base.to_string();
    }
    let tail = parts.join(&separator.to_string());
    if base.is_empty() {
        return tail;
    }
    format!("{base}{separator}{tail}")
}

/// Whether `path` is `root` or lies inside it, comparing the way Windows does.
fn is_inside_or_equal(path: &str, root: &str) -> bool {
    fn normalize(value: &str) -> String {
        value
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_ascii_lowercase()
    }
    let path = normalize(path);
    let root = normalize(root);
    if path.is_empty() || root.is_empty() {
        return false;
    }
    path == root
        || (path.len() > root.len()
            && path.starts_with(&root)
            && path.as_bytes()[root.len()] == b'\\')
}

/// Chooses the release an install uses: the overlay's pinned version when it names one, else the
/// upstream's verified version. A withdrawn release is never installable.
pub fn select_release(
    entry: &CatalogEntry,
    releases: &[PortRelease],
    withdrawn: &[WithdrawnRelease],
) -> Result<PortRelease, PlanError> {
    let version = match &entry.pinned_release {
        Some(version) => version.clone(),
        None => entry
            .verified
            .as_ref()
            .map(|verified| verified.version.clone())
            .ok_or(PlanError::NoVerifiedRelease)?,
    };
    if let Some(pulled) = withdrawn.iter().find(|pulled| pulled.version == version) {
        return Err(PlanError::Withdrawn {
            version,
            reason: pulled.reason.clone(),
        });
    }
    releases
        .iter()
        .find(|release| release.version == version)
        .cloned()
        .ok_or(PlanError::ReleaseUnavailable { version })
}

/// Chooses the Windows asset of a release: the entry's asset filter, then the operating system,
/// then the architecture (64-bit first, then unknown, then anything else).
pub fn select_asset(entry: &CatalogEntry, release: &PortRelease) -> Result<PortAsset, PlanError> {
    let windows: Vec<&PortAsset> = release
        .assets
        .iter()
        .filter(|asset| asset.is_windows())
        .collect();
    if windows.is_empty() {
        return Err(PlanError::NoWindowsAsset);
    }
    let filter = entry
        .launcher
        .release_asset_filter
        .as_deref()
        .map(str::trim)
        .filter(|filter| !filter.is_empty());
    let candidates: Vec<&PortAsset> = match filter {
        Some(filter) => {
            let needle = filter.to_ascii_lowercase();
            let matched: Vec<&PortAsset> = windows
                .into_iter()
                .filter(|asset| asset.filename.to_ascii_lowercase().contains(&needle))
                .collect();
            if matched.is_empty() {
                return Err(PlanError::NoMatchingAsset {
                    filter: filter.to_string(),
                });
            }
            matched
        }
        None => windows,
    };
    let mut best: Option<&PortAsset> = None;
    let mut best_rank = u8::MAX;
    for asset in candidates {
        let rank = asset.architecture_rank();
        if rank < best_rank {
            best_rank = rank;
            best = Some(asset);
        }
    }
    let asset = best.cloned().ok_or(PlanError::NoWindowsAsset)?;
    let format = asset_format(&asset).ok_or_else(|| PlanError::UnknownAssetFormat {
        filename: asset.filename.clone(),
    })?;
    if !SUPPORTED_FORMATS.contains(&format.as_str()) {
        return Err(PlanError::UnsupportedFormat { format });
    }
    Ok(asset)
}

/// The asset's file format in lowercase, from its declared format or its file extension.
fn asset_format(asset: &PortAsset) -> Option<String> {
    asset.normalized_format().or_else(|| {
        asset
            .filename
            .rsplit_once('.')
            .map(|(_, extension)| extension.trim().to_ascii_lowercase())
            .filter(|extension| !extension.is_empty())
    })
}

/// Everything the host knows before an install can be planned.
#[derive(Clone, Debug)]
pub struct InstallPlanRequest {
    /// The merged catalog entry, with the overlay already applied.
    pub entry: CatalogEntry,
    /// The entry's releases, as fetched from the upstream.
    pub releases: Vec<PortRelease>,
    /// Releases the upstream withdrew.
    pub withdrawn: Vec<WithdrawnRelease>,
    /// The parent folder the user chose.
    pub parent: String,
    /// The job id; it names the staging folder.
    pub job_id: String,
    /// Whether the resolved install folder is absent or empty.
    pub target_empty: bool,
    /// Free bytes on the target volume.
    pub free_space: u64,
    /// Game folders of existing library entries. The target may not be one of them or lie inside.
    pub library_roots: Vec<String>,
    /// The choices the user made, one per setup slot.
    pub supplied: Vec<SuppliedChoice>,
    /// Bytes the supplied choices occupy in the install folder.
    pub supplied_bytes: u64,
    /// The user confirmed the risks a previous [`PlanError::ConfirmationRequired`] listed.
    pub confirmed_risk: bool,
}

/// One supplied slot the user filled in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuppliedChoice {
    pub slot: String,
    pub source: SuppliedSource,
}

/// Where a supplied slot comes from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum SuppliedSource {
    /// A file or folder the user picked.
    Path { path: String },
    /// Console packages the pinned extractor turns into the folder the slot needs.
    Packages {
        base: String,
        update: Option<String>,
    },
}

/// A setup slot as the install will satisfy it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedSlot {
    pub id: String,
    pub label: String,
    pub required: bool,
    pub kind: SetupKind,
    pub accepted: Vec<AcceptedHash>,
    pub placement: Placement,
    pub from_package: Option<FromPackage>,
    /// `None` for a skipped optional slot.
    pub source: Option<SuppliedSource>,
    /// The slot's final absolute path under the install folder, when the copy root is the port's
    /// folder. While the job runs, the same relative path lives under the staging folder. `None` for
    /// a user-data root (only the host can resolve it), for a path passed as an argument, for a slot
    /// the port asks for itself, and for a skipped optional slot.
    pub target: Option<String>,
}

/// The release an install will download.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedRelease {
    pub release_id: String,
    pub version: String,
    /// Unix milliseconds.
    pub released_at: i64,
    pub prerelease: bool,
}

/// The file an install will download and verify.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedAsset {
    pub id: String,
    pub filename: String,
    pub url: String,
    /// Lowercase hex SHA-256 the download is checked against.
    pub sha256: String,
    pub size: u64,
    pub format: String,
    /// True when the checksum is the upstream's unverified one, which needed a confirmation.
    pub unverified: bool,
}

/// The library entry an install will register (design D4, D5).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationTemplate {
    pub title: String,
    /// Relative to the install folder. `None` when no preferred executable exists and the user has
    /// to pick one after extraction. The installer turns it into the absolute
    /// `Registration.executable` once it confirms the file exists.
    pub executable: Option<String>,
    pub working_directory: String,
    /// Recipe arguments, positional paths first, then flags. The user's own arguments follow them.
    pub arguments: Vec<String>,
    pub gameplay: Vec<String>,
    pub launchers: Vec<String>,
}

impl RegistrationTemplate {
    /// Builds the DRM-free registration at the job's commit point. `executable` must be the absolute
    /// path the installer confirmed, and `file_identity` its identity.
    ///
    /// `gameplay` and `launchers` are recorded relative to the install folder by recipes and the
    /// catalog; they are made absolute against `working_directory` (the install folder) so the
    /// tracker can compare them with full process paths. Unsafe relative entries are dropped.
    pub fn to_registration(
        &self,
        id: String,
        executable: String,
        file_identity: String,
        available: bool,
    ) -> Registration {
        Registration {
            id,
            title: self.title.clone(),
            executable,
            file_identity,
            root: self.working_directory.clone(),
            working_directory: self.working_directory.clone(),
            arguments: self.arguments.clone(),
            gameplay: self
                .gameplay
                .iter()
                .filter_map(|path| resolve_install_path(&self.working_directory, path))
                .collect(),
            launchers: self
                .launchers
                .iter()
                .filter_map(|path| resolve_install_path(&self.working_directory, path))
                .collect(),
            revision: 1,
            removed: false,
            rediscovery_allowed: false,
            transferred_from: None,
            available,
        }
    }
}

/// A complete, executable install plan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortInstallPlan {
    pub catalog_id: String,
    pub slug: String,
    pub title: String,
    pub release: PlannedRelease,
    pub asset: PlannedAsset,
    pub folder_name: String,
    pub parent: String,
    /// The final install folder.
    pub target: String,
    /// `<parent>\.gamenode-staging-<job id>`, on the target volume.
    pub staging: String,
    pub slots: Vec<PlannedSlot>,
    /// Files the recipe adds after extraction, relative to the install folder.
    pub files_to_add: Vec<String>,
    pub registration: RegistrationTemplate,
    /// Environment variables the port is launched with, from `env` slots.
    pub environment: BTreeMap<String, String>,
    /// Extraction tools the package slots use.
    pub extractors: Vec<Extractor>,
    /// Bytes the target volume must have free before the job starts.
    pub space_required: u64,
    /// Risks the user confirmed; empty for a clean install.
    pub risks: Vec<InstallRisk>,
}

/// A risk an install would expose the user to. The UI names it before the install starts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum InstallRisk {
    /// The verified file changed upstream since. The download is checked against `sha256`.
    UnverifiedChecksum { sha256: String },
    /// A malware scan flagged the file.
    ScanFlagged {
        engines: Option<String>,
        url: Option<String>,
    },
}

/// Why an install cannot be planned.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    #[error("this entry does not support Windows")]
    NotWindows,
    #[error("the entry has no verified release")]
    NoVerifiedRelease,
    #[error("release {version} was withdrawn: {reason}")]
    Withdrawn { version: String, reason: String },
    #[error("release {version} is no longer available upstream")]
    ReleaseUnavailable { version: String },
    #[error("the release has no Windows asset")]
    NoWindowsAsset,
    #[error("no asset file name matches the entry's asset filter {filter:?}")]
    NoMatchingAsset { filter: String },
    #[error("the asset {filename:?} has no file format")]
    UnknownAssetFormat { filename: String },
    #[error("the asset format {format:?} cannot be installed")]
    UnsupportedFormat { format: String },
    #[error("the asset has no checksum to verify the download against")]
    NoChecksum,
    #[error("the install needs an explicit confirmation")]
    ConfirmationRequired { risks: Vec<InstallRisk> },
    #[error("choose an install folder")]
    MissingParent,
    #[error("the recipe has no usable folder name")]
    InvalidFolderName,
    #[error("the job id {job_id:?} cannot be used in a folder name")]
    InvalidJobId { job_id: String },
    #[error("the install folder {target} already contains files")]
    TargetNotEmpty { target: String },
    #[error("the install folder {target} is inside the library entry {root}")]
    TargetInsideLibrary { target: String, root: String },
    #[error("the install needs {required} bytes, but only {available} are free")]
    NotEnoughSpace { required: u64, available: u64 },
    #[error("the recipe adds a file outside the install folder: {path:?}")]
    UnsafeRecipePath { path: String },
    #[error("setup file {slot:?} ({label}) has no choice")]
    MissingSlot { slot: String, label: String },
    #[error("no setup file is named {slot:?}")]
    UnknownSlot { slot: String },
    #[error("setup file {slot:?} cannot be used: {reason}")]
    InvalidSlot { slot: String, reason: String },
}

/// Plans an install from what the host already knows. Every check that can fail before the job
/// starts is here; the job itself only executes and journals (design D5).
pub fn plan_install(request: &InstallPlanRequest) -> Result<PortInstallPlan, PlanError> {
    let entry = &request.entry;
    if !entry.supports_windows() {
        return Err(PlanError::NotWindows);
    }

    let release = select_release(entry, &request.releases, &request.withdrawn)?;
    let asset = select_asset(entry, &release)?;

    let mut risks = Vec::new();
    let (sha256, unverified) = match asset.verified_sha256() {
        Some(sha256) => (sha256, false),
        None => match asset.unverified_sha256() {
            Some(sha256) => {
                risks.push(InstallRisk::UnverifiedChecksum {
                    sha256: sha256.clone(),
                });
                (sha256, true)
            }
            None => return Err(PlanError::NoChecksum),
        },
    };
    if asset.is_scan_flagged() {
        let scan = asset.scan.clone().unwrap_or_default();
        risks.push(InstallRisk::ScanFlagged {
            engines: scan.engines,
            url: scan.url,
        });
    }
    if !risks.is_empty() && !request.confirmed_risk {
        return Err(PlanError::ConfirmationRequired { risks });
    }

    if request.parent.trim().is_empty() {
        return Err(PlanError::MissingParent);
    }
    if request.job_id.is_empty()
        || !request
            .job_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(PlanError::InvalidJobId {
            job_id: request.job_id.clone(),
        });
    }
    let folder = folder_name(entry);
    if folder.is_empty() {
        return Err(PlanError::InvalidFolderName);
    }
    let parent = request.parent.trim_end_matches(['/', '\\']).to_string();
    let target = join_folder(&parent, &folder);
    let staging = join_folder(&parent, &format!(".gamenode-staging-{}", request.job_id));
    if !request.target_empty {
        return Err(PlanError::TargetNotEmpty { target });
    }
    if let Some(root) = request
        .library_roots
        .iter()
        .find(|root| is_inside_or_equal(&target, root))
    {
        return Err(PlanError::TargetInsideLibrary {
            target,
            root: root.clone(),
        });
    }
    let space_required = space_estimate(asset.size, request.supplied_bytes);
    if request.free_space < space_required {
        return Err(PlanError::NotEnoughSpace {
            required: space_required,
            available: request.free_space,
        });
    }
    for path in &entry.launcher.files_to_add {
        if !is_safe_relative_path(path) {
            return Err(PlanError::UnsafeRecipePath { path: path.clone() });
        }
    }

    let mut choices: BTreeMap<&str, &SuppliedChoice> = BTreeMap::new();
    for choice in &request.supplied {
        if !entry
            .launcher
            .setup_files
            .iter()
            .any(|slot| slot.id == choice.slot)
        {
            return Err(PlanError::UnknownSlot {
                slot: choice.slot.clone(),
            });
        }
        choices.insert(choice.slot.as_str(), choice);
    }

    let mut slots = Vec::new();
    let mut positional = Vec::new();
    let mut flags = Vec::new();
    let mut environment = BTreeMap::new();
    let mut extractors: Vec<Extractor> = Vec::new();
    for slot in &entry.launcher.setup_files {
        slot.validate().map_err(|error| PlanError::InvalidSlot {
            slot: slot.id.clone(),
            reason: error.to_string(),
        })?;
        let placement = slot.placement().map_err(|error| PlanError::InvalidSlot {
            slot: slot.id.clone(),
            reason: error.to_string(),
        })?;
        let choice = choices.get(slot.id.as_str()).copied();
        if choice.is_none() && slot.required {
            return Err(PlanError::MissingSlot {
                slot: slot.id.clone(),
                label: slot.label.clone(),
            });
        }
        let slot_target = match &placement {
            Placement::Copy {
                root: CopyRoot::App,
                destination,
            } => Some(join_relative(&target, destination)),
            // A user-data root is outside the install folder; only the host can resolve it.
            Placement::Copy { .. } => None,
            Placement::Flag { .. } | Placement::Positional | Placement::Env { .. } => {
                match choice {
                    Some(SuppliedChoice {
                        source: SuppliedSource::Packages { .. },
                        ..
                    }) => Some(join_folder(&target, &format!(".gamenode-slot-{}", slot.id))),
                    _ => None,
                }
            }
            Placement::Unused => None,
        };
        if let Some(choice) = choice {
            if let SuppliedSource::Packages { .. } = &choice.source {
                let package = slot
                    .from_package
                    .as_ref()
                    .ok_or_else(|| PlanError::InvalidSlot {
                        slot: slot.id.clone(),
                        reason: "the slot does not accept console packages".into(),
                    })?;
                let extractor =
                    entry
                        .extractor(&package.extractor)
                        .ok_or_else(|| PlanError::InvalidSlot {
                            slot: slot.id.clone(),
                            reason: format!("extractor {:?} is not pinned", package.extractor),
                        })?;
                if !extractors.iter().any(|known| known.id == extractor.id) {
                    extractors.push(extractor.clone());
                }
            }
            let value = match &choice.source {
                SuppliedSource::Path { path } => path.clone(),
                SuppliedSource::Packages { .. } => slot_target.clone().unwrap_or_default(),
            };
            match &placement {
                Placement::Positional => {
                    if !value.is_empty() {
                        positional.push(value);
                    }
                }
                Placement::Flag { flag, joined } => {
                    if !value.is_empty() {
                        if *joined {
                            flags.push(format!("{flag}{value}"));
                        } else {
                            flags.push(flag.clone());
                            flags.push(value);
                        }
                    }
                }
                Placement::Env { name } => {
                    if !value.is_empty() {
                        environment.insert(name.clone(), value);
                    }
                }
                Placement::Copy { .. } | Placement::Unused => {}
            }
        }
        slots.push(PlannedSlot {
            id: slot.id.clone(),
            label: slot.label.clone(),
            required: slot.required,
            kind: slot.kind,
            accepted: slot.accepted.clone(),
            placement,
            from_package: slot.from_package.clone(),
            source: choice.map(|choice| choice.source.clone()),
            target: slot_target,
        });
    }
    let mut arguments = positional;
    arguments.extend(flags);

    let roles = entry.executable_roles();
    let registration = RegistrationTemplate {
        title: entry.name.clone(),
        executable: roles.executable,
        working_directory: target.clone(),
        arguments,
        gameplay: roles.gameplay,
        launchers: roles.launchers,
    };
    let format = asset_format(&asset).unwrap_or_default();

    Ok(PortInstallPlan {
        catalog_id: entry.catalog_id.clone(),
        slug: entry.slug.clone(),
        title: entry.name.clone(),
        release: PlannedRelease {
            release_id: release.release_id,
            version: release.version,
            released_at: release.released_at,
            prerelease: release.prerelease,
        },
        asset: PlannedAsset {
            id: asset.id,
            filename: asset.filename,
            url: asset.url,
            sha256,
            size: asset.size,
            format,
            unverified,
        },
        folder_name: folder,
        parent,
        target,
        staging,
        slots,
        files_to_add: entry.launcher.files_to_add.clone(),
        registration,
        environment,
        extractors,
        space_required,
        risks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::catalog::{
        CatalogProject, HashAlgorithm, LauncherRecipe, PreferredExecutables, ReleaseScan,
        ScanVerdict, SetupFile, VerifiedRelease,
    };
    fn digest(seed: char) -> String {
        seed.to_string().repeat(64)
    }

    fn verified_release() -> PortRelease {
        PortRelease {
            release_id: "rel-verified".into(),
            version: "v0.9.38".into(),
            released_at: 1_793_220_348_000,
            assets: vec![
                PortAsset {
                    id: "linux".into(),
                    filename: "bloodborne-linux-x64.zip".into(),
                    url: "https://example.com/linux.zip".into(),
                    os: Some("linux".into()),
                    architecture: Some("x86_64".into()),
                    format: Some("zip".into()),
                    size: 100,
                    checksum: Some(format!("sha256:{}", digest('a'))),
                    ..PortAsset::default()
                },
                PortAsset {
                    id: "windows-unknown".into(),
                    filename: "bloodborne_windows_port_unknown.zip".into(),
                    url: "https://example.com/unknown.zip".into(),
                    os: Some("windows".into()),
                    architecture: Some("unknown".into()),
                    format: Some("zip".into()),
                    size: 200,
                    checksum: Some(format!("sha256:{}", digest('b'))),
                    ..PortAsset::default()
                },
                PortAsset {
                    id: "windows-x64".into(),
                    filename: "bloodborne_windows_port_x64.zip".into(),
                    url: "https://example.com/x64.zip".into(),
                    os: Some("windows".into()),
                    architecture: Some("x86_64".into()),
                    format: Some("zip".into()),
                    size: 300,
                    checksum: Some(format!("sha256:{}", digest('c'))),
                    ..PortAsset::default()
                },
            ],
            ..PortRelease::default()
        }
    }

    fn entry() -> CatalogEntry {
        CatalogEntry {
            catalog_id: "qcat_lantern".into(),
            slug: "bloodborne-bloodborne-pc-windows-port".into(),
            name: "Bloodborne".into(),
            project: Some(CatalogProject::default()),
            supported_os: vec!["windows".into()],
            games: vec![crate::ports::catalog::CatalogGame {
                title: "Bloodborne".into(),
                ..Default::default()
            }],
            verified: Some(VerifiedRelease {
                version: "v0.9.38".into(),
                released_at: 1_793_220_348_000,
                ..VerifiedRelease::default()
            }),
            launcher: LauncherRecipe {
                folder_name: Some("Bloodborne-BloodbornePC".into()),
                files_to_add: vec!["portable.txt".into()],
                release_asset_filter: None,
                preferred_executables: PreferredExecutables {
                    windows: vec!["Bloodborne.Launcher.exe".into()],
                    ..Default::default()
                },
                setup_files: Vec::new(),
            },
            ..CatalogEntry::default()
        }
    }

    fn request(entry: CatalogEntry, releases: Vec<PortRelease>) -> InstallPlanRequest {
        InstallPlanRequest {
            entry,
            releases,
            withdrawn: Vec::new(),
            parent: r"D:\Ports".into(),
            job_id: "job-1".into(),
            target_empty: true,
            free_space: 10_000,
            library_roots: Vec::new(),
            supplied: Vec::new(),
            supplied_bytes: 0,
            confirmed_risk: false,
        }
    }

    #[test]
    fn verified_release_plans_its_windows_asset() {
        let plan = plan_install(&request(entry(), vec![verified_release()])).expect("plan");
        assert_eq!(plan.release.version, "v0.9.38");
        assert_eq!(plan.asset.id, "windows-x64");
        assert_eq!(plan.asset.sha256, digest('c'));
        assert!(!plan.asset.unverified);
        assert_eq!(plan.folder_name, "Bloodborne-BloodbornePC");
        assert_eq!(plan.target, r"D:\Ports\Bloodborne-BloodbornePC");
        assert_eq!(plan.staging, r"D:\Ports\.gamenode-staging-job-1");
        assert_eq!(plan.files_to_add, vec!["portable.txt".to_string()]);
        assert_eq!(
            plan.registration.executable.as_deref(),
            Some("Bloodborne.Launcher.exe")
        );
        assert_eq!(plan.registration.title, "Bloodborne");
        assert_eq!(plan.space_required, 900);
        assert!(plan.risks.is_empty());
    }

    #[test]
    fn the_asset_filter_selects_the_named_build() {
        let mut filtered = entry();
        filtered.launcher.release_asset_filter = Some("WINDOWS_PORT_UNKNOWN".into());
        let plan = plan_install(&request(filtered, vec![verified_release()])).unwrap();
        assert_eq!(plan.asset.id, "windows-unknown");

        let mut unmatched = entry();
        unmatched.launcher.release_asset_filter = Some("linux".into());
        assert_eq!(
            plan_install(&request(unmatched, vec![verified_release()])),
            Err(PlanError::NoMatchingAsset {
                filter: "linux".into()
            })
        );
    }

    #[test]
    fn withdrawn_releases_are_never_installed() {
        let mut request = request(entry(), vec![verified_release()]);
        request.withdrawn = vec![WithdrawnRelease {
            version: "v0.9.38".into(),
            reason: "withdrawn while it was being checked".into(),
            at: 42,
        }];
        assert_eq!(
            plan_install(&request),
            Err(PlanError::Withdrawn {
                version: "v0.9.38".into(),
                reason: "withdrawn while it was being checked".into(),
            })
        );
    }

    #[test]
    fn a_pinned_release_wins_over_the_verified_one() {
        let mut entry = entry();
        entry.pinned_release = Some("v0.8.0".into());
        let older = PortRelease {
            release_id: "rel-old".into(),
            version: "v0.8.0".into(),
            assets: vec![PortAsset {
                id: "windows-old".into(),
                filename: "port-v0.8.0.zip".into(),
                url: "https://example.com/old.zip".into(),
                os: Some("windows".into()),
                architecture: Some("unknown".into()),
                format: Some("zip".into()),
                size: 50,
                checksum: Some(format!("sha256:{}", digest('d'))),
                ..PortAsset::default()
            }],
            ..PortRelease::default()
        };
        let plan = plan_install(&request(entry, vec![verified_release(), older])).unwrap();
        assert_eq!(plan.release.version, "v0.8.0");
        assert_eq!(plan.asset.id, "windows-old");

        let mut hidden = self::entry();
        hidden.pinned_release = Some("v0.7.0".into());
        assert_eq!(
            plan_install(&request(hidden, vec![verified_release()])),
            Err(PlanError::ReleaseUnavailable {
                version: "v0.7.0".into()
            })
        );
    }

    #[test]
    fn unverified_checksums_need_a_confirmation_and_are_checked_against() {
        let mut release = verified_release();
        release.assets.retain(|asset| asset.id == "windows-x64");
        release.assets[0].checksum = None;
        release.assets[0].unverified_checksum = Some(format!("sha256:{}", digest('e')));

        let mut request = request(entry(), vec![release]);
        assert_eq!(
            plan_install(&request),
            Err(PlanError::ConfirmationRequired {
                risks: vec![InstallRisk::UnverifiedChecksum {
                    sha256: digest('e')
                }]
            })
        );
        request.confirmed_risk = true;
        let plan = plan_install(&request).unwrap();
        assert!(plan.asset.unverified);
        assert_eq!(plan.asset.sha256, digest('e'));
        assert_eq!(
            plan.risks,
            vec![InstallRisk::UnverifiedChecksum {
                sha256: digest('e')
            }]
        );
    }

    #[test]
    fn scan_flagged_files_need_a_confirmation_too() {
        let mut release = verified_release();
        release.assets.retain(|asset| asset.id == "windows-x64");
        release.assets[0].scan = Some(ReleaseScan {
            verdict: ScanVerdict::Flagged,
            engines: Some("3 of 70 engines".into()),
            url: Some("https://example.com/scan".into()),
        });
        let mut request = request(entry(), vec![release]);
        assert!(matches!(
            plan_install(&request),
            Err(PlanError::ConfirmationRequired { .. })
        ));
        request.confirmed_risk = true;
        let plan = plan_install(&request).unwrap();
        assert_eq!(
            plan.risks,
            vec![InstallRisk::ScanFlagged {
                engines: Some("3 of 70 engines".into()),
                url: Some("https://example.com/scan".into())
            }]
        );
    }

    #[test]
    fn entries_without_a_verified_release_or_asset_are_refused() {
        let mut no_release = entry();
        no_release.verified = None;
        assert_eq!(
            plan_install(&request(no_release, vec![verified_release()])),
            Err(PlanError::NoVerifiedRelease)
        );

        let mut linux_only = verified_release();
        linux_only
            .assets
            .retain(|asset| asset.os.as_deref() == Some("linux"));
        assert_eq!(
            plan_install(&request(entry(), vec![linux_only])),
            Err(PlanError::NoWindowsAsset)
        );

        let mut unsupported = verified_release();
        unsupported.assets.retain(|asset| asset.id == "windows-x64");
        unsupported.assets[0].format = Some("msi".into());
        unsupported.assets[0].filename = "setup.msi".into();
        assert_eq!(
            plan_install(&request(entry(), vec![unsupported])),
            Err(PlanError::UnsupportedFormat {
                format: "msi".into()
            })
        );

        let mut no_checksum = verified_release();
        no_checksum.assets.retain(|asset| asset.id == "windows-x64");
        no_checksum.assets[0].checksum = None;
        assert_eq!(
            plan_install(&request(entry(), vec![no_checksum])),
            Err(PlanError::NoChecksum)
        );

        let mut linux_entry = entry();
        linux_entry.supported_os = vec!["linux".into()];
        assert_eq!(
            plan_install(&request(linux_entry, vec![verified_release()])),
            Err(PlanError::NotWindows)
        );
    }

    #[test]
    fn the_folder_name_rule_prefers_the_recipe_then_the_entry_name() {
        let mut entry = entry();
        entry.launcher.folder_name = None;
        assert_eq!(folder_name(&entry), "Bloodborne");

        entry.launcher.folder_name = Some("Blood/Por|t".into());
        assert_eq!(folder_name(&entry), "Blood-Por-t");

        entry.launcher.folder_name = Some("  ..  ".into());
        assert_eq!(folder_name(&entry), "Bloodborne");

        entry.launcher.folder_name = Some("CON".into());
        assert_eq!(folder_name(&entry), "CON-port");

        entry.launcher.folder_name = Some(String::new());
        entry.name = String::new();
        entry.slug = "bloodborne-port".into();
        assert_eq!(folder_name(&entry), "bloodborne-port");
    }

    #[test]
    fn the_target_folder_must_be_new_and_outside_the_library() {
        let mut request = request(entry(), vec![verified_release()]);
        request.parent = String::new();
        assert_eq!(plan_install(&request), Err(PlanError::MissingParent));
        request.parent = r"D:\Ports".into();

        request.target_empty = false;
        assert_eq!(
            plan_install(&request),
            Err(PlanError::TargetNotEmpty {
                target: r"D:\Ports\Bloodborne-BloodbornePC".into()
            })
        );
        request.target_empty = true;

        request.library_roots = vec![r"d:/ports/bloodborne-bloodbornepc".into()];
        assert_eq!(
            plan_install(&request),
            Err(PlanError::TargetInsideLibrary {
                target: r"D:\Ports\Bloodborne-BloodbornePC".into(),
                root: r"d:/ports/bloodborne-bloodbornepc".into()
            })
        );
        request.library_roots = vec![r"D:\Ports\Other".into()];
        assert!(plan_install(&request).is_ok());

        request.free_space = 100;
        assert_eq!(
            plan_install(&request),
            Err(PlanError::NotEnoughSpace {
                required: 900,
                available: 100
            })
        );
    }

    #[test]
    fn supplied_slots_fill_the_registration_arguments() {
        let mut entry = entry();
        entry.launcher.setup_files = vec![
            SetupFile {
                id: "game".into(),
                label: "Game folder".into(),
                kind: SetupKind::Folder,
                destination: Some("CUSA03173".into()),
                required: true,
                accepted: vec![AcceptedHash {
                    algo: HashAlgorithm::Sha256,
                    value: digest('f'),
                    label: Some("CUSA03173 01.09".into()),
                }],
                ..SetupFile::default()
            },
            SetupFile {
                id: "disc".into(),
                label: "Disc image".into(),
                flag: Some("--iso".into()),
                ..SetupFile::default()
            },
            SetupFile {
                id: "rom".into(),
                label: "ROM".into(),
                positional: true,
                required: false,
                ..SetupFile::default()
            },
            SetupFile {
                id: "extra".into(),
                label: "Extra data".into(),
                env: Some("PORT_DATA".into()),
                required: false,
                ..SetupFile::default()
            },
        ];
        let mut request = request(entry, vec![verified_release()]);
        request.supplied = vec![
            SuppliedChoice {
                slot: "game".into(),
                source: SuppliedSource::Path {
                    path: r"D:\Dumps\CUSA03173".into(),
                },
            },
            SuppliedChoice {
                slot: "disc".into(),
                source: SuppliedSource::Path {
                    path: r"D:\Dumps\game.iso".into(),
                },
            },
            SuppliedChoice {
                slot: "rom".into(),
                source: SuppliedSource::Path {
                    path: r"D:\Dumps\game.z64".into(),
                },
            },
            SuppliedChoice {
                slot: "extra".into(),
                source: SuppliedSource::Path {
                    path: r"D:\Dumps\extra".into(),
                },
            },
        ];
        let plan = plan_install(&request).unwrap();
        // Positional paths come before flags, and a joined flag keeps its argument attached.
        assert_eq!(
            plan.registration.arguments,
            vec![
                r"D:\Dumps\game.z64".to_string(),
                "--iso".to_string(),
                r"D:\Dumps\game.iso".to_string(),
            ]
        );
        assert_eq!(
            plan.environment,
            BTreeMap::from([("PORT_DATA".to_string(), r"D:\Dumps\extra".to_string())])
        );
        let game = plan.slots.iter().find(|slot| slot.id == "game").unwrap();
        assert_eq!(
            game.target.as_deref(),
            Some(r"D:\Ports\Bloodborne-BloodbornePC\CUSA03173")
        );
        assert_eq!(
            plan.slots
                .iter()
                .find(|slot| slot.id == "disc")
                .unwrap()
                .target,
            None
        );
    }

    #[test]
    fn required_slots_block_and_optional_slots_can_be_skipped() {
        let mut entry = entry();
        entry.launcher.setup_files = vec![
            SetupFile {
                id: "game".into(),
                label: "Game folder".into(),
                kind: SetupKind::Folder,
                destination: Some("CUSA03173".into()),
                ..SetupFile::default()
            },
            SetupFile {
                id: "extra".into(),
                label: "Optional".into(),
                required: false,
                ..SetupFile::default()
            },
        ];
        let mut request = request(entry, vec![verified_release()]);
        assert_eq!(
            plan_install(&request),
            Err(PlanError::MissingSlot {
                slot: "game".into(),
                label: "Game folder".into()
            })
        );
        request.supplied = vec![SuppliedChoice {
            slot: "game".into(),
            source: SuppliedSource::Path {
                path: r"D:\Dumps\CUSA03173".into(),
            },
        }];
        let plan = plan_install(&request).unwrap();
        assert_eq!(plan.slots.len(), 2);
        assert!(plan.slots[1].source.is_none());

        request.supplied[0].slot = "nope".into();
        assert_eq!(
            plan_install(&request),
            Err(PlanError::UnknownSlot {
                slot: "nope".into()
            })
        );
    }

    #[test]
    fn package_slots_carry_their_pinned_extractor() {
        let mut entry = entry();
        entry.extractors = vec![Extractor {
            id: "ps4-pkg".into(),
            name: "PKG extractor".into(),
            repository: "owner/repo".into(),
            tag: "v1.2.3".into(),
            asset: "tool.zip".into(),
            sha256: digest('1'),
            license: "GPL-3.0".into(),
            executable: "tool.exe".into(),
            args: vec!["{package}".into()],
        }];
        entry.launcher.setup_files = vec![SetupFile {
            id: "game".into(),
            label: "Game folder".into(),
            kind: SetupKind::Folder,
            destination: Some("CUSA03173".into()),
            from_package: Some(FromPackage {
                extractor: "ps4-pkg".into(),
                title_id: Some("CUSA03173".into()),
                needs_update: Some("01.09".into()),
            }),
            ..SetupFile::default()
        }];
        let mut request = request(entry, vec![verified_release()]);
        request.supplied = vec![SuppliedChoice {
            slot: "game".into(),
            source: SuppliedSource::Packages {
                base: r"D:\Dumps\base.pkg".into(),
                update: Some(r"D:\Dumps\update.pkg".into()),
            },
        }];
        let plan = plan_install(&request).unwrap();
        assert_eq!(plan.extractors.len(), 1);
        assert_eq!(plan.extractors[0].id, "ps4-pkg");
        let slot = &plan.slots[0];
        assert_eq!(
            slot.target.as_deref(),
            Some(r"D:\Ports\Bloodborne-BloodbornePC\CUSA03173")
        );
        assert_eq!(
            slot.source,
            Some(SuppliedSource::Packages {
                base: r"D:\Dumps\base.pkg".into(),
                update: Some(r"D:\Dumps\update.pkg".into())
            })
        );

        // A package choice for a slot without `from_package` is refused.
        let mut plain = request;
        plain.entry.launcher.setup_files[0].from_package = None;
        assert!(matches!(
            plan_install(&plain),
            Err(PlanError::InvalidSlot { .. })
        ));
    }

    #[test]
    fn space_estimates_cover_download_extraction_and_copies() {
        assert_eq!(space_estimate(0, 0), 0);
        assert_eq!(space_estimate(100, 50), 350);
        assert_eq!(space_estimate(u64::MAX, u64::MAX), u64::MAX);
    }

    #[test]
    fn joins_respect_the_parent_separator() {
        assert_eq!(join_folder(r"D:\Ports", "Game"), r"D:\Ports\Game");
        assert_eq!(join_folder("D:/Ports/", "Game"), "D:/Ports/Game");
        assert_eq!(join_folder(r"D:\", "Game"), r"D:\Game");
        assert_eq!(
            join_relative(r"D:\Ports\Game", "a/b/c.txt"),
            r"D:\Ports\Game\a\b\c.txt"
        );
        assert_eq!(join_relative("/ports", r"a\b"), "/ports/a/b");
    }

    #[test]
    fn unsafe_files_to_add_are_refused() {
        let mut entry = entry();
        entry.launcher.files_to_add = vec!["../evil.dll".into()];
        assert_eq!(
            plan_install(&request(entry, vec![verified_release()])),
            Err(PlanError::UnsafeRecipePath {
                path: "../evil.dll".into()
            })
        );
    }

    #[test]
    fn unsafe_entry_slots_are_refused_before_work_starts() {
        let mut entry = entry();
        entry.launcher.setup_files = vec![SetupFile {
            id: "game".into(),
            label: "Game folder".into(),
            kind: SetupKind::Folder,
            destination: Some(r"..\..\Windows".into()),
            ..SetupFile::default()
        }];
        let mut request = request(entry, vec![verified_release()]);
        request.supplied = vec![SuppliedChoice {
            slot: "game".into(),
            source: SuppliedSource::Path {
                path: r"D:\Dumps\Game".into(),
            },
        }];
        assert!(matches!(
            plan_install(&request),
            Err(PlanError::InvalidSlot { .. })
        ));
    }

    #[test]
    fn copies_to_user_data_roots_have_no_path_under_the_install() {
        let mut entry = entry();
        entry.launcher.setup_files = vec![
            SetupFile {
                id: "settings".into(),
                label: "Settings".into(),
                destination: Some("settings.ini".into()),
                root: Some(CopyRoot::Data),
                ..SetupFile::default()
            },
            SetupFile {
                id: "game".into(),
                label: "Game".into(),
                destination: Some("CUSA03173".into()),
                ..SetupFile::default()
            },
        ];
        let mut request = request(entry, vec![verified_release()]);
        request.supplied = vec![
            SuppliedChoice {
                slot: "settings".into(),
                source: SuppliedSource::Path {
                    path: r"D:\Dumps\settings.ini".into(),
                },
            },
            SuppliedChoice {
                slot: "game".into(),
                source: SuppliedSource::Path {
                    path: r"D:\Dumps\CUSA03173".into(),
                },
            },
        ];
        let plan = plan_install(&request).unwrap();
        let settings = plan
            .slots
            .iter()
            .find(|slot| slot.id == "settings")
            .unwrap();
        assert_eq!(settings.target, None);
        assert!(matches!(
            settings.placement,
            Placement::Copy {
                root: CopyRoot::Data,
                ..
            }
        ));
        let game = plan.slots.iter().find(|slot| slot.id == "game").unwrap();
        assert_eq!(
            game.target.as_deref(),
            Some(r"D:\Ports\Bloodborne-BloodbornePC\CUSA03173")
        );
    }

    #[test]
    fn unsafe_job_ids_are_refused() {
        let mut request = request(entry(), vec![verified_release()]);
        request.job_id = "..\\..\\evil".into();
        assert_eq!(
            plan_install(&request),
            Err(PlanError::InvalidJobId {
                job_id: "..\\..\\evil".into()
            })
        );
        request.job_id = "job_1-a".into();
        assert!(plan_install(&request).is_ok());
    }

    #[test]
    fn the_plan_round_trips_through_json_for_the_webview() {
        let plan = plan_install(&request(entry(), vec![verified_release()])).unwrap();
        let json = serde_json::to_string(&plan).expect("serializable");
        let decoded: PortInstallPlan = serde_json::from_str(&json).expect("deserializable");
        assert_eq!(decoded, plan);
        assert!(json.contains("\"catalogId\""));
        assert!(json.contains("\"spaceRequired\""));
        assert!(json.contains("\"folderName\""));
    }

    #[test]
    fn a_template_builds_the_library_registration() {
        let template = RegistrationTemplate {
            title: "Bloodborne".into(),
            executable: Some("Lantern Engine.exe".into()),
            working_directory: r"D:\Ports\Bloodborne".into(),
            arguments: vec!["--iso".into(), r"D:\game.iso".into()],
            gameplay: vec!["Bloodborne.exe".into()],
            launchers: vec!["Lantern Engine.exe".into()],
        };
        let registration = template.to_registration(
            "install-1".into(),
            "Lantern Engine.exe".into(),
            "volume:file".into(),
            true,
        );
        assert_eq!(registration.key(), "manual:install-1");
        assert_eq!(registration.root, r"D:\Ports\Bloodborne");
        assert_eq!(registration.arguments.len(), 2);
        assert_eq!(
            registration.launchers,
            vec![r"D:\Ports\Bloodborne\Lantern Engine.exe".to_string()]
        );
        assert_eq!(
            registration.gameplay,
            vec![r"D:\Ports\Bloodborne\Bloodborne.exe".to_string()]
        );
        assert!(registration.available);
    }

    #[test]
    fn template_paths_are_resolved_against_the_install_folder() {
        let template = RegistrationTemplate {
            title: "Bloodborne".into(),
            executable: Some("Lantern Engine.exe".into()),
            working_directory: r"D:\Ports\Bloodborne".into(),
            arguments: vec![],
            gameplay: vec![
                r"BloodbornePC/engine/out/bb-probe.exe".into(),
                r"E:\Shared\probe.exe".into(),
                r"..\escape.exe".into(),
            ],
            launchers: vec![r"Lantern Engine.exe".into()],
        };
        let registration = template.to_registration(
            "install-1".into(),
            r"D:\Ports\Bloodborne\Lantern Engine.exe".into(),
            "volume:file".into(),
            true,
        );
        assert_eq!(
            registration.gameplay,
            vec![
                r"D:\Ports\Bloodborne\BloodbornePC\engine\out\bb-probe.exe".to_string(),
                r"E:\Shared\probe.exe".to_string(),
            ],
            "relative paths resolve under the install folder and absolute ones stay"
        );
        assert_eq!(
            registration.launchers,
            vec![r"D:\Ports\Bloodborne\Lantern Engine.exe".to_string()]
        );
    }
}
