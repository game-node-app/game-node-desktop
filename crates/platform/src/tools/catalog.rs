//! Release catalog fetching and content-addressed archive caching.
//!
//! Archive detection, path validation and extraction live in [`crate::archive`]; checksummed
//! download caching lives in [`crate::download`]. The re-exports below keep the game-tools API
//! and tests unchanged.

#![allow(clippy::collapsible_if)]

use std::path::{Path, PathBuf};

use gamenode_core::tools::{
    ArchiveFormat, CachedToolArchive, ToolCatalogApi, ToolChannel, ToolDescriptor, ToolError,
    ToolRelease,
};
use serde::Deserialize;

use crate::download::{
    CachedDownloadRequest, GitHubDownloadPolicy, HttpDownloadFetcher, ensure_cached_download,
    sanitize_path_segment,
};

pub use crate::archive::{
    MAX_TOTAL_EXTRACTED_BYTES, extract_archive_to_dir, list_archive_entries,
    validate_archive_entry_path,
};
pub use crate::download::{compute_bytes_sha256, compute_file_sha256};

#[cfg(test)]
use crate::download::DownloadUrlPolicy;
#[cfg(test)]
use std::fs::File;
#[cfg(test)]
use std::io::Write;
#[cfg(test)]
use std::sync::atomic::Ordering;
#[cfg(test)]
use std::time::Duration;

fn find_64_hex_token(line: &str) -> Option<String> {
    for word in line.split(|c: char| {
        c.is_whitespace() || c == '|' || c == '`' || c == '(' || c == ')' || c == ':' || c == '='
    }) {
        let clean = word.trim();
        if clean.len() == 64 && clean.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(clean.to_ascii_lowercase());
        }
    }
    None
}

/// Parses an optional SHA-256 digest from release body markdown or checksum table matching `asset_name`.
pub fn parse_body_sha256(body: &str, asset_name: &str) -> Option<String> {
    let lines: Vec<&str> = body.lines().collect();
    let lower_asset = asset_name.to_ascii_lowercase();

    for (i, line) in lines.iter().enumerate() {
        if line.to_ascii_lowercase().contains(&lower_asset) {
            if let Some(hash) = find_64_hex_token(line) {
                return Some(hash);
            }
            if i + 1 < lines.len() {
                if let Some(hash) = find_64_hex_token(lines[i + 1]) {
                    return Some(hash);
                }
            }
            if i > 0 {
                if let Some(hash) = find_64_hex_token(lines[i - 1]) {
                    return Some(hash);
                }
            }
        }
    }

    // Fallback: if there is only one 64-hex token in the entire body, use it
    let mut all_hashes = Vec::new();
    for line in &lines {
        for word in line.split(|c: char| {
            c.is_whitespace()
                || c == '|'
                || c == '`'
                || c == '('
                || c == ')'
                || c == ':'
                || c == '='
        }) {
            let clean = word.trim();
            if clean.len() == 64 && clean.chars().all(|c| c.is_ascii_hexdigit()) {
                all_hashes.push(clean.to_ascii_lowercase());
            }
        }
    }
    all_hashes.dedup();
    if all_hashes.len() == 1 {
        return Some(all_hashes.remove(0));
    }

    None
}

fn extract_asset_sha256(
    asset_digest: Option<&str>,
    body: Option<&str>,
    asset_name: &str,
) -> Option<String> {
    if let Some(digest) = asset_digest {
        let clean = digest.trim();
        if let Some(hex) = clean.strip_prefix("sha256:") {
            let hex = hex.trim();
            if hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(hex.to_ascii_lowercase());
            }
        } else if clean.len() == 64 && clean.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(clean.to_ascii_lowercase());
        }
    }

    if let Some(body_text) = body {
        if let Some(hash) = parse_body_sha256(body_text, asset_name) {
            return Some(hash);
        }
    }

    None
}

/// GitHub release JSON representation.
#[derive(Debug, Deserialize)]
struct GitHubReleaseDto {
    tag_name: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<GitHubAssetDto>,
}

