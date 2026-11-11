//! Manifest schema for managed third-party plugin packages.
//!
//! This is deliberately separate from the bundled provider descriptors: parsing a
//! manifest does not register, trust, install, or execute a plugin.

use std::collections::BTreeMap;

use semver::Version;
use serde::{
    Deserialize,
    de::{self, MapAccess, SeqAccess, Visitor},
};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub id: String,
    pub kind: PluginKind,
    #[serde(default)]
    pub provider_id: Option<String>,
    pub name: String,
    pub version: String,
    pub api: ApiRange,
    #[serde(default)]
    pub permissions: Vec<PermissionRequest>,
    pub targets: Vec<Target>,
    pub publisher: Publisher,
    pub license: LicenseNotice,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub update_url: Option<String>,
    #[serde(default)]
    pub configuration: ConfigurationModes,
    /// Package-relative paths for validated provider/login/action descriptors.
    #[serde(default)]
    pub resources: BTreeMap<String, String>,
    #[serde(default)]
    pub schemas: Vec<PackagedSchema>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PluginKind {
    Provider,
    Extension,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ApiRange {
    pub min: String,
    pub max: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PermissionRequest {
    pub id: String,
    pub required: bool,
    pub scope: String,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub os: String,
    pub arch: String,
    pub abi: String,
    pub entrypoint: String,
    #[serde(default)]
    pub requires: Option<TargetRequirements>,
    #[serde(default)]
    pub tested_on: Vec<TestedEnvironment>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TargetRequirements {
    #[serde(default)]
    pub windows: Option<WindowsRequirements>,
    #[serde(default)]
    pub linux: Option<LinuxRequirements>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WindowsProduct {
    Client,
    Server,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WindowsRequirements {
    #[serde(default)]
    pub product: Option<WindowsProduct>,
    #[serde(default)]
    pub min_version: Option<String>,
    #[serde(default)]
    pub max_version: Option<String>,
    #[serde(default)]
    pub min_build: Option<u32>,
    #[serde(default)]
    pub max_build: Option<u32>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LibcFamily {
    Glibc,
    Musl,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LibcRequirements {
    pub family: LibcFamily,
    pub min_version: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LinuxRequirements {
    #[serde(default)]
    pub libc: Option<LibcRequirements>,
    /// Alternatives: any one listed distribution/version pair is sufficient.
    #[serde(default)]
    pub distributions: Option<Vec<DistributionRequirement>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DistributionRequirement {
    pub id: String,
    /// Exact distribution version strings. Entries within this list are alternatives.
    pub versions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TestedEnvironment {
    #[serde(default)]
    pub product: Option<String>,
    #[serde(default)]
    pub release: Option<String>,
    #[serde(default)]
    pub distribution: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PackagedSchema {
    /// Owner-controlled reverse-domain namespace, equal to the plugin id or its child.
    pub namespace: String,
    /// Version of this schema contract, independent of the plugin package version.
    pub version: String,
    /// Maximum classification declared for any field in the schema asset.
    pub classification: DataClassification,
    /// Package-relative, integrity-covered JSON Schema subset asset.
    pub asset: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum DataClassification {
    Public,
    Sensitive,
    Private,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Publisher {
    pub key_id: String,
    pub public_key: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LicenseNotice {
    /// SPDX expression, or `LicenseRef-*` when the bundled text defines it.
    pub expression: String,
    pub files: Vec<String>,
    #[serde(default)]
    pub notices: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationModes {
    #[serde(default)]
    pub external: bool,
    #[serde(default)]
    pub declarative: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedManifest(Manifest);

impl ValidatedManifest {
    pub fn manifest(&self) -> &Manifest {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestError(pub String);

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ManifestError {}

impl Manifest {
    pub fn parse_json(bytes: &[u8]) -> Result<ValidatedManifest, ManifestError> {
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(ManifestError(
                "plugin manifest exceeds the 4 MiB metadata limit".into(),
            ));
        }
        let mut deserializer = serde_json::Deserializer::from_slice(bytes);
        NoDuplicateJson::deserialize(&mut deserializer)
            .map_err(|e| ManifestError(format!("invalid plugin manifest JSON: {e}")))?;
        deserializer
            .end()
            .map_err(|e| ManifestError(format!("invalid plugin manifest JSON: {e}")))?;
        let manifest: Self = serde_json::from_slice(bytes)
            .map_err(|e| ManifestError(format!("invalid plugin manifest: {e}")))?;
        manifest.validate()?;
        Ok(ValidatedManifest(manifest))
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        let invalid = |message: &str| Err(ManifestError(message.into()));
        if self.schema != 1 {
            return invalid("unsupported manifest schema (expected 1)");
        }
        if !reverse_domain_id(&self.id) {
            return invalid("plugin id must be a reverse-domain identifier");
        }
        match self.kind {
            PluginKind::Provider
                if self
                    .provider_id
                    .as_deref()
                    .is_none_or(|id| !provider_id(id)) =>
            {
                return invalid("provider packages require a valid provider_id");
            }
            PluginKind::Extension if self.provider_id.is_some() => {
                return invalid("extension packages must not declare provider_id");
            }
            _ => {}
        }
        if self.name.trim().is_empty() || self.name.len() > 128 {
            return invalid("name must contain 1 to 128 characters");
        }
        let version = Version::parse(&self.version)
            .map_err(|_| ManifestError("version must be valid SemVer".into()))?;
        let min = Version::parse(&self.api.min)
            .map_err(|_| ManifestError("api.min must be valid SemVer".into()))?;
        let max = Version::parse(&self.api.max)
            .map_err(|_| ManifestError("api.max must be valid SemVer".into()))?;
        if min > max {
            return invalid("api.min must not exceed api.max");
        }
        let _ = version;
        if self.targets.is_empty() {
            return invalid("at least one target is required");
        }
        let mut seen_targets = std::collections::HashSet::new();
        for target in &self.targets {
            let valid_abi = match target.os.as_str() {
                "linux" => matches!(target.abi.as_str(), "gnu" | "musl"),
                "windows" => matches!(target.abi.as_str(), "msvc" | "gnu"),
                _ => false,
            };
            if !matches!(target.os.as_str(), "linux" | "windows")
                || !matches!(target.arch.as_str(), "x86_64" | "aarch64" | "x86")
                || !valid_abi
            {
                return invalid("target must specify a supported OS, architecture, and ABI");
            }
            if !package_path(&target.entrypoint) {
                return invalid("target entrypoint must be a safe package-relative path");
            }
            if let Some(requirements) = &target.requires {
                validate_target_requirements(target, requirements)?;
            }
            for tested in &target.tested_on {
                validate_tested_environment(target, tested)?;
            }
            if !seen_targets.insert((&target.os, &target.arch, &target.abi)) {
                return invalid("ambiguous target selection: duplicate OS/architecture/ABI tuple");
            }
        }
        if self.schemas.len() > 32 {
            return invalid("at most 32 packaged schemas are allowed per plugin");
        }
        let mut seen_schemas = std::collections::HashSet::new();
        let mut seen_assets = std::collections::HashSet::new();
        for schema in &self.schemas {
            let namespace_owned = schema.namespace == self.id
                || schema.namespace.starts_with(&format!("{}.", self.id));
            if !reverse_domain_id(&schema.namespace)
                || schema.namespace.starts_with("org.gamenode.")
                || !namespace_owned
            {
                return invalid(
                    "schema namespace must be a reverse-domain namespace owned by this plugin",
                );
            }
            Version::parse(&schema.version)
                .map_err(|_| ManifestError("schema version must be valid SemVer".into()))?;
            if !package_path(&schema.asset)
                || !schema.asset.starts_with("schemas/")
                || !seen_schemas.insert((&schema.namespace, &schema.version))
                || !seen_assets.insert(&schema.asset)
            {
                return invalid(
                    "schema namespace/version and asset paths must be unique safe schema paths",
                );
            }
        }
        let mut permissions = std::collections::HashSet::new();
        for permission in &self.permissions {
            if !identifier(&permission.id)
                || permission.scope.trim().is_empty()
                || permission.reason.trim().is_empty()
                || permission.reason.len() > 512
            {
                return invalid(
                    "permission requests require a valid id, scope, and bounded reason",
                );
            }
            if !permissions.insert(&permission.id) {
                return invalid("duplicate permission request");
            }
        }
        if self.publisher.key_id.trim().is_empty()
            || self.publisher.key_id.len() > 128
            || decode_key(&self.publisher.public_key).is_none()
        {
            return invalid("publisher public_key must be base64-encoded 32-byte Ed25519 key data");
        }
        if !license_expression(&self.license.expression) || self.license.files.is_empty() {
            return invalid(
                "license requires a valid SPDX expression or LicenseRef and at least one bundled text",
            );
        }
        for path in self
            .license
            .files
            .iter()
            .chain(self.license.notices.iter())
            .chain(self.resources.values())
        {
            if !package_path(path) {
                return invalid(
                    "license, notice, and resource paths must be safe package-relative paths",
                );
            }
        }
        if let Some(source) = &self.source_url {
            let parsed = url::Url::parse(source)
                .map_err(|_| ManifestError("source URL is invalid".into()))?;
            if !matches!(parsed.scheme(), "https" | "http") || parsed.host_str().is_none() {
                return invalid("source URL must be an absolute HTTP(S) URL");
            }
        }
        if let Some(update) = &self.update_url {
            let parsed = url::Url::parse(update)
                .map_err(|_| ManifestError("update URL is invalid".into()))?;
            if parsed.scheme() != "https" || parsed.host_str().is_none() {
                return invalid("update URL must use HTTPS");
            }
        }
        Ok(())
    }

    /// Validate packaged schema bytes after archive staging. The caller supplies only
    /// integrity-verified package entries; this function never retrieves remote assets.
    pub fn validate_schema_assets(
        &self,
        assets: &BTreeMap<String, Vec<u8>>,
    ) -> Result<(), ManifestError> {
        self.validate()?;
        for descriptor in &self.schemas {
            let bytes = assets.get(&descriptor.asset).ok_or_else(|| {
                ManifestError(format!("missing schema asset `{}`", descriptor.asset))
            })?;
            if bytes.len() > 64 * 1024 {
                return Err(ManifestError(format!(
                    "schema asset `{}` exceeds the 64 KiB limit",
                    descriptor.asset
                )));
            }
            let mut duplicate_check = serde_json::Deserializer::from_slice(bytes);
            NoDuplicateJson::deserialize(&mut duplicate_check).map_err(|e| {
                ManifestError(format!("invalid schema asset `{}`: {e}", descriptor.asset))
            })?;
            duplicate_check.end().map_err(|e| {
                ManifestError(format!("invalid schema asset `{}`: {e}", descriptor.asset))
            })?;
            let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| {
                ManifestError(format!("invalid schema asset `{}`: {e}", descriptor.asset))
            })?;
            let mut budget = SchemaBudget { keys: 0 };
            let maximum = validate_schema_node(&value, 0, false, &mut budget)?;
            if maximum.is_some_and(|field_class| field_class > descriptor.classification) {
                return Err(ManifestError(format!(
                    "schema `{}` understates its field classification",
                    descriptor.namespace
                )));
            }
        }
        Ok(())
    }

    /// Reject namespace claims that overlap another installed package's ownership.
    /// `existing_owners` is the host registry's namespace-to-plugin-id index.
    pub fn validate_namespace_ownership(
        &self,
        existing_owners: &BTreeMap<String, String>,
    ) -> Result<(), ManifestError> {
        self.validate()?;
        for schema in &self.schemas {
            for (claimed_namespace, owner) in existing_owners {
                let overlaps = schema.namespace == *claimed_namespace
                    || schema
                        .namespace
                        .starts_with(&format!("{claimed_namespace}."))
                    || claimed_namespace.starts_with(&format!("{}.", schema.namespace));
                if overlaps && owner != &self.id {
                    return Err(ManifestError(format!(
                        "schema namespace `{}` conflicts with namespace owned by `{owner}`",
                        schema.namespace
                    )));
                }
            }
        }
        Ok(())
    }
}

fn reverse_domain_id(s: &str) -> bool {
    s.len() <= 255
        && s.split('.').count() >= 2
        && s.split('.').all(|part| {
            !part.is_empty()
                && part.as_bytes()[0].is_ascii_alphanumeric()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}
fn provider_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_')
}
fn package_path(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('/')
        && !s.starts_with('\\')
        && !s.contains(':')
        && s.split(['/', '\\'])
            .all(|p| !p.is_empty() && p != "." && p != "..")
}

fn validate_target_requirements(
    target: &Target,
    requirements: &TargetRequirements,
) -> Result<(), ManifestError> {
    let invalid = |message: &str| Err(ManifestError(message.into()));
    match target.os.as_str() {
        "windows" => {
            if requirements.linux.is_some() || requirements.windows.is_none() {
                return invalid("Windows targets must declare only Windows requirements");
            }
            let req = requirements.windows.as_ref().expect("checked above");
            if req.product.is_none()
                && req.min_version.is_none()
                && req.max_version.is_none()
                && req.min_build.is_none()
                && req.max_build.is_none()
            {
                return invalid("Windows requirements must specify at least one selector");
            }
            let min_version = req
                .min_version
                .as_deref()
                .map(parse_numeric_version)
                .transpose()?;
            let max_version = req
                .max_version
                .as_deref()
                .map(parse_numeric_version)
                .transpose()?;
            if let (Some(min), Some(max)) = (&min_version, &max_version) {
                if compare_numeric_versions(min, max).is_gt() {
                    return invalid("Windows min_version must not exceed max_version");
                }
            }
            if req
                .min_build
                .zip(req.max_build)
                .is_some_and(|(min, max)| min > max)
            {
                return invalid("Windows min_build must not exceed max_build");
            }
        }
        "linux" => {
            if requirements.windows.is_some() || requirements.linux.is_none() {
                return invalid("Linux targets must declare only Linux requirements");
            }
            let req = requirements.linux.as_ref().expect("checked above");
            if req.libc.is_none() && req.distributions.is_none() {
                return invalid("Linux requirements must specify libc or distributions");
            }
            if let Some(libc) = &req.libc {
                parse_numeric_version(&libc.min_version)?;
                let abi_matches = matches!(
                    (libc.family, target.abi.as_str()),
                    (LibcFamily::Glibc, "gnu") | (LibcFamily::Musl, "musl")
                );
                if !abi_matches {
                    return invalid("Linux libc family contradicts target ABI");
                }
            }
            if let Some(distributions) = &req.distributions {
                if distributions.is_empty() {
                    return invalid("Linux distribution alternatives must not be empty");
                }
                let mut seen_ids = std::collections::HashSet::new();
                for distribution in distributions {
                    if !distribution_id(&distribution.id) || !seen_ids.insert(&distribution.id) {
                        return invalid("Linux distribution alternatives require unique valid IDs");
                    }
                    if distribution.versions.is_empty()
                        || distribution
                            .versions
                            .iter()
                            .any(|v| v.trim().is_empty() || v.len() > 64)
                    {
                        return invalid(
                            "each Linux distribution alternative requires bounded version strings",
                        );
                    }
                    let mut versions = std::collections::HashSet::new();
                    if distribution.versions.iter().any(|v| !versions.insert(v)) {
                        return invalid("duplicate version in Linux distribution alternatives");
                    }
                }
            }
        }
        _ => return invalid("unsupported target OS requirements"),
    }
    Ok(())
}

fn validate_tested_environment(
    target: &Target,
    tested: &TestedEnvironment,
) -> Result<(), ManifestError> {
    let valid = match target.os.as_str() {
        "windows" => {
            tested
                .product
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty() && s.len() <= 128)
                && tested
                    .release
                    .as_deref()
                    .is_some_and(|s| !s.trim().is_empty() && s.len() <= 128)
                && tested.distribution.is_none()
                && tested.version.is_none()
        }
        "linux" => {
            tested.distribution.as_deref().is_some_and(distribution_id)
                && tested
                    .version
                    .as_deref()
                    .is_some_and(|s| !s.trim().is_empty() && s.len() <= 64)
                && tested.product.is_none()
                && tested.release.is_none()
        }
        _ => false,
    };
    if !valid {
        return Err(ManifestError("tested_on entries must identify a matching Windows product/release or Linux distribution/version".into()));
    }
    Ok(())
}

fn distribution_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        })
}

fn parse_numeric_version(value: &str) -> Result<Vec<u32>, ManifestError> {
    if value.is_empty() || value.len() > 64 {
        return Err(ManifestError(
            "numeric version bound is empty or too long".into(),
        ));
    }
    let parts: Vec<_> = value.split('.').collect();
    if !(1..=4).contains(&parts.len()) {
        return Err(ManifestError(
            "numeric version bounds require one to four components".into(),
        ));
    }
    parts
        .iter()
        .map(|part| {
            if part.is_empty()
                || !part.bytes().all(|b| b.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
            {
                return Err(ManifestError(
                    "numeric version bounds must contain canonical decimal components".into(),
                ));
            }
            part.parse::<u32>()
                .map_err(|_| ManifestError("numeric version component exceeds range".into()))
        })
        .collect()
}

fn compare_numeric_versions(left: &[u32], right: &[u32]) -> std::cmp::Ordering {
    let width = left.len().max(right.len());
    (0..width)
        .map(|i| {
            left.get(i)
                .copied()
                .unwrap_or(0)
                .cmp(&right.get(i).copied().unwrap_or(0))
        })
        .find(|order| !order.is_eq())
        .unwrap_or(std::cmp::Ordering::Equal)
}

struct SchemaBudget {
    keys: usize,
}

const JSON_SCHEMA_DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";
const MAX_SCHEMA_DEPTH: usize = 8;
const MAX_SCHEMA_KEYS: usize = 256;

fn validate_schema_node(
    value: &serde_json::Value,
    depth: usize,
    require_classification: bool,
    budget: &mut SchemaBudget,
) -> Result<Option<DataClassification>, ManifestError> {
    let invalid = |message: &str| Err(ManifestError(message.into()));
    if depth > MAX_SCHEMA_DEPTH {
        return invalid("declarative schema exceeds the depth limit of 8");
    }
    let object = value
        .as_object()
        .ok_or_else(|| ManifestError("each declarative schema node must be an object".into()))?;
    budget.keys = budget.keys.saturating_add(object.len());
    if budget.keys > MAX_SCHEMA_KEYS {
        return invalid("declarative schema exceeds the 256-key limit");
    }

    const ALLOWED: &[&str] = &[
        "$schema",
        "type",
        "title",
        "description",
        "properties",
        "required",
        "additionalProperties",
        "items",
        "enum",
        "minimum",
        "maximum",
        "minLength",
        "maxLength",
        "minItems",
        "maxItems",
        "x-gamenode-classification",
    ];
    for key in object.keys() {
        if key == "$ref" || key == "$dynamicRef" || key == "$recursiveRef" {
            return invalid("schema references are unsupported; assets must be self-contained");
        }
        if !ALLOWED.contains(&key.as_str()) {
            return Err(ManifestError(format!(
                "unsupported declarative schema keyword `{key}`"
            )));
        }
    }

    if let Some(dialect) = object.get("$schema") {
        if dialect.as_str() != Some(JSON_SCHEMA_DIALECT) {
            return invalid(
                "schema $schema must identify the supported JSON Schema 2020-12 subset",
            );
        }
    }
    for text_key in ["title", "description"] {
        if let Some(text) = object.get(text_key) {
            let max = if text_key == "title" { 128 } else { 1024 };
            if text.as_str().is_none_or(|s| s.len() > max) {
                return invalid("schema title/description must be bounded strings");
            }
        }
    }

    let type_name = object
        .get("type")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ManifestError("declarative schema nodes require one string type".into()))?;
    if !matches!(
        type_name,
        "object" | "array" | "string" | "number" | "integer" | "boolean" | "null"
    ) {
        return invalid("declarative schema has an unsupported type");
    }

    let own_class = match object.get("x-gamenode-classification") {
        Some(value) => Some(match value.as_str() {
            Some("public") => DataClassification::Public,
            Some("sensitive") => DataClassification::Sensitive,
            Some("private") => DataClassification::Private,
            _ => return invalid("field classification must be public, sensitive, or private"),
        }),
        None if require_classification => {
            return invalid("every declared field requires x-gamenode-classification");
        }
        None => None,
    };

    let mut maximum_class = own_class;
    if type_name == "object" {
        if object.get("additionalProperties") != Some(&serde_json::Value::Bool(false)) {
            return invalid("object schemas must set additionalProperties to false");
        }
        let properties = object
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| ManifestError("object schemas require a properties object".into()))?;
        if properties.len() > MAX_SCHEMA_KEYS {
            return invalid("object schema has too many properties");
        }
        for (key, child) in properties {
            if !schema_field_name(key) {
                return Err(ManifestError(format!(
                    "schema field name `{key}` is invalid or reserved"
                )));
            }
            if let Some(child_class) = validate_schema_node(child, depth + 1, true, budget)? {
                maximum_class =
                    Some(maximum_class.map_or(child_class, |current| current.max(child_class)));
            }
        }
        let required = object
            .get("required")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| {
                ManifestError("object schemas require a required array (which may be empty)".into())
            })?;
        let mut required_names = std::collections::HashSet::new();
        for entry in required {
            let name = entry
                .as_str()
                .ok_or_else(|| ManifestError("required entries must be strings".into()))?;
            if !properties.contains_key(name) || !required_names.insert(name) {
                return invalid("required entries must name unique declared properties");
            }
        }
    } else if object.contains_key("properties")
        || object.contains_key("required")
        || object.contains_key("additionalProperties")
    {
        return invalid("object-only keywords require type object");
    }

    if type_name == "array" {
        let items = object
            .get("items")
            .ok_or_else(|| ManifestError("array schemas require items".into()))?;
        let max_items = object
            .get("maxItems")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| ManifestError("array schemas require bounded maxItems".into()))?;
        if max_items > 256 {
            return invalid("array maxItems exceeds 256");
        }
        if object
            .get("minItems")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|min| min > max_items)
        {
            return invalid("array minItems must not exceed maxItems");
        }
        if let Some(item_class) = validate_schema_node(items, depth + 1, false, budget)? {
            maximum_class =
                Some(maximum_class.map_or(item_class, |current| current.max(item_class)));
        }
    } else if object.contains_key("items")
        || object.contains_key("minItems")
        || object.contains_key("maxItems")
    {
        return invalid("array-only keywords require type array");
    }

    if type_name == "string" {
        let max_length = object
            .get("maxLength")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| ManifestError("string schemas require bounded maxLength".into()))?;
        if max_length > 4096 {
            return invalid("string maxLength exceeds 4096");
        }
        if object
            .get("minLength")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|min| min > max_length)
        {
            return invalid("string minLength must not exceed maxLength");
        }
    } else if object.contains_key("minLength") || object.contains_key("maxLength") {
        return invalid("string-only keywords require type string");
    }

    if let Some(values) = object.get("enum") {
        let values = values
            .as_array()
            .ok_or_else(|| ManifestError("enum must be an array".into()))?;
        if values.is_empty()
            || values.len() > 128
            || values.iter().any(|v| v.is_array() || v.is_object())
        {
            return invalid("enum must contain 1 to 128 scalar values");
        }
    }
    let min = object.get("minimum").and_then(serde_json::Value::as_f64);
    let max = object.get("maximum").and_then(serde_json::Value::as_f64);
    if object.contains_key("minimum") && min.is_none()
        || object.contains_key("maximum") && max.is_none()
    {
        return invalid("numeric schema bounds must be finite numbers");
    }
    if min.zip(max).is_some_and(|(min, max)| min > max) {
        return invalid("numeric minimum must not exceed maximum");
    }
    if (min.is_some() || max.is_some()) && !matches!(type_name, "number" | "integer") {
        return invalid("numeric bounds require number or integer type");
    }

    Ok(maximum_class)
}

fn schema_field_name(name: &str) -> bool {
    const RESERVED: &[&str] = &[
        "id",
        "plugin_id",
        "owner",
        "creator",
        "creator_plugin_id",
        "owner_plugin_id",
        "provider_id",
        "publisher",
        "origin",
        "provenance",
        "resource_id",
        "revision",
        "permissions",
        "permission_revision",
        "scope",
        "schema_id",
        "schema_version",
        "schema_digest",
        "command",
        "method",
        "operation",
        "dispatch",
        "action",
        "launch",
        "argv",
        "environment",
        "executable",
        "entrypoint",
        "credential",
        "secret",
        "token",
    ];
    identifier(name) && !RESERVED.contains(&name.to_ascii_lowercase().as_str())
}

fn decode_key(s: &str) -> Option<Vec<u8>> {
    // 32 bytes have a canonical 44-character encoding: 43 data characters and
    // one final `=` whose two unused bits must be zero.
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    if s.len() != 44 || !s.ends_with('=') || s.as_bytes()[42] == b'=' {
        return None;
    }
    let mut out = Vec::with_capacity(33);
    let bytes = s.as_bytes();
    for chunk in bytes.chunks_exact(4) {
        let a = alphabet.iter().position(|b| *b == chunk[0])? as u32;
        let b = alphabet.iter().position(|b| *b == chunk[1])? as u32;
        let c = if chunk[2] == b'=' {
            0
        } else {
            alphabet.iter().position(|v| *v == chunk[2])? as u32
        };
        let d = if chunk[3] == b'=' {
            0
        } else {
            alphabet.iter().position(|v| *v == chunk[3])? as u32
        };
        if chunk[3] == b'=' && (c & 0b11) != 0 {
            return None;
        }
        if chunk[2] == b'=' && (b & 0b1111) != 0 {
            return None;
        }
        let n = (a << 18) | (b << 12) | (c << 6) | d;
        out.push((n >> 16) as u8);
        if chunk[2] != b'=' {
            out.push((n >> 8) as u8);
        }
        if chunk[3] != b'=' {
            out.push(n as u8);
        }
    }
    (out.len() == 32).then_some(out)
}
fn license_expression(s: &str) -> bool {
    !s.is_empty() && s.len() <= 256 && spdx::Expression::parse(s).is_ok()
}

/// Walk the JSON value before typed deserialization so duplicate object keys cannot
/// be silently overwritten by maps such as `resources`.
struct NoDuplicateJson;

impl<'de> Deserialize<'de> for NoDuplicateJson {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct JsonVisitor;
        impl<'de> Visitor<'de> for JsonVisitor {
            type Value = NoDuplicateJson;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON value without duplicate object keys")
            }
            fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
                Ok(NoDuplicateJson)
            }
            fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
                Ok(NoDuplicateJson)
            }
            fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
                Ok(NoDuplicateJson)
            }
            fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
                Ok(NoDuplicateJson)
            }
            fn visit_str<E>(self, _: &str) -> Result<Self::Value, E> {
                Ok(NoDuplicateJson)
            }
            fn visit_string<E>(self, _: String) -> Result<Self::Value, E> {
                Ok(NoDuplicateJson)
            }
            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(NoDuplicateJson)
            }
            fn visit_none<E>(self) -> Result<Self::Value, E> {
                Ok(NoDuplicateJson)
            }
            fn visit_some<D>(self, d: D) -> Result<Self::Value, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                NoDuplicateJson::deserialize(d)
            }
            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                while seq.next_element::<NoDuplicateJson>()?.is_some() {}
                Ok(NoDuplicateJson)
            }
            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut keys = std::collections::HashSet::new();
                while let Some(key) = map.next_key::<String>()? {
                    if !keys.insert(key.clone()) {
                        return Err(de::Error::custom(format!(
                            "duplicate JSON object key `{key}`"
                        )));
                    }
                    map.next_value::<NoDuplicateJson>()?;
                }
                Ok(NoDuplicateJson)
            }
        }
        deserializer.deserialize_any(JsonVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"{"schema":1,"id":"org.example.reader","kind":"provider","provider_id":"example-reader","name":"Example Reader","version":"1.2.3","api":{"min":"1.4.0","max":"1.5.0"},"permissions":[{"id":"library.read","required":false,"scope":"own-provider","reason":"Read imported games"}],"targets":[{"os":"linux","arch":"x86_64","abi":"gnu","entrypoint":"bin/linux-x64/plugin"}],"publisher":{"key_id":"dev-key","public_key":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="},"license":{"expression":"MIT","files":["licenses/LICENSE"]},"configuration":{"external":true,"declarative":false},"resources":{"provider":"provider.toml"}}"#;
    const VALID_SCHEMA: &str = r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","properties":{"supports_cloud_sync":{"type":"boolean","x-gamenode-classification":"public"},"notes":{"type":"string","maxLength":512,"x-gamenode-classification":"sensitive"}},"required":["supports_cloud_sync"],"additionalProperties":false}"#;

    fn manifest_with_schema(
        namespace: &str,
        version: &str,
        classification: &str,
        asset: &str,
    ) -> String {
        let descriptor = format!(
            "\"schemas\":[{{\"namespace\":\"{namespace}\",\"version\":\"{version}\",\"classification\":\"{classification}\",\"asset\":\"{asset}\"}}]"
        );
        VALID.replace(
            "\"resources\":{\"provider\":\"provider.toml\"}",
            &format!("\"resources\":{{\"provider\":\"provider.toml\"}},{descriptor}"),
        )
    }

    #[test]
    fn accepts_provider_and_extension_manifest_fixtures() {
        assert!(Manifest::parse_json(VALID.as_bytes()).is_ok());
        assert!(license_expression("MIT OR (Apache-2.0 AND BSD-3-Clause)"));
        assert!(license_expression("LicenseRef-Internal"));
        let extension = VALID.replace(
            "\"kind\":\"provider\",\"provider_id\":\"example-reader\",",
            "\"kind\":\"extension\",",
        );
        assert!(Manifest::parse_json(extension.as_bytes()).is_ok());
    }

    #[test]
    fn accepts_platform_requirements_and_tested_environment_alternatives() {
        let linux = VALID.replace(
            "\"os\":\"linux\",\"arch\":\"x86_64\",\"abi\":\"gnu\",\"entrypoint\":\"bin/linux-x64/plugin\"",
            "\"os\":\"linux\",\"arch\":\"x86_64\",\"abi\":\"gnu\",\"entrypoint\":\"bin/linux-x64/plugin\",\"requires\":{\"linux\":{\"libc\":{\"family\":\"glibc\",\"min_version\":\"2.36\"},\"distributions\":[{\"id\":\"debian\",\"versions\":[\"13\",\"13.1\"]},{\"id\":\"fedora\",\"versions\":[\"43\"]}]}},\"tested_on\":[{\"distribution\":\"debian\",\"version\":\"13\"}]",
        );
        assert!(Manifest::parse_json(linux.as_bytes()).is_ok());

        let windows = VALID.replace(
            "\"os\":\"linux\",\"arch\":\"x86_64\",\"abi\":\"gnu\",\"entrypoint\":\"bin/linux-x64/plugin\"",
            "\"os\":\"windows\",\"arch\":\"x86_64\",\"abi\":\"msvc\",\"entrypoint\":\"bin/windows-x64/plugin.exe\",\"requires\":{\"windows\":{\"product\":\"client\",\"min_version\":\"10.0\",\"max_version\":\"10.0.99999\",\"min_build\":22000,\"max_build\":30000}},\"tested_on\":[{\"product\":\"windows-11\",\"release\":\"23H2\"}]",
        );
        assert!(Manifest::parse_json(windows.as_bytes()).is_ok());
    }

    #[test]
    fn rejects_malformed_and_unsafe_manifest_fixtures() {
        for malformed in [
            VALID.replace("\"schema\":1", "\"schema\":2"),
            VALID.replace("\"provider_id\":\"example-reader\",", ""),
            VALID.replace("\"kind\":\"provider\"", "\"kind\":\"extension\""),
            VALID.replace("\"max\":\"1.5.0\"", "\"max\":\"1.3.0\""),
            VALID.replace(
                "\"entrypoint\":\"bin/linux-x64/plugin\"",
                "\"entrypoint\":\"bin/linux-x64/plugin\",\"requires\":{\"windows\":{\"min_build\":22000}}",
            ),
            VALID.replace(
                "\"entrypoint\":\"bin/linux-x64/plugin\"",
                "\"entrypoint\":\"bin/linux-x64/plugin\",\"requires\":{\"macos\":{\"min_version\":\"14.0\"}}",
            ),
            VALID.replace(
                "\"entrypoint\":\"bin/linux-x64/plugin\"",
                "\"entrypoint\":\"bin/linux-x64/plugin\",\"requires\":{\"linux\":{\"libc\":{\"family\":\"glibc\",\"min_version\":\"2.x\"}}}",
            ),
            VALID.replace(
                "\"entrypoint\":\"bin/linux-x64/plugin\"",
                "\"entrypoint\":\"bin/linux-x64/plugin\",\"requires\":{\"linux\":{\"libc\":{\"family\":\"glibc\",\"min_version\":\"2.36\"}}}",
            ).replace("\"abi\":\"gnu\"", "\"abi\":\"musl\""),
            VALID.replace(
                "\"entrypoint\":\"bin/linux-x64/plugin\"",
                "\"entrypoint\":\"bin/linux-x64/plugin\",\"requires\":{\"linux\":{\"distributions\":[]}}",
            ),
            VALID.replace(
                "\"entrypoint\":\"bin/linux-x64/plugin\"",
                "\"entrypoint\":\"bin/linux-x64/plugin\",\"requires\":{\"linux\":{\"distributions\":[{\"id\":\"debian\",\"versions\":[\"13\"]},{\"id\":\"debian\",\"versions\":[\"14\"]}]}}",
            ),
            VALID.replace(
                "\"entrypoint\":\"bin/linux-x64/plugin\"",
                "\"entrypoint\":\"bin/linux-x64/plugin\",\"tested_on\":[{\"product\":\"windows-11\",\"release\":\"23H2\"}]",
            ),
            VALID.replace(
                "\"targets\":[{\"os\":\"linux\",\"arch\":\"x86_64\",\"abi\":\"gnu\",\"entrypoint\":\"bin/linux-x64/plugin\"}]",
                "\"targets\":[{\"os\":\"linux\",\"arch\":\"x86_64\",\"abi\":\"gnu\",\"entrypoint\":\"bin/linux-x64/plugin\"},{\"os\":\"linux\",\"arch\":\"x86_64\",\"abi\":\"gnu\",\"entrypoint\":\"bin/linux-x64/alternate\"}]",
            ),
            VALID.replace("bin/linux-x64/plugin", "../escape"),
            VALID.replace("licenses/LICENSE", "../../LICENSE"),
            VALID
                .replace("\"MIT\"", "\"LicenseRef-Custom\"")
                .replace("licenses/LICENSE", ""),
            VALID.replace("\"reason\":\"Read imported games\"", "\"reason\":\"\""),
            VALID.replace("\"MIT\"", "\"MIT AND\""),
            VALID.replace("\"MIT\"", "\"MadeUp-License\""),
            VALID.replace("\"schema\":1,", "\"schema\":1,\"schema\":1,"),
            VALID.replace(
                "\"provider\":\"provider.toml\"",
                "\"provider\":\"first.toml\",\"provider\":\"second.toml\"",
            ),
            VALID.replace("AAA=", "AAB="),
            VALID.replace(
                "\"configuration\":{\"external\":true,\"declarative\":false},",
                "\"unexpected\":true,",
            ),
        ] {
            assert!(
                Manifest::parse_json(malformed.as_bytes()).is_err(),
                "accepted fixture: {malformed}"
            );
        }

        let windows_base = VALID.replace(
            "\"os\":\"linux\",\"arch\":\"x86_64\",\"abi\":\"gnu\",\"entrypoint\":\"bin/linux-x64/plugin\"",
            "\"os\":\"windows\",\"arch\":\"x86_64\",\"abi\":\"msvc\",\"entrypoint\":\"bin/windows-x64/plugin.exe\",\"requires\":{\"windows\":{\"product\":\"client\",\"min_version\":\"10.0\",\"max_version\":\"11.0\",\"min_build\":22000,\"max_build\":30000}}",
        );
        for malformed in [
            windows_base.replace("\"min_version\":\"10.0\"", "\"min_version\":\"10.x\""),
            windows_base
                .replace("\"min_version\":\"10.0\"", "\"min_version\":\"11.0\"")
                .replace("\"max_version\":\"11.0\"", "\"max_version\":\"10.0\""),
            windows_base.replace("\"min_build\":22000", "\"min_build\":31000"),
        ] {
            assert!(
                Manifest::parse_json(malformed.as_bytes()).is_err(),
                "accepted Windows range fixture: {malformed}"
            );
        }
    }

    #[test]
    fn validates_owned_versioned_schema_assets_and_classifications() {
        let manifest_text = manifest_with_schema(
            "org.example.reader.compatibility",
            "1.0.0",
            "sensitive",
            "schemas/compatibility.v1.json",
        );
        let manifest = Manifest::parse_json(manifest_text.as_bytes()).unwrap();
        let assets = BTreeMap::from([(
            "schemas/compatibility.v1.json".into(),
            VALID_SCHEMA.as_bytes().to_vec(),
        )]);
        manifest
            .manifest()
            .validate_schema_assets(&assets)
            .expect("valid packaged schema fixture");
        assert!(
            manifest
                .manifest()
                .validate_namespace_ownership(&BTreeMap::from([(
                    "org.example.reader.compatibility.v2".into(),
                    "org.example.other".into(),
                )]))
                .is_err()
        );
        assert!(
            manifest
                .manifest()
                .validate_namespace_ownership(&BTreeMap::from([(
                    "org.example.reader.launch".into(),
                    "org.example.other".into(),
                )]))
                .is_ok()
        );

        let underclassified = Manifest::parse_json(
            manifest_with_schema(
                "org.example.reader.compatibility",
                "1.0.0",
                "public",
                "schemas/compatibility.v1.json",
            )
            .as_bytes(),
        )
        .unwrap();
        assert!(
            underclassified
                .manifest()
                .validate_schema_assets(&assets)
                .is_err()
        );
        assert!(
            manifest
                .manifest()
                .validate_schema_assets(&BTreeMap::new())
                .is_err()
        );
    }

    #[test]
    fn rejects_schema_ownership_collisions_and_unsafe_assets() {
        let foreign = manifest_with_schema(
            "org.other.compatibility",
            "1.0.0",
            "sensitive",
            "schemas/compat.json",
        );
        assert!(Manifest::parse_json(foreign.as_bytes()).is_err());

        let invalid_version = manifest_with_schema(
            "org.example.reader.compatibility",
            "v1",
            "public",
            "schemas/compat.json",
        );
        assert!(Manifest::parse_json(invalid_version.as_bytes()).is_err());

        let descriptor = "{\"namespace\":\"org.example.reader.compatibility\",\"version\":\"1.0.0\",\"classification\":\"public\",\"asset\":\"schemas/compat.json\"}";
        let duplicate = VALID.replace(
            "\"resources\":{\"provider\":\"provider.toml\"}",
            &format!("\"resources\":{{\"provider\":\"provider.toml\"}},\"schemas\":[{descriptor},{descriptor}]"),
        );
        assert!(Manifest::parse_json(duplicate.as_bytes()).is_err());

        let declared = Manifest::parse_json(
            manifest_with_schema(
                "org.example.reader.compatibility",
                "1.0.0",
                "sensitive",
                "schemas/compat.json",
            )
            .as_bytes(),
        )
        .unwrap();
        for unsafe_asset in [
            r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","$ref":"https://evil.invalid/schema.json","type":"object","properties":{},"required":[],"additionalProperties":false}"#,
            r#"{"type":"object","properties":{"launch":{"type":"string","maxLength":20,"x-gamenode-classification":"public"}},"required":[],"additionalProperties":false}"#,
            r#"{"type":"object","properties":{"field":{"type":"string","maxLength":20,"x-gamenode-classification":"secret"}},"required":[],"additionalProperties":false}"#,
            r#"{"type":"object","properties":{"field":{"type":"string","maxLength":20}},"required":[],"additionalProperties":false}"#,
            r#"{"type":"object","properties":{"field":{"type":"string","maxLength":20,"x-gamenode-classification":"public","x-gamenode-dispatch":"launch"}},"required":[],"additionalProperties":false}"#,
            r#"{"type":"object","properties":{"field":{"type":"string","maxLength":5000,"x-gamenode-classification":"public"}},"required":[],"additionalProperties":false}"#,
            r#"{"type":"object","properties":{"field":{"type":"string","maxLength":20,"x-gamenode-classification":"public"}},"required":[],"additionalProperties":true}"#,
            r#"{"type":"object","type":"object","properties":{},"required":[],"additionalProperties":false}"#,
        ] {
            let assets = BTreeMap::from([(
                "schemas/compat.json".into(),
                unsafe_asset.as_bytes().to_vec(),
            )]);
            assert!(
                declared.manifest().validate_schema_assets(&assets).is_err(),
                "accepted schema asset: {unsafe_asset}"
            );
        }

        let oversized = vec![b' '; 64 * 1024 + 1];
        assert!(
            declared
                .manifest()
                .validate_schema_assets(&BTreeMap::from([(
                    "schemas/compat.json".into(),
                    oversized
                )]))
                .is_err()
        );

        let mut deep = serde_json::json!({
            "type": "string",
            "maxLength": 16,
            "x-gamenode-classification": "public"
        });
        for _ in 0..9 {
            deep = serde_json::json!({
                "type": "object",
                "properties": {"child": deep},
                "required": ["child"],
                "additionalProperties": false,
                "x-gamenode-classification": "public"
            });
        }
        let deep = serde_json::to_vec(&deep).unwrap();
        assert!(
            declared
                .manifest()
                .validate_schema_assets(&BTreeMap::from([("schemas/compat.json".into(), deep)]))
                .is_err()
        );

        let mut properties = serde_json::Map::new();
        for i in 0..90 {
            properties.insert(
                format!("field{i}"),
                serde_json::json!({
                    "type": "string",
                    "maxLength": 16,
                    "x-gamenode-classification": "public"
                }),
            );
        }
        let broad = serde_json::json!({
            "type": "object",
            "properties": properties,
            "required": [],
            "additionalProperties": false
        });
        let broad = serde_json::to_vec(&broad).unwrap();
        assert!(
            declared
                .manifest()
                .validate_schema_assets(&BTreeMap::from([("schemas/compat.json".into(), broad)]))
                .is_err()
        );
    }
}
