//! Account-owned organization data: saved views and hidden entries.
//!
//! A saved view restores a scope, filters, sort, grouping and view mode. It belongs to the
//! signed-in account and is stored by the platform; the machine keeps the list in memory so the
//! sidebar can show it and count its rows without another read.
//!
//! Hidden entries are display preferences too: they are stored per account and key, but the entry
//! is still scanned, matched, tracked and launched exactly as before.

use serde::{Deserialize, Serialize};

use super::layout::{GroupBy, LibraryFilters, LibraryLayout, LibraryScope, Sort, ViewMode};
use super::machine::{Effect, LibraryMachine};

/// Everything a saved view restores. The search text is deliberately absent: it is never saved.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SavedViewPayload {
    pub scope: LibraryScope,
    pub filters: LibraryFilters,
    pub sort: Sort,
    pub group: GroupBy,
    pub view: ViewMode,
}

/// One named view of the sidebar, stored per account.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SavedView {
    /// The database id. Zero until the insert answer supplies the real AUTOINCREMENT id.
    pub id: i64,
    pub name: String,
    pub payload: SavedViewPayload,
    /// The sidebar position.
    pub position: i64,
    /// Unix seconds.
    pub created_at: i64,
}

/// Why the machine refused a view name. The dialog also validates live against `savedViews`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ViewError {
    /// The trimmed name is empty.
    Empty,
    /// Another view of the account already has this name, ignoring case.
    Duplicate,
}

/// One successful save, for the "view saved" notification. `run` identifies the save so the UI
/// notifies once per save instead of once per snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedViewNotice {
    pub id: i64,
    pub name: String,
    pub run: u64,
}

/// Which saved-view write an attempt belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ViewWriteOperation {
    Save,
    Rename,
}

/// How a saved-view attempt ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ViewWriteState {
    /// The input was accepted and the store has not answered yet.
    Pending,
    /// The store confirmed the write. The memory and disk state already match it.
    Succeeded,
    /// A refused name or a failed store write. The dialog stays open and can retry.
    Failed,
}

/// Why a saved-view attempt failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ViewWriteError {
    /// The trimmed name is empty.
    Empty,
    /// Another view of the account already has this name, ignoring case.
    Duplicate,
    /// The storage worker failed. Nothing was written; a retry is safe.
    Failed,
}

impl From<ViewError> for ViewWriteError {
    fn from(error: ViewError) -> Self {
        match error {
            ViewError::Empty => Self::Empty,
            ViewError::Duplicate => Self::Duplicate,
        }
    }
}

/// The latest saved-view attempt, so the dialog can tell a success or failure apart from the
/// previous attempt and stop its spinner. `run` is a monotonically increasing attempt id (the
/// `PersistView` op once a write starts); the UI baselines it before submitting and reacts only to
/// a larger run. A validation refusal gets its own run too, so repeating the same refusal still
/// ends the attempt instead of looking unchanged.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewWriteView {
    pub run: u64,
    pub operation: ViewWriteOperation,
    /// The id of the view being renamed; 0 for a new view.
    pub view_id: i64,
    /// The trimmed name the attempt used, for the UI to correlate its submit.
    pub name: String,
    pub state: ViewWriteState,
    /// Set exactly when `state` is `Failed`.
    pub error: Option<ViewWriteError>,
    /// The database id after a successful save; `None` while pending or after a failure.
    pub saved_view_id: Option<i64>,
}

impl ViewWriteView {
    fn pending(run: u64, operation: ViewWriteOperation, view_id: i64, name: String) -> Self {
        Self {
            run,
            operation,
            view_id,
            name,
            state: ViewWriteState::Pending,
            error: None,
            saved_view_id: None,
        }
    }

    fn failed(
        run: u64,
        operation: ViewWriteOperation,
        view_id: i64,
        name: String,
        error: ViewWriteError,
    ) -> Self {
        Self {
            run,
            operation,
            view_id,
            name,
            state: ViewWriteState::Failed,
            error: Some(error),
            saved_view_id: None,
        }
    }
}