#[derive(Debug, Deserialize)]
struct GitHubAssetDto {
    name: String,
    browser_download_url: String,
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

/// Production implementation of `ToolCatalogApi` fetching releases and archives from authorized GitHub repositories.
pub struct GitHubToolCatalog {
    cache_dir: PathBuf,
    api_base_url: String,
    download_policy: GitHubDownloadPolicy,
    fetcher: HttpDownloadFetcher,
}

impl GitHubToolCatalog {
    pub fn new(cache_dir: impl Into<PathBuf>) -> Result<Self, ToolError> {
        Self::with_base_url(cache_dir, "https://api.github.com".to_string())
    }

    pub fn with_base_url(
        cache_dir: impl Into<PathBuf>,
        api_base_url: String,
    ) -> Result<Self, ToolError> {
        let api_base_url = api_base_url.trim_end_matches('/').to_string();
        Ok(Self {
            cache_dir: cache_dir.into(),
            download_policy: GitHubDownloadPolicy::with_api_base_url(api_base_url.clone()),
            fetcher: HttpDownloadFetcher::new()?,
            api_base_url,
        })
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    pub fn api_base_url(&self) -> &str {
        &self.api_base_url
    }

    #[cfg(test)]
    fn is_allowed_download_url(&self, raw_url: &str) -> bool {
        self.download_policy.is_allowed(raw_url)
    }
}

fn walk_cache_usage(dir: &Path) -> std::io::Result<(u64, usize)> {
    let mut total_bytes = 0u64;
    let mut count = 0usize;
    if !dir.exists() {
        return Ok((0, 0));
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let (sub_bytes, sub_count) = walk_cache_usage(&path)?;
            total_bytes += sub_bytes;
            count += sub_count;
        } else if path.is_file() {
            let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if file_name.ends_with(".part") || file_name.contains(".part.") {
                continue;
            }
            let is_archive = file_name.to_ascii_lowercase().ends_with(".7z")
                || file_name.to_ascii_lowercase().ends_with(".zip");
            if is_archive {
                if let Ok(meta) = entry.metadata() {
                    total_bytes += meta.len();
                    count += 1;
                }
            }
        }
    }
    Ok((total_bytes, count))
}

impl ToolCatalogApi for GitHubToolCatalog {
    fn fetch_releases(
        &self,
        descriptor: &ToolDescriptor,
        channel: ToolChannel,
    ) -> Result<Vec<ToolRelease>, ToolError> {
        let channel_spec =
            descriptor
                .channel_spec(channel)
                .ok_or_else(|| ToolError::ReleaseNotFound {
                    channel,
                    version: String::new(),
                })?;

        // Guardrail: verify repo is declared on this descriptor
        if !descriptor
            .channels
            .iter()
            .any(|c| c.github_repo == channel_spec.github_repo)
        {
            return Err(ToolError::Failed(format!(
                "Repository {} is not declared in descriptor channels",
                channel_spec.github_repo
            )));
        }

        let endpoint = format!(
            "{}/repos/{}/releases?per_page=20",
            self.api_base_url, channel_spec.github_repo
        );

        let response = self
            .fetcher
            .client()
            .get(&endpoint)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .send()
            .map_err(|e| {
                ToolError::DownloadFailed(format!("Failed to query GitHub releases: {e}"))
            })?;

        if !response.status().is_success() {
            return Err(ToolError::DownloadFailed(format!(
                "GitHub releases API returned HTTP {}",
                response.status()
            )));
        }

        let raw_releases: Vec<GitHubReleaseDto> =
            serde_json::from_reader(response).map_err(|e| {
                ToolError::DownloadFailed(format!("Failed to parse releases JSON: {e}"))
            })?;

        let mut releases = Vec::new();
        let target_ext = match channel_spec.archive_format {
            ArchiveFormat::SevenZip => ".7z",
            ArchiveFormat::Zip => ".zip",
        };
        let fallback_ext = match channel_spec.archive_format {
            ArchiveFormat::SevenZip => ".zip",
            ArchiveFormat::Zip => ".7z",
        };

        for rel in raw_releases {
            if rel.draft {
                continue;
            }

            let matching_asset = rel
                .assets
                .iter()
                .find(|a| a.name.to_ascii_lowercase().ends_with(target_ext))
                .or_else(|| {
                    rel.assets
                        .iter()
                        .find(|a| a.name.to_ascii_lowercase().ends_with(fallback_ext))
                });

            let Some(asset) = matching_asset else {
                continue;
            };

            let trimmed_tag = rel.tag_name.trim();
            let clean_ver = if let Some(stripped) = trimmed_tag
                .strip_prefix('v')
                .or_else(|| trimmed_tag.strip_prefix('V'))
            {
                stripped.to_string()
            } else {
                trimmed_tag.to_string()
            };

            let mut sha256 =
                extract_asset_sha256(asset.digest.as_deref(), rel.body.as_deref(), &asset.name);

            // Check if archive already exists in cache
            let sanitized_version = sanitize_path_segment(&clean_ver);
            let candidate_path = self
                .cache_dir
                .join(descriptor.id.as_str())
                .join(channel.as_str())
                .join(&sanitized_version)
                .join(&asset.name);

            let mut cached_path = None;
            if candidate_path.is_file() {
                if let Ok(computed) = compute_file_sha256(&candidate_path) {
                    if let Some(ref expected) = sha256 {
                        if expected.eq_ignore_ascii_case(&computed) {
                            cached_path = Some(candidate_path.to_string_lossy().to_string());
                        }
                    } else if list_archive_entries(&candidate_path).is_ok() {
                        sha256 = Some(computed);
                        cached_path = Some(candidate_path.to_string_lossy().to_string());
                    }
                }
            }

            releases.push(ToolRelease {
                tool_id: descriptor.id.clone(),
                channel,
                version: clean_ver,
                tag_name: rel.tag_name.clone(),
                name: rel.name.unwrap_or_else(|| rel.tag_name.clone()),
                published_at: rel.published_at.unwrap_or_default(),
                download_url: asset.browser_download_url.clone(),
                asset_name: asset.name.clone(),
                size_bytes: asset.size,
                sha256,
                cached_path,
            });
        }

        Ok(releases)
    }

