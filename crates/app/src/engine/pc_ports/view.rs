//! Builds the `Snapshot.ports` payload and the lazy detail payload from durable state.
//!
//! Everything here is pure: the caller passes the catalog entries, job rows, provenance rows,
//! live registration ids and remembered preferences it already read. The view types are the
//! UI contract (`ui/src/api/ports.ts`), so this module is where UI and host vocabulary meet:
//!
//! - catalog timestamps arrive in Unix milliseconds and leave in seconds;
//! - job phases collapse to the UI's flat vocabulary, with `AwaitingExecutable` shown as a
//!   completed install that still needs an executable;
//! - `CopyRoot::App` becomes the UI's `game` placement root.
//!
//! This module is also where a port entry's installability is decided. The upstream listing does
//! not carry release assets, so the entry starts optimistic and the detail response (which does
//! have them) adds facts that turn Install off with a reason.

use std::collections::{BTreeMap, HashMap, HashSet};

use gamenode_core::ports::catalog::{
    CatalogEntry, CopyRoot, Placement, PortAsset, PortRelease, SetupKind,
};
use gamenode_core::ports::job::{PortJobPhase as WorkerJobPhase, SuppliedStep};
use gamenode_core::ports::plan::{select_asset, select_release};
use gamenode_core::ports::store::{PortInstallation, PortJobRow, PortPreferences};
use gamenode_core::ports::view::{
    PortArgumentKind, PortArgumentSpec, PortArtView, PortCatalogError, PortCatalogState,
    PortCatalogStatus, PortDetailView, PortEntryView, PortFiltersView, PortInstallBlockReason,
    PortInstallationView, PortJobFailure, PortJobOutcome, PortJobPhaseView, PortJobView, PortLink,
    PortNoAssetReason, PortPackageOption, PortPlacementRoot, PortPreferencesView, PortProgress,
    PortProgressPhase, PortReleaseView, PortSetupKind, PortSetupSlot, PortsView,
};
use gamenode_platform::ports::catalog_worker::{CatalogError, CatalogState, CatalogStatus};

/// Facts a detail response adds to an entry the listing could not answer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EntryFacts {
    /// The selected release has no installable Windows asset.
    pub no_asset_reason: Option<PortNoAssetReason>,
    /// The selected release was withdrawn upstream.
    pub withdrawn: bool,
}

/// Everything one view build needs.
pub struct ViewInput<'a> {
    pub status: CatalogStatus,
    /// The stored snapshot with the overlay applied, already filtered to Windows entries.
    pub entries: &'a [CatalogEntry],
    /// Newest first.
    pub jobs: &'a [PortJobRow],
    pub installations: &'a [PortInstallation],
    /// Raw registration ids (without the `manual:`/`drmfree:` key prefix) that are live.
    pub live_registrations: &'a HashSet<String>,
    pub preferences: &'a PortPreferences,
    /// Detail-derived facts by catalog id.
    pub facts: &'a HashMap<String, EntryFacts>,
    /// Job ids the install runner currently owns.
    pub active_jobs: &'a HashSet<String>,
}

/// Builds the complete PC ports projection.
pub fn build(input: ViewInput<'_>) -> PortsView {
    let installed_keys = installed_registration_keys(
        input.entries,
        input.installations,
        input.jobs,
        input.live_registrations,
    );
    let entries = input
        .entries
        .iter()
        .map(|entry| {
            let facts = input
                .facts
                .get(&entry.catalog_id)
                .cloned()
                .unwrap_or_default();
            entry_view(
                entry,
                installed_keys.get(&entry.catalog_id).cloned(),
                &facts,
            )
        })
        .collect::<Vec<_>>();
    let jobs = input
        .jobs
        .iter()
        .map(|job| job_view(job, input.active_jobs.contains(&job.id)))
        .collect::<Vec<_>>();
    let unseen_count = jobs
        .iter()
        .filter(|job| match job.outcome {
            Some(PortJobOutcome::Cancelled) => false,
            Some(_) => !job.seen,
            None => true,
        })
        .count();
    PortsView {
        catalog: catalog_status(input.status),
        entries,
        provenance: provenance(input.installations, input.jobs),
        jobs,
        unseen_count,
        preferences: preferences_view(input.preferences),
    }
}

/// The UI's status vocabulary. `fetched_at` is Unix milliseconds in the worker and Unix seconds
/// in the contract; the conversion is here and tested.
pub fn catalog_status(status: CatalogStatus) -> PortCatalogStatus {
    PortCatalogStatus {
        state: match status.state {
            CatalogState::Idle => PortCatalogState::Idle,
            CatalogState::Refreshing => PortCatalogState::Refreshing,
            CatalogState::Ready => PortCatalogState::Ready,
            CatalogState::Unavailable => PortCatalogState::Unavailable,
            CatalogState::Stale => PortCatalogState::Stale,
        },
        fetched_at: status
            .fetched_at
            .map(|millis| millis.div_euclid(1000).max(0)),
        entry_count: status.entry_count,
        error: status.error.map(|error| match error {
            CatalogError::Unreachable => PortCatalogError::Unreachable,
            CatalogError::RateLimited => PortCatalogError::RateLimited,
            CatalogError::InvalidResponse => PortCatalogError::InvalidResponse,
            CatalogError::EmptyListing => PortCatalogError::EmptyListing,
            CatalogError::Unknown => PortCatalogError::Unknown,
        }),
    }
}

