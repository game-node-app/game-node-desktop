//! Achievement icon cache: trusted Steam image hosts, an HTTPS-only bounded downloader and the
//! on-disk files served by `gamenode-asset://localhost/achievement/<hash>`.
//!
//! Everything here performs disk or network I/O and is blocking; call it from a worker thread.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use reqwest::header::CONTENT_TYPE;
use reqwest::redirect::Policy;
use url::Url;

/// Largest icon accepted. Steam achievement icons are a few KiB; anything far larger is refused.
pub const MAX_ACHIEVEMENT_ICON_BYTES: u64 = 1024 * 1024;

/// Icons for one list are fetched concurrently; Steam serves them from a CDN and a whole list
/// must not take one round trip per icon.
const ICON_FETCH_WORKERS: usize = 6;

/// Hosts trusted to serve Steam achievement icons. This list is host metadata, never plugin data.
pub const TRUSTED_ICON_HOSTS: &[&str] = &[
    "cdn.akamai.steamstatic.com",
    "cdn.cloudflare.steamstatic.com",
    "shared.akamai.steamstatic.com",
    "steamcdn-a.akamaihd.net",
    "media.steampowered.com",
];

static PART_COUNTER: AtomicU64 = AtomicU64::new(1);

/// `<covers>/achievement-icons/<hash>.img`.
pub fn achievement_icon_dir(covers_dir: &Path) -> PathBuf {
    covers_dir.join("achievement-icons")
}

/// A stable file name derived from the validated URL, so remote text never becomes a path.
pub fn achievement_icon_hash(url: &str) -> String {
    crate::download::compute_bytes_sha256(url.as_bytes())
}

/// The WebView URL for a cached icon hash. The protocol never fetches a request-supplied URL.
pub fn achievement_icon_asset_url(hash: &str) -> String {
    format!("gamenode-asset://localhost/achievement/{hash}")
}

/// A valid cache file name is exactly the 64 lowercase hex characters of a SHA-256 digest.
pub fn valid_icon_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Whether this icon URL may be fetched. HTTPS and a trusted host are both required.
pub fn trusted_icon_url(raw: &str) -> bool {
    trusted_icon_url_with(raw, false)
}

