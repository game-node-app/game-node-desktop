//! Achievement cache rules: ordering, hidden masking, staleness and the storage port.
//!
//! The data is keyed by provider and connected store account, independently of any GameNode
//! account. Core keeps no file, database or network access here.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ports::StorageError;

/// How fresh a game's achievement data must be for each trigger.
pub const SELECTED_STALE_SECS: i64 = 60 * 60;
/// A failed read that never succeeded, or failed transiently, is retried after this cool-down.
pub const FAILED_RETRY_SECS: i64 = 60 * 60;
pub const RECONCILE_READY_SECS: i64 = 7 * 24 * 60 * 60;
pub const RECONCILE_NONE_SECS: i64 = 30 * 24 * 60 * 60;
pub const GAME_EXIT_SETTLING_SECS: i64 = 30;

/// Stored state of one game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AchievementGameState {
    Ready,
    None,
    Failed,
}

impl AchievementGameState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::None => "none",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ready" => Some(Self::Ready),
            "none" => Some(Self::None),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

/// The filter of the full achievement list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AchievementFilter {
    All,
    Unlocked,
    Locked,
}

/// How the full achievement list is ordered. Sorting runs in core on the storage worker.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AchievementSort {
    /// Unlocked newest first, then unlocked without a time, then locked. The default order.
    #[default]
    MostRecent,
    /// Lowest global percentage first; rows without rarity last.
    Rarest,
    /// Highest global percentage first; rows without rarity last.
    MostCommon,
    /// Alphabetical by displayed label; rows without rarity are unaffected.
    Name,
}

/// The segment counts of the full list. They ignore the active filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AchievementCounts {
    pub all: usize,
    pub unlocked: usize,
    pub locked: usize,
}

impl AchievementCounts {
    pub fn from_records(achievements: &[AchievementRecord]) -> Self {
        let unlocked = achievements.iter().filter(|row| row.unlocked).count();
        Self {
            all: achievements.len(),
            unlocked,
            locked: achievements.len() - unlocked,
        }
    }
}

/// One achievement definition joined with the connected account's unlock state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AchievementRecord {
    pub id: String,
    pub name: String,
    pub description: String,
    pub hidden: bool,
    pub icon_url: String,
    pub icon_gray_url: String,
    pub unlocked: bool,
    pub unlocked_at: Option<i64>,
    /// Global unlock percentage in hundredths of a percent (420 = 4.2%), when the provider
    /// publishes rarity.
    #[serde(default)]
    pub global_percent: Option<u16>,
    /// The provider's schema order, used as the stable order among locked achievements.
    pub position: i64,
}

/// One game's cached achievement data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredAchievementGame {
    pub entry_id: String,
    pub state: AchievementGameState,
    pub language: String,
    pub last_success_at: Option<i64>,
    pub last_attempt_at: i64,
    pub failure: Option<crate::library::CatalogError>,
    pub achievements: Vec<AchievementRecord>,
}

/// The compact summary loaded for every game at startup and kept in memory by the engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredAchievementSummary {
    pub entry_id: String,
    pub state: AchievementGameState,
    pub language: String,
    pub last_success_at: Option<i64>,
    pub last_attempt_at: i64,
    pub failure: Option<crate::library::CatalogError>,
    pub unlocked: usize,
    pub total: usize,
    pub latest_unlock: Option<AchievementUnlock>,
}

/// The newest unlock shown in the detail pane.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AchievementUnlock {
    pub id: String,
    pub name: String,
    pub unlocked_at: Option<i64>,
}

/// The detail pane's compact achievement state. The full list never travels in a snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AchievementSummaryView {
    pub state: AchievementGameState,
    pub unlocked: usize,
    pub total: usize,
    pub latest_unlock: Option<AchievementUnlock>,
    pub last_success_at: Option<i64>,
    pub failure: Option<crate::library::CatalogError>,
}