/// One hide run, for the "hidden from the library" notification and its Show again action. `run`
/// identifies the run so the UI notifies once per hide.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HiddenNotice {
    pub keys: Vec<String>,
    pub hidden: bool,
    pub run: u64,
}

impl SavedViewPayload {
    /// The payload as it is applied to the layout: legacy filter keys folded into their new
    /// equivalents. A saved view can never point at another saved view, so a stored marker falls
    /// back to All games instead of recursing.
    pub fn normalized(&self) -> Self {
        Self {
            scope: match &self.scope {
                LibraryScope::SavedView(_) => LibraryScope::All,
                scope => scope.clone(),
            },
            filters: self.filters.clone().normalized(),
            sort: self.sort,
            group: self.group,
            view: self.view,
        }
    }

    /// A payload taken from the layout. `scope` is the *effective* scope: a highlighted saved view
    /// contributes its own payload scope, never the marker id.
    pub fn of_layout(layout: &LibraryLayout, scope: LibraryScope) -> Self {
        Self {
            scope,
            filters: layout.filters.clone().normalized(),
            sort: layout.sort,
            group: layout.group,
            view: layout.view,
        }
    }

    /// Whether this payload matches the default library: no scope, facet, sort, grouping or view
    /// mode to save. The Save as view action is only offered when this is false.
    pub fn is_default(&self) -> bool {
        let payload = self.normalized();
        payload.scope == LibraryScope::All
            && payload.filters.is_default()
            && payload.sort == Sort::default()
            && payload.group == GroupBy::default()
            && payload.view == ViewMode::default()
    }
}

/// Trims `name` and checks it is non-empty and unique for the account, ignoring case. `exclude`
/// is the view being renamed, so changing only its capitalization is allowed.
pub fn validate_view_name(
    name: &str,
    views: &[SavedView],
    exclude: Option<i64>,
) -> Result<String, ViewError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ViewError::Empty);
    }
    let folded = trimmed.to_lowercase();
    let duplicate = views
        .iter()
        .filter(|view| Some(view.id) != exclude)
        .any(|view| view.name.trim().to_lowercase() == folded);
    if duplicate {
        return Err(ViewError::Duplicate);
    }
    Ok(trimmed.to_owned())
}

impl LibraryMachine {
    /// The scope the rows are filtered by. A highlighted saved view contributes its payload scope;
    /// the layout only holds the `SavedView(id)` marker that names it in the header.
    pub(super) fn effective_scope(&self) -> &LibraryScope {
        match (
            &self.layout.scope,
            self.applied_view_id,
            &self.applied_view_payload,
        ) {
            (LibraryScope::SavedView(id), Some(applied_id), Some(payload)) if *id == applied_id => {
                &payload.scope
            }
            (scope, _, _) => scope,
        }
    }

    /// The payload Save as view would store now. A highlighted view contributes its own effective
    /// scope, never the marker, so saving a copy cannot recurse into `SavedView(id)`.
    pub(super) fn current_payload(&self) -> SavedViewPayload {
        SavedViewPayload::of_layout(&self.layout, self.effective_scope().clone())
    }

    /// Whether there is anything beyond the default library to save.
    pub(super) fn can_save_view(&self) -> bool {
        !self.current_payload().is_default()
    }

    fn organization_account_id(&self) -> Option<String> {
        self.account.as_ref().map(|account| account.id.clone())
    }

