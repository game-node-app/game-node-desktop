# GameNode Desktop: Tauri v2 + React + TanStack migration plan

Started 2026-11-06. This plan replaces the Slint UI layer, its Rust glue, and the Slint headless studio with a
**Tauri v2** shell and a **React + TypeScript + TanStack** frontend, while preserving the Rust domain and OS crates
(`gamenode-core`, `gamenode-platform`, `gamenode-api`) and the engine orchestration (`crates/app/src/engine`).

**Hard cutover rule:** There is no dual-stack period. Slint (`*.slint`), `slint-build`, the Slint Rust glue
(`crates/app/src/library_ui*`, `crates/app/src/ui_commands.rs`), the Slint headless studio
(`crates/app/examples/ui-review.rs`, `scripts/dev-screen.sh`), and the Slint software/Skia fallback machinery are
**deleted** in Step 1. Every step below builds exclusively on Tauri v2 and React.

---

## Why we are migrating (and how this architecture prevents agent footguns)

Under Slint, UI callbacks ran synchronously on the event loop, `slint::Image` was `!Send` (requiring custom worker
threads, channels, and 16ms polling timers), every cover decode triggered a full `Snapshot` re-render across all rows,
and nested grid virtualization inside Slint's `ListView` degraded scrolling. AI agents repeatedly placed work on the UI
thread or grew unbounded texture caches in RAM.

Tauri v2 + React + TanStack eliminates these failure modes by construction:

- **No Rust code ever runs on the UI thread.** Every `#[tauri::command] async fn` executes on Tokio's background thread
  pool in the Rust host process. The WebView runs in its own OS process (WebView2 on Windows, WebKitGTK on Linux) with a
  dedicated GPU compositor thread for 120Hz scrolling.
- **Zero manual image decoding or texture caches in Rust or JS.** Covers and artwork are served directly from disk
  through a custom Tauri asset protocol (`gamenode-asset://covers/<file>` and `gamenode-asset://artwork/<game_id>`) and
  rendered with `<img loading="lazy" decoding="async" />`. The browser engine's native C++ compositor decodes off-thread,
  caps GPU texture memory with a hardware LRU cache, and evicts off-screen tiles automatically.
- **True 2D/row virtualization.** `@tanstack/react-virtual` mounts only the DOM nodes inside the viewport (plus a small
  overscan), whether the library has 50 or 10,000 games.
- **Declarative server/engine state.** `@tanstack/react-query` owns all async queries, mutations, and cache invalidation
  driven by Tauri backend events (`engine://snapshot-updated`). No imperative 1,700-line diffing glue.

---

## What is deleted, what is kept, and what is refactored

### Deleted outright (zero remnants)

| Area | Files / Dependencies removed |
| --- | --- |
| **Slint UI files** | `crates/app/ui/*.slint` (`app.slint`, `library.slint`, `library-dialogs.slint`, `library-state.slint`, `library-text.slint`, `drm-free.slint`, `tools.slint`, `settings.slint`, `settings-state.slint`, `settings-style.slint`, `settings-controls.slint`, `playtime-ui.slint`, `playtime-text.slint`, `controls.slint`, `theme.slint`, `tray.slint`) |
| **Slint Rust glue & image workers** | `crates/app/src/library_ui.rs`, `crates/app/src/library_ui/artwork.rs`, `crates/app/src/library_ui/tests.rs`, `crates/app/src/ui_commands.rs` |
| **Slint headless studio & tests** | `crates/app/examples/ui-review.rs`, `scripts/dev-screen.sh`, `crates/app/tests/discovery_screen.rs`, `crates/app/tests/library_screen.rs`, `crates/app/tests/library_pt_br.rs`, `crates/app/tests/i18n_catalogs.rs` |
| **Slint renderer fallback machinery** | All `--gamenode-internal-software-fallback`, `--gamenode-internal-gpu-error`, Skia/software restart logic in `crates/app/src/main.rs`, and `RendererPreference` (`Gpu` / `Software`) in `gamenode-core::settings` |
| **Gettext `.po` catalogs** | `i18n/en/LC_MESSAGES/gamenode-app.po`, `i18n/pt_BR/LC_MESSAGES/gamenode-app.po` (replaced by typed JSON/TS translation catalogs consumed by the frontend and tray) |
| **Cargo dependencies** | `slint`, `slint-build`, `image` (in `crates/app`, since WebView decodes JPEG/PNG natively in C++/GPU) |