/// The async command's response for the full list. It reads only the local cache.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AchievementListView {
    /// Whether a store account is currently connected for this provider.
    pub connected: bool,
    /// `None` means the game has never been read.
    pub state: Option<AchievementGameState>,
    pub last_success_at: Option<i64>,
    /// The successful read the footer's relative time is based on.
    pub refreshed_at: Option<i64>,
    pub failure: Option<crate::library::CatalogError>,
    /// Counts for the all/unlocked/locked segments, independent of the active filter.
    pub counts: AchievementCounts,
    /// Whether any stored achievement of this game has a global percentage.
    pub has_rarity: bool,
    /// The provider's display name, for provider-named copy such as the footer.
    pub provider_name: String,
    /// Whether the provider publishes a public achievements page for the game.
    pub has_provider_page: bool,
    /// Ordered and hidden-masked before serialization.
    pub achievements: Vec<AchievementRecord>,
}

impl From<&StoredAchievementSummary> for AchievementSummaryView {
    fn from(summary: &StoredAchievementSummary) -> Self {
        Self {
            state: summary.state,
            unlocked: summary.unlocked,
            total: summary.total,
            latest_unlock: summary.latest_unlock.clone(),
            last_success_at: summary.last_success_at,
            failure: summary.failure,
        }
    }
}

/// The trigger that asks whether a game should be refreshed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshTrigger {
    Selected,
    GameExit,
    Import,
    Reconcile,
}

/// Whether `state` is stale for `trigger` at `now`. `language` is the current UI locale; a stored
/// language that differs makes a selected game stale even inside the one-hour window.
pub fn is_stale(
    state: Option<&StoredAchievementSummary>,
    trigger: RefreshTrigger,
    now: i64,
    language: &str,
) -> bool {
    let age = state.and_then(|state| state.last_success_at.map(|at| now.saturating_sub(at)));
    match trigger {
        RefreshTrigger::Selected => match state {
            None => true,
            Some(state) if state.state == AchievementGameState::Failed => {
                state.language != language
                    || now.saturating_sub(state.last_attempt_at) >= SELECTED_STALE_SECS
            }
            Some(state) => {
                age.is_none_or(|age| age >= SELECTED_STALE_SECS) || state.language != language
            }
        },
        RefreshTrigger::GameExit => true,
        RefreshTrigger::Import => match state {
            None => true,
            Some(state) => {
                short_failed_retry(state)
                    && now.saturating_sub(state.last_attempt_at) >= FAILED_RETRY_SECS
            }
        },
        RefreshTrigger::Reconcile => match state {
            None => false,
            Some(state) => match state.state {
                AchievementGameState::Ready => age.is_none_or(|age| age >= RECONCILE_READY_SECS),
                AchievementGameState::None => age.is_none_or(|age| age >= RECONCILE_NONE_SECS),
                AchievementGameState::Failed => {
                    let age = now.saturating_sub(state.last_attempt_at);
                    if short_failed_retry(state) {
                        age >= FAILED_RETRY_SECS
                    } else {
                        age >= RECONCILE_READY_SECS
                    }
                }
            },
        },
    }
}

/// A failure worth retrying after the short cool-down: a game that has never succeeded and did
/// not fail for a privacy or expiry reason, or a transient category. Expiry already stops the
/// worker; privacy keeps the previous slower reconciliation rule.
fn short_failed_retry(state: &StoredAchievementSummary) -> bool {
    if state.state != AchievementGameState::Failed {
        return false;
    }
    let transient = matches!(
        state.failure,
        Some(
            crate::library::CatalogError::RateLimited
                | crate::library::CatalogError::Unreachable
                | crate::library::CatalogError::Server
        )
    );
    transient
        || (state.last_success_at.is_none()
            && !matches!(
                state.failure,
                Some(crate::library::CatalogError::Privacy | crate::library::CatalogError::Expired)
            ))
}

/// The label used only for ordering a locked hidden achievement. Its real name never leaves the
/// store, and the UI renders its own translated generic label; a stable placeholder keeps the
/// order independent of the hidden text.
const HIDDEN_SORT_LABEL: &str = "Hidden achievement";

