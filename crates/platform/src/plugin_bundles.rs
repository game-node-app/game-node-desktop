//! Signed bundle manifests for multi-package plugin distribution.
//!
//! A bundle is a ZIP envelope: `bundle.json`, `components/*.gnplugin` and `signature.ed25519`.
//! This module owns the manifest contract and its signature domain. The archive extractor and
//! the per-component package verification live with the package/registry code; a bundle only
//! groups packages that are validated exactly as if imported alone.

use base64::Engine as _;
use ring::signature::{ED25519, UnparsedPublicKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Seek};
use std::path::Path;
use zip::ZipArchive;

use crate::plugin_packages::{
    MAX_COMPRESSED_BYTES, MAX_ENTRIES, MAX_EXPANDED_BYTES, MAX_METADATA_BYTES,
    parse_integrity_json, verify_integrity_signature, verify_payload_hashes,
};

/// ASCII domain label followed by one NUL byte; signed before the raw `bundle.json` bytes.
pub const BUNDLE_SIGNATURE_DOMAIN_PREFIX: &[u8] = b"GameNode/plugin-bundle/v1\0";
/// The manifest name at the bundle root.
pub const BUNDLE_MANIFEST_NAME: &str = "bundle.json";
/// Component archives live below this package-relative directory.
pub const COMPONENTS_DIRECTORY: &str = "components";
/// Most components one bundle may carry.
pub const MAX_BUNDLE_COMPONENTS: usize = 8;
/// Largest `bundle.json` accepted, in bytes.
pub const MAX_BUNDLE_MANIFEST_BYTES: u64 = 256 * 1024;
/// Largest bundle archive accepted, in bytes.
pub const MAX_BUNDLE_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
/// Largest expanded bundle accepted, in bytes.
pub const MAX_BUNDLE_EXPANDED_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BundleError {
    #[error("bundle manifest is malformed or exceeds the size limit")]
    InvalidManifest,
    #[error("bundle manifest has an unsupported schema version")]
    UnsupportedSchema,
    #[error("bundle publisher key or signature has an invalid length")]
    InvalidKeyOrSignature,
    #[error("bundle signature is invalid")]
    InvalidSignature,
    #[error("bundle component path is unsafe or not a component archive")]
    InvalidComponentPath,
    #[error("bundle repeats a component plugin id or archive path")]
    DuplicateComponent,
    #[error("bundle carries too many components")]
    TooManyComponents,
    #[error("bundle component kind must be \"provider\" or \"extension\"")]
    UnsupportedComponentKind,
    #[error("bundle archive is malformed or contains an unexpected entry")]
    InvalidArchive,
    #[error("bundle or component exceeds an archive size or entry limit")]
    ArchiveLimit,
    #[error("bundle component is not a valid signed package")]
    InvalidComponent,
    #[error("bundle component identity does not match the bundle manifest")]
    ComponentMismatch,
}

