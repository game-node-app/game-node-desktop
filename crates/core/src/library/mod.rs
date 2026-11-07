//! The game library: what the provider scans found, how entries are matched to GameNode games,
//! and the view state (filter, search, sort, selection) the window shows.
//!
//! Like the auth state machine, everything here is pure: inputs in, effects out, no I/O.

pub mod actions;
mod add_dialog;
mod add_run;
pub mod cover;
mod cover_queue;
pub mod favorites;
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

pub use actions::{Actions, Availability, Reason, Why};
pub use add_dialog::{
    AddDialogView, AddProgress, AddProgressView, AddTarget, CollectionOption, CollectionsView,
};
pub use add_run::{AddItem, AddOutcome, AddOutcomeItem, AddSummary};
pub use launch::{
    LaunchError, LaunchKind, LaunchState, LaunchStateView, LaunchStatus, open_action,
};
pub use layout::{
    AddedFilter, AvailabilityFilter, DAY_SECONDS, FIRST_RELEASE_DECADE, LastPlayedFilter,
    LibraryFilters, LibraryLayout, MatchFilter, PlaytimeFilter, ProviderFilter, ReleaseFilter,
    SidebarSelection, Sort, SortDirection, SortField, SortKey, SortOrder, ViewMode, YEAR_SECONDS,
};
pub use machine::{Access, Effect, Input, LibraryMachine, ScanOutcome};
pub use match_dialog::{
    CandidateOption, MatchDialogView, MatchOption, SearchFailure, SearchResultItem,
};
pub use matching::{MatchSource, MatchStatus};
pub use membership::{EntryStatus, GameStatus, Membership, StandingView};
pub use query::{ProviderCount, Row};
pub use selection::{Direction, SelectMode, Selection};
pub use store::{
    AutoMatch, CatalogError, CatalogRecord, CatalogUpdate, ChosenMatch, FavoriteRecord, GameRecord,
    LastAccount, LibraryCache, LibraryStore, ProviderScanState, StoreAccount, StoredEntry,
    entry_key,
};
pub use view::{DetailView, EmptyState, LibraryView};
