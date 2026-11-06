# Proposal

## Why

Watched-folder DRM-free game discovery currently hides newly detected games inside a manual checklist at the bottom of `Settings → Stores`, forcing users to open Settings just to see or approve games that already met strict recognition criteria. At the same time, library items cannot be launched by double-clicking, DRM-free games still display a permanently disabled `Install` button in the detail pane and context menu, and the per-game `Local save protection` dialog uses unstyled controls without showing which folder or files on disk are actually being backed up.

## What Changes

- **Auto-add recognized DRM-free games from watched folders and surface discoveries in the Library**:
  - When a watched folder scan finds a strongly identified game installation, automatically select a preferred launch executable (when multiple executables exist in the folder, preferring manifest launch hints, non-launcher binaries, and 64-bit targets) and register the game in the library by default.
  - Run GameNode catalog matching and save-profile resolution automatically in the background for newly discovered entries.
  - Show a dismissible banner in the main Library view announcing newly added DRM-free games from watched folders so discoveries and matches are immediately visible without opening Settings.
  - Keep watched-folder management (`Watch folder`, `Remove watch`, `Rescan watched folders`, and `Reset ignored discoveries`) in `Settings → Stores`, removing the hidden manual candidate checklist from Settings. Removing a DRM-free game from the library continues to suppress rediscovery until reset.
  - Expose alternative executables detected in a game's installation folder within `Game settings` (`LocalGameEditor`) so users can switch the launch target with one click if the auto-selected executable is not the desired one.
- **Double-click and Enter to launch in the Library**:
  - Change double-clicking a `GameTile` (grid view) or `GameRow` (list view) — or pressing `Enter` on a focused/selected library item — to select the entry and trigger `Play` immediately when launch is available, instead of only opening the detail pane (`Ctrl+Enter` opens the detail pane without launching). If `Play` is unavailable (for example, the game is already running or its executable is missing), select the entry and open the detail pane so the reason is visible.
- **Remove the `Install` action for DRM-free games**:
  - Hide the `Install` button and its `"This store has no install action."` caption in the detail pane, as well as the `Install` item in the context menu, whenever a DRM-free (`manual`) game is selected.
- **Redesign the per-game Local Backups (`SaveProtection`) modal**:
  - Restyle `SaveProtection` using the card/section modal visual language (`SettingsStyle` / `SettingsCard` / `SettingsRow` / `SettingsToggle` / `SettingsButton` / `Pill` / `InfoBox`) with a fixed header, `Esc` badge, close button, and scrollable body.
  - Clearly display **what is being backed up**: the active save profile title, the exact save folders/files and Windows registry keys resolved for the installation, and the local backup archive destination path.
  - Automatically load backup history when the modal opens (removing the manual `Load backup history` step), format backup sizes in human-readable units (`KB` / `MB`), and display each snapshot as a structured row with timestamp, size, and a `Restore…` action.
  - Improve the restore preview and custom save-location / profile override layout so routine backup status and history are front-and-center while profile/path overrides live in a clean configuration section.

## Capabilities

### New Capabilities

<!-- None -->

### Modified Capabilities

- `desktop-library`: Double-clicking a library tile or row launches the game when playable; DRM-free entries omit the `Install` button and context-menu item; newly auto-added DRM-free games from watched folders are announced in the library view; per-game local save protection modal shows resolved save paths, auto-loaded history, and structured restore controls.

## Impact

- **UI (Slint)**:
  - `crates/app/ui/library.slint`: Wire `GameGrid` and `GameList` `activated` callbacks to launch playable games on double-click; hide `Install` in `DetailPane` and `ContextMenuArea` for DRM-free entries; add a dismissible discovery notice banner for newly added watched-folder games; auto-load history when opening `Local save protection`.
  - `crates/app/ui/drm-free.slint`: Redesign `SaveProtection` with structured cards, visible save folders/files/registry targets, backup destination, human-readable snapshot history, and restore confirmation; add detected executable target switching in `LocalGameEditor`.
  - `crates/app/ui/settings.slint`: Keep watched-folder configuration and `Reset ignored discoveries` in `Stores` while removing the manual candidate checklist.
- **App & Core (Rust)**:
  - `crates/platform/src/discovery.rs` and `crates/core/src/drm_free/identity.rs`: Deterministic preferred executable target selection when an installation folder contains multiple candidate executables.
  - `crates/app/src/engine/drm_free.rs`: Auto-register discovered watched-folder candidates that are not ignored or tombstoned, record newly added titles for the library banner, and trigger background GameNode matching and save-profile preview.
  - `crates/app/src/library_ui.rs`: Expose active save profile title, resolved save file/folder paths, registry keys, formatted backup history rows, alternative executable targets, and newly discovered game banner state to Slint.
- **i18n & Preview Tooling**:
  - Update English (`i18n/en`) and Brazilian Portuguese (`i18n/pt_BR`) translations and `ui-review` presets (`protection`, `restore`, `discovery`, `drm-detail`).
