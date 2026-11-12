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
        ..Default::default()
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
    assert_eq!(
        common_data::permission(CommonDataCategory::LibraryRoots),
        Some("library.roots.read")
    );
    assert_eq!(
        common_data::permission(CommonDataCategory::SaveProfiles),
        Some("game.saves.read")
    );
}

#[test]
fn invalid_or_ambiguous_category_values_are_rejected() {
    for raw in [CommonDataCategory::Unspecified as i32, 99] {
        assert!(
            common_data::categories(Some(&CommonDataSelector {
                categories: vec![raw],
                ..Default::default()
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
            ..Default::default()
        })),
        Err(common_data::SelectorError::Duplicate)
    );
}

#[test]
fn selected_record_ids_are_explicit_independent_query_selectors() {
    let selector = CommonDataSelector {
        selected_record_ids: vec!["runtime:one".to_owned(), "save:two".to_owned()],
        ..Default::default()
    };
    assert_eq!(
        common_data::selected_record_ids(Some(&selector)).unwrap(),
        vec!["runtime:one", "save:two"]
    );
    assert_eq!(
        common_data::selected_record_ids(None).unwrap(),
        Vec::<&str>::new()
    );
    assert_eq!(
        common_data::selected_record_ids(Some(&CommonDataSelector {
            selected_record_ids: vec!["runtime:one".to_owned(), "runtime:one".to_owned()],
            ..Default::default()
        })),
        Err(common_data::SelectorError::DuplicateRecordId)
    );
    assert_eq!(
        common_data::selected_record_ids(Some(&CommonDataSelector {
            selected_record_ids: vec![" ".to_owned()],
            ..Default::default()
        })),
        Err(common_data::SelectorError::EmptyRecordId)
    );
}
