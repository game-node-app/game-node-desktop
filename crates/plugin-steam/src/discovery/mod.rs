//! Plugin-owned installed discovery and counters, independent of Core/OS ports.

use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::Path,
};
pub mod counter;
mod keyvalues;
#[cfg(test)]
mod memory;
mod path;
mod registry;
mod scanner;
#[cfg(test)]
mod tests;

pub trait Files: Send + Sync {
    fn canonicalize(&self, path: &str) -> io::Result<String>;
    fn read_dir(&self, path: &str) -> io::Result<Vec<String>>;
    fn read_to_string(&self, path: &str) -> io::Result<String>;
    fn is_dir(&self, path: &str) -> bool;
}

pub trait RootResolver {
    fn expand_home(&self, path: &str) -> Option<String>;
    fn registry_string(&self, key: &str, value: &str) -> Option<String>;
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Windows,
    Linux,
}

#[derive(Clone, Deserialize)]
pub struct Action {
    pub uri: String,
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
    #[serde(default)]
    linux: Vec<Root>,
}

/// Only provider-owned configuration crosses into this implementation. The host
/// validates the complete manifest before sending it through provider context.
#[derive(Clone, Deserialize)]
pub struct Manifest {
    pub id: String,
    #[serde(default)]
    roots: Roots,
    #[serde(default)]
    pub exclude_ids: Vec<String>,
    pub launch: Option<Action>,
    pub install: Option<Action>,
}

#[derive(Default)]
struct RawRecord {
    id: Option<String>,
    name: Option<String>,
    install_dir: Option<String>,
    fields: BTreeMap<String, String>,
}

pub struct ScanInput<'a> {
    pub files: &'a dyn Files,
    pub roots: &'a [String],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    pub id: String,
    pub name: Option<String>,
    pub install_dir: String,
    pub fields: BTreeMap<String, String>,
}

impl Manifest {
    pub fn resolve_roots(&self, platform: Platform, resolver: &dyn RootResolver) -> Vec<String> {
        let roots = match platform {
            Platform::Windows => &self.roots.windows,
            Platform::Linux => &self.roots.linux,
        };
        roots
            .iter()
            .filter_map(|root| match (&root.path, &root.registry, &root.value) {
                (Some(path), None, None) => resolver.expand_home(path),
                (None, Some(key), Some(value)) => {
                    resolver.registry_string(key.strip_prefix("HKCU\\").unwrap_or(key), value)
                }
                _ => None,
            })
            .filter(|path| !path.trim().is_empty())
            .collect()
    }

    pub fn scan(
        &self,
        platform: Platform,
        files: &dyn Files,
        resolver: &dyn RootResolver,
    ) -> Result<Vec<Installed>, String> {
        let roots = self.resolve_roots(platform, resolver);
        let raw = scanner::scan(&ScanInput {
            files,
            roots: &roots,
        })?;
        let mut seen = BTreeSet::new();
        Ok(raw
            .into_iter()
            .filter_map(|record| {
                let id = record.id?.trim().to_owned();
                let install_dir = record.install_dir?.trim().to_owned();
                if id.is_empty()
                    || id.chars().any(char::is_control)
                    || install_dir.is_empty()
                    || self.exclude_ids.contains(&id)
                    || !seen.insert(id.clone())
                {
                    return None;
                }
                Some(Installed {
                    id,
                    install_dir,
                    name: record
                        .name
                        .map(|name| name.trim().to_owned())
                        .filter(|name| !name.is_empty()),
                    fields: record.fields,
                })
            })
            .collect())
    }
}

pub struct SystemRoots;
impl RootResolver for SystemRoots {
    fn expand_home(&self, path: &str) -> Option<String> {
        match path.strip_prefix('~') {
            None => Some(path.to_owned()),
            Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => dirs::home_dir()
                .and_then(|home| home.into_os_string().into_string().ok())
                .map(|home| format!("{}{}", home.trim_end_matches(['/', '\\']), rest)),
            _ => None,
        }
    }
    fn registry_string(&self, key: &str, value: &str) -> Option<String> {
        registry::platform::read_current_user_string(key, value)
    }
}

pub struct StdFiles;
impl Files for StdFiles {
    fn canonicalize(&self, path: &str) -> io::Result<String> {
        std::fs::canonicalize(path)?
            .into_os_string()
            .into_string()
            .map(|path| self::path::normalize(&path))
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "non-text path"))
    }
    fn read_dir(&self, path: &str) -> io::Result<Vec<String>> {
        let mut names = vec![];
        for entry in std::fs::read_dir(path)? {
            if let Ok(name) = entry?.file_name().into_string() {
                names.push(name);
            }
        }
        Ok(names)
    }
    fn read_to_string(&self, path: &str) -> io::Result<String> {
        std::fs::read_to_string(path)
    }
    fn is_dir(&self, path: &str) -> bool {
        Path::new(path).is_dir()
    }
}
