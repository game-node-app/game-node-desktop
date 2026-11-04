//! One running instance per OS user, and delivery of launches and callbacks to it.
//!
//! * Linux: an exclusive `flock` on `instance.lock` elects the owner (the kernel releases it on
//!   a crash), which then binds a private Unix socket in the user's runtime directory. A socket
//!   file left by a crashed owner is stale and is replaced by the next owner.
//! * Windows: the first instance of a per-user named pipe (`FILE_FLAG_FIRST_PIPE_INSTANCE`)
//!   elects the owner; its access control admits only the current user.
//!
//! Secondary launches forward one [`Message`] to the owner (waiting briefly while it starts) and
//! exit. Owners must still validate everything: same-user delivery does not prove a sign-in
//! was initiated.

mod protocol;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::time::Duration;

pub use protocol::{ACK, MAX_FRAME, Message, NACK, ProtocolError};
#[cfg(unix)]
pub use unix::{Endpoint, Existing, Owner, OwnerGuard};
#[cfg(windows)]
pub use windows::{Endpoint, Existing, Owner, OwnerGuard};

/// How long a secondary launch waits for an owner that is still starting.
pub const FORWARD_WAIT: Duration = Duration::from_secs(5);
/// Per-connection limit for the owner to receive a complete message.
pub(crate) const READ_TIMEOUT: Duration = Duration::from_secs(2);
/// Simultaneous connections the owner handles; more are dropped.
pub(crate) const MAX_CONNECTIONS: usize = 16;

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("the per-user runtime location is unsafe or unavailable: {0}")]
    Location(String),
    #[error("could not create the single-instance endpoint: {0}")]
    Bind(std::io::Error),
    #[error("the running instance did not accept the message in time")]
    Unreachable,
    #[error("the running instance rejected the message")]
    Rejected,
}

/// The result of trying to become the single instance.
pub enum Instance {
    /// This process owns the application session.
    Owner(Owner),
    /// Another process owns it; forward to it and exit.
    Existing(Existing),
}

/// A redundant login/background start does not activate or send anything to the owner.
pub fn forward_launch(
    existing: &Existing,
    message: &Message,
    background: bool,
    wait: Duration,
) -> Result<(), IpcError> {
    if background {
        Ok(())
    } else {
        existing.forward(message, wait)
    }
}

/// Becomes the owner, or finds the existing one. Call before any database access.
///
/// `runtime` hosts the owner's IPC server; it must be provided (and entered) because the Windows
/// named pipe registers with Tokio's reactor when created.
pub fn acquire(
    endpoint: &Endpoint,
    runtime: &tokio::runtime::Handle,
) -> Result<Instance, IpcError> {
    endpoint.acquire(runtime)
}

#[cfg(test)]
mod tests;
