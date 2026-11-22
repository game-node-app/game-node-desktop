//! Fixtures for the bounded package-manager queries.
//!
//! The backend processes and the PackageKit connection are replaced by in-memory fakes so every
//! outcome (success, empty, timeout, malformed output, unavailable backend) and the pacman /
//! PackageKit record parity can be asserted without touching the host package manager.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

use async_trait::async_trait;

use super::scan::{
    DesktopEntry, InstalledFileQuery, MAX_RECORDS, PackageBackend, PackageRow, PacmanRunner,
    ScanFailure, candidate_from_desktop, exec_program, normalized_package_name,
    parse_game_desktop_entry, parse_package_owner, resolve_exec, scan_packagekit, scan_pacman,
    search_installed_files,
};

const SHORT_TIMEOUT: Duration = Duration::from_millis(20);

struct FakePacman {
    owners: BTreeMap<PathBuf, Result<Option<String>, ScanFailure>>,
    queries: Mutex<Vec<PathBuf>>,
}

impl FakePacman {
    fn new(owners: impl IntoIterator<Item = (PathBuf, Option<&'static str>)>) -> Self {
        Self {
            owners: owners
                .into_iter()
                .map(|(path, owner)| (path, Ok(owner.map(str::to_owned))))
                .collect(),
            queries: Mutex::new(Vec::new()),
        }
    }

    fn failing(failure: ScanFailure) -> Self {
        Self {
            owners: BTreeMap::from([(
                PathBuf::from("/usr/share/applications/supertux2.desktop"),
                Err(failure),
            )]),
            queries: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl PacmanRunner for FakePacman {
    async fn owner(&self, path: &Path) -> Result<Option<String>, ScanFailure> {
        self.queries.lock().unwrap().push(path.to_owned());
        self.owners.get(path).cloned().unwrap_or(Ok(None))
    }
}

struct HangingPacman;

#[async_trait]
impl PacmanRunner for HangingPacman {
    async fn owner(&self, _: &Path) -> Result<Option<String>, ScanFailure> {
        std::future::pending().await
    }
}

struct FakePackageKit {
    rows: BTreeMap<String, PackageRow>,
    failure: Option<ScanFailure>,
    calls: Mutex<Vec<(u64, Vec<String>)>>,
}

impl FakePackageKit {
    fn new(rows: impl IntoIterator<Item = (String, PackageRow)>) -> Self {
        Self {
            rows: rows.into_iter().collect(),
            failure: None,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn empty() -> Self {
        Self::new(Vec::new())
    }

    fn failing(failure: ScanFailure) -> Self {
        Self {
            rows: BTreeMap::new(),
            failure: Some(failure),
            calls: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl InstalledFileQuery for FakePackageKit {
    async fn search_installed_files(
        &self,
        filter: u64,
        files: Vec<String>,
    ) -> Result<Vec<PackageRow>, ScanFailure> {
        self.calls.lock().unwrap().push((filter, files.clone()));
        if let Some(failure) = self.failure {
            return Err(failure);
        }
        Ok(files
            .iter()
            .filter_map(|file| self.rows.get(file).cloned())
            .collect())
    }
}

/// Returns as many rows as a PackageKit transaction can be asked to report.
struct FloodingPackageKit;

#[async_trait]
impl InstalledFileQuery for FloodingPackageKit {
    async fn search_installed_files(
        &self,
        _: u64,
        _: Vec<String>,
    ) -> Result<Vec<PackageRow>, ScanFailure> {
        Ok((0..MAX_RECORDS + 100)
            .map(|index| PackageRow {
                package_id: format!("game-{index};1.0;x86_64;installed"),
                summary: String::new(),
            })
            .collect())
    }
}

struct HangingPackageKit;

#[async_trait]
impl InstalledFileQuery for HangingPackageKit {
    async fn search_installed_files(
        &self,
        _: u64,
        _: Vec<String>,
    ) -> Result<Vec<PackageRow>, ScanFailure> {
        std::future::pending().await
    }
}

fn executable(directory: &Path, name: &str) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, b"fixture").unwrap();
    path
}

fn entry(path: &str, name: &str, program: &Path) -> DesktopEntry {
    DesktopEntry {
        path: PathBuf::from(path),
        name: Some(name.to_owned()),
        exec: program.to_string_lossy().into_owned(),
        binary: Some(program.to_owned()),
    }
}

#[tokio::test]
async fn pacman_reports_the_installed_owner_of_a_visible_game_entry() {
    let directory = tempfile::tempdir().unwrap();
    let program = executable(directory.path(), "supertux2");
    let game = entry(
        "/usr/share/applications/supertux2.desktop",
        "SuperTux",
        &program,
    );
    let other = entry("/usr/share/applications/editor.desktop", "Editor", &program);
    let runner = FakePacman::new([(game.path.clone(), Some("supertux"))]);

    let records = scan_pacman(&runner, &[game.clone(), other.clone()], SHORT_TIMEOUT)
        .await
        .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, "supertux");
    assert_eq!(records[0].name.as_deref(), Some("SuperTux"));
    assert_eq!(records[0].install_dir, directory.path().to_string_lossy());
    assert_eq!(records[0].fields["package_manager"], "pacman");
    assert_eq!(records[0].fields["package_id"], "supertux");
    assert_eq!(
        records[0].fields["desktop_file"],
        "/usr/share/applications/supertux2.desktop"
    );
    assert_eq!(records[0].fields["executable"], program.to_string_lossy());
    // Only candidate entries were queried, and each owner was asked exactly once.
    assert_eq!(
        runner.queries.lock().unwrap().as_slice(),
        [game.path, other.path]
    );
}

#[tokio::test]
async fn pacman_distinguishes_a_successful_empty_scan_from_backend_failures() {
    let directory = tempfile::tempdir().unwrap();
    let program = executable(directory.path(), "supertux2");
    let game = entry(
        "/usr/share/applications/supertux2.desktop",
        "SuperTux",
        &program,
    );

    let empty = scan_pacman(
        &FakePacman::new(Vec::new()),
        std::slice::from_ref(&game),
        SHORT_TIMEOUT,
    )
    .await
    .expect("an unowned entry is a successful empty scan");
    assert!(empty.is_empty());

    for (runner, failure) in [
        (
            FakePacman::failing(ScanFailure::PacmanUnavailable),
            ScanFailure::PacmanUnavailable,
        ),
        (
            FakePacman::failing(ScanFailure::PacmanFailed),
            ScanFailure::PacmanFailed,
        ),
        (
            FakePacman::failing(ScanFailure::PacmanMalformed),
            ScanFailure::PacmanMalformed,
        ),
    ] {
        assert_eq!(
            scan_pacman(&runner, std::slice::from_ref(&game), SHORT_TIMEOUT)
                .await
                .unwrap_err(),
            failure
        );
    }

    assert_eq!(
        scan_pacman(&HangingPacman, &[game], SHORT_TIMEOUT)
            .await
            .unwrap_err(),
        ScanFailure::PacmanTimeout
    );
}

#[tokio::test]
async fn pacman_scan_never_executes_the_desktop_command() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("executed");
    let program = directory.path().join("game");
    std::fs::write(
        &program,
        format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&program).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(&program, permissions).unwrap();

    let text = format!(
        "[Desktop Entry]\nType=Application\nName=Trap\nExec={} --launch %U\nCategories=Game;\n",
        program.display()
    );
    let game =
        parse_game_desktop_entry(PathBuf::from("/usr/share/applications/trap.desktop"), &text)
            .expect("the entry resolves to an existing program");
    let runner = FakePacman::new([(game.path.clone(), Some("trap-game"))]);

    let records = scan_pacman(&runner, &[game], SHORT_TIMEOUT).await.unwrap();

    assert_eq!(records.len(), 1);
    assert!(
        !marker.exists(),
        "the desktop command must never be executed"
    );
}

#[tokio::test]
async fn pacman_keeps_one_stable_record_for_repeated_package_owners() {
    let directory = tempfile::tempdir().unwrap();
    let program = executable(directory.path(), "supertux2");
    let main = entry(
        "/usr/share/applications/supertux2.desktop",
        "SuperTux",
        &program,
    );
    let editor = entry(
        "/usr/share/applications/supertux-editor.desktop",
        "SuperTux Editor",
        &program,
    );
    let runner = FakePacman::new([
        (main.path.clone(), Some("supertux")),
        (editor.path.clone(), Some("supertux")),
    ]);

    let records = scan_pacman(&runner, &[main, editor], SHORT_TIMEOUT)
        .await
        .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, "supertux");
    assert_eq!(records[0].name.as_deref(), Some("SuperTux"));
}

#[tokio::test]
async fn packagekit_queries_only_installed_files_in_bounded_batches() {
    let paths = (0..130)
        .map(|index| format!("/usr/share/applications/game-{index}.desktop"))
        .collect::<Vec<_>>();
    let rows = paths
        .iter()
        .map(|path| {
            (
                path.clone(),
                PackageRow {
                    package_id: "game;1.0;x86_64;installed".into(),
                    summary: "Game".into(),
                },
            )
        })
        .collect::<Vec<_>>();
    let query = FakePackageKit::new(rows);

    let found = search_installed_files(&query, &paths, SHORT_TIMEOUT)
        .await
        .unwrap();

    let calls = query.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    // PackageKit's Installed enum is ordinal 2, so the wire bitfield is 4.
    assert!(calls.iter().all(|(filter, _)| *filter == 4));
    assert_eq!(calls[0].1.len(), 128);
    assert_eq!(calls[1].1.len(), 2);
    assert_eq!(
        calls
            .iter()
            .flat_map(|(_, files)| files.clone())
            .collect::<Vec<_>>(),
        paths
    );
    assert_eq!(found.len(), 130);
}

#[tokio::test]
async fn packagekit_truncates_an_over_long_record_set() {
    let paths = vec!["/usr/share/applications/game.desktop".to_owned()];
    let found = search_installed_files(&FloodingPackageKit, &paths, SHORT_TIMEOUT)
        .await
        .unwrap();
    assert_eq!(found.len(), MAX_RECORDS);
}

#[tokio::test]
async fn packagekit_distinguishes_empty_results_from_failures_and_timeouts() {
    let directory = tempfile::tempdir().unwrap();
    let program = executable(directory.path(), "supertux2");
    let game = entry(
        "/usr/share/applications/supertux2.desktop",
        "SuperTux",
        &program,
    );

    let empty = scan_packagekit(
        &FakePackageKit::empty(),
        std::slice::from_ref(&game),
        SHORT_TIMEOUT,
    )
    .await
    .expect("no installed owner is a successful empty scan");
    assert!(empty.is_empty());

    for (failure, expected) in [
        (
            ScanFailure::PackageKitUnavailable,
            ScanFailure::PackageKitUnavailable,
        ),
        (ScanFailure::PackageKitFailed, ScanFailure::PackageKitFailed),
    ] {
        assert_eq!(
            scan_packagekit(
                &FakePackageKit::failing(failure),
                std::slice::from_ref(&game),
                SHORT_TIMEOUT
            )
            .await
            .unwrap_err(),
            expected
        );
    }

    assert_eq!(
        scan_packagekit(
            &HangingPackageKit,
            std::slice::from_ref(&game),
            SHORT_TIMEOUT
        )
        .await
        .unwrap_err(),
        ScanFailure::PackageKitTimeout
    );

    // Malformed package ids are skipped, not turned into a backend failure.
    let malformed = FakePackageKit::new([(
        game.path.to_string_lossy().into_owned(),
        PackageRow {
            package_id: ";1.0;x86_64;installed".into(),
            summary: "Broken".into(),
        },
    )]);
    assert!(
        scan_packagekit(&malformed, &[game], SHORT_TIMEOUT)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn packagekit_normalizes_a_package_record() {
    let directory = tempfile::tempdir().unwrap();
    let program = executable(directory.path(), "supertux2");
    let game = entry(
        "/usr/share/applications/supertux2.desktop",
        "SuperTux",
        &program,
    );
    let query = FakePackageKit::new([(
        game.path.to_string_lossy().into_owned(),
        PackageRow {
            package_id: "supertux;0.6.3-1;x86_64;installed".into(),
            summary: "A classic 2D jump'n run".into(),
        },
    )]);

    let records = scan_packagekit(&query, &[game], SHORT_TIMEOUT)
        .await
        .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, "supertux");
    assert_eq!(records[0].name.as_deref(), Some("SuperTux"));
    assert_eq!(records[0].fields["package_manager"], "packagekit");
    assert_eq!(
        records[0].fields["package_id"],
        "supertux;0.6.3-1;x86_64;installed"
    );
}

#[tokio::test]
async fn pacman_and_packagekit_report_the_same_provider_local_records() {
    let directory = tempfile::tempdir().unwrap();
    let program = executable(directory.path(), "supertux2");
    let game = entry(
        "/usr/share/applications/supertux2.desktop",
        "SuperTux",
        &program,
    );
    let editor = entry(
        "/usr/share/applications/supertux-editor.desktop",
        "SuperTux Editor",
        &program,
    );
    let entries = [game.clone(), editor.clone()];

    let pacman = scan_pacman(
        &FakePacman::new([
            (game.path.clone(), Some("supertux")),
            (editor.path.clone(), Some("supertux")),
        ]),
        &entries,
        SHORT_TIMEOUT,
    )
    .await
    .unwrap();
    let packagekit = scan_packagekit(
        &FakePackageKit::new([
            (
                game.path.to_string_lossy().into_owned(),
                PackageRow {
                    package_id: "supertux;0.6.3-1;x86_64;installed".into(),
                    summary: String::new(),
                },
            ),
            (
                editor.path.to_string_lossy().into_owned(),
                PackageRow {
                    package_id: "supertux;0.6.3-1;x86_64;installed".into(),
                    summary: String::new(),
                },
            ),
        ]),
        &entries,
        SHORT_TIMEOUT,
    )
    .await
    .unwrap();

    let identity = |records: &[super::scan::PackageCandidate]| {
        records
            .iter()
            .map(|record| {
                (
                    record.id.clone(),
                    record.name.clone(),
                    record.install_dir.clone(),
                    record.fields["desktop_file"].clone(),
                    record.fields["executable"].clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(identity(&pacman), identity(&packagekit));
    assert_eq!(pacman[0].fields["package_manager"], "pacman");
    assert_eq!(packagekit[0].fields["package_manager"], "packagekit");
}

/// Manual, read-only evidence on a live Arch-family host.
///
/// The scan only stats desktop files and runs `pacman -Qqo`; it never executes a desktop entry.
/// Run with `cargo test -p gamenode-plugin-packagekit --offline -- --ignored --nocapture`.
#[tokio::test]
#[ignore = "reads the live /etc/os-release and pacman database; run manually on Arch"]
async fn live_arch_system_reports_installed_game_desktop_owners() {
    let os_release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    assert_eq!(
        PackageBackend::for_os_release(&os_release),
        PackageBackend::Pacman,
        "the live evidence needs an Arch-family host"
    );
    let records = super::scan::discover().await.unwrap();
    assert!(
        !records.is_empty(),
        "the live host should report at least one installed game"
    );
    for record in &records {
        assert_eq!(record.fields["package_manager"], "pacman");
        assert!(Path::new(&record.fields["executable"]).is_file());
        assert!(Path::new(&record.fields["desktop_file"]).is_file());
        println!(
            "{} ({}) -> {} [{}]",
            record.id,
            record.name.as_deref().unwrap_or_default(),
            record.fields["executable"],
            record.fields["desktop_file"]
        );
    }
}

#[test]
fn selects_pacman_for_arch_and_arch_derivatives() {
    assert_eq!(
        PackageBackend::for_os_release("ID=arch\n"),
        PackageBackend::Pacman
    );
    assert_eq!(
        PackageBackend::for_os_release("ID=manjaro\nID_LIKE=arch\n"),
        PackageBackend::Pacman
    );
    assert_eq!(
        PackageBackend::for_os_release("ID=\"arch\"\n"),
        PackageBackend::Pacman
    );
}

#[test]
fn selects_packagekit_for_other_or_unknown_distributions() {
    assert_eq!(
        PackageBackend::for_os_release("ID=fedora\nID_LIKE=redhat\n"),
        PackageBackend::PackageKit
    );
    assert_eq!(
        PackageBackend::for_os_release(""),
        PackageBackend::PackageKit
    );
}

#[test]
fn pacman_owner_output_accepts_package_and_skips_empty_or_malformed_values() {
    assert_eq!(
        parse_package_owner(b"supertux\n").unwrap(),
        Some("supertux".to_owned())
    );
    assert_eq!(parse_package_owner(b"\n").unwrap(), None);
    assert_eq!(parse_package_owner(b"bad\nname\n").unwrap(), None);
    assert_eq!(parse_package_owner(b"bad\x00name\n").unwrap(), None);
    assert_eq!(
        parse_package_owner(&[0xff]),
        Err(ScanFailure::PacmanMalformed)
    );
}

#[test]
fn packagekit_ids_normalize_to_the_same_package_name_as_pacman() {
    assert_eq!(
        normalized_package_name("supertux;0.6.3-4;x86_64;installed"),
        Some("supertux")
    );
    assert_eq!(normalized_package_name("supertux"), Some("supertux"));
    assert_eq!(normalized_package_name(";1.0;x86_64;installed"), None);
    assert_eq!(normalized_package_name("bad/name;1.0;arch;installed"), None);
}

#[test]
fn game_desktop_metadata_is_required_and_candidates_keep_package_identity() {
    let desktop = parse_game_desktop_entry(
        PathBuf::from("/usr/share/applications/supertux2.desktop"),
        "[Desktop Entry]\nType=Application\nName=SuperTux\nExec=/usr/bin/true\nCategories=Game;\n",
    )
    .unwrap();
    let candidate = candidate_from_desktop("supertux", &desktop, "pacman").unwrap();
    assert_eq!(candidate.id, "supertux");
    assert_eq!(candidate.name.as_deref(), Some("SuperTux"));
    assert_eq!(candidate.fields["package_manager"], "pacman");

    assert!(parse_game_desktop_entry(
        PathBuf::from("/usr/share/applications/not-a-game.desktop"),
        "[Desktop Entry]\nType=Application\nName=Text Editor\nExec=/usr/bin/true\nCategories=Utility;\n",
    )
    .is_none());
    assert!(parse_game_desktop_entry(
        PathBuf::from("/usr/share/applications/hidden.desktop"),
        "[Desktop Entry]\nType=Application\nName=Hidden\nExec=/usr/bin/true\nCategories=Game;\nNoDisplay=true\n",
    )
    .is_none());
    assert!(
        parse_game_desktop_entry(
            PathBuf::from("/usr/share/applications/malformed.desktop"),
            "not a desktop entry",
        )
        .is_none()
    );
    assert!(parse_game_desktop_entry(
        PathBuf::from("/usr/share/applications/missing.desktop"),
        "[Desktop Entry]\nType=Application\nName=Missing\nExec=/nonexistent/program\nCategories=Game;\n",
    )
    .is_some_and(|entry| entry.binary.is_none()));
}

#[test]
fn quoted_exec_program_with_spaces_is_discovered_without_interpreting_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let program = dir.path().join("Game With Spaces");
    std::fs::write(&program, b"fixture").unwrap();
    let exec = format!("\"{}\" --fullscreen %U", program.display());
    assert_eq!(resolve_exec(&exec), Some(program));
    assert!(exec_program("\"unterminated path").is_none());
    assert!(exec_program("game\\").is_none());
    assert!(exec_program("%f --game").is_none());
    assert!(exec_program(&"a".repeat(8_193)).is_none());
    assert_eq!(
        exec_program("\"game\\\"name\" %U").as_deref(),
        Some("game\"name")
    );
    assert_eq!(resolve_exec("/nonexistent/game"), None);
}