    fn ensure_cached_archive(
        &self,
        descriptor: &ToolDescriptor,
        release: &ToolRelease,
    ) -> Result<CachedToolArchive, ToolError> {
        let channel_slug = release.channel.as_str();
        let sanitized_version = sanitize_path_segment(&release.version);
        let target_dir = self
            .cache_dir
            .join(descriptor.id.as_str())
            .join(channel_slug)
            .join(&sanitized_version);

        let request = CachedDownloadRequest {
            url: &release.download_url,
            cache_dir: &target_dir,
            file_name: &release.asset_name,
            expected_sha256: release.sha256.as_deref(),
        };
        // Tool releases are archives; reject a corrupt download even without a published hash.
        let validate = |path: &Path| {
            list_archive_entries(path).map(|_| ()).map_err(|e| {
                ToolError::ExtractionFailed(format!(
                    "Downloaded archive integrity check failed: {e}"
                ))
            })
        };
        let cached = ensure_cached_download(
            &self.fetcher,
            &self.download_policy,
            &request,
            Some(&validate),
        )?;

        Ok(CachedToolArchive {
            tool_id: descriptor.id.clone(),
            channel: release.channel,
            version: release.version.clone(),
            path: cached.path.to_string_lossy().to_string(),
            size_bytes: cached.size_bytes,
            sha256: cached.sha256,
        })
    }

    fn cache_usage(&self) -> Result<(u64, usize), ToolError> {
        walk_cache_usage(&self.cache_dir).map_err(|e| ToolError::Io(e.to_string()))
    }

