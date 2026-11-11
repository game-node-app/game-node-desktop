use gamenode_plugin_protocol::{common_data, proto::*};

#[test]
fn absent_and_empty_selectors_remain_private_only() {
    assert_eq!(
        common_data::categories(None).unwrap(),
        Vec::<CommonDataCategory>::new()
    );
    assert_eq!(
        common_data::categories(Some(&CommonDataSelector::default())).unwrap(),
        Vec::<CommonDataCategory>::new()
    );
}

#[test]
fn explicit_categories_are_typed_and_map_to_independent_grants() {
    let selector = CommonDataSelector {
        categories: vec![
            CommonDataCategory::GamePerformanceHistory as i32,
            CommonDataCategory::ContentAvailability as i32,
        ],
    };
    assert_eq!(
        common_data::categories(Some(&selector)).unwrap(),
        vec![
            CommonDataCategory::GamePerformanceHistory,
            CommonDataCategory::ContentAvailability,
        ]
    );
    assert_eq!(
        common_data::permission(CommonDataCategory::GamePerformanceHistory),
        Some("common.game_performance_history.read")
    );
    assert_eq!(
        common_data::permission(CommonDataCategory::ContentAvailability),
        Some("common.content_availability.read")
    );
}

#[test]
fn invalid_or_ambiguous_category_values_are_rejected() {
    for raw in [CommonDataCategory::Unspecified as i32, 99] {
        assert!(
            common_data::categories(Some(&CommonDataSelector {
                categories: vec![raw],
            }))
            .is_err()
        );
    }
    assert_eq!(
        common_data::categories(Some(&CommonDataSelector {
            categories: vec![
                CommonDataCategory::GamePaths as i32,
                CommonDataCategory::GamePaths as i32,
            ],
        })),
        Err(common_data::SelectorError::Duplicate)
    );
}
