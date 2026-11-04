//! User-facing statuses and errors, as codes. The app translates them (English / pt-BR).

/// How prominently a notice is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// A foundation status or error. Carries no credential, URL or other sensitive data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    /// The system browser could not be opened.
    BrowserLaunchFailed,
    /// GameNode could not start the sign-in (server unreachable or refused the request).
    SignInUnavailable,
    /// The provider or GameNode reported a failed sign-in.
    ProviderRejected,
    /// The sign-in was not completed in time.
    AttemptExpired,
    /// The user cancelled the sign-in.
    SignInCancelled,
    /// GameNode rejected the session that came back from the browser.
    SignInRejected,
    /// GameNode could not be reached; the stored session is kept and can be retried.
    ConnectionProblem,
    /// The stored session was expired or revoked and was removed.
    SessionRejected,
    /// The stored session could not be read from the credential store.
    CredentialUnreadable,
    /// The session could not be saved; the user stays signed in only until the app exits.
    PersistFailed,
    /// Local sign-out finished but the server session could not be invalidated.
    SignOutNotConfirmed,
    /// The stored credential could not be deleted.
    CredentialDeleteFailed,
}

impl Notice {
    pub fn severity(self) -> Severity {
        match self {
            Self::SignInCancelled | Self::AttemptExpired => Severity::Info,
            Self::ConnectionProblem | Self::PersistFailed | Self::SignOutNotConfirmed => {
                Severity::Warning
            }
            _ => Severity::Error,
        }
    }
}
