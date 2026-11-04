//! The game library: what the provider scans found, how entries are matched to GameNode games,
//! and the view state (filter, search, sort, selection) the window shows.
//!
//! Like the auth state machine, everything here is pure: inputs in, effects out, no I/O.

pub mod actions;
mod add_dialog;
mod add_run;
pub mod cover;
mod cover_queue;
pub mod launch;
pub mod layout;
mod lookup;
pub mod machine;
pub mod match_dialog;
pub mod matching;
pub mod membership;
pub mod query;
pub mod selection;
pub mod store;
pub mod view;

pub use actions::{Actions, Availability, Reason};
pub use add_dialog::{AddDialogView, AddProgress, AddTarget, CollectionOption, CollectionsView};
pub use add_run::{AddItem, AddOutcome, AddSummary};
pub use launch::{LaunchError, LaunchKind, LaunchState, LaunchStatus, open_action};
pub use layout::{LibraryLayout, SidebarSelection, Sort, SortDirection, SortKey, ViewMode};
pub use machine::{Access, Effect, Input, LibraryMachine, ScanOutcome};
pub use match_dialog::{MatchDialogView, MatchOption, SearchFailure};
pub use matching::{MatchSource, MatchStatus};
pub use membership::{EntryStatus, Membership};
pub use query::{ProviderCount, Row};
pub use selection::{Direction, SelectMode, Selection};
pub use store::{
    AutoMatch, ChosenMatch, GameRecord, LastAccount, LibraryCache, LibraryStore, ProviderScanState,
    StoredEntry, entry_key,
};
pub use view::{DetailView, EmptyState, LibraryView};
