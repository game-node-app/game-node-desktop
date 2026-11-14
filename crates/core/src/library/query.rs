//! The rows the library shows, and how they are filtered, searched, sorted and counted.
//!
//! Everything here is a pure function over [`Row`]s, so the rules are tested without a machine.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use super::layout::{
    AddedFilter, AvailabilityFilter, DAY_SECONDS, FIRST_RELEASE_DECADE, LastPlayedFilter,
    LibraryFilters, MatchFilter, PlaytimeFilter, ReleaseFilter, SidebarSelection, Sort,
    SortDirection, SortKey, YEAR_SECONDS,
};
use super::machine::LibraryMachine;
use super::matching::{self, MatchStatus};
use super::store::StoredEntry;

/// One visible game as the view shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    /// Found by the installed scan on this PC.
    pub installed: bool,
    /// Found in the connected account's owned catalog.
    pub owned: bool,
    /// Shared through the provider's declared shared access; never true when the same entry is
    /// owned.
    pub shared: bool,
    pub status: MatchStatus,
    /// File name of the cached cover under the per-user `covers/` folder.
    pub cover_file: Option<String>,
    /// The matched game's release year; unknown for unmatched entries and games without one.
    pub release_year: Option<i32>,
    /// Unix seconds of the scan that first detected the entry.
    pub added_at: i64,
    /// Whether the matched game is a favorite of the signed-in account.
    pub is_favorite: bool,
}