/// A verified bundle: the signed manifest plus the identity of every component package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedBundle {
    pub manifest: BundleManifest,
    pub components: Vec<VerifiedBundleComponent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedBundleComponent {
    pub path: String,
    pub plugin_id: String,
    pub kind: String,
    pub version: String,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleManifest {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub publisher: BundlePublisher,
    pub components: Vec<BundleComponent>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundlePublisher {
    pub key_id: String,
    /// Base64-encoded 32-byte Ed25519 public key, matching the component publisher keys.
    pub public_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleComponent {
    /// Package-relative archive path below `components/`, ending in `.gnplugin`.
    pub path: String,
    /// Lowercase SHA-256 of the exact component archive bytes.
    pub sha256: String,
    pub plugin_id: String,
    /// `provider` or `extension`.
    pub kind: String,
}

impl BundleManifest {
    pub fn publisher_key(&self) -> Result<[u8; 32], BundleError> {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(self.publisher.public_key.as_bytes())
            .map_err(|_| BundleError::InvalidKeyOrSignature)?;
        decoded
            .try_into()
            .map_err(|_| BundleError::InvalidKeyOrSignature)
    }
}

fn valid_text(value: &str, max: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= max
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn valid_identifier(value: &str) -> bool {
    valid_text(value, 128)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn valid_component_path(path: &str) -> bool {
    let Some(relative) = path.strip_prefix("components/") else {
        return false;
    };
    relative.len() > ".gnplugin".len()
        && relative.ends_with(".gnplugin")
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && path.split('/').all(|segment| {
            !segment.is_empty() && segment != "." && segment != ".." && !segment.contains(':')
        })
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Parse and validate the bounded bundle manifest. The caller verifies the detached signature
/// before trusting any field.
pub fn parse_bundle_manifest(raw: &[u8]) -> Result<BundleManifest, BundleError> {
    if raw.is_empty() || raw.len() as u64 > MAX_BUNDLE_MANIFEST_BYTES {
        return Err(BundleError::InvalidManifest);
    }
    let manifest: BundleManifest =
        serde_json::from_slice(raw).map_err(|_| BundleError::InvalidManifest)?;
    if manifest.schema != 1 {
        return Err(BundleError::UnsupportedSchema);
    }
    if !valid_identifier(&manifest.id) || !valid_text(&manifest.name, 200) {
        return Err(BundleError::InvalidManifest);
    }
    if semver::Version::parse(&manifest.version).is_err() {
        return Err(BundleError::InvalidManifest);
    }
    if !valid_text(&manifest.publisher.key_id, 128) || manifest.publisher_key().is_err() {
        return Err(BundleError::InvalidKeyOrSignature);
    }
    if manifest.components.is_empty() {
        return Err(BundleError::InvalidManifest);
    }
    if manifest.components.len() > MAX_BUNDLE_COMPONENTS {
        return Err(BundleError::TooManyComponents);
    }
    let mut plugin_ids = std::collections::BTreeSet::new();
    let mut paths = std::collections::BTreeSet::new();
    for component in &manifest.components {
        if !valid_component_path(&component.path) {
            return Err(BundleError::InvalidComponentPath);
        }
        if !valid_digest(&component.sha256) || !valid_identifier(&component.plugin_id) {
            return Err(BundleError::InvalidManifest);
        }
        if !matches!(component.kind.as_str(), "provider" | "extension") {
            return Err(BundleError::UnsupportedComponentKind);
        }
        if !plugin_ids.insert(component.plugin_id.as_str())
            || !paths.insert(component.path.to_ascii_lowercase())
        {
            return Err(BundleError::DuplicateComponent);
        }
    }
    Ok(manifest)
}

/// Verify Ed25519 over the fixed domain prefix plus the exact raw `bundle.json` bytes.
pub fn verify_bundle_signature(
    public_key: &[u8],
    raw_manifest: &[u8],
    signature: &[u8],
) -> Result<(), BundleError> {
    if public_key.len() != 32 || signature.len() != 64 {
        return Err(BundleError::InvalidKeyOrSignature);
    }
    let mut signed = Vec::with_capacity(BUNDLE_SIGNATURE_DOMAIN_PREFIX.len() + raw_manifest.len());
    signed.extend_from_slice(BUNDLE_SIGNATURE_DOMAIN_PREFIX);
    signed.extend_from_slice(raw_manifest);
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(&signed, signature)
        .map_err(|_| BundleError::InvalidSignature)
}

/// The manifest's declared key must equal the key that actually verified it, so a bundle cannot
/// point trust at a key different from the one used to sign it.
pub fn verify_bundle_manifest(
    raw_manifest: &[u8],
    publisher_key: &[u8],
    signature: &[u8],
) -> Result<BundleManifest, BundleError> {
    verify_bundle_signature(publisher_key, raw_manifest, signature)?;
    let manifest = parse_bundle_manifest(raw_manifest)?;
    if manifest.publisher_key()? != publisher_key {
        return Err(BundleError::InvalidSignature);
    }
    Ok(manifest)
}

/// The optional development-key marker at the bundle root.
pub const DEVELOPMENT_KEY: &str = "DEVELOPMENT-KEY.txt";

/// Verify a bundle archive from disk without extracting or executing it.
pub fn verify_bundle_archive(path: &Path) -> Result<VerifiedBundle, BundleError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| BundleError::InvalidArchive)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_BUNDLE_ARCHIVE_BYTES
    {
        return Err(BundleError::ArchiveLimit);
    }
    let file = std::fs::File::open(path).map_err(|_| BundleError::InvalidArchive)?;
    verify_bundle_reader(std::io::BufReader::new(file))
}

/// Verify a bundle held in memory; used by fixtures and import staging.
pub fn verify_bundle_bytes(bytes: &[u8]) -> Result<VerifiedBundle, BundleError> {
    if bytes.len() as u64 > MAX_BUNDLE_ARCHIVE_BYTES {
        return Err(BundleError::ArchiveLimit);
    }
    verify_bundle_reader(Cursor::new(bytes))
}

fn read_bounded<R: Read>(mut reader: R, limit: usize) -> Result<Vec<u8>, BundleError> {
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    reader
        .read_to_end(&mut bytes)
        .map_err(|_| BundleError::InvalidArchive)?;
    if bytes.len() > limit {
        return Err(BundleError::ArchiveLimit);
    }
    Ok(bytes)
}

fn safe_relative(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('\\')
        && !name.contains('\0')
        && !name.starts_with('/')
        && !name.chars().any(char::is_control)
        && name
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".." && !part.contains(':'))
}

fn valid_outer_name(name: &str) -> bool {
    name == BUNDLE_MANIFEST_NAME
        || name == "signature.ed25519"
        || name == DEVELOPMENT_KEY
        || valid_component_path(name)
}

fn verify_bundle_reader<R: Read + Seek>(reader: R) -> Result<VerifiedBundle, BundleError> {
    let mut archive = ZipArchive::new(reader).map_err(|_| BundleError::InvalidArchive)?;
    if archive.len() > MAX_BUNDLE_COMPONENTS + 3 {
        return Err(BundleError::ArchiveLimit);
    }
    let mut files = BTreeMap::new();
    let mut folded = BTreeSet::new();
    let mut expanded = 0u64;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|_| BundleError::InvalidArchive)?;
        if entry.is_dir() {
            return Err(BundleError::InvalidArchive);
        }
        let name = entry.name().to_owned();
        if !valid_outer_name(&name) || !folded.insert(name.to_ascii_lowercase()) {
            return Err(BundleError::InvalidArchive);
        }
        expanded = expanded
            .checked_add(entry.size())
            .ok_or(BundleError::ArchiveLimit)?;
        if expanded > MAX_BUNDLE_EXPANDED_BYTES {
            return Err(BundleError::ArchiveLimit);
        }
        let limit = match name.as_str() {
            BUNDLE_MANIFEST_NAME => MAX_BUNDLE_MANIFEST_BYTES as usize,
            "signature.ed25519" => 64,
            DEVELOPMENT_KEY => 64 * 1024,
            _ => MAX_COMPRESSED_BYTES as usize,
        };
        let data = read_bounded(entry.take(limit as u64 + 1), limit)?;
        files.insert(name, data);
    }

    let raw_manifest = files.remove(BUNDLE_MANIFEST_NAME).ok_or(BundleError::InvalidManifest)?;
    let signature = files
        .remove("signature.ed25519")
        .ok_or(BundleError::InvalidKeyOrSignature)?;
    let manifest = parse_bundle_manifest(&raw_manifest)?;
    let publisher_key = manifest.publisher_key()?;
    verify_bundle_signature(&publisher_key, &raw_manifest, &signature)?;

    let mut components = Vec::with_capacity(manifest.components.len());
    for component in &manifest.components {
        let bytes = files
            .remove(&component.path)
            .ok_or(BundleError::InvalidComponent)?;
        if format!("{:x}", Sha256::digest(&bytes)) != component.sha256 {
            return Err(BundleError::InvalidComponent);
        }
        let identity = verify_component(&bytes, &publisher_key)?;
        if identity.id != component.plugin_id || identity.kind != component.kind {
            return Err(BundleError::ComponentMismatch);
        }
        components.push(VerifiedBundleComponent {
            path: component.path.clone(),
            plugin_id: identity.id,
            kind: identity.kind,
            version: identity.version,
            sha256: component.sha256.clone(),
        });
    }
    files.remove(DEVELOPMENT_KEY);
    if !files.is_empty() {
        return Err(BundleError::InvalidArchive);
    }
    Ok(VerifiedBundle {
        manifest,
        components,
    })
}

#[derive(Deserialize)]
struct ComponentManifest {
    id: String,
    kind: String,
    version: String,
    publisher: ComponentPublisher,
}

#[derive(Deserialize)]
struct ComponentPublisher {
    public_key: String,
}

struct ComponentIdentity {
    id: String,
    kind: String,
    version: String,
}

fn verify_component(bytes: &[u8], publisher_key: &[u8; 32]) -> Result<ComponentIdentity, BundleError> {
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).map_err(|_| BundleError::InvalidComponent)?;
    if archive.len() > MAX_ENTRIES {
        return Err(BundleError::ArchiveLimit);
    }
    let mut payload = BTreeMap::new();
    let mut integrity = None;
    let mut signature = None;
    let mut folded = BTreeSet::new();
    let mut expanded = 0u64;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|_| BundleError::InvalidComponent)?;
        if entry.is_dir() {
            return Err(BundleError::InvalidComponent);
        }
        let name = entry.name().to_owned();
        if !safe_relative(&name) || !folded.insert(name.to_ascii_lowercase()) {
            return Err(BundleError::InvalidComponent);
        }
        expanded = expanded
            .checked_add(entry.size())
            .ok_or(BundleError::ArchiveLimit)?;
        if expanded > MAX_EXPANDED_BYTES {
            return Err(BundleError::ArchiveLimit);
        }
        let limit = if name == "integrity.json"
            || name == "signature.ed25519"
            || name == "plugin.json"
            || name.starts_with("licenses/")
        {
            MAX_METADATA_BYTES as usize
        } else {
            MAX_COMPRESSED_BYTES as usize
        };
        let data = read_bounded(entry.take(limit as u64 + 1), limit)?;
        match name.as_str() {
            "integrity.json" => integrity = Some(data),
            "signature.ed25519" => signature = Some(data),
            _ => {
                payload.insert(name, data);
            }
        }
    }
    let integrity = integrity.ok_or(BundleError::InvalidComponent)?;
    let signature = signature.ok_or(BundleError::InvalidComponent)?;
    verify_integrity_signature(publisher_key, &integrity, &signature)
        .map_err(|_| BundleError::InvalidComponent)?;
    let manifest =
        parse_integrity_json(&integrity).map_err(|_| BundleError::InvalidComponent)?;
    verify_payload_hashes(&manifest.files.0, &payload)
        .map_err(|_| BundleError::InvalidComponent)?;
    let plugin = payload
        .get("plugin.json")
        .ok_or(BundleError::InvalidComponent)?;
    let identity: ComponentManifest =
        serde_json::from_slice(plugin).map_err(|_| BundleError::InvalidComponent)?;
    let declared = base64::engine::general_purpose::STANDARD
        .decode(identity.publisher.public_key.as_bytes())
        .map_err(|_| BundleError::InvalidComponent)?;
    if declared.as_slice() != publisher_key {
        return Err(BundleError::ComponentMismatch);
    }
    if !valid_identifier(&identity.id) || semver::Version::parse(&identity.version).is_err() {
        return Err(BundleError::InvalidComponent);
    }
    if !matches!(identity.kind.as_str(), "provider" | "extension") {
        return Err(BundleError::UnsupportedComponentKind);
    }
    Ok(ComponentIdentity {
        id: identity.id,
        kind: identity.kind,
        version: identity.version,
    })
}

