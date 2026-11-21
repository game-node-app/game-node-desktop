# Tasks

Read before starting:

- `design.md` (decisions D1–D16) and the specs under `specs/`.
- Everything in `design-reference/`: the `screens/*.png` visual targets, `raw/*.txt` exact sizes, and `tokens.md`, `components.md`, `icons.md`, `copy.md` and `catalog-view.md`.
- Figma access is not needed.

Rules for every group:

- Follow AGENTS.md:
  - no UI-thread blocking; async commands;
  - Tailwind tokens, with inline style only for runtime values;
  - Mantine v9 for complex controls;
  - Tabler icons;
  - the shared modal layer;
  - `gamenode-asset://` with `<img loading="lazy" decoding="async">`;
  - virtualized grid and list.
- Every user-facing string a task adds goes into both `ui/src/i18n/en.ts` and `ui/src/i18n/pt-BR.ts`, using the text from `copy.md`, in the same task. `i18n.test.ts` must stay green.
- Every new library input and view field is mirrored in `ui/src/mock/host.ts` within the same group that adds it to the engine. That keeps `VITE_UI_MOCK=1 pnpm --dir ui dev` usable at every step.
- Verification commands:
  - `cargo test --locked -p gamenode-core -p gamenode-api -p gamenode-platform -p gamenode-app`
  - `pnpm --dir ui test --run`
  - `pnpm --dir ui typecheck` (or `pnpm --dir ui exec tsc --noEmit` if no script exists)
- No performance tests or benchmarks.

## 1. Design tokens and shared primitives

- [x] 1.1 Add the new color, radius, font-size, letter-spacing, shadow and size tokens from `design-reference/tokens.md`.
  - Add them to `ui/src/styles/theme.css` (`:root`) and `ui/tailwind.config.ts`. Set `--topbar-height` to 48px and add `--statusbar-height: 26px` and `--window-control-width: 46px`. Leave `--sidebar-width` (Settings) untouched.
  - Verify: `pnpm --dir ui test --run` and the typecheck pass, and a scratch class such as `text-caption bg-canvas rounded-popover shadow-popover` compiles in `pnpm --dir ui build`.
- [x] 1.2 Create `ui/src/components/ui/` with `index.ts`. Implement `Button` (primary/secondary/ghost/danger × md/sm, leading/trailing icon, loading, disabled), `IconButton` (outlined/toggle/overlay, required `label`, optional `shortcut`), `Tooltip` (Mantine Tooltip restyled to the tooltip token, with label plus KbdCombo) and `Kbd`/`KbdCombo`, exactly per `components.md`.
  - Verify: `components/ui/Button.test.tsx`, `IconButton.test.tsx` and `Tooltip.test.tsx` cover the variants, the disabled state, `aria-label` and the shortcut rendering.
- [x] 1.3 Implement `Pill` (5 tones, optional icon), `Switch` and `Checkbox` (Mantine, restyled, including the indeterminate state), `SegmentedControl` (Mantine, icon-only option support with labels), `ChoiceChip`, `FilterChip` (removable, with a remove label) and `SectionLabel`, per `components.md`.
  - Verify: Vitest files assert the tone classes, `checked`/`indeterminate` ARIA, keyboard toggling, and the chip remove button's accessible name.
- [x] 1.4 Implement `NavItem` (icon, label, count, trailing slot, active/hover/focus, collapsed-rail mode with tooltip), `StatCard`, `KeyValueRow`, `GroupHeader` (chevron, label, count, collapsed, `aria-expanded`), `Banner` (4 tones, icon, title, body, action, dismiss), `EmptyState` (icon circle, title, body, actions) and `Skeleton`.
  - Verify: Vitest files cover the states and ARIA (`aria-current` on the active NavItem, `aria-expanded` on GroupHeader).
- [x] 1.5 Implement `menuClassNames` / `popoverClassNames` presets for Mantine `Menu`/`Popover` per the common menu surface spec in `raw/06-L4-menus-popovers.txt`: item height, kbd hints, checked items, section labels, danger items, disabled reason sub-label.
  - Verify: `MenuSurface.test.tsx` renders a Menu with a checked item, a disabled item with its reason, and a danger item, and asserts their roles and classes.
