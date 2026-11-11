//! Typed per-query selection of common, non-secret data projections.

use crate::proto::{CommonDataCategory, CommonDataSelector};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SelectorError {
    #[error("common-data selector contains an unspecified category")]
    Unspecified,
    #[error("common-data selector contains an unsupported category")]
    Unsupported,
    #[error("common-data selector contains a duplicate category")]
    Duplicate,
}

/// Validate and decode a selector. No selector and an empty selector both
/// mean private/owner-visible data only. Authorization is always checked by
/// the host against the caller's current grant for every decoded category.
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

/// Permission required by each stable common-data category.
pub fn permission(category: CommonDataCategory) -> Option<&'static str> {
    match category {
        CommonDataCategory::GamePaths => Some("common.game_paths.read"),
        CommonDataCategory::GamePerformanceHistory => Some("common.game_performance_history.read"),
        CommonDataCategory::RuntimeCatalog => Some("common.runtime_catalog.read"),
        CommonDataCategory::ContentAvailability => Some("common.content_availability.read"),
        CommonDataCategory::Unspecified => None,
    }
}