    /// Saves the effective payload under a trimmed, unique name. The view is only added to memory
    /// once the store answers with its real database id; a failed write leaves no phantom row.
    pub(super) fn save_view(&mut self, name: String) -> Vec<Effect> {
        if !matches!(self.phase, super::machine::Phase::Ready) {
            return vec![];
        }
        self.view_error = None;
        let payload = self.current_payload();
        // The UI only offers Save as view when this is true; a direct input for a default library
        // is a no-op rather than a nameless duplicate of the default state.
        if payload.is_default() {
            return vec![];
        }
        let run = self.next_op();
        let operation = ViewWriteOperation::Save;
        let name = match validate_view_name(&name, &self.saved_views, None) {
            Ok(name) => name,
            Err(error) => {
                self.view_error = Some(error);
                self.record_view_write(ViewWriteView::failed(
                    run,
                    operation,
                    0,
                    name.trim().to_owned(),
                    error.into(),
                ));
                return vec![];
            }
        };
        let Some(account_id) = self.organization_account_id() else {
            // Without an account there is nothing to write; the attempt still ends so the dialog
            // does not wait for an answer that will never come.
            self.record_view_write(ViewWriteView::failed(
                run,
                operation,
                0,
                name,
                ViewWriteError::Failed,
            ));
            return vec![];
        };
        let view = SavedView {
            id: 0,
            name: name.clone(),
            payload,
            position: 0,
            created_at: self.now,
        };
        self.record_view_write(ViewWriteView::pending(run, operation, 0, name));
        self.pending_view_saves.insert(
            run,
            super::machine::PendingViewSave {
                account_id: account_id.clone(),
                view: view.clone(),
                operation,
            },
        );
        vec![Effect::PersistView {
            op: run,
            account_id,
            view,
        }]
    }

    /// Renames one saved view once the store confirms the write. The payload is untouched.
    pub(super) fn rename_view(&mut self, id: i64, name: String) -> Vec<Effect> {
        if !matches!(self.phase, super::machine::Phase::Ready) {
            return vec![];
        }
        self.view_error = None;
        let Some(existing) = self.saved_views.iter().find(|view| view.id == id).cloned() else {
            return vec![];
        };
        let run = self.next_op();
        let operation = ViewWriteOperation::Rename;
        let name = match validate_view_name(&name, &self.saved_views, Some(id)) {
            Ok(name) => name,
            Err(error) => {
                self.view_error = Some(error);
                self.record_view_write(ViewWriteView::failed(
                    run,
                    operation,
                    id,
                    name.trim().to_owned(),
                    error.into(),
                ));
                return vec![];
            }
        };
        let Some(account_id) = self.organization_account_id() else {
            self.record_view_write(ViewWriteView::failed(
                run,
                operation,
                id,
                name,
                ViewWriteError::Failed,
            ));
            return vec![];
        };
        let view = SavedView {
            name: name.clone(),
            ..existing
        };
        self.record_view_write(ViewWriteView::pending(run, operation, id, name));
        self.pending_view_saves.insert(
            run,
            super::machine::PendingViewSave {
                account_id: account_id.clone(),
                view: view.clone(),
                operation,
            },
        );
        vec![Effect::PersistView {
            op: run,
            account_id,
            view,
        }]
    }

    /// Deletes a view. Deleting the highlighted one keeps its effective filters, scope, sort,
    /// grouping and view mode but drops the marker, so the header falls back to the payload scope
    /// instead of naming a view that no longer exists.
    pub(super) fn delete_view(&mut self, id: i64) -> Vec<Effect> {
        let Some(index) = self.saved_views.iter().position(|view| view.id == id) else {
            return vec![];
        };
        let Some(account_id) = self.organization_account_id() else {
            return vec![];
        };
        self.saved_views.remove(index);
        self.pending_view_saves
            .retain(|_, pending| pending.view.id != id);
        // A rename that is still in flight can never land once its view is gone. Report it as
        // failed so a dialog waiting on that attempt stops instead of spinning forever.
        if let Some(write) = &mut self.view_write
            && write.state == ViewWriteState::Pending
            && write.view_id == id
        {
            write.state = ViewWriteState::Failed;
            write.error = Some(ViewWriteError::Failed);
            write.saved_view_id = None;
        }
        let mut effects = vec![Effect::DeleteViewEffect { account_id, id }];
        if matches!(self.layout.scope, LibraryScope::SavedView(active) if active == id) {
            let scope = if self.applied_view_id == Some(id) {
                self.applied_view_payload
                    .as_ref()
                    .map_or(LibraryScope::All, |payload| payload.scope.clone())
            } else {
                LibraryScope::All
            };
            effects.extend(self.update_layout(|layout| layout.scope = scope));
        }
        effects
    }

