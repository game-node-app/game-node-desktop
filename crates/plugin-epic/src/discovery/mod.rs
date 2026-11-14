//! Plugin-owned Epic Games Launcher installed discovery, independent of Core and OS ports.
//!
//! The launcher data directory is resolved from the trusted provider manifest roots: the registry
//! `AppDataPath` value first, then the `%ProgramData%` fallback. The plugin expands environment
//! variables itself because the host only forwards the manifest. Every blocking filesystem or
//! registry read runs on a worker supplied by the runtime (`spawn_blocking`).

use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, io,
    path::{Path, PathBuf},
};

pub trait Files: Send + Sync {
    fn read_dir(&self, path: &Path) -> io::Result<Vec<String>>;
    fn read_to_string(&self, path: &Path) -> io::Result<String>;
    fn is_dir(&self, path: &Path) -> bool;
}

pub trait RootResolver {
    /// Expands the plugin-owned environment placeholders in a root path.
    fn expand_env(&self, path: &str) -> Option<String>;
    /// Reads a Windows registry string. A key is `HIVE\subkey`, where the hive is `HKLM`,
    /// `HKEY_LOCAL_MACHINE`, `HKCU` or `HKEY_CURRENT_USER`.
    fn registry_string(&self, key: &str, value: &str) -> Option<String>;
}

#[derive(Clone, Default, Deserialize)]
pub struct Action {
    #[serde(default)]
    pub uri: Option<String>,
}

#[derive(Clone, Default, Deserialize)]
struct Root {
    path: Option<String>,
    registry: Option<String>,
    value: Option<String>,
}

#[derive(Clone, Default, Deserialize)]
struct Roots {
    #[serde(default)]
    windows: Vec<Root>,
}

/// Only the provider-owned manifest fields this plugin interprets. The host validates the complete
/// manifest before sending it through provider context.
#[derive(Clone, Deserialize)]
pub struct Manifest {
    pub id: String,
    #[serde(default)]
    roots: Roots,
    #[serde(default)]
    pub launch: Option<Action>,
    #[serde(default)]
    pub install: Option<Action>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    pub id: String,
    pub name: Option<String>,
    pub install_dir: String,
    pub fields: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanError {
    /// The data directory exists but its `Manifests` directory cannot be listed.
    Unreadable,
}

impl fmt::Display for ScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable => f.write_str("the Epic manifest directory could not be read"),
        }
    }
}

impl std::error::Error for ScanError {}

impl Manifest {
    /// Candidate data directories in declaration order. The registry value is the primary root and
    /// the portable path is the fallback; discovery uses the first candidate that exists.
    pub fn resolve_roots(&self, resolver: &dyn RootResolver) -> Vec<String> {
        self.roots
            .windows
            .iter()
            .filter_map(|root| match (&root.path, &root.registry, &root.value) {
                (Some(path), None, None) => resolver.expand_env(path),
                (None, Some(key), Some(value)) => {
                    resolver.registry_string(key, value).and_then(|path| {
                        // Some launchers write an unexpanded value; expansion is harmless for an
                        // absolute path and keeps the registry and fallback handling identical.
                        resolver.expand_env(&path)
                    })
                }
                _ => None,
            })
            .filter(|path| !path.trim().is_empty())
            .collect()
    }

    /// Reads the installed-item manifests of the first existing data directory.
    ///
    /// A missing launcher data directory is a successful empty result. A data directory without a
    /// `Manifests` directory is also empty. A `Manifests` directory that exists but cannot be read
    /// is a failure, so previous installed facts are kept.
    pub fn scan(
        &self,
        files: &dyn Files,
        resolver: &dyn RootResolver,
    ) -> Result<Vec<Installed>, ScanError> {
        for root in self.resolve_roots(resolver) {
            let root = PathBuf::from(root);
            if files.is_dir(&root) {
                return scan_data_dir(files, &root);
            }
        }
        Ok(Vec::new())
    }
}

fn scan_data_dir(files: &dyn Files, data: &Path) -> Result<Vec<Installed>, ScanError> {
    let manifests = data.join("Manifests");
    let mut names = match files.read_dir(&manifests) {
        Ok(names) => names,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(ScanError::Unreadable),
    };
    names.sort();

    let mut seen = BTreeSet::new();
    let mut installed = Vec::new();
    let mut malformed = 0usize;
    let mut filtered = 0usize;
    for name in names {
        if !name.to_ascii_lowercase().ends_with(".item") {
            continue;
        }
        let text = match files.read_to_string(&manifests.join(&name)) {
            Ok(text) => text,
            Err(_) => {
                malformed += 1;
                continue;
            }
        };
        let Some(item) = Item::from_json(&text) else {
            malformed += 1;
            continue;
        };
        let Some(record) = item.into_installed() else {
            filtered += 1;
            continue;
        };
        if !seen.insert(record.id.clone()) {
            continue;
        }
        installed.push(record);
    }
    if malformed > 0 {
        // Names and contents are not logged: both can carry a user name or an install path.
        tracing::warn!(malformed, "skipped malformed or unreadable Epic manifests");
    }
    if filtered > 0 {
        tracing::debug!(filtered, "skipped non-game Epic manifests");
    }
    Ok(installed)
}

