use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::{Path, PathBuf},
};

use super::{
    Files, Hive, Manifest, RootResolver, ScanError, StdFiles, expand_env_with, split_registry_key,
};

#[derive(Default)]
struct MemoryFiles {
    dirs: BTreeSet<PathBuf>,
    files: BTreeMap<PathBuf, String>,
    unreadable: BTreeSet<PathBuf>,
}

impl MemoryFiles {
    fn with_dir(mut self, path: &str) -> Self {
        self.dirs.insert(PathBuf::from(path));
        self
    }

    fn with_manifest(mut self, root: &str, name: &str, body: &str) -> Self {
        let root = PathBuf::from(root);
        let manifests = root.join("Manifests");
        self.dirs.insert(root);
        self.dirs.insert(manifests.clone());
        self.files.insert(manifests.join(name), body.to_owned());
        self
    }

    fn unreadable(mut self, path: &str) -> Self {
        self.unreadable.insert(PathBuf::from(path));
        self
    }
}

impl Files for MemoryFiles {
    fn read_dir(&self, path: &Path) -> io::Result<Vec<String>> {
        if self.unreadable.contains(path) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        if !self.dirs.contains(path) {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        let mut names = BTreeSet::new();
        for candidate in self.dirs.iter().chain(self.files.keys()) {
            if candidate == path {
                continue;
            }
            if let Ok(relative) = candidate.strip_prefix(path)
                && let Some(first) = relative.components().next()
            {
                names.insert(first.as_os_str().to_string_lossy().into_owned());
            }
        }
        Ok(names.into_iter().collect())
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.dirs.contains(path)
    }
}

struct FixedRoots {
    registry: Option<String>,
    program_data: Option<String>,
}

impl RootResolver for FixedRoots {
    fn expand_env(&self, path: &str) -> Option<String> {
        expand_env_with(path, |name| {
            (name == "ProgramData").then(|| self.program_data.clone())?
        })
    }

