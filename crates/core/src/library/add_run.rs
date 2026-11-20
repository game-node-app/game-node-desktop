//! Confirming the Add to GameNode dialog.
//!
//! Games are handled one at a time. For each matched game GameNode is asked whether it is already
//! in the library. Add and multi-add create absent entries and skip existing ones; explicit
//! single-game edits update existing entries. Unmatched games are skipped. A failed game does
//! not stop the others, except that a rejected session ends the run: every later game would be
//! rejected too.
//!
//! A multi-game run lives in [`LibraryMachine::add_runs`], outside the dialog, so hiding the
//! dialog does not stop it; a single-game run is abandoned when its dialog closes. A create has no
//! entry id in its answer, so the run reads the entry back once GameNode confirms it. Only entries
//! this add created carry an id, and only those can be undone.

use crate::game_api::{ApiError, LibraryEntry};

use super::add_dialog::{AddMode, AddProgress, AddTarget, Collections, Stage};
use super::machine::{Effect, LibraryMachine};
use serde::{Deserialize, Serialize};

use super::membership::{EntryStatus, Membership};

/// What happened to one game, or what is happening to it now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AddItemState {
    /// Not reached yet.
    Waiting,
    /// Its standing is being read, its entry created, or its edit sent.
    Adding,
    /// Created with the status GameNode applied.
    Added,
    /// It was already in the library, so nothing was sent.
    AlreadyPresent,
    /// The entry has no GameNode game.
    Skipped,
    Failed,
}

/// One game of an add run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddItem {
    pub target: AddTarget,
    /// The local library key, for a row target.
    pub entry_key: Option<String>,
    pub title: String,
    pub state: AddItemState,
    /// The status GameNode has (or applied) for the game, when it is known.
    pub status: Option<EntryStatus>,
    /// Why the request failed, for a `Failed` item.
    pub error: Option<ApiError>,
}

/// One game in the run or in its summary, as the window shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddItemView {
    pub target: AddTarget,
    /// The local library key, for a row target.
    pub entry_key: Option<String>,
    pub title: String,
    pub state: AddItemState,
    /// The status GameNode has (or applied) for the game, when it is known.
    pub status: Option<EntryStatus>,
    /// Why the request failed, for a `Failed` item.
    pub error: Option<ApiError>,
}

impl From<&AddItem> for AddItemView {
    fn from(item: &AddItem) -> Self {
        Self {
            target: item.target.clone(),
            entry_key: item.entry_key.clone(),
            title: item.title.clone(),
            state: item.state,
            status: item.status,
            error: item.error,
        }
    }
}

/// A library entry this add created, with the id the read-back returned. Undo deletes exactly
/// these ids.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedEntry {
    pub game_id: u64,
    pub entry_id: String,
}

/// The result of a confirmed add: one item per target, in the order they were selected, and the
/// entries this run created.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddSummary {
    /// Identifies this run for the notification and for `UndoAdd`.
    pub run: u64,
    pub items: Vec<AddItem>,
    /// Only entries created by this run; never an edit or an already-present entry.
    pub created: Vec<CreatedEntry>,
}

/// The result of a confirmed add, as the window shows it: flat item states and their counts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddSummaryView {
    pub run_id: u64,
    /// Whether at least one entry was created and its id read back, so Undo can delete it.
    pub undoable: bool,
    pub items: Vec<AddItemView>,
    pub added: usize,
    pub already_present: usize,
    pub skipped: usize,
    pub failed: usize,
}

/// The latest finished add run as the window's notification.
///
/// It lives outside the library view so a hidden batch that is stopped by sign-out or an account
/// change still reports its outcome after the view is gone. `account_id` is the account the run
/// belonged to, which is what keeps Undo and View on GameNode from acting on another account.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddNoticeView {
    pub account_id: Option<String>,
    /// Several targets: an all-skipped, all-already-present or all-failed batch still notifies.
    pub batch: bool,
    pub summary: AddSummaryView,
}

impl AddSummary {
    fn count(&self, state: AddItemState) -> usize {
        self.items.iter().filter(|item| item.state == state).count()
    }

    pub fn added(&self) -> usize {
        self.count(AddItemState::Added)
    }

    pub fn already_present(&self) -> usize {
        self.count(AddItemState::AlreadyPresent)
    }

    pub fn skipped_unmatched(&self) -> usize {
        self.count(AddItemState::Skipped)
    }

    pub fn failed(&self) -> usize {
        self.count(AddItemState::Failed)
    }

