//! Batch GameNode writes: undo an add, change status, and add to a collection.
//!
//! All three are sequential: one request at a time, like the add run. Each game is refreshed from
//! GameNode before it is written, because the cached account entry (D4) does not carry the
//! platforms or the completion date `update_entry` must send back. Collections come from the full
//! entry too, so a status change never drops them and a collection add keeps the server's status.

use std::collections::{BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::game_api::{ApiError, LibraryEntry};

use super::add_run::CreatedEntry;
use super::machine::{Effect, LibraryMachine};
use super::membership::{EntryStatus, Membership};

/// How an undo ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UndoState {
    /// Every created entry was deleted.
    Done,
    /// Some were deleted and some deletes failed.
    Partial,
    /// No created entry could be deleted.
    Failed,
}

/// The result of one Undo, for the notification. `run` is the add run it undid.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoAddView {
    pub run: u64,
    pub state: UndoState,
    pub deleted: usize,
    pub failed: usize,
}

/// One finished status change, for the notification. `run` makes each run notify once.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusRunView {
    pub run: u64,
    pub status: EntryStatus,
    pub changed: usize,
    pub skipped: usize,
    pub failed: usize,
    /// Keys with no GameNode game.
    pub skipped_unmatched: usize,
    /// Matched games the account library does not contain.
    pub skipped_not_in_library: usize,
    /// Games in the library with a status this build does not know; their server value is kept.
    pub skipped_unknown_status: usize,
}

/// One finished collection add, for the notification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionRunView {
    pub run: u64,
    pub collection_id: String,
    pub collection_name: String,
    pub changed: usize,
    pub skipped: usize,
    pub failed: usize,
    pub skipped_unmatched: usize,
    pub skipped_not_in_library: usize,
    pub skipped_unknown_status: usize,
    pub skipped_already_in_collection: usize,
}

/// A matched game resolved from the account library, with the cached collections the resolve saw.
struct ResolvedGame {
    game_id: u64,
    collection_ids: Vec<String>,
}

#[derive(Default)]
struct ResolvedGames {
    games: Vec<ResolvedGame>,
    skipped_unmatched: usize,
    skipped_not_in_library: usize,
    skipped_unknown_status: usize,
}

#[derive(Debug, PartialEq, Eq)]
enum StatusStepKind {
    /// The full entry is being read, so platforms and completion date are known.
    Checking,
    Updating,
    /// The write succeeded; one read confirms the server's state.
    Refreshing,
}

#[derive(Debug)]
struct StatusStep {
    game_id: u64,
    op: u64,
    kind: StatusStepKind,
    collection_ids: Vec<String>,
}

#[derive(Debug)]
pub(super) struct StatusRun {
    run: u64,
    status: EntryStatus,
    waiting: VecDeque<u64>,
    step: Option<StatusStep>,
    changed: usize,
    failed: usize,
    skipped_unmatched: usize,
    skipped_not_in_library: usize,
    skipped_unknown_status: usize,
}