    fn clear_cache(&self) -> Result<u64, ToolError> {
        if !self.cache_dir.exists() {
            return Ok(0);
        }
        let (total_bytes, _) = self.cache_usage()?;
        let entries =
            std::fs::read_dir(&self.cache_dir).map_err(|e| ToolError::Io(e.to_string()))?;
        for entry in entries {
            let entry = entry.map_err(|e| ToolError::Io(e.to_string()))?;
            let path = entry.path();
            if path.is_dir() {
                std::fs::remove_dir_all(&path).map_err(|e| ToolError::Io(e.to_string()))?;
            } else {
                std::fs::remove_file(&path).map_err(|e| ToolError::Io(e.to_string()))?;
            }
        }
        Ok(total_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gamenode_core::tools::registry::optiscaler_descriptor;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::thread;

    #[test]
    fn test_compute_sha256_helpers() {
        let text = b"hello world";
        let hash = compute_bytes_sha256(text);
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );

        let temp = tempfile::tempdir().unwrap();
        let file_path = temp.path().join("test.txt");
        std::fs::write(&file_path, text).unwrap();
        let file_hash = compute_file_sha256(&file_path).unwrap();
        assert_eq!(file_hash, hash);
    }

    #[test]
    fn test_validate_archive_entry_path() {
        assert!(validate_archive_entry_path("safe/path/OptiScaler.dll").is_ok());
        assert!(validate_archive_entry_path("OptiScaler.dll").is_ok());
        assert!(validate_archive_entry_path("./OptiScaler.dll").is_ok());

        assert!(validate_archive_entry_path("../secret.txt").is_err());
        assert!(validate_archive_entry_path("sub/../../secret.txt").is_err());
        assert!(validate_archive_entry_path("/etc/shadow").is_err());
        assert!(validate_archive_entry_path("C:\\Windows\\system32").is_err());
    }

    #[test]
    fn test_parse_body_sha256_formats() {
        let expected_hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

        // Markdown table format
        let body_table = format!(
            "Checksums\n| Asset | SHA256 |\n| --- | --- |\n| OptiScaler-v0.7.9.7z | {expected_hash} |\n"
        );
        assert_eq!(
            parse_body_sha256(&body_table, "OptiScaler-v0.7.9.7z"),
            Some(expected_hash.to_string())
        );

        // sha256sum format
        let body_sha256sum = format!("{expected_hash}  OptiScaler-v0.7.9.7z\n");
        assert_eq!(
            parse_body_sha256(&body_sha256sum, "OptiScaler-v0.7.9.7z"),
            Some(expected_hash.to_string())
        );

        // Name with colon
        let body_colon = format!("OptiScaler-v0.7.9.7z: {expected_hash}\n");
        assert_eq!(
            parse_body_sha256(&body_colon, "OptiScaler-v0.7.9.7z"),
            Some(expected_hash.to_string())
        );

        // Next line format
        let body_multiline = format!("OptiScaler-v0.7.9.7z\n{expected_hash}\n");
        assert_eq!(
            parse_body_sha256(&body_multiline, "OptiScaler-v0.7.9.7z"),
            Some(expected_hash.to_string())
        );

        // Fallback single hash
        assert_eq!(
            parse_body_sha256(&body_table, "OtherTool.zip"),
            Some(expected_hash.to_string())
        );
    }

    #[test]
    fn test_allowed_download_urls() {
        let catalog = GitHubToolCatalog::new(PathBuf::from("/tmp/cache")).unwrap();

        assert!(catalog.is_allowed_download_url(
            "https://github.com/optiscaler/OptiScaler/releases/download/v0.7.9/OptiScaler.7z"
        ));
        assert!(catalog.is_allowed_download_url(
            "https://objects.githubusercontent.com/github-production-release-asset-2e65be/12345"
        ));
        assert!(catalog.is_allowed_download_url("file:///local/path/archive.7z"));
        assert!(catalog.is_allowed_download_url("http://127.0.0.1:8080/download/tool.7z"));
        assert!(catalog.is_allowed_download_url("http://localhost:3000/download/tool.zip"));

        // Insecure or untrusted external hosts
        assert!(!catalog.is_allowed_download_url("http://github.com/download/tool.7z"));
        assert!(!catalog.is_allowed_download_url("https://malicious-site.com/tool.7z"));
        assert!(!catalog.is_allowed_download_url("ftp://github.com/tool.7z"));
    }

    #[test]
    fn test_zip_creation_listing_and_extraction() {
        let temp = tempfile::tempdir().unwrap();
        let zip_path = temp.path().join("test.zip");

        // Create a valid zip archive
        {
            let file = File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);

            zip.start_file("OptiScaler.dll", options).unwrap();
            zip.write_all(b"fake OptiScaler DLL bytes").unwrap();

            zip.start_file("OptiScaler.ini", options).unwrap();
            zip.write_all(b"[General]\nEnabled=1\n").unwrap();

            zip.add_directory("plugins", options).unwrap();
            zip.start_file("plugins/sample.asi", options).unwrap();
            zip.write_all(b"fake asi plugin").unwrap();

            zip.finish().unwrap();
        }

        // Test listing
        let entries = list_archive_entries(&zip_path).unwrap();
        assert!(
            entries
                .iter()
                .any(|e| e.relative_path == "OptiScaler.dll" && !e.is_dir)
        );
        assert!(
            entries
                .iter()
                .any(|e| e.relative_path == "OptiScaler.ini" && !e.is_dir)
        );
        assert!(
            entries
                .iter()
                .any(|e| e.relative_path == "plugins/sample.asi" && !e.is_dir)
        );

        // Test extraction
        let extract_dir = temp.path().join("extracted_zip");
        let extracted_entries = extract_archive_to_dir(&zip_path, &extract_dir).unwrap();
        assert_eq!(extracted_entries.len(), entries.len());

        let dll_content = std::fs::read(extract_dir.join("OptiScaler.dll")).unwrap();
        assert_eq!(dll_content, b"fake OptiScaler DLL bytes");

        let ini_content = std::fs::read_to_string(extract_dir.join("OptiScaler.ini")).unwrap();
        assert_eq!(ini_content, "[General]\nEnabled=1\n");

        let asi_content = std::fs::read(extract_dir.join("plugins/sample.asi")).unwrap();
        assert_eq!(asi_content, b"fake asi plugin");
    }

