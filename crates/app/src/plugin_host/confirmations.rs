//! The application's side of plugin confirmations (task 10.10): the bridge between the host's
//! [`Prompter`] port and the window, and the settings list of confirmations the user asked the
//! host to remember.
//!
//! The window is told what to ask with the `plugin:confirmation` event and answers with the
//! `plugin_confirmation_answer` command. A confirmation that stops waiting without an answer (the
//! plugin withdrew the request, the time ran out) is closed with `plugin:confirmation-closed`.
//! Everything in the event except the ids the host made is text the plugin or its provider
//! supplied: the window shows it as plain text.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::oneshot;
use tonic::Status;

use super::intents::{Answer, ConfirmationRequest, Prompter};
use super::profile::PluginProfiles;
use super::storage::PluginStorage;

/// The event that asks the window to show a confirmation.
pub const EVENT: &str = "plugin:confirmation";
/// The event that tells the window a confirmation is no longer waiting.
pub const CLOSED_EVENT: &str = "plugin:confirmation-closed";

/// What the window shows. `id` is the host's; the rest identifies the plugin the host
/// authenticated and the game its provider named.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginConfirmation {
    pub id: String,
    pub plugin_id: String,
    pub publisher_fingerprint: String,
    pub game_name: String,
    /// `launch` or `install`.
    pub operation: String,
    /// How long the host waits for the answer.
    pub timeout_seconds: u64,
}

/// What the bridge tells the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Signal {
    Open(PluginConfirmation),
    Closed(String),
}

/// Delivers a signal to the window. `false` when there is no window to receive it.
pub type Emit = Arc<dyn Fn(Signal) -> bool + Send + Sync>;

/// Asks the user through the window.
pub struct ConfirmationBridge {
    emit: Emit,
    waiting: Mutex<HashMap<String, oneshot::Sender<Answer>>>,
}

/// Forgets a confirmation that stopped waiting, and closes it in the window.
struct Forget<'a> {
    bridge: &'a ConfirmationBridge,
    id: String,
}

impl Drop for Forget<'_> {
    fn drop(&mut self) {
        let waiting = self
            .bridge
            .waiting
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.id)
            .is_some();
        if waiting {
            (self.bridge.emit)(Signal::Closed(self.id.clone()));
        }
    }
}

impl ConfirmationBridge {
    pub fn new(emit: Emit) -> Self {
        Self {
            emit,
            waiting: Mutex::default(),
        }
    }

    /// Delivers the user's answer to the confirmation `id`. `false` when it is not waiting any
    /// more (answered already, withdrawn or timed out), so a late or repeated answer does nothing.
    pub fn answer(&self, id: &str, approved: bool, remember: bool) -> bool {
        let sender = self
            .waiting
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(id);
        sender.is_some_and(|sender| sender.send(Answer { approved, remember }).is_ok())
    }
}

#[async_trait]
impl Prompter for ConfirmationBridge {
    async fn confirm(&self, request: ConfirmationRequest) -> Result<Answer, Status> {
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| Status::internal("no source of randomness"))?;
        let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let (send, receive) = oneshot::channel();
        self.waiting
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id.clone(), send);
        // From here on, however this call ends, the confirmation stops waiting.
        let _forget = Forget {
            bridge: self,
            id: id.clone(),
        };
        let shown = (self.emit)(Signal::Open(PluginConfirmation {
            id,
            plugin_id: request.plugin_id,
            publisher_fingerprint: request.publisher_fingerprint,
            game_name: request.game_name,
            operation: request.operation.as_str().to_owned(),
            timeout_seconds: super::intents::CONFIRMATION_TIMEOUT.as_secs(),
        }));
        if !shown {
            return Err(Status::unavailable("there is no window to ask the user in"));
        }
        receive
            .await
            .map_err(|_| Status::unavailable("the confirmation was closed"))
    }
}

/// One confirmation the user asked the host to remember, as the settings list shows it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RememberedConfirmation {
    pub id: String,
    pub game_name: String,
    /// `launch` or `install`.
    pub operation: String,
    /// Unix seconds.
    pub approved_at: i64,
}

/// The settings view of remembered confirmations, read and revoked on the host's own behalf.
pub struct PluginApprovals {
    storage: Arc<PluginStorage>,
    profiles: Arc<PluginProfiles>,
}

impl PluginApprovals {
    pub fn new(storage: Arc<PluginStorage>, profiles: Arc<PluginProfiles>) -> Self {
        Self { storage, profiles }
    }

    fn describe(status: Status) -> String {
        status.message().to_owned()
    }

    /// What the user allowed `plugin_id` (of the publisher `publisher_fingerprint`) to start
    /// without asking, in the active profile.
    pub async fn list(
        &self,
        plugin_id: &str,
        publisher_fingerprint: &str,
    ) -> Result<Vec<RememberedConfirmation>, String> {
        let resolver = self
            .profiles
            .resolver(plugin_id, publisher_fingerprint, None)
            .map_err(Self::describe)?;
        let approvals = self
            .storage
            .run_for_host(&resolver, self.profiles.epoch(), |namespace| {
                namespace.intent_approvals()
            })
            .await
            .map_err(Self::describe)?;
        Ok(approvals
            .into_iter()
            .map(|approval| RememberedConfirmation {
                id: approval.id,
                game_name: approval.game_name,
                operation: approval.operation,
                approved_at: approval.approved_at,
            })
            .collect())
    }

    /// Forgets one remembered confirmation: the next request asks the user again. `false` when
    /// there was none with that id.
    pub async fn revoke(
        &self,
        plugin_id: &str,
        publisher_fingerprint: &str,
        id: &str,
    ) -> Result<bool, String> {
        let resolver = self
            .profiles
            .resolver(plugin_id, publisher_fingerprint, None)
            .map_err(Self::describe)?;
        let id = id.to_owned();
        self.storage
            .run_for_host(&resolver, self.profiles.epoch(), move |namespace| {
                namespace.revoke_intent_approval(&id)
            })
            .await
            .map_err(Self::describe)
    }
}

#[cfg(test)]
mod tests;
