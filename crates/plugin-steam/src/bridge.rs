//! SDK-backed ports with immutable per-operation authority. Synchronous storage
//! ports are called only by the account service's blocking workers; login
//! controls must likewise be dispatched on a worker by the command handler.

use crate::{
    login::{SteamLoginError, SteamLoginOutcome, SteamLoginResult, SteamLoginWindow},
    steam::SteamSession,
};
use gamenode_plugin_protocol::{
    models::*,
    proto::{
        LoginOperation, LoginReply, LoginState, SecretOperation, ServiceFailure, secret_reply,
    },
};
use gamenode_plugin_sdk::{Error, HostClient};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

pub struct SdkStorage {
    host: HostClient,
    runtime: tokio::runtime::Handle,
    method: StorageMethod,
    validity: Arc<AtomicBool>,
}

impl SdkStorage {
    pub fn new(host: HostClient, method: StorageMethod, validity: Arc<AtomicBool>) -> Self {
        Self {
            host,
            runtime: tokio::runtime::Handle::current(),
            method,
            validity,
        }
    }
    fn mutation(
        &self,
        operation: SecretOperation,
        value: Option<&SessionCredential>,
    ) -> Result<(), CredentialError> {
        let reply = self
            .runtime
            .block_on(self.host.secret(operation, "session", value))
            .map_err(|error| {
                revoke(&self.validity, &error);
                CredentialError::Failed
            })?;
        match reply.outcome {
            Some(secret_reply::Outcome::Accepted(true)) => Ok(()),
            Some(secret_reply::Outcome::Failed(code)) if code == ServiceFailure::Locked as i32 => {
                Err(CredentialError::Locked)
            }
            _ => Err(CredentialError::Failed),
        }
    }
}

impl CredentialStore for SdkStorage {
    fn method(&self) -> StorageMethod {
        self.method
    }
    fn load(&self) -> Result<Option<SessionCredential>, CredentialError> {
        let reply = self
            .runtime
            .block_on(self.host.secret(SecretOperation::Read, "session", None))
            .map_err(|error| {
                revoke(&self.validity, &error);
                CredentialError::Failed
            })?;
        match reply.outcome {
            Some(secret_reply::Outcome::Value(bytes)) => String::from_utf8(bytes)
                .ok()
                .and_then(SessionCredential::new)
                .map(Some)
                .ok_or(CredentialError::Failed),
            Some(secret_reply::Outcome::Absent(true)) => Ok(None),
            Some(secret_reply::Outcome::Failed(code)) if code == ServiceFailure::Locked as i32 => {
                Err(CredentialError::Locked)
            }
            _ => Err(CredentialError::Failed),
        }
    }
    fn save(&self, credential: &SessionCredential) -> Result<(), CredentialError> {
        self.mutation(SecretOperation::Write, Some(credential))
    }
    fn delete(&self) -> Result<(), CredentialError> {
        self.mutation(SecretOperation::Delete, None)
    }
}

impl StoreAccountStore for SdkStorage {
    fn load(&self) -> Result<Option<StoreAccount>, StorageError> {
        self.runtime
            .block_on(self.host.account_load())
            .map_err(|error| {
                revoke(&self.validity, &error);
                StorageError::Failed("account read failed".into())
            })
    }
    fn save(&self, account: &StoreAccount) -> Result<(), StorageError> {
        self.runtime
            .block_on(self.host.account_save(account.clone()))
            .map_err(|error| {
                revoke(&self.validity, &error);
                StorageError::Failed("account write failed".into())
            })
    }
    fn clear(&self) -> Result<(), StorageError> {
        self.runtime
            .block_on(self.host.account_clear())
            .map_err(|error| {
                revoke(&self.validity, &error);
                StorageError::Failed("account removal failed".into())
            })
    }
}

/// Shared only to route focus/cancel to the current handle. It does not store
/// cookies, page data, credentials or a replaceable authorization context.
#[derive(Clone, Default)]
pub struct LoginAttempt(Arc<Mutex<Option<String>>>);

