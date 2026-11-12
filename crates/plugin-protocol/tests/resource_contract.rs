use gamenode_plugin_protocol::prost::Message;
use gamenode_plugin_protocol::prost_types::FileDescriptorSet;
use serde_json::Value;

const DESCRIPTOR: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/plugin_descriptor.bin"));
const CATALOG: &str = include_str!("../contracts/resource_host.json");
const DOMAIN_CATALOG: &str = include_str!("../contracts/domain_host.json");
const FIELD_POLICY: &str = include_str!("../contracts/endpoint_field_policy.json");

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
        let contract = &route["contract"];
        assert!(!contract["operation_owner"].as_str().unwrap().is_empty());
        assert!(!contract["resource_owner"].as_str().unwrap().is_empty());
        assert!(contract["revision"].is_object());
        assert!(contract["idempotency"]["required"].is_boolean());
        assert!(!contract["quota"].as_str().unwrap().is_empty());
        assert!(!contract["commit_authorization"].as_str().unwrap().is_empty());
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

#[test]
fn resource_routes_use_typed_operation_messages() {
    let catalog: Value = serde_json::from_str(CATALOG).unwrap();
    for route in catalog["routes"].as_array().unwrap() {
        assert_ne!(route["request"], "ResourceRequest", "{}", route["method"]);
        assert_ne!(route["response"], "ResourceReply", "{}", route["method"]);
        assert!(!route["permission"].as_str().unwrap().is_empty());
        assert!(!route["owner"].as_str().unwrap().is_empty());
    }
    let ordinary = catalog["routes"].as_array().unwrap().iter()
        .find(|route| route["method"] == "GamesRequestLaunch").unwrap();
    assert_eq!(ordinary["request"], "GameLaunchRequest");
    let custom = catalog["routes"].as_array().unwrap().iter()
        .find(|route| route["method"] == "GamesRequestCustomLaunch").unwrap();
    assert_eq!(custom["request"], "CustomLaunchRequest");
    assert_eq!(custom["permission"], "game.custom_launch");
}

#[test]
fn endpoint_field_policy_covers_all_current_services_and_marks_future_routes() {
    let fixture: Value = serde_json::from_str(FIELD_POLICY).unwrap();
    let routes = fixture["routes"].as_array().unwrap();
        assert_eq!(routes.len(), 106);
    for route in routes {
        assert!(!route["request_policy"].as_str().unwrap().is_empty());
        assert!(!route["response_policy"].as_str().unwrap().is_empty());
        assert!(!route["extension_or_opaque_policy"].as_str().unwrap().is_empty());
        assert!(!route["access_model"].as_str().unwrap().is_empty());
        match route["status"].as_str().unwrap() {
            "protobuf_contract" => {
                assert!(!route["wire_service"].as_str().unwrap().is_empty());
                assert!(!route["wire_method"].as_str().unwrap().is_empty());
                assert!(!route["request_type"].as_str().unwrap().is_empty());
                assert!(!route["response_type"].as_str().unwrap().is_empty());
            }
            "planned" => panic!("route remains planned: {}", route["route"]),
            state => panic!("unexpected route state: {state}"),
        }
    }

    let descriptors = FileDescriptorSet::decode(DESCRIPTOR).unwrap();
    let mut actual = Vec::new();
    for file in &descriptors.file {
        for service in &file.service {
            let Some(service_name) = service.name.as_deref() else { continue };
            if !matches!(service_name, "PluginHost" | "ProviderHost" | "ResourceHost" | "Saves" | "Locations" | "StorageDiagnostics" | "Computer" | "GamePresence" | "GamePerformance") { continue }
            for method in &service.method {
                actual.push((service_name.to_owned(), method.name.clone().unwrap()));
            }
        }
    }
    let mut mapped: Vec<_> = routes.iter()
        .filter(|route| route["status"] == "protobuf_contract")
        .map(|route| (
            route["wire_service"].as_str().unwrap().to_owned(),
            route["wire_method"].as_str().unwrap().to_owned(),
        ))
        .collect();
    actual.sort(); mapped.sort();
    assert_eq!(mapped, actual, "field policy must cover each current service route exactly once");

    let exchange = &fixture["exchange_event_families"];
    assert!(exchange.as_array().unwrap().iter().any(|item| item["family"] == "CatalogChunk"));
    assert!(exchange.as_array().unwrap().iter().any(|item| item["family"] == "runtime session event/result"));

    let runtime_sessions = exchange.as_array().unwrap().iter()
        .find(|item| item["family"] == "runtime session event/result").unwrap();
    assert_eq!(runtime_sessions["event_message"], "RuntimeSessionReport");
    assert_eq!(runtime_sessions["result_message"], "RuntimeSessionResult");
    assert_eq!(runtime_sessions["permission"], "game.sessions.report; approved game/installation/runtime scope");

    let message_field = |message_name: &str, field_name: &str| {
        descriptors.file.iter().flat_map(|file| &file.message_type)
            .find(|message| message.name.as_deref() == Some(message_name))
            .unwrap().field.iter().find(|field| field.name.as_deref() == Some(field_name)).unwrap().type_name.clone().unwrap()
    };
    assert_eq!(message_field("ProviderEvent", "runtime_session"), ".gamenode.plugin.v1.RuntimeSessionReport");
    assert_eq!(message_field("ProviderCommand", "runtime_session_result"), ".gamenode.plugin.v1.RuntimeSessionResult");
}

