//! Confirming the Add to GameNode dialog.
//!
//! Games are handled one at a time. For each matched game GameNode is asked whether it is already
//! in the library, and only when it is absent is it created, so an entry that exists is never
//! overwritten and never sent a create request. Unmatched games are skipped. A failed game does
//! not stop the others, except that a rejected session ends the run: every later game would be
//! rejected too.

use std::collections::VecDeque;

use crate::game_api::{ApiError, LibraryEntry};

use super::add_dialog::{AddProgress, Collections, Stage};
use super::machine::{Effect, LibraryMachine};
use super::membership::{EntryStatus, Membership};

/// What happened to one game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddItem {
    pub entry_key: String,
    pub title: String,
    pub outcome: AddOutcome,
}

/// The result of a confirmed add: one item per target, in the order they were selected.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AddSummary {
    pub items: Vec<AddItem>,
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
}

#[derive(Debug, PartialEq, Eq)]
enum StepKind {
    /// Asking whether the game is already in the library.
    Checking,
    /// The create request is out.
    Creating,
}

#[derive(Debug)]
struct Step {
    key: String,
    title: String,
    game_id: u64,
    op: u64,
    kind: StepKind,
}

#[derive(Debug)]
pub(super) struct AddRun {
    waiting: VecDeque<String>,
    step: Option<Step>,
    items: Vec<AddItem>,
    total: usize,
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
            && !matches!(add.collections, Collections::Loading { .. });
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
        add.stage = Stage::Running(AddRun {
            waiting: add.targets.iter().cloned().collect(),
            step: None,
            items: Vec::new(),
            total: add.targets.len(),
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
            let Some(key) = run.waiting.pop_front() else {
                let summary = AddSummary {
                    items: std::mem::take(&mut run.items),
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
                    _ => add.stage = Stage::Done(summary),
                }
                break;
            };
            let (title, game) = self.target_info(&key);
            let Some(game_id) = game else {
                run.items.push(AddItem {
                    entry_key: key,
                    title,
                    outcome: AddOutcome::SkippedUnmatched,
                });
                continue;
            };
            let op = self.next_op();
            run.step = Some(Step {
                key,
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

    /// The answer to the membership read that precedes a create.
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
            step.op == op && step.game_id == game_id && step.kind == StepKind::Checking
        }) else {
            self.add = Some(add);
            return vec![];
        };

        match result {
            Ok(Some(entry)) => {
                self.membership
                    .insert(game_id, Membership::InLibrary(entry.status));
                self.membership_reads.remove(&game_id);
                run.items
                    .push(item(step, AddOutcome::AlreadyPresent(entry.status)));
                self.advance_add(add)
            }
            Ok(None) => {
                let op = self.next_op();
                let effect = Effect::CreateEntry {
                    op,
                    game_id,
                    status: run.status,
                    collection_ids: run.collection_ids.clone(),
                };
                run.step = Some(Step {
                    op,
                    kind: StepKind::Creating,
                    ..step
                });
                self.add = Some(add);
                vec![effect]
            }
            Err(error) => self.fail_add(add, step, error),
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
                run.items.push(item(step, AddOutcome::Added(run.status)));
                self.advance_add(add)
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
            run.items.push(item(step, AddOutcome::Failed(error)));
            if error == ApiError::Rejected {
                // The session is gone: every remaining game would be refused too.
                let remaining: Vec<String> = run.waiting.drain(..).collect();
                for key in remaining {
                    let (title, _) = self.target_info(&key);
                    run.items.push(AddItem {
                        entry_key: key,
                        title,
                        outcome: AddOutcome::Failed(error),
                    });
                }
            }
        }
        self.advance_add(add)
    }
}

fn item(step: Step, outcome: AddOutcome) -> AddItem {
    AddItem {
        entry_key: step.key,
        title: step.title,
        outcome,
    }
}

#[cfg(test)]
mod tests;
