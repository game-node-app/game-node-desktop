//! Hybrid typed records, versioned extension namespaces, and custom-data wire shapes.
//!
//! The typed runtime fields remain authoritative. Extension values are bounded annotations; an
//! older client should patch only the namespace it understands and carry every other namespace
//! forward unchanged. Both the Data service and the owner-only `RuntimesUpdate` namespace patch are
//! served by the desktop host; the host, not the client, assigns the next revision and validates
//! any packaged schema before committing.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use gamenode_plugin_protocol::WireError;
use gamenode_plugin_sdk::{
    Durability, Resources, Status, extensions, idempotency_key,
    prost_types::{Struct, Value, value::Kind},
    proto::{
        self, ExtensionNamespacePatch, ExtensionPatch, ExtensionPayload, RuntimeRecord,
        StorageDurability, extension_namespace_patch::Operation,
    },
};

pub const KNOWN_NAMESPACE: &str = "org.example.runtime";
pub const FUTURE_NAMESPACE: &str = "org.vendor.future-runtime";
const RUNTIME_V1_SCHEMA: &[u8] = br#"{"type":"object","properties":{"display_family":{"type":"string"}},"additionalProperties":false}"#;
const FUTURE_RUNTIME_V9_SCHEMA: &[u8] =
    br#"{"type":"object","properties":{"new_hint":{"type":"string"}},"additionalProperties":true}"#;
// SHA-256 of the exact schema JSON byte strings above. This SDK example intentionally does not
// add a separate crypto dependency just to calculate fixture constants at runtime.
const RUNTIME_V1_DIGEST: [u8; 32] = [
    0xad, 0xff, 0x7d, 0x45, 0x8e, 0xcc, 0x45, 0x7a, 0x23, 0xb3, 0x57, 0x06, 0x07, 0x10, 0xc4, 0xa5,
    0x2a, 0xea, 0x4f, 0xf7, 0x28, 0x64, 0x8b, 0xa4, 0xab, 0xc6, 0x86, 0xdc, 0x3f, 0x21, 0x0d, 0xe3,
];
const FUTURE_RUNTIME_V9_DIGEST: [u8; 32] = [
    0xfe, 0x9e, 0x92, 0x61, 0xb9, 0x16, 0x4d, 0x56, 0x87, 0x30, 0xca, 0xac, 0x16, 0x91, 0x6d, 0x50,
    0xe9, 0x4b, 0xa8, 0x62, 0xad, 0xdd, 0x1b, 0xb9, 0x91, 0x5c, 0x30, 0x88, 0xf8, 0xf4, 0xef, 0x50,
];

fn object(fields: impl IntoIterator<Item = (String, Value)>) -> Struct {
    Struct {
        fields: fields.into_iter().collect::<BTreeMap<_, _>>(),
    }
}

fn string(value: &str) -> Value {
    Value {
        kind: Some(Kind::StringValue(value.to_owned())),
    }
}

fn schema_fixture(schema_id: &str) -> (&'static [u8], &'static [u8; 32]) {
    match schema_id {
        "org.example.runtime.v1" => (RUNTIME_V1_SCHEMA, &RUNTIME_V1_DIGEST),
        "org.vendor.future-runtime.v9" => (FUTURE_RUNTIME_V9_SCHEMA, &FUTURE_RUNTIME_V9_DIGEST),
        _ => unreachable!("this example only declares its two fixture schemas"),
    }
}

fn payload(schema_id: &str, schema_version: u32, data: Struct) -> ExtensionPayload {
    let (schema_json, schema_digest) = schema_fixture(schema_id);
    debug_assert!(!schema_json.is_empty());
    ExtensionPayload {
        schema_id: schema_id.to_owned(),
        schema_version,
        schema_digest: schema_digest.to_vec(),
        data: Some(data),
    }
}

