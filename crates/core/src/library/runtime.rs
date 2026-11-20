//! Approved runtime resources and content-to-runtime bindings.
//!
//! Runtime paths and content locations are never stored here. Resource handles are opaque host
//! identifiers, and game/install identity remains independent of the runtime that can launch it.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_RUNTIME_ID_BYTES: usize = 128;
pub const MAX_RUNTIME_FIELD_BYTES: usize = 4096;
pub const MAX_RUNTIME_ITEMS: usize = 128;

/// A durable, host-only reference to a location inside an approved file root. Unlike the opaque
/// handle returned to a plugin, this reference is not session-sealed and can be stored. It never
/// includes an absolute path; the host rechecks the approval and resolves the relative path when
/// it issues a fresh session handle or launches a runtime.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovedFileReference {
    pub approval_id: String,
    pub relative_path: Vec<String>,
}

impl ApprovedFileReference {
    pub fn validate(&self) -> bool {
        !self.approval_id.is_empty()
            && self.approval_id.len() <= MAX_RUNTIME_ID_BYTES
            && !self
                .approval_id
                .chars()
                .any(|ch| ch == '\0' || ch.is_control())
            && self.relative_path.len() <= MAX_RUNTIME_ITEMS
            && self.relative_path.iter().all(|part| {
                !part.is_empty()
                    && part.len() <= MAX_RUNTIME_FIELD_BYTES
                    && part != "."
                    && part != ".."
                    && !part.chars().any(|ch| matches!(ch, '/' | '\\' | '\0'))
                    && !part.chars().any(char::is_control)
            })
    }
}

/// A plugin-owned, host-approved installed runtime. This describes software already present on
/// the host; registering it never installs or downloads anything.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeResource {
    pub id: String,
    pub name: String,
    pub version: String,
    /// Stable host-only reference to an approved executable; never a session token or path.
    pub executable: ApprovedFileReference,
    /// Target platform identifier, such as `linux` or `windows`.
    pub platform: String,
    /// Content formats this runtime can execute, such as `snes-rom`.
    pub supported_formats: Vec<String>,
    /// Publisher-owned descriptive extension annotations. The host validates these at the
    /// protocol boundary and never interprets them as authority or launch instructions.
    #[serde(default)]
    pub extensions: BTreeMap<String, RuntimeExtension>,
    /// Ordered argument template. Each token is a literal or a content path; never shell text.
    pub launch_arguments: Vec<RuntimeArgumentTemplate>,
    pub revision: u64,
    /// Unix seconds assigned by the host when the resource is stored.
    pub observed_at: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeExtension {
    pub schema_id: String,
    pub schema_version: u32,
    pub schema_digest: Vec<u8>,
    pub data: serde_json::Map<String, serde_json::Value>,
}

/// Authenticated identity registered against a durable runtime owner key.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeOwnerIdentity {
    pub owner_key: String,
    pub plugin_id: String,
    pub publisher_fingerprint: String,
    pub profile_id: String,
}

/// Host-only common-catalog row. It contains approved references internally; RPC projections
/// must omit them and authorize the owner identity before returning any metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCatalogRecord {
    pub owner: RuntimeOwnerIdentity,
    pub runtime: RuntimeResource,
    pub bindings: Vec<RuntimeContentBinding>,
}

impl RuntimeResource {
    pub fn validate(&self) -> bool {
        let valid_text = |value: &str, max: usize| {
            !value.is_empty()
                && value.len() <= max
                && !value.chars().any(|ch| ch == '\0' || ch.is_control())
        };
        valid_text(&self.id, MAX_RUNTIME_ID_BYTES)
            && valid_text(&self.name, MAX_RUNTIME_FIELD_BYTES)
            && self.version.len() <= MAX_RUNTIME_FIELD_BYTES
            && !self.version.chars().any(|ch| ch == '\0' || ch.is_control())
            && self.executable.validate()
            && valid_text(&self.platform, MAX_RUNTIME_ID_BYTES)
            && !self.supported_formats.is_empty()
            && self.supported_formats.len() <= MAX_RUNTIME_ITEMS
            && self
                .supported_formats
                .iter()
                .all(|format| valid_text(format, MAX_RUNTIME_ID_BYTES))
            && self.launch_arguments.len() <= MAX_RUNTIME_ITEMS
            && self
                .launch_arguments
                .iter()
                .all(RuntimeArgumentTemplate::validate)
            && self.extensions.len() <= 32
            && self.extensions.iter().all(|(namespace, extension)| {
                !namespace.is_empty()
                    && namespace.len() <= 256
                    && namespace.split('.').count() >= 2
                    && !namespace.chars().any(char::is_control)
                    && !extension.schema_id.is_empty()
                    && extension.schema_id.len() <= 256
                    && extension.schema_version > 0
                    && extension.schema_digest.len() == 32
                    && (extension.schema_id == *namespace
                        || extension.schema_id.starts_with(&format!("{namespace}.")))
                    && serde_json::to_vec(&extension.data)
                        .is_ok_and(|bytes| bytes.len() <= 64 * 1024)
            })
            && self.revision > 0
            && self.observed_at >= 0
    }
}

/// One token in a runtime's argument vector. The host inserts a content path as one argv item;
/// literal arguments remain separate items and are never expanded by a shell.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum RuntimeArgumentTemplate {
    Literal(String),
    ContentPath,
}

impl RuntimeArgumentTemplate {
    fn validate(&self) -> bool {
        match self {
            Self::Literal(value) => value.len() <= MAX_RUNTIME_FIELD_BYTES && !value.contains('\0'),
            Self::ContentPath => true,
        }
    }
}

/// A stable logical game's content as runnable by one plugin-owned runtime.
///
/// `content_id` remains stable when content is moved or renamed; `content` identifies its current
/// approved location and is not a path or file payload. The composite key allows one content to
/// have several selectable runtime bindings and one runtime to serve many games.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeContentBinding {
    pub game_id: u64,
    pub installation_id: String,
    /// Stable host-issued identity, independent of content's current name or location.
    pub content_id: String,
    /// Current approved host-only location, never a session token or absolute path.
    pub content: ApprovedFileReference,
    pub content_kind: String,
    pub runtime_id: String,
    pub supported_platforms: Vec<String>,
    pub revision: u64,
}

impl RuntimeContentBinding {
    pub fn validate(&self) -> bool {
        let valid = |value: &str, max: usize| {
            !value.is_empty()
                && value.len() <= max
                && !value.chars().any(|ch| ch == '\0' || ch.is_control())
        };
        self.game_id > 0
            && valid(&self.installation_id, MAX_RUNTIME_ID_BYTES)
            && valid(&self.content_id, MAX_RUNTIME_ID_BYTES)
            && self.content.validate()
            && valid(&self.content_kind, MAX_RUNTIME_ID_BYTES)
            && valid(&self.runtime_id, MAX_RUNTIME_ID_BYTES)
            && self.supported_platforms.len() <= MAX_RUNTIME_ITEMS
            && self
                .supported_platforms
                .iter()
                .all(|platform| valid(platform, MAX_RUNTIME_ID_BYTES))
            && self.revision > 0
    }
}

/// Outcome of a revision-checked runtime or binding mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeMutation {
    Applied { revision: u64 },
    Conflict { current: u64 },
    Missing,
}
