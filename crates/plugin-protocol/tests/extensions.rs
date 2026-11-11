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

    let mut shadow = payload("org.example.runtime.v1", "stable");
    shadow.data.as_mut().unwrap().fields.insert(
        "revision".into(),
        Value {
            kind: Some(Kind::NumberValue(7.0)),
        },
    );
    assert_eq!(validate_payload(&shadow), Err(WireError));

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
