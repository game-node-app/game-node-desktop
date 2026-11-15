# Proposal

## Why

The desktop library was built feature by feature, so its chrome, toolbar, filters and detail pane look and behave inconsistently. Some jobs users now expect cannot be done at all: grouping, saved views, hiding games, batch status changes, undoing an add, and seeing their GameNode status without opening every game. The Version 3.0 redesign of section 1 · Biblioteca (Figma `ikcThguwowTdynGz0L2YkS`, frames `5361:766`, components `5357:677`, Tabler icons `5356:678`) defines the whole library surface. This change delivers all of it in one pass: the redesigned existing parts and every new item. The visual specs are extracted into `design-reference/`, so implementers do not need Figma.

## What Changes

- **Shell and window chrome**
  - A frameless window replaces the native title bar.
  - New 48 px top bar: wordmark, DESKTOP badge, command-search trigger (Ctrl+K), GameNode sync status, account menu, and minimize/maximize/close controls.
  - Native window chrome is replaced by custom chrome (**BREAKING** for the OS look: Windows Snap Layouts on hover over maximize is lost).
  - New 26 px status bar: store health, last watched-folder check, and the running game with its session timer.
  - The redesigned app menu adds Keyboard shortcuts.
- **Sidebar**
  - Redesigned into four groups:
    - Seu espaço: All, Installed, Favorites, Recently played, Running.
    - Lojas: Steam, Epic, DRM-free, with per-store attention icons.
    - Coleções e visões: GameNode collections and local saved views.
    - Navegar: Discovered games, Explore GameNode, PC ports.
  - Footer: Add DRM-free game and Settings.
- **Header and toolbar**
  - New header with title, live subtitle, Rescan and Add to GameNode.
  - New toolbar: search, Filters popover (badge with the active count), Sort menu, Group menu, grid/list segmented control and details toggle.
  - Active-filter chips with Clear and "Save as view".
- **Filters**
  - The three-column filter popover adds facets: store, GameNode status, separate playtime and history facets, more last-session and date-added windows, release decades shown only when they have counts, and Show hidden.
  - Every option shows a faceted count.
- **Grouping**
  - Group by store, GameNode status, match, installation or release decade.
  - Groups have collapsible sticky headers, and their collapsed state is remembered.
- **List view**
  - New columns: GameNode status, match, playtime, last session and favorite.
  - Sortable headers, a checkbox column and a sort by GameNode status.
- **Grid tiles**
  - Fixed 172×270 tiles with installed, running, family, favorite and hidden badges.
  - Hover play affordance and a brand selection ring.
- **Selection and batch actions**
  - Multi-select with a floating batch bar: Add to GameNode, Change status, Favorite all, Match, Clear.
  - A multi-select detail panel.
  - Favoriting several games becomes possible.
- **Detail panel**
  - Redesigned with hero, action row, pills and stat cards.
  - Sections: On GameNode (status select, collections), Match, Achievements, On this PC, and Game tools.
  - New variants: DRM-free running, unmatched with match suggestions and confidence, family-shared not installed, and multi-select.
- **GameNode account library state** (new)
  - The account's library entries (status, collections) are read and cached per account, so status columns, filters, groups, collection scopes and the detail pane work offline.
  - Games that exist only in the account library are still not listed as library rows.
- **Organization** (new)
  - Saved views: named filter, sort, group and view combinations, kept per account in the sidebar.
  - Hiding games from the library: per account, revealed with Show hidden.
  - Adding to a collection from the context menu.
  - Changing GameNode status in batch.
- **Undo add** (new): the add notification offers Undo, which deletes exactly the entries that add created.
- **Match suggestions** (new): unmatched or ambiguous entries show up to two scored suggestions with a one-click Use.
- **Catalog view** (new): "Explore GameNode" searches the GameNode catalog inside the app and adds results.
- **Command palette and shortcuts dialog** (new)
  - Ctrl+K palette over games, actions and settings.
  - A shortcuts dialog opened with `?`.
  - Shortcuts: Ctrl+L, Ctrl+I, Ctrl+Shift+F, F, Ctrl+A, Shift+S and Esc.
- **States**
  - Redesigned first-run checklist, no-results, scanning panel with per-store progress, and four banner tones (session expired, unreachable, offline, discovered).
  - Notifications are rebuilt on Mantine `Notification`, with actions and progress.