    #[test]
    fn test_sevenz_creation_listing_and_extraction() {
        let temp = tempfile::tempdir().unwrap();
        let src_dir = temp.path().join("sevenz_src");
        std::fs::create_dir_all(src_dir.join("D3D12_OptiScaler")).unwrap();
        std::fs::write(src_dir.join("OptiScaler.dll"), b"sevenz dll content").unwrap();
        std::fs::write(
            src_dir.join("OptiScaler.ini"),
            b"[General]\nLogLevel=debug\n",
        )
        .unwrap();
        std::fs::write(
            src_dir.join("D3D12_OptiScaler/helper.dll"),
            b"d3d12 helper content",
        )
        .unwrap();

        let sevenz_path = temp.path().join("test_optiscaler.7z");
        sevenz_rust2::compress_to_path(&src_dir, &sevenz_path).unwrap();

        // Test listing
        let entries = list_archive_entries(&sevenz_path).unwrap();
        assert!(
            entries
                .iter()
                .any(|e| e.relative_path == "OptiScaler.dll" && !e.is_dir)
        );
        assert!(
            entries
                .iter()
                .any(|e| e.relative_path == "OptiScaler.ini" && !e.is_dir)
        );
        assert!(
            entries
                .iter()
                .any(|e| e.relative_path == "D3D12_OptiScaler/helper.dll" && !e.is_dir)
        );

        // Test extraction
        let extract_dir = temp.path().join("extracted_7z");
        let extracted_entries = extract_archive_to_dir(&sevenz_path, &extract_dir).unwrap();
        assert_eq!(extracted_entries.len(), entries.len());

        assert_eq!(
            std::fs::read(extract_dir.join("OptiScaler.dll")).unwrap(),
            b"sevenz dll content"
        );
        assert_eq!(
            std::fs::read_to_string(extract_dir.join("OptiScaler.ini")).unwrap(),
            "[General]\nLogLevel=debug\n"
        );
        assert_eq!(
            std::fs::read(extract_dir.join("D3D12_OptiScaler/helper.dll")).unwrap(),
            b"d3d12 helper content"
        );
    }

