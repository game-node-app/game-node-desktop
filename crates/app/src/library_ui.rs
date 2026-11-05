//! Connects the Slint library screens to the engine.
//!
//! [`install`] wires every `LibraryState` callback to a library command; [`render`] copies each
//! snapshot into the `LibraryState` properties. The mappings between core types and the
//! enums/structs of the `.slint` files are plain functions, tested without a window. No sentence
//! is built here: the screens turn the enums into localized text, and game titles, collection
//! names and store names are passed through exactly as received.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use gamenode_core::Snapshot;
use gamenode_core::library::{
    Access, Actions, AddOutcome, Availability, CollectionsView, Direction, EmptyState, EntryStatus,
    Input as LibInput, LaunchError, LaunchKind, LaunchState, LaunchStatus, LibraryView,
    MatchOption, MatchSource, MatchStatus, Membership, Reason, SearchFailure, SelectMode,
    SidebarSelection, Sort, SortDirection, SortKey, ViewMode,
};
use slint::{ComponentHandle, Image, Model, ModelRc, VecModel};

use crate::engine::Engine;
use crate::{
    AccessKind, AddError, AppWindow, CollectionItem, CollectionsKind, EmptyKind, GameItem,
    LaunchNote, LibraryState, MatchKind, OptionItem, Outcome, ProviderItem, SearchError, SortKind,
    SourceKind, Standing, StatusKind, SummaryItem, TargetItem, ViewKind, Why,
};

mod artwork;

// ---------------------------------------------------------------------------------------------
// Core -> screen mappings.

pub fn status_kind(status: EntryStatus) -> StatusKind {
    match status {
        EntryStatus::Playing => StatusKind::Playing,
        EntryStatus::Finished => StatusKind::Finished,
        EntryStatus::Planned => StatusKind::Planned,
        EntryStatus::Dropped => StatusKind::Dropped,
        EntryStatus::Ongoing => StatusKind::Ongoing,
    }
}

/// `None` for the "unknown" status, which is never something the user can pick.
pub fn entry_status(kind: StatusKind) -> Option<EntryStatus> {
    match kind {
        StatusKind::Playing => Some(EntryStatus::Playing),
        StatusKind::Finished => Some(EntryStatus::Finished),
        StatusKind::Planned => Some(EntryStatus::Planned),
        StatusKind::Dropped => Some(EntryStatus::Dropped),
        StatusKind::Ongoing => Some(EntryStatus::Ongoing),
        StatusKind::Unknown => None,
    }
}

fn optional_status(status: Option<EntryStatus>) -> StatusKind {
    status.map_or(StatusKind::Unknown, status_kind)
}

pub fn why(reason: Reason) -> Why {
    match reason {
        Reason::Running => Why::Running,
        Reason::NothingSelected => Why::NothingSelected,
        Reason::MultipleSelected => Why::MultipleSelected,
        Reason::NoLaunchRule => Why::NoLaunchRule,
        Reason::NoInstallRule => Why::NoInstallRule,
        Reason::MissingField => Why::MissingField,
        Reason::NeedsVerifiedSession => Why::NeedsVerifiedSession,
        Reason::Offline => Why::Offline,
        Reason::NotMatched => Why::NotMatched,
        Reason::NoMatchedGames => Why::NoMatchedGames,
        Reason::CheckingMembership => Why::CheckingMembership,
        Reason::MembershipUnavailable => Why::MembershipUnavailable,
        Reason::AlreadyInLibrary(_) => Why::AlreadyInLibrary,
    }
}

pub fn availability(availability: Availability) -> Why {
    match availability {
        Availability::Available => Why::Available,
        Availability::Disabled(reason) => why(reason),
    }
}

pub fn match_kind(status: &MatchStatus) -> MatchKind {
    match status {
        MatchStatus::Matched { .. } => MatchKind::Matched,
        MatchStatus::Ambiguous { .. } => MatchKind::Ambiguous,
        MatchStatus::NoGame => MatchKind::NoGame,
        MatchStatus::Unmatched => MatchKind::Unmatched,
    }
}

fn source_kind(status: &MatchStatus) -> SourceKind {
    match status {
        MatchStatus::Matched {
            source: MatchSource::Chosen,
            ..
        } => SourceKind::Chosen,
        MatchStatus::Matched {
            source: MatchSource::Automatic,
            ..
        } => SourceKind::Automatic,
        _ => SourceKind::None,
    }
}

fn access_kind(access: Access) -> AccessKind {
    match access {
        Access::Unverified => AccessKind::Unverified,
        Access::Offline => AccessKind::Offline,
        Access::Verified => AccessKind::Verified,
    }
}

fn view_kind(mode: ViewMode) -> ViewKind {
    match mode {
        ViewMode::Grid => ViewKind::Grid,
        ViewMode::List => ViewKind::List,
    }
}

