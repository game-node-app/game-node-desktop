//! Typed per-query selection of published data projections.

use crate::proto::{CommonDataCategory, CommonDataSelector};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SelectorError {
    #[error("common-data selector contains an unspecified category")]
    Unspecified,
    #[error("common-data selector contains an unsupported category")]
    Unsupported,
    #[error("common-data selector contains a duplicate category")]
    Duplicate,
    #[error("common-data selector contains an empty selected record id")]
    EmptyRecordId,
    #[error("common-data selector contains a duplicate selected record id")]
    DuplicateRecordId,
    #[error("common-data selector exceeds the selected-record limit")]
    TooManyRecordIds,
}

/// Validate and decode category selectors. No selector and an empty selector
/// both mean private/owner-visible data only. Authorization is always checked
/// by the host against the caller's current grant for every decoded category.
pub fn categories(
    selector: Option<&CommonDataSelector>,
) -> Result<Vec<CommonDataCategory>, SelectorError> {
    let Some(selector) = selector else {
        return Ok(Vec::new());
    };

    let mut categories = Vec::with_capacity(selector.categories.len());
    for raw in &selector.categories {
        let category =
            CommonDataCategory::try_from(*raw).map_err(|_| SelectorError::Unsupported)?;
        if category == CommonDataCategory::Unspecified {
            return Err(SelectorError::Unspecified);
        }
        if categories.contains(&category) {
            return Err(SelectorError::Duplicate);
        }
        categories.push(category);
    }
    Ok(categories)
}

/// Validate selected published record ids. These ids are only query selectors;
/// the host must independently check a user grant for every returned record.
pub fn selected_record_ids(
    selector: Option<&CommonDataSelector>,
) -> Result<Vec<&str>, SelectorError> {
    let Some(selector) = selector else {
        return Ok(Vec::new());
    };
    if selector.selected_record_ids.len() > 1000 {
        return Err(SelectorError::TooManyRecordIds);
    }
    let mut ids = Vec::with_capacity(selector.selected_record_ids.len());
    for id in &selector.selected_record_ids {
        if id.trim().is_empty() {
            return Err(SelectorError::EmptyRecordId);
        }
        if ids.contains(&id.as_str()) {
            return Err(SelectorError::DuplicateRecordId);
        }
        ids.push(id.as_str());
    }
    Ok(ids)
}

/// Permission required by each stable common-data category.
pub fn permission(category: CommonDataCategory) -> Option<&'static str> {
    match category {
        CommonDataCategory::GamePaths => Some("common.game_paths.read"),
        CommonDataCategory::GamePerformanceHistory => Some("common.game_performance_history.read"),
        CommonDataCategory::RuntimeCatalog => Some("common.runtime_catalog.read"),
        CommonDataCategory::ContentAvailability => Some("common.content_availability.read"),
        CommonDataCategory::LibraryRoots => Some("library.roots.read"),
        CommonDataCategory::SaveProfiles => Some("game.saves.read"),
        CommonDataCategory::Unspecified => None,
    }
}