/// The list order for the chosen sort, applied inside the filter. `MostRecent` puts unlocked
/// achievements first (newest known unlock first, then unlocked entries without a time in schema
/// order, then locked entries in schema order). The rarity sorts mix unlocked and locked and put
/// rows without a percentage last; `Name` uses the displayed label, so a locked hidden achievement
/// sorts by its generic label.
pub fn ordered_achievements(
    achievements: &[AchievementRecord],
    filter: AchievementFilter,
    sort: AchievementSort,
) -> Vec<AchievementRecord> {
    let mut rows: Vec<AchievementRecord> = achievements
        .iter()
        .filter(|achievement| match filter {
            AchievementFilter::All => true,
            AchievementFilter::Unlocked => achievement.unlocked,
            AchievementFilter::Locked => !achievement.unlocked,
        })
        .cloned()
        .collect();
    match sort {
        AchievementSort::MostRecent => rows.sort_by_key(|achievement| {
            let group = match (achievement.unlocked, achievement.unlocked_at) {
                (true, Some(at)) => (0, std::cmp::Reverse(at)),
                (true, None) => (1, std::cmp::Reverse(0)),
                (false, _) => (2, std::cmp::Reverse(0)),
            };
            (group.0, group.1, achievement.position)
        }),
        AchievementSort::Rarest => rows.sort_by_key(|achievement| {
            // Missing rarity comes after every row that has one; position keeps ties stable.
            (
                achievement.global_percent.is_none(),
                achievement.global_percent,
                achievement.position,
            )
        }),
        AchievementSort::MostCommon => rows.sort_by_key(|achievement| {
            (
                achievement.global_percent.is_none(),
                std::cmp::Reverse(achievement.global_percent),
                achievement.position,
            )
        }),
        AchievementSort::Name => rows.sort_by(|left, right| {
            sort_label(left)
                .to_lowercase()
                .cmp(&sort_label(right).to_lowercase())
                .then(left.position.cmp(&right.position))
        }),
    }
    rows
}

fn sort_label(achievement: &AchievementRecord) -> &str {
    if achievement.hidden && !achievement.unlocked {
        HIDDEN_SORT_LABEL
    } else {
        &achievement.name
    }
}

/// Keeps the previous global percentage for achievements whose refresh returned none. A provider
/// that fails only the rarity request must not erase values it reported before.
pub fn keep_previous_rarity(
    achievements: &mut [AchievementRecord],
    previous: &BTreeMap<String, u16>,
) {
    for achievement in achievements {
        if achievement.global_percent.is_none() {
            achievement.global_percent = previous.get(&achievement.id).copied();
        }
    }
}

/// Removes a locked hidden achievement's store text before it can reach the WebView. The hidden
/// flag stays so the UI can render a translated generic label.
pub fn mask_hidden(achievements: &mut [AchievementRecord]) {
    for achievement in achievements {
        if achievement.hidden && !achievement.unlocked {
            achievement.name.clear();
            achievement.description.clear();
        }
    }
}

/// Builds the compact summary from a cached game.
pub fn summarize(game: &StoredAchievementGame) -> StoredAchievementSummary {
    let unlocked = game
        .achievements
        .iter()
        .filter(|achievement| achievement.unlocked)
        .count();
    let latest_unlock = game
        .achievements
        .iter()
        .filter(|achievement| achievement.unlocked)
        .max_by_key(|achievement| (achievement.unlocked_at, achievement.position))
        .map(|achievement| AchievementUnlock {
            id: achievement.id.clone(),
            name: achievement.name.clone(),
            unlocked_at: achievement.unlocked_at,
        });
    StoredAchievementSummary {
        entry_id: game.entry_id.clone(),
        state: game.state,
        language: game.language.clone(),
        last_success_at: game.last_success_at,
        last_attempt_at: game.last_attempt_at,
        failure: game.failure,
        unlocked,
        total: game.achievements.len(),
        latest_unlock,
    }
}

/// The achievement cache. All calls may block; run them on the storage worker.
pub trait AchievementStore: Send + Sync {
    /// One row per game ever read for `account_id`, with counts for the detail summary.
    fn load_summaries(
        &self,
        provider: &str,
        account_id: &str,
    ) -> Result<Vec<StoredAchievementSummary>, StorageError>;

    fn load_game(
        &self,
        provider: &str,
        account_id: &str,
        entry_id: &str,
    ) -> Result<Option<StoredAchievementGame>, StorageError>;

