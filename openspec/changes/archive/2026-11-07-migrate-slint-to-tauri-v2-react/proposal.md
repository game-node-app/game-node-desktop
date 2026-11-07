# Proposal

## Why

In the current Slint-based desktop application, UI callbacks run synchronously on the main event loop, `slint::Image` is `!Send` (forcing bespoke worker threads, channels, and 16ms polling timers), every decoded cover triggers a full `Snapshot` re-render across all rows, and nested grid virtualization in Slint's `ListView` degrades scrolling and responsiveness. AI coding agents repeatedly introduce main-thread blocking work or unbounded in-memory texture caches (causing 600MB–1.3GB RAM spikes when opening game covers/artwork). Migrating with a hard cutover to **Tauri v2 + React + TanStack** eliminates these footguns by architecture: every `#[tauri::command] async fn` executes on Tokio's background pool, local covers and artwork stream directly to the WebView compositor via a custom `gamenode-asset://` protocol (`<img loading="lazy" decoding="async" />`) with native C++/GPU LRU caching, and `@tanstack/react-virtual` + `@tanstack/react-query` guarantee constant-DOM 120Hz scrolling and declarative state synchronization.

## What Changes

- **BREAKING (Internal UI Stack Cutover) — Purge Slint, Rust UI Glue, and the Slint Headless Studio**:
  - Delete all `crates/app/ui/*.slint` files, `crates/app/src/library_ui.rs`, `crates/app/src/library_ui/`, `crates/app/src/ui_commands.rs`, `crates/app/examples/ui-review.rs`, `scripts/dev-screen.sh`, `crates/app/tests/*.rs`, and gettext `.po` files (`i18n/`).
  - Remove `slint`, `slint-build`, and `image` from `crates/app/Cargo.toml` and `crates/app/build.rs`.
  - Remove Slint's Skia-to-software renderer fallback process restart logic (`--gamenode-internal-software-fallback`, `--gamenode-internal-gpu-error`) and `RendererPreference` (`Gpu` / `Software`) from `gamenode-core` and the Settings UI (preserving SQLite backwards compatibility by ignoring legacy `renderer` rows).
- **Preserve and Reuse Rust Core, Platform, API, and Engine Orchestration**:
  - Keep `gamenode-core`, `gamenode-platform`, `gamenode-api`, and `crates/app/src/engine/*` intact (authentication, single-instance IPC, `app.gamenode://` deep links, provider manifests and scanners, SQLite migrations `0001`..`0005`, process tracking, Ludusavi save backups/restore, and OptiScaler Game Tools).
  - Add `#[derive(Serialize, Deserialize)]` (`#[serde(rename_all = "camelCase")]`) to `gamenode-core` snapshot, view, and input types so Tauri commands and events serialize them directly without manual mapping glue.
- **Stand Up Tauri v2 Host, Native System Tray, and `gamenode-asset://` Protocol (`crates/app`)**:
  - Replace the Slint window and `ui/tray.slint` with a Tauri v2 host (`tauri.conf.json`, strict CSP capabilities, `TrayIconBuilder` on Windows, close-to-tray lifecycle, and `WindowGeometry` restoration).
  - Implement a custom `gamenode-asset://` URI scheme protocol serving cached covers (`gamenode-asset://localhost/cover/<file>`) and game landscape artwork (`gamenode-asset://localhost/artwork/<game_id>`) directly from disk/cache with path-traversal protection and immutable HTTP cache headers—never base64-encoding or decoding bitmaps on the Rust UI thread.
  - Expose all `Engine` actions as non-blocking `#[tauri::command] async fn` handlers in `crates/app/src/commands.rs` and publish coalesced `engine:snapshot` Tauri events whenever `Engine` state changes.
