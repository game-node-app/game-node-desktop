//! The Add to GameNode dialog: which status and collections the user picked, and the data the
//! window shows. Confirming is in [`super::add_run`].
//!
//! Nothing is sent by opening, editing or cancelling. The only calls made before the user confirms
//! are reads: the user's collections, and (when an entry is selected) whether the game is already
//! in the library. Creating a collection is an explicit user action and sends its own request
//! without touching any library entry.

use std::collections::BTreeSet;

use crate::game_api::{ApiError, Collection, CreateCollectionError};

use super::actions::Reason;
use super::add_run::{AddItemView, AddRun, AddSummary, AddSummaryView};
use super::machine::{Effect, LibraryMachine};
use super::membership::EntryStatus;

/// The user's collections while the dialog is open.
#[derive(Debug)]
pub(super) enum Collections {
    Loading { op: u64 },
    Loaded(Vec<Collection>),
    Failed(ApiError),
}

/// The inline "New collection" field, while the user is creating one.
#[derive(Debug)]
pub(super) struct NewCollection {
    pub(super) name: String,
    /// The create request in flight.
    pub(super) creating_op: Option<u64>,
    /// Set after the server accepted the create: the collections are being re-listed so the new
    /// id can be picked. The name is kept until the re-list answer arrives.
    pub(super) created_name: Option<String>,
    /// The ids the list had before the create, so the new id is the one that appeared.
    pub(super) before: Vec<String>,
    pub(super) error: Option<NewCollectionError>,
}

impl NewCollection {
    fn new() -> Self {
        Self {
            name: String::new(),
            creating_op: None,
            created_name: None,
            before: Vec::new(),
            error: None,
        }
    }

    fn creating(&self) -> bool {
        self.creating_op.is_some() || self.created_name.is_some()
    }
}

#[derive(Debug)]
pub(super) enum Stage {
    /// The user is choosing.
    Editing,
    /// Confirmed: the run's games are being checked and added, one at a time. The run itself
    /// lives in [`LibraryMachine::add_runs`], so hiding the dialog does not stop it.
    Running,
    /// Every game was handled; the summary is shown until the dialog is closed.
    Done(AddSummary),
}

#[derive(Debug)]
pub(super) struct AddDialog {
    /// What the dialog was opened for, in display order.
    pub(super) targets: Vec<AddTarget>,
    /// Whether this is a library add or a catalog add (`AddTarget::Game`).
    pub(super) mode: AddMode,
    pub(super) editing_existing: bool,
    /// The status the user picked. A collection's default status can override it, see
    /// [`LibraryMachine::applied_status`].
    pub(super) status: EntryStatus,
    pub(super) picked: BTreeSet<String>,
    pub(super) collections: Collections,
    pub(super) stage: Stage,
    /// Why the last confirm failed. The choices stay, so the user can retry.
    pub(super) error: Option<ApiError>,
    /// The inline new-collection field, while it is open.
    pub(super) new_collection: Option<NewCollection>,
}

use serde::{Deserialize, Serialize};

/// Where the Add to GameNode dialog was opened from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AddMode {
    /// One or more local library rows.
    #[default]
    Library,
    /// A game from the GameNode catalog, addressed by its game id instead of a local key.
    Catalog,
}

/// One thing the dialog will add.
///
/// The catalog has no local row to key on, so it addresses the GameNode game directly. The add run
/// treats both the same way after resolving the target's game id.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AddTarget {
    /// A local library entry key (`<provider>:<id>`).
    Row(String),
    /// A GameNode game id, as a catalog result gives it.
    Game(u64),
}

impl AddTarget {
    /// The local library key, for a row target.
    pub fn entry_key(&self) -> Option<&str> {
        match self {
            Self::Row(key) => Some(key),
            Self::Game(_) => None,
        }
    }

    /// The target's GameNode game id, when it is already known.
    pub fn game_id(&self) -> Option<u64> {
        match self {
            Self::Row(_) => None,
            Self::Game(game_id) => Some(*game_id),
        }
    }
}