fn sort_kind(key: SortKey) -> SortKind {
    match key {
        SortKey::Title => SortKind::Title,
        SortKey::Provider => SortKind::Provider,
        SortKey::MatchStatus => SortKind::MatchStatus,
    }
}

/// The note under the actions: only for the game the latest Play or Install was for.
pub fn launch_note(launch: Option<&LaunchStatus>, selected: Option<&str>) -> LaunchNote {
    let Some(launch) = launch else {
        return LaunchNote::None;
    };
    if selected != Some(launch.entry_key.as_str()) {
        return LaunchNote::None;
    }
    match (&launch.state, launch.kind) {
        (LaunchState::Opening, LaunchKind::Play) => LaunchNote::OpeningPlay,
        (LaunchState::Opening, LaunchKind::Install) => LaunchNote::OpeningInstall,
        (LaunchState::Failed(LaunchError::NoHandler), _) => LaunchNote::NoHandler,
        (LaunchState::Failed(LaunchError::Failed), _) => LaunchNote::Failed,
        (LaunchState::Failed(LaunchError::Incomplete), _) => LaunchNote::Incomplete,
        (LaunchState::Failed(LaunchError::Unsupported), _) => LaunchNote::Unsupported,
    }
}

fn standing(membership: Membership) -> (Standing, StatusKind) {
    match membership {
        Membership::NotChecked => (Standing::NotChecked, StatusKind::Unknown),
        Membership::Checking => (Standing::Checking, StatusKind::Unknown),
        Membership::InLibrary(status) => (Standing::InLibrary, optional_status(status)),
        Membership::NotInLibrary => (Standing::NotInLibrary, StatusKind::Unknown),
        Membership::Unavailable => (Standing::Unavailable, StatusKind::Unknown),
    }
}

fn search_error(error: Option<SearchFailure>) -> (SearchError, Option<Why>) {
    match error {
        None => (SearchError::None, None),
        Some(SearchFailure::Blocked(reason)) => (SearchError::Blocked, Some(why(reason))),
        Some(SearchFailure::Rejected) => (SearchError::Rejected, None),
        Some(SearchFailure::Failed) => (SearchError::Failed, None),
    }
}

fn api_error(error: gamenode_core::game_api::ApiError) -> AddError {
    match error {
        gamenode_core::game_api::ApiError::Rejected => AddError::Rejected,
        gamenode_core::game_api::ApiError::Failed => AddError::Failed,
    }
}

fn outcome(outcome: &AddOutcome) -> (Outcome, StatusKind) {
    match outcome {
        AddOutcome::Added(status) => (Outcome::Added, status_kind(*status)),
        AddOutcome::AlreadyPresent(status) => (Outcome::AlreadyPresent, optional_status(*status)),
        AddOutcome::SkippedUnmatched => (Outcome::Skipped, StatusKind::Unknown),
        AddOutcome::Failed(_) => (Outcome::Failed, StatusKind::Unknown),
    }
}

// ---------------------------------------------------------------------------------------------
// Screen -> core commands.

/// A click: Shift extends a range, Ctrl toggles, otherwise only this game is selected.
pub fn select_input(key: &str, ctrl: bool, shift: bool) -> LibInput {
    let mode = if shift {
        SelectMode::Range
    } else if ctrl {
        SelectMode::Toggle
    } else {
        SelectMode::Replace
    };
    LibInput::Select {
        key: key.to_owned(),
        mode,
    }
}

/// The direction codes the keyboard handler in `library.slint` sends.
pub fn move_input(direction: i32, extend: bool) -> Option<LibInput> {
    let direction = match direction {
        0 => Direction::Up,
        1 => Direction::Down,
        2 => Direction::Left,
        3 => Direction::Right,
        4 => Direction::Home,
        5 => Direction::End,
        _ => return None,
    };
    Some(LibInput::Move { direction, extend })
}

/// An empty id means "all games".
pub fn filter_input(provider: &str) -> LibInput {
    LibInput::SetFilter(if provider.is_empty() {
        SidebarSelection::All
    } else {
        SidebarSelection::Provider(provider.to_owned())
    })
}

pub fn sort_input(key: SortKind, descending: bool) -> LibInput {
    LibInput::SetSort(Sort {
        key: match key {
            SortKind::Title => SortKey::Title,
            SortKind::Provider => SortKey::Provider,
            SortKind::MatchStatus => SortKey::MatchStatus,
        },
        direction: if descending {
            SortDirection::Descending
        } else {
            SortDirection::Ascending
        },
    })
}

pub fn view_input(view: ViewKind) -> LibInput {
    LibInput::SetView(match view {
        ViewKind::Grid => ViewMode::Grid,
        ViewKind::List => ViewMode::List,
    })
}

/// Pane sizes arrive as logical pixels; a negative size is ignored.
pub fn width_input(width: i32, make: fn(u32) -> LibInput) -> Option<LibInput> {
    u32::try_from(width).ok().map(make)
}

