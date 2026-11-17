//! PC-wide sync preferences and the gate for sending local playtime to GameNode.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPreferences {
    pub enabled: bool,
    pub explained: bool,
}
impl Default for SyncPreferences {
    fn default() -> Self {
        Self {
            enabled: true,
            explained: false,
        }
    }
}
impl SyncPreferences {
    /// Corrupt opt-out data must never silently enable submissions.
    pub fn parse(enabled: Option<&str>, explained: Option<&str>) -> Self {
        let enabled = match enabled {
            None | Some("1") => true,
            Some("0") => false,
            Some(_) => {
                return Self {
                    enabled: false,
                    explained: false,
                };
            }
        };
        Self {
            enabled,
            explained: explained == Some("1"),
        }
    }
    pub fn may_submit(self, verified: bool) -> bool {
        self.enabled && self.explained && verified
    }
}

/// The counts one completed submission pass leaves for its account. No server response body or
/// credential is part of it; totals still held stay visible until a later pass accepts them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSummary {
    /// Unix seconds when the pass completed.
    pub completed_at: i64,
    /// Totals GameNode accepted in that pass, one per game.
    pub accepted_games: u32,
    /// Stores that contributed an accepted total.
    pub stores: u32,
    /// Totals still waiting for acceptance, including rejected ones.
    pub held: u32,
}

/// What the snapshot shows about sync: the stored preferences, the last completed pass and
/// whether one runs now. `running` is true for "Sync now" until its pass ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncView {
    pub enabled: bool,
    pub explained: bool,
    pub summary: Option<SyncSummary>,
    pub running: bool,
}

impl Default for SyncView {
    fn default() -> Self {
        Self {
            enabled: true,
            explained: false,
            summary: None,
            running: false,
        }
    }
}

impl From<SyncPreferences> for SyncView {
    fn from(preferences: SyncPreferences) -> Self {
        Self {
            enabled: preferences.enabled,
            explained: preferences.explained,
            summary: None,
            running: false,
        }
    }
}

/// Why "Sync now" cannot start. A pass that is already running is ignored, not an error.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncNowError {
    #[error("sync is off")]
    Disabled,
    #[error("the session is not verified")]
    Unverified,
}
