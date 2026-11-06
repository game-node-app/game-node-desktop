//! The Add to GameNode dialog: which status and collections the user picked, and the data the
//! window shows. Confirming is in [`super::add_run`].
//!
//! Nothing is sent by opening, editing or cancelling. The only calls made before the user confirms
//! are reads: the user's collections, and (when an entry is selected) whether the game is already
//! in the library.

use std::collections::BTreeSet;

use crate::game_api::{ApiError, Collection};

use super::actions::Reason;
use super::add_run::{AddRun, AddSummary};
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
    /// The entries that were selected when the dialog opened, in display order.
    pub(super) targets: Vec<String>,
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

/// One game the dialog will add.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddTarget {
    pub entry_key: String,
    pub title: String,
    /// Unmatched games are skipped when the user confirms.
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
    pub targets: Vec<AddTarget>,
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
    pub summary: Option<AddSummary>,
}

impl LibraryMachine {
    /// The title and GameNode game of an entry, as the library shows it.
    pub(super) fn target_info(&self, key: &str) -> (String, Option<u64>) {
        let Some(entry) = self.entries.get(key) else {
            return (key.to_owned(), None);
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

    /// Opens the dialog for the selected entries, if Add to GameNode is available for them.
    pub(super) fn open_add(&mut self) -> Vec<Effect> {
        if self.dialog.is_some() || self.add.is_some() {
            return vec![];
        }
        if !self.actions().add_to_gamenode.is_available() {
            return vec![];
        }
        let Some(user_id) = self.account.as_ref().map(|account| account.id.clone()) else {
            return vec![];
        };
        let op = self.next_op();
        let existing = self
            .selected_game()
            .and_then(|id| self.account_entries.get(&id));
        let editing_existing = existing.is_some();
        self.add = Some(AddDialog {
            targets: self.selected_keys(),
            editing_existing,
            status: existing
                .and_then(|e| e.status)
                .unwrap_or(EntryStatus::Playing),
            picked: existing
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
        let targets: Vec<AddTarget> = add
            .targets
            .iter()
            .map(|key| {
                let (title, game) = self.target_info(key);
                AddTarget {
                    entry_key: key.clone(),
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
                Stage::Done(summary) => Some(summary.clone()),
                _ => None,
            },
            applied_from: from.map(|collection| collection.name.clone()),
            targets,
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
