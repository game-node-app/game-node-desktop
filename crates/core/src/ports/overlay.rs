//! The GameNode port overlay: schema, validation and merge rules (design D4).
//!
//! `ports/overlay.toml` in the GameNode repository corrects and extends the upstream catalog,
//! keyed by the upstream's stable catalog id. The bundled copy is embedded at build time and the
//! host may fetch a newer one; an overlay that fails to parse or validate is ignored whole, so the
//! newest valid copy stays in use.
//!
//! Merge rules:
//!
//! - overlay scalar fields replace upstream values;
//! - an overlay `setup_file` list replaces the whole upstream `setupFiles` list, because partial
//!   merging of recipe slots is too easy to get wrong;
//! - `hidden = true` removes the entry from the list;
//! - `launcher` becomes `Registration.executable` and `Registration.launchers`, and `gameplay`
//!   becomes `Registration.gameplay`;
//! - only entries that support Windows stay in the listable catalog.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::ports::catalog::{
    AcceptedHash, CatalogEntry, CopyRoot, Extractor, FromPackage, LauncherRecipe, RecipeError,
    SetupFile, SetupKind, is_safe_relative_path,
};

/// The overlay schema version this build understands.
///
/// `schema_version` is the major version: a copy that declares a newer one is rejected without
/// looking at its contents, and the previous valid copy stays in use.
pub const SUPPORTED_SCHEMA_VERSION: u32 = 1;

/// The GameNode overlay document.
///
/// Construct one through [`Overlay::parse`], which validates it. The fields are public so the
/// document can be serialized and stored, not so callers can build an unvalidated overlay.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Overlay {
    /// The overlay schema's major version.
    pub schema_version: u32,
    /// Extraction tools pinned for the package slots that reference them.
    pub extractor: Vec<Extractor>,
    /// Corrections, keyed by the upstream's stable catalog id.
    pub entry: BTreeMap<String, OverlayEntry>,
}

/// The overlay's corrections for one catalog entry.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayEntry {
    /// The GameNode game id used as matching evidence (design D11).
    pub gamenode_game_id: Option<u64>,
    /// The launch executable, relative to the install folder.
    pub launcher: Option<String>,
    /// Gameplay executables; `[]` clears the upstream list.
    pub gameplay: Option<Vec<String>>,
    pub license: Option<String>,
    /// A release version to install instead of the upstream's verified one.
    pub release: Option<String>,
    /// An edition note shown in the detail pane.
    pub edition: Option<String>,
    /// Removes the entry from the list. Installed copies stay in the library.
    pub hidden: bool,
    /// Replaces the upstream `setupFiles` list. `[]` removes guided setup.
    pub setup_file: Option<Vec<OverlaySetupFile>>,
}

/// One overlay-authored setup file. Fields are optional because the overlay restates only what it
/// wants to change from the slot's defaults; the overlay list as a whole still replaces the
/// upstream list.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlaySetupFile {
    pub id: String,
    pub label: Option<String>,
    pub prompt: Option<String>,
    /// Omitted means [`SetupKind::Folder`] when the slot has folder markers or packages, else
    /// [`SetupKind::File`].
    pub kind: Option<SetupKind>,
    /// Omitted means required.
    pub required: Option<bool>,
    pub destination: Option<String>,
    pub root: Option<CopyRoot>,
    pub flag: Option<String>,
    pub positional: Option<bool>,
    pub env: Option<String>,
    pub contains: Vec<String>,
    pub extensions: Vec<String>,
    pub accepted: Vec<AcceptedHash>,
    pub hash_file: Option<String>,
    pub from_package: Option<FromPackage>,
}

impl OverlaySetupFile {
    /// Builds the merged slot. The overlay is validated before it is used, so nothing here can
    /// fail; [`Overlay::validate`] is what rejects a bad slot.
    fn to_setup_file(&self) -> SetupFile {
        let kind = self.kind.unwrap_or({
            if !self.contains.is_empty() || self.from_package.is_some() {
                SetupKind::Folder
            } else {
                SetupKind::File
            }
        });
        SetupFile {
            id: self.id.clone(),
            label: self.label.clone().unwrap_or_else(|| self.id.clone()),
            prompt: self.prompt.clone(),
            required: self.required.unwrap_or(true),
            kind,
            destination: self.destination.clone(),
            root: self.root,
            flag: self.flag.clone(),
            positional: self.positional.unwrap_or(false),
            env: self.env.clone(),
            contains: self.contains.clone(),
            extensions: self.extensions.clone(),
            accepted: self.accepted.clone(),
            hash_file: self.hash_file.clone(),
            from_package: self.from_package.clone(),
        }
    }
}

