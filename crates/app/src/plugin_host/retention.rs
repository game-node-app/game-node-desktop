//! One keep-or-delete choice for a plugin's durable shared records.
//!
//! The same choice is used by single-plugin and bundle uninstall. Keeping makes the rows inert
//! under the original owner; deleting removes only that owner's rows. Applying it is host code:
//! a plugin RPC never reaches these types, and no physical path is included in the outcome.

use std::future::Future;
use std::pin::Pin;

/// What an uninstall does with a producer's durable shared records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataRetention {
    /// Keep the records on disk but make them inert under the original owner.
    Keep,
    /// Delete the records and report the bytes actually reclaimed.
    Delete,
}

impl DataRetention {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keep => "kept",
            Self::Delete => "deleted",
        }
    }
}

/// The durable part of one uninstall or reviewed reuse decision. No physical path is included.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableRetentionOutcome {
    pub plugin_id: String,
    /// `kept` or `deleted`, matching the retention choice the host applied.
    pub data_retention: &'static str,
    pub archived_records: u64,
    pub removed_records: u64,
    pub reclaimed_bytes: u64,
}

/// Applies the host retention choice to one plugin identity. Implemented by the lifecycle; used
/// by both single-plugin and bundle uninstall so they cannot drift.
pub trait DurableRetentionSink: Send + Sync {
    fn apply<'a>(
        &'a self,
        plugin_id: &'a str,
        retention: DataRetention,
    ) -> Pin<Box<dyn Future<Output = Result<DurableRetentionOutcome, String>> + Send + 'a>>;
}
