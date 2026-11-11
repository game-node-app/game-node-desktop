use gamenode_plugin_protocol::prost::Message;
use gamenode_plugin_protocol::prost_types::FileDescriptorSet;
use serde_json::Value;

const DESCRIPTOR: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/plugin_descriptor.bin"));
const CATALOG: &str = include_str!("../contracts/resource_host.json");

#[test]
fn checked_resource_catalog_matches_generated_protobuf_service() {
    let catalog: Value = serde_json::from_str(CATALOG).unwrap();
    assert_eq!(catalog["api"], "1.4");
    assert_eq!(catalog["service"], "ResourceHost");
    assert_eq!(catalog["unknown_route"], "UNIMPLEMENTED");

    let descriptors = FileDescriptorSet::decode(DESCRIPTOR).unwrap();
    let service = descriptors
        .file
        .iter()
        .flat_map(|file| &file.service)
        .find(|service| service.name.as_deref() == Some("ResourceHost"))
        .expect("generated ResourceHost descriptor");
    let mut described: Vec<_> = service
        .method
        .iter()
        .map(|method| {
            (
                method.name.as_deref().unwrap().to_owned(),
                method
                    .input_type
                    .as_deref()
                    .unwrap()
                    .replace(".gamenode.plugin.v1.", ""),
                method
                    .output_type
                    .as_deref()
                    .unwrap()
                    .replace(".gamenode.plugin.v1.", ""),
                method.server_streaming.unwrap_or(false),
            )
        })
        .collect();
    let mut catalogued: Vec<_> = catalog["routes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|route| {
            (
                route["method"].as_str().unwrap().to_owned(),
                route["request"].as_str().unwrap().to_owned(),
                route["response"].as_str().unwrap().to_owned(),
                route["streaming"].as_bool().unwrap(),
            )
        })
        .collect();
    described.sort();
    catalogued.sort();
    assert_eq!(
        catalogued, described,
        "catalog and generated wire routes diverged"
    );

    for route in catalog["routes"].as_array().unwrap() {
        assert!(!route["permission"].as_str().unwrap().is_empty());
        assert!(!route["owner"].as_str().unwrap().is_empty());
        assert!(!route["semantics"].as_array().unwrap().is_empty());
    }
}

#[test]
fn ordinary_and_custom_launch_are_distinct_wire_contracts() {
    let catalog: Value = serde_json::from_str(CATALOG).unwrap();
    let route = |name: &str| {
        catalog["routes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|route| route["method"] == name)
            .unwrap()
    };
    let ordinary = route("GamesRequestLaunch");
    assert_eq!(ordinary["request"], "GameLaunchRequest");
    assert_eq!(ordinary["permission"], "game.launch");
    let custom = route("GamesRequestCustomLaunch");
    assert_eq!(custom["request"], "CustomLaunchRequest");
    assert_eq!(custom["permission"], "game.custom_launch");
    assert!(
        custom["semantics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item.as_str().unwrap().contains("never aliases game.launch"))
    );
}