#[cfg(test)]
mod tests {
    use ring::signature::{Ed25519KeyPair, KeyPair};

    use super::*;

    fn manifest(key: &[u8]) -> Vec<u8> {
        let public = base64::engine::general_purpose::STANDARD.encode(key);
        serde_json::to_vec(&serde_json::json!({
            "schema": 1,
            "id": "org.example.dolphin",
            "name": "Dolphin GameCube and Wii",
            "version": "0.1.0",
            "publisher": { "key_id": "dolphin-dev", "public_key": public },
            "components": [
                {
                    "path": "components/provider.gnplugin",
                    "sha256": "a".repeat(64),
                    "plugin_id": "org.example.dolphin",
                    "kind": "provider"
                },
                {
                    "path": "components/launch.gnplugin",
                    "sha256": "b".repeat(64),
                    "plugin_id": "org.example.dolphin.launch",
                    "kind": "extension"
                }
            ]
        }))
        .unwrap()
    }

    fn signed(key: &Ed25519KeyPair, raw: &[u8]) -> Vec<u8> {
        let mut payload = BUNDLE_SIGNATURE_DOMAIN_PREFIX.to_vec();
        payload.extend_from_slice(raw);
        key.sign(&payload).as_ref().to_vec()
    }

    #[test]
    fn a_bundle_manifest_parses_with_its_publisher_and_components() {
        let key = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
        let public = key.public_key().as_ref();
        let raw = manifest(public);
        let parsed = parse_bundle_manifest(&raw).unwrap();
        assert_eq!(parsed.id, "org.example.dolphin");
        assert_eq!(parsed.version, "0.1.0");
        assert_eq!(parsed.publisher.key_id, "dolphin-dev");
        assert_eq!(parsed.publisher_key().unwrap(), public);
        assert_eq!(parsed.components.len(), 2);
        assert_eq!(parsed.components[0].kind, "provider");
        assert_eq!(parsed.components[1].kind, "extension");
        assert_eq!(parsed.components[0].sha256, "a".repeat(64));
    }