impl Overlay {
    /// Parses and validates an overlay document. Any error means the caller keeps the overlay it
    /// already has.
    pub fn parse(text: &str) -> Result<Self, OverlayError> {
        let overlay: Self =
            toml::from_str(text).map_err(|error| OverlayError::Parse(error.to_string()))?;
        overlay.validate()?;
        Ok(overlay)
    }

    /// Validates an overlay value that was deserialized elsewhere, such as from local storage.
    pub fn validate(&self) -> Result<(), OverlayError> {
        if self.schema_version == 0 {
            return Err(OverlayError::MissingSchemaVersion);
        }
        if self.schema_version > SUPPORTED_SCHEMA_VERSION {
            return Err(OverlayError::UnsupportedSchema {
                found: self.schema_version,
                supported: SUPPORTED_SCHEMA_VERSION,
            });
        }

        let mut extractor_ids = BTreeSet::new();
        for extractor in &self.extractor {
            if !extractor_ids.insert(extractor.id.as_str()) {
                return Err(OverlayError::DuplicateExtractor {
                    id: extractor.id.clone(),
                });
            }
            if !extractor.is_valid() {
                return Err(OverlayError::InvalidExtractor {
                    id: extractor.id.clone(),
                    reason: "a field is missing or the pinned SHA-256 is not a hex digest".into(),
                });
            }
        }

        for (catalog_id, entry) in &self.entry {
            if catalog_id.trim().is_empty() {
                return Err(OverlayError::MissingCatalogId);
            }
            if let Some(launcher) = &entry.launcher
                && !is_safe_relative_path(launcher)
            {
                return Err(OverlayError::InvalidPath {
                    entry: catalog_id.clone(),
                    path: launcher.clone(),
                });
            }
            if let Some(gameplay) = &entry.gameplay {
                for executable in gameplay {
                    if !is_safe_relative_path(executable) {
                        return Err(OverlayError::InvalidPath {
                            entry: catalog_id.clone(),
                            path: executable.clone(),
                        });
                    }
                }
            }
            let Some(setup_files) = &entry.setup_file else {
                continue;
            };
            let mut slot_ids = BTreeSet::new();
            for slot in setup_files {
                let setup_file = slot.to_setup_file();
                setup_file
                    .validate()
                    .map_err(|error| OverlayError::InvalidSlot {
                        entry: catalog_id.clone(),
                        error,
                    })?;
                if !slot_ids.insert(setup_file.id.clone()) {
                    return Err(OverlayError::DuplicateSlot {
                        entry: catalog_id.clone(),
                        slot: setup_file.id,
                    });
                }
                if let Some(package) = &setup_file.from_package
                    && !extractor_ids.contains(package.extractor.as_str())
                {
                    return Err(OverlayError::UnknownExtractor {
                        entry: catalog_id.clone(),
                        extractor: package.extractor.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    /// The extractor declared for a package slot, when there is one.
    pub fn extractor(&self, id: &str) -> Option<&Extractor> {
        self.extractor.iter().find(|extractor| extractor.id == id)
    }

    /// Applies the overlay to one entry.
    ///
    /// Returns `None` when the overlay hides the entry. The entry keeps its operating systems;
    /// callers that build the listable catalog use [`Overlay::apply`], which filters Windows.
    pub fn merge(&self, entry: CatalogEntry) -> Option<CatalogEntry> {
        let Some(overlay) = self.entry.get(&entry.catalog_id) else {
            return Some(entry);
        };
        if overlay.hidden {
            return None;
        }

        let mut merged = entry;
        // Extraction tools are always derived from the overlay's package slots, so a re-merge never
        // keeps a pin the current overlay no longer declares.
        merged.extractors.clear();
        // The overlay template uses 0 as the "not authored yet" placeholder.
        if let Some(game_id) = overlay.gamenode_game_id.filter(|game_id| *game_id != 0) {
            merged.gamenode_game_id = Some(game_id);
        }
        if let Some(launcher) = &overlay.launcher {
            merged.launcher_executable = Some(launcher.clone());
        }
        if let Some(gameplay) = &overlay.gameplay {
            merged.gameplay_executables = gameplay.clone();
        }
        if let Some(license) = &overlay.license {
            merged.license = Some(license.clone());
        }
        if let Some(release) = &overlay.release {
            merged.pinned_release = Some(release.clone());
        }
        if let Some(edition) = &overlay.edition {
            merged.edition = Some(edition.clone());
        }
        if let Some(slots) = &overlay.setup_file {
            let launcher = std::mem::take(&mut merged.launcher);
            merged.launcher = LauncherRecipe {
                setup_files: slots.iter().map(OverlaySetupFile::to_setup_file).collect(),
                ..launcher
            };
            merged.extractors.clear();
            for slot in &merged.launcher.setup_files {
                let Some(package) = &slot.from_package else {
                    continue;
                };
                if let Some(extractor) = self.extractor(&package.extractor)
                    && !merged
                        .extractors
                        .iter()
                        .any(|known| known.id == extractor.id)
                {
                    merged.extractors.push(extractor.clone());
                }
            }
        }
        Some(merged)
    }

    /// The listable catalog: the overlay merged into every entry, hidden entries removed and
    /// entries that do not support Windows dropped.
    pub fn apply(&self, entries: Vec<CatalogEntry>) -> Vec<CatalogEntry> {
        entries
            .into_iter()
            .filter_map(|entry| self.merge(entry))
            .filter(CatalogEntry::supports_windows)
            .collect()
    }
}

/// Why an overlay document cannot be used. The caller keeps its previous valid copy for every
/// variant.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum OverlayError {
    #[error("the overlay is not valid TOML: {0}")]
    Parse(String),
    #[error("the overlay has no schema_version")]
    MissingSchemaVersion,
    #[error("the overlay uses schema version {found}, but this build supports up to {supported}")]
    UnsupportedSchema { found: u32, supported: u32 },
    #[error("the overlay declares extractor {id:?} more than once")]
    DuplicateExtractor { id: String },
    #[error("the overlay extractor {id:?} is invalid: {reason}")]
    InvalidExtractor { id: String, reason: String },
    #[error("the overlay entry {entry:?} has an invalid setup file: {error}")]
    InvalidSlot { entry: String, error: RecipeError },
    #[error("the overlay entry {entry:?} names setup file {slot:?} more than once")]
    DuplicateSlot { entry: String, slot: String },
    #[error("the overlay entry {entry:?} names unknown extractor {extractor:?}")]
    UnknownExtractor { entry: String, extractor: String },
    #[error("the overlay entry {entry:?} has a path that is not a safe relative path: {path:?}")]
    InvalidPath { entry: String, path: String },
    #[error("the overlay has an entry with an empty catalog id")]
    MissingCatalogId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::catalog::{CatalogProject, VerifiedRelease};

    fn digest() -> String {
        "941f887a562aae054fac35af8cc8f27cf075f3d4cc2e029fb5ae2a663aaa5ae7".into()
    }

    fn overlay_toml() -> String {
        format!(
            r#"
schema_version = 1

[[extractor]]
id = "ps4-pkg"
name = "PKG extractor"
repository = "owner/repo"
tag = "v1.2.3"
asset = "ps4-pkg-win64.zip"
sha256 = "{sha}"
license = "GPL-3.0"
executable = "ps4-pkg.exe"
args = ["extract", "{{package}}", "--update", "{{update}}", "--out", "{{out}}"]

[entry."qcat_lantern"]
gamenode_game_id = 2100
launcher = "Lantern Engine.exe"
gameplay = ["Bloodborne.exe"]
license = "GPL-3.0-or-later"
release = "v0.9.38"

[[entry."qcat_lantern".setup_file]]
id = "game"
label = "Bloodborne (CUSA03173) with update 01.09"
kind = "folder"
contains = ["eboot.bin", "sce_sys/param.sfo"]
hash_file = "eboot.bin"
accepted = [{{ algo = "sha256", value = "{sha}", label = "CUSA03173 01.09" }}]
destination = "CUSA03173"
from_package = {{ extractor = "ps4-pkg", title_id = "CUSA03173", needs_update = "01.09" }}
"#,
            sha = digest()
        )
    }

    fn upstream_entry() -> CatalogEntry {
        CatalogEntry {
            catalog_id: "qcat_lantern".into(),
            slug: "bloodborne-bloodborne-pc-windows-port".into(),
            name: "Bloodborne".into(),
            project: Some(CatalogProject {
                name: "bloodborne_pc_windows_port".into(),
                ..CatalogProject::default()
            }),
            supported_os: vec!["windows".into()],
            verified: Some(VerifiedRelease {
                version: "v0.8.0".into(),
                released_at: 1_793_556_259_000,
                ..VerifiedRelease::default()
            }),
            launcher: LauncherRecipe {
                folder_name: Some("Bloodborne-BloodbornePcWindowsPort".into()),
                release_asset_filter: Some("windows_port".into()),
                preferred_executables: crate::ports::catalog::PreferredExecutables {
                    windows: vec!["Bloodborne.exe".into()],
                    ..Default::default()
                },
                setup_files: vec![SetupFile {
                    id: "cusa03173".into(),
                    label: "Bloodborne CUSA03173 version 1.09".into(),
                    kind: SetupKind::Folder,
                    destination: Some("CUSA03173".into()),
                    contains: vec!["eboot.bin".into()],
                    ..SetupFile::default()
                }],
                ..LauncherRecipe::default()
            },
            ..CatalogEntry::default()
        }
    }

    #[test]
    fn overlay_parses_the_documented_shape() {
        let overlay = Overlay::parse(&overlay_toml()).expect("valid overlay");
        assert_eq!(overlay.schema_version, SUPPORTED_SCHEMA_VERSION);
        assert_eq!(overlay.extractor.len(), 1);
        assert_eq!(overlay.extractor[0].id, "ps4-pkg");
        assert_eq!(overlay.extractor[0].args.len(), 6);

        let entry = &overlay.entry["qcat_lantern"];
        assert_eq!(entry.gamenode_game_id, Some(2100));
        assert_eq!(entry.launcher.as_deref(), Some("Lantern Engine.exe"));
        assert_eq!(entry.gameplay, Some(vec!["Bloodborne.exe".to_string()]));
        let slots = entry.setup_file.as_ref().expect("setup files");
        assert_eq!(slots.len(), 1);
        let slot = slots[0].to_setup_file();
        assert_eq!(slot.kind, SetupKind::Folder);
        assert!(slot.required, "an omitted required means required");
        assert_eq!(slot.hash_file.as_deref(), Some("eboot.bin"));
        assert_eq!(
            slot.from_package.as_ref().map(|p| p.extractor.as_str()),
            Some("ps4-pkg")
        );
        // The overlay writes snake_case, while the plan and view model serialize camelCase.
        let package = slot.from_package.as_ref().expect("a package option");
        assert_eq!(package.title_id.as_deref(), Some("CUSA03173"));
        assert_eq!(package.needs_update.as_deref(), Some("01.09"));
    }

    #[test]
    fn merging_twice_changes_nothing() {
        let overlay = Overlay::parse(&overlay_toml()).unwrap();
        let once = overlay.merge(upstream_entry()).unwrap();
        let twice = overlay.merge(once.clone()).unwrap();
        assert_eq!(twice, once);
        assert_eq!(twice.extractors.len(), 1);
    }

    #[test]
    fn recipe_replacement_uses_the_whole_overlay_list() {
        let overlay = Overlay::parse(&overlay_toml()).unwrap();
        let merged = overlay.merge(upstream_entry()).expect("entry is visible");
        assert_eq!(merged.launcher.setup_files.len(), 1);
        let slot = &merged.launcher.setup_files[0];
        assert_eq!(slot.id, "game");
        assert_eq!(slot.label, "Bloodborne (CUSA03173) with update 01.09");
        // Upstream launcher fields the overlay does not restate survive.
        assert_eq!(
            merged.launcher.folder_name.as_deref(),
            Some("Bloodborne-BloodbornePcWindowsPort")
        );
        assert_eq!(
            merged.launcher.release_asset_filter.as_deref(),
            Some("windows_port")
        );
        // The referenced extractor is pinned on the merged entry for the install worker.
        assert_eq!(merged.extractors.len(), 1);
        assert_eq!(merged.extractors[0].id, "ps4-pkg");
    }

    #[test]
    fn an_overlay_can_remove_guided_setup() {
        let overlay = Overlay {
            schema_version: 1,
            entry: BTreeMap::from([(
                "qcat_lantern".to_string(),
                OverlayEntry {
                    setup_file: Some(Vec::new()),
                    ..OverlayEntry::default()
                },
            )]),
            ..Overlay::default()
        };
        overlay.validate().unwrap();
        let merged = overlay.merge(upstream_entry()).unwrap();
        assert!(merged.launcher.setup_files.is_empty());
        assert!(!merged.has_guided_setup());
        assert!(merged.extractors.is_empty());
    }

    #[test]
    fn hidden_entries_leave_the_list() {
        let hidden = Overlay {
            schema_version: 1,
            entry: BTreeMap::from([(
                "qcat_lantern".to_string(),
                OverlayEntry {
                    hidden: true,
                    ..OverlayEntry::default()
                },
            )]),
            ..Overlay::default()
        };
        hidden.validate().unwrap();
        assert!(hidden.merge(upstream_entry()).is_none());
        assert!(hidden.apply(vec![upstream_entry()]).is_empty());
    }

    #[test]
    fn executable_roles_follow_the_overlay() {
        let overlay = Overlay::parse(&overlay_toml()).unwrap();
        let merged = overlay.merge(upstream_entry()).unwrap();
        let roles = merged.executable_roles();
        assert_eq!(roles.executable.as_deref(), Some("Lantern Engine.exe"));
        assert_eq!(roles.launchers, vec!["Lantern Engine.exe".to_string()]);
        assert_eq!(roles.gameplay, vec!["Bloodborne.exe".to_string()]);

        let plain = Overlay::default()
            .merge(upstream_entry())
            .expect("no overlay keeps the entry");
        let roles = plain.executable_roles();
        assert_eq!(roles.executable.as_deref(), Some("Bloodborne.exe"));
        assert_eq!(roles.launchers, vec!["Bloodborne.exe".to_string()]);
        assert!(roles.gameplay.is_empty());
    }

    #[test]
    fn a_zero_game_id_is_a_placeholder_not_an_identity() {
        let overlay = Overlay {
            schema_version: 1,
            entry: BTreeMap::from([(
                "qcat_lantern".to_string(),
                OverlayEntry {
                    gamenode_game_id: Some(0),
                    ..OverlayEntry::default()
                },
            )]),
            ..Overlay::default()
        };
        let merged = overlay.merge(upstream_entry()).unwrap();
        assert_eq!(merged.gamenode_game_id, None);
    }

    #[test]
    fn scalars_replace_upstream_values() {
        let overlay = Overlay {
            schema_version: 1,
            entry: BTreeMap::from([(
                "qcat_lantern".to_string(),
                OverlayEntry {
                    gamenode_game_id: Some(7),
                    launcher: Some("Lantern.exe".into()),
                    gameplay: Some(vec!["Game.exe".into()]),
                    license: Some("MIT".into()),
                    release: Some("v0.9.38".into()),
                    edition: Some("Director's Cut".into()),
                    ..OverlayEntry::default()
                },
            )]),
            ..Overlay::default()
        };
        let merged = overlay.merge(upstream_entry()).unwrap();
        assert_eq!(merged.gamenode_game_id, Some(7));
        assert_eq!(merged.launcher_executable.as_deref(), Some("Lantern.exe"));
        assert_eq!(merged.gameplay_executables, vec!["Game.exe".to_string()]);
        assert_eq!(merged.license.as_deref(), Some("MIT"));
        assert_eq!(merged.pinned_release.as_deref(), Some("v0.9.38"));
        assert_eq!(merged.edition.as_deref(), Some("Director's Cut"));
    }

    #[test]
    fn the_listed_catalog_is_windows_only_and_keeps_order() {
        let overlay = Overlay::parse(&overlay_toml()).unwrap();
        let linux = CatalogEntry {
            catalog_id: "qcat_linux".into(),
            slug: "linux".into(),
            name: "Linux only".into(),
            supported_os: vec!["linux".into()],
            ..CatalogEntry::default()
        };
        let unsupported = CatalogEntry {
            catalog_id: "qcat_unknown".into(),
            slug: "unknown".into(),
            name: "Unknown".into(),
            supported_os: vec!["unknown".into()],
            ..CatalogEntry::default()
        };
        let listed = overlay.apply(vec![linux, upstream_entry(), unsupported]);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].catalog_id, "qcat_lantern");
    }

    #[test]
    fn unknown_major_schema_versions_are_rejected_whole() {
        let newer = overlay_toml().replace("schema_version = 1", "schema_version = 2");
        assert_eq!(
            Overlay::parse(&newer),
            Err(OverlayError::UnsupportedSchema {
                found: 2,
                supported: SUPPORTED_SCHEMA_VERSION
            })
        );
        let missing = overlay_toml().replace("schema_version = 1", "");
        assert_eq!(
            Overlay::parse(&missing),
            Err(OverlayError::MissingSchemaVersion)
        );
    }

    #[test]
    fn invalid_overlays_name_the_problem() {
        let bad_toml = "schema_version = ";
        assert!(matches!(
            Overlay::parse(bad_toml),
            Err(OverlayError::Parse(_))
        ));

        let unknown_extractor =
            overlay_toml().replace("extractor = \"ps4-pkg\"", "extractor = \"nope\"");
        assert!(matches!(
            Overlay::parse(&unknown_extractor),
            Err(OverlayError::UnknownExtractor { .. })
        ));

        let bad_hash = overlay_toml().replace(
            &format!("value = \"{}\"", digest()),
            "value = \"not-a-digest\"",
        );
        assert!(matches!(
            Overlay::parse(&bad_hash),
            Err(OverlayError::InvalidSlot { .. })
        ));

        let escaping = overlay_toml().replace(
            "destination = \"CUSA03173\"",
            "destination = \"../CUSA03173\"",
        );
        assert!(matches!(
            Overlay::parse(&escaping),
            Err(OverlayError::InvalidSlot {
                error: RecipeError::UnsafeDestination { .. },
                ..
            })
        ));

        let unsafe_launcher = overlay_toml().replace(
            "launcher = \"Lantern Engine.exe\"",
            "launcher = \"..\\\\Lantern Engine.exe\"",
        );
        assert!(matches!(
            Overlay::parse(&unsafe_launcher),
            Err(OverlayError::InvalidPath { .. })
        ));

        let duplicate_slot = overlay_toml().replace(
            "from_package = { extractor = \"ps4-pkg\", title_id = \"CUSA03173\", needs_update = \"01.09\" }",
            "from_package = { extractor = \"ps4-pkg\" }\n\n[[entry.\"qcat_lantern\".setup_file]]\nid = \"game\"\nkind = \"folder\"\ndestination = \"CUSA03173\"",
        );
        assert!(matches!(
            Overlay::parse(&duplicate_slot),
            Err(OverlayError::DuplicateSlot { .. })
        ));

        let duplicate_extractor = format!(
            "{}\n[[extractor]]\nid = \"ps4-pkg\"\nname = \"x\"\nrepository = \"a/b\"\ntag = \"v\"\nasset = \"a.zip\"\nsha256 = \"{}\"\nlicense = \"MIT\"\nexecutable = \"x.exe\"\n",
            overlay_toml(),
            digest()
        );
        assert!(matches!(
            Overlay::parse(&duplicate_extractor),
            Err(OverlayError::DuplicateExtractor { .. })
        ));
    }

    #[test]
    fn overlay_without_entries_is_valid() {
        let overlay = Overlay::parse("schema_version = 1").expect("empty overlay is valid");
        assert!(overlay.entry.is_empty());
        assert_eq!(overlay.apply(vec![upstream_entry()]).len(), 1);
    }

    /// The example in the authoring guide is a fixture: it must keep parsing and validating
    /// through the same loader contributors will use, or the documented shape has drifted.
    #[test]
    fn the_documented_overlay_example_parses_and_validates() {
        const DOC: &str = include_str!("../../../../docs/pc-ports.md");
        const MARKER: &str = "<!-- overlay-example -->";
        let (_, after_marker) = DOC
            .split_once(MARKER)
            .expect("docs/pc-ports.md carries the overlay example marker");
        let fence_open = "```toml\n";
        let start = after_marker
            .find(fence_open)
            .expect("a TOML fence follows the overlay example marker")
            + fence_open.len();
        let rest = &after_marker[start..];
        let end = rest
            .find("\n```")
            .expect("the overlay example's TOML fence is closed");
        let example = &rest[..end];

        let overlay = Overlay::parse(example).expect("the documented overlay example is valid");
        assert_eq!(overlay.schema_version, SUPPORTED_SCHEMA_VERSION);

        let entry = overlay
            .entry
            .get("qcat_example_port")
            .expect("the example documents a fictional catalog entry");
        assert_eq!(entry.launcher.as_deref(), Some("Example Game Launcher.exe"));
        let slot = entry
            .setup_file
            .as_ref()
            .and_then(|slots| slots.first())
            .expect("the example documents a guided setup slot");
        assert_eq!(
            slot.from_package
                .as_ref()
                .map(|package| package.extractor.as_str()),
            Some("example-pkg-tool")
        );
        assert!(
            overlay.extractor("example-pkg-tool").is_some(),
            "the slot's extractor is declared in the example"
        );
    }
}
