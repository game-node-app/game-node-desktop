//! Durable PC ports state behind a host port (design D5, D10).
//!
//! The SQLite implementation lives in `gamenode-platform`; this module only fixes the contract that
//! the install worker, the catalog worker and the snapshot depend on. Every method may block, so
//! callers run it on the engine's storage thread.
//!
//! Two rules are load-bearing:
//!
//! - a catalog snapshot is replaced in one transaction, so a reader never sees a half-written list;
//! - an install job's created paths are journaled *before* the path is written, so startup recovery
//!   can remove partial work after a crash.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::drm_free::Registration;
use crate::ports::StorageError;
use crate::ports::catalog::CatalogSnapshot;
use crate::ports::job::PortJobPhase;
use crate::ports::matching::PortMatchInput;
use crate::ports::plan::PortInstallPlan;

/// The durable port state.
///
/// Everything here belongs to the Windows user and is shared across GameNode accounts: no method
/// takes an account id, and nothing here is a credential.
pub trait PortStore: Send + Sync {
    /// Replaces the cached catalog snapshot in one transaction: readers see either the previous
    /// snapshot or the new one, never a mixture. Called only after a complete, validated fetch.
    fn replace_catalog_snapshot(&self, snapshot: &CatalogSnapshot) -> Result<(), StorageError>;

    /// The last complete snapshot, or `None` when PC ports was never opened. The snapshot holds the
    /// upstream entries before the overlay is merged.
    fn catalog_snapshot(&self) -> Result<Option<CatalogSnapshot>, StorageError>;

    /// The newest valid overlay and the ETag its fetch used, or `None` when only the bundled copy
    /// exists.
    fn overlay(&self) -> Result<Option<PortOverlayRow>, StorageError>;

    /// Stores an overlay that already parsed and validated, replacing the previous one. A copy that
    /// fails validation never reaches this method.
    fn save_overlay(&self, overlay: &PortOverlayRow) -> Result<(), StorageError>;

    /// Every install job, newest first. Callers use this for the badge, the history and startup
    /// recovery.
    fn jobs(&self) -> Result<Vec<PortJobRow>, StorageError>;

    fn job(&self, id: &str) -> Result<Option<PortJobRow>, StorageError>;

    /// Inserts or updates a job row. `created_paths` is not written here: it only grows through
    /// [`PortStore::record_created_path`], so a stale in-memory copy cannot erase the journal.
    fn save_job(&self, job: &PortJobRow) -> Result<(), StorageError>;

    /// Stores the resolved plan and matcher evidence the runner needs to finish the job after a
    /// restart, notably when it waits for the user to pick an executable. Written once by
    /// [`crate::ports::job`]-driven callers before the job starts; [`PortStore::save_job`] never
    /// rewrites it, so a later phase change keeps it.
    fn save_job_resume(&self, job_id: &str, resume: &PortJobResume) -> Result<(), StorageError>;

    /// The stored resume of a job, or `None` when it has none or it cannot be read.
    fn job_resume(&self, job_id: &str) -> Result<Option<PortJobResume>, StorageError>;

    /// Appends one absolute path to the job's journal. Must be committed before the job writes that
    /// path. Recording the same path twice is allowed and keeps one entry.
    fn record_created_path(&self, job_id: &str, path: &str) -> Result<(), StorageError>;

    /// The paths recorded for a job, in the order they were journaled.
    fn created_paths(&self, job_id: &str) -> Result<Vec<String>, StorageError>;

    /// The commit point of an install: saves the DRM-free registration, marks the job
    /// [`PortJobPhase::Completed`], records the registration id and stores the provenance row in one
    /// transaction. On failure nothing is written, so recovery sees the job as unfinished.
    ///
    /// `job` must already carry `Completed` and `registration_id = Some(registration.id)`.
    fn commit_install(
        &self,
        job: &PortJobRow,
        registration: &Registration,
        installation: &PortInstallation,
    ) -> Result<(), StorageError>;

    /// Provenance rows, including those whose registration the user removed (design D10). Callers
    /// match them against the live registrations to decide whether a port is installed.
    fn installations(&self) -> Result<Vec<PortInstallation>, StorageError>;

    /// Inserts or replaces the provenance row of its job. Rows are never removed when a registration
    /// is: that is what makes "not installed, files still on disk" work (design D10).
    fn save_installation(&self, installation: &PortInstallation) -> Result<(), StorageError>;

    /// Points the provenance row of `job_id` at a new registration, for a port the user added again
    /// after removing its entry. The files and the installed version are unchanged (design D10).
    fn relink_installation(&self, job_id: &str, registration_id: &str) -> Result<(), StorageError>;

    /// The remembered catalog view, filters and last install parent folder.
    fn preferences(&self) -> Result<PortPreferences, StorageError>;

    fn save_preferences(&self, preferences: &PortPreferences) -> Result<(), StorageError>;
}

