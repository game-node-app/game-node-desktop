use std::collections::{BTreeMap, HashMap};

use gamenode_plugin_protocol::{WireError, extensions::*, proto::*};
use prost::Message;
use prost_types::{Struct, Value, value::Kind};

fn payload(schema_id: &str, label: &str) -> ExtensionPayload {
    ExtensionPayload {
        schema_id: schema_id.into(),
        schema_version: 1,
        schema_digest: vec![7; 32],
        data: Some(Struct {
            fields: BTreeMap::from([(
                "label".into(),
                Value {
                    kind: Some(Kind::StringValue(label.into())),
                },
            )]),
        }),
    }
}

fn descriptor() -> ExtensionSchemaDescriptor {
    let schema_json = br#"{"type":"object","properties":{"label":{"type":"string"}}}"#.to_vec();
    use sha2::Digest;
    ExtensionSchemaDescriptor {
        namespace: "org.example.runtime".into(),
        schema_id: "org.example.runtime.v1".into(),
        schema_version: 1,
        schema_digest: sha2::Sha256::digest(&schema_json).to_vec(),
        schema_json,
        fields: vec![ExtensionSchemaField {
            path: "label".into(),
            classification: ExtensionFieldClassification::Public as i32,
        }],
        optional: true,
    }
}

fn broker_projection_descriptor() -> ExtensionSchemaDescriptor {
    let schema_json = br#"{"type":"object","properties":{"summary":{"type":"string"},"custom":{"type":"string"},"sensitive_history":{"type":"string"}}}"#.to_vec();
    use sha2::Digest;
    ExtensionSchemaDescriptor {
        namespace: "org.example.telemetry".into(),
        schema_id: "org.example.telemetry.v1".into(),
        schema_version: 1,
        schema_digest: sha2::Sha256::digest(&schema_json).to_vec(),
        schema_json,
        fields: vec![
            ExtensionSchemaField {
                path: "summary".into(),
                classification: ExtensionFieldClassification::Public as i32,
            },
            ExtensionSchemaField {
                path: "custom".into(),
                classification: ExtensionFieldClassification::Private as i32,
            },
            ExtensionSchemaField {
                path: "sensitive_history".into(),
                classification: ExtensionFieldClassification::Sensitive as i32,
            },
        ],
        optional: true,
    }
}

fn string_value(value: &str) -> Value {
    Value {
        kind: Some(Kind::StringValue(value.into())),
    }
}

#[test]
fn extension_schema_is_bounded_packaged_and_digest_checked() {
    let descriptor = descriptor();
    validate_schema_descriptor(&descriptor).unwrap();
    validate_schema_catalog(std::slice::from_ref(&descriptor)).unwrap();
    let mut wrong = descriptor.clone();
    wrong.schema_digest[0] ^= 1;
    assert_eq!(validate_schema_descriptor(&wrong), Err(WireError));
    let mut external_ref = descriptor.clone();
    external_ref.schema_json =
        br#"{"type":"object","$ref":"https://example.test/schema"}"#.to_vec();
    use sha2::Digest;
    external_ref.schema_digest = sha2::Sha256::digest(&external_ref.schema_json).to_vec();
    assert_eq!(validate_schema_descriptor(&external_ref), Err(WireError));
    assert_eq!(
        validate_schema_catalog(&[descriptor.clone(), descriptor]),
        Err(WireError)
    );
}

