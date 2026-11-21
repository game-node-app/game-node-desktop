use gamenode_plugin_protocol::{
    common_data,
    prost::Message,
    prost_types::FileDescriptorSet,
    proto::{
        CommonDataCategory, CommonDataSelector, DataMutationReply, DataRecord, StorageDurability,
    },
};
use serde_json::Value;

const DESCRIPTOR: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/plugin_descriptor.bin"));
const CONTRACT: &str = include_str!("../contracts/common_data_selector.json");

#[test]
fn common_data_fixture_matches_the_generated_wire_contract() {
    let contract: Value = serde_json::from_str(CONTRACT).unwrap();
    assert_eq!(contract["api"], "1.6");
    assert_eq!(contract["field"]["private_default"], true);

    let descriptor = FileDescriptorSet::decode(DESCRIPTOR).unwrap();
    let request = descriptor
        .file
        .iter()
        .flat_map(|file| &file.message_type)
        .find(|message| message.name.as_deref() == Some("ResourceRequest"))
        .expect("ResourceRequest descriptor");
    let field = request
        .field
        .iter()
        .find(|field| field.name.as_deref() == Some("common_data_categories"))
        .expect("common-data selector field");
    assert_eq!(field.number, Some(13));
    assert_eq!(
        field.type_name.as_deref(),
        Some(".gamenode.plugin.v1.CommonDataSelector")
    );

    let selector = descriptor
        .file
        .iter()
        .flat_map(|file| &file.message_type)
        .find(|message| message.name.as_deref() == Some("CommonDataSelector"))
        .expect("CommonDataSelector descriptor");
    let selected_ids = selector
        .field
        .iter()
        .find(|field| field.name.as_deref() == Some("selected_record_ids"))
        .expect("selected-record selector field");
    assert_eq!(selected_ids.number, Some(2));
    assert_eq!(selected_ids.label, Some(3));

    let data_list = descriptor
        .file
        .iter()
        .flat_map(|file| &file.message_type)
        .find(|message| message.name.as_deref() == Some("DataListRequest"))
        .expect("DataListRequest descriptor");
    assert!(
        data_list
            .field
            .iter()
            .any(|field| field.name.as_deref() == Some("common_data_categories")),
        "Data.List must support the same per-query category and record selector"
    );

    let data_publish = descriptor
        .file
        .iter()
        .flat_map(|file| &file.message_type)
        .find(|message| message.name.as_deref() == Some("DataPublishRequest"))
        .expect("DataPublishRequest descriptor");
    for field in [
        "dataset_id",
        "schema_id",
        "schema_version",
        "value",
        "opaque_payload",
        "resource_id",
        "observed_at",
        "sequence",
        "source_observation_id",
    ] {
        assert!(
            data_publish
                .field
                .iter()
                .any(|candidate| candidate.name.as_deref() == Some(field)),
            "DataPublishRequest must carry typed {field} metadata"
        );
    }
    let data_record = descriptor
        .file
        .iter()
        .flat_map(|file| &file.message_type)
        .find(|message| message.name.as_deref() == Some("DataRecord"))
        .expect("DataRecord descriptor");
    for field in [
        "schema_id",
        "schema_version",
        "opaque_payload",
        "provenance",
        "resource_id",
        "observed_at",
        "received_at",
        "expires_at",
        "sequence",
        "source_observation_id",
    ] {
        assert!(
            data_record
                .field
                .iter()
                .any(|candidate| candidate.name.as_deref() == Some(field)),
            "DataRecord must carry typed {field} metadata"
        );
    }

    for category in contract["categories"].as_array().unwrap() {
        let name = category["name"].as_str().unwrap();
        let number = category["number"].as_i64().unwrap() as i32;
        let permission = category["permission"].as_str().unwrap();
        let decoded = CommonDataCategory::try_from(number).unwrap();
        assert_eq!(
            format!("{decoded:?}").to_ascii_uppercase().replace('_', ""),
            name.replace('_', "")
        );
        assert_eq!(common_data::permission(decoded), Some(permission));
    }

    for query in contract["queries"].as_array().unwrap() {
        let id = query["id"].as_str().unwrap();
        let categories = query["categories"].as_array().unwrap();
        let selector = CommonDataSelector {
            categories: categories
                .iter()
                .map(|name| {
                    let category = contract["categories"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|category| category["name"] == *name)
                        .unwrap();
                    category["number"].as_i64().unwrap() as i32
                })
                .collect(),
            selected_record_ids: query["selected_records"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| id.as_str().unwrap().to_owned())
                .collect(),
        };
        let selected = if query["selector"] == "omitted" {
            common_data::categories(None).unwrap()
        } else {
            common_data::categories(Some(&selector)).unwrap()
        };
        let grants = query["grants"].as_array().unwrap();
        let categories_authorized = selected.iter().all(|category| {
            let required = common_data::permission(*category).unwrap();
            grants.iter().any(|grant| grant.as_str() == Some(required))
        });
        let selected_records = common_data::selected_record_ids(Some(&selector)).unwrap();
        let record_grants = query["record_grants"].as_array().unwrap();
        let records_authorized = selected_records.iter().all(|record| {
            record_grants
                .iter()
                .any(|grant| grant.as_str() == Some(*record))
        });
        let expected = query["expected"].as_str().unwrap();
        let actual = if selected.is_empty() && selected_records.is_empty() {
            "PRIVATE_ONLY"
        } else if categories_authorized && records_authorized {
            "COMMON_INCLUDED"
        } else {
            "PERMISSION_DENIED"
        };
        assert_eq!(actual, expected, "query fixture {id}");
    }
    assert_eq!(contract["excluded_data"].as_array().unwrap().len(), 3);
}

