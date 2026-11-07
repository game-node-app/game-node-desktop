# Design

## Context

See `proposal.md` for motivation and the delta specs for the behavior contract. The current implementation is a pure `LibraryMachine` (`crates/core/src/library`) with pure filter/sort/count functions in `query.rs`; the React UI reads a serialized `LibraryView` and sends `LibraryInput` commands. The layout is stored as JSON in the settings table, so changing its shape needs backward compatibility. `Row` already carries playtime, running, provider, installed/owned/family facts and match status; `GameRecord` has `release_year`, `StoredEntry` has `first_seen`, and `Total`/`Session` already carry the dates needed for last played. `GameApi` is the port to `game-node-server`, which already exposes the favorite flag and the two endpoints this change needs. Core never reads the clock: inputs carry `now`, which matters for relative date filters.

## Goals / Non-Goals

**Goals:**

- One composable, serializable filter model owned by core, used identically by grid, list, keyboard navigation, counts and empty states.
- More sort keys with deterministic ordering and honest handling of unknown values.
- Favorites that are useful offline and with sync off, and that converge with GameNode without losing a local user action.
- Preserve existing layouts and user data across the schema change.

**Non-Goals:**

- Genre, tag, platform, controller or Deck-compatibility filters; that metadata is not stored locally.
- Grouping, saved smart collections, bulk favorite, or server-side collection editing.
- Changing `game-node-server`; the endpoints are already shipped.

## Decisions

### 1. Structured `LibraryFilters` replaces `installed_only`

Add to `layout.rs`:

- `AvailabilityFilter { installed, owned, family }`, all default `false` meaning "any"; when any is `true`, an entry matches if any selected fact is true (OR).
- `MatchFilter`: `Any`, `Matched`, `Ambiguous`, `Unmatched`, `NoGame` (one-to-one with `MatchStatus`).
- `PlaytimeFilter`: `Any`, `Unplayed`, `Played`, `AtLeast10h`, `AtLeast100h`.
- `LastPlayedFilter`: `Any`, `Last7Days`, `Last30Days`, `LastYear`.
- `ReleaseFilter`: `Any`, `Decade(u16)`, `Earlier`, `Unknown`.
- `AddedFilter`: `Any`, `Last7Days`, `Last30Days`, `Last90Days`, `LastYear`.
- `LibraryFilters` groups them plus `running_only` and `favorites_only`, with `Default` = every facet at "any".

All types use `camelCase` serde names, like the existing layout. `LibraryLayout` gets `filters: LibraryFilters` and keeps `#[serde(rename = "installedOnly", default, skip_serializing)] legacy_installed_only: Option<bool>`; `effective_filters()` maps `Some(true)` to `AvailabilityFilter { installed: true, .. }` only when `filters` is still at its default. New saves serialize `filters` and omit the legacy key. This is less code than a custom `Deserialize` and keeps the migration observable and testable.

**Alternatives considered:** a generic `Vec<Filter>` key-value model (no type safety, every consumer needs string parsing); filtering in the UI (duplicated rules, counts and keyboard movement would drift); adding a `SidebarSelection::Favorites` variant (conflates navigation with a facet and complicates provider counts).

### 2. One filter pipeline, counts computed with the provider facet removed

`query::filter(rows, sidebar, &filters, search, now)` applies, in order: provider → availability → running → match → playtime → last played → release decade → date added → favorites → search. Unknown values (`None` playtime, last played or release year) pass only `Any`; they never satisfy a threshold or window. Search stays title-only and case-insensitive, as today.

`visible_rows(search, now)` filters then sorts. Provider counts are computed over rows filtered by every active facet **except** the sidebar provider selection, then grouped by provider. The existing raw `installedCount`/`ownedCount`/`familyCount` stay for the sidebar subline. This is the only way a provider count can match what the view would show without recomputing per provider on every render.

`now` is injected by adding `now: i64` to `LibraryMachine`, refreshed by the engine immediately before each `library_input` (the engine already calls `unix_now()`), and by tests before building views. `view()` keeps its signature. **Alternative considered:** `view(now)`; it is more precise but touches 60+ call sites for a window whose unit is days, while every scan/selection/view rebuild refreshes the stored value. Drift is bounded by the next input and is irrelevant at day granularity.

### 3. Sort keys, defaults and unknown values

Extend `SortKey` with `ReleaseYear`, `Playtime`, `LastPlayed`, `DateAdded`. Add `default_direction()`:

| Key | Default |
| --- | --- |
| Title, Provider | Ascending |
| Match status | Ascending (best first) |
| Release year, Playtime, Last played, Date added | Descending (newest/most first) |

