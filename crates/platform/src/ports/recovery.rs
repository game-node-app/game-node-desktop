//! Startup recovery for PC port installs (design D5, D6).
//!
//! The install worker journals every path before it writes it and writes the DRM-free registration
//! in the same transaction that marks the job completed. Startup recovery therefore has exactly
//! three cases for an unfinished job:
//!
//! - the job waits for the user's executable pick: its install folder is complete, so keep the
//!   files and the [`PortJobPhase::AwaitingExecutable`] phase; the runner resumes it after start;
//! - a registration was committed: mark the job completed and keep the entry, the provenance row and
//!   the files. This is the crash after the commit point but before the row was read back;
//! - no registration was committed: delete every journaled path and the staging folder, keep the
//!   verified downloads (they are cached outside the journal), and mark the job interrupted with a
//!   Retry action.
//!
//! Recovery may run before the install worker starts. Everything here performs blocking I/O and
//! SQLite work, so call it on a worker thread, never on the UI thread.

use std::fs;
use std::path::{Component, Path};

use gamenode_core::ports::{PortJobPhase, PortStore, StorageError, join_folder};

/// What one recovery pass did, for the snapshot and the logs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    /// Jobs whose registration had been committed; they were marked completed and kept.
    pub completed: Vec<String>,
    /// Jobs waiting for the user's executable pick. Their complete install folder is kept and the
    /// phase is unchanged.
    pub awaiting: Vec<String>,
    /// Jobs whose partial files were removed and which were marked interrupted.
    pub interrupted: Vec<String>,
    /// Jobs that were already terminal and were left exactly as they were.
    pub untouched: Vec<String>,
    /// Recorded paths that could not be removed, with a user-safe reason.
    pub warnings: Vec<String>,
}

impl RecoveryReport {
    /// True when recovery completed or interrupted no job. Keeping a job that waits for an
    /// executable pick is not a recovery action, so it does not affect this.
    pub fn is_empty(&self) -> bool {
        self.completed.is_empty() && self.interrupted.is_empty()
    }
}

/// Finds unfinished install jobs, resolves the commit point and removes partial work.
///
/// `registration_exists` answers whether a registration id is still present in the DRM-free store
/// (a registration the user removed counts as absent, so its job is treated as unfinished).
/// `now` is Unix milliseconds used for the terminal row's timestamps.
///
/// The verified download cache under `<data_dir>/ports/cache` is deliberately not journaled by the
/// install worker, so recovery never deletes it (spec: interrupted installs keep cached downloads).
pub fn recover_interrupted_jobs(
    store: &dyn PortStore,
    registration_exists: &dyn Fn(&str) -> Result<bool, StorageError>,
    now: i64,
) -> Result<RecoveryReport, StorageError> {
    let mut report = RecoveryReport::default();
    for job in store.jobs()? {
        if job.phase.is_terminal() {
            report.untouched.push(job.id);
            continue;
        }

        // The install folder is complete; only the user's executable pick is missing. Keep the
        // files and the phase so the runner can register the entry after the restart.
        if job.phase == PortJobPhase::AwaitingExecutable {
            report.awaiting.push(job.id);
            continue;
        }

        let committed = match job.registration_id.as_deref() {
            Some(registration_id) => registration_exists(registration_id)?,
            None => false,
        };
        if committed {
            let mut completed = job.clone();
            // This is a repair, not a pipeline replay: the registration, the provenance row and the
            // files are already in their final state.
            completed.phase = PortJobPhase::Completed;
            completed.error = None;
            completed.updated_at = now;
            completed.finished_at = Some(now);
            store.save_job(&completed)?;
            report.completed.push(job.id);
            continue;
        }

        remove_partial_work(store, &job, &mut report.warnings)?;
        let reason = "GameNode stopped before the install finished".to_string();
        let mut interrupted = job.clone();
        interrupted.phase = PortJobPhase::Interrupted {
            reason: reason.clone(),
        };
        interrupted.error = Some(reason);
        interrupted.updated_at = now;
        interrupted.finished_at = Some(now);
        store.save_job(&interrupted)?;
        report.interrupted.push(job.id);
    }
    Ok(report)
}

/// Removes the journal of an unfinished job, newest entry first, and its staging folder.
fn remove_partial_work(
    store: &dyn PortStore,
    job: &gamenode_core::ports::PortJobRow,
    warnings: &mut Vec<String>,
) -> Result<(), StorageError> {
    let paths = match store.created_paths(&job.id) {
        Ok(paths) => paths,
        // An unreadable journal is reported by the store; the staging folder below is still known.
        Err(error) => {
            warnings.push(format!(
                "{}: the journal could not be read: {error}",
                job.id
            ));
            Vec::new()
        }
    };
    for path in paths.iter().rev() {
        if let Err(reason) = remove_path(Path::new(path)) {
            warnings.push(format!("{}: {reason}", job.id));
        }
    }
    if let Some(folder) = &job.install_folder
        && let Some(staging) = staging_folder(folder, &job.id)
        && let Err(reason) = remove_path(Path::new(&staging))
    {
        warnings.push(format!("{}: {reason}", job.id));
    }
    Ok(())
}

/// The staging folder the planner creates for the job (design D5).
fn staging_folder(install_folder: &str, job_id: &str) -> Option<String> {
    let trimmed = install_folder.trim_end_matches(['/', '\\']);
    let index = trimmed.rfind(['/', '\\'])?;
    let parent = &trimmed[..index];
    if parent.is_empty() {
        return Some(format!("/.gamenode-staging-{job_id}"));
    }
    let parent = if parent.len() == 2 && parent.ends_with(':') {
        let separator = if install_folder.as_bytes().get(index) == Some(&b'\\') {
            '\\'
        } else {
            '/'
        };
        return Some(format!("{parent}{separator}.gamenode-staging-{job_id}"));
    } else {
        parent
    };
    Some(join_folder(parent, &format!(".gamenode-staging-{job_id}")))
}

/// Removes a recorded file or folder tree.
///
/// A journal is written by this application, but recovery still refuses anything that is not an
/// absolute path with at least one normal component and no `..`, so a damaged database can never
/// make the app delete outside the install.
fn remove_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!(
            "refused to remove the relative path {}",
            path.display()
        ));
    }
    let components: Vec<Component<'_>> = path.components().collect();
    if components
        .iter()
        .any(|component| matches!(component, Component::ParentDir))
        || !components
            .iter()
            .any(|component| matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "refused to remove the unsafe path {}",
            path.display()
        ));
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let result = if metadata.file_type().is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    match result {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

#[cfg(test)]
mod tests;