- **Build the React + TypeScript + TanStack Frontend (`ui/`) with Full Functional Parity**:
  - **TanStack Query (`@tanstack/react-query`)**: Manage `Snapshot` queries and command mutations, synchronized in real time via a single `listen('engine:snapshot')` cache updater.
  - **TanStack Virtual (`@tanstack/react-virtual`)**: Virtualize both `GameGrid` (responsive multi-column rows computed via `ResizeObserver`) and `GameList` (`48px` rows), keeping DOM node counts bounded regardless of library size.
  - **TanStack Store (`@tanstack/react-store`)**: Manage transient UI-only state (live sidebar/detail resize drag widths, context menu coordinates, active settings topic, and unsaved form dirty state).
  - **1:1 Visual, Keyboard, and Feature Parity**: Implement the GameNode Version 3.0 and Settings (`4987:10202`) design tokens (`theme-provenance.json`, bundled `Roboto-Variable.ttf`, SVG icons), English and Brazilian Portuguese (`en` / `pt-BR`) typed translation catalogs, full keyboard navigation (`Arrow*`, `Home`, `End`, `Enter`, `Ctrl+Enter`, `Shift+F10`/`ContextMenu`, `Ctrl+,`, `Esc` focus traps), `MatchDialog`, `AddDialog`, DRM-free discovery/editor/save-protection/restore modals, `GameToolsModal` (OptiScaler), and `SettingsModal`.
- **Update Packaging, Verification, and Contributor Guardrails**:
  - Update Windows and Linux build/packaging scripts (`scripts/*`, `packaging/*`) to bundle the Vite frontend dist into the Tauri binary.
  - Replace the Slint headless studio workflow in `AGENTS.md` and `docs/development.md` with Vite + Vitest + React Testing Library + T3 Code `preview_*` on port `4777` and strict architectural guardrails preventing UI-thread work.

## Capabilities

### New Capabilities

<!-- None: all product capabilities already exist in openspec/specs/; this change modifies their UI/runtime requirements and removes Slint-specific renderer settings. -->

### Modified Capabilities

- `desktop-shell`: Replace the Slint window and Slint-based renderer selection (`Gpu`/`Software` fallback) with the Tauri v2 native window shell, CSS/WebView theme token system from `theme-provenance.json`, and typed `en`/`pt-BR` frontend/tray localization while preserving all navigation, window geometry, keyboard access, and About/license disclosures.
- `desktop-library`: Require virtualized Grid and List rendering (`@tanstack/react-virtual`) and asynchronous off-thread cover/artwork loading via the custom `gamenode-asset://` protocol (`<img loading="lazy" decoding="async" />`) so scrolling and selection never decode images on the UI thread or retain unbounded bitmap caches.
- `desktop-settings`: Remove the Slint-only `Avançado` (`Advanced`) software-rendering toggle panel while retaining all other Settings topics (`Conta`, `Lojas`, `Backups`, `Ferramentas` / `Tools`, `Sobre`), modal focus trapping, dirty-edit confirmation, and toast notifications in the React + TanStack frontend.
- `desktop-local-state`: Stop persisting or requiring the Slint `renderer` preference in active `Settings` while maintaining backwards-compatible SQLite loading for existing databases that still have the `renderer` column in `foundation_settings`.

## Impact

- **Removed**: `crates/app/ui/*.slint`, `crates/app/src/library_ui*`, `crates/app/src/ui_commands.rs`, `crates/app/examples/ui-review.rs`, `scripts/dev-screen.sh`, `crates/app/tests/*.rs`, `i18n/**/*.po`, and Cargo dependencies `slint`, `slint-build`, `image`.
- **Modified (`crates/core`)**: `#[derive(Serialize, Deserialize)]` added to view/snapshot/input types; `RendererPreference` removed from `Settings` (or ignored in SQLite).
- **Modified (`crates/app`)**: Rewritten around `tauri` v2 (`src/main.rs`, `src/commands.rs`, `src/protocol.rs`, `tauri.conf.json`, `capabilities/default.json`), keeping `src/engine/*` and `src/providers.rs`.
- **Added (`ui/`)**: New Vite + React 19 + TypeScript + `@tanstack/react-query` + `@tanstack/react-virtual` + `@tanstack/react-store` frontend workspace with Vitest component/integration tests.
- **Modified (`scripts/`, `docs/`, `AGENTS.md`)**: Build/packaging scripts compile `ui/` before Cargo packaging; contributor docs enforce Tauri v2 + TanStack patterns.
