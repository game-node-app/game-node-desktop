//! GameNode Desktop platform adapters for Windows and Linux.
//!
//! Implements the `gamenode-core` ports that need the OS: per-user data locations, the SQLite
//! database and its migrations, credential storage, single-instance IPC and the system browser.

pub mod browser;
pub mod covers;
pub mod credentials;
pub mod db;
pub mod display;
pub mod host;
pub mod ipc;
pub mod library_db;
pub mod paths;
pub mod playtime_db;
pub mod processes;
pub mod random;
pub mod uri;

#[cfg(windows)]
pub mod autostart;
