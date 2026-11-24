use gamenode_plugin_protocol::proto::CommonDataCategory;
use serde_json::Value;

const CONTRACT: &str = include_str!("../../plugin-protocol/contracts/common_data_selector.json");

fn category(name: &str) -> CommonDataCategory {
    match name {
        "GAME_PATHS" => CommonDataCategory::GamePaths,
        "GAME_PERFORMANCE_HISTORY" => CommonDataCategory::GamePerformanceHistory,
        "RUNTIME_CATALOG" => CommonDataCategory::RuntimeCatalog,
        "CONTENT_AVAILABILITY" => CommonDataCategory::ContentAvailability,
        _ => panic!("unrecognized common data category in shared contract: {name}"),
    }
}

#[test]
fn sdk_selector_matches_shared_contract_for_private_and_common_queries() {
    let contract: Value = serde_json::from_str(CONTRACT).unwrap();
    let categories = contract["categories"].as_array().unwrap();
    for query in contract["queries"].as_array().unwrap() {
        let names = query["categories"].as_array().unwrap();
        let selector = gamenode_plugin_sdk::common_data_selector(
            names.iter().map(|name| category(name.as_str().unwrap())),
        )
        .unwrap();
        let expected = names
            .iter()
            .map(|name| {
                let name = name.as_str().unwrap();
                let entry = categories
                    .iter()
                    .find(|entry| entry["name"] == name)
                    .unwrap();
                entry["number"].as_i64().unwrap() as i32
            })
            .collect::<Vec<_>>();
        assert_eq!(selector.categories, expected, "query {}", query["id"]);
        if names.is_empty() {
            assert!(
                selector.categories.is_empty(),
                "empty selector is private-only"
            );
        }
    }
}
