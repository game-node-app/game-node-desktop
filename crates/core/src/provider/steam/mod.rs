//! The native Steam scanner.
//!
//! Per root: canonicalize it (a missing root or a second path to the same folder is skipped),
//! read `libraryfolders.vdf` for the other library folders, then read every
//! `steamapps/appmanifest_*.acf` and keep games that have a numeric app id and an install folder.
//! Anything unreadable or malformed is skipped with a log so the other games still appear.

pub mod keyvalues;

use std::collections::BTreeSet;

use keyvalues::{Value, parse_root};

use super::path::{is_plain_name, join, normalize};
use super::record::RawRecord;
use super::registry::{ScanError, ScanInput};
use crate::ports::HostFiles;

/// Reads the library folders Steam lists, in either file format, as raw (uncanonicalized) paths.
fn listed_libraries(files: &dyn HostFiles, root: &str) -> Vec<String> {
    let candidates = [
        join(&join(root, "steamapps"), "libraryfolders.vdf"),
        join(&join(root, "config"), "libraryfolders.vdf"),
    ];
    let Some((file, text)) = candidates
        .iter()
        .find_map(|file| files.read_to_string(file).ok().map(|text| (file, text)))
    else {
        return Vec::new();
    };
    let folders = match parse_root(&text, "libraryfolders") {
        Ok(folders) => folders,
        Err(error) => {
            tracing::warn!(%error, file, "ignored a malformed libraryfolders file");
            return Vec::new();
        }
    };
    let mut paths = Vec::new();
    for (key, value) in folders.entries() {
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        // Current format: "0" { "path" "..." }. Old format: "1" "D:\\Games".
        let path = match value {
            Value::Block(_) => value.text("path"),
            Value::Text(text) => Some(text.as_str()),
        };
        if let Some(path) = path.filter(|path| !path.trim().is_empty()) {
            paths.push(path.to_owned());
        }
    }
    paths
}

/// One `appmanifest_*.acf`, or `None` (logged) if it is unusable.
fn read_game(files: &dyn HostFiles, library: &str, file_name: &str) -> Option<RawRecord> {
    let steamapps = join(library, "steamapps");
    let path = join(&steamapps, file_name);
    let text = match files.read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            tracing::warn!(%error, path, "could not read a Steam app manifest");
            return None;
        }
    };
    let state = match parse_root(&text, "AppState") {
        Ok(state) => state,
        Err(error) => {
            tracing::warn!(%error, path, "ignored a malformed Steam app manifest");
            return None;
        }
    };
    let id = state.text("appid")?.trim();
    if id.is_empty() || id.len() > 20 || !id.bytes().all(|b| b.is_ascii_digit()) {
        tracing::warn!(
            path,
            "ignored a Steam app manifest without a numeric app id"
        );
        return None;
    }
    let install_name = state.text("installdir")?.trim();
    if !is_plain_name(install_name) {
        tracing::warn!(
            path,
            "ignored a Steam app manifest with an unusable install folder"
        );
        return None;
    }
    let install_dir = join(&join(&steamapps, "common"), install_name);
    if !files.is_dir(&install_dir) {
        return None;
    }
    let mut record = RawRecord {
        id: Some(id.to_owned()),
        name: state.text("name").map(str::to_owned),
        install_dir: Some(install_dir),
        ..RawRecord::default()
    };
    if let Some(flags) = state.text("StateFlags") {
        record
            .fields
            .insert("state_flags".to_owned(), flags.trim().to_owned());
    }
    Some(record)
}

/// Scans every Steam root and returns one record per installed game.
pub fn scan(input: &ScanInput<'_>) -> Result<Vec<RawRecord>, ScanError> {
    let files = input.files;
    let mut seen_roots = BTreeSet::new();
    let mut seen_libraries = BTreeSet::new();
    let mut records = Vec::new();

    for root in input.roots {
        let Ok(root) = files.canonicalize(root) else {
            continue;
        };
        if !seen_roots.insert(normalize(&root).to_lowercase()) {
            continue;
        }

        let mut libraries = vec![root.clone()];
        libraries.extend(listed_libraries(files, &root));

        for library in libraries {
            // A library on an unplugged drive does not canonicalize and is skipped.
            let Ok(library) = files.canonicalize(&library) else {
                tracing::info!(
                    library,
                    "skipped a Steam library folder that is not available"
                );
                continue;
            };
            if !seen_libraries.insert(normalize(&library).to_lowercase()) {
                continue;
            }
            let steamapps = join(&library, "steamapps");
            let Ok(mut names) = files.read_dir(&steamapps) else {
                continue;
            };
            names.sort();
            for name in names {
                let lower = name.to_ascii_lowercase();
                if lower.starts_with("appmanifest_") && lower.ends_with(".acf") {
                    records.extend(read_game(files, &library, &name));
                }
            }
        }
    }
    Ok(records)
}

#[cfg(test)]
mod tests;
