//! PC-wide sync preferences and the gate for sending local playtime to GameNode.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
