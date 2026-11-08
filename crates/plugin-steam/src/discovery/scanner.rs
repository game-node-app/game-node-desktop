//! Plugin-owned Steam library manifest scanner.

use std::collections::BTreeSet;

use super::keyvalues::{Value, parse_root};
use super::{Files, RawRecord, ScanInput};

use super::path;

// Preserve the baseline scanner's single-component rule on the native OS.
fn is_install_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && std::path::Path::new(name).components().count() == 1
}

fn listed_libraries(files: &dyn Files, root: &str) -> Vec<String> {
    let candidates = [
        path::join(&path::join(root, "steamapps"), "libraryfolders.vdf"),
        path::join(&path::join(root, "config"), "libraryfolders.vdf"),
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

fn read_game(files: &dyn Files, library: &str, file_name: &str) -> Option<RawRecord> {
    let steamapps = path::join(library, "steamapps");
    let file = path::join(&steamapps, file_name);
    let text = match files.read_to_string(&file) {
        Ok(text) => text,
        Err(error) => {
            tracing::warn!(%error, file, "could not read a Steam app manifest");
            return None;
        }
    };
    let state = match parse_root(&text, "AppState") {
        Ok(state) => state,
        Err(error) => {
            tracing::warn!(%error, file, "ignored a malformed Steam app manifest");
            return None;
        }
    };
    let id = state.text("appid")?.trim();
    if id.is_empty() || id.len() > 20 || !id.bytes().all(|b| b.is_ascii_digit()) {
        tracing::warn!(file, "ignored a Steam manifest without a numeric app id");
        return None;
    }
    let install_name = state.text("installdir")?.trim();
    if !is_install_name(install_name) {
        tracing::warn!(
            file,
            "ignored a Steam manifest with an unusable install folder"
        );
        return None;
    }
    let install_dir = path::join(&path::join(&steamapps, "common"), install_name);
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

pub fn scan(input: &ScanInput<'_>) -> Result<Vec<RawRecord>, String> {
    let mut seen_roots = BTreeSet::new();
    let mut seen_libraries = BTreeSet::new();
    let mut records = Vec::new();
    for root in input.roots {
        let Ok(root) = input.files.canonicalize(root) else {
            continue;
        };
        if !seen_roots.insert(path::normalize(&root).to_lowercase()) {
            continue;
        }
        let mut libraries = vec![root.clone()];
        libraries.extend(listed_libraries(input.files, &root));
        for library in libraries {
            let Ok(library) = input.files.canonicalize(&library) else {
                continue;
            };
            if !seen_libraries.insert(path::normalize(&library).to_lowercase()) {
                continue;
            }
            let steamapps = path::join(&library, "steamapps");
            let Ok(mut names) = input.files.read_dir(&steamapps) else {
                continue;
            };
            names.sort();
            for name in names {
                let lower = name.to_ascii_lowercase();
                if lower.starts_with("appmanifest_") && lower.ends_with(".acf") {
                    records.extend(read_game(input.files, &library, &name));
                }
            }
        }
    }
    Ok(records)
}
