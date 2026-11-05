# Design

## Context

See `proposal.md` for motivation and `specs/desktop-library/spec.md` for behavioral requirements.

Current implementation state across the workspace:

- **Watched-folder discovery (`crates/platform/src/discovery.rs`, `crates/app/src/engine/drm_free.rs`, `crates/app/ui/settings.slint`)**:
  - `WindowsDiscovery::discover` already applies strict positive-evidence filtering: it skips symlinks and maintenance binaries (`identity::maintenance`), inspects PE metadata, corroborates against `manifest.index`, and emits a `Candidate` only when `inferred.strong && !inferred.conflict && manifest.index.exact(&inferred.title).len() == 1`.
  - Despite this high-confidence filter, `rescan_local_folders` stores every discovered `Candidate` in a pending list that is only rendered inside `Settings → Stores`. When an installation has multiple executables (`targets.len() > 1`), `selected_target` is left as `None`, blocking registration until the user opens Settings and clicks `Use executable` and `Add selected`.
- **Double-click in Library (`crates/app/ui/library.slint`, `crates/app/src/library_ui.rs`)**:
  - `GameTile` (line 75) and `GameRow` (line 120) already handle `double-clicked => { root.activated(); }`, but `ItemsView` (lines 248 and 255) wires `activated(key)` to `LibraryState.select(key, false, false); LibraryState.open-details();` instead of launching the game.
- **`Install` action on DRM-free games (`crates/app/ui/library.slint`)**:
  - For `manual` provider entries, `actions.install` evaluates to `Availability::Disabled(Reason::NoInstallRule)` (`Why.no-install-rule`). `DetailPane` (line 378) unconditionally renders `ActionWithReason { text: "Install"; why: LibraryState.install-why; }`, showing a disabled button plus `"This store has no install action."`, and `ContextMenuArea` (line 561) renders a disabled `Install` menu item.
- **Per-game Local Backups screen (`crates/app/ui/drm-free.slint`, `crates/app/src/library_ui.rs`)**:
  - `SaveProfile` in `gamenode_core::drm_free` already holds `title: String`, `files: Vec<String>`, and `registry: Vec<String>` populated by `preview_local_profile`, and `LocalView` holds `settings.destination`. However, `library_ui::render` only projects `save_status: int` and `game_automatic: bool` to `LocalGames`, hiding the actual save folders/files, registry keys, and profile name from the UI.
  - `SaveProtection` in `crates/app/ui/drm-free.slint` uses raw `std-widgets.slint` controls (`CheckBox`, `LineEdit`, plain `Action` buttons), requires manually clicking `Load backup history` before showing snapshots, formats snapshot rows as raw byte counts (`"2026-11-05 11:30 · 48120 bytes · ..."`), and has no way to cancel a restore preview without closing the dialog.

## Goals / Non-Goals

**Goals:**

- Automatically register recognized DRM-free games from watched folders with a deterministic preferred launch executable, kick off background GameNode matching and save-profile resolution, and surface newly added games in a dismissible Library banner.
- Preserve existing tombstones and ignores so removed DRM-free games are not re-added on rescan unless reset in Settings.
- Launch playable games on double-click in both grid and list views, falling back to opening the detail pane when `Play` is unavailable.
- Omit the `Install` button and context-menu item for entries whose provider has no install rule (`Why.no-install-rule`).
- Redesign `SaveProtection` using the `SettingsStyle` / `settings-controls.slint` component system, showing the exact save folders/files/registry keys being backed up, the backup archive destination, auto-loaded human-readable snapshot history, and a clear restore confirmation flow.
- Keep all filesystem, process, and backup work off the UI thread.

**Non-Goals:**

- Changing Ludusavi CLI invocation rules, backup staging/journaling, or SQLite schema tables (no new migration is required).
- Relaxing `WindowsDiscovery` positive-evidence rules to import arbitrary unknown `.exe` files from watched folders.
- Changing `Install` behavior for store providers that declare an install URI (such as Steam).

## Decisions

### D1. Automatic Registration of Watched-Folder Discoveries & Preferred Executable Selection