The UI sends `{ key, direction: key.defaultDirection() }` when the key changes, so choosing Playtime immediately shows the most played. The direction control is an icon button whose accessible label names the resulting order for the key (for example "Newest first", "Most played first"), not a generic A–Z label.

`sort` precomputes a `SortKeys` per row (lowercased title/provider, playtime seconds, last played, release year, added at) so comparisons do not allocate. `Option` values compare `Some` before `None` regardless of direction; direction flips only the `Some` comparison. The existing tie-break (title, provider, key) is never flipped, so the order stays total and deterministic.

### 4. Row enrichment

Add to `Row`: `release_year: Option<i32>`, `added_at: i64`, `is_favorite: bool`. Add `last_played_at: Option<i64>` to `PlaytimeView`.

- `release_year` comes from the matched `GameRecord`; unmatched entries have `None`.
- `added_at` is `StoredEntry.first_seen` (the scan that first detected the entry; preserved on rescan).
- `is_favorite` is derived from the favorites map keyed by the matched game id, so entries sharing a game agree.
- `view_for_entry` computes `last_played_at`: Store uses the matching `Total.last_played_at`; Internal uses `max(session.ended_at.unwrap_or(now))` over the entry's sessions, which naturally follows an open session. No data means `None`, never `0`.

**Why put last played in `PlaytimeView`?** It shares the account, source and session logic with the seconds value; splitting it would create two sources of truth and two places to clear on account change.

### 5. Favorites: local-first, keyed by GameNode game, one sync loop

**Data.** New `FavoriteRecord { account_id, game_id, is_favorite, pending, updated_at }`. Migration 7 creates:

```sql
CREATE TABLE favorites (
  account_id TEXT NOT NULL,
  game_id    INTEGER NOT NULL,
  is_favorite INTEGER NOT NULL,
  pending    INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (account_id, game_id)
) STRICT;
```

`LibraryCache` gains `favorites: Vec<FavoriteRecord>` loaded for the account, like `chosen_matches`. `LibraryStore` gains `save_favorite(&FavoriteRecord)`; a false, non-pending row is deleted rather than kept. `LibraryMachine` holds `favorites: BTreeMap<u64, FavoriteState>` where `FavoriteState { is_favorite, pending }`.

**Why per game, not per entry?** GameNode stores `isFavorite` on the collection entry, which is per game and per user. Two local entries matched to the same game must show the same star, and one server call updates both. Unmatched entries cannot be favorited because there is no game id; the UI disables the action with "Match the game first".

**Inputs and effects.**

- `SetFavorite { key, favorite, now }`: resolves the entry's matched game; no-op if unmatched. Applies the state immediately, marks it pending, persists it, and emits `PushFavorite` only when sync is active.
- `FavoritesSynced { op, result, now }`: merges a complete server set.
- `FavoritePushed { op, game_id, favorite, result, now }`: confirms or keeps pending.
- `SetSyncActive(bool)`: the engine pushes the current gate (`enabled && explained && verified`) whenever it changes; `true` triggers a sync and pending pushes.
- `SyncFavoritesRequested`: explicit retry (sync turned on, window opened, add-to-library success).
- Effects: `PersistFavorite`, `SyncFavorites { op, user_id }`, `PushFavorite { op, game_id, favorite }`.

One sync may be in flight and one push per game; op ids discard stale answers. On a push result, pending clears only if the local state still equals the pushed value, so a fast toggle-back is not lost.

**Merge rule.** Pending local changes win. A completed read sets true for server favorites with no pending local change, sets false for local favorites absent from the server with no pending local change, leaves pending changes alone, then emits pushes for pending changes. `Err` (including an incomplete pagination) changes nothing. This makes the server authoritative for untouched games and the user authoritative for their own unsent action.

**API.** `GameApi::list_favorites(credential, user_id)` pages `GET /v1/collections-entries/library/{user_id}/favorites?offset=…&limit=100` and reads `data[].gameId` until `pagination.hasNextPage` is false; any failed page returns `Err`, so a partial list is never applied. `GameApi::set_favorite(credential, game_id, favorite)` posts `{ "isFavorite": bool }` to `POST /v1/collections-entries/game/{game_id}/favorite`; 204 is success, 404 is `FavoriteError::NotInLibrary`, 401 is `Rejected`, everything else is `Failed`. A separate `FavoriteError` keeps the 404 meaningful without adding a variant to the widely matched `ApiError`.