- **Design system**
  - New tokens (colors, radii, type scale, shadows, sizes).
  - A shared primitive set in `ui/src/components/ui/`, using Tabler icons only.
  - Top bar 54 → 48 px, default sidebar 248 → 240 px, default detail pane 340 → 360 px.
- **Strings**: every new string is translated into English and Brazilian Portuguese.

## Capabilities

### New Capabilities
- `desktop-library-organization`: user-defined organization of the local library: grouping, saved views, hidden games, and their sidebar presence.
- `desktop-gamenode-catalog`: the in-app GameNode catalog search view, used to find and add games that are not on this PC.

### Modified Capabilities
- `desktop-library`:
  - sidebar scopes and store shortcuts;
  - expanded facets with faceted counts;
  - GameNode status sort and list columns;
  - grid badges;
  - batch bar and multi-select actions;
  - redesigned detail pane and its variants;
  - redesigned states and banners;
  - remembered layout including grouping;
  - scanning progress per store.
- `desktop-shell`:
  - foundation shell with frameless chrome, top bar, status bar and app menu;
  - the theme requirement now targets the redesign references;
  - keyboard access with global shortcuts;
  - new command palette, shortcuts dialog and notification requirements.
- `desktop-account-library`:
  - account library entries are cached and shown for local games;
  - membership is refreshed on selection;
  - undo of an add;
  - batch status change;
  - add to collection;
  - opening entries on the GameNode website.
- `desktop-favorites`: favoriting and unfavoriting several selected games.
- `desktop-game-matching`: scored match suggestions in the detail pane.
- `desktop-local-state`: persisting saved views, hidden games and the account library cache per account (migration 0010).
- `desktop-sync`: reading the account library, and the user-initiated undo, status and collection updates, are allowed while sync is off.

## Impact

- **Rust core** (`crates/core/src/library/`)
  - `layout.rs`: scope, filters, group, collapsed groups.
  - `query.rs`: facets, counts, groups, GameNode status sort.
  - `machine.rs`: new inputs and effects.
  - `view.rs`: groups, filter counts, catalog, status bar data.
  - `add_run.rs`: created entry ids, undo.
  - New `organization` module (saved views, hidden).
  - New account-library cache logic and match-suggestion scoring.
- **Rust API** (`crates/core/src/game_api.rs`, `crates/api/src/game.rs`)
  - `LibraryEntry.id`.
  - New `delete_entry` (`DELETE /v1/collections-entries/:id`) and `list_library_entries` (`GET /v1/collections-entries/library/:userId`, all categories).
  - `DEFAULT_WEB_URL` and `GAMENODE_WEB_URL`.
- **Rust platform**
  - Migration `0010_library_organization.sql`: `saved_views`, `hidden_entries`, `account_library_entries`.
  - Store methods in `library_db.rs`.
  - Discovery last-check timestamp.
- **Rust app**
  - `main.rs`: `decorations(false)`.
  - `capabilities/default.json`: window permissions.
  - Engine effect handlers.
  - New `open_gamenode_page` command.
  - Per-store scan progress.
- **UI** (`ui/src/`)
  - New `components/ui/` primitives.
  - Rebuilt `features/shell/` (TopBar, Sidebar, StatusBar, AppMenu, WindowControls, CommandPalette, ShortcutsModal).
  - Rebuilt `features/library/` (Header, Toolbar, FilterPopover, SortMenu, GroupMenu, Grid, List, GroupHeader, BatchBar, DetailPane variants, states, banners).
  - New `features/catalog/`.
  - `ToastContainer` on Mantine `Notification`.
  - `uiStore`: destinations, palette, shortcuts, toasts.
  - Tokens in `styles/theme.css` and `tailwind.config.ts`.
  - i18n catalogs.
  - The mock host mirrors every new input and view.
- **Dependencies**: no new packages. Uses the existing `@mantine/core`, `@tabler/icons-react`, `@tanstack/*` and `@tauri-apps/api/window`.
- **Native acceptance** (manual, Windows): frameless drag, double-click maximize, resize edges, close-to-tray through the custom close button, and DPI scaling of the 48 px bar.
- **Out of scope**: the PC ports screen keeps its current content (only its sidebar entry is restyled); Settings internals are unchanged.
