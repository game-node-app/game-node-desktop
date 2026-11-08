//! Versioned transport contract between the Tauri host and external plugins.
//!
//! This crate contains only generated protocol types and has no dependency on the
//! application Engine, platform adapters, or Tauri.

pub mod proto {
    tonic::include_proto!("gamenode.plugin.v1");
}

pub mod capability;
pub mod catalog;
pub mod login;
pub mod models;
pub mod settings;
mod wire;

pub const API_MAJOR: u32 = 1;
pub const API_MINOR: u32 = 3;
pub const MIN_API_MINOR: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("invalid public plugin value")]
pub struct WireError;

// Secret-bearing generated messages must never derive an exposing Debug impl.
macro_rules! redacted_debug {
    ($($ty:ty),+ $(,)?) => {$ (
        impl std::fmt::Debug for $ty {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(concat!(stringify!($ty), "(<redacted>)"))
            }
        }
    )+ };
}

redacted_debug!(
    proto::OpenSessionRequest,
    proto::SubmitDiscoveryRequest,
    proto::ServiceContext,
    proto::SecretRequest,
    proto::SecretReply,
    proto::secret_reply::Outcome,
    proto::LoginCookie,
    proto::LoginReply,
    proto::SettingsValue,
    proto::settings_value::Value,
);