fn trusted_icon_url_with(raw: &str, allow_loopback: bool) -> bool {
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    if url.scheme() != "https" && !(allow_loopback && url.scheme() == "http") {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    TRUSTED_ICON_HOSTS.contains(&host.as_str())
        || (allow_loopback && matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1" | "[::1]"))
}

#[derive(Debug, thiserror::Error)]
pub enum IconError {
    #[error("the icon URL is not trusted")]
    RejectedUrl,
    #[error("the icon download failed: {0}")]
    Fetch(String),
    #[error("the icon exceeds the {limit}-byte limit")]
    TooLarge { limit: u64 },
    #[error("the icon is not a supported image")]
    NotImage,
    #[error("the icon cache I/O failed: {0}")]
    Io(String),
}

/// Blocking cache-backed icon downloader.
#[derive(Clone)]
pub struct AchievementIconLoader {
    client: reqwest::blocking::Client,
    dir: PathBuf,
    allow_loopback: bool,
}

impl AchievementIconLoader {
    /// The production loader: trusted Steam hosts only, `<covers>/achievement-icons`.
    pub fn production(covers_dir: &Path) -> Result<Self, IconError> {
        Self::new(covers_dir, false)
    }

    fn new(covers_dir: &Path, allow_loopback: bool) -> Result<Self, IconError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(redirect_policy(allow_loopback))
            .user_agent("GameNode Desktop")
            .build()
            .map_err(|error| IconError::Fetch(error.to_string()))?;
        Ok(Self {
            client,
            dir: achievement_icon_dir(covers_dir),
            allow_loopback,
        })
    }

    /// Fetches every URL that is not cached yet. Best effort: a failed icon never fails the list;
    /// the WebView simply keeps its placeholder.
    pub fn ensure(&self, urls: impl IntoIterator<Item = String>) {
        let mut seen = std::collections::BTreeSet::new();
        let pending = urls
            .into_iter()
            .filter(|url| {
                seen.insert(url.clone()) && trusted_icon_url_with(url, self.allow_loopback)
            })
            .filter(|url| !self.cache_path(&achievement_icon_hash(url)).is_file())
            .collect::<Vec<_>>();
        if pending.is_empty() {
            return;
        }
        let next = AtomicUsize::new(0);
        let workers = pending.len().min(ICON_FETCH_WORKERS);
        std::thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| {
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(url) = pending.get(index) else {
                            return;
                        };
                        if let Err(error) = self.fetch_and_cache(url) {
                            tracing::debug!(%error, "achievement icon was not cached");
                        }
                    }
                });
            }
        });
    }

    fn cache_path(&self, hash: &str) -> PathBuf {
        self.dir.join(format!("{hash}.img"))
    }

    fn fetch_and_cache(&self, url: &str) -> Result<(), IconError> {
        if !trusted_icon_url_with(url, self.allow_loopback) {
            return Err(IconError::RejectedUrl);
        }
        let response = self
            .client
            .get(url)
            .send()
            .map_err(|error| IconError::Fetch(error.to_string()))?;
        let final_url = response.url().clone();
        if !trusted_icon_url_with(final_url.as_str(), self.allow_loopback)
            || response.status().is_redirection()
        {
            return Err(IconError::RejectedUrl);
        }
        if !response.status().is_success() {
            return Err(IconError::Fetch(format!(
                "HTTP {} fetching an achievement icon",
                response.status()
            )));
        }
        let media_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .map(str::trim)
            .unwrap_or("")
            .to_ascii_lowercase();
        if !media_type.starts_with("image/") || media_type == "image/svg+xml" {
            return Err(IconError::NotImage);
        }
        let mut bytes = Vec::new();
        response
            .take(MAX_ACHIEVEMENT_ICON_BYTES.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| IconError::Fetch(error.to_string()))?;
        if bytes.len() as u64 > MAX_ACHIEVEMENT_ICON_BYTES {
            return Err(IconError::TooLarge {
                limit: MAX_ACHIEVEMENT_ICON_BYTES,
            });
        }
        self.write(&achievement_icon_hash(url), &bytes)
    }

    fn write(&self, hash: &str, bytes: &[u8]) -> Result<(), IconError> {
        fs::create_dir_all(&self.dir).map_err(|error| IconError::Io(error.to_string()))?;
        let temp = self.dir.join(format!(
            ".{hash}.{}.{}.part",
            std::process::id(),
            PART_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let written = (|| {
            let mut file = File::create(&temp)?;
            file.write_all(bytes)?;
            file.flush()?;
            drop(file);
            fs::rename(&temp, self.cache_path(hash))
        })();
        if written.is_err() {
            let _ = fs::remove_file(&temp);
        }
        written.map_err(|error| IconError::Io(error.to_string()))
    }
}

fn redirect_policy(allow_loopback: bool) -> Policy {
    Policy::custom(move |attempt| {
        if attempt.previous().len() >= 5 {
            attempt.stop()
        } else if trusted_icon_url_with(attempt.url().as_str(), allow_loopback) {
            attempt.follow()
        } else {
            attempt.stop()
        }
    })
}

/// Bytes held by the achievement icon cache.
pub fn achievement_icon_usage(covers_dir: &Path) -> u64 {
    fn walk(dir: &Path) -> u64 {
        let Ok(entries) = fs::read_dir(dir) else {
            return 0;
        };
        entries
            .flatten()
            .map(|entry| {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path)
                } else {
                    entry.metadata().map_or(0, |metadata| metadata.len())
                }
            })
            .sum()
    }
    walk(&achievement_icon_dir(covers_dir))
}