/// One target as the window shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddTargetView {
    pub target: AddTarget,
    /// The local library key, for a row target.
    pub entry_key: Option<String>,
    pub title: String,
    /// The cached cover file, when the game has one. Served over `gamenode-asset://`.
    pub cover_file: Option<String>,
    /// Unmatched local games are skipped when the user confirms.
    pub matched: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionOption {
    pub id: String,
    pub name: String,
    pub default_status: Option<EntryStatus>,
    pub picked: bool,
    /// How many games this collection holds, from the last complete read of the account library.
    /// `None` when that read does not exist yet: an unknown count is never shown as zero.
    pub entry_count: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CollectionsView {
    Loading,
    /// The list could not be read; the user can retry.
    Failed(ApiError),
    Loaded(Vec<CollectionOption>),
}

/// Why the inline new-collection field shows an error. The typed name and every other choice stay
/// exactly as they were.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NewCollectionError {
    /// The account reached GameNode's collection limit.
    LimitReached,
    /// The session was refused; the user has to sign in again.
    Rejected,
    /// GameNode cannot be reached now.
    Offline,
    /// Any other failed request.
    Failed,
}

/// The inline new-collection field as the window shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewCollectionView {
    pub name: String,
    /// A create or its re-list is in flight.
    pub creating: bool,
    /// The create action is available: a non-blank name and no request in flight.
    pub can_create: bool,
    pub error: Option<NewCollectionError>,
}

/// How far a confirmed add has come.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddProgress {
    pub done: usize,
    pub total: usize,
}

pub type AddProgressView = AddProgress;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddDialogView {
    pub targets: Vec<AddTargetView>,
    pub mode: AddMode,
    pub editing_existing: bool,
    /// The status the user picked.
    pub status: EntryStatus,
    /// The status GameNode will apply: a picked collection's default, else `status`.
    pub applied_status: EntryStatus,
    /// The collection whose default status applies, if one does.
    pub applied_from: Option<String>,
    pub collections: CollectionsView,
    pub progress: Option<AddProgress>,
    /// The per-game states while the run is going, or its final states once it is done.
    pub items: Vec<AddItemView>,
    /// The last confirm failed (single game). The choices are kept.
    pub error: Option<ApiError>,
    pub can_confirm: bool,
    /// Why nothing can be sent right now (no verified session, or offline).
    pub blocked: Option<Reason>,
    /// Present once every game was handled.
    pub summary: Option<AddSummaryView>,
    /// The inline new-collection field, while it is open.
    pub new_collection: Option<NewCollectionView>,
}

