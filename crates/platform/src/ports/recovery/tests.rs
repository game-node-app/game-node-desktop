use std::fs;
use std::path::{Path, PathBuf};

use gamenode_core::drm_free::{LocalGameStore, Registration};
use gamenode_core::ports::job::SuppliedStep;
use gamenode_core::ports::{PortInstallation, PortJobPhase, PortJobRow, PortStore, StorageError};
use tempfile::TempDir;

use super::*;
use crate::db::SqliteSettingsStore;

fn open(dir: &TempDir) -> (SqliteSettingsStore, std::sync::Arc<dyn PortStore>) {
    let settings = SqliteSettingsStore::open(&dir.path().join("recovery.sqlite3")).unwrap();
    let store: std::sync::Arc<dyn PortStore> = std::sync::Arc::new(settings.ports_store());
    (settings, store)
}

fn job(id: &str, phase: PortJobPhase, install_folder: &Path) -> PortJobRow {
    PortJobRow {
        id: id.into(),
        catalog_id: format!("qcat_{id}"),
        slug: format!("slug-{id}"),
        port_name: "Game".into(),
        release_version: Some("v1.0.0".into()),
        install_folder: Some(install_folder.display().to_string()),
        phase,
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

fn write(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, bytes).unwrap();
}

fn exists(store: &dyn PortStore, id: &str) -> PortJobPhase {
    store.job(id).unwrap().expect("the job row").phase
}

#[test]
fn unfinished_jobs_at_every_phase_boundary_are_cleaned_and_interrupted() {
    let dir = TempDir::new().unwrap();
    let (settings, store) = open(&dir);
    let ports = dir.path().join("Ports");
    fs::create_dir_all(&ports).unwrap();
    let cache = dir
        .path()
        .join("data/ports/cache/qcat_keep/v1.0.0/asset.zip");
    write(&cache, b"cached verified download");

    let mut recovered_ids = Vec::new();
    for (index, phase) in [
        PortJobPhase::Downloading,
        PortJobPhase::Verifying,
        PortJobPhase::Extracting,
        PortJobPhase::SuppliedFiles {
            slot: "game".into(),
            step: SuppliedStep::Copying,
        },
        PortJobPhase::Finalizing,
        PortJobPhase::Registering,
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("job-{index}");
        let target = ports.join(format!("Game-{index}"));
        let staging = ports.join(format!(".gamenode-staging-{id}"));
        let row = job(&id, phase.clone(), &target);
        store.save_job(&row).unwrap();

        match phase {
            PortJobPhase::Downloading | PortJobPhase::Verifying => {
                // Crash before staging exists: only the row is left.
            }
            PortJobPhase::Extracting => {
                store
                    .record_created_path(&id, &staging.display().to_string())
                    .unwrap();
                write(&staging.join("partial.txt"), b"partial");
            }
            PortJobPhase::SuppliedFiles { .. } => {
                store
                    .record_created_path(&id, &staging.display().to_string())
                    .unwrap();
                write(&staging.join("partial.txt"), b"partial");
                let external = dir.path().join("roaming/GameNode/game.ini");
                store
                    .record_created_path(&id, &external.display().to_string())
                    .unwrap();
                write(&external, b"copied");
            }
            PortJobPhase::Finalizing | PortJobPhase::Registering => {
                // Crash after the rename: the staging folder is gone and the target is journaled.
                store
                    .record_created_path(&id, &staging.display().to_string())
                    .unwrap();
                store
                    .record_created_path(&id, &target.display().to_string())
                    .unwrap();
                write(&target.join("Bloodborne.exe"), b"launcher");
            }
            _ => {}
        }
        recovered_ids.push(id);
    }

    let settings_local = settings.local_game_store();
    let registration_exists = |id: &str| -> Result<bool, StorageError> {
        Ok(settings_local
            .registrations()?
            .iter()
            .any(|registration| registration.id == id && !registration.removed))
    };
    let report = recover_interrupted_jobs(store.as_ref(), &registration_exists, 500).unwrap();

    assert_eq!(report.completed, Vec::<String>::new());
    assert_eq!(report.interrupted.len(), recovered_ids.len());
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    for (index, id) in recovered_ids.iter().enumerate() {
        let phase = exists(store.as_ref(), id);
        assert!(matches!(phase, PortJobPhase::Interrupted { .. }), "{phase}");
        let row = store.job(id).unwrap().unwrap();
        assert_eq!(row.finished_at, Some(500));
        assert!(row.error.is_some());
        assert!(!ports.join(format!(".gamenode-staging-{id}")).exists());
        if index >= 4 {
            assert!(
                !ports.join(format!("Game-{index}")).exists(),
                "the renamed target was removed"
            );
        }
    }
    assert!(!dir.path().join("roaming/GameNode/game.ini").exists());
    assert!(
        cache.is_file(),
        "verified cached downloads are kept across recovery"
    );

    // A second pass has nothing to interrupt and leaves the terminal rows alone.
    let again = recover_interrupted_jobs(store.as_ref(), &registration_exists, 600).unwrap();
    assert!(again.interrupted.is_empty());
    assert_eq!(again.untouched.len(), recovered_ids.len());
}

#[test]
fn a_committed_registration_keeps_the_job_files_and_provenance() {
    let dir = TempDir::new().unwrap();
    let (settings, store) = open(&dir);
    let target = dir.path().join("Ports/Bloodborne");
    let staging = dir.path().join("Ports/.gamenode-staging-job-committed");
    let mut row = job("job-committed", PortJobPhase::Registering, &target);
    row.registration_id = Some("reg-1".into());
    store.save_job(&row).unwrap();
    store
        .record_created_path("job-committed", &staging.display().to_string())
        .unwrap();
    store
        .record_created_path("job-committed", &target.display().to_string())
        .unwrap();
    write(&target.join("Bloodborne.exe"), b"launcher");

    let registration = Registration {
        id: "reg-1".into(),
        title: "Bloodborne".into(),
        executable: target.join("Bloodborne.exe").display().to_string(),
        file_identity: "1:2:3".into(),
        root: target.display().to_string(),
        working_directory: target.display().to_string(),
        arguments: Vec::new(),
        gameplay: vec!["Bloodborne.exe".into()],
        launchers: vec!["Bloodborne.exe".into()],
        revision: 1,
        removed: false,
        rediscovery_allowed: false,
        transferred_from: None,
        available: true,
    };
    settings
        .local_game_store()
        .save_registration(&registration)
        .unwrap();
    store
        .save_installation(&PortInstallation {
            registration_id: "reg-1".into(),
            catalog_id: "qcat_committed".into(),
            slug: "slug".into(),
            port_name: "Bloodborne".into(),
            installed_version: "v1.0.0".into(),
            install_folder: target.display().to_string(),
            environment: Default::default(),
            job_id: "job-committed".into(),
        })
        .unwrap();

    let local = settings.local_game_store();
    let registration_exists = |id: &str| -> Result<bool, StorageError> {
        Ok(local
            .registrations()?
            .iter()
            .any(|registration| registration.id == id && !registration.removed))
    };
    let report = recover_interrupted_jobs(store.as_ref(), &registration_exists, 500).unwrap();

    assert_eq!(report.completed, vec!["job-committed".to_string()]);
    assert!(report.interrupted.is_empty());
    let completed = store.job("job-committed").unwrap().unwrap();
    assert_eq!(completed.phase, PortJobPhase::Completed);
    assert_eq!(completed.registration_id.as_deref(), Some("reg-1"));
    assert_eq!(completed.finished_at, Some(500));
    assert!(target.join("Bloodborne.exe").is_file());
    assert!(!staging.exists());
    assert_eq!(
        settings.local_game_store().registrations().unwrap().len(),
        1
    );
    assert_eq!(store.installations().unwrap().len(), 1);
}

#[test]
fn a_removed_registration_counts_as_uncommitted() {
    let dir = TempDir::new().unwrap();
    let (settings, store) = open(&dir);
    let target = dir.path().join("Ports/Removed");
    let mut row = job("job-removed", PortJobPhase::Registering, &target);
    row.registration_id = Some("reg-removed".into());
    store.save_job(&row).unwrap();
    store
        .record_created_path("job-removed", &target.display().to_string())
        .unwrap();
    write(&target.join("Bloodborne.exe"), b"launcher");

    let registration = Registration {
        id: "reg-removed".into(),
        title: "Removed".into(),
        executable: target.join("Bloodborne.exe").display().to_string(),
        file_identity: "1:2:3".into(),
        root: target.display().to_string(),
        working_directory: target.display().to_string(),
        arguments: Vec::new(),
        gameplay: Vec::new(),
        launchers: Vec::new(),
        revision: 1,
        removed: true,
        rediscovery_allowed: false,
        transferred_from: None,
        available: true,
    };
    settings
        .local_game_store()
        .save_registration(&registration)
        .unwrap();

    let local = settings.local_game_store();
    let registration_exists = |id: &str| -> Result<bool, StorageError> {
        Ok(local
            .registrations()?
            .iter()
            .any(|registration| registration.id == id && !registration.removed))
    };
    let report = recover_interrupted_jobs(store.as_ref(), &registration_exists, 500).unwrap();

    assert!(report.completed.is_empty());
    assert_eq!(report.interrupted, vec!["job-removed".to_string()]);
    assert!(!target.exists());
}

#[test]
fn terminal_jobs_are_left_untouched() {
    let dir = TempDir::new().unwrap();
    let (settings, store) = open(&dir);
    let target = dir.path().join("Ports/Finished");
    write(&target.join("Bloodborne.exe"), b"launcher");
    let row = job("job-done", PortJobPhase::Completed, &target);
    store.save_job(&row).unwrap();

    let local = settings.local_game_store();
    let registration_exists = |id: &str| -> Result<bool, StorageError> {
        Ok(local
            .registrations()?
            .iter()
            .any(|registration| registration.id == id && !registration.removed))
    };
    let report = recover_interrupted_jobs(store.as_ref(), &registration_exists, 500).unwrap();

    assert_eq!(report.untouched, vec!["job-done".to_string()]);
    assert!(report.is_empty());
    assert_eq!(
        store.job("job-done").unwrap().unwrap().phase,
        PortJobPhase::Completed
    );
    assert!(target.join("Bloodborne.exe").is_file());
}

#[test]
fn unsafe_journal_entries_are_refused_with_a_warning() {
    let dir = TempDir::new().unwrap();
    let (settings, store) = open(&dir);
    let target = dir.path().join("Ports/Unsafe");
    let row = job("job-unsafe", PortJobPhase::Extracting, &target);
    store.save_job(&row).unwrap();
    store
        .record_created_path("job-unsafe", "relative/partial.txt")
        .unwrap();
    store
        .record_created_path("job-unsafe", "../outside.txt")
        .unwrap();

    let local = settings.local_game_store();
    let registration_exists = |id: &str| -> Result<bool, StorageError> {
        Ok(local
            .registrations()?
            .iter()
            .any(|registration| registration.id == id && !registration.removed))
    };
    let report = recover_interrupted_jobs(store.as_ref(), &registration_exists, 500).unwrap();

    assert_eq!(report.interrupted, vec!["job-unsafe".to_string()]);
    assert_eq!(report.warnings.len(), 2, "{:?}", report.warnings);
    assert!(matches!(
        exists(store.as_ref(), "job-unsafe"),
        PortJobPhase::Interrupted { .. }
    ));
}

#[test]
fn an_awaiting_executable_job_keeps_its_complete_folder_and_phase() {
    let dir = TempDir::new().unwrap();
    let (settings, store) = open(&dir);
    let target = dir.path().join("Ports/Waiting");
    let staging = dir.path().join("Ports/.gamenode-staging-job-waiting");
    let row = job("job-waiting", PortJobPhase::AwaitingExecutable, &target);
    store.save_job(&row).unwrap();
    store
        .record_created_path("job-waiting", &staging.display().to_string())
        .unwrap();
    store
        .record_created_path("job-waiting", &target.display().to_string())
        .unwrap();
    write(&target.join("picked.exe"), b"launcher");

    let local = settings.local_game_store();
    let registration_exists = |id: &str| -> Result<bool, StorageError> {
        Ok(local
            .registrations()?
            .iter()
            .any(|registration| registration.id == id && !registration.removed))
    };
    let report = recover_interrupted_jobs(store.as_ref(), &registration_exists, 500).unwrap();

    assert_eq!(report.awaiting, vec!["job-waiting".to_string()]);
    assert!(report.completed.is_empty());
    assert!(report.interrupted.is_empty());
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    let kept = store.job("job-waiting").unwrap().unwrap();
    assert_eq!(kept.phase, PortJobPhase::AwaitingExecutable);
    assert_eq!(kept.finished_at, None);
    assert!(kept.error.is_none());
    assert!(target.join("picked.exe").is_file());
    assert!(!staging.exists());
    assert!(store.installations().unwrap().is_empty());
    assert!(
        settings
            .local_game_store()
            .registrations()
            .unwrap()
            .is_empty()
    );

    // A second pass reports the job again without changing it.
    let again = recover_interrupted_jobs(store.as_ref(), &registration_exists, 600).unwrap();
    assert_eq!(again.awaiting, vec!["job-waiting".to_string()]);
    assert_eq!(
        store.job("job-waiting").unwrap().unwrap().phase,
        PortJobPhase::AwaitingExecutable
    );
    assert!(target.join("picked.exe").is_file());
}

#[test]
fn the_staging_folder_name_is_derived_for_a_drive_root_and_a_unix_root() {
    assert_eq!(
        staging_folder(r"D:\Ports\Bloodborne", "job-1"),
        Some(r"D:\Ports\.gamenode-staging-job-1".into())
    );
    assert_eq!(
        staging_folder(r"D:\Bloodborne", "job-1"),
        Some(r"D:\.gamenode-staging-job-1".into())
    );
    assert_eq!(
        staging_folder("D:/Bloodborne", "job-1"),
        Some("D:/.gamenode-staging-job-1".into())
    );
    assert_eq!(
        staging_folder("/ports/Bloodborne", "job-1"),
        Some("/ports/.gamenode-staging-job-1".into())
    );
    assert_eq!(
        staging_folder("/Bloodborne", "job-1"),
        Some("/.gamenode-staging-job-1".into())
    );
    assert_eq!(staging_folder("Bloodborne", "job-1"), None);
}

#[test]
fn a_journal_path_that_is_already_gone_is_not_a_warning() {
    let dir = TempDir::new().unwrap();
    let target: PathBuf = dir.path().join("Ports/Gone");
    assert!(remove_path(&target.join("never-created.txt")).is_ok());
}