When `WindowsDiscovery` (or core candidate resolution) finds one or more valid game executables in an installation folder, it will always resolve a deterministic `selected_target: Some(String)` using a pure helper `preferred_target(root: &str, targets: &[String], manifest_launch_hints: &[String]) -> Option<String>`:

1. **Manifest launch hint match**: Prefer a target whose path relative to `root` matches a declared launch executable in the matched manifest entry.
2. **Gameplay binary over launcher**: Prefer executables where `!identity::launcher(target)` over known launcher wrappers.
3. **64-bit over 32-bit**: Prefer executables whose filename or relative path contains a 64-bit token (`x64`, `win64`, `64`) over 32-bit tokens (`x86`, `win32`, `32`).
4. **Shallowest relative path, then lexicographical order**: Prefer executables closer to the installation root, breaking ties case-insensitively by normalized path.

During `Engine::rescan_local_folders` (`crates/app/src/engine/drm_free.rs`):

- For each discovered `Candidate`:
  - Skip if `prior` has `ignored == true` for `candidate.fingerprint`, or if any existing `Registration` matches `candidate_matches_registration(&candidate, r) && (!r.removed || !r.rediscovery_allowed)`, or if the candidate root lies inside an existing non-manual store entry's `install_dir`.
  - Persist `candidate` via `store.save_candidate(&candidate)` so its `targets` list remains available for executable switching.
  - Inspect the resolved `target` via `host.inspect(target)`, set `registration.root = candidate.root.clone()` and `registration.title = candidate.title.clone()`, and if no active registration shares `registration.file_identity`, persist it with `store.save_registration(&registration)` and record its title in `state.local.recently_added`.
- When at least one new registration is added during a discovery pass, `rescan_local_folders` triggers `engine.rescan()` and `engine.enrich_local_games()` so the library view updates immediately and background GameNode matching + save-profile preview run automatically.
- **Alternative considered**: Keep multi-executable candidates in a blocking review modal before adding. Rejected per user decision—selecting the preferred executable immediately gets the game into the library and matched right away, while `Game settings` (`LocalGameEditor`) lets the user switch among `candidate.targets` in one click.

### D2. Library Discovery Notice Banner & Simplified Settings → Stores Section