/// One merged catalog entry as the UI shows it.
pub fn entry_view(
    entry: &CatalogEntry,
    installed_registration_key: Option<String>,
    facts: &EntryFacts,
) -> PortEntryView {
    let pinned = entry
        .pinned_release
        .as_deref()
        .map(str::trim)
        .filter(|version| !version.is_empty());
    let verified = entry.has_verified_release();
    let version = pinned.map(str::to_owned).or_else(|| {
        entry
            .verified
            .as_ref()
            .map(|release| release.version.clone())
    });
    let withdrawn = !entry.withdrawn.is_empty() || facts.withdrawn;
    let no_asset_reason = facts.no_asset_reason;
    let installable = entry.supports_windows()
        && !withdrawn
        && (verified || pinned.is_some())
        && no_asset_reason.is_none();
    let install_block_reason = if withdrawn {
        Some(PortInstallBlockReason::Withdrawn)
    } else if !verified && pinned.is_none() {
        Some(PortInstallBlockReason::NoVerifiedRelease)
    } else {
        no_asset_reason.map(PortInstallBlockReason::from)
    };
    PortEntryView {
        catalog_id: entry.catalog_id.clone(),
        slug: entry.slug.clone(),
        name: entry.name.clone(),
        games: entry.game_titles().map(str::to_owned).collect(),
        consoles: console_names(&entry.consoles),
        supported_os: entry.supported_os.clone(),
        verified,
        prerelease: entry.is_prerelease_only(),
        version,
        guided: entry.has_guided_setup(),
        withdrawn,
        has_windows_asset: no_asset_reason.is_none(),
        no_asset_reason,
        installable,
        install_block_reason,
        art: art_view(entry),
        project: project_link(entry),
        // The upstream model has no original-game URL, so the optional Source row stays empty.
        source: None,
        license: entry
            .license
            .as_deref()
            .map(str::trim)
            .filter(|license| !license.is_empty())
            .map(str::to_owned),
        requirements: setup_slots(entry),
        installed_registration_key,
    }
}

/// The setup slots of a merged recipe, in recipe order.
pub fn setup_slots(entry: &CatalogEntry) -> Vec<PortSetupSlot> {
    entry
        .launcher
        .setup_files
        .iter()
        .map(|slot| {
            let placement = slot.placement().ok();
            let (root, destination, argument) = match &placement {
                Some(Placement::Copy { root, destination }) => {
                    (Some(placement_root(*root)), Some(destination.clone()), None)
                }
                Some(Placement::Flag { flag, .. }) => (
                    None,
                    None,
                    Some(PortArgumentSpec {
                        kind: PortArgumentKind::Flag,
                        name: flag.clone(),
                    }),
                ),
                Some(Placement::Positional) => (
                    None,
                    None,
                    Some(PortArgumentSpec {
                        kind: PortArgumentKind::Positional,
                        name: String::new(),
                    }),
                ),
                Some(Placement::Env { name }) => (
                    None,
                    None,
                    Some(PortArgumentSpec {
                        kind: PortArgumentKind::Env,
                        name: name.clone(),
                    }),
                ),
                Some(Placement::Unused) | None => (None, None, None),
            };
            PortSetupSlot {
                id: slot.id.clone(),
                label: slot.label.clone(),
                prompt: slot
                    .prompt
                    .as_deref()
                    .map(str::trim)
                    .filter(|prompt| !prompt.is_empty())
                    .map(str::to_owned),
                kind: match slot.kind {
                    SetupKind::File => PortSetupKind::File,
                    SetupKind::Folder => PortSetupKind::Folder,
                },
                required: slot.required,
                extensions: slot
                    .extensions
                    .iter()
                    .map(|extension| {
                        extension
                            .trim()
                            .trim_start_matches('.')
                            .to_ascii_lowercase()
                    })
                    .filter(|extension| !extension.is_empty())
                    .map(|extension| format!(".{extension}"))
                    .collect(),
                contains: slot.contains.clone(),
                accepted_labels: slot
                    .accepted
                    .iter()
                    .map(|accepted| {
                        accepted
                            .label
                            .as_deref()
                            .map(str::trim)
                            .filter(|label| !label.is_empty())
                            .map(str::to_owned)
                            .unwrap_or_else(|| {
                                format!("{}:{}", accepted.algo.label(), accepted.value)
                            })
                    })
                    .collect(),
                destination,
                root,
                argument,
                package_option: slot.from_package.as_ref().map(|package| {
                    let extractor = entry.extractor(&package.extractor);
                    PortPackageOption {
                        extractor_id: package.extractor.clone(),
                        tool_name: extractor
                            .map(|extractor| extractor.name.clone())
                            .filter(|name| !name.trim().is_empty())
                            .unwrap_or_else(|| package.extractor.clone()),
                        tool_source_url: extractor
                            .filter(|extractor| !extractor.repository.trim().is_empty())
                            .map(|extractor| {
                                if extractor.repository.contains("://") {
                                    extractor.repository.clone()
                                } else {
                                    format!(
                                        "https://github.com/{}",
                                        extractor.repository.trim_matches('/')
                                    )
                                }
                            }),
                        tool_license: extractor
                            .map(|extractor| extractor.license.clone())
                            .filter(|license| !license.trim().is_empty()),
                        title_id: package.title_id.clone(),
                        update_version: package.needs_update.clone(),
                        requires_update: package.needs_update.is_some(),
                    }
                }),
            }
        })
        .collect()
}

