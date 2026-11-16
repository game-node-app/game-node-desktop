//! Sign-in screen data read from the local cache.

use std::collections::HashSet;
use std::path::Path;
use std::time::SystemTime;

/// The most tiles the sign-in mural renders.
pub(super) const COVER_WALL_LIMIT: usize = 25;

const SAMPLE_PREFIX: &str = "signin-cover-";

/// Real IGDB cover art bundled with the application.
///
/// The catalogue titles and IGDB image URLs are recorded in
/// `ui/src/assets/signin-covers/manifest.json`; the same files are imported by the UI for the
/// mock/browser mural. Copying them into the cover cache lets the native custom protocol serve
/// them offline without a GameNode session, while the user's own cached covers stay first.
struct SampleCover {
    /// Name written in the cover cache and returned by [`cover_wall`].
    cache_file: &'static str,
    /// File name in `ui/src/assets/signin-covers/`, shared with the UI build.
    #[cfg_attr(not(test), allow(dead_code))]
    local_file: &'static str,
    bytes: &'static [u8],
}

const SAMPLE_COVERS: &[SampleCover] = &[
    SampleCover {
        cache_file: "signin-cover-elden-ring.jpg",
        local_file: "elden-ring.jpg",
        bytes: include_bytes!("../../../../ui/src/assets/signin-covers/elden-ring.jpg"),
    },
    SampleCover {
        cache_file: "signin-cover-hades.jpg",
        local_file: "hades.jpg",
        bytes: include_bytes!("../../../../ui/src/assets/signin-covers/hades.jpg"),
    },
    SampleCover {
        cache_file: "signin-cover-baldurs-gate-iii.jpg",
        local_file: "baldurs-gate-iii.jpg",
        bytes: include_bytes!("../../../../ui/src/assets/signin-covers/baldurs-gate-iii.jpg"),
    },
    SampleCover {
        cache_file: "signin-cover-hollow-knight.jpg",
        local_file: "hollow-knight.jpg",
        bytes: include_bytes!("../../../../ui/src/assets/signin-covers/hollow-knight.jpg"),
    },
    SampleCover {
        cache_file: "signin-cover-cyberpunk-2077.jpg",
        local_file: "cyberpunk-2077.jpg",
        bytes: include_bytes!("../../../../ui/src/assets/signin-covers/cyberpunk-2077.jpg"),
    },
    SampleCover {
        cache_file: "signin-cover-stardew-valley.jpg",
        local_file: "stardew-valley.jpg",
        bytes: include_bytes!("../../../../ui/src/assets/signin-covers/stardew-valley.jpg"),
    },
    SampleCover {
        cache_file: "signin-cover-portal-2.jpg",
        local_file: "portal-2.jpg",
        bytes: include_bytes!("../../../../ui/src/assets/signin-covers/portal-2.jpg"),
    },
    SampleCover {
        cache_file: "signin-cover-the-witcher-3-wild-hunt.jpg",
        local_file: "the-witcher-3-wild-hunt.jpg",
        bytes: include_bytes!(
            "../../../../ui/src/assets/signin-covers/the-witcher-3-wild-hunt.jpg"
        ),
    },
    SampleCover {
        cache_file: "signin-cover-grand-theft-auto-v.jpg",
        local_file: "grand-theft-auto-v.jpg",
        bytes: include_bytes!("../../../../ui/src/assets/signin-covers/grand-theft-auto-v.jpg"),
    },
    SampleCover {
        cache_file: "signin-cover-celeste.jpg",
        local_file: "celeste.jpg",
        bytes: include_bytes!("../../../../ui/src/assets/signin-covers/celeste.jpg"),
    },
    SampleCover {
        cache_file: "signin-cover-terraria.jpg",
        local_file: "terraria.jpg",
        bytes: include_bytes!("../../../../ui/src/assets/signin-covers/terraria.jpg"),
    },
    SampleCover {
        cache_file: "signin-cover-doom-eternal.jpg",
        local_file: "doom-eternal.jpg",
        bytes: include_bytes!("../../../../ui/src/assets/signin-covers/doom-eternal.jpg"),
    },
    SampleCover {
        cache_file: "signin-cover-dark-souls-iii.jpg",
        local_file: "dark-souls-iii.jpg",
        bytes: include_bytes!("../../../../ui/src/assets/signin-covers/dark-souls-iii.jpg"),
    },
    SampleCover {
        cache_file: "signin-cover-mass-effect-2.jpg",
        local_file: "mass-effect-2.jpg",
        bytes: include_bytes!("../../../../ui/src/assets/signin-covers/mass-effect-2.jpg"),
    },
    SampleCover {
        cache_file: "signin-cover-disco-elysium.jpg",
        local_file: "disco-elysium.jpg",
        bytes: include_bytes!("../../../../ui/src/assets/signin-covers/disco-elysium.jpg"),
    },
    SampleCover {
        cache_file: "signin-cover-outer-wilds.jpg",
        local_file: "outer-wilds.jpg",
        bytes: include_bytes!("../../../../ui/src/assets/signin-covers/outer-wilds.jpg"),
    },
    SampleCover {
        cache_file: "signin-cover-ori-and-the-blind-forest.jpg",
        local_file: "ori-and-the-blind-forest.jpg",
        bytes: include_bytes!(
            "../../../../ui/src/assets/signin-covers/ori-and-the-blind-forest.jpg"
        ),
    },
    SampleCover {
        cache_file: "signin-cover-undertale.jpg",
        local_file: "undertale.jpg",
        bytes: include_bytes!("../../../../ui/src/assets/signin-covers/undertale.jpg"),
    },
    SampleCover {
        cache_file: "signin-cover-slay-the-spire.jpg",
        local_file: "slay-the-spire.jpg",
        bytes: include_bytes!("../../../../ui/src/assets/signin-covers/slay-the-spire.jpg"),
    },
    SampleCover {
        cache_file: "signin-cover-sekiro-shadows-die-twice.jpg",
        local_file: "sekiro-shadows-die-twice.jpg",
        bytes: include_bytes!(
            "../../../../ui/src/assets/signin-covers/sekiro-shadows-die-twice.jpg"
        ),
    },
    SampleCover {
        cache_file: "signin-cover-the-last-of-us-part-ii-remastered.jpg",
        local_file: "the-last-of-us-part-ii-remastered.jpg",
        bytes: include_bytes!(
            "../../../../ui/src/assets/signin-covers/the-last-of-us-part-ii-remastered.jpg"
        ),
    },
    SampleCover {
        cache_file: "signin-cover-factorio.jpg",
        local_file: "factorio.jpg",
        bytes: include_bytes!("../../../../ui/src/assets/signin-covers/factorio.jpg"),
    },
    SampleCover {
        cache_file: "signin-cover-vampire-survivors.jpg",
        local_file: "vampire-survivors.jpg",
        bytes: include_bytes!("../../../../ui/src/assets/signin-covers/vampire-survivors.jpg"),
    },
    SampleCover {
        cache_file: "signin-cover-death-stranding-directors-cut.jpg",
        local_file: "death-stranding-directors-cut.jpg",
        bytes: include_bytes!(
            "../../../../ui/src/assets/signin-covers/death-stranding-directors-cut.jpg"
        ),
    },
    SampleCover {
        cache_file: "signin-cover-control.jpg",
        local_file: "control.jpg",
        bytes: include_bytes!("../../../../ui/src/assets/signin-covers/control.jpg"),
    },
];

