//! Which library entries are selected, and moving around the grid and list with the keyboard.
//!
//! The stored selection can hold keys that a filter or search currently hides. They are not part
//! of what is *shown* as selected or acted on ([`Selection::visible`]), but they come back when
//! the filter is cleared. Moving the pointer or the keyboard always works on the visible order.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::machine::LibraryMachine;

/// How a click or Enter changes the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SelectMode {
    /// A plain click: only this entry.
    Replace,
    /// Ctrl-click: add or remove this entry.
    Toggle,
    /// Shift-click: everything between the anchor and this entry.
    Range,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

    /// Clears the selection, the range anchor and the keyboard focus.
    pub fn clear(&mut self) {
        self.selected.clear();
        self.anchor = None;
        self.focus = None;
    }

    /// Selects every visible row. With nothing focused, the first visible row gets the focus.
    pub fn select_all(&mut self, visible: &[String]) {
        self.selected = visible.iter().cloned().collect();
        let focus_is_visible = self
            .focus
            .as_deref()
            .is_some_and(|key| index_of(visible, key).is_some());
        if !focus_is_visible {
            self.focus = visible.first().cloned();
        }
        self.anchor = self.focus.clone();
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

    /// Grid movement when groups are shown. Each group starts a new line, so up/down move between
    /// rendered lines and cross a group boundary to the nearest column (the last column when the
    /// target line is shorter). Left/right stay on the line. Collapsed groups are already absent
    /// from `visible` and `group_sizes` only lists expanded groups.
    pub fn step_grouped(
        &mut self,
        direction: Direction,
        extend: bool,
        columns: usize,
        visible: &[String],
        group_sizes: &[usize],
    ) {
        let Some(last) = visible.len().checked_sub(1) else {
            return;
        };
        let columns = columns.max(1);
        let lines = grid_lines(group_sizes, columns, visible.len());
        if lines.is_empty() {
            self.step(direction, extend, columns, visible);
            return;
        }

        let target = match (
            self.focus(visible).and_then(|key| index_of(visible, key)),
            direction,
        ) {
            (_, Direction::Home) => 0,
            (_, Direction::End) => last,
            (None, Direction::Up) => last,
            (None, _) => 0,
            (Some(at), Direction::Left) => {
                let line = line_of(&lines, at);
                let column = at - lines[line].start;
                if column > 0 { at - 1 } else { at }
            }
            (Some(at), Direction::Right) => {
                let line = line_of(&lines, at);
                let line_end = lines[line].start + lines[line].len - 1;
                if at < line_end { at + 1 } else { at }
            }
            (Some(at), Direction::Up) => {
                let line = line_of(&lines, at);
                if line == 0 {
                    at
                } else {
                    let column = at - lines[line].start;
                    let target = lines[line - 1];
                    target.start + column.min(target.len - 1)
                }
            }
            (Some(at), Direction::Down) => {
                let line = line_of(&lines, at);
                if line + 1 >= lines.len() {
                    at
                } else {
                    let column = at - lines[line].start;
                    let target = lines[line + 1];
                    target.start + column.min(target.len - 1)
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DisplayLine {
    start: usize,
    len: usize,
}

/// Splits the expanded groups into rendered grid lines: every group starts a new line.
fn grid_lines(group_sizes: &[usize], columns: usize, row_count: usize) -> Vec<DisplayLine> {
    let mut lines = Vec::new();
    let mut start = 0;
    for &size in group_sizes {
        let size = size.min(row_count.saturating_sub(start));
        let mut offset = 0;
        while offset < size {
            let len = (size - offset).min(columns);
            lines.push(DisplayLine {
                start: start + offset,
                len,
            });
            offset += len;
        }
        start += size;
    }
    // A key that belongs to no group (or a stale group count) still has a line to move on.
    while start < row_count {
        let len = (row_count - start).min(columns);
        lines.push(DisplayLine { start, len });
        start += len;
    }
    lines
}

fn line_of(lines: &[DisplayLine], index: usize) -> usize {
    lines
        .iter()
        .position(|line| index >= line.start && index < line.start + line.len)
        .unwrap_or_else(|| lines.len().saturating_sub(1))
}

impl LibraryMachine {
    /// The keys of the rows shown, in the order shown.
    pub fn visible_keys(&self) -> Vec<String> {
        self.visible_rows(&self.search)
            .into_iter()
            .map(|row| row.key)
            .collect()
    }

    /// The keys shown plus the number of visible rows each expanded group contributes, in order.
    fn visible_structure(&self) -> (Vec<String>, Vec<usize>) {
        let query = self.query(&self.search);
        let keys = query.rows.iter().map(|row| row.key.clone()).collect();
        let groups = query
            .groups
            .iter()
            .filter(|group| !group.collapsed)
            .map(|group| group.count)
            .collect();
        (keys, groups)
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

    pub(super) fn select_all(&mut self) {
        let visible = self.visible_keys();
        self.selection.select_all(&visible);
    }

    pub(super) fn step(&mut self, direction: Direction, extend: bool) {
        let (visible, groups) = self.visible_structure();
        let columns = self.columns();
        if self.layout.view == super::layout::ViewMode::Grid
            && self.layout.group != super::layout::GroupBy::None
        {
            self.selection
                .step_grouped(direction, extend, columns, &visible, &groups);
        } else {
            self.selection.step(direction, extend, columns, &visible);
        }
    }

    /// Entries per row: the grid's column count the window reported, or 1 in the list.
    pub(super) fn columns(&self) -> usize {
        match self.layout.view {
            super::layout::ViewMode::Grid => self.grid_columns.max(1),
            super::layout::ViewMode::List => 1,
        }
    }

    /// Forgets selected entries that the library no longer shows.
    pub(super) fn prune_selection(&mut self) {
        let entries = &self.entries;
        let excluded = &self.excluded;
        self.selection.retain(|key| {
            entries
                .get(key)
                .is_some_and(|entry| entry.is_visible() && !excluded.contains(key))
        });
    }

    /// Forgets selected entries whose row is inside a collapsed group of the active grouping.
    /// Expanding the group again never brings them back into the selection.
    pub(super) fn prune_collapsed_selection(&mut self) {
        let group = self.layout.group;
        if group == super::layout::GroupBy::None {
            return;
        }
        let collapsed = self
            .layout
            .collapsed_groups
            .get(&group)
            .cloned()
            .unwrap_or_default();
        if collapsed.is_empty() {
            return;
        }
        let keys: BTreeSet<String> = self
            .all_rows()
            .into_iter()
            .filter(|row| collapsed.contains(&super::query::group_id(row, group)))
            .map(|row| row.key)
            .collect();
        if keys.is_empty() {
            return;
        }
        self.selection.retain(|key| !keys.contains(key));
    }
}

#[cfg(test)]
mod tests;