fn placement_root(root: CopyRoot) -> PortPlacementRoot {
    match root {
        CopyRoot::App => PortPlacementRoot::Game,
        CopyRoot::Data => PortPlacementRoot::Data,
        CopyRoot::Config => PortPlacementRoot::Config,
        CopyRoot::LocalData => PortPlacementRoot::LocalData,
    }
}

fn art_view(entry: &CatalogEntry) -> PortArtView {
    PortArtView {
        capsule: has_text(entry.art.capsule.as_deref()),
        hero: has_text(entry.art.hero.as_deref()),
    }
}

fn project_link(entry: &CatalogEntry) -> Option<PortLink> {
    let project = entry.project.as_ref()?;
    let url = project.source_url()?;
    let label = [
        project.name.as_str(),
        project.repository.as_deref().unwrap_or(""),
    ]
    .into_iter()
    .map(str::trim)
    .find(|label| !label.is_empty())
    .unwrap_or(&entry.name)
    .to_owned();
    Some(PortLink { label, url })
}

fn has_text(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

/// Turns upstream console ids (`ps4`) into the names the UI shows (`PlayStation 4`).
pub fn console_names(consoles: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    for console in consoles {
        let trimmed = console.trim();
        if trimmed.is_empty() {
            continue;
        }
        let name = console_name(trimmed);
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

fn console_name(console: &str) -> String {
    let known = match console.to_ascii_lowercase().as_str() {
        "ps1" | "psx" | "playstation" => "PlayStation",
        "ps2" | "playstation2" => "PlayStation 2",
        "ps3" | "playstation3" => "PlayStation 3",
        "ps4" | "playstation4" => "PlayStation 4",
        "ps5" | "playstation5" => "PlayStation 5",
        "psp" => "PlayStation Portable",
        "psvita" | "psv" => "PlayStation Vita",
        "nes" => "NES",
        "snes" => "SNES",
        "n64" => "Nintendo 64",
        "gc" | "gamecube" => "GameCube",
        "wii" => "Wii",
        "wiiu" => "Wii U",
        "switch" => "Nintendo Switch",
        "gb" => "Game Boy",
        "gbc" => "Game Boy Color",
        "gba" => "Game Boy Advance",
        "nds" | "ds" => "Nintendo DS",
        "3ds" => "Nintendo 3DS",
        "xbox" => "Xbox",
        "xbox360" => "Xbox 360",
        "xboxone" => "Xbox One",
        "xboxseries" | "xboxseriesx" => "Xbox Series",
        "pc" | "windows" => "PC",
        "saturn" => "Sega Saturn",
        "dreamcast" => "Dreamcast",
        "genesis" | "megadrive" => "Sega Genesis",
        _ => "",
    };
    if !known.is_empty() {
        return known.to_owned();
    }
    let mut chars = console.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn installed_registration_keys(
    entries: &[CatalogEntry],
    installations: &[PortInstallation],
    jobs: &[PortJobRow],
    live: &HashSet<String>,
) -> HashMap<String, String> {
    let by_job: HashMap<&str, &PortInstallation> = installations
        .iter()
        .map(|installation| (installation.job_id.as_str(), installation))
        .collect();
    let mut installed = HashMap::new();
    // Jobs are newest first, so the first match is the installation PC ports should link to.
    for job in jobs {
        let Some(registration_id) = job.registration_id.as_deref() else {
            continue;
        };
        if !live.contains(registration_id) {
            continue;
        }
        let Some(installation) = by_job.get(job.id.as_str()) else {
            continue;
        };
        if installation.registration_id != registration_id {
            continue;
        }
        if !entries
            .iter()
            .any(|entry| entry.catalog_id == installation.catalog_id)
        {
            continue;
        }
        installed
            .entry(installation.catalog_id.clone())
            .or_insert_with(|| registration_key(registration_id));
    }
    installed
}

/// The UI contract names DRM-free registrations `drmfree:<id>` (the library provider's own keys
/// are `manual:<id>`; the app translates when a command selects one).
pub fn registration_key(registration_id: &str) -> String {
    format!("drmfree:{registration_id}")
}

fn provenance(
    installations: &[PortInstallation],
    jobs: &[PortJobRow],
) -> BTreeMap<String, PortInstallationView> {
    let by_job: HashMap<&str, &PortJobRow> =
        jobs.iter().map(|job| (job.id.as_str(), job)).collect();
    installations
        .iter()
        .map(|installation| {
            let job = by_job.get(installation.job_id.as_str()).copied();
            let key = registration_key(&installation.registration_id);
            let view = PortInstallationView {
                registration_key: key.clone(),
                catalog_id: installation.catalog_id.clone(),
                slug: installation.slug.clone(),
                port_name: installation.port_name.clone(),
                installed_version: installation.installed_version.clone(),
                install_folder: installation.install_folder.clone(),
                job_id: Some(installation.job_id.clone()),
                installed_at: job
                    .and_then(|job| job.finished_at.or(Some(job.updated_at)))
                    .map(|millis| millis.div_euclid(1000).max(0)),
            };
            (key, view)
        })
        .collect()
}

fn job_view(job: &PortJobRow, active: bool) -> PortJobView {
    let awaiting = matches!(job.phase, WorkerJobPhase::AwaitingExecutable);
    let (phase, outcome) = match &job.phase {
        WorkerJobPhase::Queued => (PortJobPhaseView::Queued, None),
        WorkerJobPhase::Resolving => (PortJobPhaseView::Resolving, None),
        WorkerJobPhase::Downloading => (PortJobPhaseView::Downloading, None),
        WorkerJobPhase::Verifying => (PortJobPhaseView::Verifying, None),
        WorkerJobPhase::Extracting => (PortJobPhaseView::Extracting, None),
        WorkerJobPhase::SuppliedFiles { .. } => (PortJobPhaseView::SuppliedFiles, None),
        WorkerJobPhase::Finalizing => (PortJobPhaseView::Finalizing, None),
        // The UI treats a job that only waits for an executable as a completed install whose
        // follow-up picker is still open; it has no `awaitingExecutable` phase of its own.
        WorkerJobPhase::AwaitingExecutable => {
            (PortJobPhaseView::Completed, Some(PortJobOutcome::Completed))
        }
        WorkerJobPhase::Registering => (PortJobPhaseView::Registering, None),
        WorkerJobPhase::Completed => (PortJobPhaseView::Completed, Some(PortJobOutcome::Completed)),
        WorkerJobPhase::Cancelling => (PortJobPhaseView::Cancelling, None),
        WorkerJobPhase::Cancelled => (PortJobPhaseView::Cancelled, Some(PortJobOutcome::Cancelled)),
        WorkerJobPhase::Failed { .. } => (PortJobPhaseView::Failed, Some(PortJobOutcome::Failed)),
        WorkerJobPhase::Interrupted { .. } => (
            PortJobPhaseView::Interrupted,
            Some(PortJobOutcome::Interrupted),
        ),
    };
    let failure = match &job.phase {
        WorkerJobPhase::Failed { reason } => Some(PortJobFailure {
            message: reason.clone(),
            tool_output: job.failure_output.clone(),
        }),
        WorkerJobPhase::Interrupted { reason } => Some(PortJobFailure {
            message: reason.clone(),
            tool_output: job.failure_output.clone(),
        }),
        _ => None,
    };
    PortJobView {
        job_id: job.id.clone(),
        catalog_id: job.catalog_id.clone(),
        slug: job.slug.clone(),
        name: job.port_name.clone(),
        phase,
        slot_id: match &job.phase {
            WorkerJobPhase::SuppliedFiles { slot, .. } => Some(slot.clone()),
            _ => None,
        },
        outcome,
        failure,
        interruptible: !job.phase.is_terminal() && !awaiting && active,
        retryable: matches!(
            job.phase,
            WorkerJobPhase::Failed { .. }
                | WorkerJobPhase::Cancelled
                | WorkerJobPhase::Interrupted { .. }
        ),
        needs_executable: awaiting,
        seen: job.seen,
        started_at: Some(job.created_at.div_euclid(1000).max(0)),
        finished_at: job.finished_at.map(|millis| millis.div_euclid(1000).max(0)),
    }
}

fn preferences_view(preferences: &PortPreferences) -> PortPreferencesView {
    PortPreferencesView {
        view: preferences.view,
        filters: PortFiltersView {
            consoles: preferences.consoles.clone(),
            guided: preferences.guided,
            verified: preferences.verified,
            installed: preferences.installed,
        },
        last_parent_folder: preferences.last_parent.clone(),
    }
}

/// Builds `ports_detail` and the facts the entry list should learn from it.
pub fn build_detail(
    entry: &CatalogEntry,
    detail: Option<&CatalogEntry>,
    releases: &[PortRelease],
    installed_version: Option<&str>,
) -> (PortDetailView, EntryFacts) {
    let merged = detail.unwrap_or(entry);
    let mut releases = releases.to_vec();
    releases.sort_by_key(|release| std::cmp::Reverse(release.released_at));

    // A withdrawn release still opens the detail pane so it can explain the withdrawal; only
    // the install plan refuses it.
    let configured_version = merged
        .pinned_release
        .as_deref()
        .map(str::trim)
        .filter(|version| !version.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            merged
                .verified
                .as_ref()
                .map(|release| release.version.clone())
        });
    let selected = select_release(merged, &releases, &merged.withdrawn)
        .ok()
        .or_else(|| {
            let version = configured_version.as_deref()?;
            releases
                .iter()
                .find(|release| release.version == version)
                .cloned()
        });
    let selected_view = selected
        .as_ref()
        .map(|release| release_view(merged, release));
    let selected_asset = selected
        .as_ref()
        .and_then(|release| select_asset(merged, release).ok());
    let selected_withdrawn = selected.as_ref().is_some_and(|release| {
        merged
            .withdrawn
            .iter()
            .any(|withdrawn| withdrawn.version == release.version)
    });
    let no_asset_reason = match (&selected, &selected_asset) {
        (None, _) | (_, Some(_)) => None,
        (Some(_), None) => Some(no_asset_reason(merged, selected.as_ref())),
    };
    let withdrawal_reason = configured_version.as_deref().and_then(|version| {
        merged
            .withdrawn
            .iter()
            .find(|withdrawn| withdrawn.version == version)
            .map(|withdrawn| withdrawn.reason.clone())
            .filter(|reason| !reason.trim().is_empty())
    });
    let update_available = update_available(merged, &releases, installed_version);
    let facts = EntryFacts {
        no_asset_reason,
        withdrawn: selected_withdrawn,
    };
    let view = PortDetailView {
        catalog_id: merged.catalog_id.clone(),
        description: text(&merged.description),
        developer: merged
            .developer
            .as_ref()
            .map(|developer| developer.name.clone())
            .and_then(|name| text(&name)),
        license: text(merged.license.as_deref().unwrap_or_default()),
        project: project_link(merged),
        source: None,
        art: art_view(merged),
        requirements: setup_slots(merged),
        releases: releases
            .iter()
            .map(|release| release_view(merged, release))
            .collect(),
        selected_release: selected_view,
        withdrawal_reason,
        update_available,
        no_asset_reason,
    };
    (view, facts)
}

fn no_asset_reason(entry: &CatalogEntry, release: Option<&PortRelease>) -> PortNoAssetReason {
    let Some(release) = release else {
        return PortNoAssetReason::NoWindowsAsset;
    };
    match select_asset(entry, release) {
        Ok(_) => PortNoAssetReason::NoExecutable,
        Err(gamenode_core::ports::plan::PlanError::NoWindowsAsset) => {
            PortNoAssetReason::NoWindowsAsset
        }
        Err(gamenode_core::ports::plan::PlanError::NoMatchingAsset { .. })
        | Err(gamenode_core::ports::plan::PlanError::NoChecksum) => PortNoAssetReason::MissingAsset,
        Err(gamenode_core::ports::plan::PlanError::UnknownAssetFormat { .. })
        | Err(gamenode_core::ports::plan::PlanError::UnsupportedFormat { .. }) => {
            PortNoAssetReason::InstallerOnly
        }
        Err(_) => PortNoAssetReason::MissingAsset,
    }
}

fn release_view(entry: &CatalogEntry, release: &PortRelease) -> PortReleaseView {
    let asset = select_asset(entry, release).ok();
    let withdrawn = entry
        .withdrawn
        .iter()
        .any(|withdrawn| withdrawn.version == release.version);
    PortReleaseView {
        version: release.version.clone(),
        released_at: iso_date(release.released_at),
        verified: entry
            .verified
            .as_ref()
            .is_some_and(|verified| verified.version == release.version),
        withdrawn,
        asset_name: asset.as_ref().map(|asset| asset.filename.clone()),
        asset_format: asset.as_ref().and_then(normalized_format),
        size_bytes: asset.as_ref().map(|asset| asset.size),
        checksum_verified: asset
            .as_ref()
            .is_some_and(|asset| asset.verified_sha256().is_some()),
        requires_checksum_confirmation: asset.as_ref().is_some_and(|asset| {
            asset.verified_sha256().is_none() && asset.unverified_sha256().is_some()
        }),
        malware_flag: asset.as_ref().and_then(|asset| {
            asset
                .scan
                .as_ref()
                .filter(|scan| scan.verdict == gamenode_core::ports::catalog::ScanVerdict::Flagged)
                .map(|scan| {
                    scan.engines
                        .clone()
                        .filter(|engines| !engines.trim().is_empty())
                        .unwrap_or_else(|| "a malware scan flagged this file".to_owned())
                })
        }),
    }
}

/// Only the formats the UI knows leave the host; an installer or an archive the installer does
/// not handle reports no format and is explained by [`PortNoAssetReason::InstallerOnly`].
fn normalized_format(asset: &PortAsset) -> Option<String> {
    let format = asset.normalized_format().or_else(|| {
        asset
            .filename
            .rsplit_once('.')
            .map(|(_, extension)| extension.trim().to_ascii_lowercase())
            .filter(|extension| !extension.is_empty())
    })?;
    matches!(format.as_str(), "zip" | "7z" | "exe").then_some(format)
}

fn update_available(
    entry: &CatalogEntry,
    releases: &[PortRelease],
    installed_version: Option<&str>,
) -> Option<PortReleaseView> {
    let installed_version = installed_version?.trim();
    let verified = entry.verified.as_ref()?;
    if verified.version == installed_version {
        return None;
    }
    let verified_release = releases
        .iter()
        .find(|release| release.version == verified.version)?;
    let installed_index = releases
        .iter()
        .position(|release| release.version == installed_version);
    let verified_index = releases
        .iter()
        .position(|release| release.version == verified.version)?;
    // The list is newest first. A verified release that precedes the installed one is newer.
    if installed_index.is_some_and(|index| verified_index > index) {
        return None;
    }
    Some(release_view(entry, verified_release))
}

fn text(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// Formats Unix milliseconds as the ISO-8601 calendar date the UI parses.
pub fn iso_date(millis: i64) -> Option<String> {
    if millis <= 0 {
        return None;
    }
    let days = millis.div_euclid(1000).div_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to a proleptic Gregorian date.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    (year, month as u32, day as u32)
}

/// Maps a worker progress phase to the `ports:progress` vocabulary.
pub fn progress_from_job(progress: gamenode_platform::ports::JobProgress) -> Option<PortProgress> {
    let phase = match &progress.phase {
        WorkerJobPhase::Downloading => PortProgressPhase::Downloading,
        WorkerJobPhase::Verifying => PortProgressPhase::Verifying,
        WorkerJobPhase::Extracting => PortProgressPhase::Extracting,
        WorkerJobPhase::Finalizing => PortProgressPhase::Finalizing,
        WorkerJobPhase::Registering => PortProgressPhase::Registering,
        WorkerJobPhase::SuppliedFiles { step, .. } => match step {
            SuppliedStep::Hashing => PortProgressPhase::Hashing,
            SuppliedStep::ExtractingPackage => PortProgressPhase::ExtractingPackage,
            SuppliedStep::Copying => PortProgressPhase::Copying,
        },
        _ => return None,
    };
    Some(PortProgress {
        job_id: progress.job_id,
        phase,
        slot_id: progress.slot,
        bytes_done: progress.done,
        bytes_total: progress.total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gamenode_core::ports::catalog::{
        AcceptedHash, CatalogProject, HashAlgorithm, LauncherRecipe, PreferredExecutables,
        SetupFile, VerifiedRelease, WithdrawnRelease,
    };

    fn entry() -> CatalogEntry {
        CatalogEntry {
            catalog_id: "qcat_lantern".into(),
            slug: "bloodborne-pc".into(),
            name: "Bloodborne".into(),
            supported_os: vec!["windows".into()],
            consoles: vec!["ps4".into()],
            games: vec![gamenode_core::ports::catalog::CatalogGame {
                title: "Bloodborne".into(),
                ..Default::default()
            }],
            project: Some(CatalogProject {
                name: "Lantern Engine".into(),
                repository: Some("owner/lantern".into()),
                ..Default::default()
            }),
            verified: Some(VerifiedRelease {
                version: "v0.9.38".into(),
                released_at: 1_793_220_348_000,
                ..Default::default()
            }),
            launcher: LauncherRecipe {
                folder_name: Some("Bloodborne".into()),
                preferred_executables: PreferredExecutables {
                    windows: vec!["Bloodborne.exe".into()],
                    ..Default::default()
                },
                setup_files: vec![SetupFile {
                    id: "game".into(),
                    label: "Bloodborne (CUSA03173)".into(),
                    kind: SetupKind::Folder,
                    contains: vec!["eboot.bin".into()],
                    destination: Some("CUSA03173".into()),
                    accepted: vec![AcceptedHash {
                        algo: HashAlgorithm::Sha256,
                        value: "a".repeat(64),
                        label: Some("CUSA03173 01.09".into()),
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn job(id: &str, phase: WorkerJobPhase) -> PortJobRow {
        PortJobRow {
            id: id.into(),
            catalog_id: "qcat_lantern".into(),
            slug: "bloodborne-pc".into(),
            port_name: "Bloodborne".into(),
            release_version: Some("v0.9.38".into()),
            install_folder: Some(r"D:\Ports\Bloodborne".into()),
            phase,
            created_paths: Vec::new(),
            registration_id: None,
            error: None,
            failure_output: None,
            seen: false,
            created_at: 1_793_220_348_123,
            updated_at: 1_793_220_400_999,
            finished_at: None,
        }
    }

    fn input<'a>(
        entries: &'a [CatalogEntry],
        jobs: &'a [PortJobRow],
        installations: &'a [PortInstallation],
        live: &'a HashSet<String>,
        active: &'a HashSet<String>,
    ) -> ViewInput<'a> {
        static STATUS: std::sync::OnceLock<CatalogStatus> = std::sync::OnceLock::new();
        static PREFERENCES: std::sync::OnceLock<PortPreferences> = std::sync::OnceLock::new();
        static FACTS: std::sync::OnceLock<HashMap<String, EntryFacts>> = std::sync::OnceLock::new();
        let status = STATUS.get_or_init(|| CatalogStatus {
            state: CatalogState::Ready,
            fetched_at: Some(1_793_220_348_000),
            entry_count: 1,
            error: None,
        });
        let preferences = PREFERENCES.get_or_init(PortPreferences::default);
        let facts = FACTS.get_or_init(HashMap::new);
        ViewInput {
            status: status.clone(),
            entries,
            jobs,
            installations,
            live_registrations: live,
            preferences,
            facts,
            active_jobs: active,
        }
    }

    #[test]
    fn fetched_at_is_milliseconds_in_the_worker_and_seconds_in_the_contract() {
        let status = catalog_status(CatalogStatus {
            state: CatalogState::Ready,
            fetched_at: Some(1_793_220_348_999),
            entry_count: 7,
            error: None,
        });
        assert_eq!(status.fetched_at, Some(1_793_220_348));
        assert_eq!(status.entry_count, 7);
        assert_eq!(status.state, PortCatalogState::Ready);
    }

    #[test]
    fn entries_carry_console_names_slots_and_the_project_link() {
        let entries = [entry()];
        let live = HashSet::new();
        let active = HashSet::new();
        let view = build(input(&entries, &[], &[], &live, &active));
        let entry = &view.entries[0];
        assert_eq!(entry.consoles, vec!["PlayStation 4"]);
        assert_eq!(entry.version.as_deref(), Some("v0.9.38"));
        assert!(entry.installable);
        assert_eq!(entry.requirements[0].root, Some(PortPlacementRoot::Game));
        assert_eq!(
            entry.requirements[0].destination.as_deref(),
            Some("CUSA03173")
        );
        assert_eq!(entry.requirements[0].accepted_labels, ["CUSA03173 01.09"]);
        assert_eq!(
            entry.project.as_ref().map(|link| link.url.as_str()),
            Some("https://github.com/owner/lantern")
        );
        assert!(!entry.art.capsule);
    }

    #[test]
    fn withdrawn_and_no_asset_entries_are_not_installable() {
        let mut withdrawn = entry();
        withdrawn.withdrawn.push(WithdrawnRelease {
            version: "v0.9.38".into(),
            reason: "unsafe".into(),
            at: 0,
        });
        let facts = EntryFacts {
            no_asset_reason: Some(PortNoAssetReason::InstallerOnly),
            withdrawn: false,
        };
        let view = entry_view(&withdrawn, None, &EntryFacts::default());
        assert_eq!(
            view.install_block_reason,
            Some(PortInstallBlockReason::Withdrawn)
        );
        assert!(!view.installable);
        let view = entry_view(&entry(), None, &facts);
        assert_eq!(
            view.install_block_reason,
            Some(PortInstallBlockReason::InstallerOnly)
        );
        assert!(!view.has_windows_asset);
    }

    #[test]
    fn awaiting_an_executable_reads_as_a_completed_install_that_needs_one() {
        let jobs = [job("job-1", WorkerJobPhase::AwaitingExecutable)];
        let live = HashSet::new();
        let active: HashSet<String> = ["job-1".to_owned()].into_iter().collect();
        let view = build(input(&[], &jobs, &[], &live, &active));
        let job = &view.jobs[0];
        assert_eq!(job.phase, PortJobPhaseView::Completed);
        assert_eq!(job.outcome, Some(PortJobOutcome::Completed));
        assert!(job.needs_executable);
        assert!(!job.interruptible);
        assert_eq!(view.unseen_count, 1);
        let json = serde_json::to_value(job).unwrap();
        assert_eq!(json["startedAt"], 1_793_220_348_i64);
    }

    #[test]
    fn the_unseen_count_excludes_seen_and_cancelled_jobs_and_includes_running_ones() {
        let mut running = job("run", WorkerJobPhase::Downloading);
        running.seen = false;
        let mut seen = job("seen", WorkerJobPhase::Completed);
        seen.seen = true;
        let mut failed = job(
            "failed",
            WorkerJobPhase::Failed {
                reason: "boom".into(),
            },
        );
        failed.seen = false;
        let mut cancelled = job("cancelled", WorkerJobPhase::Cancelled);
        cancelled.seen = false;
        let jobs = [running, seen, failed, cancelled];
        let live = HashSet::new();
        let active: HashSet<String> = ["run".to_owned()].into_iter().collect();
        let view = build(input(&[], &jobs, &[], &live, &active));
        assert_eq!(view.unseen_count, 2);
    }

    #[test]
    fn a_live_registration_links_the_entry_to_its_provenance() {
        let entries = [entry()];
        let installation = PortInstallation {
            registration_id: "reg-1".into(),
            catalog_id: "qcat_lantern".into(),
            slug: "bloodborne-pc".into(),
            port_name: "Bloodborne".into(),
            installed_version: "v0.9.38".into(),
            install_folder: r"D:\Ports\Bloodborne".into(),
            environment: BTreeMap::new(),
            job_id: "job-1".into(),
        };
        let mut completed = job("job-1", WorkerJobPhase::Completed);
        completed.registration_id = Some("reg-1".into());
        completed.finished_at = Some(1_793_220_500_000);
        let installations = [installation];
        let live: HashSet<String> = ["reg-1".to_owned()].into_iter().collect();
        let active = HashSet::new();
        let view = build(input(
            &entries,
            &[completed],
            &installations,
            &live,
            &active,
        ));
        assert_eq!(
            view.entries[0].installed_registration_key.as_deref(),
            Some("drmfree:reg-1")
        );
        let provenance = &view.provenance["drmfree:reg-1"];
        assert_eq!(provenance.install_folder, r"D:\Ports\Bloodborne");
        assert_eq!(provenance.installed_at, Some(1_793_220_500));

        let gone = HashSet::new();
        let view = build(input(&entries, &[], &installations, &gone, &active));
        assert_eq!(view.entries[0].installed_registration_key, None);
        assert!(view.provenance.contains_key("drmfree:reg-1"));
    }

    #[test]
    fn detail_picks_the_verified_release_and_its_windows_asset() {
        let entry = entry();
        let release = PortRelease {
            release_id: "rel".into(),
            version: "v0.9.38".into(),
            released_at: 1_793_220_348_000,
            assets: vec![PortAsset {
                id: "win".into(),
                url: "https://example.com/port.zip".into(),
                filename: "bloodborne-windows.zip".into(),
                os: Some("windows".into()),
                architecture: Some("x86_64".into()),
                format: Some("zip".into()),
                size: 100,
                checksum: Some(format!("sha256:{}", "b".repeat(64))),
                ..Default::default()
            }],
            ..Default::default()
        };
        let (detail, facts) = build_detail(&entry, None, &[release], None);
        assert_eq!(facts, EntryFacts::default());
        let selected = detail.selected_release.expect("selected");
        assert_eq!(selected.version, "v0.9.38");
        assert_eq!(
            selected.asset_name.as_deref(),
            Some("bloodborne-windows.zip")
        );
        assert_eq!(selected.size_bytes, Some(100));
        assert!(selected.checksum_verified);
        assert!(!selected.requires_checksum_confirmation);
        assert_eq!(
            detail.releases[0].released_at.as_deref(),
            Some("2026-10-28")
        );
    }

    #[test]
    fn an_installer_only_asset_turns_install_off_after_detail() {
        let entry = entry();
        let release = PortRelease {
            release_id: "rel".into(),
            version: "v0.9.38".into(),
            released_at: 1,
            assets: vec![PortAsset {
                id: "win".into(),
                url: "https://example.com/setup.exe".into(),
                filename: "setup.msi".into(),
                os: Some("windows".into()),
                format: Some("msi".into()),
                size: 1,
                checksum: Some(format!("sha256:{}", "c".repeat(64))),
                ..Default::default()
            }],
            ..Default::default()
        };
        let (detail, facts) = build_detail(&entry, None, &[release], None);
        assert_eq!(
            facts.no_asset_reason,
            Some(PortNoAssetReason::InstallerOnly)
        );
        assert_eq!(
            detail.no_asset_reason,
            Some(PortNoAssetReason::InstallerOnly)
        );
    }

    #[test]
    fn a_newer_verified_release_is_offered_as_an_update() {
        let entry = entry();
        let newer = PortRelease {
            release_id: "new".into(),
            version: "v0.9.40".into(),
            released_at: 2,
            ..Default::default()
        };
        let older = PortRelease {
            release_id: "old".into(),
            version: "v0.9.38".into(),
            released_at: 1,
            ..Default::default()
        };
        let mut entry = entry;
        entry.verified = Some(VerifiedRelease {
            version: "v0.9.40".into(),
            ..Default::default()
        });
        let (detail, _) = build_detail(&entry, None, &[newer, older], Some("v0.9.38"));
        assert_eq!(
            detail.update_available.map(|release| release.version),
            Some("v0.9.40".to_owned())
        );
    }

    #[test]
    fn a_withdrawn_release_still_explains_itself_in_the_detail_pane() {
        let mut entry = entry();
        entry.withdrawn.push(WithdrawnRelease {
            version: "v0.9.38".into(),
            reason: "the author pulled it".into(),
            at: 1,
        });
        let release = PortRelease {
            release_id: "rel".into(),
            version: "v0.9.38".into(),
            released_at: 1_793_220_348_000,
            assets: vec![PortAsset {
                id: "win".into(),
                url: "https://example.com/port.zip".into(),
                filename: "bloodborne-windows.zip".into(),
                os: Some("windows".into()),
                format: Some("zip".into()),
                size: 10,
                checksum: Some(format!("sha256:{}", "d".repeat(64))),
                ..Default::default()
            }],
            ..Default::default()
        };
        let (detail, facts) = build_detail(&entry, None, &[release], None);
        assert!(facts.withdrawn);
        assert_eq!(
            detail.withdrawal_reason.as_deref(),
            Some("the author pulled it")
        );
        assert_eq!(
            detail
                .selected_release
                .as_ref()
                .map(|release| (release.version.as_str(), release.withdrawn)),
            Some(("v0.9.38", true))
        );
        let view = entry_view(&entry, None, &facts);
        assert!(!view.installable);
        assert_eq!(
            view.install_block_reason,
            Some(PortInstallBlockReason::Withdrawn)
        );
    }

    #[test]
    fn iso_dates_follow_the_gregorian_calendar() {
        assert_eq!(iso_date(0), None);
        assert_eq!(iso_date(1_700_000_000_000).as_deref(), Some("2023-11-14"));
        assert_eq!(iso_date(1_793_220_348_000).as_deref(), Some("2026-10-28"));
        assert_eq!(iso_date(951_782_400_000).as_deref(), Some("2000-02-29"));
    }

    #[test]
    fn progress_phases_use_the_ui_vocabulary() {
        let progress = gamenode_platform::ports::JobProgress {
            job_id: "job".into(),
            phase: WorkerJobPhase::SuppliedFiles {
                slot: "game".into(),
                step: SuppliedStep::Copying,
            },
            slot: Some("game".into()),
            done: 5,
            total: 10,
        };
        let view = progress_from_job(progress).expect("mapped");
        assert_eq!(view.phase, PortProgressPhase::Copying);
        assert_eq!(view.job_id, "job");
        assert_eq!(view.bytes_done, 5);
        let queued = gamenode_platform::ports::JobProgress {
            job_id: "job".into(),
            phase: WorkerJobPhase::Queued,
            slot: None,
            done: 0,
            total: 0,
        };
        assert!(progress_from_job(queued).is_none());
    }
}
