//! SQLite implementation of [`PortStore`] on the shared application connection.
//!
//! Three database rules carry the crash-safety design (D5, D10):
//!
//! - a catalog snapshot is replaced by one upsert inside one transaction, so a failed refresh
//!   leaves the previous snapshot readable;
//! - [`PortStore::record_created_path`] commits each path before the install worker writes it, so
//!   startup recovery can delete partial work;
//! - [`PortStore::commit_install`] writes the DRM-free registration, the completed job and the
//!   provenance row in one transaction, which is what makes the registration the commit point.
//!
//! Nothing here takes an account id: the port state belongs to the Windows user and is shared
//! across GameNode accounts. Nothing here is a credential.
//!
//! `port_install_jobs` stores the coarse lifecycle `state` beside the detailed `phase` as JSON, so
//! a job survives a restart exactly as the worker left it. `created_paths` only grows through
//! `record_created_path`: `save_job` never rewrites the journal, so a stale in-memory row cannot
//! erase paths that still need cleanup.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use gamenode_core::drm_free::Registration;
use gamenode_core::ports::catalog::CatalogSnapshot;
use gamenode_core::ports::job::PortJobPhase;
use gamenode_core::ports::{
    PortInstallation, PortJobResume, PortJobRow, PortOverlayRow, PortPreferences, PortStore,
    PortViewMode, StorageError,
};
use rusqlite::{Connection, OptionalExtension, params};

/// Port state backed by the application database.
///
/// The connection is shared with the other repositories (see
/// [`SqliteSettingsStore::ports_store`](crate::db::SqliteSettingsStore::ports_store)), so a port
/// write never interleaves with a library or settings write.
pub struct SqlitePortStore {
    conn: Arc<Mutex<Connection>>,
}

fn failed(error: impl std::fmt::Display) -> StorageError {
    StorageError::Failed(error.to_string())
}

/// The coarse lifecycle state stored beside the detailed phase.
fn lifecycle_state(phase: &PortJobPhase) -> &'static str {
    match phase {
        PortJobPhase::Queued => "queued",
        PortJobPhase::Cancelling => "cancelling",
        PortJobPhase::Completed => "completed",
        PortJobPhase::Cancelled => "cancelled",
        PortJobPhase::Failed { .. } => "failed",
        PortJobPhase::Interrupted { .. } => "interrupted",
        PortJobPhase::Resolving
        | PortJobPhase::Downloading
        | PortJobPhase::Verifying
        | PortJobPhase::Extracting
        | PortJobPhase::SuppliedFiles { .. }
        | PortJobPhase::Finalizing
        | PortJobPhase::AwaitingExecutable
        | PortJobPhase::Registering => "running",
    }
}

/// Reads the detailed phase, falling back to the coarse state when the stored JSON is unreadable.
///
/// An unreadable unfinished job becomes [`PortJobPhase::Interrupted`], which is the safe reading:
/// recovery removes its partial work and keeps its downloads, and a committed registration is
/// still recognized by its `registration_id`.
fn decode_phase(state: &str, payload: &str, reason: Option<&str>) -> PortJobPhase {
    if let Ok(phase) = serde_json::from_str::<PortJobPhase>(payload) {
        return phase;
    }
    tracing::warn!(state, "ignored an unreadable port job phase");
    let reason = reason
        .unwrap_or("the saved job phase could not be read")
        .to_string();
    match state {
        "queued" => PortJobPhase::Queued,
        "cancelling" => PortJobPhase::Cancelling,
        "completed" => PortJobPhase::Completed,
        "cancelled" => PortJobPhase::Cancelled,
        "failed" => PortJobPhase::Failed { reason },
        _ => PortJobPhase::Interrupted { reason },
    }
}

/// Reads the journal. An unreadable list is reported and treated as empty: the job's install
/// folder and staging folder are still known to recovery.
fn decode_created_paths(job_id: &str, payload: &str) -> Vec<String> {
    serde_json::from_str(payload).unwrap_or_else(|error| {
        tracing::warn!(%error, job_id, "ignored an unreadable port job path journal");
        Vec::new()
    })
}

/// A terminal job always carries the time it finished; callers may leave it unset.
fn finished_at(job: &PortJobRow) -> Option<i64> {
    if job.phase.is_terminal() {
        job.finished_at.or(Some(job.updated_at))
    } else {
        None
    }
}

fn write_registration(
    tx: &rusqlite::Transaction<'_>,
    value: &Registration,
) -> Result<(), StorageError> {
    let payload = serde_json::to_string(value).map_err(failed)?;
    tx.execute(
        "INSERT INTO manual_registrations (id, file_identity, removed, payload)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET
            file_identity = excluded.file_identity,
            removed = excluded.removed,
            payload = excluded.payload",
        params![value.id, value.file_identity, value.removed, payload],
    )
    .map(drop)
    .map_err(failed)
}

