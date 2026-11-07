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
pub mod steam;
pub mod sync;
pub mod tools;
pub mod tracker;

pub use auth::{
    AccountSummary, AuthMachine, AuthState, Effect, Input, PendingSignInView, SessionError,
    SessionRestoreFailure, SignInFailure, User,
};
pub use callback::{CallbackError, ParsedCallback, Provider};
pub use credential::{SessionCredential, StorageMethod};
pub use notice::{Notice, NoticeKind};
pub use settings::{Settings, WindowGeometry};
pub use snapshot::{AuthView, Snapshot};
pub use steam::{SteamError, SteamFamily, SteamOwnedGame, SteamSession, SteamWebApi};
pub use tools::{
    AntiCheatReport, AntiCheatSystem, AntiCheatWarningView, ArchiveFormat, InstallStrategy,
    ProxyDllName, TargetCandidate, TargetInspection, ToolCardView, ToolChannel, ToolDescriptor,
    ToolId, ToolInstallState, ToolInstallStateView, ToolInstallation, ToolProxyOptionView,
    ToolStore, ToolTargetExeView, ToolVersionView, ToolsView,
};
