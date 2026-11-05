//! What the UI renders: plain data derived from core state.

use crate::auth::{AuthMachine, AuthState, Stage, User};
use crate::callback::Provider;
use crate::credential::StorageMethod;
use crate::library::{LaunchStatus, LibraryView};
use crate::notice::Notice;
use crate::settings::Settings;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthView {
    /// Startup, before the stored session has been read.
    Starting,
    SignedOut,
    /// A sign-in is in progress. `awaiting_browser` is true once the browser was opened.
    Pending {
        provider: Provider,
        awaiting_browser: bool,
    },
    /// Validating a session with GameNode.
    Verifying,
    SignedIn {
        user: User,
    },
    /// A stored session exists but GameNode was unreachable.
    RestoreFailed,
    SigningOut,
}

/// A complete, immutable view of the application for the UI. Never contains credentials.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub local_games: crate::drm_free::LocalView,
    pub playtime_error: bool,
    pub tracking: TrackingView,
    pub sync: crate::sync::SyncPreferences,
    pub playtime_source: crate::playtime::PlaytimeSource,
    pub auth: AuthView,
    pub notice: Option<Notice>,
    pub storage: StorageMethod,
    pub settings: Settings,
    /// The game library, present while an account's library is open.
    pub library: Option<LibraryView>,
    /// The latest Play or Install, while it is opening or after it failed.
    pub launch: Option<LaunchStatus>,
}

impl Snapshot {
    pub fn new(
        machine: &AuthMachine,
        storage: StorageMethod,
        settings: Settings,
        library: Option<LibraryView>,
        launch: Option<LaunchStatus>,
    ) -> Self {
        let auth = match machine.state() {
            AuthState::Starting => AuthView::Starting,
            AuthState::SignedOut => AuthView::SignedOut,
            AuthState::Pending { provider, stage } => AuthView::Pending {
                provider: *provider,
                awaiting_browser: *stage == Stage::AwaitingBrowser,
            },
            AuthState::Verifying(_) => AuthView::Verifying,
            AuthState::SignedIn(user) => AuthView::SignedIn { user: user.clone() },
            AuthState::RestoreFailed => AuthView::RestoreFailed,
            AuthState::SigningOut => AuthView::SigningOut,
        };
        Self {
            local_games: Default::default(),
            playtime_error: false,
            tracking: TrackingView::default(),
            sync: Default::default(),
            playtime_source: Default::default(),
            auth,
            notice: machine.notice(),
            storage,
            settings,
            library,
            launch,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrackingView {
    pub title: String,
    pub available: bool,
    pub running: std::collections::BTreeSet<String>,
}