/// Typed fields such as identity, version, supported formats and revision stay outside `Struct`.
/// The future namespace is intentionally included so clients that do not understand it can
/// preserve it without interpreting it.
pub fn hybrid_runtime() -> RuntimeRecord {
    RuntimeRecord {
        id: "runtime:example".into(),
        name: "Example Runtime".into(),
        version: "2.4.1".into(),
        executable_handle: "opaque-runtime-handle".into(),
        supported_formats: vec!["rom.example".into()],
        platform: std::env::consts::OS.into(),
        extensions: [
            (
                KNOWN_NAMESPACE.into(),
                payload(
                    "org.example.runtime.v1",
                    1,
                    object([(String::from("display_family"), string("desktop"))]),
                ),
            ),
            (
                FUTURE_NAMESPACE.into(),
                payload(
                    "org.vendor.future-runtime.v9",
                    9,
                    object([(String::from("new_hint"), string("preserve without parsing"))]),
                ),
            ),
        ]
        .into(),
        revision: 7,
        observed_at: 1_800_000_000,
        ..Default::default()
    }
}

/// The revision-checked patch used by [`patch_known_runtime_namespace`]. Only the described
/// namespace and typed fields change; every other namespace is retained by the host.
pub fn known_namespace_patch(current_revision: u64) -> ExtensionPatch {
    ExtensionPatch {
        expected_revision: current_revision,
        typed_field_mask: vec!["name".into()],
        namespaces: vec![ExtensionNamespacePatch {
            namespace: KNOWN_NAMESPACE.into(),
            operation: Some(Operation::Set(payload(
                "org.example.runtime.v1",
                1,
                object([(String::from("display_family"), string("handheld"))]),
            ))),
        }],
    }
}

/// Build a revision-checked patch for only the namespace this client understands. The protocol
/// helper preserves the unknown namespace and host-owned fields, matching `RuntimesUpdate`.
pub fn patch_known_runtime_namespace(current: &RuntimeRecord) -> Result<RuntimeRecord, WireError> {
    let patch = known_namespace_patch(current.revision);
    let proposed = RuntimeRecord {
        name: "Example Runtime (handheld)".into(),
        ..current.clone()
    };
    extensions::apply_runtime_patch(current, &proposed, &patch)
}

/// A consumer may request only fields the host has authorized for its projection. This helper
/// illustrates the field shape; it does not create a grant or replace host-side projection.
pub fn project_display_family(record: &RuntimeRecord) -> Result<Struct, WireError> {
    let value = record
        .extensions
        .get(KNOWN_NAMESPACE)
        .and_then(|extension| extension.data.as_ref())
        .ok_or(WireError)?;
    extensions::project_struct_paths(value, &BTreeSet::from(["display_family".into()]))
}

/// The signed descriptor for the namespace this client understands. The host resolves this from
/// the active package; an example builds it locally to show the shape.
pub fn known_descriptor() -> proto::ExtensionSchemaDescriptor {
    proto::ExtensionSchemaDescriptor {
        namespace: KNOWN_NAMESPACE.into(),
        schema_id: "org.example.runtime.v1".into(),
        schema_version: 1,
        schema_digest: RUNTIME_V1_DIGEST.to_vec(),
        schema_json: RUNTIME_V1_SCHEMA.to_vec(),
        fields: vec![proto::ExtensionSchemaField {
            path: "display_family".into(),
            classification: proto::ExtensionFieldClassification::Public as i32,
        }],
        optional: true,
    }
}

/// Projects only the declared and granted fields of a known namespace. A shared catalog read
/// passes the recipient's current field grant; this helper performs structure, not authority.
pub fn project_known_namespace(
    record: &RuntimeRecord,
    granted: &BTreeSet<String>,
) -> Result<ExtensionPayload, WireError> {
    let payload = record.extensions.get(KNOWN_NAMESPACE).ok_or(WireError)?;
    extensions::project_payload_fields(payload, &known_descriptor(), granted)
}

