//! In-memory [`HostFiles`] and [`RootResolver`] doubles for tests.
//!
//! Paths may be Windows- or Unix-style, so scanner fixtures with `C:\Games` run on any OS. Windows
//! paths compare case-insensitively, like the real file system.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Error, ErrorKind, Result};

use super::path::{is_windows_style, normalize};
use crate::ports::{HostFiles, RootResolver};

#[derive(Clone, Debug, Default)]
pub struct MemoryFiles {
    files: BTreeMap<String, String>,
    dirs: BTreeSet<String>,
    links: BTreeMap<String, String>,
    unreadable: BTreeSet<String>,
}

fn key(path: &str) -> String {
    let normalized = normalize(path);
    if is_windows_style(&normalized) {
        normalized.to_lowercase()
    } else {
        normalized
    }
}

fn parent(path: &str) -> Option<String> {
    let normalized = normalize(path);
    let cut = normalized.rfind(['\\', '/'])?;
    if cut == 0 {
        return (normalized.len() > 1).then(|| "/".to_owned());
    }
    let parent = &normalized[..cut];
    let parent = if parent.ends_with(':') {
        format!("{parent}\\")
    } else {
        parent.to_owned()
    };
    // A root is its own parent; stop there.
    (!parent.is_empty() && parent != normalized).then_some(parent)
}

impl MemoryFiles {
    pub fn new() -> Self {
        Self::default()
    }

    fn add_dirs(&mut self, path: &str) {
        let mut current = Some(normalize(path));
        while let Some(dir) = current {
            if !self.dirs.insert(key(&dir)) {
                break;
            }
            current = parent(&dir);
        }
    }

    /// Adds a file and every folder above it.
    pub fn with_file(mut self, path: &str, contents: &str) -> Self {
        self.files.insert(key(path), contents.to_owned());
        if let Some(dir) = parent(path) {
            self.add_dirs(&dir);
        }
        self
    }

    /// Adds an (possibly empty) folder and every folder above it.
    pub fn with_dir(mut self, path: &str) -> Self {
        self.add_dirs(path);
        self
    }

    /// Makes `link` resolve to `target` when canonicalized. The link itself exists as a folder.
    pub fn with_link(mut self, link: &str, target: &str) -> Self {
        self.links.insert(key(link), normalize(target));
        self.add_dirs(link);
        self
    }

    /// Makes reading `path` fail with a permission error although it exists.
    pub fn with_unreadable(mut self, path: &str) -> Self {
        self.unreadable.insert(key(path));
        self
    }

    fn resolve(&self, path: &str) -> String {
        // Resolve the longest linked prefix, repeatedly (links to links), with a hop limit.
        let mut current = normalize(path);
        for _ in 0..8 {
            let mut changed = false;
            let mut prefix = Some(current.clone());
            while let Some(candidate) = prefix {
                if let Some(target) = self.links.get(&key(&candidate)) {
                    let rest = &current[candidate.len()..];
                    current = normalize(&format!("{target}{rest}"));
                    changed = true;
                    break;
                }
                prefix = parent(&candidate);
            }
            if !changed {
                break;
            }
        }
        current
    }
}

fn not_found() -> Error {
    Error::new(ErrorKind::NotFound, "not found")
}

impl HostFiles for MemoryFiles {
    fn read_to_string(&self, path: &str) -> Result<String> {
        let resolved = self.resolve(path);
        if self.unreadable.contains(&key(&resolved)) {
            return Err(Error::new(ErrorKind::PermissionDenied, "denied"));
        }
        self.files
            .get(&key(&resolved))
            .cloned()
            .ok_or_else(not_found)
    }

    fn read_dir(&self, path: &str) -> Result<Vec<String>> {
        let resolved = self.resolve(path);
        if self.unreadable.contains(&key(&resolved)) {
            return Err(Error::new(ErrorKind::PermissionDenied, "denied"));
        }
        if !self.dirs.contains(&key(&resolved)) {
            return Err(not_found());
        }
        let wanted = key(&resolved);
        let mut names = BTreeSet::new();
        // Windows-style entries are stored lowercase, so their listed names are lowercase too.
        for candidate in self.files.keys().chain(&self.dirs) {
            if let Some(name) = child_name(candidate, &wanted) {
                names.insert(name);
            }
        }
        Ok(names.into_iter().collect())
    }

    fn is_dir(&self, path: &str) -> bool {
        self.dirs.contains(&key(&self.resolve(path)))
    }

    fn canonicalize(&self, path: &str) -> Result<String> {
        let resolved = self.resolve(path);
        let k = key(&resolved);
        if self.dirs.contains(&k) || self.files.contains_key(&k) {
            Ok(resolved)
        } else {
            Err(not_found())
        }
    }
}