/// Writes one job row. `created_paths` is intentionally absent: it only grows through
/// [`PortStore::record_created_path`], so a stale in-memory row cannot erase the journal.
fn write_job(tx: &rusqlite::Transaction<'_>, job: &PortJobRow) -> Result<(), StorageError> {
    let phase = serde_json::to_string(&job.phase).map_err(failed)?;
    tx.execute(
        "INSERT INTO port_install_jobs
            (id, catalog_id, slug, port_name, release_version, state, phase, install_folder,
             registration_id, result_seen, failure_reason, failure_output, created_at,
             updated_at, finished_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         ON CONFLICT(id) DO UPDATE SET
            catalog_id = excluded.catalog_id,
            slug = excluded.slug,
            port_name = excluded.port_name,
            release_version = excluded.release_version,
            state = excluded.state,
            phase = excluded.phase,
            install_folder = excluded.install_folder,
            registration_id = excluded.registration_id,
            result_seen = excluded.result_seen,
            failure_reason = excluded.failure_reason,
            failure_output = excluded.failure_output,
            updated_at = excluded.updated_at,
            finished_at = excluded.finished_at",
        params![
            job.id,
            job.catalog_id,
            job.slug,
            job.port_name,
            job.release_version,
            lifecycle_state(&job.phase),
            phase,
            job.install_folder,
            job.registration_id,
            job.seen,
            job.error,
            job.failure_output,
            job.created_at,
            job.updated_at,
            finished_at(job),
        ],
    )
    .map(drop)
    .map_err(failed)
}

/// Writes one provenance row. One row per job: re-saving the same job replaces its row, and a
/// relink moves the row to the new registration.
fn write_installation(
    tx: &rusqlite::Transaction<'_>,
    installation: &PortInstallation,
) -> Result<(), StorageError> {
    let environment = serde_json::to_string(&installation.environment).map_err(failed)?;
    tx.execute(
        "INSERT INTO port_installations
            (registration_id, catalog_id, slug, port_name, installed_version, install_folder,
             environment_json, job_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(job_id) DO UPDATE SET
            registration_id = excluded.registration_id,
            catalog_id = excluded.catalog_id,
            slug = excluded.slug,
            port_name = excluded.port_name,
            installed_version = excluded.installed_version,
            install_folder = excluded.install_folder,
            environment_json = excluded.environment_json",
        params![
            installation.registration_id,
            installation.catalog_id,
            installation.slug,
            installation.port_name,
            installation.installed_version,
            installation.install_folder,
            environment,
            installation.job_id,
        ],
    )
    .map(drop)
    .map_err(failed)
}

impl SqlitePortStore {
    pub(crate) fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        crate::db::lock(&self.conn)
    }
}

impl PortStore for SqlitePortStore {
    fn replace_catalog_snapshot(&self, snapshot: &CatalogSnapshot) -> Result<(), StorageError> {
        let payload = serde_json::to_string(snapshot).map_err(failed)?;
        let entry_count = i64::try_from(snapshot.entry_count()).map_err(failed)?;
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        tx.execute(
            "INSERT INTO port_catalog_snapshot (id, payload, fetched_at, entry_count)
             VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET
                payload = excluded.payload,
                fetched_at = excluded.fetched_at,
                entry_count = excluded.entry_count",
            params![payload, snapshot.fetched_at, entry_count],
        )
        .map(drop)
        .map_err(failed)?;
        tx.commit().map_err(failed)
    }

    fn catalog_snapshot(&self) -> Result<Option<CatalogSnapshot>, StorageError> {
        let payload: Option<String> = self
            .conn()
            .query_row(
                "SELECT payload FROM port_catalog_snapshot WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(failed)?;
        let Some(payload) = payload else {
            return Ok(None);
        };
        match serde_json::from_str(&payload) {
            Ok(snapshot) => Ok(Some(snapshot)),
            Err(error) => {
                // A damaged cache reads as "never fetched", so PC ports offers Retry instead of
                // failing the whole snapshot.
                tracing::warn!(%error, "ignored an unreadable cached port catalog");
                Ok(None)
            }
        }
    }

    fn overlay(&self) -> Result<Option<PortOverlayRow>, StorageError> {
        self.conn()
            .query_row(
                "SELECT version, etag, payload FROM port_overlay WHERE id = 1",
                [],
                |row| {
                    Ok(PortOverlayRow {
                        version: row.get::<_, i64>(0)?.try_into().unwrap_or(0),
                        etag: row.get(1)?,
                        payload: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(failed)
    }

    fn save_overlay(&self, overlay: &PortOverlayRow) -> Result<(), StorageError> {
        self.conn()
            .execute(
                "INSERT INTO port_overlay (id, version, etag, payload) VALUES (1, ?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET
                    version = excluded.version,
                    etag = excluded.etag,
                    payload = excluded.payload",
                params![i64::from(overlay.version), overlay.etag, overlay.payload],
            )
            .map(drop)
            .map_err(failed)
    }

    fn jobs(&self) -> Result<Vec<PortJobRow>, StorageError> {
        self.conn()
            .prepare(
                "SELECT id, catalog_id, slug, port_name, release_version, install_folder, state,
                        phase, created_paths, registration_id, failure_reason, failure_output,
                        result_seen, created_at, updated_at, finished_at
                 FROM port_install_jobs ORDER BY created_at DESC, id",
            )
            .map_err(failed)?
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let state: String = row.get(6)?;
                let phase: String = row.get(7)?;
                let reason: Option<String> = row.get(10)?;
                let created_paths: String = row.get(8)?;
                Ok(PortJobRow {
                    port_name: row.get(3)?,
                    release_version: row.get(4)?,
                    install_folder: row.get(5)?,
                    phase: decode_phase(&state, &phase, reason.as_deref()),
                    created_paths: decode_created_paths(&id, &created_paths),
                    registration_id: row.get(9)?,
                    error: reason,
                    failure_output: row.get(11)?,
                    seen: row.get(12)?,
                    created_at: row.get(13)?,
                    updated_at: row.get(14)?,
                    finished_at: row.get(15)?,
                    id,
                    catalog_id: row.get(1)?,
                    slug: row.get(2)?,
                })
            })
            .map_err(failed)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(failed)
    }

    fn job(&self, id: &str) -> Result<Option<PortJobRow>, StorageError> {
        self.conn()
            .query_row(
                "SELECT id, catalog_id, slug, port_name, release_version, install_folder, state,
                        phase, created_paths, registration_id, failure_reason, failure_output,
                        result_seen, created_at, updated_at, finished_at
                 FROM port_install_jobs WHERE id = ?1",
                [id],
                |row| {
                    let id: String = row.get(0)?;
                    let state: String = row.get(6)?;
                    let phase: String = row.get(7)?;
                    let reason: Option<String> = row.get(10)?;
                    let created_paths: String = row.get(8)?;
                    Ok(PortJobRow {
                        port_name: row.get(3)?,
                        release_version: row.get(4)?,
                        install_folder: row.get(5)?,
                        phase: decode_phase(&state, &phase, reason.as_deref()),
                        created_paths: decode_created_paths(&id, &created_paths),
                        registration_id: row.get(9)?,
                        error: reason,
                        failure_output: row.get(11)?,
                        seen: row.get(12)?,
                        created_at: row.get(13)?,
                        updated_at: row.get(14)?,
                        finished_at: row.get(15)?,
                        id,
                        catalog_id: row.get(1)?,
                        slug: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(failed)
    }

    fn save_job(&self, job: &PortJobRow) -> Result<(), StorageError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        write_job(&tx, job)?;
        tx.commit().map_err(failed)
    }

    fn save_job_resume(&self, job_id: &str, resume: &PortJobResume) -> Result<(), StorageError> {
        let plan = serde_json::to_string(resume).map_err(failed)?;
        let release = serde_json::to_string(&resume.plan.release).map_err(failed)?;
        let changed = self
            .conn()
            .execute(
                "UPDATE port_install_jobs SET plan_json = ?2, release_json = ?3 WHERE id = ?1",
                params![job_id, plan, release],
            )
            .map_err(failed)?;
        if changed == 0 {
            return Err(StorageError::Failed(format!(
                "unknown port install job {job_id:?}"
            )));
        }
        Ok(())
    }

    fn job_resume(&self, job_id: &str) -> Result<Option<PortJobResume>, StorageError> {
        let stored: Option<Option<String>> = self
            .conn()
            .query_row(
                "SELECT plan_json FROM port_install_jobs WHERE id = ?1",
                [job_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(failed)?;
        let Some(Some(payload)) = stored else {
            return Ok(None);
        };
        match serde_json::from_str(&payload) {
            Ok(resume) => Ok(Some(resume)),
            Err(error) => {
                // A damaged plan reads as absent: the awaiting job stays visible and the user is
                // asked to install again instead of PC ports breaking on it.
                tracing::warn!(%error, job_id, "ignored an unreadable port job plan");
                Ok(None)
            }
        }
    }

    fn record_created_path(&self, job_id: &str, path: &str) -> Result<(), StorageError> {
        if path.trim().is_empty() {
            return Err(StorageError::Failed(
                "a port install job path cannot be empty".into(),
            ));
        }
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        let stored: Option<String> = tx
            .query_row(
                "SELECT created_paths FROM port_install_jobs WHERE id = ?1",
                [job_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(failed)?;
        let Some(stored) = stored else {
            return Err(StorageError::Failed(format!(
                "unknown port install job {job_id:?}"
            )));
        };
        let mut paths = decode_created_paths(job_id, &stored);
        // Recording the same path twice keeps one entry.
        if !paths.iter().any(|known| known == path) {
            paths.push(path.to_string());
            let journal = serde_json::to_string(&paths).map_err(failed)?;
            tx.execute(
                "UPDATE port_install_jobs SET created_paths = ?2 WHERE id = ?1",
                params![job_id, journal],
            )
            .map(drop)
            .map_err(failed)?;
        }
        tx.commit().map_err(failed)
    }

    fn created_paths(&self, job_id: &str) -> Result<Vec<String>, StorageError> {
        let stored: Option<String> = self
            .conn()
            .query_row(
                "SELECT created_paths FROM port_install_jobs WHERE id = ?1",
                [job_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(failed)?;
        match stored {
            Some(stored) => Ok(decode_created_paths(job_id, &stored)),
            None => Err(StorageError::Failed(format!(
                "unknown port install job {job_id:?}"
            ))),
        }
    }

    fn commit_install(
        &self,
        job: &PortJobRow,
        registration: &Registration,
        installation: &PortInstallation,
    ) -> Result<(), StorageError> {
        if job.phase != PortJobPhase::Completed {
            return Err(StorageError::Failed(
                "only a completed job can commit an install".into(),
            ));
        }
        if job.registration_id.as_deref() != Some(registration.id.as_str()) {
            return Err(StorageError::Failed(
                "the completed job must name the registration it commits".into(),
            ));
        }
        if installation.registration_id != registration.id || installation.job_id != job.id {
            return Err(StorageError::Failed(
                "the provenance row must belong to the committed job and registration".into(),
            ));
        }

        let registration = crate::local_game_db::registration_paths(registration.clone());
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        write_registration(&tx, &registration)?;
        write_job(&tx, job)?;
        write_installation(&tx, installation)?;
        // One commit: either the entry, the completed job and the provenance all exist, or
        // startup recovery still sees an unfinished job and removes its partial work.
        tx.commit().map_err(failed)
    }

    fn installations(&self) -> Result<Vec<PortInstallation>, StorageError> {
        self.conn()
            .prepare(
                "SELECT registration_id, catalog_id, slug, port_name, installed_version,
                        install_folder, environment_json, job_id
                 FROM port_installations ORDER BY job_id",
            )
            .map_err(failed)?
            .query_map([], |row| {
                let job_id: String = row.get(7)?;
                let environment: String = row.get(6)?;
                let environment: BTreeMap<String, String> = serde_json::from_str(&environment)
                    .unwrap_or_else(|error| {
                        tracing::warn!(%error, %job_id, "ignored an unreadable port environment");
                        BTreeMap::new()
                    });
                Ok(PortInstallation {
                    registration_id: row.get(0)?,
                    catalog_id: row.get(1)?,
                    slug: row.get(2)?,
                    port_name: row.get(3)?,
                    installed_version: row.get(4)?,
                    install_folder: row.get(5)?,
                    environment,
                    job_id,
                })
            })
            .map_err(failed)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(failed)
    }

    fn save_installation(&self, installation: &PortInstallation) -> Result<(), StorageError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        write_installation(&tx, installation)?;
        tx.commit().map_err(failed)
    }

    fn relink_installation(&self, job_id: &str, registration_id: &str) -> Result<(), StorageError> {
        if registration_id.trim().is_empty() {
            return Err(StorageError::Failed(
                "an installation cannot be linked to an empty registration".into(),
            ));
        }
        let changed = self
            .conn()
            .execute(
                "UPDATE port_installations SET registration_id = ?2 WHERE job_id = ?1",
                params![job_id, registration_id],
            )
            .map_err(failed)?;
        if changed == 0 {
            return Err(StorageError::Failed(format!(
                "no port installation for job {job_id:?}"
            )));
        }
        Ok(())
    }

    fn preferences(&self) -> Result<PortPreferences, StorageError> {
        let row: Option<(String, String, Option<String>)> = self
            .conn()
            .query_row(
                "SELECT view_mode, filters_json, install_parent FROM port_preferences WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(failed)?;
        let Some((view, filters, last_parent)) = row else {
            return Ok(PortPreferences::default());
        };
        let view = match view.as_str() {
            "list" => PortViewMode::List,
            "grid" => PortViewMode::Grid,
            other => {
                tracing::warn!(view = other, "ignored an unknown port view mode");
                PortViewMode::Grid
            }
        };
        // An unreadable filter row falls back to no filters instead of blocking PC ports.
        let filters: StoredFilters = serde_json::from_str(&filters).unwrap_or_else(|error| {
            tracing::warn!(%error, "ignored unreadable port catalog filters");
            StoredFilters::default()
        });
        let consoles = if filters.consoles.is_empty() {
            // Rows written before multi-console filters stored one id.
            filters.console.into_iter().collect()
        } else {
            filters.consoles
        };
        Ok(PortPreferences {
            view,
            consoles,
            guided: filters.guided,
            verified: filters.verified,
            installed: filters.installed,
            last_parent,
        })
    }

    fn save_preferences(&self, preferences: &PortPreferences) -> Result<(), StorageError> {
        let filters = StoredFilters {
            console: None,
            consoles: preferences.consoles.clone(),
            guided: preferences.guided,
            verified: preferences.verified,
            installed: preferences.installed,
        };
        let filters = serde_json::to_string(&filters).map_err(failed)?;
        let view = match preferences.view {
            PortViewMode::Grid => "grid",
            PortViewMode::List => "list",
        };
        self.conn()
            .execute(
                "INSERT INTO port_preferences (id, view_mode, filters_json, install_parent)
                 VALUES (1, ?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET
                    view_mode = excluded.view_mode,
                    filters_json = excluded.filters_json,
                    install_parent = excluded.install_parent,
                    updated_at = unixepoch()",
                params![view, filters, preferences.last_parent],
            )
            .map(drop)
            .map_err(failed)
    }
}

/// The filter half of [`PortPreferences`], stored in `port_preferences.filters_json`. The search
/// text is deliberately absent (never persisted).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct StoredFilters {
    /// Written by older builds; read for backwards compatibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    console: Option<String>,
    consoles: Vec<String>,
    guided: bool,
    verified: bool,
    installed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use gamenode_core::drm_free::LocalGameStore;
    use gamenode_core::ports::catalog::CatalogEntry;
    use gamenode_core::ports::job::SuppliedStep;
    use gamenode_core::ports::matching::PortMatchInput;
    use gamenode_core::ports::plan::{
        PlannedAsset, PlannedRelease, PortInstallPlan, RegistrationTemplate,
    };
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    fn db_path(dir: &TempDir) -> PathBuf {
        dir.path().join("ports.sqlite3")
    }

    fn open(path: &Path) -> crate::db::SqliteSettingsStore {
        crate::db::SqliteSettingsStore::open(path).unwrap()
    }

    fn snapshot(fetched_at: i64, entries: usize) -> CatalogSnapshot {
        CatalogSnapshot::new(
            fetched_at,
            (0..entries)
                .map(|index| CatalogEntry {
                    catalog_id: format!("qcat_{index}"),
                    slug: format!("slug-{index}"),
                    name: format!("Game {index}"),
                    supported_os: vec!["windows".into()],
                    ..CatalogEntry::default()
                })
                .collect(),
        )
    }

    fn job(id: &str) -> PortJobRow {
        PortJobRow {
            id: id.into(),
            catalog_id: "qcat_1".into(),
            slug: "bloodborne".into(),
            port_name: "Bloodborne".into(),
            release_version: Some("v0.9.38".into()),
            install_folder: Some(r"D:\Ports\Bloodborne".into()),
            phase: PortJobPhase::Queued,
            created_paths: Vec::new(),
            registration_id: None,
            error: None,
            failure_output: None,
            seen: false,
            created_at: 100,
            updated_at: 100,
            finished_at: None,
        }
    }

    fn registration(id: &str) -> Registration {
        Registration {
            id: id.into(),
            title: "Bloodborne".into(),
            executable: r"D:\Ports\Bloodborne\Lantern Engine.exe".into(),
            file_identity: "volume:file".into(),
            root: r"D:\Ports\Bloodborne".into(),
            working_directory: r"D:\Ports\Bloodborne".into(),
            arguments: vec!["--game-dir".into(), "CUSA03173".into()],
            gameplay: vec!["Bloodborne.exe".into()],
            launchers: vec!["Lantern Engine.exe".into()],
            revision: 1,
            removed: false,
            rediscovery_allowed: false,
            transferred_from: None,
            available: true,
        }
    }

    fn installation(job_id: &str, registration_id: &str) -> PortInstallation {
        PortInstallation {
            registration_id: registration_id.into(),
            catalog_id: "qcat_1".into(),
            slug: "bloodborne".into(),
            port_name: "Bloodborne".into(),
            installed_version: "v0.9.38".into(),
            install_folder: r"D:\Ports\Bloodborne".into(),
            environment: BTreeMap::from([(
                "GAME_ISO".to_string(),
                r"D:\Dumps\game.iso".to_string(),
            )]),
            job_id: job_id.into(),
        }
    }

    /// A completed job that names the registration it commits.
    fn completed_job(id: &str, registration_id: &str) -> PortJobRow {
        PortJobRow {
            phase: PortJobPhase::Completed,
            registration_id: Some(registration_id.into()),
            updated_at: 200,
            finished_at: Some(200),
            ..job(id)
        }
    }

    fn stored_plan() -> PortInstallPlan {
        PortInstallPlan {
            catalog_id: "qcat_1".into(),
            slug: "bloodborne".into(),
            title: "Bloodborne".into(),
            release: PlannedRelease {
                release_id: "rel-1".into(),
                version: "v0.9.38".into(),
                released_at: 1,
                prerelease: false,
            },
            asset: PlannedAsset {
                id: "windows".into(),
                filename: "port.zip".into(),
                url: "https://example.invalid/port.zip".into(),
                sha256: "a".repeat(64),
                size: 10,
                format: "zip".into(),
                unverified: false,
            },
            folder_name: "Bloodborne".into(),
            parent: r"D:\Ports".into(),
            target: r"D:\Ports\Bloodborne".into(),
            staging: r"D:\Ports\.gamenode-staging-job-await".into(),
            slots: Vec::new(),
            files_to_add: vec!["portable.txt".into()],
            registration: RegistrationTemplate {
                title: "Bloodborne".into(),
                executable: None,
                working_directory: r"D:\Ports\Bloodborne".into(),
                arguments: vec!["--game-dir".into(), "CUSA03173".into()],
                gameplay: vec!["Bloodborne.exe".into()],
                launchers: Vec::new(),
            },
            environment: BTreeMap::new(),
            extractors: Vec::new(),
            space_required: 10,
            risks: Vec::new(),
        }
    }

    #[test]
    fn an_awaiting_executable_job_round_trips_with_its_stored_plan() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        let resume = PortJobResume {
            plan: stored_plan(),
            match_input: PortMatchInput::GameNodeId { game_id: 42 },
        };
        let mut awaiting = job("job-await");
        awaiting.phase = PortJobPhase::AwaitingExecutable;
        awaiting.updated_at = 150;
        {
            let store = open(&path).ports_store();
            store.save_job(&awaiting).unwrap();
            store.save_job_resume("job-await", &resume).unwrap();
            // A later phase write never erases the stored plan.
            let mut touched = awaiting.clone();
            touched.updated_at = 160;
            store.save_job(&touched).unwrap();
            assert_eq!(store.job_resume("job-await").unwrap(), Some(resume.clone()));
            assert!(store.save_job_resume("missing", &resume).is_err());
            assert_eq!(store.job_resume("missing").unwrap(), None);
        }

        // A restart keeps the non-terminal phase and the plan intact.
        let store = open(&path).ports_store();
        let reopened = store.job("job-await").unwrap().unwrap();
        assert_eq!(reopened.phase, PortJobPhase::AwaitingExecutable);
        assert!(!reopened.phase.is_terminal());
        assert_eq!(reopened.finished_at, None);
        assert_eq!(store.job_resume("job-await").unwrap(), Some(resume));
        assert_eq!(
            store.jobs().unwrap()[0].phase,
            PortJobPhase::AwaitingExecutable
        );

        // A damaged plan reads as absent instead of breaking the view or recovery.
        crate::db::lock(&store.conn)
            .execute(
                "UPDATE port_install_jobs SET plan_json = 'not json' WHERE id = 'job-await'",
                [],
            )
            .unwrap();
        assert_eq!(store.job_resume("job-await").unwrap(), None);
    }

    #[test]
    fn snapshot_and_overlay_survive_a_restart_without_network() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        let saved = snapshot(1_700_000_000_000, 3);
        let overlay = PortOverlayRow {
            version: 1,
            etag: Some("\"overlay-1\"".into()),
            payload: "schema_version = 1\n".into(),
        };
        {
            let store = open(&path).ports_store();
            store.replace_catalog_snapshot(&saved).unwrap();
            store.save_overlay(&overlay).unwrap();
        }

        let store = open(&path).ports_store();
        assert_eq!(store.catalog_snapshot().unwrap(), Some(saved));
        assert_eq!(store.overlay().unwrap(), Some(overlay));
    }

    #[test]
    fn a_failed_snapshot_replace_keeps_the_previous_one() {
        let dir = TempDir::new().unwrap();
        let store = open(&db_path(&dir)).ports_store();
        let previous = snapshot(1_000, 2);
        store.replace_catalog_snapshot(&previous).unwrap();

        // Reject the next write the way a full or damaged database would.
        crate::db::lock(&store.conn)
            .execute_batch(
                "CREATE TRIGGER reject_snapshot BEFORE UPDATE ON port_catalog_snapshot
                 BEGIN SELECT RAISE(ABORT, 'snapshot write refused'); END;",
            )
            .unwrap();
        let err = store
            .replace_catalog_snapshot(&snapshot(2_000, 5))
            .unwrap_err();
        assert!(matches!(err, StorageError::Failed(_)));

        assert_eq!(store.catalog_snapshot().unwrap(), Some(previous));
    }

    #[test]
    fn port_state_is_device_level_and_survives_an_account_switch() {
        let dir = TempDir::new().unwrap();
        let settings = open(&db_path(&dir));
        let store = settings.ports_store();
        store.replace_catalog_snapshot(&snapshot(1_000, 1)).unwrap();
        store
            .save_preferences(&PortPreferences {
                view: PortViewMode::List,
                last_parent: Some(r"D:\Ports".into()),
                ..PortPreferences::default()
            })
            .unwrap();
        store.save_job(&job("job-1")).unwrap();
        store
            .commit_install(
                &completed_job("job-2", "reg-1"),
                &registration("reg-1"),
                &installation("job-2", "reg-1"),
            )
            .unwrap();

        // Sign out, then a different account signs in under the same Windows user.
        {
            let conn = crate::db::lock(&store.conn);
            conn.execute(
                "DELETE FROM settings WHERE key IN ('account.last_id', 'account.last_name')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO settings (key, value) VALUES
                    ('account.last_id', 'someone-else'), ('account.last_name', 'Bia')",
                [],
            )
            .unwrap();
        }

        assert!(store.catalog_snapshot().unwrap().is_some());
        assert_eq!(
            store.preferences().unwrap().last_parent,
            Some(r"D:\Ports".into())
        );
        assert_eq!(store.jobs().unwrap().len(), 2);
        assert_eq!(store.installations().unwrap().len(), 1);

        // No port table is account scoped in the first place.
        let conn = crate::db::lock(&store.conn);
        for table in [
            "port_catalog_snapshot",
            "port_overlay",
            "port_install_jobs",
            "port_installations",
            "port_preferences",
        ] {
            let columns: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name LIKE '%account%'",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(columns, 0, "{table} must not be account scoped");
        }
    }

    #[test]
    fn created_paths_are_journaled_in_order_and_never_lost() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        let store = open(&path).ports_store();
        store.save_job(&job("job-1")).unwrap();

        store
            .record_created_path("job-1", r"D:\Ports\Bloodborne")
            .unwrap();
        store
            .record_created_path("job-1", r"D:\Ports\Bloodborne\CUSA03173")
            .unwrap();
        // Recording the same path twice keeps one entry.
        store
            .record_created_path("job-1", r"D:\Ports\Bloodborne")
            .unwrap();
        assert_eq!(
            store.created_paths("job-1").unwrap(),
            vec![
                r"D:\Ports\Bloodborne".to_string(),
                r"D:\Ports\Bloodborne\CUSA03173".to_string()
            ]
        );

        // A stale in-memory row cannot erase the journal.
        let mut running = job("job-1");
        running.phase = PortJobPhase::SuppliedFiles {
            slot: "game".into(),
            step: SuppliedStep::Copying,
        };
        running.created_paths = Vec::new();
        running.updated_at = 150;
        store.save_job(&running).unwrap();
        assert_eq!(store.created_paths("job-1").unwrap().len(), 2);

        // Each path is committed before the write it covers: reopening (a crash) keeps them.
        drop(store);
        let store = open(&path).ports_store();
        store
            .record_created_path("job-1", r"C:\Users\me\AppData\Roaming\GameNode\port.ini")
            .unwrap();
        let reopened = store.job("job-1").unwrap().unwrap();
        assert_eq!(reopened.created_paths.len(), 3);
        assert_eq!(reopened.phase, running.phase);

        assert!(store.record_created_path("missing", r"D:\x").is_err());
        assert!(store.created_paths("missing").is_err());
    }

    #[test]
    fn provenance_survives_removing_the_registration_and_can_be_relinked() {
        let dir = TempDir::new().unwrap();
        let store = open(&db_path(&dir)).ports_store();
        let installation = installation("job-1", "reg-1");
        store
            .commit_install(
                &completed_job("job-1", "reg-1"),
                &registration("reg-1"),
                &installation,
            )
            .unwrap();
        assert_eq!(store.installations().unwrap(), vec![installation.clone()]);

        // Removing the entry keeps the provenance row and the files it describes.
        let mut removed = registration("reg-1");
        removed.removed = true;
        crate::db::lock(&store.conn)
            .execute(
                "UPDATE manual_registrations SET removed = 1, payload = ?2 WHERE id = ?1",
                params![removed.id, serde_json::to_string(&removed).unwrap()],
            )
            .unwrap();
        assert_eq!(store.installations().unwrap(), vec![installation.clone()]);

        // Even deleting the row outright leaves it alone: there is no foreign key.
        crate::db::lock(&store.conn)
            .execute("DELETE FROM manual_registrations WHERE id = 'reg-1'", [])
            .unwrap();
        assert_eq!(store.installations().unwrap(), vec![installation.clone()]);

        // Adding the same port again links the existing provenance to the new registration.
        store.relink_installation("job-1", "reg-2").unwrap();
        let linked = store.installations().unwrap().remove(0);
        assert_eq!(linked.registration_id, "reg-2");
        assert_eq!(linked.install_folder, installation.install_folder);
        assert_eq!(linked.installed_version, installation.installed_version);
        assert_eq!(linked.environment, installation.environment);
        assert!(store.relink_installation("missing", "reg-3").is_err());
    }

    #[test]
    fn commit_writes_registration_job_and_provenance_or_nothing() {
        let dir = TempDir::new().unwrap();
        let settings = open(&db_path(&dir));
        let store = settings.ports_store();

        store
            .commit_install(
                &completed_job("job-1", "reg-1"),
                &registration("reg-1"),
                &installation("job-1", "reg-1"),
            )
            .unwrap();
        let committed = store.job("job-1").unwrap().unwrap();
        assert_eq!(committed.phase, PortJobPhase::Completed);
        assert_eq!(committed.registration_id.as_deref(), Some("reg-1"));
        assert_eq!(committed.finished_at, Some(200));
        assert_eq!(
            settings
                .local_game_store()
                .registrations()
                .unwrap()
                .iter()
                .map(|registration| registration.id.as_str())
                .collect::<Vec<_>>(),
            vec!["reg-1"]
        );

        // A second job that resolves to the same executable cannot commit: the active
        // `file_identity` is unique. The failure must leave all three writes undone.
        store
            .save_job(&{
                let mut second = job("job-2");
                second.phase = PortJobPhase::Registering;
                second.updated_at = 150;
                second
            })
            .unwrap();
        let mut second = completed_job("job-2", "reg-2");
        second.phase = PortJobPhase::Completed;
        let err = store
            .commit_install(
                &second,
                &registration("reg-2"),
                &installation("job-2", "reg-2"),
            )
            .unwrap_err();
        assert!(matches!(err, StorageError::Failed(_)));

        let second = store.job("job-2").unwrap().unwrap();
        assert_eq!(
            second.phase,
            PortJobPhase::Registering,
            "the job rolled back"
        );
        assert_eq!(second.registration_id, None);
        assert!(!second.phase.is_terminal());
        assert_eq!(
            settings
                .local_game_store()
                .registrations()
                .unwrap()
                .iter()
                .map(|registration| registration.id.as_str())
                .collect::<Vec<_>>(),
            vec!["reg-1"],
            "the second registration was not written"
        );
        let provenance: Vec<_> = store
            .installations()
            .unwrap()
            .into_iter()
            .map(|installation| installation.job_id)
            .collect();
        assert_eq!(provenance, vec!["job-1"]);
    }

    #[test]
    fn commit_refuses_a_job_that_did_not_reach_completed() {
        let dir = TempDir::new().unwrap();
        let store = open(&db_path(&dir)).ports_store();
        let mut unfinished = job("job-1");
        unfinished.registration_id = Some("reg-1".into());
        assert!(matches!(
            store.commit_install(
                &unfinished,
                &registration("reg-1"),
                &installation("job-1", "reg-1")
            ),
            Err(StorageError::Failed(_))
        ));
        assert!(store.installations().unwrap().is_empty());
    }

    #[test]
    fn jobs_round_trip_their_phases_newest_first() {
        let dir = TempDir::new().unwrap();
        let store = open(&db_path(&dir)).ports_store();

        let mut queued = job("job-queued");
        queued.created_at = 100;
        let mut supplied = job("job-supplied");
        supplied.created_at = 200;
        supplied.phase = PortJobPhase::SuppliedFiles {
            slot: "game".into(),
            step: SuppliedStep::Hashing,
        };
        let mut failed = job("job-failed");
        failed.created_at = 300;
        failed.phase = PortJobPhase::Failed {
            reason: "not enough free space".into(),
        };
        failed.error = Some("not enough free space".into());
        failed.failure_output = Some("tool output".into());
        failed.finished_at = Some(310);
        for row in [&queued, &supplied, &failed] {
            store.save_job(row).unwrap();
        }

        let jobs = store.jobs().unwrap();
        assert_eq!(
            jobs.iter().map(|job| job.id.as_str()).collect::<Vec<_>>(),
            vec!["job-failed", "job-supplied", "job-queued"]
        );
        assert_eq!(jobs[0].phase, failed.phase);
        assert_eq!(jobs[0].error.as_deref(), Some("not enough free space"));
        assert_eq!(jobs[0].failure_output.as_deref(), Some("tool output"));
        assert_eq!(jobs[0].finished_at, Some(310));
        assert_eq!(jobs[1].phase, supplied.phase);
        assert_eq!(jobs[2].phase, PortJobPhase::Queued);
        assert_eq!(jobs[0].port_name, "Bloodborne");
        assert_eq!(jobs[0].release_version.as_deref(), Some("v0.9.38"));
        assert!(store.job("missing").unwrap().is_none());

        // Marking the result seen rewrites only the row; the phase and journal stay.
        let mut seen = jobs[0].clone();
        seen.seen = true;
        store.save_job(&seen).unwrap();
        assert!(store.job("job-failed").unwrap().unwrap().seen);
    }

    #[test]
    fn preferences_default_to_the_grid_and_round_trip() {
        let dir = TempDir::new().unwrap();
        let path = db_path(&dir);
        {
            let store = open(&path).ports_store();
            assert_eq!(store.preferences().unwrap(), PortPreferences::default());

            let saved = PortPreferences {
                view: PortViewMode::List,
                consoles: vec!["PlayStation 4".into(), "PlayStation 3".into()],
                guided: true,
                verified: true,
                installed: false,
                last_parent: Some(r"D:\Ports".into()),
            };
            store.save_preferences(&saved).unwrap();
        }
        {
            let reopened = open(&path).ports_store();
            assert_eq!(
                reopened.preferences().unwrap(),
                PortPreferences {
                    view: PortViewMode::List,
                    consoles: vec!["PlayStation 4".into(), "PlayStation 3".into()],
                    guided: true,
                    verified: true,
                    installed: false,
                    last_parent: Some(r"D:\Ports".into()),
                }
            );

            // A row written by an older build with a single console id still loads.
            crate::db::lock(&reopened.conn)
                .execute(
                    r#"UPDATE port_preferences SET filters_json = '{"console":"ps4","guided":false}' WHERE id = 1"#,
                    [],
                )
                .unwrap();
            assert_eq!(
                reopened.preferences().unwrap().consoles,
                vec!["ps4".to_owned()]
            );

            // An unreadable filter row falls back to no filters without blocking PC ports.
            crate::db::lock(&reopened.conn)
                .execute(
                    "UPDATE port_preferences SET filters_json = 'not json' WHERE id = 1",
                    [],
                )
                .unwrap();
        }
        let recovered = open(&path).ports_store().preferences().unwrap();
        assert_eq!(recovered.view, PortViewMode::List);
        assert!(!recovered.guided);
        assert_eq!(recovered.last_parent, Some(r"D:\Ports".into()));
    }
}
