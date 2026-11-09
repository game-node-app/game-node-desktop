//! Scanner tests over an in-memory file system.
//!
//! The `fixtures/` files follow the layout Steam writes. Their ids, paths and sizes are made up.

use super::memory::{FixedRoots, MemoryFiles};
use super::{Installed as Record, Manifest, Platform, path};

const PORTAL2: &str = include_str!("fixtures/appmanifest_portal2.acf");
const MALFORMED: &str = include_str!("fixtures/appmanifest_malformed.acf");
const LIBRARIES_WINDOWS: &str = include_str!("fixtures/libraryfolders_windows.vdf");
const LIBRARIES_OLD: &str = include_str!("fixtures/libraryfolders_old.vdf");
const LIBRARIES_LINUX: &str = include_str!("fixtures/libraryfolders_linux.vdf");

fn acf(appid: &str, name: &str, installdir: &str) -> String {
    format!(
        "\"AppState\"\n{{\n\t\"appid\"\t\t\"{appid}\"\n\t\"name\"\t\t\"{name}\"\n\t\"StateFlags\"\t\t\"4\"\n\t\"installdir\"\t\t\"{installdir}\"\n}}\n"
    )
}

/// Adds `steamapps/appmanifest_<id>.acf` and, unless `installed` is false, the install folder.
fn game(
    files: MemoryFiles,
    library: &str,
    id: &str,
    name: &str,
    dir: &str,
    installed: bool,
) -> MemoryFiles {
    let steamapps = path::join(library, "steamapps");
    let files = files.with_file(
        &path::join(&steamapps, &format!("appmanifest_{id}.acf")),
        &acf(id, name, dir),
    );
    if installed {
        files.with_dir(&path::join(&path::join(&steamapps, "common"), dir))
    } else {
        files
    }
}

fn steam_provider(_platform: Platform) -> Manifest {
    toml::from_str(include_str!("../../../../providers/steam.toml")).unwrap()
}

fn ids(records: &[Record]) -> Vec<&str> {
    let mut ids: Vec<&str> = records.iter().map(|r| r.id.as_str()).collect();
    ids.sort_unstable();
    ids
}

fn windows_scan(files: &MemoryFiles) -> Vec<Record> {
    let roots = FixedRoots::with_home("C:\\Users\\me").with_registry(
        "Software\\Valve\\Steam",
        "SteamPath",
        "C:/Program Files (x86)/Steam",
    );
    steam_provider(Platform::Windows)
        .scan(Platform::Windows, files, &roots)
        .unwrap()
}

fn linux_scan(files: &MemoryFiles) -> Vec<Record> {
    steam_provider(Platform::Linux)
        .scan(Platform::Linux, files, &FixedRoots::with_home("/home/me"))
        .unwrap()
}

#[test]
fn windows_with_two_library_folders() {
    let main = "C:\\Program Files (x86)\\Steam";
    let files = MemoryFiles::new()
        .with_file(
            &format!("{main}\\steamapps\\libraryfolders.vdf"),
            LIBRARIES_WINDOWS,
        )
        .with_file(&format!("{main}\\steamapps\\appmanifest_620.acf"), PORTAL2)
        .with_dir(&format!("{main}\\steamapps\\common\\Portal 2"))
        .with_dir("D:\\SteamLibrary\\steamapps");
    let files = game(
        files,
        "D:\\SteamLibrary",
        "570",
        "Dota 2",
        "dota 2 beta",
        true,
    );

    let records = windows_scan(&files);
    assert_eq!(ids(&records), ["570", "620"]);

    let portal = records.iter().find(|r| r.id == "620").unwrap();
    assert_eq!(portal.name.as_deref(), Some("Portal 2"));
    assert_eq!(
        portal.install_dir,
        "C:\\Program Files (x86)\\Steam\\steamapps\\common\\Portal 2"
    );
    assert_eq!(
        portal.fields.get("state_flags").map(String::as_str),
        Some("4")
    );
    let dota = records.iter().find(|r| r.id == "570").unwrap();
    assert_eq!(
        dota.install_dir,
        "D:\\SteamLibrary\\steamapps\\common\\dota 2 beta"
    );
}

