//! Bounded, declarative resource metadata extensions.
//!
//! Struct values are annotations only. The host never interprets them as
//! identity, authorization, filesystem paths, or executable commands.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use prost::Message;
use prost_types::{Value, value::Kind};

use crate::{WireError, proto};

pub const MAX_NAMESPACES: usize = 32;
pub const MAX_EXTENSION_BYTES: usize = 64 * 1024;
pub const MAX_SCHEMA_BYTES: usize = 64 * 1024;
pub const MAX_SCHEMA_CATALOG_BYTES: usize = 256 * 1024;
pub const MAX_DEPTH: usize = 8;
pub const MAX_OBJECT_KEYS: usize = 256;
pub const MAX_SCHEMA_FIELDS: usize = 256;

const PROTECTED_ROOT_KEYS: &[&str] = &[
    "id",
    "revision",
    "source",
    "provenance",
    "owner",
    "creator",
    "permissions",
    "scope",
    "executable",
    "executable_handle",
    "argv",
    "cwd",
    "path",
    "launch_target",
    "operation",
    "command",
];

#[derive(Default)]
struct ProjectionNode {
    include_value: bool,
    children: BTreeMap<String, ProjectionNode>,
}

/// Returns a copy containing only the requested declared field paths.
///
/// A selected path includes its entire value. Callers must derive `paths` from
/// the recipient's current field grants and the dataset's validated schema;
/// this helper only performs structural projection and grants no authority.
pub fn project_struct_paths(
    value: &prost_types::Struct,
    paths: &BTreeSet<String>,
) -> Result<prost_types::Struct, WireError> {
    if paths.len() > MAX_SCHEMA_FIELDS {
        return Err(WireError);
    }
    if paths.is_empty() {
        return Ok(prost_types::Struct::default());
    }
    let mut root = ProjectionNode::default();
    for path in paths {
        if !valid_field_path(path) {
            return Err(WireError);
        }
        let mut node = &mut root;
        for part in path.split('.') {
            node = node.children.entry(part.to_owned()).or_default();
        }
        node.include_value = true;
    }
    let projected = project_value(
        &prost_types::Value {
            kind: Some(Kind::StructValue(value.clone())),
        },
        &root,
    )?
    .ok_or(WireError)?;
    let Some(Kind::StructValue(projected)) = projected.kind else {
        return Err(WireError);
    };
    if projected.encoded_len() > MAX_EXTENSION_BYTES {
        return Err(WireError);
    }
    let mut key_count = 0;
    validate_struct(&projected, 0, &mut key_count)?;
    Ok(projected)
}

/// Validates a payload against its declared schema and projects only fields
/// that are both declared and present in the caller's current field grant.
pub fn project_payload_fields(
    payload: &proto::ExtensionPayload,
    descriptor: &proto::ExtensionSchemaDescriptor,
    granted_paths: &BTreeSet<String>,
) -> Result<proto::ExtensionPayload, WireError> {
    validate_payload_for_schema(payload, descriptor)?;
    let declared_paths: BTreeSet<_> = descriptor
        .fields
        .iter()
        .map(|field| field.path.as_str())
        .collect();
    if granted_paths.iter().any(|path| !valid_field_path(path)) {
        return Err(WireError);
    }
    let selected_paths = granted_paths
        .iter()
        .filter(|path| declared_paths.contains(path.as_str()))
        .cloned()
        .collect();
    let data = project_struct_paths(payload.data.as_ref().ok_or(WireError)?, &selected_paths)?;
    let mut projected = payload.clone();
    projected.data = Some(data);
    Ok(projected)
}

