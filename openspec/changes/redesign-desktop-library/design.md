# Design

## Context

See `proposal.md` for the motivation. The specs under `specs/` define the required behavior. All visual detail lives in `design-reference/`:

| File | Contents |
|---|---|
| `tokens.md` | Design tokens |
| `components.md` | Primitives and their states |
| `icons.md` | Tabler icon names |
| `copy.md` | Every string, in en and pt-BR, with suggested i18n keys |
| `catalog-view.md` | Spec for the view that has no Figma frame |
| `raw/*.txt` | Exact Figma dumps |
| `screens/*.png` | Visual targets |

Implementers must not need Figma.

Current state that shapes the approach:

- **Library engine.** `LibraryMachine` (`crates/core/src/library/machine.rs`) is a pure state machine.
  - The UI sends `LibraryInput` through `commands.libraryInput`. The engine (`crates/app/src/engine/mod.rs`) turns each `Effect` into a Tokio task or a storage-worker job, then feeds the answer back as an `Input`.
  - The view is rebuilt lazily (`view_dirty`) and published in the coalesced `engine:snapshot`.
  - Filtering and sorting live in `query.rs` (`filter`, `sort`, provider counts). The layout lives in `layout.rs` and is saved as JSON in the global settings table (`save_layout`).
- **Layout model.** `SidebarSelection` is `All | Provider(id)`.
  - `LibraryFilters` is `Copy`. It has a single `match_state`, a `playtime` that mixes thresholds with played/unplayed, and `release` with `Earlier` meaning before 1980.
- **Account library.** GameNode library state is only read per selected game (`Effect::ReadEntry` → `Membership`).
  - `LibraryEntry` has no `id`, and `create_entry` returns `()`. There is no delete call.
  - Collections are listed only while the add dialog is open.
- **UI shell.**
  - `ui/src/features/shell/` has a 54 px `TopBar`, the `Sidebar` (collapsible to a 72 px rail), `AppMenu` and `ResizeHandle`.
  - The library is `LibraryScreen` + `LibraryToolbar` + `GameGrid` / `GameList` + `DetailPane` + `ContextMenu`.
  - Toasts are `{kind: success|danger, message}` in `uiStore`, rendered by `ToastContainer`.
- **Window.** Created in `crates/app/src/main.rs` with native decorations.
  - `capabilities/default.json` grants only `core:default`, `core:event:default` and `core:window:default`.
- **Mock host.** `ui/src/mock/host.ts` re-implements filtering and sorting in TypeScript for `VITE_UI_MOCK=1`.
- **Constraints (AGENTS.md).**
  - Never block the UI thread.
  - Async commands only.
  - Covers via `gamenode-asset://` and `<img loading="lazy" decoding="async">`.
  - Virtualized grid and list.
  - Tailwind tokens; inline style only for runtime values.
  - Mantine v9 for complex controls; `FloatingModal` / `AppModal` and the shared modal layer for overlays.
  - Tabler icons only.
  - en + pt-BR for every string.
  - Windows-first. No performance tests or benchmarks.

## Goals / Non-Goals

**Goals:**

- Keep the engine the single source of truth for rows, counts, groups and selection.
  - The UI only renders the view and sends inputs.
  - Faceted counts and grouping are computed in Rust, once per view build.
- Keep every layout saved by an older build loadable, through serde defaults and aliases plus explicit normalization.
- Introduce a reusable primitive set (`ui/src/components/ui/`). Every redesigned surface uses it, so later redesign sections reuse it too.
- Make each phase independently testable: Rust unit tests per module, Vitest per component or view, and the mock host for manual browser checks.

**Non-Goals:**

- No server changes. Only existing GameNode endpoints are used.
- No light theme. The redesign is dark only, as today.
- No redesign of the Settings modal, the PC ports screen content, the match and add dialogs' inner layouts, game tools, save protection or achievements modals.
  - They keep their behavior. They get the new primitives only where they render shared chips or buttons.
- No Linux-specific window chrome work beyond keeping it functional (MVP is Windows-first).
- No `@mantine/notifications` package. No new npm or crate dependencies.

## Decisions