impl StatusRun {
    fn view(&self) -> StatusRunView {
        StatusRunView {
            run: self.run,
            status: self.status,
            changed: self.changed,
            skipped: self.skipped_unmatched
                + self.skipped_not_in_library
                + self.skipped_unknown_status,
            failed: self.failed,
            skipped_unmatched: self.skipped_unmatched,
            skipped_not_in_library: self.skipped_not_in_library,
            skipped_unknown_status: self.skipped_unknown_status,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum CollectionStepKind {
    Checking,
    Updating,
    Refreshing,
}

#[derive(Debug)]
struct CollectionStep {
    game_id: u64,
    op: u64,
    kind: CollectionStepKind,
    status: EntryStatus,
    collection_ids: Vec<String>,
}

#[derive(Debug)]
pub(super) struct CollectionRun {
    run: u64,
    collection_id: String,
    collection_name: String,
    waiting: VecDeque<u64>,
    step: Option<CollectionStep>,
    changed: usize,
    failed: usize,
    skipped_unmatched: usize,
    skipped_not_in_library: usize,
    skipped_unknown_status: usize,
    skipped_already_in_collection: usize,
}

impl CollectionRun {
    fn view(&self) -> CollectionRunView {
        CollectionRunView {
            run: self.run,
            collection_id: self.collection_id.clone(),
            collection_name: self.collection_name.clone(),
            changed: self.changed,
            skipped: self.skipped_unmatched
                + self.skipped_not_in_library
                + self.skipped_unknown_status
                + self.skipped_already_in_collection,
            failed: self.failed,
            skipped_unmatched: self.skipped_unmatched,
            skipped_not_in_library: self.skipped_not_in_library,
            skipped_unknown_status: self.skipped_unknown_status,
            skipped_already_in_collection: self.skipped_already_in_collection,
        }
    }
}

#[derive(Debug)]
pub(super) struct UndoRun {
    run: u64,
    waiting: VecDeque<CreatedEntry>,
    step: Option<UndoStep>,
    deleted: usize,
    failed: usize,
}

#[derive(Debug)]
struct UndoStep {
    op: u64,
    game_id: u64,
}

impl LibraryMachine {
    /// Whether an unanswered status step is the one `op`/`game_id` answer.
    pub(super) fn status_step_matches(&self, op: u64, game_id: u64) -> bool {
        self.status_run.as_ref().is_some_and(|run| {
            run.step
                .as_ref()
                .is_some_and(|step| step.op == op && step.game_id == game_id)
        })
    }

    pub(super) fn collection_step_matches(&self, op: u64, game_id: u64) -> bool {
        self.collection_run.as_ref().is_some_and(|run| {
            run.step
                .as_ref()
                .is_some_and(|step| step.op == op && step.game_id == game_id)
        })
    }

    /// Matched games present in the account library cache, deduplicated by game id. Keys that
    /// resolve to nothing are counted with their reason.
    fn resolve_library_games(&self, keys: &[String]) -> ResolvedGames {
        let mut resolved = ResolvedGames::default();
        let mut seen_keys = BTreeSet::new();
        let mut seen_games = BTreeSet::new();
        for key in keys {
            if !seen_keys.insert(key.as_str()) {
                continue;
            }
            let Some(entry) = self.entries.get(key) else {
                resolved.skipped_unmatched += 1;
                continue;
            };
            let Some(game_id) = self.match_status(entry).game_id() else {
                resolved.skipped_unmatched += 1;
                continue;
            };
            if !seen_games.insert(game_id) {
                continue;
            }
            match self
                .account_library
                .as_ref()
                .and_then(|library| library.entries.get(&game_id))
            {
                Some(cached) if cached.status.is_some() => resolved.games.push(ResolvedGame {
                    game_id,
                    collection_ids: cached.collection_ids.clone(),
                }),
                Some(_) => resolved.skipped_unknown_status += 1,
                None => resolved.skipped_not_in_library += 1,
            }
        }
        resolved
    }

    /// Sets one status on every selected game that is in the GameNode library, preserving each
    /// entry's collections, platforms and completion date.
    pub(super) fn set_status(&mut self, keys: Vec<String>, status: EntryStatus) -> Vec<Effect> {
        if self.network_block().is_some() || self.status_run.is_some() {
            return vec![];
        }
        let resolved = self.resolve_library_games(&keys);
        let run = StatusRun {
            run: self.next_op(),
            status,
            waiting: resolved
                .games
                .into_iter()
                .map(|game| game.game_id)
                .collect(),
            step: None,
            changed: 0,
            failed: 0,
            skipped_unmatched: resolved.skipped_unmatched,
            skipped_not_in_library: resolved.skipped_not_in_library,
            skipped_unknown_status: resolved.skipped_unknown_status,
        };
        if run.waiting.is_empty() {
            self.last_status_run = Some(run.view());
            return vec![];
        }
        self.status_run = Some(run);
        self.advance_status()
    }

    fn advance_status(&mut self) -> Vec<Effect> {
        let Some(mut run) = self.status_run.take() else {
            return vec![];
        };
        let Some(game_id) = run.waiting.pop_front() else {
            self.last_status_run = Some(run.view());
            return vec![];
        };
        let op = self.next_op();
        run.step = Some(StatusStep {
            game_id,
            op,
            kind: StatusStepKind::Checking,
            collection_ids: Vec::new(),
        });
        self.status_run = Some(run);
        vec![Effect::ReadEntry { op, game_id }]
    }

    /// The fresh full entry read before a status write.
    pub(super) fn on_status_checked(
        &mut self,
        op: u64,
        game_id: u64,
        result: Result<Option<LibraryEntry>, ApiError>,
    ) -> Vec<Effect> {
        let Some(mut run) = self.status_run.take() else {
            return vec![];
        };
        if run.step.as_ref().is_some_and(|step| {
            step.op == op && step.game_id == game_id && step.kind == StatusStepKind::Refreshing
        }) {
            self.status_run = Some(run);
            return self.on_status_refreshed(op, game_id, result);
        }
        if !run.step.as_ref().is_some_and(|step| {
            step.op == op && step.game_id == game_id && step.kind == StatusStepKind::Checking
        }) {
            self.status_run = Some(run);
            return vec![];
        }
        run.step = None;
        let status = run.status;
        match result {
            Err(ApiError::Rejected) => {
                run.failed += 1 + run.waiting.len();
                run.waiting.clear();
                self.last_status_run = Some(run.view());
                vec![]
            }
            Err(ApiError::Failed) => {
                run.failed += 1;
                self.status_run = Some(run);
                self.advance_status()
            }
            Ok(None) => {
                run.skipped_not_in_library += 1;
                let mut effects = self.record_entry_read(game_id, &Ok(None));
                self.status_run = Some(run);
                effects.extend(self.advance_status());
                effects
            }
            Ok(Some(entry)) => {
                let mut effects = self.record_entry_read(game_id, &Ok(Some(entry.clone())));
                if entry.status.is_none() {
                    // Never overwrite a server status this build cannot name.
                    run.skipped_unknown_status += 1;
                    self.status_run = Some(run);
                    effects.extend(self.advance_status());
                    return effects;
                }
                let collection_ids = entry.collection_ids.clone();
                let op = self.next_op();
                run.step = Some(StatusStep {
                    game_id,
                    op,
                    kind: StatusStepKind::Updating,
                    collection_ids: collection_ids.clone(),
                });
                self.status_run = Some(run);
                effects.push(Effect::UpdateEntry {
                    op,
                    game_id,
                    status,
                    collection_ids,
                    existing: entry,
                });
                effects
            }
        }
    }

    /// The answer to a status `UpdateEntry`.
    pub(super) fn on_status_updated(
        &mut self,
        op: u64,
        game_id: u64,
        result: Result<(), ApiError>,
    ) -> Vec<Effect> {
        let Some(mut run) = self.status_run.take() else {
            return vec![];
        };
        if !run.step.as_ref().is_some_and(|step| {
            step.op == op && step.game_id == game_id && step.kind == StatusStepKind::Updating
        }) {
            self.status_run = Some(run);
            return vec![];
        }
        let Some(step) = run.step.take() else {
            self.status_run = Some(run);
            return vec![];
        };
        match result {
            Ok(()) => {
                run.changed += 1;
                self.membership
                    .insert(game_id, Membership::InLibrary(Some(run.status)));
                let mut effects = Vec::new();
                if let Some(effect) =
                    self.update_cached_entry(game_id, run.status, &step.collection_ids)
                {
                    effects.push(effect);
                }
                let op = self.next_op();
                run.step = Some(StatusStep {
                    game_id,
                    op,
                    kind: StatusStepKind::Refreshing,
                    collection_ids: step.collection_ids,
                });
                self.status_run = Some(run);
                effects.push(Effect::ReadEntry { op, game_id });
                effects
            }
            Err(ApiError::Rejected) => {
                run.failed += 1 + run.waiting.len();
                run.waiting.clear();
                self.last_status_run = Some(run.view());
                vec![]
            }
            Err(ApiError::Failed) => {
                run.failed += 1;
                // The failed write may still have changed the server: read the game back before
                // moving on, so the row shows its real state.
                let op = self.next_op();
                run.step = Some(StatusStep {
                    game_id,
                    op,
                    kind: StatusStepKind::Refreshing,
                    collection_ids: step.collection_ids,
                });
                self.status_run = Some(run);
                vec![Effect::ReadEntry { op, game_id }]
            }
        }
    }

    /// The confirmation read after a status write.
    pub(super) fn on_status_refreshed(
        &mut self,
        op: u64,
        game_id: u64,
        result: Result<Option<LibraryEntry>, ApiError>,
    ) -> Vec<Effect> {
        let Some(mut run) = self.status_run.take() else {
            return vec![];
        };
        if !run.step.as_ref().is_some_and(|step| {
            step.op == op && step.game_id == game_id && step.kind == StatusStepKind::Refreshing
        }) {
            self.status_run = Some(run);
            return vec![];
        }
        run.step = None;
        let mut effects = self.record_entry_read(game_id, &result);
        self.status_run = Some(run);
        effects.extend(self.advance_status());
        effects
    }

    /// Adds every selected game already in the GameNode library to one collection, keeping its
    /// status and its other collections.
    pub(super) fn add_to_collection(
        &mut self,
        keys: Vec<String>,
        collection_id: String,
    ) -> Vec<Effect> {
        if self.network_block().is_some() || self.collection_run.is_some() {
            return vec![];
        }
        let resolved = self.resolve_library_games(&keys);
        let collection_name = self
            .account_library
            .as_ref()
            .and_then(|library| {
                library
                    .collections
                    .iter()
                    .find(|collection| collection.id == collection_id)
            })
            .map_or_else(
                || collection_id.clone(),
                |collection| collection.name.clone(),
            );
        let mut run = CollectionRun {
            run: self.next_op(),
            collection_id: collection_id.clone(),
            collection_name,
            waiting: VecDeque::new(),
            step: None,
            changed: 0,
            failed: 0,
            skipped_unmatched: resolved.skipped_unmatched,
            skipped_not_in_library: resolved.skipped_not_in_library,
            skipped_unknown_status: resolved.skipped_unknown_status,
            skipped_already_in_collection: 0,
        };
        for game in resolved.games {
            if game.collection_ids.iter().any(|id| id == &collection_id) {
                run.skipped_already_in_collection += 1;
            } else {
                run.waiting.push_back(game.game_id);
            }
        }
        if run.waiting.is_empty() {
            self.last_collection_run = Some(run.view());
            return vec![];
        }
        self.collection_run = Some(run);
        self.advance_collection()
    }

    fn advance_collection(&mut self) -> Vec<Effect> {
        let Some(mut run) = self.collection_run.take() else {
            return vec![];
        };
        let Some(game_id) = run.waiting.pop_front() else {
            self.last_collection_run = Some(run.view());
            return vec![];
        };
        let op = self.next_op();
        run.step = Some(CollectionStep {
            game_id,
            op,
            kind: CollectionStepKind::Checking,
            status: EntryStatus::Playing,
            collection_ids: Vec::new(),
        });
        self.collection_run = Some(run);
        vec![Effect::ReadEntry { op, game_id }]
    }

    /// The fresh full entry read before a collection write.
    pub(super) fn on_collection_checked(
        &mut self,
        op: u64,
        game_id: u64,
        result: Result<Option<LibraryEntry>, ApiError>,
    ) -> Vec<Effect> {
        let Some(mut run) = self.collection_run.take() else {
            return vec![];
        };
        if run.step.as_ref().is_some_and(|step| {
            step.op == op && step.game_id == game_id && step.kind == CollectionStepKind::Refreshing
        }) {
            self.collection_run = Some(run);
            return self.on_collection_refreshed(op, game_id, result);
        }
        if !run.step.as_ref().is_some_and(|step| {
            step.op == op && step.game_id == game_id && step.kind == CollectionStepKind::Checking
        }) {
            self.collection_run = Some(run);
            return vec![];
        }
        run.step = None;
        match result {
            Err(ApiError::Rejected) => {
                run.failed += 1 + run.waiting.len();
                run.waiting.clear();
                self.last_collection_run = Some(run.view());
                vec![]
            }
            Err(ApiError::Failed) => {
                run.failed += 1;
                self.collection_run = Some(run);
                self.advance_collection()
            }
            Ok(None) => {
                run.skipped_not_in_library += 1;
                let mut effects = self.record_entry_read(game_id, &Ok(None));
                self.collection_run = Some(run);
                effects.extend(self.advance_collection());
                effects
            }
            Ok(Some(entry)) => {
                let mut effects = self.record_entry_read(game_id, &Ok(Some(entry.clone())));
                let Some(status) = entry.status else {
                    run.skipped_unknown_status += 1;
                    self.collection_run = Some(run);
                    effects.extend(self.advance_collection());
                    return effects;
                };
                if entry
                    .collection_ids
                    .iter()
                    .any(|id| id == &run.collection_id)
                {
                    run.skipped_already_in_collection += 1;
                    self.collection_run = Some(run);
                    effects.extend(self.advance_collection());
                    return effects;
                }
                let mut collection_ids = entry.collection_ids.clone();
                collection_ids.push(run.collection_id.clone());
                let op = self.next_op();
                run.step = Some(CollectionStep {
                    game_id,
                    op,
                    kind: CollectionStepKind::Updating,
                    status,
                    collection_ids: collection_ids.clone(),
                });
                self.collection_run = Some(run);
                effects.push(Effect::UpdateEntry {
                    op,
                    game_id,
                    status,
                    collection_ids,
                    existing: entry,
                });
                effects
            }
        }
    }

    /// The answer to a collection `UpdateEntry`.
    pub(super) fn on_collection_updated(
        &mut self,
        op: u64,
        game_id: u64,
        result: Result<(), ApiError>,
    ) -> Vec<Effect> {
        let Some(mut run) = self.collection_run.take() else {
            return vec![];
        };
        if !run.step.as_ref().is_some_and(|step| {
            step.op == op && step.game_id == game_id && step.kind == CollectionStepKind::Updating
        }) {
            self.collection_run = Some(run);
            return vec![];
        }
        let Some(step) = run.step.take() else {
            self.collection_run = Some(run);
            return vec![];
        };
        match result {
            Ok(()) => {
                run.changed += 1;
                self.membership
                    .insert(game_id, Membership::InLibrary(Some(step.status)));
                let mut effects = Vec::new();
                if let Some(effect) =
                    self.update_cached_entry(game_id, step.status, &step.collection_ids)
                {
                    effects.push(effect);
                }
                let op = self.next_op();
                run.step = Some(CollectionStep {
                    game_id,
                    op,
                    kind: CollectionStepKind::Refreshing,
                    status: step.status,
                    collection_ids: step.collection_ids,
                });
                self.collection_run = Some(run);
                effects.push(Effect::ReadEntry { op, game_id });
                effects
            }
            Err(ApiError::Rejected) => {
                run.failed += 1 + run.waiting.len();
                run.waiting.clear();
                self.last_collection_run = Some(run.view());
                vec![]
            }
            Err(ApiError::Failed) => {
                run.failed += 1;
                // Refresh the game even after a failed write, so its row shows what GameNode has.
                let op = self.next_op();
                run.step = Some(CollectionStep {
                    game_id,
                    op,
                    kind: CollectionStepKind::Refreshing,
                    status: step.status,
                    collection_ids: step.collection_ids,
                });
                self.collection_run = Some(run);
                vec![Effect::ReadEntry { op, game_id }]
            }
        }
    }

    /// The confirmation read after a collection write.
    pub(super) fn on_collection_refreshed(
        &mut self,
        op: u64,
        game_id: u64,
        result: Result<Option<LibraryEntry>, ApiError>,
    ) -> Vec<Effect> {
        let Some(mut run) = self.collection_run.take() else {
            return vec![];
        };
        if !run.step.as_ref().is_some_and(|step| {
            step.op == op && step.game_id == game_id && step.kind == CollectionStepKind::Refreshing
        }) {
            self.collection_run = Some(run);
            return vec![];
        }
        run.step = None;
        let mut effects = self.record_entry_read(game_id, &result);
        self.collection_run = Some(run);
        effects.extend(self.advance_collection());
        effects
    }

    /// Deletes exactly the entries the latest add created. A different run is stale and ignored.
    pub(super) fn undo_add(&mut self, run: u64) -> Vec<Effect> {
        if self.network_block().is_some() || self.undo.is_some() {
            return vec![];
        }
        let Some(summary) = self.last_add.as_ref() else {
            return vec![];
        };
        if summary.run != run || summary.created.is_empty() {
            return vec![];
        }
        if self.last_undo.as_ref().is_some_and(|undo| undo.run == run) {
            return vec![];
        }
        self.undo = Some(UndoRun {
            run,
            waiting: summary.created.clone().into(),
            step: None,
            deleted: 0,
            failed: 0,
        });
        self.advance_undo()
    }

    fn advance_undo(&mut self) -> Vec<Effect> {
        let Some(mut undo) = self.undo.take() else {
            return vec![];
        };
        let Some(created) = undo.waiting.pop_front() else {
            let state = if undo.failed == 0 {
                UndoState::Done
            } else if undo.deleted == 0 {
                UndoState::Failed
            } else {
                UndoState::Partial
            };
            self.last_undo = Some(UndoAddView {
                run: undo.run,
                state,
                deleted: undo.deleted,
                failed: undo.failed,
            });
            self.mark_add_undone(undo.run);
            return vec![];
        };
        let op = self.next_op();
        undo.step = Some(UndoStep {
            op,
            game_id: created.game_id,
        });
        self.undo = Some(undo);
        vec![Effect::DeleteEntry {
            op,
            game_id: created.game_id,
            entry_id: created.entry_id,
        }]
    }

    /// The answer to one `DeleteEntry`. A successful delete removes the cached entry, then reads
    /// the game once so the state column shows GameNode's result.
    pub(super) fn on_entry_deleted(
        &mut self,
        op: u64,
        game_id: u64,
        result: Result<(), ApiError>,
    ) -> Vec<Effect> {
        let Some(mut undo) = self.undo.take() else {
            return vec![];
        };
        if !undo
            .step
            .as_ref()
            .is_some_and(|step| step.op == op && step.game_id == game_id)
        {
            self.undo = Some(undo);
            return vec![];
        }
        undo.step = None;
        let mut effects = Vec::new();
        match result {
            Ok(()) => {
                undo.deleted += 1;
                self.membership.insert(game_id, Membership::NotInLibrary);
                effects.extend(self.remove_cached_account_entry(game_id));
            }
            Err(ApiError::Rejected) => {
                undo.failed += 1 + undo.waiting.len();
                undo.waiting.clear();
                self.last_undo = Some(UndoAddView {
                    run: undo.run,
                    state: if undo.deleted == 0 {
                        UndoState::Failed
                    } else {
                        UndoState::Partial
                    },
                    deleted: undo.deleted,
                    failed: undo.failed,
                });
                self.mark_add_undone(undo.run);
                return effects;
            }
            Err(ApiError::Failed) => {
                undo.failed += 1;
            }
        }
        // Refresh every affected game, including one whose delete failed: its state stays visible.
        let read_op = self.next_op();
        self.membership_reads.insert(game_id, read_op);
        effects.push(Effect::ReadEntry {
            op: read_op,
            game_id,
        });
        self.undo = Some(undo);
        effects.extend(self.advance_undo());
        effects
    }

    /// An add that was undone (fully or not) can no longer offer Undo, in the stored summary and
    /// in a dialog that is still open.
    fn mark_add_undone(&mut self, run: u64) {
        if let Some(summary) = &mut self.last_add
            && summary.run == run
        {
            summary.created.clear();
        }
        if let Some(add) = &mut self.add
            && let super::add_dialog::Stage::Done(summary) = &mut add.stage
            && summary.run == run
        {
            summary.created.clear();
        }
    }

    /// Removes one game's cached account entry and persists the removal. Before the first complete
    /// read there is nothing to persist, but the removal is still remembered so a late first read
    /// cannot resurrect an entry the undo just deleted.
    fn remove_cached_account_entry(&mut self, game_id: u64) -> Vec<Effect> {
        self.account_entries.remove(&game_id);
        let Some(account_id) = self.account.as_ref().map(|account| account.id.clone()) else {
            return vec![];
        };
        let complete = self.account_library.is_some();
        if let Some(library) = self.account_library.as_mut() {
            library.entries.remove(&game_id);
        }
        self.record_local_account_entry(game_id, None);
        if !complete {
            return vec![];
        }
        vec![Effect::PersistAccountEntry {
            account_id,
            game_id,
            entry: None,
        }]
    }
}

#[cfg(test)]
mod tests;
