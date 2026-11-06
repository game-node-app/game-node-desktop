//! Authentication state transitions.
//!
//! [`AuthMachine`] is a pure reducer: callers feed it [`Input`]s and execute the returned
//! [`Effect`]s (HTTP, keyring, browser) with the platform/API adapters, then feed the results
//! back. Every effect carries an operation id; results with an outdated id are ignored, so a
//! late browser callback or verification can never resurrect a cancelled or signed-out session.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::callback::{self, CallbackError, CallbackOutcome, Provider};
use crate::credential::SessionCredential;
use crate::notice::Notice;

pub type AccountSummary = User;
pub type PendingSignInView = crate::snapshot::AuthView;
pub type SessionRestoreFailure = SessionError;

/// How long a started sign-in accepts its callback.
pub const ATTEMPT_TTL: Duration = Duration::from_secs(10 * 60);

/// The signed-in GameNode user, as returned by `/v1/auth/get-session`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub name: String,
    pub email: Option<String>,
    pub image: Option<String>,
}

impl User {
    /// The name shown in the top bar.
    pub fn display_name(&self) -> &str {
        if !self.name.trim().is_empty() {
            return self.name.trim();
        }
        match self.email.as_deref() {
            Some(email) if !email.is_empty() => email.split('@').next().unwrap_or(email),
            _ => &self.id,
        }
    }
}

/// Why verifying a credential with GameNode failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionError {
    /// GameNode answered that the session is invalid, expired or revoked.
    Rejected,
    /// GameNode could not be reached or answered unexpectedly; the credential may still be good.
    Unreachable,
}

/// Why starting a sign-in failed before the browser round trip.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SignInFailure {
    BrowserLaunch,
    ServerUnavailable,
}

