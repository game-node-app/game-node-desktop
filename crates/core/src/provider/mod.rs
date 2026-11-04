//! Game sources described as data.
//!
//! A provider is a TOML [`manifest`] (what to scan, how to launch and install, how to match).
//! The [`registry`] validates manifests and ties each to a native scanner. Scanners turn what is
//! on disk into plain [`record`]s using only the [`HostFiles`](crate::ports::HostFiles) port, and
//! launch and install are URI [`template`]s filled from those records. Nothing here touches the
//! OS, so every scanner is tested with an in-memory file system.

pub mod manifest;
pub mod memory;
pub mod path;
pub mod record;
pub mod registry;
pub mod steam;
pub mod template;

pub use manifest::{Manifest, Platform};
pub use record::{RawRecord, Record};
pub use registry::{
    ActionError, Diagnostic, LoadedProviders, Provider, ScanError, ScanInput, load,
};
pub use template::{FillError, Template, TemplateError};