- [x] 1.6 Implement `GameTile` and `GameRow` as presentational components (props only, no store access) per `components.md`: fixed 172×270 tile, badges (installed, running with time, family, favorite, hidden), hover play circle, selected ring, hidden opacity, row columns, and the status pill tone map from `copy.md`. Covers use `<img loading="lazy" decoding="async">`.
  - Verify: `GameTile.test.tsx` and `GameRow.test.tsx` cover each badge, the hidden indicator with an icon (not opacity alone), the cover-vs-placeholder title overlay, and the selected state.

## 2. Rust layout model (scope, filters, grouping)

- [x] 2.1 In `crates/core/src/library/layout.rs`, add the new types:
  - `LibraryScope` (D1).
  - `GroupBy`, and `collapsed_groups: BTreeMap<GroupBy, BTreeSet<String>>` on `LibraryLayout`.
  - `SortKey::GameNodeStatus` (default direction ascending).
  - The new `LibraryFilters` shape (D2): `stores`, `match_states`, `gamenode_status`, `PlaytimeThreshold`, `PlayedFilter`, extended `LastPlayedFilter` and `AddedFilter`, `show_hidden`.
  - Drop `Copy` from `LibraryFilters`, set `FIRST_RELEASE_DECADE = 1990`, and change the defaults to `DEFAULT_SIDEBAR_WIDTH = 240` and `DEFAULT_DETAIL_WIDTH = 360`.
  - Fix every compile error across `crates/core` and `crates/app` mechanically.
  - Verify: `cargo build --locked -p gamenode-core -p gamenode-app` succeeds.
- [x] 2.2 Implement legacy reading and `normalized()` folding:
  - `filter`/`sidebar` `{provider}` → `stores=[id]`, scope `All`;
  - single `matchState` → `match_states`;
  - `playtime` `played`/`unplayed` → `played`;
  - `release {decade:1980}` → `Earlier`;
  - the existing `installedOnly`.
  - New saves write only new keys.
  - Verify: new tests in `layout.rs` load JSON from the previous build (one per legacy field, plus a full old layout) and assert the normalized result and the re-serialized keys. The existing layout tests stay green.

## 3. Rust query pipeline (facets, counts, groups, sort, navigation)

- [x] 3.1 Extend `Row` (`query.rs`) with `hidden: bool` and `gamenode: Option<RowGameNode {status: Option<EntryStatus>, collection_ids, entry_id}>`. Fill them from new machine state (hidden set and account library; both empty for now). Precompute `RowFacts` per row.
  - Verify: the existing query and view tests pass unchanged.
- [x] 3.2 Rewrite `filter` as the D3 pipeline:
  - scope predicate: All/Installed/Favorites/Recent (14 days)/Running/Collection, with SavedView treated as All;
  - hidden rule (`show_hidden`);
  - search;
  - each facet, with stores, match states and status as alternatives within their facet;
  - `OlderThanYear` needs a known timestamp.
  - Verify: `query/tests.rs` gains one test per facet and option (including unknown playtime and release behavior, and Not on GameNode covering unmatched rows), and scope tests including Recent at 13 and 15 days.
- [x] 3.3 Implement `FilterCounts` and `ScopeCounts` (D3), with store counts that ignore the stores facet and scope counts that keep facets except stores. Implement release options that list only non-zero decades plus earlier and unknown, and `gamenodeStatusAvailable`. Add collection and saved-view counts.
  - Verify: tests assert that each option count equals the row count obtained by applying that option alone with the other facets, that the sum of store counts equals the All count, and that hidden rows are excluded unless `show_hidden` is on.
- [x] 3.4 Implement grouping (D3): fixed group orders, stable partition after sort, `groups` with `count`/`collapsed`/`firstIndex`, collapsed rows excluded from `rows`, and selection pruned of rows in collapsed groups. Add the `ToggleGroup { id }`, `SetAllGroupsCollapsed(bool)` and `SetGroup(GroupBy)` inputs, which persist the layout.
  - Verify: tests cover the order of each grouping, empty groups omitted, decades newest first then before1990 then unknown, collapse persistence per grouping, and selection pruning.
- [x] 3.5 Implement the `SortKey::GameNodeStatus` rank (playing, ongoing, planned, finished, dropped, inLibrary, notInLibrary; ties by title then key) and group-aware `Move` navigation for grid (each group starts a line) and list. Add `Input::SelectAll` and `Input::ClearSelection`.
  - Verify: tests cover status sort in both directions, up/down crossing a group boundary to the nearest column, skipping collapsed groups, and Select all excluding collapsed and hidden rows.
