//! Cover images on disk: `<covers dir>/<game_id>.jpg`.
//!
//! A cover is written to a temporary file in the same folder, synced, and renamed over the final
//! name, so a reader never sees half an image and a failed write leaves the previous cover alone.
//! Covers are keyed by game id: changing an entry's match just points it at another file.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

/// The file name a game's cover is stored under.
pub fn cover_file_name(game_id: u64) -> String {
    format!("{game_id}.jpg")
}

/// Writes a game's cover atomically and returns its file name.
pub fn write_cover(dir: &Path, game_id: u64, bytes: &[u8]) -> io::Result<String> {
    fs::create_dir_all(dir)?;
    let name = cover_file_name(game_id);
    let temp = dir.join(format!(".{name}.{}.part", std::process::id()));

    let written = (|| {
        let mut file = File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, dir.join(&name))
    })();
    if written.is_err() {
        // Best effort: a leftover `.part` file is harmless but should not pile up.
        let _ = fs::remove_file(&temp);
    }
    written.map(|()| name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_cover_is_written_under_its_game_id_and_leaves_nothing_else_behind() {
        let dir = TempDir::new().unwrap();
        // The folder is created on first use.
        let covers = dir.path().join("covers");
        let name = write_cover(&covers, 7, b"\xFF\xD8\xFFimage").unwrap();
        assert_eq!(name, "7.jpg");
        assert_eq!(
            fs::read(covers.join("7.jpg")).unwrap(),
            b"\xFF\xD8\xFFimage"
        );
        assert_eq!(files(&covers), ["7.jpg"]);
    }

    #[test]
    fn a_new_cover_replaces_the_old_one_whole() {
        let dir = TempDir::new().unwrap();
        write_cover(dir.path(), 7, b"old cover, longer than the new one").unwrap();
        write_cover(dir.path(), 7, b"new").unwrap();
        assert_eq!(fs::read(dir.path().join("7.jpg")).unwrap(), b"new");
        assert_eq!(files(dir.path()), ["7.jpg"]);
    }

    #[test]
    fn different_games_have_different_files() {
        let dir = TempDir::new().unwrap();
        write_cover(dir.path(), 7, b"seven").unwrap();
        write_cover(dir.path(), 8, b"eight").unwrap();
        assert_eq!(files(dir.path()), ["7.jpg", "8.jpg"]);
        assert_eq!(fs::read(dir.path().join("7.jpg")).unwrap(), b"seven");
    }

    #[test]
    fn a_failed_write_reports_it_and_cleans_up() {
        let dir = TempDir::new().unwrap();
        // The final name is taken by a folder, so the rename cannot succeed.
        fs::create_dir(dir.path().join("7.jpg")).unwrap();
        assert!(write_cover(dir.path(), 7, b"image").is_err());
        assert_eq!(files(dir.path()), ["7.jpg"], "no temporary file is left");
        assert!(dir.path().join("7.jpg").is_dir());

        // A covers path that is a file cannot hold covers at all.
        let blocked = dir.path().join("not-a-folder");
        fs::write(&blocked, b"x").unwrap();
        assert!(write_cover(&blocked, 7, b"image").is_err());
    }
}
