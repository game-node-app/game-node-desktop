//! Durable, non-secret foundation settings.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::library::{LibraryLayout, LibraryScope};
use crate::tools::ToolChannel;

/// Window placement in logical pixels. `x`/`y` are absent where the window system does not allow
/// applications to read or set positions (Wayland).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowGeometry {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

/// The interface language preference. `System` follows the OS language and falls back to English
/// when that language is not supported.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Locale {
    #[default]
    System,
    En,
    PtBr,
}

/// The sidebar scope the library opens with. `Last` keeps the scope remembered in the layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartScope {
    #[default]
    Last,
    All,
    Installed,
    Favorites,
    Recent,
}

impl StartScope {
    /// The layout scope a fixed choice replaces the remembered scope with. `Last` keeps it.
    pub fn layout_scope(self) -> Option<LibraryScope> {
        match self {
            Self::Last => None,
            Self::All => Some(LibraryScope::All),
            Self::Installed => Some(LibraryScope::Installed),
            Self::Favorites => Some(LibraryScope::Favorites),
            Self::Recent => Some(LibraryScope::Recent),
        }
    }
}

/// PC-wide preferences from Settings. Every field has a default, so a database written by an
/// older build loads with today's behavior and one unreadable value falls back alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppPreferences {
    pub start_with_windows: bool,
    pub start_minimized: bool,
    pub close_to_tray: bool,
    pub locale: Locale,
    pub show_status_bar: bool,
    pub start_scope: StartScope,
    pub discovery_on_start: bool,
    pub discovery_realtime: bool,
    pub tool_auto_check: bool,
    pub tool_default_channel: ToolChannel,
    /// Provider ids whose shared-only entries are left out of the library. Empty includes all.
    pub shared_excluded: BTreeSet<String>,
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            start_with_windows: true,
            start_minimized: true,
            close_to_tray: true,
            locale: Locale::System,
            show_status_bar: true,
            start_scope: StartScope::Last,
            discovery_on_start: true,
            discovery_realtime: true,
            tool_auto_check: true,
            tool_default_channel: ToolChannel::Stable,
            shared_excluded: BTreeSet::new(),
        }
    }
}

impl AppPreferences {
    /// Applies the "Open in" preference once, when the layout is loaded at start: `Last` keeps the
    /// remembered scope and a fixed choice replaces it in memory. Later scope changes persist
    /// through the normal layout path.
    pub fn apply_start_scope(&self, mut layout: LibraryLayout) -> LibraryLayout {
        if let Some(scope) = self.start_scope.layout_scope() {
            layout.scope = scope;
        }
        layout
    }
}

/// One Settings change. Every preference has its own variant, so the host validates and applies
/// one field without trusting a whole preferences object from the WebView.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AppPreferencePatch {
    StartWithWindows(bool),
    StartMinimized(bool),
    CloseToTray(bool),
    Locale(Locale),
    ShowStatusBar(bool),
    StartScope(StartScope),
    DiscoveryOnStart(bool),
    DiscoveryRealtime(bool),
    ToolAutoCheck(bool),
    ToolDefaultChannel(ToolChannel),
    /// Replaces the whole set of providers whose shared-only entries are left out.
    SharedExcluded(BTreeSet<String>),
}

/// Why a preference change was refused. The value and the snapshot are unchanged.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AppPreferenceError {
    #[error("a shared-games exclusion names an invalid store")]
    InvalidProviderId,
    #[error("the preference could not be saved: {0}")]
    Storage(String),
    /// The value was stored, but applying it outside GameNode failed. The host rolls the stored
    /// value back when it can, so the saved and effective states stay together.
    #[error("the preference could not be applied: {0}")]
    SideEffect(String),
}

impl AppPreferencePatch {
    /// Checks the values the type system cannot, before anything is changed.
    pub fn validate(&self) -> Result<(), AppPreferenceError> {
        if let Self::SharedExcluded(providers) = self
            && providers.iter().any(|id| !valid_provider_id(id))
        {
            return Err(AppPreferenceError::InvalidProviderId);
        }
        Ok(())
    }