- [x] 3.6 Expose the new view fields in `view.rs`: `filterCounts`, `scopeCounts`, `groups`, `layout.scope/group/collapsedGroups`, and the header title source (`titleSource: scope|store|collection|savedView` with id).
  - Verify: `view/tests.rs` asserts these fields for a fixture library, and the generation stays stable when nothing changes.

## 4. GameNode API additions

- [x] 4.1 Add `id` to `LibraryEntry` (`crates/core/src/game_api.rs`) and parse it from `EntryRow` in `crates/api/src/game.rs`.
  - Verify: `crates/api/src/game/tests.rs` asserts the id is parsed, and every existing test constructing `LibraryEntry` compiles and passes.
- [x] 4.2 Add `AccountEntry` and `GameApi::list_library_entries`:
  - `GET /v1/collections-entries/library/:userId`;
  - offset/limit 100;
  - `gameFilters[category][]` 0..=14;
  - follow pages until a short page, capped at `MAX_LIBRARY_PAGES = 100` → `ApiError::Failed`.
  - Verify: API tests with mocked responses cover two pages, an empty library, unknown status → `None`, the page cap failing, and the category query parameters present.
- [x] 4.3 Add `GameApi::delete_entry(credential, entry_id)` (`DELETE /v1/collections-entries/:id`; 204 or 404 → `Ok`, 401 → `Rejected`, else `Failed`).
  - Verify: API tests cover each status code.
- [x] 4.4 Add `DEFAULT_WEB_URL = "https://gamenode.app"`, `WEB_URL_ENV = "GAMENODE_WEB_URL"` and a resolver mirroring the server URL resolver in `crates/api/src/lib.rs`, plus helpers building `/game/<id>` and `/library/<userId>`.
  - Verify: unit tests cover the default, the env override and path building. Update every fake `GameApi` in tests (core and app) with the new methods so `cargo test` compiles.

## 5. Local storage (migration 0010)

- [x] 5.1 Create `crates/platform/migrations/0010_library_organization.sql` with `saved_views`, `hidden_entries`, `account_library_entries`, `account_collections` and `account_library_reads` exactly as in design D6, and register it with the migration runner.
  - Verify: a platform test opens a database migrated to 0009 with data in every older table, applies 0010, and asserts that old data is intact and the new tables exist. The "newer schema is rejected" test still passes.