/// A descriptor for a future schema version this client does not know. `schema_version` and
/// `schema_digest` are part of the identity: a v9 payload never validates against v1, and an
/// unsupported namespace is kept as an opaque envelope instead of being projected or rewritten.
pub fn future_descriptor() -> proto::ExtensionSchemaDescriptor {
    proto::ExtensionSchemaDescriptor {
        namespace: FUTURE_NAMESPACE.into(),
        schema_id: "org.vendor.future-runtime.v9".into(),
        schema_version: 9,
        schema_digest: FUTURE_RUNTIME_V9_DIGEST.to_vec(),
        schema_json: FUTURE_RUNTIME_V9_SCHEMA.to_vec(),
        fields: vec![proto::ExtensionSchemaField {
            path: "new_hint".into(),
            classification: proto::ExtensionFieldClassification::Public as i32,
        }],
        optional: true,
    }
}

/// The explicit unsupported outcome: reading a future namespace with an older descriptor fails
/// schema validation. The caller must choose to carry the opacity forward, not interpret it.
pub fn reject_future_namespace(record: &RuntimeRecord) -> Result<(), WireError> {
    let payload = record.extensions.get(FUTURE_NAMESPACE).ok_or(WireError)?;
    extensions::validate_payload_for_schema(payload, &known_descriptor())
}

/// Keeps only the custom fields a consumer was granted. A `Struct` field carries data only; the
/// host's projection drops every ungranted key before the record leaves the broker.
pub fn project_custom_dataset(
    value: &Struct,
    granted: &BTreeSet<String>,
) -> Result<Struct, WireError> {
    extensions::project_struct_paths(value, granted)
}

/// The custom envelope uses a versioned dataset identity. A future `schema_version` is accepted
/// as an opaque label for custom records: Core does not interpret those fields, and a consumer
/// still sees only the fields its review grants.
pub fn custom_dataset_wire_shape_with_version(schema_version: u32) -> proto::DataPublishRequest {
    let mut request = custom_dataset_wire_shape();
    request.schema_version = schema_version;
    request
}

/// The custom Data envelope carries a versioned Struct. This constructs a request only; send it
/// through the generated `ResourceHost` client with a session context and `data.publish` grant.
pub fn custom_dataset_wire_shape() -> proto::DataPublishRequest {
    proto::DataPublishRequest {
        mutation: None,
        dataset_id: "org.example.game-notes".into(),
        schema_id: "org.example.game-notes.v2".into(),
        schema_version: 2,
        value: Some(object([
            (String::from("game_key"), string("game:example")),
            (String::from("note"), string("remember controller layout")),
        ])),
        durability: StorageDurability::SessionOnly as i32,
        ttl_seconds: 86_400,
        opaque_payload: Vec::new(),
        resource_id: "game-notes-producer".into(),
        observed_at: 1_800_000_000,
        sequence: 1,
        source_observation_id: "game-notes-observation-1".into(),
    }
}

/// Constructs an opaque custom-dataset publication. Consumers need an explicit
/// `opaque_payload` field grant to receive the bytes.
pub fn opaque_dataset_wire_shape() -> proto::DataPublishRequest {
    proto::DataPublishRequest {
        mutation: None,
        dataset_id: "org.example.game-notes.binary".into(),
        schema_id: "org.example.game-notes.binary.v1".into(),
        schema_version: 1,
        value: None,
        durability: StorageDurability::SessionOnly as i32,
        ttl_seconds: 86_400,
        opaque_payload: vec![0, 1, 2, 255],
        resource_id: "game-notes-producer".into(),
        observed_at: 1_800_000_000,
        sequence: 1,
        source_observation_id: "game-notes-binary-observation-1".into(),
    }
}

/// Persistent opaque bytes remain private Storage data and need a separate persistence grant.
pub async fn keep_private_opaque_bytes(
    resources: &Resources,
    key: &str,
    bytes: &[u8],
) -> Result<gamenode_plugin_sdk::StoredWrite, Status> {
    let current = resources.storage_get(key).await?;
    resources
        .storage_put(
            key,
            current.revision,
            &idempotency_key(),
            bytes.to_vec(),
            Durability::Persistent,
            Some(Duration::from_secs(86_400)),
        )
        .await
}

fn main() {}