### D1 — Scope + store facet replace `SidebarSelection`

**Scope type**

```rust
pub enum LibraryScope {
    All, Installed, Favorites, Recent, Running,
    Collection(String), SavedView(i64),
}
```

- `layout.scope` is serialized as `scope`.
- `Installed` is a scope that behaves exactly like `availability.installed` ANDed with the other facets. It is a scope rather than toggling the facet, so the sidebar highlight is unambiguous.
- `Recent` means last played ≤ 14 days ago (`RECENT_DAYS = 14`).
- `SavedView(id)` is only a highlight marker. Applying a view copies its payload into the layout (D6).

**Store facet**

- Store items set `filters.stores = vec![id]` and `scope = All`.
- A store item is active when `scope == All && filters.stores == [id]`.

**Migration of old layouts**

- `layout.filter` (`"all"` | `{provider: id}`, aliased from `sidebar`) is kept as a read-only `legacy_sidebar: Option<SidebarSelection>`, `skip_serializing`.
- `normalized()` folds `Provider(id)` into `stores = [id]`.

*Alternative considered:* keep provider as a sidebar selection next to a stores facet. Rejected: two sources of truth for the same predicate, and the counts rule ("counts ignore the store facet") would need special cases.

### D2 — Filters model (no longer `Copy`)

```rust
pub struct LibraryFilters {
    pub availability: AvailabilityFilter,        // unchanged (installed/owned/shared)
    pub stores: Vec<String>,                     // empty = any; alternatives
    pub match_states: Vec<MatchFilterValue>,     // Matched|Ambiguous|Unmatched|NoGame; empty = any
    pub gamenode_status: Vec<GameNodeStatusFilter>, // Playing|Finished|Planned|Ongoing|Dropped|InLibrary|NotInLibrary
    pub playtime: PlaytimeThreshold,             // Any|AtLeast10h|AtLeast100h
    pub played: PlayedFilter,                    // Any|Played|Unplayed
    pub last_played: LastPlayedFilter,           // Any|Last7Days|Last30Days|Last90Days|LastYear|OlderThanYear
    pub release: ReleaseFilter,                  // Any|Decade(u16)|Earlier (<1990)|Unknown
    pub added: AddedFilter,                      // Any|Last7Days|Last30Days|Last90Days|LastYear|OlderThanYear
    pub favorites_only: bool,
    pub running_only: bool,
    pub show_hidden: bool,
}
```

- The struct derives `Clone`, not `Copy`. Call sites that relied on copy use `.clone()` or references.
- `#[serde(default)]` stays on the struct.

**Legacy fields** (read-only, `skip_serializing`, folded by `normalized()`):

| Legacy field | Folds into |
|---|---|
| `matchState` (single value; `any` → empty) | `match_states` |
| `playtime: "played"` / `"unplayed"` | `played` |
| `playtime: "atLeast10h"` / `"atLeast100h"` | `playtime` |
| `release: {decade: 1980}` | `Earlier` |

The new `playtime` field reuses the JSON name `playtime`. A custom `Deserialize` on `PlaytimeThreshold` accepts the old string set and maps `played`/`unplayed` to `Any`, while `normalized()` reads the raw value into `played`. The simplest implementation is a `#[serde(deserialize_with)]` helper that returns `(PlaytimeThreshold, Option<PlayedFilter>)` into a private `legacy_played`. Pick that.

**Copy mapping**

- `Added` labels map to rolling windows: Esta semana → 7 days, Este mês → 30 days, Este ano → 365 days, Antes → `OlderThanYear`.
- `Last90Days` stays valid for old layouts but has no chip in the popover. If it is active, it shows as a "90 days" chip.

**Constants**

- `FIRST_RELEASE_DECADE` becomes `1990`.
- Unknown playtime, last-played or release never satisfies a threshold or window, except `OlderThanYear`, which also requires a known timestamp.

**Predicates**

- `GameNodeStatusFilter` uses the account library cache (D4). Unmatched entries and matched games without a cache entry count as `NotInLibrary`.
- `InLibrary` means "in the library with no status this build knows".
- If the cache has never been read for this account, `gamenode_status` facets are disabled in the UI: `filterCounts.gamenodeStatusAvailable = false`.