    #[test]
    fn test_mock_http_server_fetch_releases_and_cache_archive() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join("tools_cache");

        // Build a real 7z archive fixture
        let fixture_src = temp.path().join("fixture_src");
        std::fs::create_dir_all(&fixture_src).unwrap();
        std::fs::write(
            fixture_src.join("OptiScaler.dll"),
            b"OptiScaler 0.7.9 binary",
        )
        .unwrap();
        std::fs::write(
            fixture_src.join("OptiScaler.ini"),
            b"[OptiScaler]\nVersion=0.7.9\n",
        )
        .unwrap();
        let archive_file = temp.path().join("OptiScaler-v0.7.9.7z");
        sevenz_rust2::compress_to_path(&fixture_src, &archive_file).unwrap();
        let archive_bytes = std::fs::read(&archive_file).unwrap();
        let archive_sha256 = compute_bytes_sha256(&archive_bytes);

        // Setup mock HTTP server on 127.0.0.1:0
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}");

        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();
        let archive_bytes_clone = archive_bytes.clone();
        let archive_sha256_clone = archive_sha256.clone();

        let server_thread = thread::spawn(move || {
            while running_clone.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut reader = BufReader::new(stream.try_clone().unwrap());
                        let mut request_line = String::new();
                        if reader.read_line(&mut request_line).is_ok() {
                            loop {
                                let mut line = String::new();
                                if reader.read_line(&mut line).is_err()
                                    || line == "\r\n"
                                    || line.is_empty()
                                {
                                    break;
                                }
                            }

                            if request_line.contains("/repos/optiscaler/OptiScaler/releases") {
                                let body = serde_json::json!([
                                    {
                                        "tag_name": "v0.7.9",
                                        "name": "OptiScaler v0.7.9",
                                        "published_at": "2026-01-01T00:00:00Z",
                                        "draft": false,
                                        "prerelease": false,
                                        "body": "OptiScaler v0.7.9 release notes",
                                        "assets": [
                                            {
                                                "name": "OptiScaler-v0.7.9.7z",
                                                "browser_download_url": format!("http://127.0.0.1:{port}/download/OptiScaler-v0.7.9.7z"),
                                                "size": archive_bytes_clone.len(),
                                                "digest": format!("sha256:{archive_sha256_clone}")
                                            }
                                        ]
                                    }
                                ]);
                                let body_bytes = serde_json::to_vec(&body).unwrap();
                                let resp = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                    body_bytes.len()
                                );
                                let _ = stream.write_all(resp.as_bytes());
                                let _ = stream.write_all(&body_bytes);
                            } else if request_line
                                .contains("/repos/optiscaler/OptiScaler-nightly/releases")
                            {
                                let body = serde_json::json!([
                                    {
                                        "tag_name": "v0.8.0-nightly.1",
                                        "name": "OptiScaler Nightly 1",
                                        "published_at": "2026-01-02T00:00:00Z",
                                        "draft": false,
                                        "prerelease": true,
                                        "body": format!("| OptiScaler-nightly.7z | {archive_sha256_clone} |"),
                                        "assets": [
                                            {
                                                "name": "OptiScaler-nightly.7z",
                                                "browser_download_url": format!("http://127.0.0.1:{port}/download/OptiScaler-nightly.7z"),
                                                "size": archive_bytes_clone.len()
                                            }
                                        ]
                                    }
                                ]);
                                let body_bytes = serde_json::to_vec(&body).unwrap();
                                let resp = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                    body_bytes.len()
                                );
                                let _ = stream.write_all(resp.as_bytes());
                                let _ = stream.write_all(&body_bytes);
                            } else if request_line.contains("/download/OptiScaler-v0.7.9.7z")
                                || request_line.contains("/download/OptiScaler-nightly.7z")
                            {
                                let resp = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: application/x-7z-compressed\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                    archive_bytes_clone.len()
                                );
                                let _ = stream.write_all(resp.as_bytes());
                                let _ = stream.write_all(&archive_bytes_clone);
                            } else {
                                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                            }
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        let catalog = GitHubToolCatalog::with_base_url(&cache_dir, base_url).unwrap();
        let descriptor = optiscaler_descriptor();