pub fn columns_input(columns: i32) -> LibInput {
    LibInput::SetColumns(usize::try_from(columns).unwrap_or(1).max(1))
}

/// A game id from a dialog row; anything that is not a number is ignored.
pub fn pick_input(game_id: &str, now: i64) -> Option<LibInput> {
    game_id
        .parse()
        .ok()
        .map(|game_id| LibInput::PickMatch { game_id, now })
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

/// Connects every `LibraryState` callback to the engine.
pub fn install(ui: &AppWindow, engine: &Engine, covers_dir: PathBuf) {
    let local = ui.global::<crate::LocalGames>();
    local.set_supported(cfg!(windows));
    let e = engine.clone();
    local.on_claim_resolve(move |transfer| e.resolve_local_claim(transfer));
    let e = engine.clone();
    local.on_claim_cancel(move || e.cancel_local_claim());
    let e = engine.clone();
    local.on_add(move || e.pick_local_executable(None));
    let e = engine.clone();
    local.on_locate(move || {
        if let Some(id) = selected_local_id(&e) {
            e.pick_local_executable(Some(id));
        }
    });
    let e = engine.clone();
    local.on_remove(move || {
        if let Some(id) = selected_local_id(&e) {
            e.remove_local_game(id);
        }
    });
    let e = engine.clone();
    local.on_save(
        move |title, executable, folder, working, arguments, gameplay, launchers| {
            let snapshot = e.snapshot();
            if let Some(id) = selected_local_id(&e)
                && let Some(mut value) = snapshot
                    .local_games
                    .registrations
                    .into_iter()
                    .find(|r| r.id == id)
                && let Ok(arguments) = gamenode_core::drm_free::parse_arguments(&arguments)
            {
                value.title = title.to_string();
                value.executable = executable.to_string();
                value.root = folder.to_string();
                value.working_directory = working.to_string();
                value.arguments = arguments;
                value.gameplay = gameplay
                    .lines()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect();
                value.launchers = launchers
                    .lines()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect();
                e.edit_local_game(value);
            }
        },
    );
    let e = engine.clone();
    local.on_backup_now(move || {
        if let Some(id) = selected_local_id(&e) {
            e.backup_local_now(id);
        }
    });
    let e = engine.clone();
    local.on_backup_settings(move |automatic, destination| {
        e.set_local_backup_settings(automatic, destination.to_string())
    });
    let e = engine.clone();
    local.on_game_backup_automatic(move |automatic| {
        if let Some(id) = selected_local_id(&e) {
            e.set_local_profile_automatic(id, automatic);
        }
    });
    let e = engine.clone();
    local.on_custom_saves(move |location| {
        if let Some(id) = selected_local_id(&e) {
            e.configure_local_saves(id, location.to_string());
        }
    });
    let e = engine.clone();
    local.on_saves_pick(move |folder| {
        if let Some(id) = selected_local_id(&e) {
            e.pick_local_saves(id, folder);
        }
    });
    let e = engine.clone();
    local.on_profile_choose(move |title| {
        if let Some(id) = selected_local_id(&e) {
            e.choose_local_save_profile(id, title.to_string());
        }
    });
    let e = engine.clone();
    local.on_watch_folder(move |path| e.watch_local_folder(path.to_string()));
    let e = engine.clone();
    local.on_watch_remove(move |id| e.remove_local_watch(id.to_string()));
    let e = engine.clone();
    local.on_candidate_executable(move |fingerprint, executable| {
        e.set_local_candidate_executable(fingerprint.to_string(), executable.to_string())
    });
    let e = engine.clone();
    local.on_discovery_rescan(move || e.rescan_local_folders());
    let e = engine.clone();
    local.on_candidate_toggle(move |fingerprint| e.select_local_candidate(fingerprint.to_string()));
    let e = engine.clone();
    local.on_candidates_add(move |add| e.review_selected_local_candidates(add));
    let e = engine.clone();
    local.on_ignores_reset(move || e.reset_local_ignores());
    let e = engine.clone();
    local.on_history_load(move || {
        if let Some(id) = selected_local_id(&e) {
            e.load_local_history(id);
        }
    });
    let e = engine.clone();
    local.on_restore_preview(move |key| e.preview_local_restore(key.to_string()));
    let e = engine.clone();
    local.on_restore_confirm(move || e.confirm_local_restore());
    let state = ui.global::<LibraryState>();
    let send = |engine: &Engine, input: Option<LibInput>| {
        if let Some(input) = input {
            engine.library_input(input);
        }
    };

    // Selection and navigation.
    let e = engine.clone();
    state.on_select(move |key, ctrl, shift| e.library_input(select_input(&key, ctrl, shift)));
    let e = engine.clone();
    state.on_move(move |direction, extend| send(&e, move_input(direction, extend)));
    let e = engine.clone();
    state.on_open_details(move || e.library_input(LibInput::OpenDetails));
    let e = engine.clone();
    state.on_set_columns(move |columns| e.library_input(columns_input(columns)));

    // View, filter, search, sort and pane sizes.
    let e = engine.clone();
    state.on_set_view(move |view| e.library_input(view_input(view)));
    let e = engine.clone();
    state.on_set_filter(move |provider| e.library_input(filter_input(&provider)));
    let e = engine.clone();
    state.on_set_search(move |text| e.library_input(LibInput::SetSearch(text.to_string())));
    let e = engine.clone();
    state.on_set_sort(move |key, descending| e.library_input(sort_input(key, descending)));
    let e = engine.clone();
    state.on_sidebar_width_changed(move |width| {
        send(&e, width_input(width, LibInput::SetSidebarWidth));
    });
    let e = engine.clone();
    state.on_detail_width_changed(move |width| {
        send(&e, width_input(width, LibInput::SetDetailWidth));
    });
    let e = engine.clone();
    state.on_set_detail_open(move |open| e.library_input(LibInput::SetDetailOpen(open)));
    let e = engine.clone();
    state.on_rescan(move || e.rescan());

    // Actions on the selected game. Play and Install are only sent when the actions allow them.
    let e = engine.clone();
    state.on_play(move || launch_selected(&e, LaunchKind::Play));
    let e = engine.clone();
    state.on_install(move || launch_selected(&e, LaunchKind::Install));
    let e = engine.clone();
    state.on_dismiss_launch(move || e.dismiss_launch());
    let e = engine.clone();
    state.on_refresh_membership(move || e.library_input(LibInput::RefreshMembership));
    let e = engine.clone();
    state.on_open_match(move || e.library_input(LibInput::OpenMatch));
    let e = engine.clone();
    state.on_open_add(move || e.library_input(LibInput::OpenAdd));

    // The match dialog.
    let e = engine.clone();
    state.on_close_match(move || e.library_input(LibInput::CloseMatch));
    let e = engine.clone();
    state.on_match_query_edited(move |text| {
        e.library_input(LibInput::SetMatchQuery(text.to_string()))
    });
    let e = engine.clone();
    state.on_match_search(move || e.library_input(LibInput::SearchMatch));
    let e = engine.clone();
    state.on_pick_match(move |id| send(&e, pick_input(&id, unix_now())));
    let e = engine.clone();
    state.on_mark_no_game(move || e.library_input(LibInput::MarkNoGame { now: unix_now() }));
    let e = engine.clone();
    state.on_clear_match(move || e.library_input(LibInput::ClearMatch));

    // The Add to GameNode dialog.
    let e = engine.clone();
    state.on_close_add(move || e.library_input(LibInput::CloseAdd));
    let e = engine.clone();
    state.on_add_set_status(move |status| {
        send(&e, entry_status(status).map(LibInput::SetAddStatus));
    });
    let e = engine.clone();
    state.on_add_toggle_collection(move |id| {
        e.library_input(LibInput::ToggleAddCollection(id.to_string()));
    });
    let e = engine.clone();
    state.on_add_retry_collections(move || e.library_input(LibInput::RetryCollections));
    let e = engine.clone();
    state.on_add_confirm(move || e.library_input(LibInput::ConfirmAdd));

    let renderer = Renderer::new(covers_dir);
    renderer.attach(ui);
    RENDERER.with(|slot| *slot.borrow_mut() = Some(renderer));
}

fn launch_selected(engine: &Engine, kind: LaunchKind) {
    let Some(view) = engine.snapshot().library else {
        return;
    };
    let allowed = match kind {
        LaunchKind::Play => view.actions.play,
        LaunchKind::Install => view.actions.install,
    };
    if let (true, [key]) = (allowed.is_available(), view.selected.as_slice()) {
        engine.open_entry(key.clone(), kind);
    }
}

// ---------------------------------------------------------------------------------------------
// Snapshot -> screen.

thread_local! {
    static RENDERER: RefCell<Option<Renderer>> = const { RefCell::new(None) };
}

/// Copies the snapshot's library into the screens. Does nothing before [`install`].
pub fn render(ui: &AppWindow, snapshot: &Snapshot) {
    let local = ui.global::<crate::LocalGames>();
    local.set_busy(snapshot.local_games.busy);
    local.set_claim_open(snapshot.local_games.pending_claim.is_some());
    local.set_claim_title(
        snapshot
            .local_games
            .pending_claim
            .as_ref()
            .map(|claim| claim.store_title.clone())
            .unwrap_or_default()
            .into(),
    );

    local.set_watches(slint::ModelRc::new(slint::VecModel::from(
        snapshot
            .local_games
            .watches
            .iter()
            .filter(|w| w.enabled)
            .map(|w| crate::DiscoveryRow {
                fingerprint: w.id.clone().into(),
                folder: w.path.clone().into(),
                ..Default::default()
            })
            .collect::<Vec<_>>(),
    )));

    local.set_candidates(slint::ModelRc::new(slint::VecModel::from(
        snapshot
            .local_games
            .candidates
            .iter()
            .filter(|c| !c.ignored)
            .map(|c| crate::DiscoveryRow {
                fingerprint: c.fingerprint.clone().into(),
                title: c.title.clone().into(),
                folder: c.root.clone().into(),
                executable: c.selected_target.clone().unwrap_or_default().into(),
                picked: snapshot
                    .local_games
                    .review_selected
                    .contains(&c.fingerprint),
                resolved: c.selected_target.is_some(),
            })
            .collect::<Vec<_>>(),
    )));

    local.set_error(
        snapshot
            .local_games
            .error
            .clone()
            .unwrap_or_default()
            .into(),
    );
    let selected = snapshot.library.as_ref().and_then(|v| {
        if v.selected.len() == 1 {
            v.selected[0].strip_prefix("manual:")
        } else {
            None
        }
    });
    local.set_selected_id(selected.unwrap_or_default().into());
    local.set_match_evidence(
        selected
            .and_then(|id| {
                snapshot
                    .local_games
                    .inferences
                    .iter()
                    .find(|i| i.installation == id)
            })
            .map(|i| {
                i.evidence
                    .iter()
                    .map(|e| e.value.as_str())
                    .collect::<Vec<_>>()
                    .join(" · ")
            })
            .unwrap_or_default()
            .into(),
    );

    let job = selected.and_then(|id| {
        snapshot
            .local_games
            .jobs
            .iter()
            .filter(|j| j.installation == id)
            .max_by_key(|j| j.ready_at)
    });
    local.set_job_status(job.map_or(0, |j| match j.state {
        gamenode_core::drm_free::JobState::Queued => 1,
        gamenode_core::drm_free::JobState::Running => 2,
        gamenode_core::drm_free::JobState::Unchanged => 3,
        gamenode_core::drm_free::JobState::Successful => 4,
        gamenode_core::drm_free::JobState::Partial => 5,
        gamenode_core::drm_free::JobState::Failed => 6,
        gamenode_core::drm_free::JobState::Cancelled => 7,
    }));

    local.set_history(slint::ModelRc::new(slint::VecModel::from(
        snapshot
            .local_games
            .versions
            .iter()
            .map(|version| crate::BackupRow {
                label: format!(
                    "{} · {} bytes · {}",
                    version.label, version.bytes, version.destination
                )
                .into(),
                key: version.key().into(),
            })
            .collect::<Vec<_>>(),
    )));
    local.set_has_restore(snapshot.local_games.restore.is_some());
    local.set_restore_targets(
        snapshot
            .local_games
            .restore
            .as_ref()
            .map(|plan| plan.targets.join("\n"))
            .unwrap_or_default()
            .into(),
    );
    local.set_automatic(snapshot.local_games.settings.automatic);
    local.set_destination(snapshot.local_games.settings.destination.clone().into());
    let profile = selected.and_then(|id| {
        snapshot
            .local_games
            .profiles
            .iter()
            .rfind(|p| p.installation == id)
    });
    local.set_game_automatic(profile.is_none_or(|p| p.automatic != Some(false)));
    local.set_overlapping_saves(profile.is_some_and(|profile| {
        snapshot
            .local_games
            .profiles
            .iter()
            .filter(|other| other.installation != profile.installation)
            .any(|other| {
                profile.files.iter().any(|file| {
                    other
                        .files
                        .iter()
                        .any(|other| file.eq_ignore_ascii_case(other))
                }) || profile.registry.iter().any(|key| {
                    other
                        .registry
                        .iter()
                        .any(|other| key.eq_ignore_ascii_case(other))
                })
            })
    }));

    local.set_save_status(profile.map_or(0, |p| match p.support {
        gamenode_core::drm_free::SaveSupport::Supported => 2,
        gamenode_core::drm_free::SaveSupport::NoSaves => 3,
        gamenode_core::drm_free::SaveSupport::NeedsSelection => 1,
        gamenode_core::drm_free::SaveSupport::Unsupported => 0,
    }));

    if let Some(registration) = selected.and_then(|id| {
        snapshot
            .local_games
            .registrations
            .iter()
            .find(|r| r.id == id)
    }) {
        local.set_gameplay(registration.gameplay.join("\n").into());
        local.set_launchers(registration.launchers.join("\n").into());
        local.set_title(registration.title.clone().into());
        local.set_executable(registration.executable.clone().into());
        local.set_folder(registration.root.clone().into());
        local.set_working_directory(registration.working_directory.clone().into());
        local.set_arguments(
            gamenode_core::drm_free::format_arguments(&registration.arguments).into(),
        );
    }
    RENDERER.with(|slot| {
        if let Some(renderer) = slot.borrow_mut().as_mut() {
            renderer.render(ui, snapshot);
        }
    });
}

struct Renderer {
    covers_dir: PathBuf,
    /// Decoded covers by file name; `None` remembers a file that could not be read.
    covers: HashMap<String, Option<Image>>,
    artwork: artwork::Artworks,
    items: Rc<VecModel<GameItem>>,
    providers: Rc<VecModel<ProviderItem>>,
    candidates: Rc<VecModel<OptionItem>>,
    results: Rc<VecModel<OptionItem>>,
    targets: Rc<VecModel<TargetItem>>,
    collections: Rc<VecModel<CollectionItem>>,
    summary: Rc<VecModel<SummaryItem>>,
    match_was_open: bool,
}

/// Replaces the model's rows, touching only the rows that differ when the length is unchanged.
fn sync<T: Clone + PartialEq + 'static>(model: &VecModel<T>, rows: Vec<T>) {
    if model.row_count() == rows.len() {
        for (index, row) in rows.into_iter().enumerate() {
            if model.row_data(index).as_ref() != Some(&row) {
                model.set_row_data(index, row);
            }
        }
    } else {
        model.set_vec(rows);
    }
}

