//! Versioned transport contract between the Tauri host and external plugins.
//!
//! This crate contains only generated protocol types and has no dependency on the
//! application Engine, platform adapters, or Tauri.

pub mod proto {
    tonic::include_proto!("gamenode.plugin.v1");
}

pub const API_MAJOR: u32 = 1;
pub const API_MINOR: u32 = 1;