/// How many visible games a provider has, and how many of them by fact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCount {
    /// The provider id, such as `steam`; the sidebar sends it back as its filter.
    pub id: String,
    pub name: String,
    /// Entries the active filters (except the sidebar's provider selection) leave.
    pub count: usize,
    /// Entries installed on this PC: a raw fact for the sidebar subline and the store card.
    pub installed_count: usize,
    /// Entries the connected account owns: a raw fact for the store card.
    pub owned_count: usize,
    /// Entries shared through the provider's declared shared access and not owned: a raw fact
    /// for the store card.
    pub shared_count: usize,
    /// The provider's declared shared-access label, if it declares any. Only labelled providers
    /// offer the Shared filter, counts and warnings.
    pub shared_label: Option<String>,
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
            .lookup_key(entry)
            .and_then(|(category, uid)| self.auto_matches.get(&(category, uid)));
        matching::resolve(self.chosen.get(&entry.key), auto)
    }

    fn row(&self, entry: &StoredEntry) -> Row {
        let status = self.match_status(entry);
        let game = status.game_id().and_then(|id| self.games.get(&id));
        let detected_name = entry.name.clone().unwrap_or_else(|| entry.store_id.clone());
        let matched_name = game
            .map(|game| game.name.clone())
            .filter(|name| !name.is_empty());
        let is_favorite = status
            .game_id()
            .and_then(|id| self.favorites.get(&id))
            .is_some_and(|favorite| favorite.is_favorite);
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
            installed: entry.installed,
            owned: entry.owned,
            shared: entry.shared && !entry.owned,
            status,
            cover_file: game.and_then(|game| game.cover_file.clone()),
            release_year: game.and_then(|game| game.release_year),
            added_at: entry.first_seen,
            is_favorite,
        }
    }

    /// Every visible entry as a row, unsorted.
    pub fn all_rows(&self) -> Vec<Row> {
        self.visible_entries()
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

    /// The title of the first installed game that is running, without building every row.
    pub fn running_title(&self) -> Option<String> {
        self.installed_entries()
            .find(|entry| self.running.contains(&entry.key))
            .map(|entry| self.row(entry).title)
    }

    /// The rows for the current sidebar choice, filters, search text and saved sort.
    pub fn visible_rows(&self, search: &str) -> Vec<Row> {
        let mut rows = filter(
            &self.all_rows(),
            &self.effective_sidebar(),
            &self.layout.filters,
            search,
            self.now,
        );
        sort(&mut rows, self.layout.sort);
        rows
    }

    /// Visible games per loaded provider, in provider order. A provider's count reflects every
    /// active filter except its own selection; the raw installed/owned/shared facts stay
    /// unfiltered for the sidebar subline and the store card.
    pub fn provider_counts(&self) -> Vec<ProviderCount> {
        let rows = filter(
            &self.all_rows(),
            &SidebarSelection::All,
            &self.layout.filters,
            "",
            self.now,
        );
        let mut counts = counts(
            &rows,
            self.providers
                .iter()
                .map(|provider| (provider.id(), provider.name())),
        );
        // Counts need only the provider id here; the facts are independent of the filters.
        let mut facts = std::collections::HashMap::<&str, (usize, usize, usize)>::new();
        for entry in self.visible_entries() {
            let counts = facts.entry(entry.provider.as_str()).or_default();
            if entry.installed {
                counts.0 += 1;
            }
            if entry.owned {
                counts.1 += 1;
            } else if entry.shared {
                counts.2 += 1;
            }
        }
        for count in &mut counts {
            let (installed, owned, shared) =
                facts.get(count.id.as_str()).copied().unwrap_or_default();
            count.installed_count = installed;
            count.owned_count = owned;
            count.shared_count = shared;
            count.shared_label = self
                .provider(&count.id)
                .and_then(|provider| provider.shared_label())
                .map(str::to_owned);
        }
        counts
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

fn matches_availability(filter: AvailabilityFilter, row: &Row) -> bool {
    filter.is_any()
        || (filter.installed && row.installed)
        || (filter.owned && row.owned)
        || (filter.shared && row.shared)
}

fn matches_match_state(filter: MatchFilter, status: &MatchStatus) -> bool {
    match filter {
        MatchFilter::Any => true,
        MatchFilter::Matched => matches!(status, MatchStatus::Matched { .. }),
        MatchFilter::Ambiguous => matches!(status, MatchStatus::Ambiguous { .. }),
        MatchFilter::Unmatched => matches!(status, MatchStatus::Unmatched),
        MatchFilter::NoGame => matches!(status, MatchStatus::NoGame),
    }
}

/// Unknown playtime never satisfies a threshold; only `Any` lets it through.
fn matches_playtime(filter: PlaytimeFilter, seconds: Option<i64>) -> bool {
    match filter {
        PlaytimeFilter::Any => true,
        PlaytimeFilter::Unplayed => seconds == Some(0),
        PlaytimeFilter::Played => seconds.is_some_and(|seconds| seconds > 0),
        PlaytimeFilter::AtLeast10h => seconds.is_some_and(|seconds| seconds >= 10 * 3600),
        PlaytimeFilter::AtLeast100h => seconds.is_some_and(|seconds| seconds >= 100 * 3600),
    }
}

/// Whether a Unix timestamp lies inside the last `window` seconds.
fn within(at: Option<i64>, now: i64, window: i64) -> bool {
    at.is_some_and(|at| at >= now.saturating_sub(window))
}

fn matches_last_played(filter: LastPlayedFilter, at: Option<i64>, now: i64) -> bool {
    match filter {
        LastPlayedFilter::Any => true,
        LastPlayedFilter::Last7Days => within(at, now, 7 * DAY_SECONDS),
        LastPlayedFilter::Last30Days => within(at, now, 30 * DAY_SECONDS),
        LastPlayedFilter::LastYear => within(at, now, YEAR_SECONDS),
    }
}

fn matches_release(filter: ReleaseFilter, year: Option<i32>) -> bool {
    match filter {
        ReleaseFilter::Any => true,
        ReleaseFilter::Decade(decade) => {
            let start = i32::from(decade);
            year.is_some_and(|year| (start..start + 10).contains(&year))
        }
        ReleaseFilter::Earlier => year.is_some_and(|year| year < i32::from(FIRST_RELEASE_DECADE)),
        ReleaseFilter::Unknown => year.is_none(),
    }
}

fn matches_added(filter: AddedFilter, at: i64, now: i64) -> bool {
    match filter {
        AddedFilter::Any => true,
        AddedFilter::Last7Days => within(Some(at), now, 7 * DAY_SECONDS),
        AddedFilter::Last30Days => within(Some(at), now, 30 * DAY_SECONDS),
        AddedFilter::Last90Days => within(Some(at), now, 90 * DAY_SECONDS),
        AddedFilter::LastYear => within(Some(at), now, YEAR_SECONDS),
    }
}

/// Keeps the rows that the sidebar choice, every active facet and `search` allow, in order.
///
/// Facets combine with AND; the availability facts are alternatives within their facet. Unknown
/// playtime, last played and release never satisfy a threshold or window. Blank search keeps
/// everything; search is title-only and case-insensitive.
pub fn filter(
    rows: &[Row],
    sidebar: &SidebarSelection,
    filters: &LibraryFilters,
    search: &str,
    now: i64,
) -> Vec<Row> {
    let needle = search.trim().to_lowercase();
    rows.iter()
        .filter(|row| match sidebar {
            SidebarSelection::All => true,
            SidebarSelection::Provider(id) => &row.provider == id,
        })
        .filter(|row| matches_availability(filters.availability, row))
        .filter(|row| !filters.running_only || row.running)
        .filter(|row| matches_match_state(filters.match_state, &row.status))
        .filter(|row| matches_playtime(filters.playtime, row.playtime.seconds))
        .filter(|row| matches_last_played(filters.last_played, row.playtime.last_played_at, now))
        .filter(|row| matches_release(filters.release, row.release_year))
        .filter(|row| matches_added(filters.added, row.added_at, now))
        .filter(|row| !filters.favorites_only || row.is_favorite)
        .filter(|row| needle.is_empty() || contains(row, &needle))
        .cloned()
        .collect()
}

fn folded(text: &str) -> String {
    text.to_lowercase()
}

/// The values each row is compared by, computed once per row instead of once per comparison.
struct SortKeys {
    title: String,
    /// Only filled when sorting by provider.
    provider: String,
}

fn by_title(a: &Row, a_keys: &SortKeys, b: &Row, b_keys: &SortKeys) -> Ordering {
    a_keys
        .title
        .cmp(&b_keys.title)
        .then_with(|| a.title.cmp(&b.title))
}

/// Compares two optional values so that a known value always comes before an unknown one,
/// whichever direction the user chose. The direction only orders two known values.
fn compare_option<T: Ord>(a: Option<T>, b: Option<T>, direction: SortDirection) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => match direction {
            SortDirection::Ascending => a.cmp(&b),
            SortDirection::Descending => b.cmp(&a),
        },
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn directed(order: Ordering, direction: SortDirection) -> Ordering {
    match direction {
        SortDirection::Ascending => order,
        SortDirection::Descending => order.reverse(),
    }
}

fn compare(a: (&Row, &SortKeys), b: (&Row, &SortKeys), sort: Sort) -> Ordering {
    let direction = sort.direction;
    match sort.key {
        SortKey::Title => directed(by_title(a.0, a.1, b.0, b.1), direction),
        SortKey::Provider => directed(a.1.provider.cmp(&b.1.provider), direction),
        SortKey::MatchStatus => directed(a.0.status.rank().cmp(&b.0.status.rank()), direction),
        // Unknown-aware keys apply the direction inside `compare_option`, so unknown values stay
        // last whichever direction the user chose.
        SortKey::ReleaseYear => compare_option(a.0.release_year, b.0.release_year, direction),
        SortKey::Playtime => compare_option(a.0.playtime.seconds, b.0.playtime.seconds, direction),
        SortKey::LastPlayed => compare_option(
            a.0.playtime.last_played_at,
            b.0.playtime.last_played_at,
            direction,
        ),
        SortKey::DateAdded => compare_option(Some(a.0.added_at), Some(b.0.added_at), direction),
    }
}

/// Ties are broken the same way in both directions: only the leading key flips.
fn tie_break(a: (&Row, &SortKeys), b: (&Row, &SortKeys), key: SortKey) -> Ordering {
    let order = match key {
        SortKey::Title => Ordering::Equal,
        _ => by_title(a.0, a.1, b.0, b.1),
    };
    order
        .then_with(|| a.0.provider.cmp(&b.0.provider))
        .then_with(|| a.0.key.cmp(&b.0.key))
}

/// Sorts in place. The sort is total (title, then provider, then key break ties), so the order
/// never depends on the order the rows arrived in.
pub fn sort(rows: &mut [Row], by: Sort) {
    let keys: Vec<SortKeys> = rows
        .iter()
        .map(|row| SortKeys {
            title: folded(&row.title),
            provider: if by.key == SortKey::Provider {
                folded(&row.provider_name)
            } else {
                String::new()
            },
        })
        .collect();
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by(|&a, &b| {
        let (a, b) = ((&rows[a], &keys[a]), (&rows[b], &keys[b]));
        compare(a, b, by).then_with(|| tie_break(a, b, by.key))
    });
    // `target[i]` is where the row now at `i` must end up; follow each cycle with swaps.
    let mut target = vec![0; rows.len()];
    for (position, &from) in order.iter().enumerate() {
        target[from] = position;
    }
    for i in 0..rows.len() {
        while target[i] != i {
            let t = target[i];
            rows.swap(i, t);
            target.swap(i, t);
        }
    }
}

/// Counts rows per provider, in the order `providers` gives them (id, name). Providers with no
/// games still appear with a count of zero; the fact counts start at zero and are filled by the
/// caller when it wants them.
pub fn counts<'a>(
    rows: &[Row],
    providers: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<ProviderCount> {
    providers
        .into_iter()
        .map(|(id, name)| ProviderCount {
            id: id.to_owned(),
            name: name.to_owned(),
            count: rows.iter().filter(|row| row.provider == id).count(),
            installed_count: 0,
            owned_count: 0,
            shared_count: 0,
            shared_label: None,
        })
        .collect()
}

#[cfg(test)]
mod tests;
