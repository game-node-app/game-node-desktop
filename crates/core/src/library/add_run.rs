//! Confirming the Add to GameNode dialog.
//!
//! Games are handled one at a time. For each matched game GameNode is asked whether it is already
//! in the library. Add and multi-add create absent entries and skip existing ones; explicit
//! single-game edits update existing entries. Unmatched games are skipped. A failed game does
//! not stop the others, except that a rejected session ends the run: every later game would be
//! rejected too.
//!
//! A create has no entry id in its answer, so the run reads the entry back once GameNode confirms
//! it. Only entries this add created carry an id, and only those can be undone; an edit or an
//! entry that was already present is never in that list.

use std::collections::VecDeque;

use crate::game_api::{ApiError, LibraryEntry};

use super::add_dialog::{AddProgress, AddTarget, Collections, Stage};
use super::machine::{Effect, LibraryMachine};
use serde::{Deserialize, Serialize};

use super::membership::{EntryStatus, Membership};

/// What happened to one game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AddOutcome {
    /// Created, with the status GameNode applies.
    Added(EntryStatus),
    /// It was already in the library, so nothing was sent. The status is `None` when GameNode sent
    /// one this build does not know.
    AlreadyPresent(Option<EntryStatus>),
    /// The entry has no GameNode game.
    SkippedUnmatched,
    Failed(ApiError),
}

pub type AddOutcomeItem = AddOutcome;

impl AddOutcome {
    /// The status GameNode has (or applied) for the game, when it is known.
    pub fn status(self) -> Option<EntryStatus> {
        match self {
            Self::Added(status) => Some(status),
            Self::AlreadyPresent(status) => status,
            Self::SkippedUnmatched | Self::Failed(_) => None,
        }
    }
}

/// What happened to one game, as the window shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AddOutcomeKind {
    Added,
    AlreadyPresent,
    Skipped,
    Failed,
}