#[test]
fn extension_payload_limits_and_classification_are_enforced() {
    let schema = descriptor();
    let mut valid_payload = payload("org.example.runtime.v1", "stable");
    valid_payload.schema_digest = schema.schema_digest.clone();
    validate_payload_for_schema(&valid_payload, &schema).unwrap();

    for protected_key in ["revision", "owner", "permissions", "executable", "command"] {
        let mut shadow = payload("org.example.runtime.v1", "stable");
        shadow.data.as_mut().unwrap().fields.insert(
            protected_key.into(),
            Value {
                kind: Some(Kind::StringValue("forged".into())),
            },
        );
        assert_eq!(validate_payload(&shadow), Err(WireError), "{protected_key}");
    }

    let mut non_finite = payload("org.example.runtime.v1", "stable");
    non_finite.data.as_mut().unwrap().fields.insert(
        "measurement".into(),
        Value {
            kind: Some(Kind::NumberValue(f64::NAN)),
        },
    );
    assert_eq!(validate_payload(&non_finite), Err(WireError));

    let mut oversized = payload("org.example.runtime.v1", "stable");
    oversized.data.as_mut().unwrap().fields.insert(
        "large".into(),
        Value {
            kind: Some(Kind::StringValue("x".repeat(MAX_EXTENSION_BYTES))),
        },
    );
    assert_eq!(validate_payload(&oversized), Err(WireError));

    let mut many_keys = payload("org.example.runtime.v1", "stable");
    many_keys.data.as_mut().unwrap().fields = (0..=MAX_OBJECT_KEYS)
        .map(|index| {
            (
                format!("k{index}"),
                Value {
                    kind: Some(Kind::BoolValue(true)),
                },
            )
        })
        .collect();
    assert_eq!(validate_payload(&many_keys), Err(WireError));

    let mut deep = Value {
        kind: Some(Kind::StringValue("leaf".into())),
    };
    for depth in 0..=MAX_DEPTH {
        deep = Value {
            kind: Some(Kind::StructValue(Struct {
                fields: BTreeMap::from([(format!("level{depth}"), deep)]),
            })),
        };
    }
    let mut deep_payload = payload("org.example.runtime.v1", "stable");
    deep_payload.data.as_mut().unwrap().fields = BTreeMap::from([("root".into(), deep)]);
    assert_eq!(validate_payload(&deep_payload), Err(WireError));
}

#[test]
fn broker_projection_returns_only_explicitly_granted_custom_fields() {
    let descriptor = broker_projection_descriptor();
    let mut payload = ExtensionPayload {
        schema_id: descriptor.schema_id.clone(),
        schema_version: descriptor.schema_version,
        schema_digest: descriptor.schema_digest.clone(),
        data: Some(Struct {
            fields: BTreeMap::from([
                ("summary".into(), string_value("frame pacing")),
                ("custom".into(), string_value("producer-specific detail")),
                (
                    "sensitive_history".into(),
                    string_value("session observation"),
                ),
            ]),
        }),
    };
    validate_payload_for_schema(&payload, &descriptor).unwrap();

    let public_only =
        project_payload_fields(&payload, &descriptor, &["summary".to_owned()].into()).unwrap();
    let fields = &public_only.data.as_ref().unwrap().fields;
    assert_eq!(fields.len(), 1);
    assert_eq!(fields["summary"], string_value("frame pacing"));
    assert!(!fields.contains_key("custom"));
    assert!(!fields.contains_key("sensitive_history"));

    let reviewed = project_payload_fields(
        &payload,
        &descriptor,
        &["summary".to_owned(), "sensitive_history".to_owned()].into(),
    )
    .unwrap();
    let fields = &reviewed.data.as_ref().unwrap().fields;
    assert_eq!(fields.len(), 2);
    assert!(fields.contains_key("summary"));
    assert!(fields.contains_key("sensitive_history"));
    assert!(!fields.contains_key("custom"));

    // An unreviewed custom namespace is not made visible by a field grant from another schema.
    payload.schema_id = "org.example.other.v1".into();
    assert_eq!(
        project_payload_fields(&payload, &descriptor, &["summary".to_owned()].into()),
        Err(WireError)
    );
}

#[test]
fn namespace_projection_keeps_only_selected_nested_fields() {
    let value = Struct {
        fields: BTreeMap::from([
            (
                "display".into(),
                Value {
                    kind: Some(Kind::StructValue(Struct {
                        fields: BTreeMap::from([
                            ("name".into(), string_value("Runtime")),
                            ("internal_path".into(), string_value("/private/runtime")),
                        ]),
                    })),
                },
            ),
            ("owner".into(), string_value("forged-owner")),
        ]),
    };

    let projected = project_struct_paths(&value, &["display.name".to_owned()].into()).unwrap();
    assert_eq!(projected.fields.len(), 1);
    let Some(Kind::StructValue(display)) = projected.fields["display"].kind.as_ref() else {
        panic!("selected namespace should remain an object");
    };
    assert_eq!(
        display.fields,
        BTreeMap::from([("name".into(), string_value("Runtime"))])
    );
    assert!(!projected.fields.contains_key("owner"));
}