/// One launcher manifest. Only the fields used by the documented filters are read.
#[derive(Deserialize)]
struct Item {
    #[serde(default, rename = "AppName")]
    app_name: Option<String>,
    #[serde(default, rename = "DisplayName")]
    display_name: Option<String>,
    #[serde(default, rename = "InstallLocation")]
    install_location: Option<String>,
    #[serde(default, rename = "bIsIncompleteInstall")]
    incomplete_install: bool,
    #[serde(default, rename = "MainGameAppName")]
    main_game_app_name: Option<String>,
    #[serde(default, rename = "AppCategories")]
    app_categories: Vec<String>,
    #[serde(default, rename = "CatalogNamespace")]
    catalog_namespace: Option<String>,
    #[serde(default, rename = "Namespace")]
    namespace: Option<String>,
    #[serde(default, rename = "CatalogItemId")]
    catalog_item_id: Option<String>,
}

impl Item {
    fn from_json(text: &str) -> Option<Self> {
        serde_json::from_str(text).ok()
    }

    fn into_installed(self) -> Option<Installed> {
        let id = trimmed(self.app_name)?;
        if id.chars().any(char::is_control) {
            return None;
        }
        let install_dir = trimmed(self.install_location)?;
        if self.incomplete_install {
            return None;
        }
        let main_game = self
            .main_game_app_name
            .as_deref()
            .map(str::trim)
            .unwrap_or_default();
        if !main_game.is_empty() && main_game != id {
            return None;
        }
        if !self
            .app_categories
            .iter()
            .any(|category| category == "games")
        {
            return None;
        }
        let namespace = trimmed(self.catalog_namespace);
        // Engine items carry `ue` in both the catalog and launcher namespace; check both so a
        // launcher build that fills only one still stays out of the library.
        let launcher_namespace = trimmed(self.namespace);
        if [namespace.as_deref(), launcher_namespace.as_deref()]
            .into_iter()
            .flatten()
            .any(|namespace| namespace.eq_ignore_ascii_case("ue"))
        {
            return None;
        }
        let catalog_item_id = trimmed(self.catalog_item_id);

        let mut fields = BTreeMap::new();
        if let Some(namespace) = namespace {
            fields.insert("namespace".to_owned(), namespace);
        }
        if let Some(catalog_item_id) = catalog_item_id {
            fields.insert("catalog_item_id".to_owned(), catalog_item_id);
        }
        Some(Installed {
            id,
            name: trimmed(self.display_name),
            install_dir,
            fields,
        })
    }
}

fn trimmed(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

pub struct SystemRoots;

impl RootResolver for SystemRoots {
    fn expand_env(&self, path: &str) -> Option<String> {
        expand_env_with(path, |name| std::env::var(name).ok())
    }

    fn registry_string(&self, key: &str, value: &str) -> Option<String> {
        registry::read_string(key, value)
    }
}

pub struct StdFiles;

impl Files for StdFiles {
    fn read_dir(&self, path: &Path) -> io::Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(path)? {
            if let Ok(name) = entry?.file_name().into_string() {
                names.push(name);
            }
        }
        Ok(names)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }
}

/// Expands every `%NAME%` placeholder with `lookup`. An undefined variable leaves the root
/// unresolved. A path without placeholders, or with a lone unmatched `%`, is returned unchanged.
fn expand_env_with(path: &str, lookup: impl Fn(&str) -> Option<String>) -> Option<String> {
    let mut expanded = String::with_capacity(path.len());
    let mut rest = path;
    while let Some(start) = rest.find('%') {
        expanded.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('%') else {
            expanded.push('%');
            expanded.push_str(after);
            return Some(expanded);
        };
        let name = &after[..end];
        if name.is_empty() {
            expanded.push_str("%%");
        } else {
            expanded.push_str(&lookup(name)?);
        }
        rest = &after[end + 1..];
    }
    expanded.push_str(rest);
    Some(expanded)
}

#[cfg(any(test, windows))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hive {
    LocalMachine,
    CurrentUser,
}

#[cfg(any(test, windows))]
fn split_registry_key(key: &str) -> Option<(Hive, &str)> {
    let (hive, subkey) = key.split_once('\\')?;
    let hive = match hive.to_ascii_uppercase().as_str() {
        "HKLM" | "HKEY_LOCAL_MACHINE" => Hive::LocalMachine,
        "HKCU" | "HKEY_CURRENT_USER" => Hive::CurrentUser,
        _ => return None,
    };
    (!subkey.is_empty()).then_some((hive, subkey))
}

mod registry {
    #[cfg(windows)]
    mod platform {
        use super::super::{Hive, split_registry_key};
        use winreg::{
            RegKey,
            enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ},
        };

        pub(crate) fn read_string(key: &str, value: &str) -> Option<String> {
            if key.contains('\0') || value.contains('\0') {
                return None;
            }
            let (hive, subkey) = split_registry_key(key)?;
            let hive = match hive {
                Hive::LocalMachine => HKEY_LOCAL_MACHINE,
                Hive::CurrentUser => HKEY_CURRENT_USER,
            };
            let key = RegKey::predef(hive)
                .open_subkey_with_flags(subkey, KEY_READ)
                .ok()?;
            let value: String = key.get_value(value).ok()?;
            (!value.trim().is_empty()).then_some(value)
        }
    }

    #[cfg(not(windows))]
    mod platform {
        pub(crate) fn read_string(_: &str, _: &str) -> Option<String> {
            None
        }
    }

    pub(crate) use platform::read_string;
}

#[cfg(test)]
mod tests;