impl Renderer {
    fn new(covers_dir: PathBuf) -> Self {
        Self {
            covers_dir,
            covers: HashMap::new(),
            artwork: artwork::Artworks::default(),
            items: Rc::default(),
            providers: Rc::default(),
            candidates: Rc::default(),
            results: Rc::default(),
            targets: Rc::default(),
            collections: Rc::default(),
            summary: Rc::default(),
            match_was_open: false,
        }
    }

    fn attach(&self, ui: &AppWindow) {
        let state = ui.global::<LibraryState>();
        state.set_items(ModelRc::from(self.items.clone()));
        state.set_providers(ModelRc::from(self.providers.clone()));
        state.set_match_candidates(ModelRc::from(self.candidates.clone()));
        state.set_match_results(ModelRc::from(self.results.clone()));
        state.set_add_targets(ModelRc::from(self.targets.clone()));
        state.set_add_collections(ModelRc::from(self.collections.clone()));
        state.set_add_summary(ModelRc::from(self.summary.clone()));
    }

    fn cover(&mut self, file: Option<&str>) -> Option<Image> {
        let file = file?;
        let dir = &self.covers_dir;
        self.covers
            .entry(file.to_owned())
            .or_insert_with(|| load_cover(dir, file))
            .clone()
    }

    fn render(&mut self, ui: &AppWindow, snapshot: &Snapshot) {
        self.artwork.clear_focus();
        let state = ui.global::<LibraryState>();
        let Some(view) = &snapshot.library else {
            state.set_active(false);
            state.set_detail_valid(false);
            state.set_match_open(false);
            state.set_add_open(false);
            self.match_was_open = false;
            self.items.set_vec(vec![]);
            self.providers.set_vec(vec![]);
            return;
        };
        state.set_active(true);
        self.render_list(&state, view);
        self.render_selection(ui, &state, view, snapshot);
        self.render_match(&state, view);
        self.render_add(&state, view);
    }