### Kept 100% intact (reused as-is)

| Area | What stays |
| --- | --- |
| **`crates/core`** | `auth`, `callback`, `credential`, `drm_free`, `game_api`, `geometry`, `library` (state machine, matching, filters, sort, selection), `notice`, `playtime`, `ports`, `provider` (TOML manifests, Steam/generic scanners), `sync`, `tools` (OptiScaler domain), `tracker`. *(Only minor addition: `#[derive(Serialize, Deserialize)]` on view/input structs so Tauri IPC serializes them directly.)* |
| **`crates/platform`** | SQLite stores & migrations (`0001`..`0005`), Windows/Linux process tracking, Ludusavi backup runner (`backups.rs`), PE/executable inspection (`installations.rs`), directory watcher (`discovery.rs`), Credential Manager / Secret Service (`credentials/`), registry, autostart, URI opener, OptiScaler extractor/installer (`tools/`). |
| **`crates/api`** | `GameNodeClient` HTTP client, auth endpoints, external game matching, collections, and playtime reporting. |
| **`crates/app/src/engine/*`** | The `Engine` state coordinator (`engine/mod.rs`, `engine/artwork.rs`, `engine/drm_free.rs`, `engine/playtime.rs`, `engine/tools.rs`) and `StorageWorker`. Instead of calling `slint::invoke_from_event_loop`, its `publish` callback emits a Tauri event (`app_handle.emit("engine:updated", ...)`). |
| **Assets & manifests** | `providers/*.toml`, `licenses/*`, `packaging/*`, and brand assets (`gamenode.ico`, `gamenode.png`, `gamenode-logo.svg`, `Roboto-Variable.ttf`, SVG icons moved to `ui/src/assets/`). |

---

## Target Stack & Architecture

### Rust Host (`crates/app` as Tauri v2 Core)