**Retry triggers.** Verified access, scan completion (the existing 30-minute cadence), sync becoming active, and a successful Add to GameNode. `NotInLibrary` keeps the change pending without a tight retry; the next sync or the add flow retries it. `Failed` keeps it pending for the next trigger. `Rejected` goes through the existing `check_rejected` path and stays pending.

**Why local-first?** Offline, signed-out and sync-off use are first-class in this app; a star that waits for the network or disappears on failure is worse than a star that converges later. **Alternative considered:** require membership before allowing a favorite (the server returns 404 otherwise); rejected because it blocks a harmless local action and forces a round trip on every toggle.

### 6. UI shape

`LibraryToolbar` keeps search and gains:

- A **Filters** button with a count badge, opening a Mantine `Popover` (already a dependency) with sections for Availability (`Checkbox`), Running (`Switch`), Match, Playtime, Last played, Release and Date added (`Select` or `SegmentedControl`). The popover handles Escape, outside click and focus return; controls are ordinary focusable elements.
- A **Favorites** star toggle (`aria-pressed`) beside Filters.
- A **sort select** with the seven keys and an icon-only direction button whose `aria-label`/tooltip names the order for the current key.

Below the toolbar, when anything is active, a chip row shows one removable chip per facet, a "Showing X of Y" count and "Clear all filters". Search has its own clear action; "Clear all filters" does not clear search. The empty state offers both actions when both are active.

Grid, list and detail show a star indicator/button; the context menu adds Add/Remove favorite for a single matched entry and omits it for multi-selection or unmatched entries. The existing installed/family badges are hidden when their availability facet is the reason they are shown (for example, don't show the installed badge when only Installed is selected), matching today's `showInstalled` behavior.

`ui/src/api/types.ts` mirrors the new layout, row and playtime fields; `LibraryInput` gains `setFilters` and `setFavorite`. The mock host implements the same filter/sort rules in memory and a fake favorite set with a pending flag, so Vitest and `VITE_UI_MOCK=1` exercise the real UI states. All new strings go to `en.ts` and `pt-BR.ts`. Styling uses Tailwind utilities and existing tokens (star in `text-warning`), no static inline colors.

**Alternatives considered:** a sidebar filter panel (more room, but splits navigation and filtering and pushes the provider list down); a modal filter dialog (heavy for a frequently changed control); Mantine `Menu` with checkbox items (poor fit for seven sections and selects).

### 7. Layout and schema compatibility

- New saves write `filters`; old `installedOnly` is accepted on load and mapped to Installed availability. A downgrade to an older build reads `installedOnly` as absent/false and ignores `filters`; filters are lost but no data is corrupted.
- Migration 7 is additive; older builds ignore the table.
- View/row fields are runtime-only (never persisted), so adding `releaseYear`, `addedAt`, `isFavorite` and `lastPlayedAt` needs no view migration; the UI types and mock fixtures are updated in the same change.

### 8. Performance

Filtering is O(n) per view build with no per-row allocation beyond the existing search lowercase; favorites lookup is O(log n) per row. Sorting is O(n log n) with precomputed keys, preserving the existing `SortKeys` approach. The view cache and `Arc<Row>` reuse continue to work because `Row` derives `PartialEq`. Relative windows use the stored `now`; no timers or extra wakeups are introduced.

## Risks / Trade-offs

- **A 404 favorite stays pending forever if the game is never added.** → It is harmless and retried only on existing sync triggers; the UI keeps the local star and does not claim it is synced.
- **A server-side removal could race a local toggle.** → Pending local changes always win, and only a complete read applies; a stale read cannot erase an unsent action.
- **Large favorite lists need several pages.** → Fetch all pages before applying; any failure leaves local state untouched.
- **Old layouts or test fixtures still use `installedOnly`.** → Explicit legacy field plus tests for load/save; update UI fixtures and the mock.
- **`now` drift for relative windows.** → Refreshed on every library input; windows are measured in days, so the drift is not observable.
- **Unmatched entries cannot be favorited.** → Disabled with a clear reason; avoids duplicate local state that would need merging on match.
- **Filter panel could become cramped.** → Single scrollable popover with native controls, chips for what is active, and tests at narrow widths.
- **Sorting by an unknown key could put unknowns first.** → `Option` comparator always puts `None` last in both directions, with tests for each new key.

## Migration Plan

1. Add migration 7 and the store/cache plumbing; existing databases upgrade in place.
2. Add `filters` with legacy `installedOnly` acceptance; verify old layouts and round-trips before touching the UI.
3. Ship core/UI together (the layout contract changes), keeping the old `installedOnly` read path for saved data only.
4. Rollback is safe: an older build ignores the favorites table and reads a new layout as no filters; no destructive migration is involved.