### D3 — Faceted counts, groups and row pipeline in `query.rs`

The pipeline:

```text
scoped     = rows ∩ scope ∩ hidden-rule ∩ search
per facet  = scoped ∩ every other active facet
counts[facet][option] = |per facet ∩ option|
visible    = scoped ∩ all facets
sorted     = sort(visible)
grouped    = stable partition of sorted by group key, in the group's fixed order
rows       = grouped minus rows in collapsed groups
```

**Implementation**

- One pass per facet over `scoped`, which is O(facets × n), ≈ 10 × n. Each row's facet results are precomputed once into a small bitset struct (`RowFacts`), so per-facet passes are cheap.
- `FilterCounts` is serialized into `LibraryView.filterCounts`:

  ```text
  stores: [{id, name, count}]
  availability: {installed, owned, shared}
  matchStates: {...}
  gamenodeStatus: {..., available: bool}
  playtime: {any, atLeast10h, atLeast100h}
  played: {...}
  lastPlayed: {...}
  release: [{value, count}]   // only non-zero decades, then earlier and unknown if > 0
  added: {...}
  favoritesOnly, runningOnly, showHidden (hidden count)
  ```

- `scopeCounts: {all, installed, favorites, recent, running}` uses `scoped` without facets, but with the hidden rule and search.
- **Sidebar counts.** Scope counts apply the current facets except stores. Store counts apply the scope `All` and every facet except stores. This keeps "counts never promise hidden rows" consistent with D1.
- Collection counts and saved-view counts are computed by evaluating each one's predicate over the hidden-filtered rows.
  - The cost is bounded by the number of collections plus views (tens), so it is computed only when `view_dirty`.

**Groups**

```rust
pub enum GroupBy { None, Store, GameNodeStatus, MatchState, Installation, ReleaseDecade }
```

Fixed orders:

| Group by | Order |
|---|---|
| Store | Provider registry order |
| GameNodeStatus | playing, finished, planned, ongoing, dropped, inLibrary, notInLibrary (Figma order) |
| MatchState | matched, ambiguous, unmatched, noGame |
| Installation | installed, notInstalled |
| ReleaseDecade | Newest decade first, then before1990, unknown |

- The view gets `groups: [{id, labelKey, count, collapsed, firstIndex}]`, where `firstIndex` indexes into `rows`. Collapsed groups have `count > 0` and no rows.
- **Collapsed state.** `layout.collapsed_groups: BTreeMap<GroupBy, BTreeSet<String>>` is persisted, so each grouping remembers its own collapsed set.
- **Navigation.** `Move` uses group-aware positions.
  - In the grid, each group starts a new line. The machine computes `(group, line, col)` from `columns` and group sizes.
  - Up/down crosses group boundaries to the nearest column. Collapsed groups are skipped.
  - In the list, movement is linear over `rows`.
  - Select all (`Input::SelectAll`) selects `rows`, which already excludes collapsed groups and hidden rows.

**Sort**

- New `SortKey::GameNodeStatus`. Rank: playing 0, ongoing 1, planned 2, finished 3, dropped 4, inLibrary 5, notInLibrary 6.
- Ascending by default. Ties break by title, then key, as today.

*Alternative considered:* compute counts and groups in React. Rejected: the mock host would diverge further, the counts must match the sidebar rule exactly, and 1000+ rows × facets per keystroke belongs off the UI thread.

### D4 — Account library cache

**API**

- `LibraryEntry` gains `id: String`, parsed from `EntryRow.id`. Existing tests are updated.
- `GameApi::list_library_entries(credential, user_id) -> Result<Vec<AccountEntry>, ApiError>`, where `AccountEntry { id, game_id, status: Option<EntryStatus>, collection_ids }`.
  - Implemented in `crates/api/src/game.rs` over `GET /v1/collections-entries/library/:userId`, paginated with offset/limit (limit 100).
  - It passes `gameFilters[category][]=0..=14` so non-default categories are included.
  - It follows pages until a short page arrives, capped at `MAX_LIBRARY_PAGES = 100`. Hitting the cap returns `ApiError::Failed`, so a partial read never replaces the cache. This mirrors `list_favorites`.
