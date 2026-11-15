//! The Add to GameNode dialog: which status and collections the user picked, and the data the
//! window shows. Confirming is in [`super::add_run`].
//!
//! Nothing is sent by opening, editing or cancelling. The only calls made before the user confirms
//! are reads: the user's collections, and (when an entry is selected) whether the game is already
//! in the library.

use std::collections::BTreeSet;

use crate::game_api::{ApiError, Collection};

use super::actions::Reason;
use super::add_run::{AddRun, AddSummary, AddSummaryView};
use super::machine::{Effect, LibraryMachine};
use super::membership::EntryStatus;

/// The user's collections while the dialog is open.
#[derive(Debug)]
pub(super) enum Collections {
    Loading { op: u64 },
    Loaded(Vec<Collection>),
    Failed(ApiError),
}

#[derive(Debug)]
pub(super) enum Stage {
    /// The user is choosing.
    Editing,
    /// Confirmed: games are being checked and added, one at a time.
    Running(AddRun),
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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CollectionsView {
    Loading,
    /// The list could not be read; the user can retry.
    Failed(ApiError),
    Loaded(Vec<CollectionOption>),
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
    /// The last confirm failed (single game). The choices are kept.
    pub error: Option<ApiError>,
    pub can_confirm: bool,
    /// Why nothing can be sent right now (no verified session, or offline).
    pub blocked: Option<Reason>,
    /// Present once every game was handled.
    pub summary: Option<AddSummaryView>,
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

    /// Opens the dialog for the selected entries, if Add to GameNode is available for them.
    pub(super) fn open_add(&mut self) -> Vec<Effect> {
        if self.dialog.is_some() || self.add.is_some() {
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
        if self.dialog.is_some() || self.add.is_some() {
            return vec![];
        }
        self.start_add(vec![AddTarget::Game(game_id)], AddMode::Catalog)
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
        });
        vec![Effect::ListCollections { op, user_id }]
    }

    /// Closes the dialog. Before a confirm this sends nothing. Closing during a confirm stops
    /// after the request already in flight; its answer is ignored, so what GameNode has for that
    /// game is read again instead of guessed.
    pub(super) fn close_add(&mut self) {
        let Some(add) = self.add.take() else {
            return;
        };
        if !matches!(add.stage, Stage::Editing) {
            self.membership_focus = None;
        }
        if let Stage::Running(run) = add.stage
            && let Some(game_id) = run.in_flight_game()
        {
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

    pub(super) fn on_collections_listed(
        &mut self,
        op: u64,
        result: Result<Vec<Collection>, ApiError>,
    ) -> Vec<Effect> {
        let Some(add) = self.add.as_mut() else {
            return vec![];
        };
        if !matches!(add.collections, Collections::Loading { op: waiting } if waiting == op) {
            return vec![];
        }
        add.collections = match result {
            Ok(list)
                if add.editing_existing
                    && add
                        .picked
                        .iter()
                        .any(|id| !list.iter().any(|c| &c.id == id)) =>
            {
                Collections::Failed(ApiError::Failed)
            }
            Ok(list) => {
                add.picked
                    .retain(|id| list.iter().any(|collection| &collection.id == id));
                Collections::Loaded(list)
            }
            Err(error) => Collections::Failed(error),
        };
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

    pub(super) fn add_dialog_view(&self) -> Option<AddDialogView> {
        let add = self.add.as_ref()?;
        let targets: Vec<AddTargetView> = add
            .targets
            .iter()
            .map(|target| {
                let (title, game) = self.target_info(target);
                AddTargetView {
                    target: target.clone(),
                    entry_key: target.entry_key().map(str::to_owned),
                    title,
                    matched: game.is_some(),
                }
            })
            .collect();
        let (applied_status, from) = Self::applied_status(add);
        let blocked = self.network_block();
        let editing = matches!(add.stage, Stage::Editing);
        let can_confirm = editing
            && blocked.is_none()
            && if add.editing_existing {
                matches!(add.collections, Collections::Loaded(_))
            } else {
                !matches!(add.collections, Collections::Loading { .. })
            }
            && targets.iter().any(|target| target.matched);
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
                        })
                        .collect(),
                ),
            },
            progress: match &add.stage {
                Stage::Running(run) => Some(run.progress()),
                _ => None,
            },
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
        })
    }
}

#[cfg(test)]
pub(crate) mod tests;