- **Library notice banner (`crates/app/ui/library.slint`, `crates/app/src/library_ui.rs`)**:
  - Add `in property <string> discovered-notice` (or count + titles) and `callback dismiss-discovered()` on `LocalGames`.
  - When `state.local.recently_added` is non-empty, `library_ui::render` populates `LocalGames.discovered-notice` (for 1 game: the game's title; for multiple: count and comma-separated titles) and renders a dismissible notice row in the `LibraryScreen` notice bar alongside scan/session notices, with a `Dismiss` button that clears `state.local.recently_added`.
- **Settings → Stores (`crates/app/ui/settings.slint`)**:
  - Retain the watched-folder input, `Watch folder` button, list of watched roots with `Remove watch`, discovery status/error text, `Rescan watched folders`, and `Reset ignored discoveries`.
  - Remove the manual candidate review loop (`for candidate in LocalGames.candidates`, `Add selected`, `Ignore selected`, `Use executable`) from `SettingsModal`.
- **Switching among detected executables in `LocalGameEditor` (`crates/app/ui/drm-free.slint`)**:
  - Expose `in property <[string]> candidate-targets` on `LocalGames`, populated in `library_ui::render` from the matching `Candidate.targets` (when `targets.len() > 1`) for the selected manual registration.
  - In `LocalGameEditor`, if `LocalGames.candidate-targets.length > 1`, display the detected executable paths beneath the `Executable` field as clickable chips/buttons so choosing one updates `executable.text` (and replaces the corresponding entry in `gameplay.text` when it matches the previous executable).

### D3. Double-Click to Launch in Grid and List Views

- Add `callback activate(string)` to `LibraryState` (`crates/app/ui/library-state.slint`) and wire `GameGrid.activated(key)` and `GameList.activated(key)` in `ItemsView` (`crates/app/ui/library.slint`) to `LibraryState.activate(key); root.focus();`.
- In `crates/app/src/library_ui.rs`, wire `state.on_activate`:
  ```rust
  state.on_activate(move |key| {
      let key = key.to_string();
      e.submit(move |engine| {
          engine.library_input(select_input(&key, false, false));
          if let Some(view) = engine.snapshot().library {
              if view.actions.play.is_available() && view.selected.as_slice() == [key.as_str()] {
                  engine.open_entry(key, LaunchKind::Play);
              } else {
                  engine.library_input(LibInput::OpenDetails);
              }
          }
      });
  });
  ```
- **Rationale**: Running selection and the availability check inside a single submitted closure on the engine worker avoids any race condition between selecting a different tile and checking `view.actions.play`, and gracefully falls back to opening the detail pane when `Play` is disabled (e.g., already running or missing executable).

### D4. Omitting `Install` for DRM-Free Games

- In `DetailPane` (`crates/app/ui/library.slint` line 378), wrap the `Install` `ActionWithReason` in `if LibraryState.install-why != Why.no-install-rule:`.
- In `ContextMenuArea` (`crates/app/ui/library.slint` line 561), wrap the `Install` `MenuItem` in `if LibraryState.install-why != Why.no-install-rule:`.
- **Rationale**: Checking `LibraryState.install-why != Why.no-install-rule` is driven directly by the core action model (`Reason::NoInstallRule`), so it automatically hides `Install` for DRM-free (`manual`) games while preserving disabled-with-reason states for stores that do support installation.

### D5. Redesigned Per-Game Local Backups Modal (`SaveProtection`)

Restyle `SaveProtection` (`crates/app/ui/drm-free.slint`) using `SettingsStyle` and components from `crates/app/ui/settings-controls.slint` (`SettingsPanelHeader`, `SettingsScroll`, `SettingsSection`, `SettingsCard`, `SettingsRow`, `SettingsToggle`, `SettingsButton`, `SettingsInput`, `Pill`, `InfoBox`), matching the visual quality of `SettingsModal`:

- **Properties & Callbacks on `LocalGames` (`crates/app/ui/drm-free.slint`, `crates/app/src/library_ui.rs`)**:
  - `in property <string> profile-title`: active `SaveProfile.title` (or `""` if none).
  - `in property <[string]> save-paths`: resolved file/folder paths from `SaveProfile.files`. If `SaveProfile.files` is empty (e.g. `SaveSupport::NoSaves` before first play), extract the configured path patterns from `SaveProfile.definition` or display the installation/profile hint so the user still sees where saves are expected.
  - `in property <[string]> save-registry`: resolved registry keys from `SaveProfile.registry`.
  - Extend `BackupRow` to `{ key: string, label: string, size: string, destination: string }`, formatting `version.bytes` via a pure `format_bytes(u64) -> String` helper in `library_ui.rs` (e.g. `47.0 KB`, `1.2 MB`).
  - Add `callback restore-cancel()` on `LocalGames`, wired to `engine.cancel_local_restore()` (clearing `state.local.restore = None` and publishing).
  - Auto-trigger `LocalGames.history-load()` whenever the user opens `Local save protection` from `DetailPane`, `ContextMenuArea`, or after a backup/restore completes.
- **Modal Layout (`SaveProtection` in `crates/app/ui/drm-free.slint`)**:
  - Centered modal container (`width: min(680px, root.width - 2 * margin)`, `height: min(640px, root.height - 2 * margin)`) over `SettingsStyle.overlay`, with `SettingsPanelHeader` showing title `@tr("DRM-free" => "Local save protection")`, subtitle `LocalGames.title`, `Kbd { text: "Esc"; }`, and `SettingsCloseButton`.
  - Scrollable body (`SettingsScroll`) with four structured sections:
    1. **Status & Automatic Protection (`SettingsSection` + `SettingsCard`)**:
       - Top row: Save status title (`Save profile configured`, `Supported · No saves found yet`, or `No save profile configured`), status `Pill` (plus job status `Pill` when `job-status > 0`), and primary `SettingsButton` (`Backup now`, disabled when `busy || save-status == 0`).
       - Second row: `SettingsRow` with `@tr("DRM-free" => "Back up this game automatically")` and `SettingsToggle` bound to `LocalGames.game-automatic`.
       - Contextual `InfoBox` warnings when `!LocalGames.automatic` (global automatic backup is disabled in Settings), when `LocalGames.overlapping-saves` is true, or when `LocalGames.error != ""`.
    2. **Protected Locations (`SettingsSection` + `SettingsCard`)**:
       - `SettingsRow` showing **Save profile** (`LocalGames.profile-title` or `"Not configured"`).
       - **Folders & files to back up**: Displays each path in `LocalGames.save-paths` in a readable monospace/secondary path block (or explains that no save files exist on disk yet / asks to choose a save folder below).
       - **Registry keys** (rendered when `LocalGames.save-registry.length > 0`).
       - `SettingsRow` showing **Backup destination** (`LocalGames.destination`).
    3. **Backup History & Restore (`SettingsSection` + `SettingsCard`)**:
       - Header action to refresh (`SettingsButton` variant `ghost`) while history is already auto-loaded on open.
       - If `LocalGames.has-restore` is true: a highlighted restore confirmation card at the top of the section showing the warning text, the target paths/keys (`LocalGames.restore-targets`), a `Cancel` (`SettingsButton` secondary -> `LocalGames.restore-cancel()`) button, and a `Confirm restore` (`SettingsButton` danger -> `LocalGames.restore-confirm()`) button.
       - Snapshot list: each `BackupRow` rendered as a `SettingsRow` with `title: version.label`, `description: version.destination`, a neutral `Pill` with `version.size`, and a `SettingsButton` (`Restore…`, variant `secondary`, calling `LocalGames.restore-preview(version.key)`). Empty state row when `LocalGames.history.length == 0`.
    4. **Configure Save Location or Profile (`SettingsSection` + `SettingsCard`)**:
       - Quick picker buttons (`Choose save folder`, `Choose save file`) in a row.
       - Custom path input (`SettingsInput` + `Use this save location` `SettingsButton`).
       - Manifest profile override input (`SettingsInput` + `Change save profile` `SettingsButton`).
  - Also restyle `LocalGameEditor` and `ClaimConflict` in `crates/app/ui/drm-free.slint` to use `SettingsPanelHeader`, `SettingsCard`, `SettingsInput`, and `SettingsButton` for visual consistency across all DRM-free dialogs.

## Risks / Trade-offs

- **[Auto-adding watched-folder games could add a game the user previously deleted or doesn't want]** → Existing tombstones (`registration.removed && !registration.rediscovery_allowed`) and candidate ignores are respected before auto-registration. Removing a game via `Remove from library` suppresses future automatic re-addition until the user explicitly clicks `Reset ignored discoveries` in `Settings → Stores`.
- **[Preferred executable heuristic picks a secondary binary in a multi-exe folder]** → The heuristic prioritizes manifest launch hints, non-launcher binaries, and 64-bit executables, and `LocalGameEditor` surfaces all discovered executables (`candidate-targets`) for 1-click switching.
- **[Auto-loading backup history spawns Ludusavi subprocesses when opening `SaveProtection`]** → `load_local_history` already runs asynchronously on a `spawn_blocking` worker and only queries namespaces that have recorded backup jobs (`jobs.into_iter().filter(|j| j.installation == id)`), so games with no prior backups return an empty list instantaneously without spawning subprocesses.

## Migration Plan

1. No database schema migration is required: `candidates`, `registrations`, `profiles`, and `jobs` tables already store all required fields (`Candidate.targets`, `Candidate.selected_target`, `SaveProfile.files`, `SaveProfile.registry`, `SaveProfile.definition`).
2. Existing pending `candidates` in SQLite from earlier scans will be automatically registered on the next startup/rescan pass if they are not ignored and do not match a removed registration.
3. Verify all updated views and presets (`library`, `drm-detail`, `protection`, `restore`, `editor`, `settings-stores`) in both `en` and `pt_BR` via `./scripts/dev-screen.sh` and headless UI tests (`library_screen`, `library_pt_br`, `i18n_catalogs`).