- `GameApi::delete_entry(credential, entry_id) -> Result<(), ApiError>` calls `DELETE /v1/collections-entries/:id` and expects 204. A 404 counts as success (already gone).

**Machine**

- `account_library: Option<AccountLibrary { entries: BTreeMap<u64 /*game*/, AccountEntry>, collections: Vec<Collection>, read_at: i64 }>`. It is loaded with `LibraryCache` (new field `account_library`).
- `Effect::SyncAccountLibrary { op, user_id }` is emitted on verified access (`SetAccess` with a verified session), on `ScanRequested`, and after a manual Retry.
  - The engine runs `list_library_entries` and `list_collections` concurrently.
  - It answers `Input::AccountLibrarySynced { op, result: Result<(Vec<AccountEntry>, Vec<Collection>), ApiError>, now }`.
  - On `Ok`, the machine replaces the cache and emits `Effect::PersistAccountLibrary { account_id, entries, collections, read_at }`.
- **Single-game refresh.** After `EntryCreated` / `EntryUpdated` / `EntryDeleted`, the machine emits `ReadEntry` for each affected game. `EntryRead` also upserts or removes that game in `account_library` and emits `Effect::PersistAccountEntry { account_id, game_id, entry: Option<AccountEntry> }`.
  - `Membership` in the detail view comes from the cache first, then the per-selection read, as today.
- **Sync toggle.** Reading is allowed while sync is off (see the `desktop-sync` delta). The sync toggle is not consulted.

*Alternative considered:* read entries per visible row on demand. Rejected: the status column, the status facet and collection scopes need all rows, and offline display needs a cache.

### D5 — Undo add, batch status, add to collection

**Undo add**

- `create_entry` still returns `()`, because the server sends no id. After `EntryCreated { Ok }`, the add run emits `ReadEntry` for that game and records the id from the answer into `AddSummary.created: Vec<(u64 game_id, String entry_id)>`.
- `Input::UndoAdd { run }` emits `Effect::DeleteEntry { op, game_id, entry_id }` for each created pair. `Input::EntryDeleted { op, game_id, result }` answers each one.
- When all have answered, the machine publishes `undo: {run, state: done|partial|failed, failed: n}`. The UI shows the matching notification.
- Undo is offered only while the add notification is visible, and only for the latest run. `AddSummaryView` gains `runId` and `undoable: bool`.

**Batch status**

- `Input::SetStatus { keys, status }` resolves keys to matched game ids present in the cache.
- It emits `UpdateEntry { existing }` for each, with `existing` taken from the cache entry. This preserves collections, and platforms are preserved by `update_entry` as today.
- It counts `skipped` (unmatched, or not in the library) and publishes `statusRun: {run, changed, skipped, failed, status}` for the notification.
- **Single-game status select** in the detail pane uses the same input with one key.

**Add to collection**

- `Input::AddToCollection { keys, collection_id }` calls `UpdateEntry` with `collection_ids = existing ∪ {id}` and the existing status. The result is published the same way (`collectionRun`).
- `update_entry` currently requires a status. When a cached status is `None` (unknown to this build), the game is skipped with reason `unknownStatus`. This is rare and keeps the server's status untouched.

**Ordering.** All three run sequentially, one request at a time, like the add run. This avoids request floods and keeps the progress deterministic.

### D6 — Saved views and hidden entries (new module `library/organization.rs`)

**Storage** (`0010_library_organization.sql`):

