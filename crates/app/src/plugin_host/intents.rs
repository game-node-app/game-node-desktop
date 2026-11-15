//! Host-issued user intents (task 10.10, design section 20).
//!
//! `GamesRequestLaunch` and `GamesRequestInstall` execute nothing unless the user explicitly
//! wanted it. That is host state, never a claim the plugin makes, and it comes in two forms:
//!
//! * a **pre-issued intent**: an opaque, unguessable id the host creates from a user action in
//!   Core and hands to the plugin it concerns. It is bound to the plugin and publisher, the
//!   profile epoch, the game and installation, the operation and (for launch) the detected-target
//!   revision; it is single use and valid for [`INTENT_TTL`];
//! * an **on-request confirmation**: with no id, the host asks the user through a [`Prompter`]
//!   (a dialog), which may also offer to remember the answer for that plugin and game
//!   (`plugin_storage::IntentApproval`, kept in the plugin's own store and revoked from the
//!   settings).
//!
//! A refusal never says which part of an id was wrong. Nothing here starts a game: the caller
//! checks the intent and then runs the operation.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tonic::Status;

/// How long a pre-issued intent is valid.
pub const INTENT_TTL: Duration = Duration::from_secs(60);
/// Pre-issued intents the host holds at once; the oldest go first.
pub const MAX_ISSUED: usize = 256;
/// How long the dialog may stay unanswered before the request fails (the limit of folder
/// approvals).
pub const CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Confirmations one plugin may have waiting for the user at once.
pub const MAX_PENDING_PER_PLUGIN: usize = 4;

/// What an intent allows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Operation {
    Launch,
    CustomLaunch,
    Install,
    BackupRestore,
}

impl Operation {
    /// The word stored with a remembered confirmation and shown in the dialog.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Launch => "launch",
            Self::CustomLaunch => "custom_launch",
            Self::Install => "install",
            Self::BackupRestore => "backup_restore",
        }
    }
}

/// Everything an intent is bound to. A different value in any field is a different request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub plugin_id: String,
    pub publisher_fingerprint: String,
    pub profile_epoch: u64,
    pub game_id: String,
    pub installation_id: String,
    pub operation: Operation,
    /// The detected-target revision a launch was requested at; 0 for an install.
    pub target_revision: u64,
}

struct Issued {
    binding: Binding,
    at: Instant,
}

/// What the dialog shows. `game_name` comes from the provider's library entry and is untrusted
/// text: show it as plain text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmationRequest {
    pub plugin_id: String,
    pub publisher_fingerprint: String,
    pub game_name: String,
    pub operation: Operation,
    /// Host-resolved restore paths and snapshot identity; never a plugin explanation.
    pub details: Vec<String>,
}

/// The user's answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Answer {
    pub approved: bool,
    /// Only meaningful when approved: remember it for this plugin and game.
    pub remember: bool,
}

/// The user's side of a confirmation. The host calls it off the UI thread, and a call that takes
/// longer than [`CONFIRMATION_TIMEOUT`] counts as declined.
#[async_trait]
pub trait Prompter: Send + Sync {
    async fn confirm(&self, request: ConfirmationRequest) -> Result<Answer, Status>;
}

/// How a confirmation ended without an answer being given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Approved { remember: bool },
    Declined,
    TimedOut,
}

type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

/// The host's intent table and the port to the user. Shared by every plugin session.
pub struct Intents {
    issued: Mutex<HashMap<String, Issued>>,
    pending: Mutex<HashSet<PendingKey>>,
    prompter: Mutex<Option<Arc<dyn Prompter>>>,
    clock: Clock,
    timeout: Duration,
}

impl Default for Intents {
    fn default() -> Self {
        Self {
            issued: Mutex::default(),
            pending: Mutex::default(),
            prompter: Mutex::default(),
            clock: Arc::new(Instant::now),
            timeout: CONFIRMATION_TIMEOUT,
        }
    }
}

fn invalid() -> Status {
    // One answer for every cause, so an id cannot be probed for what is wrong with it.
    Status::failed_precondition(
        "the user intent is missing, expired, already used, or not for this request",
    )
}

/// Who is waiting: plugin, publisher, game, installation and operation.
type PendingKey = (String, String, String, String, Operation);