    fn render_list(&mut self, state: &LibraryState<'_>, view: &LibraryView) {
        let selected: BTreeSet<&str> = view.selected.iter().map(String::as_str).collect();
        let mut focused_index = -1;
        let mut items = Vec::with_capacity(view.rows.len());
        for (index, row) in view.rows.iter().enumerate() {
            let is_focused = view.focused.as_deref() == Some(row.key.as_str());
            if is_focused {
                focused_index = i32::try_from(index).unwrap_or(-1);
            }
            let cover = self.cover(row.cover_file.as_deref());
            items.push(GameItem {
                playtime_known: row.playtime.seconds.is_some(),
                playtime_hours: row
                    .playtime
                    .seconds
                    .map_or(0, |s| i32::try_from(s / 3600).unwrap_or(i32::MAX)),
                playtime_minutes: row.playtime.seconds.map_or(0, |s| ((s % 3600) / 60) as i32),
                playtime_seconds: row.playtime.seconds.map_or(0, |s| (s % 60) as i32),
                running: row.running,
                key: row.key.as_str().into(),
                title: row.title.as_str().into(),
                provider: row.provider_name.as_str().into(),
                has_cover: cover.is_some(),
                cover: cover.unwrap_or_default(),
                kind: match_kind(&row.status),
                selected: selected.contains(row.key.as_str()),
                focused: is_focused,
            });
        }
        sync(&self.items, items);
        state.set_focused_index(focused_index);

        let layout = &view.layout;
        sync(
            &self.providers,
            view.providers
                .iter()
                .map(|provider| ProviderItem {
                    id: provider.provider.as_str().into(),
                    name: provider.name.as_str().into(),
                    count: i32::try_from(provider.count).unwrap_or(i32::MAX),
                    selected: layout.sidebar
                        == SidebarSelection::Provider(provider.provider.clone()),
                })
                .collect(),
        );
        state.set_all_selected(layout.sidebar == SidebarSelection::All);
        state.set_total(i32::try_from(view.total).unwrap_or(i32::MAX));
        state.set_view(view_kind(layout.view));
        state.set_sort(sort_kind(layout.sort.key));
        state.set_descending(layout.sort.direction == SortDirection::Descending);
        // The search box owns its text while the user types; only a mismatch with a different
        // value (never the case in practice) would need pushing, so it is left alone.
        state.set_scanning(view.scanning);
        state.set_failed_stores(view.failed_providers.join(", ").into());
        state.set_access(access_kind(view.access));
        let (empty, stores) = match &view.empty {
            None => (EmptyKind::None, String::new()),
            Some(EmptyState::NoGames { stores }) => (EmptyKind::NoGames, stores.join(", ")),
            Some(EmptyState::NothingMatches) => (EmptyKind::NothingMatches, String::new()),
        };
        state.set_empty(empty);
        state.set_stores(stores.into());
        state.set_sidebar_width(i32::try_from(layout.sidebar_width).unwrap_or(220));
        state.set_detail_width(i32::try_from(layout.detail_width).unwrap_or(340));
        state.set_detail_open(layout.detail_open);
    }