```sql
CREATE TABLE saved_views (
  account_id TEXT NOT NULL, id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL, payload TEXT NOT NULL,    -- JSON {scope, filters, sort, group, view}
  position INTEGER NOT NULL, created_at INTEGER NOT NULL,
  UNIQUE (account_id, name)
) STRICT;
CREATE TABLE hidden_entries (
  account_id TEXT NOT NULL, entry_key TEXT NOT NULL, hidden_at INTEGER NOT NULL,
  PRIMARY KEY (account_id, entry_key)
) STRICT;
CREATE TABLE account_library_entries (
  account_id TEXT NOT NULL, game_id INTEGER NOT NULL, entry_id TEXT NOT NULL,
  status TEXT, collection_ids TEXT NOT NULL,    -- JSON array
  PRIMARY KEY (account_id, game_id)
) STRICT;
CREATE TABLE account_collections (
  account_id TEXT NOT NULL, collection_id TEXT NOT NULL, name TEXT NOT NULL,
  default_status TEXT, position INTEGER NOT NULL,
  PRIMARY KEY (account_id, collection_id)
) STRICT;
CREATE TABLE account_library_reads (
  account_id TEXT PRIMARY KEY, read_at INTEGER NOT NULL
) STRICT;
```

- `INTEGER PRIMARY KEY AUTOINCREMENT` with a composite UNIQUE is valid in SQLite.
- `entry_key` is the library key `<provider>:<id>`, so hiding applies to one provider entry, not to every entry of the game.
- The watched-folder last check is stored as setting `discovery.lastCheckedAt` (unix seconds) in the existing settings table. It is written by the discovery worker after each completed check and exposed as `LocalView.discoveryLastCheckedAt`.

**LibraryStore methods**

- `load_cache` adds `saved_views`, `hidden`, `account_library`.
- New methods:
  - `save_view(account, &SavedView) -> Result<i64>` (insert or rename/update by id)
  - `delete_view(account, id)`
  - `set_hidden(account, keys, hidden, now)`
  - `replace_account_library(account, entries, collections, read_at)` (one transaction)
  - `save_account_entry(account, game_id, Option<&AccountEntry>)`

**Inputs**

| Input | Effect |
|---|---|
| `SaveView { name }` | Snapshot of the current layout |
| `RenameView { id, name }` | |
| `DeleteView { id }` | |
| `ApplyView { id }` | Copies the payload into the layout and sets `scope = SavedView(id)` only as the highlight, keeping `applied_view_payload` to compare |
| `SetHidden { keys, hidden }` | |

- On every layout change, if the new effective payload differs from the applied one, the scope highlight falls back to the payload's own scope.
- **Name validation.** Names are trimmed and must be non-empty and unique (case-insensitive) per account. The machine rejects a bad name with `viewError: "duplicate"|"empty"` in the view. The dialog also validates live against `savedViews`.
- **View fields.**
  - `LibraryView.savedViews: [{id, name, count, active}]`
  - `collections: [{id, name, count}]`
  - `Row.hidden: bool`
  - `Row.gamenode: Option<{status, collectionIds, entryId}>`

*Alternative considered:* store saved views in the global layout JSON. Rejected: the specs require per-account storage, and layout JSON is PC-wide.

### D7 — Match suggestions

- When a single selected row is `Unmatched` or `Ambiguous`, the session is verified and no suggestion is cached for that key, the machine emits `Effect::SuggestMatches { op, key, query }`, where `query` is the inferred title, or else the detected name.
  - For `Ambiguous`, the candidates are scored instead and no search runs.
- The engine reuses `search_page` and answers `Input::SuggestionsFound { op, key, result }`.
- **Scoring** is a pure function in `library/suggest.rs`:
  - `title_score` is a normalized token similarity: lowercase, strip punctuation and `_`/`-`/`.`, collapse whitespace, then Sørensen–Dice on character bigrams, 0..1.
  - `year_bonus` = +0.1 if a year present in the detected path or name equals the release year, else 0.
  - `confidence = min(1, title_score*0.9 + year_bonus)`, shown as a percentage.
  - Keep results ≥ 0.40 and take the top 2.
  - Tone: ≥ 0.80 success, 0.50–0.79 warning, below that neutral.
- Results are kept in memory per key for the session; nothing is persisted.
- `Input::UseSuggestion { key, game_id }` follows the existing `PickMatch` path, including persisting the choice and fetching the cover.

### D8 — Catalog view