    #[test]
    fn the_signature_covers_the_exact_manifest_bytes() {
        let key = Ed25519KeyPair::from_seed_unchecked(&[8; 32]).unwrap();
        let public = key.public_key().as_ref();
        let raw = manifest(public);
        let signature = signed(&key, &raw);
        verify_bundle_signature(public, &raw, &signature).unwrap();
        let mut tampered = raw.clone();
        tampered.push(b' ');
        assert_eq!(
            verify_bundle_signature(public, &tampered, &signature),
            Err(BundleError::InvalidSignature)
        );
        let mut bad_signature = signature.clone();
        bad_signature[0] ^= 1;
        assert_eq!(
            verify_bundle_signature(public, &raw, &bad_signature),
            Err(BundleError::InvalidSignature)
        );
    }

    #[test]
    fn a_manifest_pointing_at_another_key_is_rejected() {
        let signer = Ed25519KeyPair::from_seed_unchecked(&[9; 32]).unwrap();
        let other = Ed25519KeyPair::from_seed_unchecked(&[10; 32]).unwrap();
        let raw = manifest(signer.public_key().as_ref());
        let signature = signed(&signer, &raw);
        assert_eq!(
            verify_bundle_manifest(&raw, other.public_key().as_ref(), &signature),
            Err(BundleError::InvalidSignature)
        );
    }