    /// Writes this patch's field and leaves every other preference as it was.
    pub fn apply(&self, preferences: &mut AppPreferences) {
        match self {
            Self::StartWithWindows(value) => preferences.start_with_windows = *value,
            Self::StartMinimized(value) => preferences.start_minimized = *value,
            Self::CloseToTray(value) => preferences.close_to_tray = *value,
            Self::Locale(value) => preferences.locale = *value,
            Self::ShowStatusBar(value) => preferences.show_status_bar = *value,
            Self::StartScope(value) => preferences.start_scope = *value,
            Self::DiscoveryOnStart(value) => preferences.discovery_on_start = *value,
            Self::DiscoveryRealtime(value) => preferences.discovery_realtime = *value,
            Self::ToolAutoCheck(value) => preferences.tool_auto_check = *value,
            Self::ToolDefaultChannel(value) => preferences.tool_default_channel = *value,
            Self::SharedExcluded(value) => preferences.shared_excluded = value.clone(),
        }
    }
}

/// A provider id as the library and Settings use it: short, ASCII and free of separators that
/// would make it ambiguous in a stored list.
fn valid_provider_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

/// Foundation settings. Never holds credentials or callback URLs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub window: Option<WindowGeometry>,
    /// PC-wide application preferences, stored one key per field.
    #[serde(default)]
    pub preferences: AppPreferences,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{Effect, Input, LibraryMachine};

    #[test]
    fn application_preferences_default_to_todays_behavior() {
        let preferences = AppPreferences::default();
        assert!(preferences.start_with_windows);
        assert!(preferences.start_minimized);
        assert!(preferences.close_to_tray);
        assert_eq!(preferences.locale, Locale::System);
        assert!(preferences.show_status_bar);
        assert_eq!(preferences.start_scope, StartScope::Last);
        assert!(preferences.discovery_on_start);
        assert!(preferences.discovery_realtime);
        assert!(preferences.tool_auto_check);
        assert_eq!(preferences.tool_default_channel, ToolChannel::Stable);
        assert!(preferences.shared_excluded.is_empty());
        assert_eq!(
            Settings::default(),
            Settings {
                window: None,
                preferences: AppPreferences::default(),
            }
        );
    }

    #[test]
    fn every_preference_round_trips_through_its_contract_names() {
        let preferences = AppPreferences {
            start_with_windows: false,
            start_minimized: false,
            close_to_tray: false,
            locale: Locale::PtBr,
            show_status_bar: false,
            start_scope: StartScope::Installed,
            discovery_on_start: false,
            discovery_realtime: false,
            tool_auto_check: false,
            tool_default_channel: ToolChannel::Nightly,
            shared_excluded: BTreeSet::from(["steam".to_owned(), "epic".to_owned()]),
        };
        let json = serde_json::to_value(&preferences).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "startWithWindows": false,
                "startMinimized": false,
                "closeToTray": false,
                "locale": "ptBr",
                "showStatusBar": false,
                "startScope": "installed",
                "discoveryOnStart": false,
                "discoveryRealtime": false,
                "toolAutoCheck": false,
                "toolDefaultChannel": "nightly",
                "sharedExcluded": ["epic", "steam"]
            })
        );
        assert_eq!(
            serde_json::from_value::<AppPreferences>(json).unwrap(),
            preferences
        );

        // A preferences object written before a field existed still loads with the field's default.
        let partial: AppPreferences =
            serde_json::from_str(r#"{"locale":"en","showStatusBar":false}"#).unwrap();
        assert_eq!(partial.locale, Locale::En);
        assert!(!partial.show_status_bar);
        assert!(partial.start_with_windows);
        assert_eq!(partial.start_scope, StartScope::Last);
    }

    #[test]
    fn every_locale_and_start_scope_uses_the_contract_name() {
        for (locale, name) in [
            (Locale::System, "system"),
            (Locale::En, "en"),
            (Locale::PtBr, "ptBr"),
        ] {
            assert_eq!(serde_json::to_value(locale).unwrap(), name);
        }
        for (scope, name) in [
            (StartScope::Last, "last"),
            (StartScope::All, "all"),
            (StartScope::Installed, "installed"),
            (StartScope::Favorites, "favorites"),
            (StartScope::Recent, "recent"),
        ] {
            assert_eq!(serde_json::to_value(scope).unwrap(), name);
        }
    }

    #[test]
    fn a_fixed_start_scope_replaces_the_remembered_scope_and_last_keeps_it() {
        let remembered = LibraryLayout {
            scope: LibraryScope::Collection("backlog".to_owned()),
            ..LibraryLayout::default()
        };
        for (preference, expected) in [
            (
                StartScope::Last,
                LibraryScope::Collection("backlog".to_owned()),
            ),
            (StartScope::All, LibraryScope::All),
            (StartScope::Installed, LibraryScope::Installed),
            (StartScope::Favorites, LibraryScope::Favorites),
            (StartScope::Recent, LibraryScope::Recent),
        ] {
            let layout = AppPreferences {
                start_scope: preference,
                ..AppPreferences::default()
            }
            .apply_start_scope(remembered.clone());
            assert_eq!(layout.scope, expected, "{preference:?}");
        }
    }

    #[test]
    fn a_fixed_start_scope_is_applied_once_and_later_scope_changes_persist() {
        let layout = AppPreferences {
            start_scope: StartScope::Installed,
            ..AppPreferences::default()
        }
        .apply_start_scope(LibraryLayout {
            scope: LibraryScope::Favorites,
            ..LibraryLayout::default()
        });
        assert_eq!(layout.scope, LibraryScope::Installed);

        let mut machine = LibraryMachine::new(Vec::new(), layout);
        let effects = machine.handle(Input::SetScope(LibraryScope::Recent));
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::PersistLayout(saved) if saved.scope == LibraryScope::Recent
            )),
            "a later scope change is saved normally: {effects:?}"
        );
    }

    #[test]
    fn a_preference_patch_changes_only_its_own_field() {
        let mut preferences = AppPreferences::default();
        AppPreferencePatch::Locale(Locale::En).apply(&mut preferences);
        assert_eq!(preferences.locale, Locale::En);
        assert!(preferences.show_status_bar);
        assert_eq!(preferences.start_scope, StartScope::Last);

        AppPreferencePatch::SharedExcluded(BTreeSet::from(["steam".to_owned()]))
            .apply(&mut preferences);
        assert_eq!(
            preferences.shared_excluded,
            BTreeSet::from(["steam".to_owned()])
        );
        assert_eq!(preferences.locale, Locale::En);
    }

    #[test]
    fn preference_patches_use_the_tagged_contract_shape() {
        for (patch, expected) in [
            (
                AppPreferencePatch::StartWithWindows(false),
                serde_json::json!({ "startWithWindows": false }),
            ),
            (
                AppPreferencePatch::Locale(Locale::PtBr),
                serde_json::json!({ "locale": "ptBr" }),
            ),
            (
                AppPreferencePatch::ToolDefaultChannel(ToolChannel::Nightly),
                serde_json::json!({ "toolDefaultChannel": "nightly" }),
            ),
            (
                AppPreferencePatch::SharedExcluded(BTreeSet::from(["steam".to_owned()])),
                serde_json::json!({ "sharedExcluded": ["steam"] }),
            ),
        ] {
            let json = serde_json::to_value(&patch).unwrap();
            assert_eq!(json, expected);
            assert_eq!(
                serde_json::from_value::<AppPreferencePatch>(json).unwrap(),
                patch
            );
        }

        // Values outside the closed sets are refused at the boundary.
        assert!(serde_json::from_str::<AppPreferencePatch>(r#"{"locale":"klingon"}"#).is_err());
        assert!(serde_json::from_str::<AppPreferencePatch>(r#"{"startScope":"running"}"#).is_err());
        assert!(serde_json::from_str::<AppPreferencePatch>(r#"{"unknown":true}"#).is_err());
    }

    #[test]
    fn a_shared_exclusion_rejects_an_invalid_provider_id() {
        for id in ["", " steam", "steam ", "steam/epic", "steam family"] {
            let patch = AppPreferencePatch::SharedExcluded(BTreeSet::from([id.to_owned()]));
            assert_eq!(
                patch.validate(),
                Err(AppPreferenceError::InvalidProviderId),
                "{id:?}"
            );
        }
        for id in ["steam", "epic", "packagekit", "org.gamenode.plugin"] {
            let patch = AppPreferencePatch::SharedExcluded(BTreeSet::from([id.to_owned()]));
            assert_eq!(patch.validate(), Ok(()), "{id:?}");
        }
        assert_eq!(AppPreferencePatch::ShowStatusBar(false).validate(), Ok(()));
    }
}
