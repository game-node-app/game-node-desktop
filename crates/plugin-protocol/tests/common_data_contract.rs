use gamenode_plugin_protocol::{
    common_data,
    prost::Message,
    prost_types::FileDescriptorSet,
    proto::{CommonDataCategory, CommonDataSelector},
};
use serde_json::Value;

const DESCRIPTOR: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/plugin_descriptor.bin"));
const CONTRACT: &str = include_str!("../contracts/common_data_selector.json");

#[test]
fn common_data_fixture_matches_the_generated_wire_contract() {
    let contract: Value = serde_json::from_str(CONTRACT).unwrap();
    assert_eq!(contract["api"], "1.4");
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
        };
        let selected = if query["selector"] == "omitted" {
            common_data::categories(None).unwrap()
        } else {
            common_data::categories(Some(&selector)).unwrap()
        };
        let grants = query["grants"].as_array().unwrap();
        let authorized = selected.iter().all(|category| {
            let required = common_data::permission(*category).unwrap();
            grants.iter().any(|grant| grant.as_str() == Some(required))
        });
        let expected = query["expected"].as_str().unwrap();
        let actual = if selected.is_empty() {
            "PRIVATE_ONLY"
        } else if authorized {
            "COMMON_INCLUDED"
        } else {
            "PERMISSION_DENIED"
        };
        assert_eq!(actual, expected, "query fixture {id}");
    }
    assert_eq!(contract["excluded_data"].as_array().unwrap().len(), 3);
}