    fn render_selection(
        &mut self,
        ui: &AppWindow,
        state: &LibraryState<'_>,
        view: &LibraryView,
        snapshot: &Snapshot,
    ) {
        let actions: &Actions = &view.actions;
        state.set_selected_count(i32::try_from(actions.selected).unwrap_or(i32::MAX));
        state.set_play_why(availability(actions.play));
        state.set_install_why(availability(actions.install));
        state.set_match_why(availability(actions.match_game));
        state.set_edit_existing(
            view.detail
                .as_ref()
                .is_some_and(|d| matches!(d.membership, Membership::InLibrary(_))),
        );
        state.set_add_why(availability(actions.add_to_gamenode));
        state.set_add_existing_status(match actions.add_to_gamenode {
            Availability::Disabled(Reason::AlreadyInLibrary(status)) => optional_status(status),
            _ => StatusKind::Unknown,
        });
        state.set_change_match(actions.change_match);

        let Some(detail) = &view.detail else {
            state.set_detail_valid(false);
            state.set_launch_note(LaunchNote::None);
            return;
        };
        let row = &detail.row;
        let artwork = detail.artwork.as_ref().and_then(|bytes| {
            row.status
                .game_id()
                .and_then(|game_id| self.artwork.get(ui, game_id, bytes))
        });
        let detail_image = artwork;
        let (standing, standing_status) = standing(detail.membership);
        state.set_detail_running(detail.running);
        state.set_detail_playtime_known(row.playtime.seconds.is_some());
        state.set_detail_playtime_hours(
            row.playtime
                .seconds
                .map_or(0, |s| i32::try_from(s / 3600).unwrap_or(i32::MAX)),
        );
        state.set_detail_playtime_minutes(
            row.playtime.seconds.map_or(0, |s| ((s % 3600) / 60) as i32),
        );
        state.set_detail_playtime_seconds(row.playtime.seconds.map_or(0, |s| (s % 60) as i32));
        state.set_detail_playtime_from_store(
            row.playtime.source == gamenode_core::playtime::PlaytimeSource::Store,
        );
        state.set_detail_valid(true);
        state.set_detail_title(row.title.as_str().into());
        state.set_detail_detected(row.detected_name.as_str().into());
        state.set_detail_matched(row.matched_name.clone().unwrap_or_default().into());
        state.set_detail_provider(row.provider_name.as_str().into());
        state.set_detail_dir(detail.install_dir.as_str().into());
        state.set_detail_year(
            detail
                .release_year
                .map(|y| y.to_string())
                .unwrap_or_default()
                .into(),
        );
        state.set_detail_kind(match_kind(&row.status));
        state.set_detail_source(source_kind(&row.status));
        state.set_detail_standing(standing);
        state.set_detail_standing_status(standing_status);
        state.set_detail_has_cover(detail_image.is_some());
        state.set_detail_cover(detail_image.unwrap_or_default());
        state.set_launch_note(launch_note(
            snapshot.launch.as_ref(),
            Some(row.key.as_str()),
        ));
    }

