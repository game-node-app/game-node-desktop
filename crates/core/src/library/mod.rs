//! The game library: what the provider scans found, how entries are matched to GameNode games,
//! and the view state (filter, search, sort, selection) the window shows.
//!
//! Like the auth state machine, everything here is pure: inputs in, effects out, no I/O.

pub mod actions;
mod add_dialog;
mod add_run;
mod batch;
mod catalog;
pub mod cover;
mod cover_queue;
pub mod custom_launch;
pub mod favorites;
pub mod launch;
pub mod launch_plan;
pub mod layout;
mod lookup;
pub mod machine;
pub mod match_dialog;
pub mod matching;
pub mod membership;
pub mod organization;
pub mod plugin_match;
pub mod query;
pub mod runtime;
pub mod selection;
pub mod store;
pub mod suggest;
pub mod target;
pub mod view;
pub use actions::{Actions, Availability, Reason, Why};
pub use add_dialog::{
    AddDialogView, AddMode, AddProgress, AddProgressView, AddTarget, AddTargetView,
    CollectionOption, CollectionsView,
};
pub use add_run::{
    AddItem, AddItemView, AddOutcome, AddOutcomeItem, AddOutcomeKind, AddSummary, AddSummaryView,
    CreatedEntry,
};
pub use batch::{CollectionRunView, StatusRunView, UndoAddView, UndoState};
pub use catalog::{CatalogResult, CatalogStatus, CatalogView};
pub use custom_launch::{CustomLaunchMutation, CustomLaunchRecipe};
pub use launch::{
    LaunchError, LaunchKind, LaunchState, LaunchStateView, LaunchStatus, open_action,
};
pub use launch_plan::{
    LAUNCH_PLAN_VERSION, LaunchPlan, LaunchPlanComponent, LaunchPlanError, LaunchPlanResource,
    LaunchPlanTarget,
};
pub use layout::{
    AddedFilter, AvailabilityFilter, DAY_SECONDS, FIRST_RELEASE_DECADE, GameNodeStatusFilter,
    GroupBy, LastPlayedFilter, LibraryFilters, LibraryLayout, LibraryScope, MatchFilterValue,
    PlayedFilter, PlaytimeThreshold, ProviderFilter, RECENT_DAYS, ReleaseFilter, SidebarSelection,
    Sort, SortDirection, SortField, SortKey, SortOrder, ViewMode, YEAR_SECONDS,
};
pub use machine::{Access, Effect, Input, LibraryMachine, ScanOutcome};
pub use match_dialog::{
    CandidateOption, MatchDialogView, MatchOption, SearchFailure, SearchResultItem,
};
pub use matching::{MatchSource, MatchStatus};
pub use membership::{EntryStatus, GameStatus, Membership, StandingView};
pub use organization::{
    HiddenNotice, SavedView, SavedViewNotice, SavedViewPayload, ViewError, ViewWriteError,
    ViewWriteOperation, ViewWriteState, ViewWriteView,
};
pub use plugin_match::{MAX_OFFERED_GAMES, PluginMatch};
pub use query::{
    AddedCounts, AvailabilityCounts, CollectionCount, FilterCounts, GameNodeStatusCounts,
    LastPlayedCounts, MatchStateCounts, PlayedCounts, PlaytimeCounts, ProviderCount, ReleaseCount,
    Row, RowGameNode, RowGroup, SavedViewCount, ScopeCounts, StoreCount, TitleSource,
    TitleSourceKind,
};
pub use runtime::{
    ApprovedFileReference, RuntimeArgumentTemplate, RuntimeContentBinding, RuntimeMutation,
    RuntimeResource,
};
pub use selection::{Direction, SelectMode, Selection};
pub use store::{
    AccountLibrary, ArtworkOverride, ArtworkWrite, AutoMatch, CatalogError, CatalogRecord,
    CatalogUpdate, ChoiceSource, ChosenMatch, DisplayOverride, DisplayWrite, FavoriteRecord,
    GameRecord, LastAccount, LibraryCache, LibraryStore, ProviderScanState, StoreAccount,
    StoredEntry, TargetApproval, entry_key,
};
pub use suggest::{SuggestionTone, SuggestionView};
pub use target::{DetectedTarget, DetectedTargetView, TargetSpec};
pub use view::{
    DetailView, EmptyState, LibraryView, PaletteEntry, ScanProgress, ScanProgressState,
};