    /// Replaces the game's definitions and unlocks in one transaction.
    fn save_ready(
        &self,
        provider: &str,
        account_id: &str,
        entry_id: &str,
        language: &str,
        achievements: &[AchievementRecord],
        now: i64,
    ) -> Result<(), StorageError>;

    /// Records a successful read that found no achievements and removes stale definitions.
    fn save_none(
        &self,
        provider: &str,
        account_id: &str,
        entry_id: &str,
        language: &str,
        now: i64,
    ) -> Result<(), StorageError>;

    /// Records a failure without changing a previously successful game's data or state.
    fn save_failure(
        &self,
        provider: &str,
        account_id: &str,
        entry_id: &str,
        failure: crate::library::CatalogError,
        now: i64,
    ) -> Result<(), StorageError>;

    /// Deletes every achievement row of one store account.
    fn delete_account(&self, provider: &str, account_id: &str) -> Result<(), StorageError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn achievement(id: &str, unlocked: bool, at: Option<i64>, position: i64) -> AchievementRecord {
        AchievementRecord {
            id: id.into(),
            name: format!("Name {id}"),
            description: format!("Description {id}"),
            hidden: id == "secret",
            icon_url: String::new(),
            icon_gray_url: String::new(),
            unlocked,
            unlocked_at: at,
            global_percent: None,
            position,
        }
    }

    fn summary(
        state: AchievementGameState,
        success: Option<i64>,
        attempt: i64,
        lang: &str,
    ) -> StoredAchievementSummary {
        StoredAchievementSummary {
            entry_id: "620".into(),
            state,
            language: lang.into(),
            last_success_at: success,
            last_attempt_at: attempt,
            failure: None,
            unlocked: 0,
            total: 0,
            latest_unlock: None,
        }
    }

    #[test]
    fn list_puts_newest_unlocks_first_then_unlocks_without_a_time_then_locked() {
        let rows = vec![
            achievement("locked-a", false, None, 0),
            achievement("old", true, Some(100), 1),
            achievement("no-time", true, None, 2),
            achievement("new", true, Some(300), 3),
            achievement("locked-b", false, None, 4),
        ];
        let ordered =
            ordered_achievements(&rows, AchievementFilter::All, AchievementSort::MostRecent);
        assert_eq!(
            ordered
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            ["new", "old", "no-time", "locked-a", "locked-b"]
        );
        assert_eq!(
            ordered_achievements(
                &rows,
                AchievementFilter::Unlocked,
                AchievementSort::MostRecent,
            )
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
            ["new", "old", "no-time"]
        );
        assert_eq!(
            ordered_achievements(
                &rows,
                AchievementFilter::Locked,
                AchievementSort::MostRecent,
            )
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
            ["locked-a", "locked-b"]
        );
    }

    #[test]
    fn rarity_sorts_mix_unlocks_and_put_missing_values_last() {
        let mut rows = vec![
            achievement("no-rarity", false, None, 0),
            achievement("common", true, Some(100), 1),
            achievement("rare", false, None, 2),
            achievement("mid", true, Some(200), 3),
        ];
        rows[1].global_percent = Some(1_890);
        rows[2].global_percent = Some(420);
        rows[3].global_percent = Some(50);
        assert_eq!(
            ordered_achievements(&rows, AchievementFilter::All, AchievementSort::Rarest)
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            ["mid", "rare", "common", "no-rarity"]
        );
        assert_eq!(
            ordered_achievements(&rows, AchievementFilter::All, AchievementSort::MostCommon)
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            ["common", "rare", "mid", "no-rarity"]
        );
        assert_eq!(
            ordered_achievements(&rows, AchievementFilter::Unlocked, AchievementSort::Rarest)
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            ["mid", "common"],
            "the chosen sort applies inside the filter"
        );
    }

    #[test]
    fn name_sort_uses_the_generic_label_for_a_locked_hidden_achievement() {
        let rows = vec![
            achievement("alpha", false, None, 0),
            AchievementRecord {
                name: "Zzz secret".into(),
                position: 2,
                ..achievement("secret", false, None, 2)
            },
            achievement("zulu", true, Some(10), 1),
        ];
        let ordered = ordered_achievements(&rows, AchievementFilter::All, AchievementSort::Name);
        assert_eq!(
            ordered
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            ["secret", "alpha", "zulu"],
            "the hidden row sorts by its generic label, never by its real name"
        );
    }