    #[test]
    fn unsafe_or_duplicate_components_are_rejected() {
        let key = Ed25519KeyPair::from_seed_unchecked(&[11; 32]).unwrap();
        let public = base64::engine::general_purpose::STANDARD.encode(key.public_key().as_ref());
        for path in [
            "components/../escape.gnplugin",
            "components//empty.gnplugin",
            "/components/absolute.gnplugin",
            "components/not-a-package.zip",
        ] {
            let raw = serde_json::to_vec(&serde_json::json!({
                "schema": 1,
                "id": "org.example.bundle",
                "name": "Bundle",
                "version": "1.0.0",
                "publisher": { "key_id": "key", "public_key": public },
                "components": [{ "path": path, "sha256": "a".repeat(64), "plugin_id": "org.example.a", "kind": "provider" }]
            }))
            .unwrap();
            assert_eq!(
                parse_bundle_manifest(&raw),
                Err(BundleError::InvalidComponentPath),
                "{path}"
            );
        }
        let raw = serde_json::to_vec(&serde_json::json!({
            "schema": 1,
            "id": "org.example.bundle",
            "name": "Bundle",
            "version": "1.0.0",
            "publisher": { "key_id": "key", "public_key": public },
            "components": [
                { "path": "components/a.gnplugin", "sha256": "a".repeat(64), "plugin_id": "org.example.a", "kind": "provider" },
                { "path": "components/b.gnplugin", "sha256": "b".repeat(64), "plugin_id": "org.example.a", "kind": "provider" }
            ]
        }))
        .unwrap();
        assert_eq!(
            parse_bundle_manifest(&raw),
            Err(BundleError::DuplicateComponent)
        );
    }

    #[test]
    fn schema_version_kind_and_size_are_bounded() {
        let key = Ed25519KeyPair::from_seed_unchecked(&[12; 32]).unwrap();
        let public = base64::engine::general_purpose::STANDARD.encode(key.public_key().as_ref());
        let make = |schema: u32, kind: &str, count: usize| {
            let components: Vec<_> = (0..count)
                .map(|index| {
                    serde_json::json!({
                        "path": format!("components/{index}.gnplugin"),
                        "sha256": "a".repeat(64),
                        "plugin_id": format!("org.example.{index}"),
                        "kind": kind
                    })
                })
                .collect();
            serde_json::to_vec(&serde_json::json!({
                "schema": schema,
                "id": "org.example.bundle",
                "name": "Bundle",
                "version": "1.0.0",
                "publisher": { "key_id": "key", "public_key": public },
                "components": components
            }))
            .unwrap()
        };
        assert_eq!(
            parse_bundle_manifest(&make(2, "provider", 1)),
            Err(BundleError::UnsupportedSchema)
        );
        assert_eq!(
            parse_bundle_manifest(&make(1, "store", 1)),
            Err(BundleError::UnsupportedComponentKind)
        );
        assert_eq!(
            parse_bundle_manifest(&make(1, "provider", MAX_BUNDLE_COMPONENTS + 1)),
            Err(BundleError::TooManyComponents)
        );
        assert!(
            parse_bundle_manifest(&vec![b' '; MAX_BUNDLE_MANIFEST_BYTES as usize + 1]).is_err()
        );
    }

