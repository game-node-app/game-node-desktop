//! SDK-backed ports with immutable per-operation authority. The synchronous storage ports are
//! called only by the account service's blocking workers; login controls are dispatched on a
//! worker by the runtime.

use crate::account::{EpicLoginOutcome, EpicLoginResult, EpicLoginWindow};
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
        // Epic keeps only its refresh token, under the host's logical `session` key.
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

/// Shared only to route focus/cancel to the current handle. It does not store page data,
/// credentials or a replaceable authorization context.
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

    async fn collect(&self, handle: &str) -> EpicLoginResult {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(600);
        loop {
            let reply =
                tokio::time::timeout_at(deadline, self.host.login(LoginOperation::Read, handle))
                    .await;
            match reply {
                Err(_) => return EpicLoginResult::TimedOut,
                Ok(Err(error @ (Error::Stale | Error::Denied))) => {
                    revoke(&self.validity, &error);
                    return EpicLoginResult::Cancelled;
                }
                Ok(Err(Error::Busy | Error::Unavailable)) => {}
                Ok(Err(_)) => return EpicLoginResult::Failed,
                Ok(Ok(reply)) => match LoginState::try_from(reply.state) {
                    Ok(LoginState::Open) => {
                        if let Some(code) = authorization_code(reply) {
                            return EpicLoginResult::Code(code);
                        }
                    }
                    Ok(LoginState::Cancelled) => return EpicLoginResult::Cancelled,
                    Ok(LoginState::Closed) => return EpicLoginResult::Closed,
                    Ok(LoginState::TimedOut) => return EpicLoginResult::TimedOut,
                    _ => return EpicLoginResult::Failed,
                },
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}

#[async_trait::async_trait]
impl EpicLoginWindow for SdkLogin {
    async fn run(&self) -> EpicLoginOutcome {
        let opened = match self.host.login(LoginOperation::Open, "").await {
            Ok(reply) if reply.state == LoginState::Open as i32 && !reply.handle.is_empty() => {
                reply
            }
            Err(error) => {
                revoke(&self.validity, &error);
                return EpicLoginOutcome {
                    result: EpicLoginResult::Failed,
                };
            }
            _ => {
                return EpicLoginOutcome {
                    result: EpicLoginResult::Failed,
                };
            }
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
        EpicLoginOutcome { result }
    }

    fn focus(&self) -> bool {
        let Some(handle) = self.handle() else {
            return false;
        };
        let reply = self
            .runtime
            .block_on(self.host.login(LoginOperation::Focus, &handle))
            .map_err(|error| {
                revoke(&self.validity, &error);
            });
        reply.is_ok_and(|reply| reply.state == LoginState::Open as i32)
    }

    fn cancel(&self) -> bool {
        let Some(handle) = self.handle() else {
            return true;
        };
        let cancelled = self
            .runtime
            .block_on(self.host.login(LoginOperation::Cancel, &handle))
            .map_err(|error| {
                revoke(&self.validity, &error);
            })
            .is_ok();
        self.forget(&handle);
        cancelled
    }

    // The host owns ephemeral browser profiles and startup recovery. The plugin must never know
    // or inspect the host's private profile directory.
    fn cleanup_profile(&self) {}
}

fn revoke(validity: &AtomicBool, error: &Error) {
    if matches!(error, Error::Stale | Error::Denied) {
        validity.store(false, Ordering::Release);
    }
}

#[derive(serde::Deserialize)]
struct RedirectProbe {
    authorization_code: String,
}

/// The Epic login policy's probe returns only `{ "authorization_code": … }` at the redirect
/// origin. Cookies are never part of this flow, so the code is the only accepted value.
fn authorization_code(reply: LoginReply) -> Option<SessionCredential> {
    let page: RedirectProbe = serde_json::from_str(reply.page_result_json.as_deref()?).ok()?;
    SessionCredential::new(page.authorization_code.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(json: &str) -> LoginReply {
        LoginReply {
            page_result_json: Some(json.into()),
            ..Default::default()
        }
    }

    #[test]
    fn only_a_non_empty_authorization_code_is_accepted() {
        let code = authorization_code(reply(r#"{"authorization_code":"one-time-secret"}"#))
            .expect("a code");
        assert_eq!(code.expose(), "one-time-secret");
        assert!(!format!("{code:?}").contains("one-time-secret"));

        assert!(authorization_code(reply("null")).is_none());
        assert!(authorization_code(reply("not json")).is_none());
        assert!(authorization_code(reply(r#"{"authorization_code":""}"#)).is_none());
        assert!(authorization_code(reply(r#"{"authorization_code":"   "}"#)).is_none());
        assert!(authorization_code(reply(r#"{"authorizationCode":"camel"}"#)).is_none());
        assert!(authorization_code(LoginReply::default()).is_none());
    }
}
