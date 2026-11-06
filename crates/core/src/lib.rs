//! GameNode Desktop core.
//!
//! Owns application state (settings, authentication state transitions, the game library, commands
//! and snapshots), the provider runtime (manifests, scanners, launch templates) and the adapter
//! interfaces they need. It never touches the OS, the database file, the network or the UI:
//! `gamenode-platform` and `gamenode-api` implement the [`ports`], and `gamenode-app` wires
//! everything together.

pub mod auth;
pub mod callback;
pub mod credential;
pub mod drm_free;
pub mod game_api;
pub mod geometry;
pub mod library;
pub mod notice;
pub mod playtime;
pub mod ports;
pub mod provider;
pub mod settings;
pub mod snapshot;
pub mod sync;
pub mod tools;
pub mod tracker;

pub use auth::{AuthMachine, AuthState, Effect, Input, SessionError, SignInFailure, User};
pub use callback::{CallbackError, ParsedCallback, Provider};
pub use credential::{SessionCredential, StorageMethod};
pub use notice::Notice;
pub use settings::{RendererPreference, Settings, WindowGeometry};
pub use snapshot::{AuthView, Snapshot};
pub use tools::{
    ArchiveFormat, InstallStrategy, ProxyDllName, ToolChannel, ToolDescriptor, ToolId,
    ToolInstallState, ToolInstallation, ToolStore, ToolsView,
};