impl LibraryMachine {
    /// The title and GameNode game of one target, as the library shows it.
    pub(super) fn target_info(&self, target: &AddTarget) -> (String, Option<u64>) {
        match target {
            AddTarget::Row(key) => {
                let Some(entry) = self.entries.get(key) else {
                    return (key.clone(), None);
                };
                let game_id = self.match_status(entry).game_id();
                let title = game_id
                    .and_then(|id| self.games.get(&id))
                    .map(|game| game.name.clone())
                    .filter(|name| !name.is_empty())
                    .or_else(|| entry.name.clone())
                    .unwrap_or_else(|| entry.store_id.clone());
                (title, game_id)
            }
            // A catalog target names the game directly. Its title comes from the game metadata
            // the catalog stored, or from the id until that metadata is known.
            AddTarget::Game(game_id) => {
                let title = self
                    .games
                    .get(game_id)
                    .map(|game| game.name.clone())
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| game_id.to_string());
                (title, Some(*game_id))
            }
        }
    }

    /// The cached cover file of one target: a DRM-free entry's custom cover first, then the
    /// matched GameNode game's cover.
    fn target_cover(&self, target: &AddTarget) -> Option<String> {
        if let AddTarget::Row(key) = target
            && let Some(custom) = self.local_covers.get(key)
        {
            return Some(custom.clone());
        }
        let game_id = match target {
            AddTarget::Row(key) => self
                .entries
                .get(key)
                .and_then(|entry| self.match_status(entry).game_id()),
            AddTarget::Game(game_id) => Some(*game_id),
        }?;
        self.games
            .get(&game_id)
            .and_then(|game| game.cover_file.clone())
    }

    /// Opens the dialog for the selected entries, if Add to GameNode is available for them.
    pub(super) fn open_add(&mut self) -> Vec<Effect> {
        if self.dialog.is_some() || self.add.is_some() {
            return vec![];
        }
        // One add run at a time: while a hidden batch is still going, Add reopens its progress
        // instead of starting another run.
        if self.add_runs.is_some() {
            self.reopen_add_run();
            return vec![];
        }
        if !self.actions().add_to_gamenode.is_available() {
            return vec![];
        }
        let targets = self
            .selected_keys()
            .into_iter()
            .map(AddTarget::Row)
            .collect();
        self.start_add(targets, AddMode::Library)
    }

    /// Opens the same dialog for one GameNode catalog game (D8). No local row or selection is
    /// needed; the game is addressed by id. The catalog view calls this once its result is chosen.
    pub(super) fn open_add_for_game(&mut self, game_id: u64) -> Vec<Effect> {
        if self.dialog.is_some() {
            return vec![];
        }
        if self.add.is_some() || self.add_runs.is_some() {
            self.reopen_add_run();
            return vec![];
        }
        self.start_add(vec![AddTarget::Game(game_id)], AddMode::Catalog)
    }

    /// Shows the running run in the dialog again, without sending anything.
    fn reopen_add_run(&mut self) {
        if self.add.is_some() {
            return;
        }
        let Some(run) = self.add_runs.as_ref() else {
            return;
        };
        self.add = Some(AddDialog {
            targets: run.targets(),
            mode: run.mode(),
            editing_existing: run.editing_existing,
            status: run.status,
            picked: run.collection_ids.iter().cloned().collect(),
            collections: Collections::Loaded(Vec::new()),
            stage: Stage::Running,
            error: None,
            new_collection: None,
        });
    }

    fn start_add(&mut self, targets: Vec<AddTarget>, mode: AddMode) -> Vec<Effect> {
        let Some(user_id) = self.account.as_ref().map(|account| account.id.clone()) else {
            return vec![];
        };
        // Only a single selected row whose entry is known can be edited in place. Catalog games
        // are always adds: an existing one is offered View on GameNode instead.
        let existing = (mode == AddMode::Library)
            .then(|| {
                self.selected_game()
                    .and_then(|id| self.account_entries.get(&id))
            })
            .flatten()
            .cloned();
        let editing_existing = existing.is_some();
        let op = self.next_op();
        self.add = Some(AddDialog {
            targets,
            mode,
            editing_existing,
            status: existing
                .as_ref()
                .and_then(|e| e.status)
                .unwrap_or(EntryStatus::Playing),
            picked: existing
                .as_ref()
                .map(|e| e.collection_ids.iter().cloned().collect())
                .unwrap_or_default(),
            collections: Collections::Loading { op },
            stage: Stage::Editing,
            error: None,
            new_collection: None,
        });
        vec![Effect::ListCollections { op, user_id }]
    }

    /// Closes the dialog. Before a confirm this sends nothing. Closing during a single-game run
    /// abandons it; a hidden batch keeps going through [`Self::hide_add`]. The answer to a request
    /// already in flight is ignored, so what GameNode has for that game is read again instead of
    /// guessed.
    pub(super) fn close_add(&mut self) {
        let Some(add) = self.add.take() else {
            return;
        };
        if !matches!(add.stage, Stage::Editing) {
            self.membership_focus = None;
        }
        if matches!(add.stage, Stage::Running)
            && !self.add_runs.as_ref().is_some_and(AddRun::is_batch)
        {
            self.abandon_add_run();
        }
    }

    /// Closes the dialog without abandoning a batch run. Anything else still closes the way
    /// [`Self::close_add`] does, so a single-game run keeps its existing "close abandons" rule.
    pub(super) fn hide_add(&mut self) {
        let Some(add) = self.add.take() else {
            return;
        };
        let keeps_running = matches!(add.stage, Stage::Running)
            && self.add_runs.as_ref().is_some_and(AddRun::is_batch);
        if !keeps_running {
            if !matches!(add.stage, Stage::Editing) {
                self.membership_focus = None;
            }
            self.abandon_add_run();
        }
    }

    fn abandon_add_run(&mut self) {
        let Some(run) = self.add_runs.take() else {
            return;
        };
        if let Some(game_id) = run.in_flight_game() {
            self.membership.remove(&game_id);
            self.membership_reads.remove(&game_id);
            self.membership_focus = None;
        }
    }

    pub(super) fn set_add_status(&mut self, status: EntryStatus) {
        if let Some(add) = self
            .add
            .as_mut()
            .filter(|add| matches!(add.stage, Stage::Editing))
        {
            add.status = status;
        }
    }

    /// Picks or unpicks one of the loaded collections.
    pub(super) fn toggle_add_collection(&mut self, id: &str) {
        let Some(add) = self
            .add
            .as_mut()
            .filter(|add| matches!(add.stage, Stage::Editing))
        else {
            return;
        };
        let Collections::Loaded(list) = &add.collections else {
            return;
        };
        if !list.iter().any(|collection| collection.id == id) {
            return;
        }
        if !add.picked.remove(id) {
            add.picked.insert(id.to_owned());
        }
    }

    /// Opens the inline new-collection field.
    pub(super) fn start_new_collection(&mut self) {
        let Some(add) = self
            .add
            .as_mut()
            .filter(|add| matches!(add.stage, Stage::Editing))
        else {
            return;
        };
        add.new_collection.get_or_insert_with(NewCollection::new);
    }

    /// Records the typed collection name. The name is kept across a failure so it can be retried.
    pub(super) fn set_new_collection_name(&mut self, name: String) {
        let Some(add) = self
            .add
            .as_mut()
            .filter(|add| matches!(add.stage, Stage::Editing))
        else {
            return;
        };
        let Some(new) = add.new_collection.as_mut().filter(|new| !new.creating()) else {
            return;
        };
        new.name = name;
        new.error = None;
    }

    /// Closes the inline new-collection field. A create already sent is not cancelled; its answer
    /// is ignored if the field is gone.
    pub(super) fn cancel_new_collection(&mut self) {
        if let Some(add) = self.add.as_mut() {
            add.new_collection = None;
        }
    }

    /// Creates the typed collection (`GameApi::create_collection`). Creating is an explicit user
    /// action, so being synced is not required; only a usable session is. No entry is touched.
    pub(super) fn create_collection(&mut self) -> Vec<Effect> {
        let Some(user_id) = self.account.as_ref().map(|account| account.id.clone()) else {
            return vec![];
        };
        let blocked = self.network_block();
        let Some(add) = self
            .add
            .as_ref()
            .filter(|add| matches!(add.stage, Stage::Editing))
        else {
            return vec![];
        };
        let Some(new) = add.new_collection.as_ref() else {
            return vec![];
        };
        let name = new.name.trim().to_owned();
        if name.is_empty() || new.creating() {
            return vec![];
        }
        let op = self.next_op();
        let Some(add) = self.add.as_mut() else {
            return vec![];
        };
        let Some(new) = add.new_collection.as_mut() else {
            return vec![];
        };
        if let Some(blocked) = blocked {
            new.error = Some(match blocked {
                Reason::Offline => NewCollectionError::Offline,
                _ => NewCollectionError::Rejected,
            });
            return vec![];
        }
        new.error = None;
        new.creating_op = Some(op);
        new.created_name = None;
        new.before = match &add.collections {
            Collections::Loaded(list) => list
                .iter()
                .map(|collection| collection.id.clone())
                .collect(),
            _ => Vec::new(),
        };
        vec![Effect::CreateCollection { op, user_id, name }]
    }

    /// The answer to a collection create. The server returns no body, so a success re-lists the
    /// collections and picks the id that appeared with the typed name.
    pub(super) fn on_collection_created(
        &mut self,
        op: u64,
        result: Result<(), CreateCollectionError>,
    ) -> Vec<Effect> {
        let Some(user_id) = self.account.as_ref().map(|account| account.id.clone()) else {
            return vec![];
        };
        let blocked = self.network_block();
        let Some(mut add) = self.add.take() else {
            return vec![];
        };
        if !matches!(add.stage, Stage::Editing) {
            self.add = Some(add);
            return vec![];
        }
        let Some(new) = add
            .new_collection
            .as_mut()
            .filter(|new| new.creating_op == Some(op))
        else {
            self.add = Some(add);
            return vec![];
        };
        new.creating_op = None;
        match result {
            Ok(()) => {
                new.created_name = Some(new.name.trim().to_owned());
                new.error = None;
                let list_op = self.next_op();
                add.collections = Collections::Loading { op: list_op };
                self.add = Some(add);
                vec![Effect::ListCollections {
                    op: list_op,
                    user_id,
                }]
            }
            Err(error) => {
                new.error = Some(match error {
                    CreateCollectionError::LimitReached => NewCollectionError::LimitReached,
                    CreateCollectionError::Rejected => NewCollectionError::Rejected,
                    CreateCollectionError::Failed => {
                        if blocked == Some(Reason::Offline) {
                            NewCollectionError::Offline
                        } else {
                            NewCollectionError::Failed
                        }
                    }
                });
                new.created_name = None;
                self.add = Some(add);
                vec![]
            }
        }
    }

    pub(super) fn on_collections_listed(
        &mut self,
        op: u64,
        result: Result<Vec<Collection>, ApiError>,
    ) -> Vec<Effect> {
        let blocked = self.network_block();
        let Some(mut add) = self.add.take() else {
            return vec![];
        };
        if !matches!(add.collections, Collections::Loading { op: waiting } if waiting == op) {
            self.add = Some(add);
            return vec![];
        }
        match result {
            Ok(list) => {
                select_created_collection(&mut add, &list);
                if add.editing_existing
                    && add
                        .picked
                        .iter()
                        .any(|id| !list.iter().any(|c| &c.id == id))
                {
                    add.collections = Collections::Failed(ApiError::Failed);
                    self.add = Some(add);
                    return vec![];
                }
                add.picked
                    .retain(|id| list.iter().any(|collection| &collection.id == id));
                add.collections = Collections::Loaded(list);
            }
            Err(error) => {
                // The create succeeded but its new id cannot be read yet. Keep the name and the
                // pending pick so a retry of the list can still select it.
                if let Some(new) = add.new_collection.as_mut()
                    && new.created_name.is_some()
                {
                    new.error = Some(if error == ApiError::Rejected {
                        NewCollectionError::Rejected
                    } else if blocked == Some(Reason::Offline) {
                        NewCollectionError::Offline
                    } else {
                        NewCollectionError::Failed
                    });
                }
                add.collections = Collections::Failed(error);
            }
        }
        self.add = Some(add);
        vec![]
    }

    /// Reads the collections again after a failure.
    pub(super) fn retry_collections(&mut self) -> Vec<Effect> {
        if self.network_block().is_some() {
            return vec![];
        }
        let Some(user_id) = self.account.as_ref().map(|account| account.id.clone()) else {
            return vec![];
        };
        let op = self.next_op();
        let Some(add) = self.add.as_mut() else {
            return vec![];
        };
        if !matches!(add.stage, Stage::Editing)
            || !matches!(add.collections, Collections::Failed(_))
        {
            return vec![];
        }
        add.collections = Collections::Loading { op };
        vec![Effect::ListCollections { op, user_id }]
    }

    /// The status GameNode will apply, and the collection that decides it, if one does. GameNode
    /// uses the first picked collection that has a default status and ignores the request's
    /// status; the picked collections are taken in the order the dialog lists them.
    pub(super) fn applied_status(add: &AddDialog) -> (EntryStatus, Option<&Collection>) {
        if let Collections::Loaded(list) = &add.collections {
            let decided = list
                .iter()
                .filter(|collection| add.picked.contains(&collection.id))
                .find_map(|collection| collection.default_status.map(|s| (s, collection)));
            if let Some((status, collection)) = decided {
                return (status, Some(collection));
            }
        }
        (add.status, None)
    }

    /// How many games each collection holds, from the last complete account library read. A
    /// collection with no entries is zero; without a complete read every count is unknown.
    fn add_collection_counts(&self) -> Option<std::collections::BTreeMap<String, u32>> {
        let library = self.account_library.as_ref()?;
        let mut counts = std::collections::BTreeMap::new();
        for entry in library.entries.values() {
            for id in &entry.collection_ids {
                *counts.entry(id.clone()).or_insert(0) += 1;
            }
        }
        Some(counts)
    }

    pub(super) fn add_dialog_view(&self) -> Option<AddDialogView> {
        let add = self.add.as_ref()?;
        let counts = self.add_collection_counts();
        let targets: Vec<AddTargetView> = add
            .targets
            .iter()
            .map(|target| {
                let (title, game) = self.target_info(target);
                AddTargetView {
                    target: target.clone(),
                    entry_key: target.entry_key().map(str::to_owned),
                    title,
                    cover_file: self.target_cover(target),
                    matched: game.is_some(),
                }
            })
            .collect();
        let (applied_status, from) = Self::applied_status(add);
        let blocked = self.network_block();
        let editing = matches!(add.stage, Stage::Editing);
        let can_confirm = editing
            && self.add_runs.is_none()
            && blocked.is_none()
            && if add.editing_existing {
                matches!(add.collections, Collections::Loaded(_))
            } else {
                !matches!(add.collections, Collections::Loading { .. })
            }
            && targets.iter().any(|target| target.matched);
        let items = match (&add.stage, self.add_runs.as_ref()) {
            (Stage::Running, Some(run)) => run.item_views(),
            (Stage::Done(summary), _) => summary.view().items,
            _ => Vec::new(),
        };
        Some(AddDialogView {
            targets,
            mode: add.mode,
            collections: match &add.collections {
                Collections::Loading { .. } => CollectionsView::Loading,
                Collections::Failed(error) => CollectionsView::Failed(*error),
                Collections::Loaded(list) => CollectionsView::Loaded(
                    list.iter()
                        .map(|collection| CollectionOption {
                            id: collection.id.clone(),
                            name: collection.name.clone(),
                            default_status: collection.default_status,
                            picked: add.picked.contains(&collection.id),
                            entry_count: counts
                                .as_ref()
                                .map(|counts| counts.get(&collection.id).copied().unwrap_or(0)),
                        })
                        .collect(),
                ),
            },
            progress: match (&add.stage, self.add_runs.as_ref()) {
                (Stage::Running, Some(run)) => Some(run.progress()),
                (Stage::Done(summary), _) => Some(AddProgress {
                    done: summary.items.len(),
                    total: summary.items.len(),
                }),
                _ => None,
            },
            items,
            summary: match &add.stage {
                Stage::Done(summary) => Some(summary.view()),
                _ => None,
            },
            applied_from: from.map(|collection| collection.name.clone()),
            editing_existing: add.editing_existing,
            status: add.status,
            applied_status,
            error: add.error,
            can_confirm,
            blocked,
            new_collection: add.new_collection.as_ref().map(|new| NewCollectionView {
                name: new.name.clone(),
                creating: new.creating(),
                can_create: !new.name.trim().is_empty() && !new.creating(),
                error: new.error,
            }),
        })
    }
}

/// Checks the collection that appeared with the typed name in a re-list after a create. The new
/// id is the one that was not in the list before; when the server does not expose a new id (for
/// example, it answered the same page), the newest same-name id is picked instead.
fn select_created_collection(add: &mut AddDialog, list: &[Collection]) {
    let Some(new) = add.new_collection.as_mut() else {
        return;
    };
    let Some(name) = new.created_name.clone() else {
        return;
    };
    let picked = list
        .iter()
        .find(|collection| collection.name == name && !new.before.contains(&collection.id))
        .map(|collection| collection.id.clone())
        .or_else(|| {
            list.iter()
                .rev()
                .find(|collection| collection.name == name)
                .map(|collection| collection.id.clone())
        });
    match picked {
        Some(id) => {
            add.picked.insert(id);
            add.new_collection = None;
        }
        None => {
            new.created_name = None;
            new.error = Some(NewCollectionError::Failed);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
