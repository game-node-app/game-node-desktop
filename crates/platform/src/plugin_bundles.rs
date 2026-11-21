//! Signed bundle manifests for multi-package plugin distribution.
//!
//! A bundle is a ZIP envelope: `bundle.json`, `components/*.gnplugin` and `signature.ed25519`.
//! This module owns the manifest contract and its signature domain. The archive extractor and
//! the per-component package verification live with the package/registry code; a bundle only
//! groups packages that are validated exactly as if imported alone.

use base64::Engine as _;
use ring::signature::{ED25519, UnparsedPublicKey};
use serde::Deserialize;

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
}