    /// Applies one saved view's payload to the layout and marks it as the highlighted view. An
    /// unknown id changes nothing, so a stale sidebar cannot leave a broken predicate or title.
    pub(super) fn apply_view(&mut self, id: i64) -> Vec<Effect> {
        let Some(payload) = self
            .saved_views
            .iter()
            .find(|view| view.id == id)
            .map(|view| view.payload.normalized())
        else {
            return vec![];
        };
        self.applied_view_payload = Some(payload.clone());
        self.applied_view_id = Some(id);
        self.update_layout(|layout| {
            layout.scope = LibraryScope::SavedView(id);
            layout.filters = payload.filters.clone();
            layout.sort = payload.sort;
            layout.group = payload.group;
            layout.view = payload.view;
        })
    }

    /// Hides or reveals keys for the account. Only the display preference and a stale selection
    /// change: matches, favorites, playtime and GameNode data are untouched.
    pub(super) fn set_hidden(&mut self, keys: Vec<String>, hidden: bool) -> Vec<Effect> {
        let Some(account_id) = self.organization_account_id() else {
            return vec![];
        };
        let mut seen = std::collections::BTreeSet::new();
        let changed: Vec<String> = keys
            .into_iter()
            .filter(|key| seen.insert(key.clone()) && self.hidden.contains(key) != hidden)
            .collect();
        if changed.is_empty() {
            return vec![];
        }
        for key in &changed {
            if hidden {
                self.hidden.insert(key.clone());
            } else {
                self.hidden.remove(key);
            }
        }
        if hidden {
            // A row that just left the normal view cannot stay selected.
            let now_hidden: std::collections::BTreeSet<&str> =
                changed.iter().map(String::as_str).collect();
            self.selection.retain(|key| !now_hidden.contains(key));
        }
        let effects = vec![Effect::PersistHidden {
            account_id,
            keys: changed.clone(),
            hidden,
            now: self.now,
        }];
        if hidden {
            let run = self.next_op();
            self.last_hidden = Some(HiddenNotice {
                keys: changed,
                hidden,
                run,
            });
        }
        effects
    }

    /// Applies the answer to [`Effect::PersistView`]. A stale answer (another account, or an
    /// input whose pending record is gone) is ignored. The id is always the one the database
    /// returned: nothing is derived from the account's current maximum.
    ///
    /// Every answer also lands in `view_write` with the same run, so the dialog can stop its
    /// spinner on a failure and retry without closing. A stale answer never overwrites a newer
    /// attempt's result.
    pub(super) fn on_view_persisted(
        &mut self,
        op: u64,
        account_id: String,
        result: Result<i64, ()>,
    ) -> Vec<Effect> {
        let Some(pending) = self.pending_view_saves.remove(&op) else {
            return vec![];
        };
        if pending.account_id != account_id
            || self.organization_account_id().as_deref() != Some(account_id.as_str())
        {
            return vec![];
        }
        let operation = pending.operation;
        let view_id = pending.view.id;
        let name = pending.view.name.clone();
        match result {
            Ok(id) => {
                if pending.view.id == 0 {
                    let position = self.saved_views.last().map_or(0, |view| view.position + 1);
                    let view = SavedView {
                        id,
                        position,
                        ..pending.view
                    };
                    self.last_saved_view = Some(SavedViewNotice {
                        id,
                        name: view.name.clone(),
                        run: op,
                    });
                    self.saved_views.push(view);
                } else if let Some(view) = self
                    .saved_views
                    .iter_mut()
                    .find(|view| view.id == pending.view.id)
                {
                    // A rename can never leave the view pointing at the marker of another view.
                    view.id = id;
                    view.name = pending.view.name;
                    view.payload = pending.view.payload;
                }
                self.view_error = None;
                self.record_view_write(ViewWriteView {
                    run: op,
                    operation,
                    view_id,
                    name,
                    state: ViewWriteState::Succeeded,
                    error: None,
                    saved_view_id: Some(id),
                });
            }
            Err(()) => {
                // The store kept its previous state; memory does too. No phantom view appears,
                // and the failed attempt is visible so the dialog stops waiting and can retry.
                tracing::warn!(op, "the library view could not be saved");
                self.record_view_write(ViewWriteView::failed(
                    op,
                    operation,
                    view_id,
                    name,
                    ViewWriteError::Failed,
                ));
            }
        }
        vec![]
    }