pub struct SdkLogin {
    host: HostClient,
    runtime: tokio::runtime::Handle,
    attempt: LoginAttempt,
    validity: Arc<AtomicBool>,
}

impl SdkLogin {
    pub fn new(host: HostClient, attempt: LoginAttempt, validity: Arc<AtomicBool>) -> Self {
        Self {
            host,
            runtime: tokio::runtime::Handle::current(),
            attempt,
            validity,
        }
    }
    fn handle(&self) -> Option<String> {
        self.attempt
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    fn forget(&self, handle: &str) {
        let mut current = self
            .attempt
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if current.as_deref() == Some(handle) {
            *current = None;
        }
    }
    async fn collect(&self, handle: &str) -> SteamLoginResult {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(600);
        loop {
            let reply =
                tokio::time::timeout_at(deadline, self.host.login(LoginOperation::Read, handle))
                    .await;
            match reply {
                Err(_) => return SteamLoginResult::TimedOut,
                Ok(Err(error @ (Error::Stale | Error::Denied))) => {
                    revoke(&self.validity, &error);
                    return SteamLoginResult::Cancelled;
                }
                Ok(Err(Error::Busy | Error::Unavailable)) => {}
                Ok(Err(_)) => return SteamLoginResult::Failed(SteamLoginError::OpenFailed),
                Ok(Ok(reply)) => match LoginState::try_from(reply.state) {
                    Ok(LoginState::Open) => {
                        if let Some(session) = authenticated_session(reply) {
                            return SteamLoginResult::Verified { session };
                        }
                    }
                    Ok(LoginState::Cancelled) => return SteamLoginResult::Cancelled,
                    Ok(LoginState::Closed) => return SteamLoginResult::Closed,
                    Ok(LoginState::TimedOut) => return SteamLoginResult::TimedOut,
                    _ => return SteamLoginResult::Failed(SteamLoginError::OpenFailed),
                },
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}

#[async_trait::async_trait]
impl SteamLoginWindow for SdkLogin {
    async fn run(&self) -> SteamLoginOutcome {
        let attempt = self.host.context().account_epoch;
        let opened = match self.host.login(LoginOperation::Open, "").await {
            Ok(reply) if reply.state == LoginState::Open as i32 && !reply.handle.is_empty() => {
                reply
            }
            Err(error) => {
                revoke(&self.validity, &error);
                return SteamLoginOutcome::failed(attempt, SteamLoginError::OpenFailed);
            }
            _ => return SteamLoginOutcome::failed(attempt, SteamLoginError::OpenFailed),
        };
        *self
            .attempt
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(opened.handle.clone());
        let result = self.collect(&opened.handle).await;
        if let Err(error) = self.host.login(LoginOperation::Close, &opened.handle).await {
            revoke(&self.validity, &error);
        }
        self.forget(&opened.handle);
        SteamLoginOutcome { attempt, result }
    }
    fn focus(&self) -> Result<(), SteamLoginError> {
        let handle = self.handle().ok_or(SteamLoginError::NotOpen)?;
        let reply = self
            .runtime
            .block_on(self.host.login(LoginOperation::Focus, &handle))
            .map_err(|error| {
                revoke(&self.validity, &error);
                SteamLoginError::FocusFailed
            })?;
        if reply.state == LoginState::Open as i32 {
            Ok(())
        } else {
            Err(SteamLoginError::NotOpen)
        }
    }
    fn cancel(&self) -> Result<(), SteamLoginError> {
        if let Some(handle) = self.handle() {
            self.runtime
                .block_on(self.host.login(LoginOperation::Cancel, &handle))
                .map_err(|error| {
                    revoke(&self.validity, &error);
                    SteamLoginError::CloseFailed
                })?;
            self.forget(&handle);
        }
        Ok(())
    }
    // The host owns ephemeral browser profiles and startup recovery. The plugin
    // must never know or inspect the host's private profile directory.
    fn cleanup_profile(&self) {}
}

fn revoke(validity: &AtomicBool, error: &Error) {
    if matches!(error, Error::Stale | Error::Denied) {
        validity.store(false, Ordering::Release);
    }
}

#[derive(serde::Deserialize)]
struct AuthenticatedPage {
    steam_id: String,
    account_name: Option<String>,
    token: String,
}

fn authenticated_session(reply: LoginReply) -> Option<SteamSession> {
    let page: AuthenticatedPage = serde_json::from_str(reply.page_result_json.as_deref()?).ok()?;
    if page.steam_id.is_empty() || !page.steam_id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let token = SessionCredential::new(page.token)?;
    let login = reply
        .cookies
        .iter()
        .find(|c| c.name == "steamLoginSecure" && !c.value.is_empty())?;
    let mut cookie = format!("steamLoginSecure={}", login.value);
    // `sessionid` is the store's CSRF cookie; `steamRefresh_steam` is the
    // long-lived renewal cookie set on login.steampowered.com. Persist both when
    // the window exposed them so a short-lived access token can be renewed.
    for name in ["sessionid", "steamRefresh_steam"] {
        if let Some(value) = reply
            .cookies
            .iter()
            .find(|c| c.name == name && !c.value.is_empty())
        {
            cookie.push_str(&format!("; {}={}", value.name, value.value));
        }
    }
    Some(SteamSession {
        steam_id: page.steam_id,
        account_name: page.account_name,
        token,
        cookie: SessionCredential::new(cookie)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gamenode_plugin_protocol::proto::LoginCookie;

    fn reply() -> LoginReply {
        LoginReply { cookies: vec![LoginCookie { name: "steamLoginSecure".into(), value: "cookie-secret".into() }, LoginCookie { name: "sessionid".into(), value: "session-secret".into() }, LoginCookie { name: "steamRefresh_steam".into(), value: "refresh-secret".into() }, LoginCookie { name: "unrelated".into(), value: "excluded".into() }], page_result_json: Some(r#"{"steam_id":"76561198000000001","account_name":"player","token":"temporary-token-secret"}"#.into()), ..Default::default() }
    }

    #[test]
    fn verified_page_and_cookie_are_both_required_and_only_the_cookie_is_persistable() {
        let session = authenticated_session(reply()).unwrap();
        assert_eq!(
            session.cookie.expose(),
            "steamLoginSecure=cookie-secret; sessionid=session-secret; \
             steamRefresh_steam=refresh-secret"
        );
        assert_eq!(session.token.expose(), "temporary-token-secret");
        let debug = format!("{session:?}");
        for secret in [
            "cookie-secret",
            "session-secret",
            "refresh-secret",
            "temporary-token-secret",
            "excluded",
        ] {
            assert!(!debug.contains(secret));
        }
        let mut cookie_only = reply();
        cookie_only.page_result_json = None;
        assert!(authenticated_session(cookie_only).is_none());
        let mut page_only = reply();
        page_only.cookies.clear();
        assert!(authenticated_session(page_only).is_none());
        let mut no_token = reply();
        no_token.page_result_json = Some(r#"{"steam_id":"76561198000000001","token":""}"#.into());
        assert!(authenticated_session(no_token).is_none());
        let mut invalid_identity = reply();
        invalid_identity.page_result_json =
            Some(r#"{"steam_id":"not-an-id","token":"secret"}"#.into());
        assert!(authenticated_session(invalid_identity).is_none());
    }

    #[test]
    fn a_login_without_remember_me_stores_only_the_session_cookies() {
        let mut reply = reply();
        reply.cookies.retain(|c| c.name != "steamRefresh_steam");
        let session = authenticated_session(reply).unwrap();
        assert_eq!(
            session.cookie.expose(),
            "steamLoginSecure=cookie-secret; sessionid=session-secret"
        );
    }
}