    fn option(&mut self, option: &MatchOption) -> OptionItem {
        let cover = self.cover(option.cover_file.as_deref());
        OptionItem {
            id: option.game_id.to_string().into(),
            name: option.name.as_str().into(),
            year: option
                .release_year
                .map(|y| y.to_string())
                .unwrap_or_default()
                .into(),
            has_cover: cover.is_some(),
            cover: cover.unwrap_or_default(),
            current: option.current,
        }
    }

    fn render_match(&mut self, state: &LibraryState<'_>, view: &LibraryView) {
        let Some(dialog) = &view.match_dialog else {
            state.set_match_open(false);
            self.match_was_open = false;
            return;
        };
        if !self.match_was_open {
            // The search starts as the detected title; after that the text box owns it.
            state.set_match_query(dialog.query.as_str().into());
        }
        self.match_was_open = true;
        state.set_match_open(true);
        state.set_match_detected(dialog.detected_name.as_str().into());
        state.set_match_provider(dialog.provider_name.as_str().into());
        state.set_match_searching(dialog.searching);
        let candidates = dialog.candidates.iter().map(|o| self.option(o)).collect();
        sync(&self.candidates, candidates);
        let results = dialog.results.iter().map(|o| self.option(o)).collect();
        sync(&self.results, results);
        state.set_match_no_results(dialog.no_results);
        let (error, blocked) = search_error(dialog.error);
        state.set_match_error(error);
        state.set_match_blocked(
            blocked
                .or_else(|| dialog.search_blocked.map(why))
                .unwrap_or(Why::Available),
        );
        state.set_match_can_clear(dialog.can_clear);
    }

