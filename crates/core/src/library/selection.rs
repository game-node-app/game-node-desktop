//! Which library entries are selected, and moving around the grid and list with the keyboard.
//!
//! The stored selection can hold keys that a filter or search currently hides. They are not part
//! of what is *shown* as selected or acted on ([`Selection::visible`]), but they come back when
//! the filter is cleared. Moving the pointer or the keyboard always works on the visible order.

use std::collections::BTreeSet;

use super::machine::LibraryMachine;

/// How a click or Enter changes the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectMode {
    /// A plain click: only this entry.
    Replace,
    /// Ctrl-click: add or remove this entry.
    Toggle,
    /// Shift-click: everything between the anchor and this entry.
    Range,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
    /// First entry in the list.
    Home,
    /// Last entry in the list.
    End,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    selected: BTreeSet<String>,
    /// Where a Shift-extend starts: the last plain or Ctrl-selected entry.
    anchor: Option<String>,
    /// The entry the keyboard is on; drawn with the focus ring.
    focus: Option<String>,
}

fn index_of(visible: &[String], key: &str) -> Option<usize> {
    visible.iter().position(|candidate| candidate == key)
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }

    /// The focused entry, if it is in `visible`.
    pub fn focus<'a>(&'a self, visible: &[String]) -> Option<&'a str> {
        self.focus
            .as_deref()
            .filter(|key| index_of(visible, key).is_some())
    }

    /// The selected entries that are in `visible`, in the order they are shown.
    pub fn visible(&self, visible: &[String]) -> Vec<String> {
        visible
            .iter()
            .filter(|key| self.selected.contains(*key))
            .cloned()
            .collect()
    }

    /// Drops entries that no longer exist (not installed any more).
    pub fn retain(&mut self, mut keep: impl FnMut(&str) -> bool) {
        self.selected.retain(|key| keep(key));
        if self.anchor.as_deref().is_some_and(|key| !keep(key)) {
            self.anchor = None;
        }
        if self.focus.as_deref().is_some_and(|key| !keep(key)) {
            self.focus = None;
        }
    }

    fn only(&mut self, key: &str) {
        self.selected = BTreeSet::from([key.to_owned()]);
        self.anchor = Some(key.to_owned());
        self.focus = Some(key.to_owned());
    }

    /// Selects everything between the anchor and `key` (inclusive), or just `key` when the
    /// anchor is not on screen.
    fn extend_to(&mut self, key: &str, visible: &[String]) {
        let (Some(anchor), Some(to)) = (
            self.anchor
                .as_deref()
                .and_then(|anchor| index_of(visible, anchor)),
            index_of(visible, key),
        ) else {
            return self.only(key);
        };
        let (first, last) = (anchor.min(to), anchor.max(to));
        self.selected = visible[first..=last].iter().cloned().collect();
        self.focus = Some(key.to_owned());
    }

    /// Applies a click. A key that is not in `visible` is ignored.
    pub fn select(&mut self, key: &str, mode: SelectMode, visible: &[String]) {
        if index_of(visible, key).is_none() {
            return;
        }
        match mode {
            SelectMode::Replace => self.only(key),
            SelectMode::Toggle => {
                if !self.selected.remove(key) {
                    self.selected.insert(key.to_owned());
                }
                self.anchor = Some(key.to_owned());
                self.focus = Some(key.to_owned());
            }
            SelectMode::Range => self.extend_to(key, visible),
        }
    }

    /// Moves the focus. `columns` is how many entries a grid row holds (1 in the list). Without
    /// `extend` the entry moved to becomes the only selected one; with it the selection grows
    /// from the anchor, as Shift-click does.
    pub fn step(&mut self, direction: Direction, extend: bool, columns: usize, visible: &[String]) {
        let Some(last) = visible.len().checked_sub(1) else {
            return;
        };
        let columns = columns.max(1);
        let current = self.focus(visible).and_then(|key| index_of(visible, key));
        let target = match (current, direction) {
            (_, Direction::Home) => 0,
            (_, Direction::End) => last,
            // Nothing focused yet: the first move lands on the first entry (the last for Up).
            (None, Direction::Up) => last,
            (None, _) => 0,
            (Some(at), Direction::Left) => at.saturating_sub(1),
            (Some(at), Direction::Right) => (at + 1).min(last),
            (Some(at), Direction::Up) => at.checked_sub(columns).unwrap_or(at),
            (Some(at), Direction::Down) => {
                // A partial last row has no entry below some columns: go to its last entry.
                let last_row_start = last / columns * columns;
                if at < last_row_start {
                    (at + columns).min(last)
                } else {
                    at
                }
            }
        };
        let key = &visible[target];
        if extend {
            self.extend_to(key, visible);
        } else {
            self.only(key);
        }
    }
}

impl LibraryMachine {
    /// The keys of the rows shown, in the order shown.
    pub fn visible_keys(&self) -> Vec<String> {
        self.visible_rows(&self.search)
            .into_iter()
            .map(|row| row.key)
            .collect()
    }

    /// The selected entries that are currently shown, in the order shown.
    pub fn selected_keys(&self) -> Vec<String> {
        self.selection.visible(&self.visible_keys())
    }

    /// The entry the keyboard is on, if it is shown.
    pub fn focused_key(&self) -> Option<String> {
        self.selection
            .focus(&self.visible_keys())
            .map(str::to_owned)
    }

    pub(super) fn select(&mut self, key: &str, mode: SelectMode) {
        let visible = self.visible_keys();
        self.selection.select(key, mode, &visible);
    }

    pub(super) fn step(&mut self, direction: Direction, extend: bool) {
        let visible = self.visible_keys();
        let columns = self.columns();
        self.selection.step(direction, extend, columns, &visible);
    }

    /// Entries per row: the grid's column count the window reported, or 1 in the list.
    pub(super) fn columns(&self) -> usize {
        match self.layout.view {
            super::layout::ViewMode::Grid => self.grid_columns.max(1),
            super::layout::ViewMode::List => 1,
        }
    }

    /// Forgets selected entries that are no longer installed.
    pub(super) fn prune_selection(&mut self) {
        let entries = &self.entries;
        self.selection
            .retain(|key| entries.get(key).is_some_and(|entry| entry.installed));
    }
}

#[cfg(test)]
mod tests;