    #[test]
    fn counts_cover_all_unlocked_and_locked_regardless_of_the_filter() {
        let rows = vec![
            achievement("a", true, Some(10), 0),
            achievement("b", false, None, 1),
            achievement("c", true, None, 2),
        ];
        assert_eq!(
            AchievementCounts::from_records(&rows),
            AchievementCounts {
                all: 3,
                unlocked: 2,
                locked: 1,
            }
        );
    }

    #[test]
    fn a_refresh_without_rarity_keeps_previous_values_by_id() {
        let mut rows = vec![
            achievement("kept", false, None, 0),
            achievement("new", false, None, 1),
            achievement("refreshed", false, None, 2),
        ];
        rows[2].global_percent = Some(1_000);
        let previous = std::collections::BTreeMap::from([
            ("kept".to_owned(), 420),
            ("refreshed".to_owned(), 50),
            ("removed".to_owned(), 900),
        ]);
        keep_previous_rarity(&mut rows, &previous);
        assert_eq!(rows[0].global_percent, Some(420));
        assert_eq!(rows[1].global_percent, None, "a new id has no history");
        assert_eq!(
            rows[2].global_percent,
            Some(1_000),
            "a reported value wins over the stored one"
        );
    }

    #[test]
    fn locked_hidden_achievements_keep_only_their_hidden_flag() {
        let mut rows = vec![
            achievement("secret", false, None, 0),
            AchievementRecord {
                hidden: true,
                ..achievement("revealed", true, Some(10), 1)
            },
        ];
        mask_hidden(&mut rows);
        assert_eq!(rows[0].name, "");
        assert_eq!(rows[0].description, "");
        assert!(rows[0].hidden);
        assert_eq!(rows[1].name, "Name revealed");
        assert_eq!(rows[1].description, "Description revealed");
    }

    #[test]
    fn selected_import_and_reconciliation_use_their_thresholds() {
        let now = 2_000_000_000;
        let fresh = summary(AchievementGameState::Ready, Some(now - 60), now - 60, "en");
        assert!(!is_stale(Some(&fresh), RefreshTrigger::Selected, now, "en"));
        assert!(is_stale(
            Some(&fresh),
            RefreshTrigger::Selected,
            now + SELECTED_STALE_SECS,
            "en"
        ));
        assert!(is_stale(
            Some(&fresh),
            RefreshTrigger::Selected,
            now,
            "pt-BR"
        ));
        assert!(!is_stale(Some(&fresh), RefreshTrigger::Import, now, "en"));
        assert!(is_stale(None, RefreshTrigger::Import, now, "en"));
        assert!(is_stale(Some(&fresh), RefreshTrigger::GameExit, now, "en"));

        let old_ready = summary(
            AchievementGameState::Ready,
            Some(now - RECONCILE_READY_SECS),
            now,
            "en",
        );
        assert!(is_stale(
            Some(&old_ready),
            RefreshTrigger::Reconcile,
            now,
            "en"
        ));
        let young_ready = summary(
            AchievementGameState::Ready,
            Some(now - RECONCILE_READY_SECS + 1),
            now,
            "en",
        );
        assert!(!is_stale(
            Some(&young_ready),
            RefreshTrigger::Reconcile,
            now,
            "en"
        ));

        let old_none = summary(
            AchievementGameState::None,
            Some(now - RECONCILE_NONE_SECS),
            now,
            "en",
        );
        assert!(is_stale(
            Some(&old_none),
            RefreshTrigger::Reconcile,
            now,
            "en"
        ));
        let young_none = summary(
            AchievementGameState::None,
            Some(now - RECONCILE_NONE_SECS + 1),
            now,
            "en",
        );
        assert!(!is_stale(
            Some(&young_none),
            RefreshTrigger::Reconcile,
            now,
            "en"
        ));
        assert!(!is_stale(None, RefreshTrigger::Reconcile, now, "en"));
    }