fn project_value(
    value: &prost_types::Value,
    node: &ProjectionNode,
) -> Result<Option<prost_types::Value>, WireError> {
    if node.include_value {
        return Ok(Some(value.clone()));
    }
    let Some(Kind::StructValue(object)) = value.kind.as_ref() else {
        return Err(WireError);
    };
    let mut projected = prost_types::Struct::default();
    for (key, child_node) in &node.children {
        let Some(child) = object.fields.get(key) else {
            continue;
        };
        if let Some(child) = project_value(child, child_node)? {
            projected.fields.insert(key.clone(), child);
        }
    }
    Ok(Some(prost_types::Value {
        kind: Some(Kind::StructValue(projected)),
    }))
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExtensionSet {
    namespaces: BTreeMap<String, proto::ExtensionPayload>,
}

impl ExtensionSet {
    pub fn from_wire(values: HashMap<String, proto::ExtensionPayload>) -> Result<Self, WireError> {
        if values.len() > MAX_NAMESPACES
            || values.values().map(Message::encoded_len).sum::<usize>() > MAX_EXTENSION_BYTES
        {
            return Err(WireError);
        }
        for (namespace, payload) in &values {
            validate_namespace(namespace)?;
            validate_payload(payload)?;
            if !payload.schema_id.starts_with(&format!("{namespace}."))
                && payload.schema_id != *namespace
            {
                return Err(WireError);
            }
        }
        Ok(Self {
            namespaces: values.into_iter().collect(),
        })
    }

    pub fn as_map(&self) -> &BTreeMap<String, proto::ExtensionPayload> {
        &self.namespaces
    }

    pub fn into_wire(self) -> HashMap<String, proto::ExtensionPayload> {
        self.namespaces.into_iter().collect()
    }

    /// Applies only named namespace operations. Namespaces absent from the
    /// patch—including namespaces the caller does not understand—are retained.
    pub fn apply_patch(
        &mut self,
        patch: &proto::ExtensionPatch,
        current_revision: u64,
    ) -> Result<u64, WireError> {
        validate_patch(patch)?;
        if patch.expected_revision != current_revision {
            return Err(WireError);
        }
        let next_revision = current_revision.checked_add(1).ok_or(WireError)?;
        let mut candidate = self.namespaces.clone();
        let mut seen = BTreeSet::new();
        for change in &patch.namespaces {
            validate_namespace(&change.namespace)?;
            if !seen.insert(&change.namespace) {
                return Err(WireError);
            }
            match change.operation.as_ref().ok_or(WireError)? {
                proto::extension_namespace_patch::Operation::Set(payload) => {
                    validate_payload(payload)?;
                    if !payload
                        .schema_id
                        .starts_with(&format!("{}.", change.namespace))
                        && payload.schema_id != change.namespace
                    {
                        return Err(WireError);
                    }
                    candidate.insert(change.namespace.clone(), payload.clone());
                }
                proto::extension_namespace_patch::Operation::Remove(true) => {
                    candidate.remove(&change.namespace);
                }
                proto::extension_namespace_patch::Operation::Remove(false) => {
                    return Err(WireError);
                }
            }
        }
        if candidate.len() > MAX_NAMESPACES
            || candidate.values().map(Message::encoded_len).sum::<usize>() > MAX_EXTENSION_BYTES
        {
            return Err(WireError);
        }
        self.namespaces = candidate;
        Ok(next_revision)
    }
}

/// Applies a revision-checked runtime patch while preserving host-owned fields
/// and extension namespaces omitted by an older client. The caller must check
/// owner authorization before calling this function.
pub fn apply_runtime_patch(
    current: &proto::RuntimeRecord,
    proposed: &proto::RuntimeRecord,
    patch: &proto::ExtensionPatch,
) -> Result<proto::RuntimeRecord, WireError> {
    validate_patch(patch)?;
    if current.revision != patch.expected_revision || proposed.id != current.id {
        return Err(WireError);
    }
    let mut updated = current.clone();
    for field in &patch.typed_field_mask {
        match field.as_str() {
            "name" => updated.name.clone_from(&proposed.name),
            "version" => updated.version.clone_from(&proposed.version),
            "supported_formats" => updated
                .supported_formats
                .clone_from(&proposed.supported_formats),
            _ => return Err(WireError),
        }
    }
    let mut extensions = ExtensionSet::from_wire(current.extensions.clone())?;
    let revision = extensions.apply_patch(patch, current.revision)?;
    updated.extensions = extensions.into_wire();
    updated.revision = revision;
    Ok(updated)
}

pub fn validate_patch(patch: &proto::ExtensionPatch) -> Result<(), WireError> {
    if patch.typed_field_mask.len() > MAX_SCHEMA_FIELDS {
        return Err(WireError);
    }
    let mut fields = BTreeSet::new();
    for path in &patch.typed_field_mask {
        if !valid_field_path(path) || !fields.insert(path) {
            return Err(WireError);
        }
    }
    let mut namespaces = BTreeSet::new();
    if patch.namespaces.len() > MAX_NAMESPACES {
        return Err(WireError);
    }
    for change in &patch.namespaces {
        validate_namespace(&change.namespace)?;
        if !namespaces.insert(&change.namespace) || change.operation.is_none() {
            return Err(WireError);
        }
        if let Some(proto::extension_namespace_patch::Operation::Set(value)) = &change.operation {
            validate_payload(value)?;
        }
        if matches!(
            change.operation,
            Some(proto::extension_namespace_patch::Operation::Remove(false))
        ) {
            return Err(WireError);
        }
    }
    Ok(())
}

pub fn validate_schema_descriptor(
    descriptor: &proto::ExtensionSchemaDescriptor,
) -> Result<(), WireError> {
    validate_namespace(&descriptor.namespace)?;
    if !valid_schema_id(&descriptor.schema_id)
        || !(descriptor.schema_id == descriptor.namespace
            || descriptor
                .schema_id
                .strip_prefix(&descriptor.namespace)
                .is_some_and(|suffix| suffix.starts_with('.')))
        || descriptor.schema_version == 0
        || descriptor.schema_digest.len() != 32
        || descriptor.schema_json.is_empty()
        || descriptor.schema_json.len() > MAX_SCHEMA_BYTES
        || descriptor.fields.len() > MAX_SCHEMA_FIELDS
        || descriptor.encoded_len() > MAX_SCHEMA_BYTES + 16 * 1024
    {
        return Err(WireError);
    }
    let schema: serde_json::Value =
        serde_json::from_slice(&descriptor.schema_json).map_err(|_| WireError)?;
    use sha2::Digest;
    if sha2::Sha256::digest(&descriptor.schema_json).as_slice() != descriptor.schema_digest {
        return Err(WireError);
    }
    let mut key_count = 0;
    validate_json_value(&schema, 0, &mut key_count)?;
    if !schema.is_object()
        || schema.get("type").and_then(serde_json::Value::as_str) != Some("object")
        || !schema_subset_valid(&schema)
        || contains_remote_ref(&schema)
        || has_protected_schema_properties(&schema)
    {
        return Err(WireError);
    }
    let mut paths = BTreeSet::new();
    for field in &descriptor.fields {
        if !valid_field_path(&field.path)
            || !paths.insert(&field.path)
            || proto::ExtensionFieldClassification::try_from(field.classification).is_err()
            || field.classification == proto::ExtensionFieldClassification::Unspecified as i32
        {
            return Err(WireError);
        }
        let mut node = &schema;
        for part in field.path.split('.') {
            node = node
                .get("properties")
                .and_then(serde_json::Value::as_object)
                .and_then(|properties| properties.get(part))
                .ok_or(WireError)?;
        }
    }
    Ok(())
}

pub fn validate_payload(payload: &proto::ExtensionPayload) -> Result<(), WireError> {
    if !valid_schema_id(&payload.schema_id)
        || payload.schema_version == 0
        || payload.schema_digest.len() != 32
        || payload.data.is_none()
        || payload.encoded_len() > MAX_EXTENSION_BYTES
    {
        return Err(WireError);
    }
    let mut count = 0;
    validate_struct(payload.data.as_ref().ok_or(WireError)?, 0, &mut count)?;
    Ok(())
}

pub fn validate_payload_for_schema(
    payload: &proto::ExtensionPayload,
    descriptor: &proto::ExtensionSchemaDescriptor,
) -> Result<(), WireError> {
    validate_schema_descriptor(descriptor)?;
    validate_payload(payload)?;
    if payload.schema_id != descriptor.schema_id
        || payload.schema_version != descriptor.schema_version
        || payload.schema_digest != descriptor.schema_digest
    {
        return Err(WireError);
    }
    let schema: serde_json::Value =
        serde_json::from_slice(&descriptor.schema_json).map_err(|_| WireError)?;
    validate_struct_shape(payload.data.as_ref().ok_or(WireError)?, &schema)?;
    Ok(())
}

fn validate_struct_shape(
    value: &prost_types::Struct,
    schema: &serde_json::Value,
) -> Result<(), WireError> {
    use serde_json::Value as J;

    let object = schema.as_object().ok_or(WireError)?;
    if let Some(expected) = object.get("type")
        && !schema_type_matches(expected)
    {
        return Err(WireError);
    }
    let properties = object.get("properties").and_then(J::as_object);
    if let Some(properties) = properties {
        if object.get("additionalProperties") == Some(&J::Bool(false))
            && value.fields.keys().any(|key| !properties.contains_key(key))
        {
            return Err(WireError);
        }
        for (key, child) in &value.fields {
            if let Some(child_schema) = properties.get(key) {
                validate_value_shape(child, child_schema)?;
            }
        }
    }
    if let Some(required) = object.get("required").and_then(J::as_array)
        && required
            .iter()
            .filter_map(J::as_str)
            .any(|key| !value.fields.contains_key(key))
    {
        return Err(WireError);
    }
    Ok(())
}

fn validate_value_shape(
    value: &prost_types::Value,
    schema: &serde_json::Value,
) -> Result<(), WireError> {
    use prost_types::value::Kind;
    use serde_json::Value as J;

    let object = schema.as_object().ok_or(WireError)?;
    if let Some(expected) = object.get("type")
        && !value
            .kind
            .as_ref()
            .is_some_and(|kind| kind_matches_type(kind, expected))
    {
        return Err(WireError);
    }
    match value.kind.as_ref().ok_or(WireError)? {
        Kind::StructValue(value) => validate_struct_shape(value, schema),
        Kind::ListValue(list) => {
            if let Some(items) = object.get("items") {
                for item in &list.values {
                    validate_value_shape(item, items)?;
                }
            }
            if let Some(minimum) = object.get("minItems").and_then(J::as_u64)
                && list.values.len() < minimum as usize
            {
                return Err(WireError);
            }
            if let Some(maximum) = object.get("maxItems").and_then(J::as_u64)
                && list.values.len() > maximum as usize
            {
                return Err(WireError);
            }
            Ok(())
        }
        Kind::StringValue(text) => {
            if object
                .get("minLength")
                .and_then(J::as_u64)
                .is_some_and(|min| text.chars().count() < min as usize)
                || object
                    .get("maxLength")
                    .and_then(J::as_u64)
                    .is_some_and(|max| text.chars().count() > max as usize)
            {
                Err(WireError)
            } else {
                Ok(())
            }
        }
        Kind::NumberValue(number) => {
            if object
                .get("minimum")
                .and_then(J::as_f64)
                .is_some_and(|min| *number < min)
                || object
                    .get("maximum")
                    .and_then(J::as_f64)
                    .is_some_and(|max| *number > max)
            {
                Err(WireError)
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

fn schema_type_matches(expected: &serde_json::Value) -> bool {
    expected.as_str() == Some("object")
        || expected
            .as_array()
            .is_some_and(|types| types.iter().any(|name| name.as_str() == Some("object")))
}

fn kind_matches_type(kind: &Kind, expected: &serde_json::Value) -> bool {
    let matches = |name: &str| match (kind, name) {
        (Kind::StructValue(_), "object") | (Kind::ListValue(_), "array") => true,
        (Kind::StringValue(_), "string") | (Kind::BoolValue(_), "boolean") => true,
        (Kind::NumberValue(_), "number") => true,
        (Kind::NumberValue(number), "integer") => number.fract() == 0.0,
        (Kind::NullValue(_), "null") => true,
        _ => false,
    };
    expected.as_str().is_some_and(matches)
        || expected.as_array().is_some_and(|types| {
            types
                .iter()
                .filter_map(serde_json::Value::as_str)
                .any(matches)
        })
}

/// A package may declare no more than one schema owner for each namespace.
pub fn validate_schema_catalog(
    descriptors: &[proto::ExtensionSchemaDescriptor],
) -> Result<(), WireError> {
    if descriptors.len() > MAX_NAMESPACES {
        return Err(WireError);
    }
    let mut namespaces = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut total_bytes = 0usize;
    for descriptor in descriptors {
        validate_schema_descriptor(descriptor)?;
        total_bytes = total_bytes.saturating_add(descriptor.encoded_len());
        if !namespaces.insert(&descriptor.namespace)
            || !ids.insert((&descriptor.schema_id, descriptor.schema_version))
            || total_bytes > MAX_SCHEMA_CATALOG_BYTES
        {
            return Err(WireError);
        }
    }
    Ok(())
}

fn validate_namespace(namespace: &str) -> Result<(), WireError> {
    if !valid_schema_id(namespace)
        || namespace.starts_with("org.gamenode")
        || namespace.split('.').count() < 2
    {
        return Err(WireError);
    }
    Ok(())
}

fn valid_schema_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.split('.').count() >= 2
        && value.split('.').all(|part| {
            !part.is_empty()
                && part.len() <= 63
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && part.as_bytes()[0].is_ascii_alphanumeric()
                && part.as_bytes()[part.len() - 1].is_ascii_alphanumeric()
        })
}

fn valid_field_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.split('.').all(|part| {
            !part.is_empty()
                && part.len() <= 64
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
}

fn validate_struct(
    value: &prost_types::Struct,
    depth: usize,
    key_count: &mut usize,
) -> Result<(), WireError> {
    if depth > MAX_DEPTH {
        return Err(WireError);
    }
    for (key, child) in &value.fields {
        *key_count += 1;
        if *key_count > MAX_OBJECT_KEYS || key.is_empty() || key.len() > 256 {
            return Err(WireError);
        }
        if depth == 0 && PROTECTED_ROOT_KEYS.contains(&key.as_str()) {
            return Err(WireError);
        }
        validate_value(child, depth + 1, key_count)?;
    }
    Ok(())
}

fn validate_value(value: &Value, depth: usize, key_count: &mut usize) -> Result<(), WireError> {
    if depth > MAX_DEPTH {
        return Err(WireError);
    }
    match value.kind.as_ref().ok_or(WireError)? {
        Kind::NumberValue(number) if !number.is_finite() => Err(WireError),
        Kind::StructValue(value) => validate_struct(value, depth, key_count),
        Kind::ListValue(list) => {
            for value in &list.values {
                validate_value(value, depth + 1, key_count)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_json_value(
    value: &serde_json::Value,
    depth: usize,
    key_count: &mut usize,
) -> Result<(), WireError> {
    if depth > MAX_DEPTH {
        return Err(WireError);
    }
    match value {
        serde_json::Value::Object(fields) => {
            for (key, child) in fields {
                *key_count += 1;
                if *key_count > MAX_OBJECT_KEYS || key.is_empty() || key.len() > 256 {
                    return Err(WireError);
                }
                validate_json_value(child, depth + 1, key_count)?;
            }
            Ok(())
        }
        serde_json::Value::Array(values) => {
            for child in values {
                validate_json_value(child, depth + 1, key_count)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn contains_remote_ref(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(fields) => fields.iter().any(|(key, child)| {
            key == "$ref"
                || key == "$dynamicRef"
                || key == "$recursiveRef"
                || contains_remote_ref(child)
        }),
        serde_json::Value::Array(values) => values.iter().any(contains_remote_ref),
        _ => false,
    }
}

fn schema_subset_valid(schema: &serde_json::Value) -> bool {
    use serde_json::Value as J;
    let Some(object) = schema.as_object() else {
        return false;
    };
    for (key, value) in object {
        match key.as_str() {
            "type" => {
                let valid_type = |name: &str| {
                    matches!(
                        name,
                        "object" | "array" | "string" | "number" | "integer" | "boolean" | "null"
                    )
                };
                let valid = value.as_str().is_some_and(valid_type)
                    || value.as_array().is_some_and(|types| {
                        !types.is_empty()
                            && types.iter().all(|v| v.as_str().is_some_and(valid_type))
                    });
                if !valid {
                    return false;
                }
            }
            "properties" => {
                let Some(properties) = value.as_object() else {
                    return false;
                };
                if properties.len() > MAX_OBJECT_KEYS
                    || properties.values().any(|child| !schema_subset_valid(child))
                {
                    return false;
                }
            }
            "items" => {
                if !schema_subset_valid(value) {
                    return false;
                }
            }
            "required" => {
                let Some(required) = value.as_array() else {
                    return false;
                };
                if !required
                    .iter()
                    .all(|v| v.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 256))
                {
                    return false;
                }
                if let Some(properties) = object.get("properties").and_then(J::as_object)
                    && required
                        .iter()
                        .filter_map(J::as_str)
                        .any(|name| !properties.contains_key(name))
                {
                    return false;
                }
            }
            "enum" => {
                if !value
                    .as_array()
                    .is_some_and(|values| values.iter().all(|v| !v.is_object() && !v.is_array()))
                {
                    return false;
                }
            }
            "additionalProperties" => {
                if !value.is_boolean() {
                    return false;
                }
            }
            "minimum" | "maximum" => {
                if !value.as_f64().is_some_and(f64::is_finite) {
                    return false;
                }
            }
            "minLength" | "maxLength" | "minItems" | "maxItems" => {
                if !value
                    .as_u64()
                    .is_some_and(|v| v <= MAX_EXTENSION_BYTES as u64)
                {
                    return false;
                }
            }
            _ => return false,
        }
    }
    // Keep this function intentionally closed; adding a JSON Schema keyword is
    // a protocol change with corresponding bounds and tests.
    !matches!(schema, J::Null)
}

fn has_protected_schema_properties(value: &serde_json::Value) -> bool {
    let Some(properties) = value
        .get("properties")
        .and_then(serde_json::Value::as_object)
    else {
        return false;
    };
    properties
        .keys()
        .any(|key| PROTECTED_ROOT_KEYS.contains(&key.as_str()))
}
