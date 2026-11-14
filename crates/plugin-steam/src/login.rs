//! Steam interpretation of the public host login broker. The host owns native
//! windows; this module owns the meaning of the authenticated page and cookies.
use crate::steam::SteamSession;
use async_trait::async_trait;
use std::fmt;

/// Why a sign-in window operation failed. Every variant is a category: no URL, cookie, token or
/// webview error text is carried in the value or its `Debug`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SteamLoginError {
    OpenFailed,
    ProfileUnavailable,
    NotOpen,
    FocusFailed,
    CloseFailed,
}

impl fmt::Display for SteamLoginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::OpenFailed => "the Steam sign-in window could not be opened",
            Self::ProfileUnavailable => "the Steam sign-in profile could not be prepared",
            Self::NotOpen => "the Steam sign-in window is not open",
            Self::FocusFailed => "the Steam sign-in window could not be focused",
            Self::CloseFailed => "the Steam sign-in window could not be closed",
        })
    }
}

impl std::error::Error for SteamLoginError {}

/// How an attempt ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SteamLoginResult {
    /// The authenticated store page supplied identity and token in the sign-in webview.
    Verified { session: SteamSession },
    /// The user pressed Cancel, or a newer attempt superseded this one.
    Cancelled,
    /// The user closed the window.
    Closed,
    /// The ten-minute lifetime elapsed.
    TimedOut,
    /// The window could not be opened or its cookie jar could not be read.
    Failed(SteamLoginError),
}

/// One attempt's result. `attempt` comes from the window's generation counter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SteamLoginOutcome {
    pub attempt: u64,
    pub result: SteamLoginResult,
}

impl SteamLoginOutcome {
    pub(crate) fn failed(attempt: u64, error: SteamLoginError) -> Self {
        Self {
            attempt,
            result: SteamLoginResult::Failed(error),
        }
    }
}

/// The sign-in window the account service drives.
///
/// [`run`](Self::run) is a long-running operation: it opens the window and resolves when the user
/// finishes, closes the window, cancels, or the timeout elapses. Implementations must be
/// [`Send`] + [`Sync`] so the engine can call it from a background task.
#[async_trait]
pub trait SteamLoginWindow: Send + Sync {
    /// Shows a fresh Steam login window and waits for its result.
    async fn run(&self) -> SteamLoginOutcome;

    /// Brings the open window to the front without losing the attempt.
    fn focus(&self) -> Result<(), SteamLoginError>;

    /// Cancels the attempt and closes the window. Calling it with no window open is success.
    fn cancel(&self) -> Result<(), SteamLoginError>;

    /// Removes a leftover profile. Blocking: call it on the storage worker or a blocking task,
    /// not the UI thread.
    fn cleanup_profile(&self);
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_trusted_login_asset_still_parses_and_validates() {
        let policy: gamenode_plugin_protocol::login::LoginPolicy =
            toml::from_str(include_str!("../assets/login.toml")).expect("trusted login policy");
        policy.validate().expect("valid login policy");
        assert!(!policy.cookie_urls.is_empty());
        assert!(!policy.cookie_names.is_empty());
        assert!(
            policy
                .cookie_names
                .iter()
                .any(|name| name == "steamRefresh_steam")
        );
    }
}