/// Result of attempting to invalidate the session on the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteSignOut {
    Confirmed,
    NotConfirmed,
    /// There was no credential to invalidate.
    Skipped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Purpose {
    /// Verifying a credential that arrived through a callback.
    SignIn,
    /// Verifying the credential found in storage at startup (or on retry).
    Restore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    /// Asking GameNode to start the provider flow.
    Preparing,
    /// The system browser was opened; waiting for the callback.
    AwaitingBrowser,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AuthState {
    /// Startup: the stored credential has not been read yet.
    Starting,
    SignedOut,
    Pending {
        provider: Provider,
        stage: Stage,
    },
    Verifying(Purpose),
    SignedIn(User),
    /// A stored credential exists but GameNode was unreachable; retry or sign out.
    RestoreFailed,
    SigningOut,
}

/// Work for the caller to perform. Credentials inside are redacted in `Debug`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Effect {
    LoadCredential {
        op: u64,
    },
    /// Ask GameNode to start the provider flow, then open the returned URL in the browser.
    BeginSignIn {
        op: u64,
        provider: Provider,
        success_url: String,
        failure_url: String,
    },
    VerifySession {
        op: u64,
        credential: SessionCredential,
    },
    StoreCredential {
        op: u64,
        credential: SessionCredential,
    },
    DeleteCredential,
    /// Invalidate the server session (if a credential is given) and delete the stored credential.
    SignOut {
        op: u64,
        credential: Option<SessionCredential>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Input {
    /// The app started; read the stored credential.
    Start,
    CredentialLoaded {
        op: u64,
        result: Result<Option<SessionCredential>, ()>,
    },
    StartSignIn(Provider),
    CancelSignIn,
    /// The browser was opened for the attempt started by `BeginSignIn { op }`.
    BrowserOpened {
        op: u64,
    },
    SignInFailed {
        op: u64,
        reason: SignInFailure,
    },
    /// A raw `app.gamenode://` URL from the OS or another process.
    CallbackReceived(String),
    SessionChecked {
        op: u64,
        result: Result<User, SessionError>,
    },
    CredentialStored {
        op: u64,
        ok: bool,
    },
    RetryRestore,
    SignOut,
    SignOutFinished {
        op: u64,
        remote: RemoteSignOut,
        local_deleted: bool,
    },
    CredentialDeleted {
        ok: bool,
    },
    /// Time passed; expires a stale attempt.
    Tick,
    /// A GameNode request made with the signed-in session was answered `401`. Handled like a
    /// rejected session check: the credential is dropped and sign-in is offered.
    SessionRejected,
}

#[derive(Debug)]
struct Attempt {
    nonce: String,
    created: Instant,
}

/// Produces unpredictable single-use correlation values.
pub type NonceSource = Box<dyn FnMut() -> String + Send>;

pub struct AuthMachine {
    state: AuthState,
    credential: Option<SessionCredential>,
    attempt: Option<Attempt>,
    notice: Option<Notice>,
    last_rejection: Option<CallbackError>,
    op: u64,
    nonce: NonceSource,
}

impl AuthMachine {
    pub fn new(nonce: NonceSource) -> Self {
        Self {
            state: AuthState::Starting,
            credential: None,
            attempt: None,
            notice: None,
            last_rejection: None,
            op: 0,
            nonce,
        }
    }

    pub fn state(&self) -> &AuthState {
        &self.state
    }

    pub fn notice(&self) -> Option<Notice> {
        self.notice
    }

    /// Why the most recent callback was ignored, if it was. Diagnostics only; carries no data.
    pub fn last_rejection(&self) -> Option<CallbackError> {
        self.last_rejection
    }

    /// The credential of the verified session, for requests made on the user's behalf. `None`
    /// until verification succeeds, so nothing is sent with an unverified credential.
    pub fn verified_credential(&self) -> Option<&SessionCredential> {
        match self.state {
            AuthState::SignedIn(_) => self.credential.as_ref(),
            _ => None,
        }
    }

    /// Whether a sign-in attempt is currently accepting a callback.
    pub fn has_attempt(&self) -> bool {
        self.attempt.is_some()
    }

    /// Clears the displayed notice (the user dismissed it).
    pub fn dismiss_notice(&mut self) {
        self.notice = None;
    }

    fn next_op(&mut self) -> u64 {
        self.op += 1;
        self.op
    }

    fn fail_to_signed_out(&mut self, notice: Notice) {
        self.attempt = None;
        self.credential = None;
        self.state = AuthState::SignedOut;
        self.notice = Some(notice);
    }

    pub fn handle(&mut self, input: Input, now: Instant) -> Vec<Effect> {
        match input {
            Input::Start => self.on_start(),
            Input::CredentialLoaded { op, result } => self.on_credential_loaded(op, result),
            Input::StartSignIn(provider) => self.on_start_sign_in(provider, now),
            Input::CancelSignIn => self.on_cancel(),
            Input::BrowserOpened { op } => {
                if let AuthState::Pending {
                    provider,
                    stage: Stage::Preparing,
                } = self.state
                    && op == self.op
                {
                    self.state = AuthState::Pending {
                        provider,
                        stage: Stage::AwaitingBrowser,
                    };
                }
                vec![]
            }
            Input::SignInFailed { op, reason } => {
                if matches!(self.state, AuthState::Pending { .. }) && op == self.op {
                    self.op += 1;
                    self.fail_to_signed_out(match reason {
                        SignInFailure::BrowserLaunch => Notice::BrowserLaunchFailed,
                        SignInFailure::ServerUnavailable => Notice::SignInUnavailable,
                    });
                }
                vec![]
            }
            Input::CallbackReceived(raw) => self.on_callback(&raw, now),
            Input::SessionChecked { op, result } => self.on_session_checked(op, result),
            Input::CredentialStored { op, ok } => {
                if op == self.op && !ok && matches!(self.state, AuthState::SignedIn(_)) {
                    self.notice = Some(Notice::PersistFailed);
                }
                vec![]
            }
            Input::RetryRestore => {
                if self.state == AuthState::RestoreFailed
                    && let Some(credential) = self.credential.clone()
                {
                    self.notice = None;
                    self.state = AuthState::Verifying(Purpose::Restore);
                    let op = self.next_op();
                    return vec![Effect::VerifySession { op, credential }];
                }
                vec![]
            }
            Input::SignOut => self.on_sign_out(),
            Input::SignOutFinished {
                op,
                remote,
                local_deleted,
            } => {
                if self.state == AuthState::SigningOut && op == self.op {
                    self.state = AuthState::SignedOut;
                    self.notice = if !local_deleted {
                        Some(Notice::CredentialDeleteFailed)
                    } else if remote == RemoteSignOut::NotConfirmed {
                        Some(Notice::SignOutNotConfirmed)
                    } else {
                        None
                    };
                }
                vec![]
            }
            Input::CredentialDeleted { ok } => {
                if !ok && self.state == AuthState::SignedOut {
                    self.notice = Some(Notice::CredentialDeleteFailed);
                }
                vec![]
            }
            Input::SessionRejected => {
                if !matches!(self.state, AuthState::SignedIn(_)) {
                    return vec![];
                }
                self.op += 1;
                self.fail_to_signed_out(Notice::SessionRejected);
                vec![Effect::DeleteCredential]
            }
            Input::Tick => {
                if let Some(attempt) = &self.attempt
                    && now.saturating_duration_since(attempt.created) >= ATTEMPT_TTL
                {
                    self.op += 1;
                    self.fail_to_signed_out(Notice::AttemptExpired);
                }
                vec![]
            }
        }
    }

    fn on_start(&mut self) -> Vec<Effect> {
        if self.state != AuthState::Starting {
            return vec![];
        }
        vec![Effect::LoadCredential { op: self.next_op() }]
    }

    fn on_credential_loaded(
        &mut self,
        op: u64,
        result: Result<Option<SessionCredential>, ()>,
    ) -> Vec<Effect> {
        if self.state != AuthState::Starting || op != self.op {
            return vec![];
        }
        match result {
            Ok(None) => self.state = AuthState::SignedOut,
            Err(()) => self.fail_to_signed_out(Notice::CredentialUnreadable),
            Ok(Some(credential)) => {
                self.credential = Some(credential.clone());
                self.state = AuthState::Verifying(Purpose::Restore);
                let op = self.next_op();
                return vec![Effect::VerifySession { op, credential }];
            }
        }
        vec![]
    }

    fn on_start_sign_in(&mut self, provider: Provider, now: Instant) -> Vec<Effect> {
        // A new attempt replaces an earlier pending one; it is not allowed over a session.
        if !matches!(self.state, AuthState::SignedOut | AuthState::Pending { .. }) {
            return vec![];
        }
        let nonce = (self.nonce)();
        let effect = Effect::BeginSignIn {
            op: self.next_op(),
            provider,
            success_url: callback::success_url(&nonce),
            failure_url: callback::failure_url(&nonce),
        };
        self.attempt = Some(Attempt {
            nonce,
            created: now,
        });
        self.notice = None;
        self.state = AuthState::Pending {
            provider,
            stage: Stage::Preparing,
        };
        vec![effect]
    }

    fn on_cancel(&mut self) -> Vec<Effect> {
        if matches!(self.state, AuthState::Pending { .. }) {
            self.op += 1;
            self.fail_to_signed_out(Notice::SignInCancelled);
        }
        vec![]
    }

    fn on_callback(&mut self, raw: &str, now: Instant) -> Vec<Effect> {
        self.last_rejection = None;
        match self.validate_callback(raw, now) {
            Ok(CallbackOutcome::Success(credential)) => {
                self.attempt = None; // single use
                self.credential = Some(credential.clone());
                self.state = AuthState::Verifying(Purpose::SignIn);
                self.notice = None;
                let op = self.next_op();
                vec![Effect::VerifySession { op, credential }]
            }
            Ok(CallbackOutcome::Failure) => {
                self.op += 1;
                self.fail_to_signed_out(Notice::ProviderRejected);
                vec![]
            }
            Err(CallbackError::Expired) => {
                self.last_rejection = Some(CallbackError::Expired);
                self.op += 1;
                self.fail_to_signed_out(Notice::AttemptExpired);
                vec![]
            }
            Err(reason) => {
                self.last_rejection = Some(reason);
                vec![]
            }
        }
    }

    fn validate_callback(&self, raw: &str, now: Instant) -> Result<CallbackOutcome, CallbackError> {
        let parsed = callback::parse(raw)?;
        let (
            Some(attempt),
            AuthState::Pending {
                stage: Stage::AwaitingBrowser,
                ..
            },
        ) = (&self.attempt, &self.state)
        else {
            return Err(CallbackError::NoActiveAttempt);
        };
        if now.saturating_duration_since(attempt.created) >= ATTEMPT_TTL {
            return Err(CallbackError::Expired);
        }
        if !constant_time_eq(attempt.nonce.as_bytes(), parsed.nonce.as_bytes()) {
            return Err(CallbackError::CorrelationMismatch);
        }
        Ok(parsed.outcome)
    }

    fn on_session_checked(&mut self, op: u64, result: Result<User, SessionError>) -> Vec<Effect> {
        let AuthState::Verifying(purpose) = self.state else {
            return vec![];
        };
        if op != self.op {
            return vec![];
        }
        match (purpose, result) {
            (Purpose::SignIn, Ok(user)) => {
                let credential = self.credential.clone();
                self.state = AuthState::SignedIn(user);
                self.notice = None;
                match credential {
                    Some(credential) => vec![Effect::StoreCredential { op, credential }],
                    None => vec![],
                }
            }
            (Purpose::Restore, Ok(user)) => {
                self.state = AuthState::SignedIn(user);
                self.notice = None;
                vec![]
            }
            (Purpose::SignIn, Err(SessionError::Rejected)) => {
                self.fail_to_signed_out(Notice::SignInRejected);
                vec![]
            }
            (Purpose::SignIn, Err(SessionError::Unreachable)) => {
                self.fail_to_signed_out(Notice::ConnectionProblem);
                vec![]
            }
            (Purpose::Restore, Err(SessionError::Rejected)) => {
                self.fail_to_signed_out(Notice::SessionRejected);
                vec![Effect::DeleteCredential]
            }
            (Purpose::Restore, Err(SessionError::Unreachable)) => {
                // Keep the credential; the user can retry or sign out.
                self.state = AuthState::RestoreFailed;
                self.notice = Some(Notice::ConnectionProblem);
                vec![]
            }
        }
    }

    fn on_sign_out(&mut self) -> Vec<Effect> {
        if matches!(
            self.state,
            AuthState::Starting | AuthState::SignedOut | AuthState::SigningOut
        ) {
            return vec![];
        }
        let credential = self.credential.take();
        self.attempt = None;
        self.notice = None;
        self.state = AuthState::SigningOut;
        let op = self.next_op();
        vec![Effect::SignOut { op, credential }]
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> User {
        User {
            id: "u1".into(),
            name: "Ana".into(),
            email: None,
            image: None,
        }
    }

    fn cred(v: &str) -> SessionCredential {
        SessionCredential::new(v).unwrap()
    }

    fn machine() -> AuthMachine {
        let mut n = 0;
        AuthMachine::new(Box::new(move || {
            n += 1;
            format!("nonce{n}")
        }))
    }

    fn cb(nonce: &str, token: &str) -> String {
        let cookie = format!("better-auth.session_token={token}; Path=/");
        let enc: String = url::form_urlencoded::byte_serialize(cookie.as_bytes()).collect();
        format!("app.gamenode://home?desktop_attempt={nonce}&cookie={enc}")
    }

    /// Starts the machine signed out.
    fn signed_out(m: &mut AuthMachine, t: Instant) {
        let [Effect::LoadCredential { op }] = m.handle(Input::Start, t)[..] else {
            panic!()
        };
        assert!(
            m.handle(
                Input::CredentialLoaded {
                    op,
                    result: Ok(None)
                },
                t
            )
            .is_empty()
        );
        assert_eq!(m.state(), &AuthState::SignedOut);
    }

    /// Drives a sign-in up to AwaitingBrowser and returns its op id.
    fn begin(m: &mut AuthMachine, t: Instant) -> u64 {
        let effects = m.handle(Input::StartSignIn(Provider::Google), t);
        let [
            Effect::BeginSignIn {
                op,
                provider,
                success_url,
                failure_url,
            },
        ] = &effects[..]
        else {
            panic!("{effects:?}")
        };
        assert_eq!(*provider, Provider::Google);
        assert!(success_url.starts_with("app.gamenode://home?desktop_attempt=nonce"));
        assert!(failure_url.contains("/sign-in-failed?desktop_attempt=nonce"));
        let op = *op;
        m.handle(Input::BrowserOpened { op }, t);
        assert_eq!(
            m.state(),
            &AuthState::Pending {
                provider: Provider::Google,
                stage: Stage::AwaitingBrowser
            }
        );
        op
    }

    fn sign_in(m: &mut AuthMachine, t: Instant, nonce: &str) {
        begin(m, t);
        let effects = m.handle(Input::CallbackReceived(cb(nonce, "tok")), t);
        let [Effect::VerifySession { op, .. }] = effects[..] else {
            panic!("{effects:?}")
        };
        let effects = m.handle(
            Input::SessionChecked {
                op,
                result: Ok(user()),
            },
            t,
        );
        assert!(matches!(effects[..], [Effect::StoreCredential { .. }]));
    }

    #[test]
    fn startup_without_credential_is_signed_out() {
        let mut m = machine();
        assert_eq!(m.state(), &AuthState::Starting);
        signed_out(&mut m, Instant::now());
        assert_eq!(m.notice(), None);
    }

    #[test]
    fn startup_unreadable_store_is_signed_out_with_notice() {
        let mut m = machine();
        let t = Instant::now();
        let [Effect::LoadCredential { op }] = m.handle(Input::Start, t)[..] else {
            panic!()
        };
        m.handle(
            Input::CredentialLoaded {
                op,
                result: Err(()),
            },
            t,
        );
        assert_eq!(m.state(), &AuthState::SignedOut);
        assert_eq!(m.notice(), Some(Notice::CredentialUnreadable));
    }

    #[test]
    fn sign_in_flow_verifies_then_stores_and_signs_in() {
        let mut m = machine();
        let t = Instant::now();
        signed_out(&mut m, t);
        let op = begin(&mut m, t);
        let effects = m.handle(Input::CallbackReceived(cb("nonce1", "tok")), t);
        let [
            Effect::VerifySession {
                op: verify_op,
                credential,
            },
        ] = &effects[..]
        else {
            panic!()
        };
        assert_eq!(credential.expose(), "tok");
        assert_eq!(m.state(), &AuthState::Verifying(Purpose::SignIn));
        assert!(
            !m.has_attempt(),
            "the attempt is consumed by the matching callback"
        );
        assert_ne!(*verify_op, op);

        let effects = m.handle(
            Input::SessionChecked {
                op: *verify_op,
                result: Ok(user()),
            },
            t,
        );
        assert!(
            matches!(&effects[..], [Effect::StoreCredential { credential, .. }] if credential.expose() == "tok")
        );
        assert_eq!(m.state(), &AuthState::SignedIn(user()));
        assert_eq!(m.notice(), None);
    }

    #[test]
    fn callback_cannot_be_replayed() {
        let mut m = machine();
        let t = Instant::now();
        signed_out(&mut m, t);
        sign_in(&mut m, t, "nonce1");
        let before = m.state().clone();
        assert!(
            m.handle(Input::CallbackReceived(cb("nonce1", "evil")), t)
                .is_empty()
        );
        assert_eq!(m.state(), &before);
        assert_eq!(m.last_rejection(), Some(CallbackError::NoActiveAttempt));
    }

    #[test]
    fn unsolicited_and_mismatched_callbacks_change_nothing() {
        let mut m = machine();
        let t = Instant::now();
        signed_out(&mut m, t);
        // Cold start: no attempt.
        assert!(
            m.handle(Input::CallbackReceived(cb("nonce1", "tok")), t)
                .is_empty()
        );
        assert_eq!(m.state(), &AuthState::SignedOut);
        assert_eq!(m.last_rejection(), Some(CallbackError::NoActiveAttempt));

        begin(&mut m, t);
        let pending = m.state().clone();
        for (raw, why) in [
            (cb("other", "tok"), CallbackError::CorrelationMismatch),
            (
                "app.gamenode://evil?desktop_attempt=nonce1&cookie=x".to_string(),
                CallbackError::WrongTarget,
            ),
            ("nonsense".to_string(), CallbackError::Malformed),
            (
                "app.gamenode://home?desktop_attempt=nonce1".to_string(),
                CallbackError::MissingCredential,
            ),
        ] {
            assert!(m.handle(Input::CallbackReceived(raw), t).is_empty());
            assert_eq!(m.state(), &pending);
            assert_eq!(m.last_rejection(), Some(why));
            assert!(
                m.has_attempt(),
                "a bad callback does not consume the attempt"
            );
        }
    }

    #[test]
    fn callback_before_the_browser_opened_is_rejected() {
        let mut m = machine();
        let t = Instant::now();
        signed_out(&mut m, t);
        m.handle(Input::StartSignIn(Provider::Discord), t);
        m.handle(Input::CallbackReceived(cb("nonce1", "tok")), t);
        assert_eq!(m.last_rejection(), Some(CallbackError::NoActiveAttempt));
        assert!(matches!(
            m.state(),
            AuthState::Pending {
                stage: Stage::Preparing,
                ..
            }
        ));
    }

    #[test]
    fn expired_attempt_is_rejected_by_callback_and_tick() {
        let mut m = machine();
        let t = Instant::now();
        signed_out(&mut m, t);
        begin(&mut m, t);
        let late = t + ATTEMPT_TTL;
        m.handle(Input::CallbackReceived(cb("nonce1", "tok")), late);
        assert_eq!(m.state(), &AuthState::SignedOut);
        assert_eq!(m.notice(), Some(Notice::AttemptExpired));
        assert!(!m.has_attempt());

        begin_again(&mut m, late);
        m.handle(Input::Tick, late + ATTEMPT_TTL);
        assert_eq!(m.notice(), Some(Notice::AttemptExpired));
        assert_eq!(m.state(), &AuthState::SignedOut);
    }

    fn begin_again(m: &mut AuthMachine, t: Instant) {
        let effects = m.handle(Input::StartSignIn(Provider::Google), t);
        let [Effect::BeginSignIn { op, .. }] = effects[..] else {
            panic!()
        };
        m.handle(Input::BrowserOpened { op }, t);
    }

    #[test]
    fn a_new_attempt_replaces_the_pending_one() {
        let mut m = machine();
        let t = Instant::now();
        signed_out(&mut m, t);
        begin(&mut m, t);
        begin_again(&mut m, t); // nonce2
        m.handle(Input::CallbackReceived(cb("nonce1", "tok")), t);
        assert_eq!(m.last_rejection(), Some(CallbackError::CorrelationMismatch));
        let effects = m.handle(Input::CallbackReceived(cb("nonce2", "tok")), t);
        assert!(matches!(effects[..], [Effect::VerifySession { .. }]));
    }

    #[test]
    fn provider_failure_callback_and_launch_failure_return_to_signed_out() {
        let mut m = machine();
        let t = Instant::now();
        signed_out(&mut m, t);
        begin(&mut m, t);
        m.handle(
            Input::CallbackReceived(
                "app.gamenode://home/sign-in-failed?desktop_attempt=nonce1".into(),
            ),
            t,
        );
        assert_eq!(
            (m.state(), m.notice()),
            (&AuthState::SignedOut, Some(Notice::ProviderRejected))
        );

        let [Effect::BeginSignIn { op, .. }] =
            m.handle(Input::StartSignIn(Provider::Twitter), t)[..]
        else {
            panic!()
        };
        assert_eq!(m.notice(), None, "a new attempt clears the old notice");
        m.handle(
            Input::SignInFailed {
                op,
                reason: SignInFailure::BrowserLaunch,
            },
            t,
        );
        assert_eq!(m.notice(), Some(Notice::BrowserLaunchFailed));
        // A new attempt is permitted afterwards.
        begin_again(&mut m, t);
    }

    #[test]
    fn stale_results_are_ignored() {
        let mut m = machine();
        let t = Instant::now();
        signed_out(&mut m, t);
        let [Effect::BeginSignIn { op, .. }] =
            m.handle(Input::StartSignIn(Provider::Google), t)[..]
        else {
            panic!()
        };
        m.handle(Input::CancelSignIn, t);
        assert_eq!(m.notice(), Some(Notice::SignInCancelled));
        m.handle(Input::BrowserOpened { op }, t);
        assert_eq!(m.state(), &AuthState::SignedOut);
        m.handle(
            Input::SignInFailed {
                op,
                reason: SignInFailure::ServerUnavailable,
            },
            t,
        );
        assert_eq!(m.notice(), Some(Notice::SignInCancelled));
    }

    #[test]
    fn sign_in_verification_failures_leave_signed_out() {
        for (error, notice) in [
            (SessionError::Rejected, Notice::SignInRejected),
            (SessionError::Unreachable, Notice::ConnectionProblem),
        ] {
            let mut m = machine();
            let t = Instant::now();
            signed_out(&mut m, t);
            begin(&mut m, t);
            let effects = m.handle(Input::CallbackReceived(cb("nonce1", "tok")), t);
            let [Effect::VerifySession { op, .. }] = effects[..] else {
                panic!()
            };
            assert!(
                m.handle(
                    Input::SessionChecked {
                        op,
                        result: Err(error)
                    },
                    t
                )
                .is_empty()
            );
            assert_eq!(
                (m.state(), m.notice()),
                (&AuthState::SignedOut, Some(notice))
            );
        }
    }

    #[test]
    fn persist_failure_keeps_the_session_but_warns() {
        let mut m = machine();
        let t = Instant::now();
        signed_out(&mut m, t);
        begin(&mut m, t);
        let effects = m.handle(Input::CallbackReceived(cb("nonce1", "tok")), t);
        let [Effect::VerifySession { op, .. }] = effects[..] else {
            panic!()
        };
        let effects = m.handle(
            Input::SessionChecked {
                op,
                result: Ok(user()),
            },
            t,
        );
        let [Effect::StoreCredential { op, .. }] = effects[..] else {
            panic!()
        };
        m.handle(Input::CredentialStored { op, ok: false }, t);
        assert_eq!(m.state(), &AuthState::SignedIn(user()));
        assert_eq!(m.notice(), Some(Notice::PersistFailed));
    }

    fn restoring(m: &mut AuthMachine, t: Instant) -> u64 {
        let [Effect::LoadCredential { op }] = m.handle(Input::Start, t)[..] else {
            panic!()
        };
        let effects = m.handle(
            Input::CredentialLoaded {
                op,
                result: Ok(Some(cred("stored"))),
            },
            t,
        );
        let [Effect::VerifySession { op, credential }] = &effects[..] else {
            panic!()
        };
        assert_eq!(credential.expose(), "stored");
        assert_eq!(m.state(), &AuthState::Verifying(Purpose::Restore));
        *op
    }

    #[test]
    fn restore_valid_session_signs_in_without_another_store_or_browser() {
        let mut m = machine();
        let t = Instant::now();
        let op = restoring(&mut m, t);
        assert!(
            m.handle(
                Input::SessionChecked {
                    op,
                    result: Ok(user())
                },
                t
            )
            .is_empty()
        );
        assert_eq!(m.state(), &AuthState::SignedIn(user()));
    }

    #[test]
    fn restore_rejected_removes_the_credential() {
        let mut m = machine();
        let t = Instant::now();
        let op = restoring(&mut m, t);
        let effects = m.handle(
            Input::SessionChecked {
                op,
                result: Err(SessionError::Rejected),
            },
            t,
        );
        assert_eq!(effects, vec![Effect::DeleteCredential]);
        assert_eq!(
            (m.state(), m.notice()),
            (&AuthState::SignedOut, Some(Notice::SessionRejected))
        );
        m.handle(Input::CredentialDeleted { ok: false }, t);
        assert_eq!(m.notice(), Some(Notice::CredentialDeleteFailed));
    }

    #[test]
    fn an_api_rejection_while_signed_in_drops_the_session() {
        let mut m = machine();
        let t = Instant::now();
        let op = restoring(&mut m, t);
        m.handle(
            Input::SessionChecked {
                op,
                result: Ok(user()),
            },
            t,
        );
        assert!(m.verified_credential().is_some());

        let effects = m.handle(Input::SessionRejected, t);
        assert_eq!(effects, vec![Effect::DeleteCredential]);
        assert_eq!(
            (m.state(), m.notice()),
            (&AuthState::SignedOut, Some(Notice::SessionRejected))
        );
        assert!(m.verified_credential().is_none());
        // A second rejection (several requests were in flight) changes nothing.
        assert!(m.handle(Input::SessionRejected, t).is_empty());
    }

    #[test]
    fn an_api_rejection_is_ignored_unless_signed_in() {
        let mut m = machine();
        let t = Instant::now();
        let op = restoring(&mut m, t);
        // Still verifying: the pending check decides, not a stray library request.
        assert!(m.handle(Input::SessionRejected, t).is_empty());
        assert_eq!(m.state(), &AuthState::Verifying(Purpose::Restore));
        assert!(m.verified_credential().is_none());
        m.handle(
            Input::SessionChecked {
                op,
                result: Ok(user()),
            },
            t,
        );
        assert_eq!(m.state(), &AuthState::SignedIn(user()));
    }

    #[test]
    fn restore_network_failure_keeps_the_credential_and_allows_retry() {
        let mut m = machine();
        let t = Instant::now();
        let op = restoring(&mut m, t);
        let effects = m.handle(
            Input::SessionChecked {
                op,
                result: Err(SessionError::Unreachable),
            },
            t,
        );
        assert!(effects.is_empty(), "nothing is deleted on a timeout");
        assert_eq!(
            (m.state(), m.notice()),
            (&AuthState::RestoreFailed, Some(Notice::ConnectionProblem))
        );

        let effects = m.handle(Input::RetryRestore, t);
        let [Effect::VerifySession { op, credential }] = &effects[..] else {
            panic!()
        };
        assert_eq!(credential.expose(), "stored");
        assert_eq!(m.notice(), None);
        m.handle(
            Input::SessionChecked {
                op: *op,
                result: Ok(user()),
            },
            t,
        );
        assert_eq!(m.state(), &AuthState::SignedIn(user()));
    }

    #[test]
    fn sign_out_clears_identity_and_ignores_late_results() {
        let mut m = machine();
        let t = Instant::now();
        signed_out(&mut m, t);
        sign_in(&mut m, t, "nonce1");
        let effects = m.handle(Input::SignOut, t);
        let [Effect::SignOut { op, credential }] = &effects[..] else {
            panic!("{effects:?}")
        };
        assert_eq!(credential.as_ref().unwrap().expose(), "tok");
        assert_eq!(m.state(), &AuthState::SigningOut);
        // A new sign-in cannot start until sign-out finishes.
        assert!(m.handle(Input::StartSignIn(Provider::Google), t).is_empty());
        m.handle(
            Input::SignOutFinished {
                op: *op,
                remote: RemoteSignOut::Confirmed,
                local_deleted: true,
            },
            t,
        );
        assert_eq!((m.state(), m.notice()), (&AuthState::SignedOut, None));
    }

    #[test]
    fn sign_out_reports_unconfirmed_server_and_failed_deletion() {
        for (remote, local, notice) in [
            (
                RemoteSignOut::NotConfirmed,
                true,
                Some(Notice::SignOutNotConfirmed),
            ),
            (
                RemoteSignOut::Confirmed,
                false,
                Some(Notice::CredentialDeleteFailed),
            ),
            (
                RemoteSignOut::NotConfirmed,
                false,
                Some(Notice::CredentialDeleteFailed),
            ),
        ] {
            let mut m = machine();
            let t = Instant::now();
            signed_out(&mut m, t);
            sign_in(&mut m, t, "nonce1");
            let [Effect::SignOut { op, .. }] = m.handle(Input::SignOut, t)[..] else {
                panic!()
            };
            m.handle(
                Input::SignOutFinished {
                    op,
                    remote,
                    local_deleted: local,
                },
                t,
            );
            assert_eq!((m.state(), m.notice()), (&AuthState::SignedOut, notice));
        }
    }

    #[test]
    fn sign_out_while_verifying_prevents_late_sign_in() {
        let mut m = machine();
        let t = Instant::now();
        signed_out(&mut m, t);
        begin(&mut m, t);
        let effects = m.handle(Input::CallbackReceived(cb("nonce1", "tok")), t);
        let [Effect::VerifySession { op: verify_op, .. }] = effects[..] else {
            panic!()
        };
        let effects = m.handle(Input::SignOut, t);
        let [Effect::SignOut { op, .. }] = effects[..] else {
            panic!()
        };
        // The verification finishes after sign-out started: it must not sign the user in.
        m.handle(
            Input::SessionChecked {
                op: verify_op,
                result: Ok(user()),
            },
            t,
        );
        assert_eq!(m.state(), &AuthState::SigningOut);
        m.handle(
            Input::SignOutFinished {
                op,
                remote: RemoteSignOut::Confirmed,
                local_deleted: true,
            },
            t,
        );
        assert_eq!(m.state(), &AuthState::SignedOut);
        // ...and a late callback from the earlier attempt is unsolicited.
        m.handle(Input::CallbackReceived(cb("nonce1", "tok")), t);
        assert_eq!(m.state(), &AuthState::SignedOut);
    }

    #[test]
    fn sign_out_from_restore_failed_deletes_the_stored_credential() {
        let mut m = machine();
        let t = Instant::now();
        let op = restoring(&mut m, t);
        m.handle(
            Input::SessionChecked {
                op,
                result: Err(SessionError::Unreachable),
            },
            t,
        );
        let effects = m.handle(Input::SignOut, t);
        assert!(matches!(
            &effects[..],
            [Effect::SignOut {
                credential: Some(_),
                ..
            }]
        ));
    }

    #[test]
    fn callback_while_signed_in_cannot_replace_the_session() {
        let mut m = machine();
        let t = Instant::now();
        signed_out(&mut m, t);
        sign_in(&mut m, t, "nonce1");
        assert!(
            m.handle(Input::CallbackReceived(cb("nonce2", "intruder")), t)
                .is_empty()
        );
        assert_eq!(m.state(), &AuthState::SignedIn(user()));
        assert!(m.handle(Input::StartSignIn(Provider::Google), t).is_empty());
    }

    #[test]
    fn debug_output_never_contains_the_credential() {
        let effect = Effect::VerifySession {
            op: 1,
            credential: cred("super-secret"),
        };
        assert!(!format!("{effect:?}").contains("super-secret"));
    }

    #[test]
    fn display_name_falls_back() {
        let mut u = user();
        assert_eq!(u.display_name(), "Ana");
        u.name = " ".into();
        u.email = Some("ana@example.com".into());
        assert_eq!(u.display_name(), "ana");
        u.email = None;
        assert_eq!(u.display_name(), "u1");
    }
}