        // 1. Fetch releases for Stable channel
        let stable_releases = catalog
            .fetch_releases(&descriptor, ToolChannel::Stable)
            .expect("fetch_releases stable succeeds");
        assert_eq!(stable_releases.len(), 1);
        let stable = &stable_releases[0];
        assert_eq!(stable.version, "0.7.9");
        assert_eq!(stable.tag_name, "v0.7.9");
        assert_eq!(stable.asset_name, "OptiScaler-v0.7.9.7z");
        assert_eq!(stable.sha256.as_deref(), Some(archive_sha256.as_str()));
        assert!(stable.cached_path.is_none());

        // 2. Fetch releases for Nightly channel
        let nightly_releases = catalog
            .fetch_releases(&descriptor, ToolChannel::Nightly)
            .expect("fetch_releases nightly succeeds");
        assert_eq!(nightly_releases.len(), 1);
        let nightly = &nightly_releases[0];
        assert_eq!(nightly.version, "0.8.0-nightly.1");
        assert_eq!(nightly.sha256.as_deref(), Some(archive_sha256.as_str()));

        // 3. Ensure cached archive downloads and caches
        let cached = catalog
            .ensure_cached_archive(&descriptor, stable)
            .expect("ensure_cached_archive succeeds");
        assert_eq!(cached.tool_id, descriptor.id);
        assert_eq!(cached.channel, ToolChannel::Stable);
        assert_eq!(cached.version, "0.7.9");
        assert_eq!(cached.sha256, archive_sha256);
        assert!(Path::new(&cached.path).exists());

        // 4. Verify fetch_releases now detects cached_path
        let refetched = catalog
            .fetch_releases(&descriptor, ToolChannel::Stable)
            .expect("fetch_releases detects cached archive");
        assert_eq!(refetched[0].cached_path, Some(cached.path.clone()));

        // 5. Calling ensure_cached_archive again uses the cached file without network request
        let cached_again = catalog
            .ensure_cached_archive(&descriptor, stable)
            .expect("reuse cached archive");
        assert_eq!(cached_again.path, cached.path);

        // 6. Test checksum mismatch rejection
        let mut mismatched = stable.clone();
        mismatched.version = "0.7.9-mismatch".to_string();
        mismatched.sha256 =
            Some("0000000000000000000000000000000000000000000000000000000000000000".to_string());
        let err = catalog
            .ensure_cached_archive(&descriptor, &mismatched)
            .unwrap_err();
        match err {
            ToolError::ChecksumMismatch {
                expected, actual, ..
            } => {
                assert_eq!(
                    expected,
                    "0000000000000000000000000000000000000000000000000000000000000000"
                );
                assert_eq!(actual, archive_sha256);
            }
            other => panic!("Expected ChecksumMismatch, got: {other:?}"),
        }

        // Verify .part file was deleted
        let target_dir = cache_dir
            .join("optiscaler")
            .join("stable")
            .join("0.7.9-mismatch");
        if target_dir.exists() {
            let files: Vec<_> = std::fs::read_dir(&target_dir).unwrap().collect();
            assert!(
                files.is_empty(),
                "Part files must be cleaned up on mismatch"
            );
        }

        // 7. Test cache usage & clear cache
        let (usage_bytes, count) = catalog.cache_usage().expect("cache_usage succeeds");
        assert_eq!(count, 1);
        assert_eq!(usage_bytes, archive_bytes.len() as u64);

        let freed = catalog.clear_cache().expect("clear_cache succeeds");
        assert_eq!(freed, archive_bytes.len() as u64);

        let (usage_after, count_after) = catalog.cache_usage().unwrap();
        assert_eq!(count_after, 0);
        assert_eq!(usage_after, 0);