- See `design-reference/catalog-view.md` for the UI.
- **Engine**
  - `Input::CatalogSearch { query }` stores the query.
    - If the trimmed length is < 2, it sets status `idle` and emits nothing.
    - Otherwise it emits `Effect::CatalogSearch { op, query }` (`search_page`). The latest op wins.
  - `Input::CatalogSearchDone { op, query, result }`.
  - `Input::CatalogClear`.
- **View.** `LibraryView.catalog: {query, status, results, incomplete}`.
  - Each result has `inLibrary` from `account_library`, and `localKey` from the first row whose `status.game_id()` equals the result's id.
  - Status is `offline` when access is not verified or the network is down (`Access`).
- **Add from catalog.**
  - `Input::OpenAddForGame { game_id, title, cover_url }` opens the existing add dialog in a "catalog" mode keyed by game id instead of row keys.
  - `add_run` accepts targets as `AddTarget::Game(u64)` or `AddTarget::Row(key)`.
  - The create flow (`ReadEntry` → `CreateEntry` → read back) is unchanged.
- **Destination.** `uiStore.activeDestination` gains `'catalog'`. The UI debounces typing by 300 ms before sending `catalogSearch`.

### D9 — Frameless window and chrome

**Rust**

- `WebviewWindowBuilder::decorations(false)` in `main.rs`. Keep `min_inner_size(960, 600)`; Tauri keeps resize borders on Windows with decorations off.
- Add to `capabilities/default.json`: `core:window:allow-minimize`, `core:window:allow-toggle-maximize`, `core:window:allow-close`, `core:window:allow-start-dragging`, `core:window:allow-is-maximized`.

**UI**

- `WindowControls.tsx` uses `getCurrentWindow()` from `@tauri-apps/api/window`.
  - Minimize, toggleMaximize and close.
  - `onResized` updates the maximized flag, which switches the icon between `IconSquare` and `IconCopy` and the label between Maximizar and Restaurar.
- `close()` triggers `CloseRequested`, so the existing close-to-background lifecycle handler still applies.
- The top bar root carries `data-tauri-drag-region`. Interactive children do not.
- Double-click on empty top-bar space calls `toggleMaximize()` explicitly. Tauri also handles it, but the explicit call makes behavior consistent across platforms.
- **Outside Tauri** (`isTauriRuntime()` false: Vite or mock), the controls render but call no-ops, so the browser preview shows the full design.
- **Geometry persistence** (`updateWindowGeometry`) is unchanged.

*Alternative considered:* keep native decorations and draw the top bar under them. Rejected by the user. The custom chrome matches the design, and Snap Layouts loss is accepted and listed for native acceptance.

### D10 — Design system implementation

**Tokens**

- Added to `ui/src/styles/theme.css` (`:root`) and `ui/tailwind.config.ts`, exactly as listed in `tokens.md`.
- Existing token names are kept. New ones are additive.
- `--topbar-height` becomes 48px.
- Rust defaults change: `DEFAULT_SIDEBAR_WIDTH = 240`, `DEFAULT_DETAIL_WIDTH = 360`. Saved layouts keep their own widths.

**Primitives** (`ui/src/components/ui/`, exported from `index.ts`)

- The list: Button, IconButton, Tooltip, Kbd, KbdCombo, Pill, Switch, Checkbox, SegmentedControl, ChoiceChip, FilterChip, NavItem, SectionLabel, StatCard, KeyValueRow, MenuSurface (class presets for Mantine `Menu` and `Popover`), GameTile, GameRow, GroupHeader, Banner, EmptyState (icon-circle layout), Skeleton.
- Mantine components are wrapped and restyled through `classNames`, so call sites never pass raw Mantine props for colors.
- Each primitive has a Vitest file covering its variants, states and accessibility attributes.

**Notifications**

- `ToastItem` becomes `{ id, kind: 'success'|'danger'|'info'|'progress', title, body?, actions?: {label, onClick}[], progress?: number, persistent?: boolean }`.
- `pushToast(kind, message)` keeps working as a shorthand, so existing callers in Settings are untouched.
- `ToastContainer` renders Mantine `Notification`s in a fixed bottom-right stack with `aria-live="polite"` and `aria-label="Notifications"`. That label is required by existing tests.
- **Auto-close.** Success and info close after 6 s via a timer that pauses on hover or focus. Danger and progress persist.
- Engine-driven notifications are created by a `useLibraryNotifications` hook. It watches `addSummary` / `undo` / `statusRun` / `collectionRun` / `viewSaved` changes, keyed by run id, so each run notifies once.