/// Cached cover file names, most recently written first, at most [`COVER_WALL_LIMIT`].
///
/// The cover cache is the only account-independent record available while signed out. Real cache
/// files always come first, ordered by modification time; the bundled IGDB samples fill any
/// remaining tiles. Samples are identified by their file name rather than by modification time,
/// so writing them never makes a sample look more recent than the user's own cover. Files that
/// the asset protocol would not serve are omitted, and an unreadable cache is an empty wall
/// rather than an error. A missing directory is left untouched: the UI fills the mural from its
/// bundled copy, so signing out never creates user data as a side effect.
pub(super) fn cover_wall(dir: &Path) -> Vec<String> {
    if !dir.is_dir() {
        return Vec::new();
    }
    seed_samples(dir);

    let sample_names: HashSet<&str> = SAMPLE_COVERS.iter().map(|cover| cover.cache_file).collect();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(SystemTime, String)> = Vec::new();
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if !crate::protocol::valid_cover_file(&name) || sample_names.contains(name.as_str()) {
            continue;
        }
        files.push((metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH), name));
    }
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    let mut wall: Vec<String> = files
        .into_iter()
        .take(COVER_WALL_LIMIT)
        .map(|(_, name)| name)
        .collect();

    if wall.len() < COVER_WALL_LIMIT {
        for cover in SAMPLE_COVERS {
            if dir.join(cover.cache_file).is_file() {
                wall.push(cover.cache_file.to_owned());
                if wall.len() == COVER_WALL_LIMIT {
                    break;
                }
            }
        }
    }
    wall
}