/// The newest valid overlay and the ETag it was fetched with.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortOverlayRow {
    /// The overlay's `schema_version`, recorded so an incompatible row is rejected before parsing.
    pub version: u32,
    pub etag: Option<String>,
    /// The overlay document text, as authored in `ports/overlay.toml`.
    pub payload: String,
}

/// One install job. It survives restarts so recovery can finish or clean up after a crash.
///
/// The implementation may normalize this row across the `port_install_jobs` columns (for example a
/// coarse state beside the detailed [`PortJobPhase`]); every field here is needed by the worker,
/// startup recovery or the PC ports view.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortJobRow {
    pub id: String,
    pub catalog_id: String,
    pub slug: String,
    /// The catalog display name (`Bloodborne`), so history and failure toasts do not depend on the
    /// entry still being in the snapshot.
    pub port_name: String,
    /// The release being installed, once it is resolved.
    pub release_version: Option<String>,
    /// The install folder, once the plan resolved it.
    pub install_folder: Option<String>,
    pub phase: PortJobPhase,
    /// Absolute paths journaled before the job wrote them. Filled by the store; see
    /// [`PortStore::record_created_path`].
    pub created_paths: Vec<String>,
    /// Set once the registration is committed.
    pub registration_id: Option<String>,
    /// The failure or interruption message for a job that did not complete.
    pub error: Option<String>,
    /// The capped output of a failed extraction tool, for the failure details view.
    pub failure_output: Option<String>,
    /// True once the user acknowledged the job's result; running jobs are never counted as unseen.
    pub seen: bool,
    /// Unix milliseconds.
    pub created_at: i64,
    /// Unix milliseconds.
    pub updated_at: i64,
    /// Unix milliseconds, once the job reached a terminal phase.
    pub finished_at: Option<i64>,
}

/// What the install runner needs to finish a job after a restart: the plan it resolved before the
/// job started, and the evidence a completed install gives the local matcher. Kept in the store
/// beside the job row so a job that waits for the user's executable pick survives a restart.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortJobResume {
    pub plan: PortInstallPlan,
    pub match_input: PortMatchInput,
}

/// Where a port installation came from. Kept beside the registration, never inside it (design D10).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortInstallation {
    /// The DRM-free registration that launched the port. Rows whose registration was removed stay,
    /// so PC ports can say "not installed" without forgetting the install.
    pub registration_id: String,
    pub catalog_id: String,
    pub slug: String,
    /// The catalog display name at install time.
    pub port_name: String,
    pub installed_version: String,
    pub install_folder: String,
    /// Environment variables from `env` slots, applied at launch (design D7).
    pub environment: BTreeMap<String, String>,
    /// The job that installed it; unique per installation, unlike the catalog id.
    pub job_id: String,
}

/// How the catalog is displayed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PortViewMode {
    #[default]
    Grid,
    List,
}

/// Remembered catalog view, filters and last install parent folder. The search text is not stored.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortPreferences {
    pub view: PortViewMode,
    /// Console names selected in the filter, such as `PlayStation 4`; empty means every console.
    pub consoles: Vec<String>,
    /// Only entries with guided setup.
    pub guided: bool,
    /// Only entries with a verified release.
    pub verified: bool,
    /// Only entries installed by GameNode.
    pub installed: bool,
    /// The parent folder offered first by the next install.
    pub last_parent: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::job::SuppliedStep;

    #[test]
    fn job_rows_and_phases_round_trip_through_json() {
        let job = PortJobRow {
            id: "job-1".into(),
            catalog_id: "qcat_x".into(),
            slug: "bloodborne".into(),
            port_name: "Bloodborne".into(),
            release_version: Some("v0.9.38".into()),
            install_folder: Some(r"D:\Ports\Bloodborne".into()),
            phase: PortJobPhase::SuppliedFiles {
                slot: "game".into(),
                step: SuppliedStep::Copying,
            },
            created_paths: vec![r"D:\Ports\Bloodborne".into()],
            registration_id: None,
            error: None,
            failure_output: None,
            seen: false,
            created_at: 1,
            updated_at: 2,
            finished_at: None,
        };
        let json = serde_json::to_string(&job).expect("serializable");
        assert!(json.contains("\"portName\""));
        assert!(json.contains("\"suppliedFiles\""));
        assert!(json.contains("\"copying\""));
        assert_eq!(
            serde_json::from_str::<PortJobRow>(&json).expect("deserializable"),
            job
        );
    }

    #[test]
    fn preferences_default_to_the_grid_with_no_filters() {
        let preferences = PortPreferences::default();
        assert_eq!(preferences.view, PortViewMode::Grid);
        assert!(preferences.consoles.is_empty());
        assert!(!preferences.guided);
        assert!(!preferences.verified);
        assert!(!preferences.installed);
        assert_eq!(preferences.last_parent, None);
        assert_eq!(
            serde_json::to_string(&preferences).unwrap(),
            r#"{"view":"grid","consoles":[],"guided":false,"verified":false,"installed":false,"lastParent":null}"#
        );
    }
}
