# Tasks

## 1. Core filter and sort model

- [x] 1.1 Add `AvailabilityFilter`, `MatchFilter`, `PlaytimeFilter`, `LastPlayedFilter`, `ReleaseFilter`, `AddedFilter` and `LibraryFilters` to `layout.rs`, replace `installed_only` with `filters`, and keep a skipped `installedOnly` legacy field. Verify with core serde tests: an old `{"installedOnly":true}` layout loads with Installed selected, and a new save contains `filters` without `installedOnly`.
- [x] 1.2 Extend `Row` with `release_year`, `added_at` and `is_favorite`, and `PlaytimeView` with `last_played_at`; update `row()` and `playtime::view_for_entry` for Store and Internal sources. Verify with core tests for matched/unmatched years, `first_seen`, and last played from totals, sessions, open sessions and no data.
- [x] 1.3 Implement `query::filter` over every facet plus search, with unknown playtime/last played/release never satisfying a threshold or window, and add the stored `now` to `LibraryMachine` for relative windows. Verify with query tests for each facet, combined facets, unknown values and search.
- [x] 1.4 Compute provider counts over rows filtered by every active facet except the provider selection, and update `visible_rows`, `build_view` and the no-installed/nothing-matches empty states. Verify with view tests for counts, no installed games and clearable search/filters.
- [x] 1.5 Extend `SortKey` with `ReleaseYear`, `Playtime`, `LastPlayed` and `DateAdded`, add per-key default directions, key-aware direction labels, and `None`-last comparisons in both directions. Verify with query tests for each key in both directions, unknown values and the unchanged tie-break.

## 2. Favorites persistence and core state

- [x] 2.1 Add migration 7 with the per-account `favorites` table, extend `LibraryCache` and `LibraryStore` with favorite records, and load/save them in `library_db`. Verify with platform tests for migration, round-trip, pending state and account isolation across sign-out.
- [x] 2.2 Add the favorites state machine: `SetFavorite`, `FavoritesSynced`, `FavoritePushed`, `SetSyncActive` and `SyncFavoritesRequested`, with local-first toggles, pending-local-wins merge, one sync/one push per game, stale-op rejection and retry triggers. Verify with core tests for merge, conflict, failure, 404-not-in-library and fast toggle-back.
- [x] 2.3 Add `GameApi::list_favorites` and `set_favorite` with complete pagination, 204/404/401 handling and a dedicated `FavoriteError`. Verify with `gamenode-api` tests against the mock HTTP server for multi-page reads, partial failure and each status.
- [x] 2.4 Wire the app engine: effect handlers for `PersistFavorite`, `SyncFavorites` and `PushFavorite`, the `enabled && explained && verified` sync gate, `now` injection before library inputs, `check_rejected`, and retries after verified access, scan completion, sync activation and Add to GameNode success. Verify with app engine tests for signed-out, sync-off, sync-on and rejected-session paths.

## 3. Library UI

- [x] 3.1 Update `ui/src/api/types.ts` and commands for `LibraryFilters`, the new row/playtime fields, `setFilters` and `setFavorite`; implement the same filter/sort/favorite behavior in the mock host. Verify with `pnpm --dir ui test --run` and the mock host tests.
- [x] 3.2 Build the Filters popover (availability, running, match, playtime, last played, release, date added), the active-filter chip row, the filtered/total count, "Clear all filters" and the search/filter empty-state actions. Verify with component tests for opening, changing, removing one chip, clearing all and preserving search.
- [x] 3.3 Add the seven-key sort select and the icon direction button with key-aware accessible labels and default directions. Verify with component tests for key change defaults, direction flips, labels and that the backend receives the expected `setSort`.
- [x] 3.4 Add the Favorites filter toggle, star indicators in grid/list, the detail-pane favorite button and the single-game context-menu action, with unmatched and multi-selection disabled/omitted states. Verify with component tests for each state and the `setFavorite` command payload.
- [x] 3.5 Add all new strings to `en.ts` and `pt-BR.ts`, update the sync explanation to mention favorites, and keep the catalogs identical. Verify with `pnpm --dir ui test --run` (the i18n key-parity test).
- [x] 3.6 Check keyboard and focus behavior for the filter popover, chips, sort controls and favorite actions, including Escape and focus return. Verify with component tests using `user-event` and a manual keyboard pass in the mock.

## 4. Integration verification

- [x] 4.1 Run `cargo test --locked -p gamenode-core -p gamenode-platform -p gamenode-api -p gamenode-app` and fix any failures across the new filter, sort, migration, API and engine tests.
- [x] 4.2 Run `pnpm --dir ui test --run` and `pnpm --dir ui build`; fix type, lint and test failures.
- [x] 4.3 Exercise `VITE_UI_MOCK=1 pnpm --dir ui dev` at `http://localhost:4777`: combine filters, sort by each new key, favorite/unfavorite a game, use the Favorites filter, and check the empty states and pt-BR locale. Update `docs/development.md` to mention filters, sorting and favorites in the mock, and verify the documented steps still work.

## Workflow follow-up

- Archive the change after the project's review requirements are satisfied.
- Verify the archived result.