### D11 — Command palette and shortcuts

**Palette**

- `CommandPalette.tsx` is built on the shared modal layer: `modalLayer.ts` registration, focus trap, Escape and focus restore. It is not built on `AppModal`, because its layout differs: 600 px wide, top-anchored at 96 px.
- **Sources**
  - Games: a lightweight `paletteIndex: [{key, title, providerName, installed, playtimeLabel}]` that the engine publishes for every non-hidden row, independent of the active filters. The palette filters it client-side by substring, with no debounce; this is cheap for a few thousand items.
  - Actions: a static list.
  - Settings: a static list built from the existing settings topic and item labels.
- At most 5 games, 5 actions and 5 settings are shown.

**Shortcuts**

- A single `useGlobalShortcuts` hook mounted in `App.tsx`. It replaces the scattered handlers and keeps `useSettingsKeyboardShortcut`'s behavior.
- It ignores events whose target is an input, textarea, select or contenteditable, except Ctrl/Cmd combos and Escape.
- Library-only keys (F, Enter, arrows, Ctrl+A, Shift+S, Shift+F10) stay in `LibraryScreen`'s focused-container handler, so they never fire over modals.

| Key | Action | Notes |
|---|---|---|
| Ctrl+K | Palette | |
| Ctrl+, | Settings | |
| F5 | Rescan | |
| Ctrl+Q | Quit (`commands.quitApp`) | |
| Ctrl+F | Focus library search | |
| Ctrl+Shift+F | Open filters | |
| Ctrl+L | Toggle view | |
| Ctrl+I | Toggle detail pane | |
| ? | Shortcuts dialog | |
| Esc | Clear selection | Only when no overlay is open |

### D12 — Status bar and scanning progress

- **Status bar data comes from existing views.**
  - `storeAccounts`: `state`, `lastSyncedAt`, `lastCheckedAt`, `catalogError`.
  - `library.failedProviders`.
  - `localGames.discoveryScanning` / `discoveryLastCheckedAt` (new).
  - The running row (`rows.find(r => r.running)`, plus `playtime` session).
- Relative time is formatted in the UI and refreshed by a 60 s interval timer.
- **Per-provider scan progress.**
  - `ScanRun` already tracks `waiting`. The view gains `scanProgress: [{provider, name, state: waiting|scanning|done|failed, found: number|null}]`. `found` is set from the `ProviderScanned` record count.
  - Concurrent scans show as `scanning` until answered.
  - DRM-free folders appear as a pseudo-provider row: `scanning` while `discoveryScanning`, otherwise `done`.
- The scanning panel is shown above the content while `scanning` and the library has rows. When the library has no rows yet, it is shown as the centered panel with 8 skeleton tiles.

### D13 — Banners

- `LibraryBanners.tsx` derives at most one banner per kind, in priority order: offline, store expired, store unreachable, discovered.
- Inputs:
  - `Access` / network state.
  - `storeAccounts[].state` (`expired` / `unreachable` equivalents) and `catalogError`.
  - `localGames.recentlyAdded.length`.
- Dismissal is kept in `uiStore.dismissedBanners: Set<string>` (session only), keyed by `kind:provider:problemSince`. A new occurrence after recovery shows again.
- This replaces the toolbar's current inline failed-provider and store-warning banners.

### D14 — Web links

- `crates/api` adds `DEFAULT_WEB_URL = "https://gamenode.app"`, overridable by the `GAMENODE_WEB_URL` env var, which is read the same way as the server URL override.
- New async command `open_gamenode_page(page: GameNodePage)`.
  - `GameNodePage = Game { id } | Library`.
  - It builds `/game/<id>` or `/library/<userId>` (user id from the verified session) and opens it with the existing `BrowserOpener` on a blocking worker.
  - Errors return a localized `CommandError`.
- The UI wraps it as `commands.openGameNodePage`.