    fn render_add(&mut self, state: &LibraryState<'_>, view: &LibraryView) {
        let Some(dialog) = &view.add_dialog else {
            state.set_add_open(false);
            return;
        };
        state.set_add_open(true);
        state.set_add_editing(dialog.editing_existing);
        sync(
            &self.targets,
            dialog
                .targets
                .iter()
                .map(|target| TargetItem {
                    title: target.title.as_str().into(),
                    matched: target.matched,
                })
                .collect(),
        );
        state.set_add_status(status_kind(dialog.status));
        state.set_add_applied_status(status_kind(dialog.applied_status));
        state.set_add_applied_from(dialog.applied_from.clone().unwrap_or_default().into());
        match &dialog.collections {
            CollectionsView::Loading => {
                state.set_add_collections_kind(CollectionsKind::Loading);
                self.collections.set_vec(vec![]);
            }
            CollectionsView::Failed(_) => {
                state.set_add_collections_kind(CollectionsKind::Failed);
                self.collections.set_vec(vec![]);
            }
            CollectionsView::Loaded(options) => {
                state.set_add_collections_kind(CollectionsKind::Loaded);
                sync(
                    &self.collections,
                    options
                        .iter()
                        .map(|option| CollectionItem {
                            id: option.id.as_str().into(),
                            name: option.name.as_str().into(),
                            picked: option.picked,
                            forces: optional_status(option.default_status),
                        })
                        .collect(),
                );
            }
        }
        state.set_add_running(dialog.progress.is_some() && dialog.summary.is_none());
        state.set_add_done(
            dialog
                .progress
                .map_or(0, |p| i32::try_from(p.done).unwrap_or(0)),
        );
        state.set_add_total(
            dialog
                .progress
                .map_or(0, |p| i32::try_from(p.total).unwrap_or(0)),
        );
        state.set_add_error(dialog.error.map_or(AddError::None, api_error));
        state.set_add_can_confirm(dialog.can_confirm);
        state.set_add_blocked(dialog.blocked.map_or(Why::Available, why));

        let Some(summary) = &dialog.summary else {
            state.set_add_has_summary(false);
            self.summary.set_vec(vec![]);
            return;
        };
        state.set_add_has_summary(true);
        let count = |wanted: Outcome| {
            let n = summary
                .items
                .iter()
                .filter(|i| outcome(&i.outcome).0 == wanted)
                .count();
            i32::try_from(n).unwrap_or(i32::MAX)
        };
        state.set_add_summary_added(count(Outcome::Added));
        state.set_add_summary_present(count(Outcome::AlreadyPresent));
        state.set_add_summary_skipped(count(Outcome::Skipped));
        state.set_add_summary_failed(count(Outcome::Failed));
        sync(
            &self.summary,
            summary
                .items
                .iter()
                .map(|item| {
                    let (kind, status) = outcome(&item.outcome);
                    SummaryItem {
                        title: item.title.as_str().into(),
                        outcome: kind,
                        status,
                    }
                })
                .collect(),
        );
    }
}

fn load_cover(dir: &Path, file: &str) -> Option<Image> {
    // File names come from the cover cache (`<game id>.jpg`); never follow anything else.
    if file.contains(['/', '\\']) || file.starts_with('.') {
        return None;
    }
    Image::load_from_path(&dir.join(file)).ok()
}

fn selected_local_id(engine: &Engine) -> Option<String> {
    let view = engine.snapshot().library?;
    if let [key] = view.selected.as_slice() {
        key.strip_prefix("manual:").map(str::to_owned)
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
