//! Play and Install: build the provider's URI for an entry and hand it to the OS.
//!
//! Only the URI the provider's own template produces is ever opened, and the template keeps its
//! literal scheme (see [`crate::provider::Template`]). Nothing here starts an executable.

use serde::{Deserialize, Serialize};

use crate::ports::{UriError, UriOpener};
use crate::provider::{ActionError, FillError, Provider, Record};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LaunchKind {
    Play,
    Install,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LaunchError {
    /// The provider declares no such action.
    #[error("the provider does not support this action")]
    Unsupported,
    /// The entry lacks data the provider's rule needs.
    #[error("the game is missing data the provider needs")]
    Incomplete,
    /// The store client is not installed or registered.
    #[error("the store application is not available")]
    NoHandler,
    #[error("the link could not be opened")]
    Failed,
}

/// What the detail pane shows about the last Play or Install: it is "opening" until the operating
/// system has accepted the link, then cleared on success or shown as an error.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchStatus {
    pub entry_key: String,
    pub kind: LaunchKind,
    pub state: LaunchState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LaunchState {
    Opening,
    Failed(LaunchError),
}

pub type LaunchStateView = LaunchState;

/// Opens the provider's launch or install URI for `record` through `opener`.
pub fn open_action(
    opener: &dyn UriOpener,
    provider: &Provider,
    record: &Record,
    kind: LaunchKind,
) -> Result<(), LaunchError> {
    let uri = match kind {
        LaunchKind::Play => provider.launch_uri(record),
        LaunchKind::Install => provider.install_uri(record),
    }
    .map_err(|error| match error {
        ActionError::Unsupported => LaunchError::Unsupported,
        ActionError::Fill(FillError::SchemeChanged) => LaunchError::Failed,
        ActionError::Fill(_) => LaunchError::Incomplete,
    })?;
    opener.open(&uri).map_err(|error| match error {
        UriError::NoHandler => LaunchError::NoHandler,
        UriError::Failed => LaunchError::Failed,
    })
}

/// Execute only the bundled manual declaration using the persisted approved registration.
/// The host revalidates the executable and folder before starting it, on a worker.
pub fn launch_manual(
    host: &dyn crate::drm_free::InstallationHost,
    provider: &Provider,
    record: &Record,
    registration: &crate::drm_free::Registration,
    kind: LaunchKind,
) -> Result<(), LaunchError> {
    if !provider.is_manual_executable() || kind != LaunchKind::Play {
        return Err(LaunchError::Unsupported);
    }
    if registration.removed || !registration.available || record.id != registration.id {
        return Err(LaunchError::Incomplete);
    }
    host.launch(registration).map_err(|error| match error {
        crate::drm_free::LocalError::InvalidTarget => LaunchError::Incomplete,
        crate::drm_free::LocalError::Unsupported => LaunchError::Unsupported,
        _ => LaunchError::Failed,
    })
}

#[cfg(test)]
mod tests;