### D15 — Mock host parity

- `ui/src/mock/host.ts` mirrors every new input and view field with fixture data. The fixture covers:
  - 3 stores;
  - every match state and GameNode status;
  - 2 collections and 1 saved view;
  - hidden entries;
  - one running game;
  - one expired store;
  - discovered candidates;
  - 40 catalog games.
- The faceted-count and grouping logic in the mock is a direct TypeScript port of D3's pipeline. `host.test.ts` covers the counts rule and grouping, so the browser preview behaves like the engine.

### D16 — Testing strategy

- **Rust (`cargo test --locked -p gamenode-core -p gamenode-api -p gamenode-platform -p gamenode-app`)**
  - layout migration;
  - each facet predicate;
  - faceted counts;
  - sidebar count rules;
  - grouping order and collapse;
  - group-aware navigation;
  - GameNode status sort;
  - organization inputs;
  - account library sync, partial read and refresh;
  - undo, batch status and add-to-collection runs;
  - suggestion scoring;
  - catalog search staleness;
  - API parsing (`id`, pagination, delete 204/404);
  - migration 0010 on a 0009 database;
  - store round-trips.
- **Vitest**
  - primitives;
  - Sidebar;
  - TopBar and WindowControls (mocked `@tauri-apps/api/window`);
  - StatusBar;
  - Toolbar;
  - FilterPopover (counts, search, chips);
  - Sort and Group menus;
  - Grid and List (group headers, badges, checkbox, header sort);
  - BatchBar;
  - Detail variants;
  - ContextMenu sections;
  - Banners;
  - states;
  - notifications (auto-close, actions, persistence);
  - CommandPalette;
  - ShortcutsModal;
  - SaveViewModal;
  - CatalogView;
  - i18n key parity.
- No performance tests or benchmarks.

## Risks / Trade-offs

- **[Snap Layouts lost on Windows 11 with `decorations(false)`]** → Accepted by the user. Listed in native acceptance. A later change can add `WM_NCHITTEST` HTMAXBUTTON handling if needed.
- **[Account library read is heavy for very large libraries (thousands of entries)]** → It is paginated at 100 per page, runs only on verification and rescan, never blocks the UI, and the cap fails safe without replacing the cache.
- **[Undo after the user already changed the game elsewhere]** → Undo deletes by the entry id it created. If the user re-added or edited it on the web in the meantime, the id is the same entry and it is deleted. This is acceptable within the notification's lifetime of about 6 s plus hover.
- **[`LibraryFilters` losing `Copy` touches many call sites]** → This is a mechanical change. The compiler finds every site.
- **[Faceted counts computed on every view build]** → They are bounded by facets × rows and computed only when dirty, using precomputed `RowFacts`. Correctness is covered by tests; no benchmarks, per project policy.
- **[Old layouts with `{provider}` and legacy filters]** → Explicit `normalized()` migrations, each with a unit test.
- **[Saved views referencing a store or collection that no longer exists]** → Applying such a view keeps the unknown ids. They match nothing and show as chips the user can remove. The view's count reflects this.
- **[Status update on a game whose cached status is unknown to this build]** → It is skipped with a reason rather than overwriting the server value.
- **[Frameless window on Linux compositors]** → Functional but not polished. Linux is post-MVP.

## Migration Plan

- **Database.** Migration `0010_library_organization.sql` is additive only. The existing `apply each migration once` logic applies. A downgrade to an older build rejects the newer schema, which is the existing behavior.
- **Layout JSON.** Old keys are read and folded by `normalized()`. New saves write only new keys. An old build reading a new layout ignores unknown fields; serde defaults apply on that side too, so the layout falls back to defaults where it cannot parse.
- **Rollout.** One change, delivered in the task order in `tasks.md`. Each phase compiles and passes tests on its own.
- **Rollback.** Revert the change. The new tables are unused by older builds, and the migration-version check prevents an older build from opening the new schema. This is the existing, documented behavior.

## Open Questions

- Exact copy for a few Figma-less strings, such as catalog errors and the undo results. English and pt-BR proposals are in `copy.md`; wording can be tuned during review without affecting behavior.
