//! Typed per-query selection of published data projections.

use std::collections::BTreeSet;

use crate::proto::{CommonDataCategory, CommonDataSelector};

/// The part of a query selector that the host has independently authorized.
/// Empty selections intentionally mean that no shared records are visible.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AuthorizedSelection {
    categories: BTreeSet<CommonDataCategory>,
    record_ids: BTreeSet<String>,
}

impl AuthorizedSelection {
    pub fn includes_category(&self, category: CommonDataCategory) -> bool {
        self.categories.contains(&category)
    }

    pub fn includes_record(&self, record_id: &str) -> bool {
        self.record_ids.contains(record_id)
    }
}

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

/// Resolves per-query selection against two separate current grants. Category
/// grants never authorize a selected record, and record grants never opt a
/// whole category into the query. Missing selectors therefore stay private.
pub fn authorize_selector(
    selector: Option<&CommonDataSelector>,
    granted_categories: &BTreeSet<CommonDataCategory>,
    granted_record_ids: &BTreeSet<String>,
) -> Result<AuthorizedSelection, SelectorError> {
    let categories = categories(selector)?
        .into_iter()
        .filter(|category| granted_categories.contains(category))
        .collect();
    let record_ids = selected_record_ids(selector)?
        .into_iter()
        .filter(|record_id| granted_record_ids.contains(*record_id))
        .map(str::to_owned)
        .collect();
    Ok(AuthorizedSelection {
        categories,
        record_ids,
    })
}

/// Resolves a selector from the permission identifiers in an authenticated
/// session. Selected record grants remain a separate input and are never
/// inferred from a category permission.
pub fn authorize_grant_selector(
    selector: Option<&CommonDataSelector>,
    permission_grants: &BTreeSet<String>,
    granted_record_ids: &BTreeSet<String>,
) -> Result<AuthorizedSelection, SelectorError> {
    let granted_categories = [
        CommonDataCategory::GamePaths,
        CommonDataCategory::GamePerformanceHistory,
        CommonDataCategory::RuntimeCatalog,
        CommonDataCategory::ContentAvailability,
        CommonDataCategory::LibraryRoots,
        CommonDataCategory::SaveProfiles,
    ]
    .into_iter()
    .filter(|category| permission(*category).is_some_and(|id| permission_grants.contains(id)))
    .collect();
    authorize_selector(selector, &granted_categories, granted_record_ids)
}

/// Permission required by each stable common-data category.
pub fn permission(category: CommonDataCategory) -> Option<&'static str> {
    match category {
        CommonDataCategory::GamePaths => Some(crate::capability::COMMON_GAME_PATHS_READ),
        CommonDataCategory::GamePerformanceHistory => {
            Some(crate::capability::COMMON_GAME_PERFORMANCE_HISTORY_READ)
        }
        CommonDataCategory::RuntimeCatalog => Some(crate::capability::COMMON_RUNTIME_CATALOG_READ),
        CommonDataCategory::ContentAvailability => {
            Some(crate::capability::COMMON_CONTENT_AVAILABILITY_READ)
        }
        CommonDataCategory::LibraryRoots => Some("library.roots.read"),
        CommonDataCategory::SaveProfiles => Some("game.saves.read"),
        CommonDataCategory::Unspecified => None,
    }
}