    use std::io::{Cursor, Read, Write};

    use zip::{ZipWriter, write::SimpleFileOptions};

    fn component_archive(
        plugin_id: &str,
        kind: &str,
        version: &str,
        key: &Ed25519KeyPair,
        declared_key: Option<&[u8]>,
    ) -> Vec<u8> {
        let declared = declared_key.unwrap_or(key.public_key().as_ref());
        let public = base64::engine::general_purpose::STANDARD.encode(declared);
        let manifest = serde_json::to_vec(&serde_json::json!({
            "schema": 1,
            "id": plugin_id,
            "kind": kind,
            "name": "Component",
            "version": version,
            "api": { "min": "1.6.0", "max": "1.6.0" },
            "permissions": [],
            "targets": [{ "os": "linux", "arch": "x86_64", "abi": "gnu", "entrypoint": "bin/plugin" }],
            "publisher": { "key_id": "bundle-dev", "public_key": public },
            "license": { "expression": "MIT", "files": ["licenses/LICENSE"] }
        }))
        .unwrap();
        let files = BTreeMap::from([
            ("bin/plugin".to_owned(), b"fixture executable".to_vec()),
            ("licenses/LICENSE".to_owned(), b"fixture license".to_vec()),
            ("plugin.json".to_owned(), manifest),
        ]);
        let hashes = files
            .iter()
            .map(|(path, bytes)| (path.clone(), format!("{:x}", Sha256::digest(bytes))))
            .collect::<BTreeMap<_, _>>();
        let integrity = serde_json::to_vec(&serde_json::json!({ "schema": 1, "files": hashes }))
            .unwrap();
        let mut signed = crate::plugin_packages::SIGNATURE_DOMAIN_PREFIX.to_vec();
        signed.extend_from_slice(&integrity);
        let signature = key.sign(&signed).as_ref().to_vec();
        let mut entries: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
            .collect();
        entries.push(("integrity.json", &integrity));
        entries.push(("signature.ed25519", &signature));
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in entries {
            writer
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn bundle_archive(
        key: &Ed25519KeyPair,
        components: &[(&str, &str, &str, Vec<u8>)],
        development: bool,
    ) -> Vec<u8> {
        let public = base64::engine::general_purpose::STANDARD.encode(key.public_key().as_ref());
        let declared = components
            .iter()
            .map(|(file, plugin_id, kind, bytes)| {
                serde_json::json!({
                    "path": format!("components/{file}"),
                    "sha256": format!("{:x}", Sha256::digest(bytes)),
                    "plugin_id": plugin_id,
                    "kind": kind
                })
            })
            .collect::<Vec<_>>();
        let manifest = serde_json::to_vec(&serde_json::json!({
            "schema": 1,
            "id": "org.example.bundle",
            "name": "Bundle",
            "version": "0.1.0",
            "publisher": { "key_id": "bundle-dev", "public_key": public },
            "components": declared
        }))
        .unwrap();
        let mut payload = BUNDLE_SIGNATURE_DOMAIN_PREFIX.to_vec();
        payload.extend_from_slice(&manifest);
        let signature = key.sign(&payload).as_ref().to_vec();
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(BUNDLE_MANIFEST_NAME, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(&manifest).unwrap();
        for (file, _, _, bytes) in components {
            writer
                .start_file(format!("components/{file}"), SimpleFileOptions::default())
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer
            .start_file("signature.ed25519", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(&signature).unwrap();
        if development {
            writer
                .start_file(DEVELOPMENT_KEY, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"DEVELOPMENT KEY ONLY").unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn sample_components(
        key: &Ed25519KeyPair,
    ) -> Vec<(&'static str, &'static str, &'static str, Vec<u8>)> {
        vec![
            (
                "provider.gnplugin",
                "org.example.provider",
                "provider",
                component_archive("org.example.provider", "provider", "0.1.0", key, None),
            ),
            (
                "launch.gnplugin",
                "org.example.launch",
                "extension",
                component_archive("org.example.launch", "extension", "0.1.0", key, None),
            ),
        ]
    }

    #[test]
    fn a_valid_bundle_verifies_every_component() {
        let key = Ed25519KeyPair::from_seed_unchecked(&[21; 32]).unwrap();
        let components = sample_components(&key);
        let bytes = bundle_archive(&key, &components, true);
        let verified = verify_bundle_bytes(&bytes).unwrap();
        assert_eq!(verified.manifest.id, "org.example.bundle");
        assert_eq!(verified.manifest.version, "0.1.0");
        assert_eq!(verified.components.len(), 2);
        assert_eq!(verified.components[0].plugin_id, "org.example.provider");
        assert_eq!(verified.components[0].kind, "provider");
        assert_eq!(verified.components[1].plugin_id, "org.example.launch");
        assert_eq!(verified.components[1].kind, "extension");
        assert_eq!(verified.components[0].version, "0.1.0");
    }

    #[test]
    fn tampered_and_missing_components_are_rejected() {
        let key = Ed25519KeyPair::from_seed_unchecked(&[22; 32]).unwrap();
        let components = sample_components(&key);
        let valid = bundle_archive(&key, &components, false);

        let tampered = {
            let mut source = ZipArchive::new(Cursor::new(valid.clone())).unwrap();
            let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
            for index in 0..source.len() {
                let mut entry = source.by_index(index).unwrap();
                let name = entry.name().to_owned();
                let mut data = Vec::new();
                entry.read_to_end(&mut data).unwrap();
                if name == "components/provider.gnplugin" {
                    data.extend_from_slice(b"tampered");
                }
                writer.start_file(name, SimpleFileOptions::default()).unwrap();
                writer.write_all(&data).unwrap();
            }
            writer.finish().unwrap().into_inner()
        };
        assert_eq!(
            verify_bundle_bytes(&tampered),
            Err(BundleError::InvalidComponent)
        );

        let missing = {
            let mut source = ZipArchive::new(Cursor::new(valid)).unwrap();
            let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
            for index in 0..source.len() {
                let mut entry = source.by_index(index).unwrap();
                let name = entry.name().to_owned();
                if name == "components/launch.gnplugin" {
                    continue;
                }
                let mut data = Vec::new();
                entry.read_to_end(&mut data).unwrap();
                writer.start_file(name, SimpleFileOptions::default()).unwrap();
                writer.write_all(&data).unwrap();
            }
            writer.finish().unwrap().into_inner()
        };
        assert_eq!(
            verify_bundle_bytes(&missing),
            Err(BundleError::InvalidComponent)
        );
    }

    #[test]
    fn mixed_publishers_duplicate_ids_and_identity_mismatches_are_rejected() {
        let key = Ed25519KeyPair::from_seed_unchecked(&[23; 32]).unwrap();
        let other = Ed25519KeyPair::from_seed_unchecked(&[24; 32]).unwrap();
        let foreign = vec![(
            "foreign.gnplugin",
            "org.example.foreign",
            "provider",
            component_archive("org.example.foreign", "provider", "0.1.0", &other, None),
        )];
        assert_eq!(
            verify_bundle_bytes(&bundle_archive(&key, &foreign, false)),
            Err(BundleError::InvalidComponent)
        );

        let mut duplicate = sample_components(&key);
        duplicate[1].1 = "org.example.provider";
        assert_eq!(
            verify_bundle_bytes(&bundle_archive(&key, &duplicate, false)),
            Err(BundleError::DuplicateComponent)
        );

        let mut mismatch = sample_components(&key);
        mismatch[0].1 = "org.example.other";
        assert_eq!(
            verify_bundle_bytes(&bundle_archive(&key, &mismatch, false)),
            Err(BundleError::ComponentMismatch)
        );
    }

    #[test]
    fn unexpected_entries_and_nested_bundles_are_rejected() {
        let key = Ed25519KeyPair::from_seed_unchecked(&[25; 32]).unwrap();
        let valid = bundle_archive(&key, &sample_components(&key), false);
        for extra in ["extra.txt", "components/nested.gnbundle"] {
            let mut source = ZipArchive::new(Cursor::new(valid.clone())).unwrap();
            let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
            for index in 0..source.len() {
                let mut entry = source.by_index(index).unwrap();
                let name = entry.name().to_owned();
                let mut data = Vec::new();
                entry.read_to_end(&mut data).unwrap();
                writer.start_file(name, SimpleFileOptions::default()).unwrap();
                writer.write_all(&data).unwrap();
            }
            writer.start_file(extra, SimpleFileOptions::default()).unwrap();
            writer.write_all(b"not allowed").unwrap();
            let bytes = writer.finish().unwrap().into_inner();
            assert_eq!(
                verify_bundle_bytes(&bytes),
                Err(BundleError::InvalidArchive),
                "{extra}"
            );
        }
    }

    #[test]
    fn a_component_declaring_another_publisher_is_rejected() {
        let key = Ed25519KeyPair::from_seed_unchecked(&[26; 32]).unwrap();
        let other = Ed25519KeyPair::from_seed_unchecked(&[27; 32]).unwrap();
        let component = component_archive(
            "org.example.provider",
            "provider",
            "0.1.0",
            &key,
            Some(other.public_key().as_ref()),
        );
        let components = vec![(
            "provider.gnplugin",
            "org.example.provider",
            "provider",
            component,
        )];
        assert_eq!(
            verify_bundle_bytes(&bundle_archive(&key, &components, false)),
            Err(BundleError::ComponentMismatch)
        );
    }

    #[test]
    fn case_colliding_and_excess_entries_are_rejected() {
        let key = Ed25519KeyPair::from_seed_unchecked(&[28; 32]).unwrap();
        let components = sample_components(&key);

        // Two component archives whose names differ only by case collide.
        let colliding = vec![
            (
                "provider.gnplugin",
                "org.example.provider",
                "provider",
                component_archive("org.example.provider", "provider", "0.1.0", &key, None),
            ),
            (
                "Provider.gnplugin",
                "org.example.provider",
                "provider",
                component_archive("org.example.provider", "provider", "0.1.0", &key, None),
            ),
        ];
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        {
            let bytes = bundle_archive(&key, &colliding, false);
            let mut source = ZipArchive::new(Cursor::new(&bytes)).unwrap();
            for index in 0..source.len() {
                let mut entry = source.by_index(index).unwrap();
                let name = entry.name().to_owned();
                let mut data = Vec::new();
                entry.read_to_end(&mut data).unwrap();
                writer.start_file(name, SimpleFileOptions::default()).unwrap();
                writer.write_all(&data).unwrap();
            }
        }
        writer.start_file("components/Extra.gnplugin", SimpleFileOptions::default()).unwrap();
        writer.write_all(b"extra case-colliding entry").unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        assert_eq!(
            verify_bundle_bytes(&bytes),
            Err(BundleError::InvalidArchive)
        );

        // More entries than components plus metadata exceeds the archive limit before any read.
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        {
            let bytes = bundle_archive(&key, &components, true);
            let mut source = ZipArchive::new(Cursor::new(&bytes)).unwrap();
            for index in 0..source.len() {
                let mut entry = source.by_index(index).unwrap();
                let name = entry.name().to_owned();
                let mut data = Vec::new();
                entry.read_to_end(&mut data).unwrap();
                writer.start_file(name, SimpleFileOptions::default()).unwrap();
                writer.write_all(&data).unwrap();
            }
        }
        for index in 0..=(MAX_BUNDLE_COMPONENTS + 1) {
            let name = format!("components/pad-{index}.gnplugin");
            writer.start_file(name, SimpleFileOptions::default()).unwrap();
            writer.write_all(b"padding entry").unwrap();
        }
        let bytes = writer.finish().unwrap().into_inner();
        assert_eq!(
            verify_bundle_bytes(&bytes),
            Err(BundleError::ArchiveLimit)
        );
    }

    #[test]
    fn bounded_reads_reject_entries_over_their_limit() {
        let data = vec![0u8; 128];
        assert_eq!(
            read_bounded(Cursor::new(&data), 64),
            Err(BundleError::ArchiveLimit)
        );
        assert_eq!(
            read_bounded(Cursor::new(&data), 128).unwrap(),
            data.as_slice()
        );
    }
}
