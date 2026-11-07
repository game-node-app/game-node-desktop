# Proposal

## Why

The library can only filter by provider and "installed only", and sort by title, provider or match status. Users with large Steam, family and DRM-free libraries cannot answer everyday questions such as "what can I play right now?", "what did I play recently?", "what did I add this month?" or "what still needs a match?". Steam, Playnite, GOG Galaxy and Lutris all treat filtering and sorting as a first-class browsing system, and all of them expose favorites as a top-level signal. GameNode already stores a favorite flag per library entry, so Desktop can mirror it instead of inventing a separate concept.

## What Changes

- Replace the single "Installed" toggle with a filter panel plus active-filter chips. The panel covers, over data the library already has:
  - **Availability**: Installed, Owned, Family shared (multi-select; any selected fact matches).
  - **State**: Running only.
  - **Match state**: Any, Matched, Needs review (ambiguous), Not matched, Marked as no game.
  - **Play history**: Unplayed, Played, 10 h+, 100 h+.
  - **Last played**: last 7 days, 30 days, 1 year.
  - **Release decade**: 2020s down to 1980s, earlier, unknown.
  - **Date added**: last 7, 30, 90 days or 1 year.
  - **Favorites only**.
- Show the filtered/total count, active-filter chips with individual removal, a "Clear all" action, and extend the "nothing matches" empty state to clear filters as well as search.
- Keep the sidebar as provider navigation, but make its counts reflect every active filter except the provider selection, so a count never promises rows the view hides.
- Extend sorting with Release year, Playtime, Last played and Date added, alongside Title, Store and Match status. Direction defaults per key (for example newest first for dates, most first for playtime), the direction control names the current order, and unknown values sort last in both directions. The total order stays deterministic.
- Add Favorites:
  - A star action in the detail pane and the single-game context menu, a star indicator in grid and list, and a Favorites filter in the toolbar.
  - Local-first, per GameNode account, so it works signed out, offline and with sync off.
  - Two-way sync with GameNode's `isFavorite` when a verified session exists and "Sync with GameNode" is on: read the account's favorites, push local changes, and retry pending changes. A game that is not yet in the GameNode library stays favorited locally and syncs after it is added.
  - Only matched entries can be favorited; unmatched entries show the action disabled with a reason, because the server flag belongs to a GameNode game.
- Remember the new filters and sort across restarts; search text stays unremembered.
- Keep everything keyboard accessible and translated in English and Brazilian Portuguese, and extend the mock host so the UI can be exercised without the native host.

Out of scope: genre/tag/platform filters (GameNode metadata is not stored locally), grouping, saved smart collections, server-side collection management, and syncing favorites for unmatched entries.

## Capabilities

### New Capabilities

- `desktop-favorites`: the favorite flag as local per-account user data, its library actions and filter, and its two-way sync with GameNode including pending, offline and not-yet-in-library behavior.

### Modified Capabilities

- `desktop-library`: the filter, search and sort requirement becomes a multi-facet filter system with more sort keys; remembered layout now includes filters; empty-state and provider-count behavior follow the active filters.
- `desktop-playtime`: the row playtime view gains last-played time, derived from the selected source, so it can be filtered and sorted without fabricating a value.
- `desktop-local-state`: favorites are persisted per account by a new migration and survive sign-out and restart.
- `desktop-sync`: "Sync with GameNode" and its first-use explanation cover favorites as well as playtime, and favorites are not sent while sync is off.

## Impact

- **Core (`crates/core`)**: `LibraryLayout`/`LibraryFilters`, `SortKey`, `Row`, `PlaytimeView`, `LibraryMachine` inputs/effects, a new favorites module, and the `GameApi` port (`list_favorites`, `set_favorite`). Old layouts that stored `installedOnly` must still load.
- **Platform (`crates/platform`)**: schema migration 7 with a per-account favorites table, cache loading, and store implementation.
- **API (`crates/api`)**: paginated `GET /v1/collections-entries/library/:userId/favorites` and `POST /v1/collections-entries/game/:id/favorite` (`{ "isFavorite": bool }`), with 404 reported as "not in library" rather than a transient failure.
- **App (`crates/app`)**: effect handlers for favorites sync/push, the sync gate, retry triggers, and injecting the current time for relative filters.
- **UI (`ui`)**: filter popover and chips, sort select/direction control, star actions and indicators, updated types, mock host, i18n (en, pt-BR), and tests.
- **Contracts**: the Rust↔React `LibraryLayout` shape changes from `installedOnly` to `filters`; saved layouts are migrated, so no user data is lost. The GameNode API is used as already shipped by `game-node-server`; no server change is required.
