//! Versioned transport contract between the Tauri host and external plugins.
//!
//! This crate contains only generated protocol types and has no dependency on the
//! application Engine, platform adapters, or Tauri.

pub mod proto {
    tonic::include_proto!("gamenode.plugin.v1");
}
pub use prost;
pub use prost_types;

pub mod capability;
pub mod catalog;
pub mod common_data;
pub mod extensions;
pub mod login;
pub mod models;
pub mod settings;
mod wire;

pub const API_MAJOR: u32 = 1;
pub const API_MINOR: u32 = 4;
/// Last minor understood by the pre-1.4 single-minor handshake.
pub const LEGACY_API_MINOR: u32 = 3;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StatusKind {
    #[error("permission denied")]
    Denied,
    #[error("authentication failed")]
    Unauthenticated,
    #[error("operation is stale")]
    Stale,
    #[error("feature is unsupported")]
    Unsupported,
    #[error("request is invalid")]
    Invalid,
    #[error("service is unavailable")]
    Unavailable,
    #[error("protocol failure")]
    Protocol,
}

/// Maps gRPC status codes without exposing transport messages that could carry
/// request data. Structured denial metadata is decoded separately by clients.
pub fn classify_status(code: tonic::Code) -> StatusKind {
    match code {
        tonic::Code::PermissionDenied => StatusKind::Denied,
        tonic::Code::Unauthenticated => StatusKind::Unauthenticated,
        tonic::Code::FailedPrecondition | tonic::Code::Cancelled => StatusKind::Stale,
        tonic::Code::Unimplemented => StatusKind::Unsupported,
        tonic::Code::InvalidArgument => StatusKind::Invalid,
        tonic::Code::Unavailable | tonic::Code::DeadlineExceeded => StatusKind::Unavailable,
        _ => StatusKind::Protocol,
    }
}