#[test]
fn generated_descriptor_keeps_typed_data_extensions_and_byte_envelopes_stable() {
    let descriptor = FileDescriptorSet::decode(DESCRIPTOR).unwrap();
    let message = |name: &str| {
        descriptor
            .file
            .iter()
            .flat_map(|file| &file.message_type)
            .find(|message| message.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("{name} descriptor"))
    };
    let field = |name: &str, field_name: &str| {
        message(name)
            .field
            .iter()
            .find(|field| field.name.as_deref() == Some(field_name))
            .unwrap_or_else(|| panic!("{name}.{field_name} descriptor"))
    };

    let resource_host = descriptor
        .file
        .iter()
        .flat_map(|file| &file.service)
        .find(|service| service.name.as_deref() == Some("ResourceHost"))
        .expect("ResourceHost descriptor");
    for method in ["DataGet", "DataList", "DataWatch"] {
        assert!(
            resource_host
                .method
                .iter()
                .any(|candidate| candidate.name.as_deref() == Some(method)),
            "ResourceHost.{method} access"
        );
    }
    for request in ["DataGetRequest", "DataListRequest", "DataWatchRequest"] {
        assert_eq!(
            field(request, "common_data_categories")
                .type_name
                .as_deref(),
            Some(".gamenode.plugin.v1.CommonDataSelector")
        );
    }

    for (field_name, number) in [
        ("schema_id", 1),
        ("schema_version", 2),
        ("schema_digest", 3),
        ("data", 4),
    ] {
        assert_eq!(field("ExtensionPayload", field_name).number, Some(number));
    }
    assert_eq!(
        field("ExtensionPayload", "data").type_name.as_deref(),
        Some(".google.protobuf.Struct")
    );
    for expected in [
        "namespace",
        "schema_id",
        "schema_version",
        "schema_digest",
        "schema_json",
        "fields",
    ] {
        assert!(
            message("ExtensionSchemaDescriptor")
                .field
                .iter()
                .any(|candidate| candidate.name.as_deref() == Some(expected)),
            "versioned namespaced schema field {expected}"
        );
    }
    for expected in ["schema_id", "schema_version", "value"] {
        assert!(
            message("DataPublishRequest")
                .field
                .iter()
                .any(|candidate| candidate.name.as_deref() == Some(expected)),
            "typed custom-data envelope field {expected}"
        );
    }
    assert_eq!(
        field("DataPublishRequest", "value").type_name.as_deref(),
        Some(".google.protobuf.Struct")
    );
    assert_eq!(
        field("StoragePutRequest", "value").r#type,
        Some(prost_types::field_descriptor_proto::Type::Bytes as i32)
    );
    assert_eq!(
        field("StorageGetReply", "value").r#type,
        Some(prost_types::field_descriptor_proto::Type::Bytes as i32)
    );

    for (message_name, expected_fields) in [
        ("RuntimeRecord", &["id", "revision", "extensions"][..]),
        (
            "PerformanceSample",
            &[
                "observed_at",
                "frames_per_second",
                "cpu_percent",
                "memory_bytes",
                "sequence",
            ][..],
        ),
        (
            "HardwareReportRequest",
            &["schema_id", "schema_version", "observations"][..],
        ),
    ] {
        for expected in expected_fields {
            assert!(
                message(message_name)
                    .field
                    .iter()
                    .any(|field| field.name.as_deref() == Some(*expected)),
                "typed base field {message_name}.{expected}"
            );
        }
    }
    assert_eq!(
        field("HardwareReportRequest", "observations")
            .type_name
            .as_deref(),
        Some(".google.protobuf.Struct")
    );
}

#[test]
fn data_durability_is_additive_on_the_publish_reply_and_record() {
    let descriptor = FileDescriptorSet::decode(DESCRIPTOR).unwrap();
    let message = |name: &str| {
        descriptor
            .file
            .iter()
            .flat_map(|file| &file.message_type)
            .find(|message| message.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("{name} descriptor"))
    };
    let field = |name: &str, field_name: &str| {
        message(name)
            .field
            .iter()
            .find(|field| field.name.as_deref() == Some(field_name))
            .unwrap_or_else(|| panic!("{name}.{field_name} descriptor"))
    };

    for (name, number) in [("DataMutationReply", 5), ("DataRecord", 16)] {
        let durability = field(name, "durability");
        assert_eq!(
            durability.number,
            Some(number),
            "{name}.durability keeps its additive number"
        );
        assert_eq!(
            durability.type_name.as_deref(),
            Some(".gamenode.plugin.v1.StorageDurability"),
            "{name}.durability carries the shared mode enum"
        );
    }

    // A reader that predates the field sees no mode; a current reader sees the actual one.
    let absent = DataMutationReply::decode([].as_slice()).unwrap();
    assert_eq!(
        absent.durability,
        StorageDurability::Unspecified as i32,
        "an old or absent value decodes as unspecified"
    );
    let durable = DataMutationReply {
        durability: StorageDurability::Persistent as i32,
        ..Default::default()
    };
    let restored = DataMutationReply::decode(durable.encode_to_vec().as_slice()).unwrap();
    assert_eq!(restored.durability, StorageDurability::Persistent as i32);
    let record = DataRecord {
        durability: StorageDurability::SessionOnly as i32,
        ..Default::default()
    };
    let restored = DataRecord::decode(record.encode_to_vec().as_slice()).unwrap();
    assert_eq!(restored.durability, StorageDurability::SessionOnly as i32);
}
