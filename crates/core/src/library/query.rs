//! The rows the library shows, and how they are filtered, searched, sorted and counted.
//!
//! Everything here is a pure function over [`Row`]s, so the rules are tested without a machine.

use std::cmp::Ordering;

use super::layout::{SidebarSelection, Sort, SortDirection, SortKey};
use super::machine::LibraryMachine;
use super::matching::{self, MatchStatus};
use super::store::StoredEntry;

/// One installed game as the view shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub playtime: crate::playtime::PlaytimeView,
    pub running: bool,
    pub key: String,
    /// The matched game's title when there is one, else the detected title.
    pub title: String,
    /// The title the provider detected (the store id when it gave none).
    pub detected_name: String,
    /// The matched GameNode game's title, when it is known.
    pub matched_name: Option<String>,
    pub provider: String,
    pub provider_name: String,
    pub status: MatchStatus,
    /// File name of the cached cover under the per-user `covers/` folder.
    pub cover_file: Option<String>,
}

/// An installed-game count for a sidebar item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderCount {
    pub provider: String,
    pub name: String,
    pub count: usize,
}

impl LibraryMachine {
    /// How an entry maps to a GameNode game right now.
    pub fn match_status(&self, entry: &StoredEntry) -> MatchStatus {
        if !self.chosen.contains_key(&entry.key)
            && let Some(game_id) = self.local_inferences.get(&entry.key)
        {
            return MatchStatus::Matched {
                game_id: *game_id,
                source: super::matching::MatchSource::Automatic,
            };
        }
        let auto = self
            .provider(&entry.provider)
            .and_then(|provider| provider.external_category())
            .and_then(|category| self.auto_matches.get(&(category, entry.store_id.clone())));
        matching::resolve(self.chosen.get(&entry.key), auto)
    }

    fn row(&self, entry: &StoredEntry) -> Row {
        let status = self.match_status(entry);
        let game = status.game_id().and_then(|id| self.games.get(&id));
        let detected_name = entry.name.clone().unwrap_or_else(|| entry.store_id.clone());
        let matched_name = game
            .map(|game| game.name.clone())
            .filter(|name| !name.is_empty());
        Row {
            playtime: self.playtime.get(&entry.key).copied().unwrap_or_default(),
            running: self.running.contains(&entry.key),
            key: entry.key.clone(),
            title: matched_name
                .clone()
                .unwrap_or_else(|| detected_name.clone()),
            detected_name,
            matched_name,
            provider: entry.provider.clone(),
            provider_name: self
                .provider(&entry.provider)
                .map_or_else(|| entry.provider.clone(), |p| p.name().to_owned()),
            status,
            cover_file: game.and_then(|game| game.cover_file.clone()),
        }
    }

    /// Every installed entry as a row, unsorted.
    pub fn all_rows(&self) -> Vec<Row> {
        self.installed_entries()
            .map(|entry| self.row(entry))
            .collect()
    }

    /// The sidebar choice in effect: a saved provider that is no longer loaded counts as "All".
    pub fn effective_sidebar(&self) -> SidebarSelection {
        match &self.layout.sidebar {
            SidebarSelection::Provider(id) if self.provider(id).is_none() => SidebarSelection::All,
            other => other.clone(),
        }
    }

    /// The rows for the current sidebar choice, the given search text and the saved sort.
    pub fn visible_rows(&self, search: &str) -> Vec<Row> {
        let mut rows = filter(&self.all_rows(), &self.effective_sidebar(), search);
        sort(&mut rows, self.layout.sort);
        rows
    }

    /// Installed games per loaded provider, in provider order. Not affected by search.
    pub fn provider_counts(&self) -> Vec<ProviderCount> {
        counts(
            &self.all_rows(),
            self.providers
                .iter()
                .map(|provider| (provider.id(), provider.name())),
        )
    }
}

/// Whether `row` contains `needle` (already trimmed and lowercased) in its detected or matched
/// title.
fn contains(row: &Row, needle: &str) -> bool {
    row.detected_name.to_lowercase().contains(needle)
        || row
            .matched_name
            .as_ref()
            .is_some_and(|name| name.to_lowercase().contains(needle))
}

/// Keeps the rows in the sidebar choice whose detected or matched title contains `search`,
/// ignoring case. Blank search keeps them all. Order is preserved.
pub fn filter(rows: &[Row], sidebar: &SidebarSelection, search: &str) -> Vec<Row> {
    let needle = search.trim().to_lowercase();
    rows.iter()
        .filter(|row| match sidebar {
            SidebarSelection::All => true,
            SidebarSelection::Provider(id) => &row.provider == id,
        })
        .filter(|row| needle.is_empty() || contains(row, &needle))
        .cloned()
        .collect()
}

fn folded(text: &str) -> String {
    text.to_lowercase()
}

fn by_title(a: &Row, b: &Row) -> Ordering {
    folded(&a.title)
        .cmp(&folded(&b.title))
        .then_with(|| a.title.cmp(&b.title))
}

fn compare(a: &Row, b: &Row, key: SortKey) -> Ordering {
    match key {
        SortKey::Title => by_title(a, b),
        SortKey::Provider => folded(&a.provider_name).cmp(&folded(&b.provider_name)),
        SortKey::MatchStatus => a.status.rank().cmp(&b.status.rank()),
    }
}

/// Ties are broken the same way in both directions: only the leading key flips.
fn tie_break(a: &Row, b: &Row, key: SortKey) -> Ordering {
    let order = match key {
        SortKey::Title => Ordering::Equal,
        SortKey::Provider | SortKey::MatchStatus => by_title(a, b),
    };
    order
        .then_with(|| a.provider.cmp(&b.provider))
        .then_with(|| a.key.cmp(&b.key))
}

/// Sorts in place. The sort is total (title, then provider, then key break ties), so the order
/// never depends on the order the rows arrived in.
pub fn sort(rows: &mut [Row], by: Sort) {
    rows.sort_by(|a, b| {
        let lead = compare(a, b, by.key);
        let lead = match by.direction {
            SortDirection::Ascending => lead,
            SortDirection::Descending => lead.reverse(),
        };
        lead.then_with(|| tie_break(a, b, by.key))
    });
}

/// Counts rows per provider, in the order `providers` gives them (id, name). Providers with no
/// games still appear with a count of zero.
pub fn counts<'a>(
    rows: &[Row],
    providers: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<ProviderCount> {
    providers
        .into_iter()
        .map(|(id, name)| ProviderCount {
            provider: id.to_owned(),
            name: name.to_owned(),
            count: rows.iter().filter(|row| row.provider == id).count(),
        })
        .collect()
}

#[cfg(test)]
mod tests;