/// Deletes every cached achievement icon and returns the freed bytes.
pub fn clear_achievement_icons(covers_dir: &Path) -> u64 {
    let dir = achievement_icon_dir(covers_dir);
    let freed = achievement_icon_usage(covers_dir);
    let _ = fs::remove_dir_all(dir);
    freed
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_loader(dir: &Path) -> AchievementIconLoader {
        AchievementIconLoader::new(dir, true).unwrap()
    }

    fn png() -> Vec<u8> {
        vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3]
    }

    #[test]
    fn only_https_trusted_hosts_are_fetched() {
        for allowed in [
            "https://cdn.akamai.steamstatic.com/steamcommunity/public/images/apps/620/a.jpg",
            "https://cdn.cloudflare.steamstatic.com/x.png",
        ] {
            assert!(trusted_icon_url(allowed), "{allowed}");
        }
        for denied in [
            "http://cdn.akamai.steamstatic.com/x.png",
            "https://evil.example/x.png",
            "https://cdn.akamai.steamstatic.com.evil.example/x.png",
            "file:///etc/passwd",
            "not a url",
        ] {
            assert!(!trusted_icon_url(denied), "{denied}");
        }
    }

    #[test]
    fn hashes_are_valid_file_names_and_the_asset_url_is_derived() {
        let hash = achievement_icon_hash("https://cdn.akamai.steamstatic.com/a.jpg");
        assert!(valid_icon_hash(&hash));
        assert!(!valid_icon_hash("../secret"));
        assert_eq!(
            achievement_icon_asset_url(&hash),
            format!("gamenode-asset://localhost/achievement/{hash}")
        );
    }

    #[test]
    fn cache_usage_and_clearing_remove_only_icons() {
        let dir = tempfile::tempdir().unwrap();
        let covers = dir.path();
        let icons = achievement_icon_dir(covers);
        fs::create_dir_all(&icons).unwrap();
        fs::write(icons.join("a.img"), b"12345").unwrap();
        fs::write(covers.join("620.jpg"), b"cover").unwrap();
        assert_eq!(achievement_icon_usage(covers), 5);
        assert_eq!(clear_achievement_icons(covers), 5);
        assert_eq!(achievement_icon_usage(covers), 0);
        assert!(covers.join("620.jpg").is_file());
    }

    #[test]
    fn downloads_trusted_icons_once_and_caches_them() {
        let server = httpmock::MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/icon.png");
            then.status(200)
                .header("content-type", "image/png")
                .body(png());
        });
        let dir = tempfile::tempdir().unwrap();
        let loader = test_loader(dir.path());
        let url = format!("{}/icon.png", server.base_url());
        loader.ensure([url.clone()]);
        loader.ensure([url]);
        assert_eq!(mock.calls(), 1);
        assert_eq!(achievement_icon_usage(dir.path()), png().len() as u64);
    }

    #[test]
    fn refuses_redirects_oversize_and_non_images() {
        let server = httpmock::MockServer::start();
        let dir = tempfile::tempdir().unwrap();
        let loader = test_loader(dir.path());

        let redirect = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/redirect");
            then.status(302)
                .header("location", "https://evil.example/icon.png");
        });
        let url = format!("{}/redirect", server.base_url());
        assert!(matches!(
            loader.fetch_and_cache(&url),
            Err(IconError::RejectedUrl)
        ));
        redirect.assert();

        let oversize = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/big.png");
            then.status(200)
                .header("content-type", "image/png")
                .body(vec![0u8; MAX_ACHIEVEMENT_ICON_BYTES as usize + 1]);
        });
        let url = format!("{}/big.png", server.base_url());
        assert!(matches!(
            loader.fetch_and_cache(&url),
            Err(IconError::TooLarge { .. })
        ));
        oversize.assert();

        let text = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/text.png");
            then.status(200)
                .header("content-type", "text/html")
                .body("<html></html>");
        });
        let url = format!("{}/text.png", server.base_url());
        assert!(matches!(
            loader.fetch_and_cache(&url),
            Err(IconError::NotImage)
        ));
        text.assert();
    }

    #[test]
    fn a_loader_records_no_icon_for_untrusted_urls() {
        let dir = tempfile::tempdir().unwrap();
        let loader = test_loader(dir.path());
        loader.ensure(["https://evil.example/icon.png".to_owned()]);
        assert_eq!(achievement_icon_usage(dir.path()), 0);
    }
}