#[test]
fn an_offline_library_folder_is_skipped_and_the_rest_appear() {
    // D: exists in the list but the drive is not there (and E: never was).
    let main = "C:\\Program Files (x86)\\Steam";
    let files = MemoryFiles::new()
        .with_file(
            &format!("{main}\\steamapps\\libraryfolders.vdf"),
            LIBRARIES_WINDOWS,
        )
        .with_file(&format!("{main}\\steamapps\\appmanifest_620.acf"), PORTAL2)
        .with_dir(&format!("{main}\\steamapps\\common\\Portal 2"));
    assert_eq!(ids(&windows_scan(&files)), ["620"]);
}

#[test]
fn steam_not_installed_is_an_empty_scan() {
    assert!(windows_scan(&MemoryFiles::new()).is_empty());
    assert!(linux_scan(&MemoryFiles::new()).is_empty());
}

#[test]
fn native_linux_with_a_second_library() {
    let root = "/home/me/.local/share/Steam";
    let files = MemoryFiles::new()
        .with_file(
            &format!("{root}/steamapps/libraryfolders.vdf"),
            LIBRARIES_LINUX,
        )
        .with_file(&format!("{root}/steamapps/appmanifest_620.acf"), PORTAL2)
        .with_dir(&format!("{root}/steamapps/common/Portal 2"));
    let files = game(
        files,
        "/mnt/games/SteamLibrary",
        "1245620",
        "ELDEN RING",
        "ELDEN RING",
        true,
    );

    let records = linux_scan(&files);
    assert_eq!(ids(&records), ["1245620", "620"]);
    assert_eq!(
        records.iter().find(|r| r.id == "620").unwrap().install_dir,
        "/home/me/.local/share/Steam/steamapps/common/Portal 2"
    );
}

#[test]
fn a_symlinked_native_root_yields_each_game_once() {
    let target = "/home/me/.local/share/Steam";
    let files = MemoryFiles::new()
        .with_file(&format!("{target}/steamapps/appmanifest_620.acf"), PORTAL2)
        .with_dir(&format!("{target}/steamapps/common/Portal 2"))
        .with_link("/home/me/.steam/steam", target);

    let records = linux_scan(&files);
    assert_eq!(ids(&records), ["620"]);
    assert_eq!(records.len(), 1);
}

#[test]
fn native_and_flatpak_steam_both_appear() {
    let native = "/home/me/.local/share/Steam";
    let flatpak = "/home/me/.var/app/com.valvesoftware.Steam/.local/share/Steam";
    let files = MemoryFiles::new()
        .with_file(&format!("{native}/steamapps/appmanifest_620.acf"), PORTAL2)
        .with_dir(&format!("{native}/steamapps/common/Portal 2"))
        .with_link("/home/me/.steam/steam", native);
    let files = game(
        files,
        flatpak,
        "413150",
        "Stardew Valley",
        "Stardew Valley",
        true,
    );

    assert_eq!(ids(&linux_scan(&files)), ["413150", "620"]);
}

#[test]
fn the_old_libraryfolders_format_and_config_fallback_are_read() {
    // Old format, and only in config/ (not steamapps/).
    let root = "C:\\Steam";
    let files = MemoryFiles::new()
        .with_file(
            &format!("{root}\\config\\libraryfolders.vdf"),
            LIBRARIES_OLD,
        )
        .with_dir(&format!("{root}\\steamapps"));
    let files = game(files, "D:\\Games\\Steam", "400", "Portal", "Portal", true);
    let roots = FixedRoots::default().with_registry("Software\\Valve\\Steam", "SteamPath", root);

    let records = steam_provider(Platform::Windows)
        .scan(Platform::Windows, &files, &roots)
        .unwrap();
    assert_eq!(ids(&records), ["400"]);
}