- [x] 5.2 Extend `LibraryStore` and `LibraryCache` (`crates/core/src/library/store.rs`) and implement them in `crates/platform/src/library_db.rs`:
  - `load_cache` returns `saved_views`, `hidden` and `account_library` for the account;
  - `save_view`, `delete_view`, `set_hidden`, `replace_account_library` (one transaction), `save_account_entry`.
  - Verify: `library_db/tests.rs` covers round trips, per-account isolation (account B sees none of account A's rows), unique view names per account, and replacement atomicity (a failed insert leaves the old cache).
- [x] 5.3 Persist the watched-folder last check as setting `discovery.lastCheckedAt` after each completed discovery check, and expose it as `LocalView.discoveryLastCheckedAt` (core view, app snapshot, TS type, mock).
  - Verify: a test in the DRM-free engine tests asserts that the timestamp is written after a check and read back on start.

## 6. Engine: account library cache

- [x] 6.1 Add machine state and inputs/effects for the account library (D4):
  - `SyncAccountLibrary` on verified access and `ScanRequested`;
  - `AccountLibrarySynced`;
  - `PersistAccountLibrary`;
  - `EntryRead` upsert or remove with `PersistAccountEntry`;
  - detail `Membership` is read from the cache first.
  - Wire `Row.gamenode` and collection scopes.
  - Verify: `machine/tests.rs` covers the sync on verification and on rescan, a failed read keeping the cache, `EntryRead` updating one game, rows and counts reflecting statuses, and collection scope filtering.
- [x] 6.2 Handle the new effects in `crates/app/src/engine/mod.rs`: run `list_library_entries` and `list_collections` concurrently on the Tokio runtime, and persist on the storage worker. A credential-less call answers `Err(Failed)` like the other effects.
  - Verify: `crates/app/src/engine/tests/library.rs` covers a sync with a fake API, and a restart that loads the cache before any request.
- [x] 6.3 Mirror the account library in the mock host: fixture statuses and collections, collection scopes, status facet and counts.
  - Verify: `mock/host.test.ts` asserts the collection scope and status-facet counts.

## 7. Engine: organization (saved views, hidden)

- [x] 7.1 Create `crates/core/src/library/organization.rs` with the `SaveView`, `RenameView`, `DeleteView`, `ApplyView` and `SetHidden` inputs, name validation (`viewError`), applied-view highlight tracking, and the `PersistView`, `DeleteViewEffect` and `PersistHidden` effects. Add the view fields `savedViews`, `collections`, `viewError` and `lastSavedView` (for the notification).
  - Verify: unit tests cover saving from the current layout, duplicate and empty names rejected, apply setting the layout and highlight, a facet change dropping the highlight, delete keeping current filters, hide and unhide changing rows and counts, `show_hidden` revealing rows, and per-account reload.
- [x] 7.2 Handle the organization effects in the engine on the storage worker.
  - Verify: an engine test saves a view, hides an entry, restarts the engine with the same store, and finds both.
- [x] 7.3 Mirror saved views and hidden entries in the mock host.
  - Verify: `mock/host.test.ts` covers save, apply, delete, and hide with Show hidden.

## 8. Engine: undo, batch status, add to collection, batch favorites

- [x] 8.1 After `EntryCreated { Ok }`, read the entry back and record `created: Vec<(game_id, entry_id)>` in the add summary. Add `runId` and `undoable` to `AddSummaryView`. Support `AddTarget::Game(u64)` targets for catalog adds (D8).
  - Verify: `add_run/tests.rs` covers recording ids for created entries only, not for already-present ones, and a game-id target.
- [x] 8.2 Implement `UndoAdd { run }`: sequential `DeleteEntry` effects for the created ids, `EntryDeleted` answers, a refresh of those games, and the `undo` view state (done/partial/failed). Handle `DeleteEntry` in the engine.
  - Verify: tests cover a full undo, a partial failure, undo of a stale run rejected, and engine wiring with a fake API.
- [x] 8.3 Implement `SetStatus { keys, status }` (D5): sequential `UpdateEntry` per in-library game, preserving collections, skipping unmatched or not-in-library games and games whose status is unknown, with the `statusRun` view state. It is also used by the single-game detail status select.
  - Verify: tests cover a mixed selection count, preserved collections in the request, and a refresh after each update.
- [x] 8.4 Implement `AddToCollection { keys, collection_id }` (existing ∪ new collection, keeping status) with the `collectionRun` view state.
  - Verify: tests cover adding, already in collection (no request), and not in library (skipped).
- [x] 8.5 Implement `SetFavorites { keys, favorite, now }`, applying the existing single-favorite logic to each matched key. Unmatched keys are ignored.
  - Verify: `favorites/tests.rs` covers Favorite all and Unfavorite all on a mixed selection and the pending and sync behavior for each game.
- [x] 8.6 Mirror runs 8.1–8.5 in the mock host, with simulated success and an optional failure toggle.
  - Verify: `mock/host.test.ts` covers undo, batch status and add to collection.

## 9. Engine: suggestions, catalog, scan progress, palette index, web links

- [x] 9.1 Create `crates/core/src/library/suggest.rs` with normalization, Dice-bigram similarity, the year bonus, confidence, the ≥0.40 threshold, top 2 and tones (D7). Add `SuggestMatches`, `SuggestionsFound` and `UseSuggestion` (via the `PickMatch` path), and detail view `suggestions: [{gameId, title, year, coverUrl, confidence, tone}]`. Handle the effect with `search_page` in the engine.
  - Verify: unit tests cover scores for exact, underscore-separated and partial titles, the year bonus, the threshold, and ambiguous entries scoring candidates without a search. A machine test covers Use creating a chosen match.
- [x] 9.2 Implement the catalog engine contract (D8): `CatalogSearch`, `CatalogSearchDone`, `CatalogClear`, the latest-op-wins rule, `catalog` view with `inLibrary`/`localKey`/`incomplete`/`offline`, and `OpenAddForGame`. Handle the effect in the engine.
  - Verify: tests cover a short query sending nothing, a stale answer dropped, `inLibrary` from the cache, `localKey` from a matched row, and offline status without a verified session.
- [x] 9.3 Add `scanProgress` to the view (D12) from `ScanRun` plus `ProviderScanned` counts, plus the DRM-free pseudo-row.
  - Verify: tests cover the waiting, scanning, done and failed transitions and the found counts.
- [x] 9.4 Add `paletteIndex` (non-hidden rows, independent of filters) to the view (D11).
  - Verify: a view test asserts that it ignores filters and excludes hidden rows.
- [x] 9.5 Add the async Tauri command `open_gamenode_page(page)` in `crates/app/src/commands.rs` (D14), using the session user id and `BrowserOpener` on a blocking worker, and register it.
  - Verify: an app test with a fake opener asserts both URLs and the error when no session exists.
- [x] 9.6 Mirror suggestions, catalog (40 fictional games), scan progress, palette index and `openGameNodePage` (a no-op that logs) in the mock host.
  - Verify: `mock/host.test.ts` covers catalog search and suggestions.

## 10. UI data layer

- [x] 10.1 Update `ui/src/api/types.ts` to match every Rust change in groups 2–9: scope, filters, group, counts, groups, row fields, saved views, collections, runs, suggestions, catalog, scan progress, palette index and `discoveryLastCheckedAt`. Update `createDefaultFilters`, `filtersAreDefault` and `createDefaultSnapshot`. Add command wrappers in `ui/src/api/commands.ts` (`openGameNodePage`).
  - Verify: the typecheck passes, and a `types.test.ts` (or existing equivalent) asserts the default filters and `filtersAreDefault`.
- [x] 10.2 Extend `uiStore`:
  - `ActiveDestination` gains `'catalog'`;
  - `paletteOpen`, `shortcutsOpen`, `saveViewOpen`/`renameViewId`;
  - `filtersOpen`;
  - `dismissedBanners`;
  - the new `ToastItem` shape (D10) with a backward-compatible `pushToast(kind, message)`, `pushNotification(item)`, `updateNotification(id, patch)`.
  - Verify: `uiStore.test.ts` covers the new actions, and existing Settings toast callers compile unchanged.
- [x] 10.3 Document the new mock scenarios (saved views, hidden, collections, catalog, failure toggles) in `docs/development.md`.
  - Verify: the documented `VITE_UI_MOCK=1 pnpm --dir ui dev` steps reach each scenario in the browser.

## 11. Window chrome, top bar, status bar, app menu

- [x] 11.1 Set `.decorations(false)` on the main `WebviewWindowBuilder` in `crates/app/src/main.rs`. Add `core:window:allow-minimize`, `allow-toggle-maximize`, `allow-close`, `allow-start-dragging` and `allow-is-maximized` to `crates/app/capabilities/default.json`.
  - Verify: `cargo build --locked -p gamenode-app` succeeds, and `cargo test --locked -p gamenode-app` passes, including any capability or config test.
- [x] 11.2 Rebuild `features/shell/TopBar.tsx` per `raw/01-shell-topbar-sidebar-statusbar.txt`:
  - 48 px, `data-tauri-drag-region` on non-interactive areas, double-click toggles maximize;
  - wordmark and DESKTOP badge;
  - 440 px search trigger with Ctrl+K kbds that opens the palette;
  - sync status (synced/syncing/off/offline);
  - account avatar opening the app menu.
  - Add `WindowControls.tsx` (D9) with Tauri no-ops outside the runtime.
  - Verify: `TopBar.test.tsx` mocks `@tauri-apps/api/window` and asserts minimize, toggle and close calls, the maximize/restore label swap on resize, the drag-region attribute, and the search trigger opening the palette.
- [x] 11.3 Create `features/shell/StatusBar.tsx` (D12): store items with icon plus text, the watched-folder last check, and the running game with its session time, refreshed by a 60 s interval.
  - Verify: `StatusBar.test.tsx` covers synced, expired, unreachable and checking stores, relative time formatting, the running item, and fake-timer refresh.
- [x] 11.4 Restyle `AppMenu.tsx` per `raw/06-L4-menus-popovers.txt` (connection header, Library section, Add a game, Rescan, Settings with kbd, Keyboard shortcuts, About and licenses, Sign out, Quit) using the menu presets.
  - Verify: the existing AppMenu tests are updated and pass, and a new test asserts that Keyboard shortcuts opens the dialog.
- [x] 11.5 Recompose `App.tsx` into the top bar, sidebar, main (library, catalog or ports), detail slot and status bar grid, keeping the resize handles.
  - Verify: `App.integration.test.tsx` passes with updated selectors, and the browser mock shows the full shell at 1440×900 matching `screens/L1-grid-detail.png`.

## 12. Sidebar

- [x] 12.1 Rebuild `features/shell/Sidebar.tsx` per the raw dump:
  - Seu espaço scopes with `scopeCounts`;
  - Lojas store shortcuts with counts, attention icons from `failedProviders` and store accounts, and active-when-exactly-one-store;
  - Coleções e visões with collections, saved views (bookmark and filter icons, counts, active highlight, a context menu with Rename and Delete) and a "+" header button that opens Save as view;
  - Navegar: Discovered games with a brand count when candidates exist (opens the discovery review), Explore GameNode (signed-in only, sets destination `catalog`), PC ports (Windows and signed-in, existing behavior and unseen badge);
  - a footer with Add DRM-free game (Secondary SM) and Settings with Ctrl+, kbds;
  - the collapsed 72 px rail keeps working with tooltips.
  - Verify: `Sidebar.test.tsx` covers each scope input sent, the store shortcut sending stores=[id] and scope All, the highlight rules, collection and view items, the view context menu, the Discovered count, catalog navigation, the collapsed rail, and keyboard activation.

## 13. Header, toolbar, chips, filter popover, sort and group menus

- [x] 13.1 Create `features/library/LibraryHeader.tsx`: the title from `titleSource`, a subtitle with live counts (`{} jogos · {} instalados neste PC · {} lojas`, or the grouped/selected variant), Ghost "Verificar novamente" (spinner while scanning) and Primary "Adicionar ao GameNode" (opens add for the selection or explains when none).
  - Verify: `LibraryHeader.test.tsx` covers each title source, the subtitle variants and the button states.
- [x] 13.2 Rebuild `LibraryToolbar.tsx`: search (Ctrl+F), a Filters button with the active-facet badge, a Sort menu button showing the current label, a Group menu button, the grid/list `SegmentedControl`, and the details `IconButton` toggle. Remove the old inline banners, which move to group 17.
  - Verify: the existing toolbar tests are updated, and new tests cover the badge count and each control's input.
- [x] 13.3 Create `FilterChipsRow.tsx`: one chip per active facet (multi-value joined with ", ", labels from `copy.md`), each removable, plus "Limpar filtros", "Mostrando {} de {}" and "Salvar como visão".
  - Verify: tests cover chip text for every facet, removing one facet, Clear keeping search, and Save as view opening the dialog.
- [x] 13.4 Create `FilterPopover.tsx` (Mantine Popover, 760×565, three columns) per `raw/05-L3-filter-popover.txt`. It has:
  - filter search;
  - Availability, Store, Shortcuts (Favorites only, Running only, Show hidden with hidden count), Match, GameNode status (disabled with a reason when unavailable), Playtime, History, Last session, Release (dynamic decades), Added;
  - every option with its `filterCounts` count;
  - footer "Os filtros são aplicados na hora", "Limpar tudo" and "Concluído";
  - Ctrl+Shift+F opens it.
  - Verify: `FilterPopover.test.tsx` covers counts rendered, multi-select alternatives, single-choice groups, dynamic decades, filter search hiding sections, the no-match message, and keyboard navigation and Escape.
- [x] 13.5 Create `SortMenu.tsx` (the keys from `copy.md` with checked state; Title A–Z/Z–A as one key with a direction; "Ordem decrescente" toggle; Shift+S) and `GroupMenu.tsx` (groupings with checked state, Collapse all/Expand all).
  - Verify: tests cover the sent inputs, natural default directions and the checked state.

## 14. Grid, list and group headers

- [x] 14.1 Rebuild `GameGrid.tsx` on `GameTile` with `@tanstack/react-virtual`. Group headers are virtual items spanning full width, rows are chunked per group by `columns`, `SetColumns` is still sent, and the hover play button activates Play.
  - Verify: `library.test.tsx` covers group headers rendering and toggling, tiles per group, the badges, and that only a bounded number of tiles are mounted for 1000 rows.
- [x] 14.2 Rebuild `GameList.tsx` on `GameRow` with a sticky 34 px header, the column set and widths from `raw/04-L2-list-grouped-multiselect.txt`, a header checkbox (select all or clear, indeterminate), sortable headers with direction icons, group header rows (36 px), and the favorite column toggling the favorite.
  - Verify: tests cover header sort clicks (key, then direction), checkbox selection, the indeterminate header, the favorite toggle, and virtualization.
- [x] 14.3 Update `useRowHandlers.ts` and `LibraryScreen.tsx` keyboard handling for groups (arrow navigation through the engine; Enter on a focused group header toggles it), Ctrl+A, Esc, F (favorite), Shift+S, Ctrl+L and Ctrl+I. Ignore printable keys while a text field is focused.
  - Verify: tests cover each key, and that typing "f" in search does not favorite.

## 15. Selection, batch bar and context menu

- [x] 15.1 Create `BatchBar.tsx` (floating 878×52 per the raw dump): count, Add to GameNode…, Change status (menu of statuses), Favorite/Unfavorite, Match… (sequential match dialogs over the unmatched and ambiguous selection), and Clear. Disabled actions carry a reason tooltip.
  - Verify: `BatchBar.test.tsx` covers visibility at two or more selected, each input, Unfavorite when all are favorites, and the sequential match flow advancing on confirm or skip.
- [x] 15.2 Rebuild `ContextMenu.tsx` on Mantine Menu presets with the sections and items from `raw/06-L4-menus-popovers.txt` and the `desktop-library` Context menus requirement:
  - Open install folder (existing `openInstallFolder`);
  - Edit on GameNode (`openGameNodePage`);
  - the Add to collection submenu (disabled "Adicione ao GameNode primeiro");
  - View achievements;
  - Hide from library / Show in library;
  - the multi-selection subset.
  - Verify: tests cover the item sets for Steam installed in-library, DRM-free, unmatched, hidden and multi-selection, plus keyboard opening with Shift+F10.

## 16. Detail pane and variants

- [x] 16.1 Rebuild `DetailPane.tsx` per `raw/03-L1-detail-panel.txt`:
  - 196 px hero from artwork or cover with fade and close;
  - title, store and year;
  - primary Play/Install/Running;
  - favorite toggle and more menu;
  - pills, 3 stat cards;
  - On GameNode (status select via `SetStatus`, collections, Edit on GameNode, not-in-library state with Add);
  - Match (provenance text, Change…);
  - Achievements (existing summary, See all);
  - On this PC;
  - Game tools.
  - Split the sections into files under `features/library/detail/`.
  - Verify: tests cover each section for a matched Steam game, the status select sending `setStatus`, and the not-installed and family-shared notes.
- [x] 16.2 Implement the variants from `raw/08-L6-detail-variations.txt`:
  - DRM-free running (session stat, executable, arguments, save folder, protection active);
  - unmatched with the no-match notice, Match…, Not a game, and the suggestions list with confidence pills and Use;
  - family shared not installed;
  - the multi-select panel (stacked covers, summaries, total time, "{} de {}" on GameNode, batch actions, selection list with remove).
  - Verify: tests cover each variant's content and actions, including Use sending `useSuggestion` and remove-from-selection.

## 17. States and banners

- [x] 17.1 Replace the empty-state markup in `LibraryScreen.tsx` with `features/library/states/`:
  - the first-run guide (three steps with done state and actions);
  - no results (search vs filters copy, active-filter count, Clear filters, Search GameNode → catalog);
  - no installed, and empty Steam (restyled, existing copy);
  - the scanning panel with per-store progress and skeleton tiles.
  - Verify: tests cover each state from the matching `EmptyState` / `scanProgress` fixtures.
- [x] 17.2 Create `LibraryBanners.tsx` (D13): offline, store expired (Sign in again → Settings › Stores), store unreachable (Try now → rescan or check), and discovered (Review → discovery modal), with session dismissal and reappearance after recovery.
  - Verify: tests cover the priority, each action, dismissal, and reappearance.

## 18. Notifications

- [x] 18.1 Rebuild `ToastContainer.tsx` on Mantine `Notification` (D10): bottom-right stack, tones and icons, title and body, up to two actions, progress bar, 6 s auto-close paused on hover and focus, persistent danger and progress, and `aria-label="Notifications"` kept.
  - Verify: `ToastContainer.test.tsx` is updated for auto-close with fake timers, hover pause, actions and persistence. `SettingsModal.test.tsx` still finds the "Notifications" region.
- [x] 18.2 Create the `useLibraryNotifications` hook for the add summary (Undo → `undoAdd`, View on GameNode → `openGameNodePage(library)`), undo result, status run, collection run, view saved, hide (Show again → `setHidden false`) and launch failure (Locate executable, View log), each emitted once per run id.
  - Verify: tests cover each notification's text, actions and the once-per-run rule.

## 19. Command palette, shortcuts dialog, save-view dialog

- [x] 19.1 Create `features/shell/CommandPalette.tsx` on the shared modal layer (600 px, top-anchored):
  - input;
  - groups Games (from `paletteIndex`), Actions and Settings (deep links via `openSettingsModal(topic)`);
  - arrows, Enter (play or open), Shift+F10 (game context menu), footer hints;
  - "Search “{}” on GameNode" → catalog with the query;
  - Ctrl+K and the top-bar trigger open it.
  - Verify: `CommandPalette.test.tsx` covers filtering, grouping caps, keyboard navigation, each action, focus trap, Escape and focus restore.
- [x] 19.2 Create `ShortcutsModal.tsx` on `AppModal` with the General, Library and Selected game lists from `copy.md`, opened by `?` (outside text fields), the app menu and the palette. Create the `useGlobalShortcuts` hook in `App.tsx` (D11), folding in `useSettingsKeyboardShortcut` behavior.
  - Verify: tests cover `?` opening it, `?` typed in search not opening it, and each global shortcut dispatching its action.
- [x] 19.3 Create `SaveViewModal.tsx` on `AppModal` (340 px per the raw dump): name field, "Inclui" summary chips (scope, filters, sort, group), hint, Cancel/Save view, live duplicate and empty validation, and a rename mode. Add a delete confirmation `AppModal`.
  - Verify: tests cover saving, rename, duplicate blocking, the engine `viewError` shown, and delete confirm.

## 20. Catalog view

- [x] 20.1 Create `features/catalog/CatalogView.tsx` per `design-reference/catalog-view.md`:
  - header and 480 px search (autofocus, 300 ms debounce, ≥2 chars);
  - virtualized `GameTile` grid with remote covers;
  - In your library / On this PC badges;
  - the idle, loading, no results, error and offline states;
  - the incomplete caption;
  - keyboard focus and navigation.
  - Verify: `CatalogView.test.tsx` covers debounce with fake timers, short query, states, badges, and stale answers ignored via the view.
- [x] 20.2 Create the catalog detail variant (Add to GameNode… via `openAddForGame`, View on GameNode, Show in library selecting and scrolling the local row) and the tile context menu.
  - Verify: tests cover each action, and the add dialog opening in catalog mode.

## 21. Integration and acceptance

- [x] 21.1 Run the full suites and fix regressions in previously passing tests: `cargo test --locked -p gamenode-core -p gamenode-api -p gamenode-platform -p gamenode-app`, `pnpm --dir ui test --run`, the typecheck, and `pnpm --dir ui build`.
  - Verify: all four commands exit 0.
- [x] 21.2 Compare in the browser mock (`VITE_UI_MOCK=1 pnpm --dir ui dev`, 1440×900) against each `design-reference/screens/*.png`:
  - L1 grid + detail;
  - L2 list grouped + multi-select;
  - L3 filters;
  - L4 menus, palette, save view, tooltip;
  - L5 states, banners, notifications, shortcuts;
  - L6 detail variants;
  - catalog;
  - both locales (en and pt-BR).
  - Verify: each surface matches its reference layout, spacing, colors and copy, with mismatches fixed. Record the check in the change notes.
- [ ] 21.3 **Windows handoff — Lamarck (GameNode Core).** Run native Windows acceptance via `scripts/install-windows-wsl.sh`:
  - top-bar drag moves the window, and double-click toggles maximize;
  - minimize, maximize/restore and close (close keeps the app in the tray per desktop-lifecycle);
  - resize from edges and corners respects 960×600;
  - window geometry is restored after restart;
  - the 48 px bar renders correctly at 100%, 125% and 150% DPI;
  - Snap Layouts loss is noted.
  - Verify: each item is checked on Windows and recorded in `docs/windows-verification.md`.
- [x] 21.4 Run `openspec validate redesign-desktop-library --strict` and update `docs/development.md` and `docs/local-state.md` for the new tables, settings key and mock scenarios.
  - Verify: validation passes, and the docs mention `0010_library_organization.sql` and `discovery.lastCheckedAt`.

## Workflow follow-up

- Commit only when the user asks, following CONTRIBUTING.md. Keep Rust core, platform, app and UI changes in separate commits where practical.
- Archive the change with `/opsx:archive` after review and native acceptance.