    /// Keeps the newest attempt visible. Answers and validation refusals share the run space, so
    /// an answer that arrives after a newer attempt started cannot replace its result.
    fn record_view_write(&mut self, write: ViewWriteView) {
        if self
            .view_write
            .as_ref()
            .is_none_or(|current| write.run >= current.run)
        {
            self.view_write = Some(write);
        }
    }

    /// Drops the saved-view marker once the layout no longer matches the payload it was applied
    /// with. Pane widths, the detail pane, collapsed groups and search are not part of the
    /// payload, so changing them keeps the view highlighted.
    pub(super) fn reconcile_applied_view(&mut self) {
        let marker = match self.layout.scope {
            LibraryScope::SavedView(id) => Some(id),
            _ => None,
        };
        let Some(id) = marker else {
            self.applied_view_payload = None;
            self.applied_view_id = None;
            return;
        };
        if self.applied_view_id != Some(id) {
            self.applied_view_payload = None;
            self.applied_view_id = None;
            return;
        }
        let Some(applied) = self.applied_view_payload.clone() else {
            self.applied_view_id = None;
            return;
        };
        let applied = applied.normalized();
        let current = SavedViewPayload::of_layout(&self.layout, applied.scope.clone()).normalized();
        if current != applied {
            self.layout.scope = applied.scope;
            self.applied_view_payload = None;
            self.applied_view_id = None;
        }
    }

    /// After a cache load, restores the payload of a highlighted saved view of this account and
    /// falls back to All games when the layout names a scope this account no longer has: an
    /// unknown saved view or a deleted collection. The persisted layout never keeps a predicate
    /// or title for a scope that cannot be applied.
    pub(super) fn reconcile_saved_view_scope(&mut self) -> Vec<Effect> {
        // A collection the account library read no longer lists was deleted on the server. Without
        // an account library read there is nothing to compare against, so the remembered scope
        // stays until a read says otherwise.
        if let LibraryScope::Collection(id) = self.layout.scope.clone()
            && let Some(library) = &self.account_library
            && !library
                .collections
                .iter()
                .any(|collection| collection.id == id)
        {
            return self.update_layout(|layout| layout.scope = LibraryScope::All);
        }
        let LibraryScope::SavedView(id) = self.layout.scope.clone() else {
            self.applied_view_payload = None;
            self.applied_view_id = None;
            return vec![];
        };
        let Some(payload) = self
            .saved_views
            .iter()
            .find(|view| view.id == id)
            .map(|view| view.payload.normalized())
        else {
            self.applied_view_payload = None;
            self.applied_view_id = None;
            return self.update_layout(|layout| layout.scope = LibraryScope::All);
        };
        let current = SavedViewPayload::of_layout(&self.layout, payload.scope.clone()).normalized();
        if current == payload {
            self.applied_view_payload = Some(payload);
            self.applied_view_id = Some(id);
            vec![]
        } else {
            // The stored layout is not the view's payload any more (for example a partial write):
            // keep the payload's scope, drop the marker, never a broken title.
            let scope = payload.scope.clone();
            self.applied_view_payload = None;
            self.applied_view_id = None;
            self.update_layout(|layout| layout.scope = scope)
        }
    }
}

#[cfg(test)]
mod tests;