#[test]
fn one_malformed_manifest_does_not_hide_the_others() {
    let root = "/home/me/.local/share/Steam";
    let files = MemoryFiles::new()
        .with_file(&format!("{root}/steamapps/appmanifest_730.acf"), MALFORMED)
        .with_dir(&format!("{root}/steamapps/common/Broken"))
        .with_file(&format!("{root}/steamapps/appmanifest_620.acf"), PORTAL2)
        .with_dir(&format!("{root}/steamapps/common/Portal 2"));
    assert_eq!(ids(&linux_scan(&files)), ["620"]);
}

#[test]
fn a_malformed_libraryfolders_file_still_scans_the_root() {
    let root = "/home/me/.local/share/Steam";
    let files = MemoryFiles::new()
        .with_file(
            &format!("{root}/steamapps/libraryfolders.vdf"),
            "\"libraryfolders\" {",
        )
        .with_file(&format!("{root}/steamapps/appmanifest_620.acf"), PORTAL2)
        .with_dir(&format!("{root}/steamapps/common/Portal 2"));
    assert_eq!(ids(&linux_scan(&files)), ["620"]);
}

#[test]
fn unusable_manifests_are_skipped() {
    let root = "/home/me/.local/share/Steam";
    let mut files = MemoryFiles::new();
    // Not installed (no common/ folder), non-numeric app id, traversal in installdir, unreadable.
    files = game(files, root, "1", "Not Installed", "Gone", false);
    files = game(files, root, "abc", "Bad Id", "Bad Id", true);
    files = game(files, root, "3", "Traversal", "../../etc", true);
    files = game(files, root, "4", "Unreadable", "Unreadable", true)
        .with_unreadable(&format!("{root}/steamapps/appmanifest_4.acf"));
    files = game(files, root, "5", "Fine", "Fine", true);
    assert_eq!(ids(&linux_scan(&files)), ["5"]);
}

#[test]
fn tools_runtimes_and_proton_are_not_games() {
    let root = "/home/me/.local/share/Steam";
    let mut files = MemoryFiles::new();
    for (id, name, dir) in [
        (
            "228980",
            "Steamworks Common Redistributables",
            "Steamworks Shared",
        ),
        (
            "1628350",
            "Steam Linux Runtime 3.0 (sniper)",
            "SteamLinuxRuntime_sniper",
        ),
        ("1493710", "Proton Experimental", "Proton - Experimental"),
        ("2805730", "Proton 9.0", "Proton 9.0"),
        ("620", "Portal 2", "Portal 2"),
    ] {
        files = game(files, root, id, name, dir, true);
    }
    assert_eq!(ids(&linux_scan(&files)), ["620"]);
}

#[test]
fn a_game_in_two_libraries_appears_once() {
    let root = "/home/me/.local/share/Steam";
    let files = MemoryFiles::new().with_file(
        &format!("{root}/steamapps/libraryfolders.vdf"),
        LIBRARIES_LINUX,
    );
    let files = game(files, root, "620", "Portal 2", "Portal 2", true);
    let files = game(
        files,
        "/mnt/games/SteamLibrary",
        "620",
        "Portal 2",
        "Portal 2",
        true,
    );
    let records = linux_scan(&files);
    assert_eq!(records.len(), 1);
    assert!(records[0].install_dir.starts_with(root));
}

#[test]
fn launch_and_install_uris_come_from_the_scanned_record() {
    let root = "/home/me/.local/share/Steam";
    let files = game(
        MemoryFiles::new(),
        root,
        "620",
        "Portal 2",
        "Portal 2",
        true,
    );
    let records = linux_scan(&files);
    let steam = steam_provider(Platform::Linux);
    assert_eq!(records[0].id, "620");
    assert_eq!(steam.launch.unwrap().uri, "steam://rungameid/{id}");
    assert_eq!(steam.install.unwrap().uri, "steam://nav/games/details/{id}");
}
