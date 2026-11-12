//! GameNode Desktop platform adapters for Windows and Linux.
//!
//! Implements the `gamenode-core` ports that need the OS: per-user data locations, the SQLite
//! database and its migrations, credential storage, single-instance IPC and the system browser.

pub mod archive;
pub mod backups;
pub mod browser;
pub mod covers;
pub mod credentials;
pub mod db;
pub mod discovery;
pub mod display;
pub mod download;
pub mod host;
pub mod installations;
pub mod ipc;
pub mod library_db;
pub mod local_game_db;
pub mod manifest_cache;
pub mod paths;
pub mod playtime_db;
pub mod plugin_environment;
pub mod plugin_packages;
pub mod plugin_registry;
pub mod ports;
pub mod ports_db;
pub mod processes;
pub mod random;
pub mod storage;
pub mod tools;
pub mod tools_db;
pub mod uri;

#[cfg(windows)]
pub mod autostart;