#[test]
fn domain_catalog_matches_generated_contracts_and_operation_grants() {
    let catalog: Value = serde_json::from_str(DOMAIN_CATALOG).unwrap();
    assert_eq!(catalog["api"], "1.4");
    let routes = catalog["routes"].as_array().unwrap();
    assert_eq!(routes.len(), 26);
    assert!(routes.iter().all(|route| !route["permission"].as_str().unwrap().is_empty()));

    let descriptors = FileDescriptorSet::decode(DESCRIPTOR).unwrap();
    let mut actual = Vec::new();
    for file in &descriptors.file {
        for service in &file.service {
            let Some(service_name) = service.name.as_deref() else { continue };
            if !matches!(service_name, "Saves" | "Locations" | "StorageDiagnostics" | "Computer" | "GamePresence" | "GamePerformance") { continue }
            for method in &service.method {
                actual.push((
                    service_name.to_owned(),
                    method.name.clone().unwrap(),
                    method.input_type.as_deref().unwrap().replace(".gamenode.plugin.v1.", ""),
                    method.output_type.as_deref().unwrap().replace(".gamenode.plugin.v1.", ""),
                    method.server_streaming.unwrap_or(false),
                ));
            }
        }
    }
    let mut described: Vec<_> = routes.iter().map(|route| (
        route["service"].as_str().unwrap().to_owned(),
        route["method"].as_str().unwrap().to_owned(),
        route["request"].as_str().unwrap().to_owned(),
        route["response"].as_str().unwrap().to_owned(),
        route["streaming"].as_bool().unwrap(),
    )).collect();
    actual.sort();
    described.sort();
    assert_eq!(described, actual, "domain catalog and protobuf descriptors diverged");

    let permission = |method: &str| routes.iter().find(|route| route["method"] == method).unwrap()["permission"].as_str().unwrap();
    assert_eq!(permission("CreateProfile"), "game.saves.create");
    assert_eq!(permission("UpdateProfile"), "game.saves.update");
    assert_eq!(permission("RemoveProfile"), "game.saves.delete");

    let proto = include_str!("../proto/plugin.proto");
    assert!(proto.contains("repeated string selected_record_ids = 2"));
    assert!(proto.contains("CommonDataSelector common_data_categories = 4;"), "Data.List must carry the same per-query selector");
}
