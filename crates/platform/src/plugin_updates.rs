//! Signed, replay-resistant third-party plugin update metadata and package retrieval.
//!
//! The caller supplies the user's pinned publisher key and the currently approved origins.
//! Sequence state is committed only after the complete index has passed validation.

use crate::plugin_network::{FetchClient, Request};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ring::signature::{ED25519, UnparsedPublicKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
};
use thiserror::Error;
use url::Url;

pub const INDEX_SIGNATURE_DOMAIN: &[u8] = b"GameNode/plugin-update-index/v1\0";
const MAX_INDEX_BYTES: usize = 1024 * 1024;
const MAX_PACKAGE_BYTES: u64 = crate::plugin_packages::MAX_COMPRESSED_BYTES;
const MAX_INDEX_RESPONSE_BYTES: u64 = 1_500_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedIndexEnvelope {
    /// Base64 of the exact JSON bytes covered by the detached signature.
    payload: String,
    /// Lowercase hexadecimal Ed25519 signature.
    signature: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum UpdateError {
    #[error("update index is malformed or unsupported")]
    InvalidIndex,
    #[error("update index signature is invalid")]
    InvalidSignature,
    #[error("update index has expired")]
    Expired,
    #[error("update index sequence is a replay")]
    Replay,
    #[error("package URL is not an HTTPS URL")]
    InvalidUrl,
    #[error("package response failed validation")]
    InvalidPackage,
    #[error("update request failed: {0}")]
    Fetch(String),
    #[error("update state I/O failed: {0}")]
    Io(String),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UpdateIndex {
    pub schema: u32,
    pub plugin_id: String,
    pub sequence: u64,
    /// Unix seconds; indexes are rejected at or after this instant.
    pub expires_at: u64,
    pub releases: Vec<UpdateRelease>,
}

/// An index whose signature, expiry, identity schema and replay sequence were accepted by the
/// verifier. The contained parsed index cannot be constructed by downstream callers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedUpdateIndex(UpdateIndex);

impl VerifiedUpdateIndex {
    pub fn index(&self) -> &UpdateIndex {
        &self.0
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UpdateRelease {
    pub version: String,
    pub targets: Vec<String>,
    pub api_range: String,
    pub package_url: String,
    pub package_size: u64,
    pub package_sha256: String,
}

/// Persists each publisher's highest accepted sequence under the app data directory.
#[derive(Clone, Debug)]
pub struct UpdateIndexVerifier {
    root: PathBuf,
    commit_lock: Arc<Mutex<()>>,
}

impl UpdateIndexVerifier {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, UpdateError> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(io_error)?;
        restrict_owner_dir(&root).map_err(io_error)?;
        Ok(Self {
            root,
            commit_lock: Arc::new(Mutex::new(())),
        })
    }

    pub fn verify(
        &self,
        raw: &[u8],
        signature: &[u8],
        key: &[u8],
        now_unix: u64,
    ) -> Result<UpdateIndex, UpdateError> {
        if raw.is_empty() || raw.len() > MAX_INDEX_BYTES || key.len() != 32 || signature.len() != 64
        {
            return Err(UpdateError::InvalidIndex);
        }
        let mut signed = Vec::with_capacity(INDEX_SIGNATURE_DOMAIN.len() + raw.len());
        signed.extend_from_slice(INDEX_SIGNATURE_DOMAIN);
        signed.extend_from_slice(raw);
        UnparsedPublicKey::new(&ED25519, key)
            .verify(&signed, signature)
            .map_err(|_| UpdateError::InvalidSignature)?;
        let index: UpdateIndex =
            serde_json::from_slice(raw).map_err(|_| UpdateError::InvalidIndex)?;
        validate_index(&index)?;
        if index.expires_at <= now_unix {
            return Err(UpdateError::Expired);
        }
        let _guard = self
            .commit_lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let state_dir = self.root.join(safe_id(&index.plugin_id));
        fs::create_dir_all(&state_dir).map_err(io_error)?;
        restrict_owner_dir(&state_dir).map_err(io_error)?;
        let previous = highest_sequence(&state_dir)?;
        if index.sequence <= previous {
            return Err(UpdateError::Replay);
        }
        let path = state_dir.join(format!("seq-{}", index.sequence));
        let mut record = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| {
                if error.kind() == io::ErrorKind::AlreadyExists {
                    UpdateError::Replay
                } else {
                    io_error(error)
                }
            })?;
        record
            .write_all(&index.sequence.to_le_bytes())
            .map_err(io_error)?;
        record.sync_all().map_err(io_error)?;
        sync_dir(&state_dir).map_err(io_error)?;
        Ok(index)
    }

    /// Fetch and verify the signed JSON envelope for one per-plugin update feed. The network
    /// client rechecks origin approval, DNS results and HTTPS at every redirect hop.
    pub async fn check_feed(
        &self,
        client: &FetchClient,
        feed_url: &str,
        publisher_key: &[u8],
        now_unix: u64,
        allowed_origin: impl Fn(&str) -> bool + Send + Sync,
    ) -> Result<VerifiedUpdateIndex, UpdateError> {
        let request = Request::build(
            feed_url,
            "GET",
            [],
            Vec::new(),
            MAX_INDEX_RESPONSE_BYTES,
            30_000,
        )
        .map_err(|_| UpdateError::InvalidUrl)?;
        let response = client
            .fetch(request, allowed_origin)
            .await
            .map_err(|error| UpdateError::Fetch(error.to_string()))?;
        if response.status != 200 || response.body.len() as u64 > MAX_INDEX_RESPONSE_BYTES {
            return Err(UpdateError::InvalidIndex);
        }
        let envelope: SignedIndexEnvelope =
            serde_json::from_slice(&response.body).map_err(|_| UpdateError::InvalidIndex)?;
        let raw = STANDARD
            .decode(envelope.payload)
            .map_err(|_| UpdateError::InvalidIndex)?;
        let signature = decode_signature(&envelope.signature)?;
        self.verify(&raw, &signature, publisher_key, now_unix)
            .map(VerifiedUpdateIndex)
    }

    /// Downloads a release through the same HTTPS, public-address, redirect and timeout checks
    /// used for plugin network fetches. Package bytes remain inert until the caller verifies and
    /// stages them. The response is bounded by the package archive size policy.
    pub async fn download_release(
        &self,
        client: &FetchClient,
        release: &UpdateRelease,
        allowed_origin: impl Fn(&str) -> bool + Send + Sync,
    ) -> Result<Vec<u8>, UpdateError> {
        validate_release(release)?;
        let request = Request::build_package_download(&release.package_url, release.package_size)
            .map_err(|_| UpdateError::InvalidUrl)?;
        let response = client
            .fetch(request, move |origin| allowed_origin(origin))
            .await
            .map_err(|error| UpdateError::Fetch(error.to_string()))?;
        let bytes = response.body;
        if response.status != 200
            || bytes.len() as u64 != release.package_size
            || format!("{:x}", Sha256::digest(&bytes)) != release.package_sha256
        {
            return Err(UpdateError::InvalidPackage);
        }
        Ok(bytes)
    }
}

fn decode_signature(value: &str) -> Result<Vec<u8>, UpdateError> {
    if value.len() != 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(UpdateError::InvalidIndex);
    }
    (0..64)
        .map(|index| {
            u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
                .map_err(|_| UpdateError::InvalidIndex)
        })
        .collect()
}

fn validate_index(index: &UpdateIndex) -> Result<(), UpdateError> {
    if index.schema != 1
        || !valid_id(&index.plugin_id)
        || index.sequence == 0
        || index.expires_at == 0
        || index.releases.is_empty()
        || index.releases.len() > 128
    {
        return Err(UpdateError::InvalidIndex);
    }
    for release in &index.releases {
        validate_release(release)?;
    }
    Ok(())
}

fn validate_release(release: &UpdateRelease) -> Result<(), UpdateError> {
    let url = Url::parse(&release.package_url).map_err(|_| UpdateError::InvalidUrl)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || release.package_size == 0
        || release.package_size > MAX_PACKAGE_BYTES
        || release.package_sha256.len() != 64
        || !release
            .package_sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || release.targets.is_empty()
        || release
            .targets
            .iter()
            .any(|target| target.is_empty() || target.len() > 128)
        || semver::Version::parse(&release.version).is_err()
        || semver::VersionReq::parse(&release.api_range).is_err()
    {
        return Err(UpdateError::InvalidIndex);
    }
    Ok(())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
}
fn safe_id(id: &str) -> String {
    format!("{:x}", Sha256::digest(id.as_bytes()))
}
fn highest_sequence(root: &std::path::Path) -> Result<u64, UpdateError> {
    let mut highest = 0u64;
    for entry in fs::read_dir(root).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let name = entry.file_name();
        let name = name.to_str().ok_or(UpdateError::InvalidIndex)?;
        if let Some(value) = name.strip_prefix("seq-") {
            highest = highest.max(
                value
                    .parse::<u64>()
                    .map_err(|_| UpdateError::InvalidIndex)?,
            );
        }
    }
    Ok(highest)
}
fn sync_dir(path: &std::path::Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
#[cfg(unix)]
fn restrict_owner_dir(path: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}
#[cfg(not(unix))]
fn restrict_owner_dir(_: &std::path::Path) -> io::Result<()> {
    Ok(())
}
fn io_error(error: io::Error) -> UpdateError {
    UpdateError::Io(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use tempfile::tempdir;

    fn signed(key: &Ed25519KeyPair, sequence: u64, expires_at: u64) -> (Vec<u8>, Vec<u8>) {
        let index = UpdateIndex {
            schema: 1,
            plugin_id: "com.example.plugin".into(),
            sequence,
            expires_at,
            releases: vec![UpdateRelease {
                version: "1.2.3".into(),
                targets: vec!["linux-x64".into()],
                api_range: ">=1.0.0, <2.0.0".into(),
                package_url: "https://updates.example/plugin.zip".into(),
                package_size: 10,
                package_sha256: "a".repeat(64),
            }],
        };
        let raw = serde_json::to_vec(&index).unwrap();
        let mut message = INDEX_SIGNATURE_DOMAIN.to_vec();
        message.extend_from_slice(&raw);
        (raw, key.sign(&message).as_ref().to_vec())
    }
    fn key() -> Ed25519KeyPair {
        Ed25519KeyPair::from_seed_unchecked(&[7u8; 32]).unwrap()
    }

    #[test]
    fn rejects_invalid_signature_expiry_and_replay_without_advancing_state() {
        let dir = tempdir().unwrap();
        let verifier = UpdateIndexVerifier::open(dir.path()).unwrap();
        let key = key();
        let (raw, signature) = signed(&key, 1, 200);
        assert_eq!(
            verifier.verify(&raw, &[0; 64], key.public_key().as_ref(), 100),
            Err(UpdateError::InvalidSignature)
        );
        assert_eq!(
            verifier.verify(&raw, &signature, key.public_key().as_ref(), 200),
            Err(UpdateError::Expired)
        );
        assert_eq!(
            verifier
                .verify(&raw, &signature, key.public_key().as_ref(), 100)
                .unwrap()
                .sequence,
            1
        );
        assert_eq!(
            verifier.verify(&raw, &signature, key.public_key().as_ref(), 100),
            Err(UpdateError::Replay)
        );
        let (next, sig) = signed(&key, 2, 300);
        assert_eq!(
            verifier
                .verify(&next, &sig, key.public_key().as_ref(), 100)
                .unwrap()
                .sequence,
            2
        );
        let reopened = UpdateIndexVerifier::open(dir.path()).unwrap();
        assert_eq!(
            reopened.verify(&next, &sig, key.public_key().as_ref(), 100),
            Err(UpdateError::Replay),
            "accepted sequences survive verifier restart"
        );
    }

    #[test]
    fn rejects_non_https_and_oversized_release_metadata() {
        let mut release = UpdateRelease {
            version: "1.0.0".into(),
            targets: vec!["linux-x64".into()],
            api_range: "^1".into(),
            package_url: "http://example.com/a".into(),
            package_size: 10,
            package_sha256: "a".repeat(64),
        };
        assert_eq!(validate_release(&release), Err(UpdateError::InvalidIndex));
        release.package_url = "https://example.com/a".into();
        release.package_size = MAX_PACKAGE_BYTES + 1;
        assert_eq!(validate_release(&release), Err(UpdateError::InvalidIndex));
    }
}