/// Write the bundled samples that are missing, never replacing an existing file. A read-only or
/// failing cache is ignored: the mural simply uses the files it can read.
fn seed_samples(dir: &Path) {
    for cover in SAMPLE_COVERS {
        debug_assert!(cover.cache_file.starts_with(SAMPLE_PREFIX));
        let path = dir.join(cover.cache_file);
        if path.exists() {
            continue;
        }
        let _ = std::fs::write(path, cover.bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::time::Duration;

    fn touch(dir: &Path, name: &str, seconds: u64) {
        let file = File::create(dir.join(name)).unwrap();
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds))
            .unwrap();
    }

    #[test]
    fn orders_by_modified_time_and_keeps_only_cache_file_names() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "old.jpg", 10);
        touch(dir.path(), "newest.png", 30);
        touch(dir.path(), "middle.webp", 20);
        std::fs::create_dir(dir.path().join("artworks-v2")).unwrap();
        touch(dir.path(), ".hidden.jpg", 40);

        let wall = cover_wall(dir.path());
        assert_eq!(&wall[..3], ["newest.png", "middle.webp", "old.jpg"]);
    }

    #[test]
    fn caps_the_wall_at_twenty_five() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..40 {
            touch(dir.path(), &format!("{index}.jpg"), index as u64);
        }
        let wall = cover_wall(dir.path());
        assert_eq!(wall.len(), COVER_WALL_LIMIT);
        assert_eq!(wall.first().map(String::as_str), Some("39.jpg"));
        assert_eq!(wall.last().map(String::as_str), Some("15.jpg"));
    }

    #[test]
    fn an_unreadable_cache_is_an_empty_wall() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        assert!(cover_wall(&missing).is_empty());
    }

    #[test]
    fn an_empty_cache_shows_the_bundled_real_covers() {
        let dir = tempfile::tempdir().unwrap();
        let wall = cover_wall(dir.path());
        assert_eq!(wall.len(), COVER_WALL_LIMIT);
        assert_eq!(
            wall.first().map(String::as_str),
            Some("signin-cover-elden-ring.jpg")
        );
        let bytes = std::fs::read(dir.path().join(&wall[0])).unwrap();
        assert_eq!(&bytes[..2], [0xff, 0xd8]);
        for name in &wall {
            assert!(dir.path().join(name).is_file(), "{name} was seeded");
        }
    }

    #[test]
    fn user_covers_stay_ahead_of_newer_seeded_samples() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "user-cover.jpg", 10);
        let wall = cover_wall(dir.path());
        assert_eq!(wall.first().map(String::as_str), Some("user-cover.jpg"));
        assert_eq!(wall.len(), COVER_WALL_LIMIT);
        assert!(dir.path().join("user-cover.jpg").is_file());
        assert!(
            wall.iter()
                .skip(1)
                .all(|name| name.starts_with(SAMPLE_PREFIX)),
            "samples fill the rest: {wall:?}"
        );
    }

    #[test]
    fn a_full_user_cache_does_not_return_samples() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..30 {
            touch(dir.path(), &format!("{index}.jpg"), index as u64);
        }
        let wall = cover_wall(dir.path());
        assert_eq!(wall.len(), COVER_WALL_LIMIT);
        assert!(wall.iter().all(|name| !name.starts_with(SAMPLE_PREFIX)));
    }

    #[test]
    fn every_bundled_sample_matches_the_manifest_and_decodes() {
        let manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../ui/src/assets/signin-covers/manifest.json"
        ))
        .unwrap();
        let covers = manifest["covers"].as_array().unwrap();
        assert_eq!(covers.len(), SAMPLE_COVERS.len());

        let scratch = tempfile::tempdir().unwrap();
        for (sample, recorded) in SAMPLE_COVERS.iter().zip(covers) {
            assert_eq!(
                recorded["localFile"].as_str(),
                Some(sample.local_file),
                "manifest order matches the bundled list"
            );
            assert!(
                recorded["title"]
                    .as_str()
                    .is_some_and(|title| !title.is_empty())
            );
            assert!(recorded["igdbImageUrl"].as_str().is_some_and(|url| {
                url.starts_with("https://images.igdb.com/igdb/image/upload/t_cover_big/")
            }));
            assert_eq!(
                sample.cache_file,
                format!("{SAMPLE_PREFIX}{}", sample.local_file),
                "cache file derives from the shared asset name"
            );
            let decoded = gamenode_platform::covers::reencode_local_cover(
                scratch.path(),
                sample.local_file,
                sample.bytes,
            )
            .unwrap_or_else(|error| panic!("{} does not decode: {error}", sample.local_file));
            assert!(decoded.ends_with(".webp"));
        }
    }
}