    /// The same summary, shaped for the window.
    pub fn view(&self) -> AddSummaryView {
        AddSummaryView {
            run_id: self.run,
            undoable: !self.created.is_empty(),
            items: self.items.iter().map(AddItemView::from).collect(),
            added: self.added(),
            already_present: self.already_present(),
            skipped: self.skipped_unmatched(),
            failed: self.failed(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StepKind {
    /// Asking whether the game is already in the library.
    Checking,
    /// The create request is out.
    Creating,
    /// The edit request is out.
    Updating,
    /// The create succeeded; GameNode is asked for the entry id so Undo can exist.
    ReadingBack,
}

#[derive(Debug)]
struct Step {
    /// The item this step belongs to.
    index: usize,
    game_id: u64,
    op: u64,
    kind: StepKind,
}

/// A confirmed add, one game at a time. A batch outlives its dialog: hiding the dialog leaves the
/// run here, and a new Add shows it again.
#[derive(Debug)]
pub(super) struct AddRun {
    items: Vec<AddItem>,
    step: Option<Step>,
    created: Vec<CreatedEntry>,
    /// Identifies the run for the notification and for Undo.
    pub(super) run: u64,
    pub(super) status: EntryStatus,
    pub(super) collection_ids: Vec<String>,
    pub(super) editing_existing: bool,
    mode: AddMode,
    /// Several targets: the run keeps going when the dialog is hidden.
    batch: bool,
    /// The account the run belonged to, for the notification's action gating.
    account_id: Option<String>,
}

impl AddRun {
    pub(super) fn targets(&self) -> Vec<AddTarget> {
        self.items.iter().map(|item| item.target.clone()).collect()
    }

    pub(super) fn mode(&self) -> AddMode {
        self.mode
    }

    pub(super) fn is_batch(&self) -> bool {
        self.batch
    }

    pub(super) fn item_views(&self) -> Vec<AddItemView> {
        self.items.iter().map(AddItemView::from).collect()
    }

    pub(super) fn progress(&self) -> AddProgress {
        AddProgress {
            done: self
                .items
                .iter()
                .filter(|item| !matches!(item.state, AddItemState::Waiting | AddItemState::Adding))
                .count(),
            total: self.items.len(),
        }
    }

    /// The game whose request has been sent and not answered.
    pub(super) fn in_flight_game(&self) -> Option<u64> {
        self.step.as_ref().map(|step| step.game_id)
    }
}

impl LibraryMachine {
    /// Starts adding every target of the open dialog.
    pub(super) fn confirm_add(&mut self) -> Vec<Effect> {
        if self.network_block().is_some() || self.add_runs.is_some() {
            return vec![];
        }
        let Some(mut add) = self.add.take() else {
            return vec![];
        };
        let ready = matches!(add.stage, Stage::Editing)
            && if add.editing_existing {
                matches!(add.collections, Collections::Loaded(_))
            } else {
                !matches!(add.collections, Collections::Loading { .. })
            };
        if !ready {
            self.add = Some(add);
            return vec![];
        }
        let (status, _) = Self::applied_status(&add);
        let collection_ids = match &add.collections {
            Collections::Loaded(list) => list
                .iter()
                .filter(|collection| add.picked.contains(&collection.id))
                .map(|collection| collection.id.clone())
                .collect(),
            _ => Vec::new(),
        };
        let items = add
            .targets
            .iter()
            .map(|target| {
                let (title, _) = self.target_info(target);
                AddItem {
                    target: target.clone(),
                    entry_key: target.entry_key().map(str::to_owned),
                    title,
                    state: AddItemState::Waiting,
                    status: None,
                    error: None,
                }
            })
            .collect::<Vec<_>>();
        let batch = add.targets.len() > 1;
        add.error = None;
        add.stage = Stage::Running;
        // A new run replaces the previous summary: only the latest add can be undone.
        self.last_add = None;
        self.last_add_notice = None;
        self.add_runs = Some(AddRun {
            items,
            step: None,
            created: Vec::new(),
            run: self.next_op(),
            status,
            collection_ids,
            editing_existing: add.editing_existing,
            mode: add.mode,
            batch,
            account_id: self.account.as_ref().map(|account| account.id.clone()),
        });
        self.add = Some(add);
        self.advance_add()
    }

    /// Starts the next game that needs a request, or finishes the run. A session that is no longer
    /// usable stops the run before another request is sent, and the rest is counted as not added.
    pub(super) fn advance_add(&mut self) -> Vec<Effect> {
        let Some(mut run) = self.add_runs.take() else {
            return vec![];
        };
        if self.network_block().is_some() {
            mark_stopped(&mut run, ApiError::Rejected);
            return self.finish_add(run);
        }
        let mut effects = Vec::new();
        loop {
            let Some(index) = run
                .items
                .iter()
                .position(|item| item.state == AddItemState::Waiting)
            else {
                effects.extend(self.finish_add(run));
                return effects;
            };
            let target = run.items[index].target.clone();
            let (title, game) = self.target_info(&target);
            run.items[index].title = title;
            let Some(game_id) = game else {
                run.items[index].state = AddItemState::Skipped;
                continue;
            };
            let op = self.next_op();
            run.items[index].state = AddItemState::Adding;
            run.step = Some(Step {
                index,
                game_id,
                op,
                kind: StepKind::Checking,
            });
            effects.push(Effect::ReadEntry { op, game_id });
            self.add_runs = Some(run);
            return effects;
        }
    }

    /// Stops a run because the session is gone: nothing more is sent, and every game that was not
    /// handled counts as not added. The dialog shows the outcome if it is open; otherwise the
    /// existing notification reports it.
    pub(super) fn stop_add_runs(&mut self) -> Vec<Effect> {
        let Some(mut run) = self.add_runs.take() else {
            return vec![];
        };
        mark_stopped(&mut run, ApiError::Rejected);
        self.finish_add(run)
    }

    /// The answer to a membership read that precedes a create, or to the read-back after one.
    pub(super) fn on_add_checked(
        &mut self,
        op: u64,
        game_id: u64,
        result: Result<Option<LibraryEntry>, ApiError>,
    ) -> Vec<Effect> {
        let Some(mut run) = self.add_runs.take() else {
            return vec![];
        };
        let Some(step) = run.step.take_if(|step| {
            step.op == op
                && step.game_id == game_id
                && matches!(step.kind, StepKind::Checking | StepKind::ReadingBack)
        }) else {
            self.add_runs = Some(run);
            return vec![];
        };

        match (step.kind, result) {
            (StepKind::Checking, Ok(Some(entry))) if run.editing_existing => {
                let mut effects = self.record_entry_read(game_id, &Ok(Some(entry.clone())));
                let op = self.next_op();
                effects.push(Effect::UpdateEntry {
                    op,
                    game_id,
                    status: run.status,
                    collection_ids: run.collection_ids.clone(),
                    existing: entry,
                });
                run.step = Some(Step {
                    op,
                    kind: StepKind::Updating,
                    ..step
                });
                self.add_runs = Some(run);
                effects
            }
            (StepKind::Checking, Ok(Some(entry))) => {
                let mut effects = self.record_entry_read(game_id, &Ok(Some(entry.clone())));
                self.membership_reads.remove(&game_id);
                let item = &mut run.items[step.index];
                item.state = AddItemState::AlreadyPresent;
                item.status = entry.status;
                item.error = None;
                self.add_runs = Some(run);
                effects.extend(self.advance_add());
                effects
            }
            (StepKind::Checking, Ok(None)) => {
                let mut effects = self.record_entry_read(game_id, &Ok(None));
                let op = self.next_op();
                effects.push(Effect::CreateEntry {
                    op,
                    game_id,
                    status: run.status,
                    collection_ids: run.collection_ids.clone(),
                });
                run.step = Some(Step {
                    op,
                    kind: StepKind::Creating,
                    ..step
                });
                self.add_runs = Some(run);
                effects
            }
            (StepKind::Checking, Err(error)) => self.fail_add(run, step, error),
            (StepKind::ReadingBack, result) => {
                let mut effects = Vec::new();
                let (status, created) = match result {
                    Ok(Some(entry)) => {
                        effects.extend(self.record_entry_read(game_id, &Ok(Some(entry.clone()))));
                        let created = (!entry.id.is_empty()).then(|| CreatedEntry {
                            game_id,
                            entry_id: entry.id.clone(),
                        });
                        (entry.status.unwrap_or(run.status), created)
                    }
                    // The create succeeded but the entry could not be read back. It is added and
                    // stays in the library, but without an id there is nothing Undo can delete.
                    Ok(None) | Err(_) => {
                        self.membership
                            .insert(game_id, Membership::InLibrary(Some(run.status)));
                        (run.status, None)
                    }
                };
                if let Some(created) = created {
                    run.created.push(created);
                }
                let item = &mut run.items[step.index];
                item.state = AddItemState::Added;
                item.status = Some(status);
                item.error = None;
                self.add_runs = Some(run);
                effects.extend(self.advance_add());
                effects
            }
            _ => {
                self.add_runs = Some(run);
                vec![]
            }
        }
    }

    /// The answer to a create request.
    pub(super) fn on_entry_created(
        &mut self,
        op: u64,
        game_id: u64,
        result: Result<(), ApiError>,
    ) -> Vec<Effect> {
        let Some(mut run) = self.add_runs.take() else {
            return vec![];
        };
        let Some(step) = run.step.take_if(|step| {
            step.op == op && step.game_id == game_id && step.kind == StepKind::Creating
        }) else {
            self.add_runs = Some(run);
            return vec![];
        };
        match result {
            Ok(()) => {
                // A read of this game that started before the create may report "absent".
                self.membership_reads.remove(&game_id);
                self.membership
                    .insert(game_id, Membership::InLibrary(Some(run.status)));
                let mut effects = Vec::new();
                if let Some(effect) =
                    self.update_cached_entry(game_id, run.status, &run.collection_ids)
                {
                    effects.push(effect);
                }
                // The create has no id in its answer. Read the entry back so Undo can delete
                // exactly this entry, and only after this run really created it.
                let op = self.next_op();
                effects.push(Effect::ReadEntry { op, game_id });
                run.step = Some(Step {
                    op,
                    kind: StepKind::ReadingBack,
                    ..step
                });
                self.add_runs = Some(run);
                effects
            }
            Err(error) => self.fail_add(run, step, error),
        }
    }

    /// The answer to an edit request. An edit never enters the created list.
    pub(super) fn on_entry_updated(
        &mut self,
        op: u64,
        game_id: u64,
        result: Result<(), ApiError>,
    ) -> Vec<Effect> {
        let Some(mut run) = self.add_runs.take() else {
            return vec![];
        };
        let Some(step) = run.step.take_if(|step| {
            step.op == op && step.game_id == game_id && step.kind == StepKind::Updating
        }) else {
            self.add_runs = Some(run);
            return vec![];
        };
        match result {
            Ok(()) => {
                self.membership_reads.remove(&game_id);
                self.membership
                    .insert(game_id, Membership::InLibrary(Some(run.status)));
                let mut effects = Vec::new();
                if let Some(effect) =
                    self.update_cached_entry(game_id, run.status, &run.collection_ids)
                {
                    effects.push(effect);
                }
                let item = &mut run.items[step.index];
                item.state = AddItemState::Added;
                item.status = Some(run.status);
                item.error = None;
                self.add_runs = Some(run);
                effects.extend(self.advance_add());
                effects
            }
            Err(error) => self.fail_add(run, step, error),
        }
    }

    fn fail_add(&mut self, mut run: AddRun, step: Step, error: ApiError) -> Vec<Effect> {
        let item = &mut run.items[step.index];
        item.state = AddItemState::Failed;
        item.error = Some(error);
        if error == ApiError::Rejected {
            // The session is gone: every remaining game would be refused too.
            mark_stopped(&mut run, error);
            return self.finish_add(run);
        }
        self.add_runs = Some(run);
        self.advance_add()
    }

    /// Completes the run: keeps its summary for the notification and, when the dialog is still
    /// open, shows the same summary in it.
    fn finish_add(&mut self, run: AddRun) -> Vec<Effect> {
        let summary = AddSummary {
            run: run.run,
            items: run.items,
            created: run.created,
        };
        // One game that failed: back to the choices, so the user can retry. A hidden batch has no
        // dialog to go back to and keeps its summary for the notification.
        if !run.batch && summary.items.len() == 1 && summary.items[0].state == AddItemState::Failed
        {
            if let Some(add) = self.add.as_mut() {
                add.error = summary.items[0].error;
                add.stage = Stage::Editing;
            }
            return vec![];
        }
        // Kept after the dialog closes, so the notification can offer Undo and open the library
        // page. This is the only place a finished run is published, which keeps it to once.
        self.last_add = Some(summary.clone());
        // The notice is independent of the library view: sign-out clears the view, and a hidden
        // batch's outcome must still reach the window.
        self.last_add_notice = Some(AddNoticeView {
            account_id: run.account_id,
            batch: run.batch,
            summary: summary.view(),
        });
        if let Some(add) = self.add.as_mut() {
            add.stage = Stage::Done(summary);
        }
        vec![]
    }

    /// Whether a run is still going. Used by the tests and by the app's progress reporting.
    pub fn add_run_active(&self) -> bool {
        self.add_runs.is_some()
    }
}

/// Marks every game that was not handled as failed with `error`. Already-finished items keep
/// their state.
fn mark_stopped(run: &mut AddRun, error: ApiError) {
    run.step = None;
    for item in &mut run.items {
        if matches!(item.state, AddItemState::Waiting | AddItemState::Adding) {
            item.state = AddItemState::Failed;
            item.error = Some(error);
        }
    }
}

#[cfg(test)]
mod tests;