    fn registry_string(&self, _: &str, _: &str) -> Option<String> {
        self.registry.clone()
    }
}

fn manifest() -> Manifest {
    toml::from_str(
        r#"id = "epic"
launch = { uri = "com.epicgames.launcher://apps/{id}?action=launch&silent=true" }
install = { uri = "com.epicgames.launcher://apps/{id}?action=install" }

[[roots.windows]]
registry = 'HKLM\SOFTWARE\WOW6432Node\Epic Games\EpicGamesLauncher'
value = "AppDataPath"

[[roots.windows]]
path = "%ProgramData%"
"#,
    )
    .unwrap()
}

fn installed_files() -> MemoryFiles {
    MemoryFiles::default()
        .with_manifest(
            "/launcher/data",
            "game_alpha.item",
            include_str!("fixtures/game_alpha.item"),
        )
        .with_manifest(
            "/launcher/data",
            "game_beta.item",
            include_str!("fixtures/game_beta.item"),
        )
        .with_manifest(
            "/launcher/data",
            "dlc.item",
            include_str!("fixtures/dlc.item"),
        )
        .with_manifest(
            "/launcher/data",
            "incomplete.item",
            include_str!("fixtures/incomplete.item"),
        )
        .with_manifest(
            "/launcher/data",
            "engine.item",
            include_str!("fixtures/engine.item"),
        )
        .with_manifest(
            "/launcher/data",
            "malformed.item",
            include_str!("fixtures/malformed.item"),
        )
}

#[test]
fn roots_resolve_the_registry_value_before_the_program_data_fallback() {
    let manifest = manifest();
    let roots = FixedRoots {
        registry: Some("/launcher/data".to_owned()),
        program_data: Some("/program/data".to_owned()),
    };
    assert_eq!(
        manifest.resolve_roots(&roots),
        ["/launcher/data", "/program/data"]
    );
}

#[test]
fn environment_placeholders_expand_in_the_plugin() {
    assert_eq!(
        expand_env_with(r"%ProgramData%\Epic\Data", |name| {
            (name == "ProgramData").then(|| r"C:\ProgramData".to_owned())
        })
        .as_deref(),
        Some(r"C:\ProgramData\Epic\Data")
    );
    assert_eq!(expand_env_with("%Missing%\\Epic", |_| None), None);
    assert_eq!(
        expand_env_with(r"C:\Epic\Data", |_| None).as_deref(),
        Some(r"C:\Epic\Data")
    );
}

#[test]
fn registry_keys_name_their_hive() {
    assert_eq!(
        split_registry_key(r"HKLM\SOFTWARE\WOW6432Node\Epic Games\EpicGamesLauncher"),
        Some((
            Hive::LocalMachine,
            r"SOFTWARE\WOW6432Node\Epic Games\EpicGamesLauncher"
        ))
    );
    assert_eq!(
        split_registry_key(r"HKEY_CURRENT_USER\Software\Epic"),
        Some((Hive::CurrentUser, r"Software\Epic"))
    );
    assert_eq!(split_registry_key(r"Software\Epic"), None);
    assert_eq!(split_registry_key(r"HKLM\"), None);
}

#[test]
fn reports_games_and_skips_dlc_incomplete_engine_and_malformed() {
    let files = installed_files().with_manifest(
        "/program/data",
        "fallback.item",
        include_str!("fixtures/game_beta.item"),
    );
    let roots = FixedRoots {
        registry: Some("/launcher/data".to_owned()),
        program_data: Some("/program/data".to_owned()),
    };
    let installed = manifest().scan(&files, &roots).unwrap();
    assert_eq!(
        installed
            .iter()
            .map(|game| game.id.as_str())
            .collect::<Vec<_>>(),
        ["GameAlpha", "GameBeta"]
    );

    let alpha = installed
        .iter()
        .find(|game| game.id == "GameAlpha")
        .unwrap();
    assert_eq!(alpha.name.as_deref(), Some("Alpha Adventure"));
    assert_eq!(alpha.install_dir, r"C:\Games\Alpha");
    assert_eq!(
        alpha.fields.get("namespace").map(String::as_str),
        Some("0123456789abcdef0123456789abcdef")
    );
    assert_eq!(
        alpha.fields.get("catalog_item_id").map(String::as_str),
        Some("catalog-alpha")
    );
}

#[test]
fn uses_the_program_data_fallback_when_the_registry_value_is_missing() {
    let files = MemoryFiles::default().with_manifest(
        "/program/data",
        "game_beta.item",
        include_str!("fixtures/game_beta.item"),
    );
    let roots = FixedRoots {
        registry: None,
        program_data: Some("/program/data".to_owned()),
    };
    let installed = manifest().scan(&files, &roots).unwrap();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].id, "GameBeta");
}

#[test]
fn a_missing_launcher_is_a_successful_empty_result() {
    let roots = FixedRoots {
        registry: Some("/launcher/missing".to_owned()),
        program_data: Some("/program/missing".to_owned()),
    };
    let installed = manifest().scan(&MemoryFiles::default(), &roots).unwrap();
    assert!(installed.is_empty());

    // A launcher data directory without a Manifests folder is also empty, not a failure.
    let files = MemoryFiles::default().with_dir("/launcher/data");
    let roots = FixedRoots {
        registry: Some("/launcher/data".to_owned()),
        program_data: None,
    };
    assert!(manifest().scan(&files, &roots).unwrap().is_empty());
}

#[test]
fn an_unreadable_manifest_directory_is_a_failure() {
    let files = MemoryFiles::default()
        .with_dir("/launcher/data")
        .unreadable("/launcher/data/Manifests");
    let roots = FixedRoots {
        registry: Some("/launcher/data".to_owned()),
        program_data: None,
    };
    assert_eq!(manifest().scan(&files, &roots), Err(ScanError::Unreadable));
}

#[test]
fn an_item_without_a_catalog_namespace_is_kept_without_the_field() {
    let files = MemoryFiles::default().with_manifest(
        "/launcher/data",
        "no_namespace.item",
        r#"{
            "AppName": "NoNamespace",
            "DisplayName": "No Namespace",
            "CatalogItemId": "catalog-no-namespace",
            "AppCategories": ["games"],
            "InstallLocation": "C:\\Games\\NoNamespace"
        }"#,
    );
    let roots = FixedRoots {
        registry: Some("/launcher/data".to_owned()),
        program_data: None,
    };
    let installed = manifest().scan(&files, &roots).unwrap();
    assert_eq!(installed.len(), 1);
    assert!(!installed[0].fields.contains_key("namespace"));
    assert_eq!(
        installed[0]
            .fields
            .get("catalog_item_id")
            .map(String::as_str),
        Some("catalog-no-namespace")
    );
}

#[test]
fn the_production_filesystem_adapter_reads_manifest_files() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("Epic");
    let manifests = data.join("Manifests");
    std::fs::create_dir_all(&manifests).unwrap();
    std::fs::write(
        manifests.join("game_alpha.item"),
        include_str!("fixtures/game_alpha.item"),
    )
    .unwrap();

    let roots = FixedRoots {
        registry: Some(data.to_string_lossy().into_owned()),
        program_data: None,
    };
    let installed = manifest().scan(&StdFiles, &roots).unwrap();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].id, "GameAlpha");
}
