//! Play and Install: build the provider's URI for an entry and hand it to the OS.
//!
//! Only the URI the provider's own template produces is ever opened, and the template keeps its
//! literal scheme (see [`crate::provider::Template`]). Nothing here starts an executable.

use crate::ports::{UriError, UriOpener};
use crate::provider::{ActionError, FillError, Provider, Record};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchKind {
    Play,
    Install,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchStatus {
    pub entry_key: String,
    pub kind: LaunchKind,
    pub state: LaunchState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchState {
    Opening,
    Failed(LaunchError),
}

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

#[cfg(test)]
mod tests;