        // Stop server thread
        running.store(false, Ordering::Relaxed);
        let _ = server_thread.join();
    }

    #[test]
    fn test_file_url_caching_and_integrity_failure_cleanup() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join("tools_cache");

        // 1. Create a valid zip archive file
        let valid_zip = temp.path().join("source.zip");
        {
            let file = File::create(&valid_zip).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("sample.txt", options).unwrap();
            zip.write_all(b"sample data").unwrap();
            zip.finish().unwrap();
        }
        let valid_bytes = std::fs::read(&valid_zip).unwrap();
        let valid_sha256 = compute_bytes_sha256(&valid_bytes);

        let catalog = GitHubToolCatalog::new(&cache_dir).unwrap();
        let descriptor = optiscaler_descriptor();

        let file_url = format!("file://{}", valid_zip.display());
        let release = ToolRelease {
            tool_id: descriptor.id.clone(),
            channel: ToolChannel::Stable,
            version: "1.0.0".to_string(),
            tag_name: "v1.0.0".to_string(),
            name: "OptiScaler 1.0.0".to_string(),
            published_at: "2026-01-01T00:00:00Z".to_string(),
            download_url: file_url,
            asset_name: "source.zip".to_string(),
            size_bytes: valid_bytes.len() as u64,
            sha256: Some(valid_sha256.clone()),
            cached_path: None,
        };

        let cached = catalog
            .ensure_cached_archive(&descriptor, &release)
            .expect("caching from file:// URL succeeds");
        assert_eq!(cached.sha256, valid_sha256);
        assert!(Path::new(&cached.path).exists());

        // 2. Corrupt archive file
        let corrupt_file = temp.path().join("corrupt.zip");
        std::fs::write(&corrupt_file, b"corrupted non-archive data").unwrap();
        let corrupt_url = format!("file://{}", corrupt_file.display());
        let corrupt_release = ToolRelease {
            tool_id: descriptor.id.clone(),
            channel: ToolChannel::Stable,
            version: "1.0.1".to_string(),
            tag_name: "v1.0.1".to_string(),
            name: "OptiScaler 1.0.1".to_string(),
            published_at: "2026-01-01T00:00:00Z".to_string(),
            download_url: corrupt_url,
            asset_name: "corrupt.zip".to_string(),
            size_bytes: 26,
            sha256: None, // Will fail archive integrity check
            cached_path: None,
        };

        let err = catalog
            .ensure_cached_archive(&descriptor, &corrupt_release)
            .unwrap_err();
        match err {
            ToolError::ExtractionFailed(msg) => {
                assert!(msg.contains("integrity check failed"));
            }
            other => panic!("Expected ExtractionFailed, got: {other:?}"),
        }

        // Verify .part file cleaned up
        let target_dir = cache_dir.join("optiscaler").join("stable").join("1.0.1");
        if target_dir.exists() {
            let files: Vec<_> = std::fs::read_dir(&target_dir).unwrap().collect();
            assert!(files.is_empty(), "Part files must be deleted on corruption");
        }
    }

    #[test]
    fn test_extract_archive_rejects_path_traversal() {
        let temp = tempfile::tempdir().unwrap();
        let evil_zip = temp.path().join("evil.zip");

        // Create zip with ../ traversal path
        {
            let file = File::create(&evil_zip).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("../evil.dll", options).unwrap();
            zip.write_all(b"malicious").unwrap();
            zip.finish().unwrap();
        }

        let extract_dir = temp.path().join("extract_target");
        let err = extract_archive_to_dir(&evil_zip, &extract_dir).unwrap_err();
        match err {
            ToolError::ExtractionFailed(msg) => {
                assert!(msg.contains("traversal"));
            }
            other => panic!("Expected ExtractionFailed, got: {other:?}"),
        }
    }

    #[test]
    fn test_fetch_releases_rejects_missing_channel() {
        let catalog = GitHubToolCatalog::new(PathBuf::from("/tmp/cache")).unwrap();
        let mut desc = optiscaler_descriptor();
        desc.channels.clear(); // remove channels

        let err = catalog
            .fetch_releases(&desc, ToolChannel::Stable)
            .unwrap_err();
        match err {
            ToolError::ReleaseNotFound { channel, .. } => {
                assert_eq!(channel, ToolChannel::Stable);
            }
            other => panic!("Expected ReleaseNotFound, got: {other:?}"),
        }
    }
}