impl From<&AddOutcome> for AddOutcomeKind {
    fn from(outcome: &AddOutcome) -> Self {
        match outcome {
            AddOutcome::Added(_) => Self::Added,
            AddOutcome::AlreadyPresent(_) => Self::AlreadyPresent,
            AddOutcome::SkippedUnmatched => Self::Skipped,
            AddOutcome::Failed(_) => Self::Failed,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddItem {
    pub target: AddTarget,
    /// The local library key, for a row target.
    pub entry_key: Option<String>,
    pub title: String,
    pub outcome: AddOutcome,
}

/// One game in the summary, as the window shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddItemView {
    pub target: AddTarget,
    /// The local library key, for a row target.
    pub entry_key: Option<String>,
    pub title: String,
    pub outcome: AddOutcomeKind,
    /// The status GameNode has (or applied) for the game, when it is known.
    pub status: Option<EntryStatus>,
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

/// The result of a confirmed add, as the window shows it: flat outcomes and their counts.
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

impl AddSummary {
    fn count(&self, wanted: impl Fn(&AddOutcome) -> bool) -> usize {
        self.items
            .iter()
            .filter(|item| wanted(&item.outcome))
            .count()
    }

    pub fn added(&self) -> usize {
        self.count(|outcome| matches!(outcome, AddOutcome::Added(_)))
    }

    pub fn already_present(&self) -> usize {
        self.count(|outcome| matches!(outcome, AddOutcome::AlreadyPresent(_)))
    }

    pub fn skipped_unmatched(&self) -> usize {
        self.count(|outcome| matches!(outcome, AddOutcome::SkippedUnmatched))
    }

    pub fn failed(&self) -> usize {
        self.count(|outcome| matches!(outcome, AddOutcome::Failed(_)))
    }

    /// The same summary, shaped for the window.
    pub fn view(&self) -> AddSummaryView {
        AddSummaryView {
            run_id: self.run,
            undoable: !self.created.is_empty(),
            items: self
                .items
                .iter()
                .map(|item| AddItemView {
                    target: item.target.clone(),
                    entry_key: item.entry_key.clone(),
                    title: item.title.clone(),
                    outcome: AddOutcomeKind::from(&item.outcome),
                    status: item.outcome.status(),
                })
                .collect(),
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
    target: AddTarget,
    title: String,
    game_id: u64,
    op: u64,
    kind: StepKind,
}

#[derive(Debug)]
pub(super) struct AddRun {
    waiting: VecDeque<AddTarget>,
    step: Option<Step>,
    items: Vec<AddItem>,
    created: Vec<CreatedEntry>,
    total: usize,
    /// Identifies the run for the notification and for Undo.
    run: u64,
    /// The status GameNode will apply.
    status: EntryStatus,
    collection_ids: Vec<String>,
}

impl AddRun {
    pub(super) fn progress(&self) -> AddProgress {
        AddProgress {
            done: self.items.len(),
            total: self.total,
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
        if self.network_block().is_some() {
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
        add.error = None;
        // A new run replaces the previous summary: only the latest add can be undone.
        self.last_add = None;
        add.stage = Stage::Running(AddRun {
            waiting: add.targets.iter().cloned().collect(),
            step: None,
            items: Vec::new(),
            created: Vec::new(),
            total: add.targets.len(),
            run: self.next_op(),
            status,
            collection_ids,
        });
        self.advance_add(add)
    }

    /// Starts the next game that needs a request, or finishes the run. Takes the dialog out of the
    /// machine for the caller and puts it back.
    fn advance_add(&mut self, mut add: super::add_dialog::AddDialog) -> Vec<Effect> {
        let Stage::Running(run) = &mut add.stage else {
            self.add = Some(add);
            return vec![];
        };
        let mut effects = Vec::new();
        loop {
            let Some(target) = run.waiting.pop_front() else {
                let summary = AddSummary {
                    run: run.run,
                    items: std::mem::take(&mut run.items),
                    created: std::mem::take(&mut run.created),
                };
                match summary.items.as_slice() {
                    // One game that failed: back to the choices, so the user can retry.
                    [
                        AddItem {
                            outcome: AddOutcome::Failed(error),
                            ..
                        },
                    ] => {
                        add.error = Some(*error);
                        add.stage = Stage::Editing;
                    }
                    _ => {
                        // Kept after the dialog closes, so the notification can offer Undo and
                        // open the library page.
                        self.last_add = Some(summary.clone());
                        add.stage = Stage::Done(summary);
                    }
                }
                break;
            };
            let (title, game) = self.target_info(&target);
            let Some(game_id) = game else {
                run.items.push(AddItem {
                    entry_key: target.entry_key().map(str::to_owned),
                    target,
                    title,
                    outcome: AddOutcome::SkippedUnmatched,
                });
                continue;
            };
            let op = self.next_op();
            run.step = Some(Step {
                target,
                title,
                game_id,
                op,
                kind: StepKind::Checking,
            });
            effects.push(Effect::ReadEntry { op, game_id });
            break;
        }
        self.add = Some(add);
        effects
    }

    /// The answer to a membership read that precedes a create, or to the read-back after one.
    pub(super) fn on_add_checked(
        &mut self,
        op: u64,
        game_id: u64,
        result: Result<Option<LibraryEntry>, ApiError>,
    ) -> Vec<Effect> {
        let Some(mut add) = self.add.take() else {
            return vec![];
        };
        let Stage::Running(run) = &mut add.stage else {
            self.add = Some(add);
            return vec![];
        };
        let Some(step) = run.step.take_if(|step| {
            step.op == op
                && step.game_id == game_id
                && matches!(step.kind, StepKind::Checking | StepKind::ReadingBack)
        }) else {
            self.add = Some(add);
            return vec![];
        };

        match (step.kind, result) {
            (StepKind::Checking, Ok(Some(entry))) if add.editing_existing => {
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
                self.add = Some(add);
                effects
            }
            (StepKind::Checking, Ok(Some(entry))) => {
                let mut effects = self.record_entry_read(game_id, &Ok(Some(entry.clone())));
                self.membership_reads.remove(&game_id);
                run.items
                    .push(item(&step, AddOutcome::AlreadyPresent(entry.status)));
                effects.extend(self.advance_add(add));
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
                self.add = Some(add);
                effects
            }
            (StepKind::Checking, Err(error)) => self.fail_add(add, step, error),
            (StepKind::ReadingBack, result) => {
                let mut effects = Vec::new();
                let (outcome, created) = match result {
                    Ok(Some(entry)) => {
                        effects.extend(self.record_entry_read(game_id, &Ok(Some(entry.clone()))));
                        let created = (!entry.id.is_empty()).then(|| CreatedEntry {
                            game_id,
                            entry_id: entry.id.clone(),
                        });
                        (
                            AddOutcome::Added(entry.status.unwrap_or(run.status)),
                            created,
                        )
                    }
                    // The create succeeded but the entry could not be read back. It is added and
                    // stays in the library, but without an id there is nothing Undo can delete.
                    Ok(None) | Err(_) => {
                        self.membership
                            .insert(game_id, Membership::InLibrary(Some(run.status)));
                        (AddOutcome::Added(run.status), None)
                    }
                };
                if let Some(created) = created {
                    run.created.push(created);
                }
                run.items.push(item(&step, outcome));
                effects.extend(self.advance_add(add));
                effects
            }
            _ => {
                self.add = Some(add);
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
        let Some(mut add) = self.add.take() else {
            return vec![];
        };
        let Stage::Running(run) = &mut add.stage else {
            self.add = Some(add);
            return vec![];
        };
        let Some(step) = run.step.take_if(|step| {
            step.op == op && step.game_id == game_id && step.kind == StepKind::Creating
        }) else {
            self.add = Some(add);
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
                self.add = Some(add);
                effects
            }
            Err(error) => self.fail_add(add, step, error),
        }
    }

    /// The answer to an edit request. An edit never enters the created list.
    pub(super) fn on_entry_updated(
        &mut self,
        op: u64,
        game_id: u64,
        result: Result<(), ApiError>,
    ) -> Vec<Effect> {
        let Some(mut add) = self.add.take() else {
            return vec![];
        };
        let Stage::Running(run) = &mut add.stage else {
            self.add = Some(add);
            return vec![];
        };
        let Some(step) = run.step.take_if(|step| {
            step.op == op && step.game_id == game_id && step.kind == StepKind::Updating
        }) else {
            self.add = Some(add);
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
                run.items.push(item(&step, AddOutcome::Added(run.status)));
                effects.extend(self.advance_add(add));
                effects
            }
            Err(error) => self.fail_add(add, step, error),
        }
    }

    fn fail_add(
        &mut self,
        mut add: super::add_dialog::AddDialog,
        step: Step,
        error: ApiError,
    ) -> Vec<Effect> {
        if let Stage::Running(run) = &mut add.stage {
            run.items.push(item(&step, AddOutcome::Failed(error)));
            if error == ApiError::Rejected {
                // The session is gone: every remaining game would be refused too.
                let remaining: Vec<AddTarget> = run.waiting.drain(..).collect();
                for target in remaining {
                    let (title, _) = self.target_info(&target);
                    run.items.push(AddItem {
                        entry_key: target.entry_key().map(str::to_owned),
                        target,
                        title,
                        outcome: AddOutcome::Failed(error),
                    });
                }
            }
        }
        self.advance_add(add)
    }
}

fn item(step: &Step, outcome: AddOutcome) -> AddItem {
    AddItem {
        target: step.target.clone(),
        entry_key: step.target.entry_key().map(str::to_owned),
        title: step.title.clone(),
        outcome,
    }
}

#[cfg(test)]
mod tests;