/// Releases a pending-confirmation slot when dropped.
pub struct Pending {
    intents: Arc<Intents>,
    key: PendingKey,
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.intents
            .pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.key);
    }
}

impl Intents {
    #[cfg(test)]
    pub(crate) fn with_clock(
        mut self,
        clock: impl Fn() -> Instant + Send + Sync + 'static,
    ) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Connects the user's side. Until it is set, a request with no pre-issued intent cannot be
    /// confirmed and fails with `FAILED_PRECONDITION`.
    pub fn set_prompter(&self, prompter: Arc<dyn Prompter>) {
        *self.prompter.lock().unwrap_or_else(|p| p.into_inner()) = Some(prompter);
    }

    /// Creates an intent for `binding`, from an explicit user action in Core. The id is for the
    /// plugin the binding names; nothing else can redeem it.
    pub fn issue(&self, binding: Binding) -> Result<String, Status> {
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| Status::internal("no source of randomness"))?;
        let id = format!(
            "intent-{}",
            bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        );
        let now = (self.clock)();
        let mut issued = self.issued.lock().unwrap_or_else(|p| p.into_inner());
        issued.retain(|_, intent| now.duration_since(intent.at) < INTENT_TTL);
        if issued.len() >= MAX_ISSUED
            && let Some(oldest) = issued
                .iter()
                .min_by_key(|(_, intent)| intent.at)
                .map(|(id, _)| id.clone())
        {
            issued.remove(&oldest);
        }
        issued.insert(id.clone(), Issued { binding, at: now });
        Ok(id)
    }

    /// Spends `id` for a request bound to `binding`. Only a match, once, within its lifetime.
    /// A failed attempt spends nothing.
    pub fn redeem(&self, binding: &Binding, id: &str) -> Result<(), Status> {
        let now = (self.clock)();
        let mut issued = self.issued.lock().unwrap_or_else(|p| p.into_inner());
        issued.retain(|_, intent| now.duration_since(intent.at) < INTENT_TTL);
        match issued.get(id) {
            Some(intent) if intent.binding == *binding => {
                issued.remove(id);
                Ok(())
            }
            _ => Err(invalid()),
        }
    }

    /// Claims the right to ask the user about this game, or refuses when the plugin already has
    /// a confirmation waiting for it or too many waiting.
    pub fn begin_pending(self: &Arc<Self>, binding: &Binding) -> Result<Pending, Status> {
        let key = (
            binding.plugin_id.clone(),
            binding.publisher_fingerprint.clone(),
            binding.game_id.clone(),
            binding.installation_id.clone(),
            binding.operation,
        );
        let mut pending = self.pending.lock().unwrap_or_else(|p| p.into_inner());
        if pending.contains(&key) {
            return Err(Status::failed_precondition(
                "a confirmation for this game is already waiting for the user",
            ));
        }
        let own = pending
            .iter()
            .filter(|(plugin, publisher, ..)| {
                *plugin == binding.plugin_id && *publisher == binding.publisher_fingerprint
            })
            .count();
        if own >= MAX_PENDING_PER_PLUGIN {
            return Err(Status::resource_exhausted(
                "too many confirmations are waiting for the user for this plugin",
            ));
        }
        pending.insert(key.clone());
        Ok(Pending {
            intents: Arc::clone(self),
            key,
        })
    }

    /// Whether this build can ask the user at all.
    pub fn can_confirm(&self) -> bool {
        self.prompter
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
    }

    /// Asks the user. `FAILED_PRECONDITION` when this build has no dialog.
    pub async fn confirm(&self, request: ConfirmationRequest) -> Result<Verdict, Status> {
        let prompter = self
            .prompter
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
            .ok_or_else(|| {
                Status::failed_precondition("this GameNode build cannot ask for confirmations yet")
            })?;
        match tokio::time::timeout(self.timeout, prompter.confirm(request)).await {
            Err(_) => Ok(Verdict::TimedOut),
            Ok(Err(status)) => Err(status),
            Ok(Ok(answer)) if answer.approved => Ok(Verdict::Approved {
                remember: answer.remember,
            }),
            Ok(Ok(_)) => Ok(Verdict::Declined),
        }
    }
}

#[cfg(test)]
mod tests;