    #[test]
    fn a_just_failed_selected_game_waits_for_the_cool_down() {
        let now = 2_000_000_000;
        let failed = summary(AchievementGameState::Failed, None, now - 10, "en");
        let failed = StoredAchievementSummary {
            failure: Some(crate::library::CatalogError::Privacy),
            ..failed
        };
        assert!(!is_stale(
            Some(&failed),
            RefreshTrigger::Selected,
            now,
            "en"
        ));
        assert!(is_stale(
            Some(&failed),
            RefreshTrigger::Selected,
            now,
            "pt-BR"
        ));
        assert!(is_stale(
            Some(&failed),
            RefreshTrigger::Selected,
            now + SELECTED_STALE_SECS,
            "en"
        ));
    }

    #[test]
    fn transient_import_and_reconcile_failures_retry_after_one_hour() {
        let now = 2_000_000_000;
        for failure in [
            crate::library::CatalogError::RateLimited,
            crate::library::CatalogError::Unreachable,
            crate::library::CatalogError::Server,
        ] {
            let failed = StoredAchievementSummary {
                failure: Some(failure),
                ..summary(AchievementGameState::Failed, None, now - 10, "en")
            };
            assert!(!is_stale(Some(&failed), RefreshTrigger::Import, now, "en"));
            assert!(!is_stale(
                Some(&failed),
                RefreshTrigger::Reconcile,
                now,
                "en"
            ));
            assert!(is_stale(
                Some(&failed),
                RefreshTrigger::Import,
                now + FAILED_RETRY_SECS,
                "en"
            ));
            assert!(is_stale(
                Some(&failed),
                RefreshTrigger::Reconcile,
                now + FAILED_RETRY_SECS,
                "en"
            ));
        }

        // A game that never succeeded but failed with privacy keeps the slower rules.
        let privacy = StoredAchievementSummary {
            failure: Some(crate::library::CatalogError::Privacy),
            ..summary(AchievementGameState::Failed, None, now - 10, "en")
        };
        assert!(!is_stale(
            Some(&privacy),
            RefreshTrigger::Import,
            now + 2 * FAILED_RETRY_SECS,
            "en"
        ));
        assert!(!is_stale(
            Some(&privacy),
            RefreshTrigger::Reconcile,
            now + 2 * FAILED_RETRY_SECS,
            "en"
        ));
        assert!(is_stale(
            Some(&privacy),
            RefreshTrigger::Reconcile,
            now + RECONCILE_READY_SECS,
            "en"
        ));

        let expired = StoredAchievementSummary {
            failure: Some(crate::library::CatalogError::Expired),
            ..summary(AchievementGameState::Failed, None, now - 10, "en")
        };
        assert!(!is_stale(
            Some(&expired),
            RefreshTrigger::Import,
            now + 2 * FAILED_RETRY_SECS,
            "en"
        ));
    }

    #[test]
    fn a_failure_is_retried_by_reconciliation_and_keeps_no_reported_time() {
        let now = 2_000_000_000;
        let failed = StoredAchievementSummary {
            failure: Some(crate::library::CatalogError::Privacy),
            ..summary(AchievementGameState::Failed, None, now - 10, "en")
        };
        assert!(!is_stale(
            Some(&failed),
            RefreshTrigger::Reconcile,
            now,
            "en"
        ));
        assert!(is_stale(
            Some(&failed),
            RefreshTrigger::Reconcile,
            now + RECONCILE_READY_SECS,
            "en"
        ));
        let game = StoredAchievementGame {
            entry_id: "620".into(),
            state: AchievementGameState::Ready,
            language: "en".into(),
            last_success_at: Some(100),
            last_attempt_at: 200,
            failure: Some(crate::library::CatalogError::Privacy),
            achievements: vec![
                achievement("no-time", true, None, 1),
                achievement("timed", true, Some(50), 0),
            ],
        };
        let summary = summarize(&game);
        assert_eq!(summary.unlocked, 2);
        assert_eq!(summary.total, 2);
        assert_eq!(summary.latest_unlock.as_ref().unwrap().id, "timed");
        let sink = AchievementSummaryView::from(&summary);
        assert_eq!(sink.state, AchievementGameState::Ready);
    }
}