/// The immediate child name of `candidate` inside `dir`, both already keys.
fn child_name(candidate: &str, dir: &str) -> Option<String> {
    let rest = candidate.strip_prefix(dir)?;
    let rest = rest
        .strip_prefix(['\\', '/'])
        .or(if dir.ends_with(['\\', '/']) {
            Some(rest)
        } else {
            None
        })?;
    if rest.is_empty() || rest.contains(['\\', '/']) {
        return None;
    }
    Some(rest.to_owned())
}

/// A resolver with fixed answers.
#[derive(Clone, Debug, Default)]
pub struct FixedRoots {
    pub home: Option<String>,
    pub registry: BTreeMap<(String, String), String>,
}

impl FixedRoots {
    pub fn with_home(home: &str) -> Self {
        Self {
            home: Some(home.to_owned()),
            registry: BTreeMap::new(),
        }
    }

    pub fn with_registry(mut self, key: &str, value: &str, data: &str) -> Self {
        self.registry
            .insert((key.to_owned(), value.to_owned()), data.to_owned());
        self
    }
}

impl RootResolver for FixedRoots {
    fn expand_home(&self, path: &str) -> Option<String> {
        match path.strip_prefix('~') {
            None => Some(path.to_owned()),
            Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
                let home = self.home.as_ref()?;
                Some(format!("{}{}", home.trim_end_matches(['/', '\\']), rest))
            }
            Some(_) => None,
        }
    }

    fn registry_string(&self, key: &str, value: &str) -> Option<String> {
        self.registry
            .get(&(key.to_owned(), value.to_owned()))
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_files_and_lists_directories_in_both_styles() {
        let files = MemoryFiles::new()
            .with_file("C:\\Steam\\steamapps\\appmanifest_1.acf", "one")
            .with_file("C:/Steam/steamapps/appmanifest_2.acf", "two")
            .with_dir("C:\\Steam\\steamapps\\common\\Game")
            .with_file("/home/me/a.txt", "unix");

        assert_eq!(
            files
                .read_to_string("c:\\steam\\STEAMAPPS\\appmanifest_1.acf")
                .unwrap(),
            "one"
        );
        assert_eq!(files.read_to_string("/home/me/a.txt").unwrap(), "unix");
        assert_eq!(
            files.read_dir("C:\\Steam\\steamapps").unwrap(),
            vec!["appmanifest_1.acf", "appmanifest_2.acf", "common"]
        );
        assert!(files.is_dir("C:/Steam/steamapps/common/Game"));
        assert!(!files.is_dir("C:\\Steam\\steamapps\\appmanifest_1.acf"));
        assert!(files.read_dir("C:\\Nowhere").is_err());
        assert_eq!(
            files.read_to_string("/home/me/missing").unwrap_err().kind(),
            ErrorKind::NotFound
        );
    }

    #[test]
    fn canonicalizes_a_linked_path() {
        let files = MemoryFiles::new()
            .with_file(
                "/home/me/.local/share/Steam/steamapps/libraryfolders.vdf",
                "x",
            )
            .with_link("/home/me/.steam/steam", "/home/me/.local/share/Steam");

        assert_eq!(
            files.canonicalize("/home/me/.steam/steam").unwrap(),
            "/home/me/.local/share/Steam"
        );
        assert_eq!(
            files
                .read_to_string("/home/me/.steam/steam/steamapps/libraryfolders.vdf")
                .unwrap(),
            "x"
        );
        assert!(files.canonicalize("/home/me/.steam/missing").is_err());
    }

    #[test]
    fn unreadable_paths_fail_with_permission_denied() {
        let files = MemoryFiles::new()
            .with_file("D:\\Lib\\steamapps\\appmanifest_9.acf", "x")
            .with_unreadable("D:\\Lib\\steamapps\\appmanifest_9.acf");
        assert_eq!(
            files
                .read_to_string("D:\\Lib\\steamapps\\appmanifest_9.acf")
                .unwrap_err()
                .kind(),
            ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn fixed_roots_expand_home_only_for_a_leading_tilde() {
        let roots = FixedRoots::with_home("/home/me").with_registry("k", "v", "C:/Steam");
        assert_eq!(
            roots.expand_home("~/.steam/steam").unwrap(),
            "/home/me/.steam/steam"
        );
        assert_eq!(roots.expand_home("/opt/steam").unwrap(), "/opt/steam");
        assert_eq!(roots.expand_home("~other/x"), None);
        assert_eq!(roots.registry_string("k", "v").unwrap(), "C:/Steam");
        assert_eq!(roots.registry_string("k", "missing"), None);
    }
}