- **`tauri` v2** (`tray-icon`, `protocol-asset` / custom URI scheme `gamenode-asset://`).
- **Native OS System Tray** via `tauri::tray::TrayIconBuilder` (replaces `ui/tray.slint`), with "Open GameNode" and "Quit".
- **Single-instance & Deep-link (`app.gamenode://`)**: keeps `gamenode_platform::ipc` (or integrates with Tauri's window lifecycle) so launching a second instance or receiving an OAuth callback focuses the window and dispatches `Input::CallbackReceived(url)`.
- **Window geometry & close-to-tray**: uses `gamenode_core::geometry::plan_window` on startup, persists `WindowGeometry` on move/resize, and intercepts `WindowEvent::CloseRequested` on Windows to hide the window to the tray instead of exiting.
- **Custom Asset Protocol (`gamenode-asset://`)**:
  - `gamenode-asset://localhost/cover/<file>` streams `<covers_dir>/<file>` with `Cache-Control: max-age=31536000, immutable`.
  - `gamenode-asset://localhost/artwork/<game_id>` serves the active game's landscape artwork bytes from `Engine` memory/cache without encoding base64 over JSON IPC.

### Frontend (`ui/` at repository root)

- **Vite + React 19 + TypeScript (strict)**.
- **TanStack Query (`@tanstack/react-query`)**:
  - Queries wrap `invoke()` calls (`useQuery({ queryKey: ['snapshot'], queryFn: () => invoke('get_snapshot') })` or granular slices: `['library']`, `['local-games']`, `['tools']`, `['settings']`).
  - A single top-level Tauri event listener (`listen('engine:snapshot', ...)`) updates the QueryClient cache via `queryClient.setQueryData` or invalidates active queries—giving instant reactive UI updates with zero polling.
  - Mutations (`useMutation`) wrap every user action (`invoke('library_input', ...)`, `invoke('launch_entry', ...)`).
- **TanStack Virtual (`@tanstack/react-virtual`)**:
  - Powers both `GameGrid` (virtualized rows of responsive CSS Grid columns calculated via `ResizeObserver`) and `GameList` (virtualized 48px table rows).
  - Guarantees constant DOM size (~20-40 nodes) regardless of library size.
- **TanStack Router (`@tanstack/react-router`)** (in-memory history mode for desktop) or **TanStack Store (`@tanstack/react-store`)**:
  - Manages transient UI-only state (active settings tab, context menu coordinates, live drag widths before commit) cleanly outside the backend snapshot.
- **Styling & Design Tokens**:
  - CSS variables generated directly from `theme-provenance.json` (`--color-primary: #F15025`, `--color-background: #161616`, `--color-surface: #262525`, `--color-chrome: #111111`, spacing, radii, and `Roboto-Variable.ttf`) paired with Tailwind CSS v4 (or CSS modules) so the look matches the GameNode desktop design 1:1.
- **Internationalization (`en` and `pt-BR`)**:
  - Typed translation dictionaries (`ui/src/i18n/en.ts`, `ui/src/i18n/pt-BR.ts`) migrated from `i18n/*/LC_MESSAGES/gamenode-app.po` with compile-time key checking.

### Repository Layout After Migration

```text
Cargo.toml                  workspace (core, platform, api, app)
crates/
  core/                     pure domain & state machines (serde-enabled view/input types)
  platform/                 OS adapters, SQLite, processes, Ludusavi, OptiScaler installer
  api/                      GameNode HTTP client
  app/                      Tauri v2 host binary, tray, asset protocol, #[tauri::command] handlers
    tauri.conf.json
    capabilities/
    src/
      main.rs               Tauri builder, single-instance IPC, window geometry, tray, lifecycle
      commands.rs           async #[tauri::command] endpoints calling Engine
      protocol.rs           gamenode-asset:// streaming for covers & artwork
      engine/               existing Engine orchestration (untouched logic, emits Tauri events)
ui/                         React + TypeScript + Vite + TanStack frontend
  package.json
  vite.config.ts
  src/
    api/                    typed invoke() wrappers & TanStack Query hooks/keys
    components/             Action, Panel, Cover, ResizeHandle, ContextMenu, Toast
    features/
      shell/                TopBar, Sidebar, AppMenu, StatusBanner, SyncExplanationModal
      auth/                 SignedOutView, PendingSignInView, RestoreFailedView
      library/              LibraryScreen, GameGrid (TanStack Virtual), GameList, DetailPane
      dialogs/              MatchDialog, AddDialog
      drm-free/             LocalGameEditor, SaveProtectionModal, DiscoveryModal, RemoveDialog, ClaimDialog
      tools/                GameToolsModal (OptiScaler version/channel/proxy/anticheat UI)
      settings/             SettingsModal (Account, Library & Backups, Tools, About & Licenses)
    i18n/                   en.ts, pt-BR.ts
    styles/                 theme.css (from theme-provenance.json)
providers/                  bundled provider manifests (*.toml)
licenses/
docs/
```

---

## Migration Order

```text
1 Purge Slint & Bootstrap Tauri v2 ──► 2 Core Serialization & IPC Bridge ──► 3 App Shell, Auth & Settings
                                                                                     │
6 Verification, Packaging & Cleanup ◄── 5 DRM-Free, Backups & Game Tools ◄── 4 Virtualized Library & Matching
```

Each step leaves `cargo check --workspace` and the frontend TypeScript build green.

---

## Step 1: Purge Slint and bootstrap Tauri v2 + React + TanStack

**Goal:** Delete every trace of Slint and its studio, and stand up a compiling Tauri v2 + Vite + React + TanStack workspace with GameNode's design tokens and fonts.

- **Delete Slint & Studio:**
  - Remove `crates/app/ui/*.slint` (keep `crates/app/ui/fonts/Roboto-Variable.ttf`, `crates/app/ui/icons/*`, and `crates/app/ui/theme-provenance.json`, moving them into `ui/src/assets/` and `crates/app/icons/`).
  - Delete `crates/app/src/library_ui.rs`, `crates/app/src/library_ui/`, `crates/app/src/ui_commands.rs`, `crates/app/examples/ui-review.rs`, `scripts/dev-screen.sh`, and `crates/app/tests/*.rs`.
  - Delete `i18n/` `.po` files after extracting all English and Brazilian Portuguese strings into `ui/src/i18n/{en,pt-BR}.ts`.
  - Remove `slint`, `slint-build`, and `image` from `crates/app/Cargo.toml` and `crates/app/build.rs`.
  - Remove `RendererPreference` (`Gpu` / `Software`) from `crates/core/src/settings.rs` (keep SQLite column compatibility by ignoring legacy `renderer` values in `SqliteSettingsStore`).
- **Bootstrap Tauri v2 in `crates/app`:**
  - Add `tauri` v2 (with `tray-icon`) and `tauri-build` to `crates/app/Cargo.toml`.
  - Create `crates/app/tauri.conf.json` and `crates/app/capabilities/default.json` with strict CSP (no remote scripts, only `self` and `gamenode-asset:`).
- **Bootstrap Frontend in `ui/`:**
  - Initialize Vite + React + TypeScript project with `@tauri-apps/api`, `@tanstack/react-query`, `@tanstack/react-virtual`, and `@tanstack/react-store`.
  - Port `theme-provenance.json` and `Roboto-Variable.ttf` into `ui/src/styles/theme.css`.
  - Configure Vite dev server on port `4777` so T3 Code's `preview_*` tools and standard browser DevTools work natively out of the box without any custom screenshot server.

**Done when:** `slint` does not appear anywhere in `Cargo.lock` or `Cargo.toml`, `pnpm --dir ui build` succeeds, and `cargo check --workspace --locked` compiles the Tauri v2 binary.

---

## Step 2: Core Serialization, Custom Asset Protocol, and TanStack Query Bridge

**Goal:** Expose `Engine` state and commands to the frontend through zero-blocking async Tauri commands, Tauri events, and the `gamenode-asset://` image protocol.

- **Serialize Core View & Input Types:**
  - Add `#[derive(Serialize, Deserialize)]` (with `#[serde(rename_all = "camelCase")]`) to view and command structs in `gamenode-core`: `Snapshot` (excluding raw `artwork: Option<Arc<[u8]>>` bytes from JSON, replacing it with `has_artwork: bool` + generation/cache key), `AuthView`, `LibraryView`, `Row`, `DetailView`, `MatchDialogView`, `AddDialogView`, `LocalView`, `ToolsView`, `Input`, and `library::Input`.
- **Custom Asset Protocol (`crates/app/src/protocol.rs`):**
  - Register `gamenode-asset://` in Tauri:
    - `gamenode-asset://localhost/cover/<file>` validates `valid_cover_file(file)` (no path traversal) and reads from `covers_dir` on a blocking thread pool, returning `image/jpeg` or `image/png` with immutable cache headers.
    - `gamenode-asset://localhost/artwork/<game_id>` returns the in-memory or cached landscape artwork bytes for the selected game.
- **Async Tauri Commands (`crates/app/src/commands.rs`):**
  - Replace `UiCommands` with `#[tauri::command] async fn` handlers backed by `tauri::State<'_, Engine>`:
    - Auth & App: `get_bootstrap_state`, `dispatch_auth`, `set_sync_enabled`, `set_playtime_source`, `dismiss_sync_explanation`, `update_window_geometry`, `quit_app`.
    - Library: `library_input`, `activate_entry`, `launch_selected`, `dismiss_launch`, `rescan_library`.
    - DRM-Free & Backups: `pick_local_executable`, `remove_entry`, `edit_local_game`, `backup_local_now`, `set_local_backup_settings`, `pick_save_destination`, `set_local_profile_automatic`, `configure_local_saves`, `pick_local_saves`, `choose_local_save_profile`, `watch_local_folder`, `pick_local_watch_folder`, `remove_local_watch`, `set_local_candidate_executable`, `rescan_local_folders`, `select_local_candidate`, `review_selected_local_candidates`, `reset_local_ignores`, `load_local_history`, `preview_local_restore`, `confirm_local_restore`, `cancel_local_restore`, `resolve_local_claim`, `cancel_local_claim`, `dismiss_local_discovered`.
    - Game Tools (OptiScaler): `open_tools_for_selected`, `close_tools`, `select_tool_channel`, `refresh_tool_versions`, `select_tool_version`, `select_tool_target_exe`, `select_tool_proxy`, `set_tool_anticheat_confirmed`, `set_tool_reset_config`, `install_or_update_tool`, `uninstall_tool`, `open_tool_source`, `clear_tool_cache`.
- **Event-Driven TanStack Query Synchronization (`ui/src/api/`):**
  - In `Engine::new`, wire the `publish` callback to emit `"engine:snapshot"` to the main WebView window (coalesced so rapid engine updates never flood IPC).
  - In `ui/src/api/useSnapshot.ts`, subscribe once via `listen('engine:snapshot', (event) => queryClient.setQueryData(['snapshot'], event.payload))` and expose selector hooks (`useAuth()`, `useLibraryView()`, `useLocalGames()`, `useTools()`, `useSettings()`) so React components only re-render when their selected slice changes.

**Done when:** Rust unit/integration tests verify command serialization and asset protocol path-traversal safety, and the frontend receives live `Snapshot` updates via TanStack Query.

---

## Step 3: Window Lifecycle, Tray, Shell, Auth, and Settings Parity

**Goal:** Achieve full parity with Steps 1 and 3 of the MVP roadmap (single-instance, deep-links, tray, background mode, sign-in/out, sync explanation, and Settings modal).

- **Tauri Host Lifecycle (`crates/app/src/main.rs`):**
  - Wire single-instance `ipc::acquire` and `ipc::forward_launch`: second launches focus/unhide the Tauri window and deliver `AuthCallback { url }` to `Engine`.
  - Apply saved `WindowGeometry` via `geometry::plan_window` before showing the window (unless `--background` was passed).
  - Build the native Windows system tray (`TrayIconBuilder`) with localized menu items ("Open GameNode", "Quit") and left-click to show/focus.
  - Intercept window close on Windows to save geometry, hide the window to tray, and keep the background tracker running.
  - Run a 1-second Tokio interval task on the Rust side to call `engine.dispatch(Input::Tick)` and `engine.maintenance()` (no UI timer needed!).
- **Shell & Auth Screens (`ui/src/features/shell/` & `ui/src/features/auth/`):**
  - `TopBar`: GameNode logo, `DESKTOP` badge, green online dot + user display name, and overlay `AppMenu` (Library, Rescan, Settings / About, Sign out, Quit).
  - `Sidebar`: resizable (`160px`..`360px` via `ResizeHandle` that updates local state at 60fps and commits `sidebar_width` on pointer-up), `All games` + per-provider filter counts and failure badges, `Settings` button (`Ctrl+,`), and version footer.
  - `SignedOutView`: centered welcome panel with Google, Discord, and Twitter sign-in buttons, busy `Cancel sign-in` state, `Retry connection` state, and localized status banner (`Notice` messages).
  - `SyncExplanationModal`: first-run explanation dialog on sign-in with quick link to Account settings.
- **Settings Modal (`ui/src/features/settings/`):**
  - Topics: Account & Sync (identity, session storage method, Sync with GameNode toggle, Playtime source toggle, Sign out), Library & Backups (automatic backups toggle, backup destination picker with dirty-state tracking, watched folders list, add/remove watch folder, reset ignored candidates), Game Tools (OptiScaler cache size, archive count, Clear cache), and About & Licenses (version, bundled license viewer with native smooth scrolling—no 2,400-char pagination hack required).
  - Toast notification system (`ToastKind::Success` / `Danger`) driven by state transitions.

**Done when:** The app starts in normal or `--background` mode, signs in via browser callback, minimizes to tray on close, switches between `en` and `pt-BR`, and all Settings tabs work end-to-end.

---

## Step 4: Virtualized Library (Grid & List), Detail Pane, Matching, and Add to GameNode

**Goal:** Achieve full parity with Step 2 of the MVP roadmap, with 120fps scrolling and zero-overhead cover rendering.

- **Toolbar (`ui/src/features/library/LibraryToolbar.tsx`):**
  - Search input (instant local state + debounced/direct `SetSearch`), Sort picker (`Title`, `Provider`, `MatchStatus` + ascending/descending toggle), View switcher (`Grid` / `List`), `Rescan` button (`F5`), and scanning/failed-stores/offline banners.
- **Virtualized Game Grid (`ui/src/features/library/GameGrid.tsx`):**
  - Uses `ResizeObserver` to compute `columns = Math.max(1, Math.floor((width - gap) / (tileW + gap)))` and notifies `Engine` (`SetColumns(columns)`) for accurate Up/Down arrow navigation.
  - Uses `useVirtualizer` from `@tanstack/react-virtual` over `Math.ceil(rows.length / columns)` rows (`294px` tile height + `12px` gap).
  - Each `GameTile` (`166x294px`) renders `<Cover />` using `<img src="gamenode-asset://localhost/cover/..." loading="lazy" decoding="async" />`, running indicator, match status dot (`Matched`, `Ambiguous`, `Unmatched`, `NoGame`), store name, and formatted playtime.
- **Virtualized Game List (`ui/src/features/library/GameList.tsx`):**
  - Uses `useVirtualizer` over `rows.length` (`48px` row height) with sticky header (`Title`, `Store`, `Match`) and thumbnail cover (`28x38px`).
- **Keyboard, Selection & Context Menu:**
  - Full keyboard parity: `ArrowUp`, `ArrowDown`, `ArrowLeft`, `ArrowRight`, `Home`, `End` (with `Shift` range extension), `Enter` (play or open details), `Ctrl+Enter` (open details), `ContextMenu` / `Shift+F10` (opens context menu at focused tile/row coordinates), and `virtualizer.scrollToIndex(focusedRow)` whenever `focusedIndex` changes.
  - Mouse click (`Ctrl` toggle, `Shift` range), double-click to activate, and right-click context menu (`Play`, `Install`, `Match game`, `Add to GameNode`, `Game tools`, `Edit game`, `Save protection`, `Remove`).
- **Resizable Detail Pane (`ui/src/features/library/DetailPane.tsx`):**
  - Landscape artwork (`gamenode-asset://localhost/artwork/<game_id>`), game title, detected/matched metadata, release year, install directory, GameNode standing badge (`InLibrary` status, `NotInLibrary`, `Checking`), `Play` / `Install` buttons with disabled-reason tooltips (`Why`) and launch error notes (`LaunchNote`), `Match` button, `Add to GameNode` / `Edit in GameNode` button, and `Game tools` summary.
- **Dialogs (`ui/src/features/dialogs/`):**
  - `MatchDialog`: candidate options, search box prefilled with detected name, search results with covers/years, `Pick match`, `Not a game`, and `Clear match`.
  - `AddDialog`: target games list, status selector (`Playing`, `Finished`, `Planned`, `Dropped`, `Ongoing`), user collections checklist (with collection default-status override badge), progress bar, and outcome summary (`Added`, `AlreadyPresent`, `Skipped`, `Failed`).

**Done when:** Scrolling a 1,000+ game library in Grid and List views stays locked at 60/120fps, memory usage stays flat when opening/scrolling covers and landscape artwork, and keyboard navigation, multi-select, matching, and Add to GameNode pass all scenarios.

---

## Step 5: DRM-Free Games, Local Save Backups, and Game Tools (OptiScaler)

**Goal:** Achieve full parity with Steps 4a and 7 of the MVP roadmap.

- **DRM-Free Management (`ui/src/features/drm-free/`):**
  - `Add game` executable picker action in sidebar/toolbar and `Locate executable` repair flow for missing binaries.
  - `LocalGameEditor` modal: edit title, executable, root folder, working directory, launch arguments, gameplay process patterns, and launcher process patterns; candidate executable selector when multiple `.exe` files exist.
  - `DiscoveryModal`: review queue for watched folders, candidate multi-select, target `.exe` dropdown per candidate, `Add selected`, `Ignore selected`, and rescan status notices.
  - `ClaimConflictModal`: prompt when a newly scanned store game matches an existing manual installation (transfer playtime/backups or keep separate).
  - `RemoveLocalGameModal`: confirm removal with checkbox to allow or suppress rediscovery in watched folders.
- **Save Protection & Restore (`ui/src/features/drm-free/SaveProtectionModal.tsx`):**
  - Displays matched Ludusavi save profile, detected save file paths and registry keys, overlapping-saves warning badge, per-game automatic backup toggle, custom save path picker, manual profile search/override, and `Back up now` button with live job status (`Queued`, `Running`, `Unchanged`, `Successful`, `Partial`, `Failed`).
  - Backup history table (up to 10 versions with timestamp, formatted byte size, destination) and two-step `Restore` confirmation modal previewing target paths to be overwritten.
- **Game Tools / OptiScaler Modal (`ui/src/features/tools/GameToolsModal.tsx`):**
  - Lists catalog tools (OptiScaler) with license (`GPL-3.0`) and source repository link.
  - Channel selector (`Stable` / `Nightly`), version dropdown (showing cached badge and publish date), target executable selector (with architecture and preferred badge), proxy DLL selector (`dxgi.dll`, `winmm.dll`, `version.dll`, etc.), and `Reset configuration` checkbox.
  - Anti-cheat warning banner (EasyAntiCheat / BattlEye detection + evidence list) with required confirmation checkbox before enabling Install/Update.
  - `Install`, `Update`, and `Uninstall` actions with live progress/error status.

**Done when:** Adding/editing/removing DRM-free games, reviewing watched-folder candidates, running/restoring Ludusavi backups, and installing/updating/uninstalling OptiScaler work with full functional parity.

---

## Step 6: Verification Workflow, Packaging, and Documentation Cleanup

**Goal:** Update test suites, Windows/Linux packaging scripts, and contributor/agent documentation so the repository reflects the Tauri v2 + React stack everywhere.

- **Frontend & Integration Testing (`ui/` + `crates/app`):**
  - Add Vitest + React Testing Library + Mock Tauri IPC (`@tauri-apps/api/mocks`) tests covering:
    - Library Grid/List rendering, keyboard navigation, multi-select, and filter/sort transitions.
    - DRM-Free discovery, editor, save protection, and restore flows.
    - Game Tools (OptiScaler) anti-cheat gating and install/uninstall states.
    - Complete `en` and `pt-BR` translation key parity check (replacing `i18n_catalogs.rs`).
- **Packaging Scripts (`scripts/` & `packaging/`):**
  - Update `scripts/build-windows-wsl.sh`, `scripts/install-windows-wsl.sh`, `scripts/package-windows-wsl.sh`, and `scripts/package-linux.sh` to build the Vite frontend (`pnpm --dir ui build`) before building the Rust binary (`cargo xwin build` / `cargo build`), embedding `ui/dist` into `gamenode.exe` / `gamenode`.
  - Update `licenses/THIRD-PARTY-NOTICES.txt` and `Cargo.toml` license comments (removing Slint licensing notes).
- **Agent Guardrails (`AGENTS.md`, `docs/development.md`, `docs/mvp-roadmap.md`):**
  - Update `AGENTS.md` with the new architecture rules:
    1. All backend operations must be `#[tauri::command] async fn` delegating to `Engine` or background tasks.
    2. All local images (covers, artwork) must use `gamenode-asset://` URLs with `<img loading="lazy" decoding="async" />`—never base64 or byte arrays over JSON IPC.
    3. All large lists/grids must use `@tanstack/react-virtual`.
    4. All backend state reads/mutations in React must go through `@tanstack/react-query` hooks in `ui/src/api/`.
  - Document the new live UI preview workflow (`pnpm --dir ui dev` with T3 Code's `preview_open` on port `4777`).
  - Update `docs/mvp-roadmap.md` and `docs/development.md` to replace Slint references with Tauri v2 + React + TanStack.

**Done when:**
- `pnpm --dir ui test`, `pnpm --dir ui build`, `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `scripts/package-windows-wsl.sh` all pass cleanly.
- Zero references to `slint` or `ui-review` remain in code, scripts, or active documentation.