#[test]
fn namespace_patch_preserves_unknown_namespaces_and_typed_u64_values() {
    let current = HashMap::from([
        (
            "org.example.runtime".into(),
            payload("org.example.runtime.v1", "old"),
        ),
        (
            "org.unknown.consumer".into(),
            payload("org.unknown.consumer.v2", "preserve-me"),
        ),
    ]);
    let mut extensions = ExtensionSet::from_wire(current).unwrap();
    let patch = ExtensionPatch {
        expected_revision: 9,
        typed_field_mask: vec!["name".into()],
        namespaces: vec![ExtensionNamespacePatch {
            namespace: "org.example.runtime".into(),
            operation: Some(extension_namespace_patch::Operation::Set(payload(
                "org.example.runtime.v1",
                "new",
            ))),
        }],
    };
    validate_patch(&patch).unwrap();
    assert_eq!(extensions.apply_patch(&patch, 9), Ok(10));
    assert_eq!(
        extensions.as_map()["org.unknown.consumer"],
        payload("org.unknown.consumer.v2", "preserve-me")
    );
    assert_eq!(
        extensions.as_map()["org.example.runtime"],
        payload("org.example.runtime.v1", "new")
    );
    assert_eq!(extensions.apply_patch(&patch, 10), Err(WireError));

    // A usable typed resource remains valid when this client does not recognize an optional
    // extension schema. Typed identity and revision remain outside the extension Struct.
    let runtime = RuntimeRecord {
        id: "runtime-one".into(),
        name: "Old name".into(),
        revision: 9,
        extensions: extensions.into_wire(),
        ..Default::default()
    };
    let updated = apply_runtime_patch(
        &runtime,
        &RuntimeRecord {
            name: "New name".into(),
            ..runtime.clone()
        },
        &ExtensionPatch {
            expected_revision: 9,
            typed_field_mask: vec!["name".into()],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(updated.id, "runtime-one");
    assert_eq!(updated.name, "New name");
    assert_eq!(updated.revision, 10);
    assert_eq!(
        updated.extensions["org.unknown.consumer"],
        payload("org.unknown.consumer.v2", "preserve-me")
    );
    let decoded_with_extension = RuntimeRecord::decode(updated.encode_to_vec().as_slice()).unwrap();
    assert_eq!(decoded_with_extension.id, "runtime-one");
    assert_eq!(decoded_with_extension.name, "New name");
    assert_eq!(decoded_with_extension.revision, 10);
    assert_eq!(
        decoded_with_extension.extensions["org.unknown.consumer"],
        payload("org.unknown.consumer.v2", "preserve-me"),
        "an unsupported optional namespace must not invalidate its typed base"
    );

    let runtime = RuntimeRecord {
        id: "runtime-one".into(),
        revision: u64::MAX,
        observed_at: i64::MAX,
        ..Default::default()
    };
    let decoded = RuntimeRecord::decode(runtime.encode_to_vec().as_slice()).unwrap();
    assert_eq!(decoded.revision, u64::MAX);
    assert_eq!(decoded.observed_at, i64::MAX);
}

#[test]
fn extension_set_enforces_object_key_budget_across_all_namespaces() {
    fn payload_with_keys(schema_id: &str, count: usize) -> ExtensionPayload {
        ExtensionPayload {
            schema_id: schema_id.into(),
            schema_version: 1,
            schema_digest: vec![3; 32],
            data: Some(Struct {
                fields: (0..count)
                    .map(|index| (format!("field-{index}"), string_value("value")))
                    .collect(),
            }),
        }
    }

    let within_budget = HashMap::from([
        (
            "org.example.first".into(),
            payload_with_keys("org.example.first.v1", MAX_OBJECT_KEYS / 2),
        ),
        (
            "org.example.second".into(),
            payload_with_keys("org.example.second.v1", MAX_OBJECT_KEYS / 2),
        ),
    ]);
    assert!(ExtensionSet::from_wire(within_budget).is_ok());

    let over_budget = HashMap::from([
        (
            "org.example.first".into(),
            payload_with_keys("org.example.first.v1", MAX_OBJECT_KEYS / 2 + 1),
        ),
        (
            "org.example.second".into(),
            payload_with_keys("org.example.second.v1", MAX_OBJECT_KEYS / 2 + 1),
        ),
    ]);
    assert_eq!(ExtensionSet::from_wire(over_budget), Err(WireError));
}
