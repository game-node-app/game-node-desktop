//! What the UI renders: plain data derived from core state.

use crate::auth::{AuthMachine, AuthState, Stage, User};
use crate::callback::Provider;
use crate::credential::StorageMethod;
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
    pub auth: AuthView,
    pub notice: Option<Notice>,
    pub storage: StorageMethod,
    pub settings: Settings,
}

impl Snapshot {
    pub fn new(machine: &AuthMachine, storage: StorageMethod, settings: Settings) -> Self {
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
            auth,
            notice: machine.notice(),
            storage,
            settings,
        }
    }
}
