# Tasks

## 1. Automatic Watched-Folder Discovery & Preferred Executable Selection

- [x] 1.1 Implement pure `preferred_target(root, targets, manifest_hints)` in `crates/core/src/drm_free/identity.rs` (ranking manifest launch hints > non-launcher gameplay binaries > 64-bit targets > shallowest path depth > lexicographical order) and wire it into `WindowsDiscovery::discover` (`crates/platform/src/discovery.rs`). Verify with unit tests in `crates/core/src/drm_free/identity.rs` covering single-exe, 32-bit vs 64-bit, launcher vs game binary, and nested subfolder cases.
- [x] 1.2 Update `Engine::rescan_local_folders` (`crates/app/src/engine/drm_free.rs`) and `LocalView` (`crates/core/src/drm_free.rs`) to automatically inspect and register discovered candidates that are not ignored, tombstoned (`removed && !rediscovery_allowed`), or inside an existing store installation, record newly added titles in `LocalView.recently_added`, and trigger `rescan()` + `enrich_local_games()`. Verify with engine unit tests in `crates/app/src/engine/tests/drm_free.rs` that discovered games are registered automatically, removed games stay suppressed on rescan, and `reset_local_ignores` re-enables rediscovery.
- [x] 1.3 Add `LocalGames.discovered-notice`, `LocalGames.dismiss-discovered()`, and `LocalGames.candidate-targets` in `crates/app/ui/drm-free.slint` and `crates/app/src/library_ui.rs`; render the dismissible discovery banner in `LibraryScreen` (`crates/app/ui/library.slint`), add 1-click executable target switching in `LocalGameEditor` (`crates/app/ui/drm-free.slint`), and remove the manual candidate checklist from `Settings → Stores` (`crates/app/ui/settings.slint`). Verify with tests in `crates/app/tests/library_screen.rs` and visual inspection via `./scripts/dev-screen.sh preset settings-stores` and `editor`.

## 2. Double-Click to Launch & DRM-Free `Install` Removal

- [x] 2.1 Add `callback activate(string)` to `LibraryState` (`crates/app/ui/library-state.slint`), wire `GameGrid.activated`, `GameList.activated`, and `Key.Return` (`Enter`) in `crates/app/ui/library.slint` to `LibraryState.activate(key)`, and handle `on_activate` via `activate_entry` in `crates/app/src/library_ui.rs` (and `crates/app/examples/ui-review.rs`) so double-clicking or pressing `Enter` selects the entry and launches `Play` when available or opens the detail pane when unavailable. Verify with headless UI tests in `crates/app/tests/library_screen.rs` and engine tests in `crates/app/src/engine/tests/library.rs`.
- [x] 2.2 Hide the `Install` `ActionWithReason` in `DetailPane` and the `Install` `MenuItem` in `ContextMenuArea` (`crates/app/ui/library.slint`) when `LibraryState.install-why == Why.no-install-rule`. Verify with a test in `crates/app/tests/library_screen.rs` and `./scripts/dev-screen.sh preset drm-detail` that DRM-free entries show `Play` without `Install` while Steam entries retain `Install`.

## 3. Redesigned Per-Game Local Backups (`SaveProtection`) Modal

- [x] 3.1 Expose `profile-title`, `save-paths`, `save-registry`, structured `BackupRow { key, label, size, destination }` with `format_bytes(u64)`, and `restore-cancel()` on `LocalGames` in `crates/app/ui/drm-free.slint`, `crates/app/src/library_ui.rs`, and `crates/app/src/engine/drm_free.rs`; auto-trigger `LocalGames.history-load()` when opening `Local save protection` or completing a backup/restore. Verify with unit tests in `crates/app/src/library_ui/tests.rs` for `format_bytes` and profile path projection.
- [x] 3.2 Redesign `SaveProtection` (and align `LocalGameEditor` / `ClaimConflict`) in `crates/app/ui/drm-free.slint` using `SettingsStyle` and `settings-controls.slint` components (`SettingsPanelHeader`, `SettingsScroll`, `SettingsSection`, `SettingsCard`, `SettingsRow`, `SettingsToggle`, `SettingsButton`, `SettingsInput`, `Pill`, `InfoBox`), showing the active save profile, exact folders/files/registry keys to back up, backup destination path, auto-loaded snapshot history with human-readable sizes, and restore preview confirmation with cancel. Verify with `./scripts/dev-screen.sh preset protection` and `restore` at `1100x720` and `640x420` and keyboard tests in `crates/app/tests/library_screen.rs`.
- [x] 3.3 Update `crates/app/examples/ui-review.rs`, `i18n/en/LC_MESSAGES/gamenode-app.po`, and `i18n/pt_BR/LC_MESSAGES/gamenode-app.po` for all new and updated strings and properties. Verify catalog completeness and translations with `cargo test --locked -p gamenode-app --test i18n_catalogs --test library_pt_br`.

## 4. Integration & Visual Verification

- [x] 4.1 Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked`, and inspect `library`, `drm-detail`, `protection`, `restore`, `editor`, and `settings-stores` presets in `en` and `pt_BR` using `./scripts/dev-screen.sh`.

## Workflow follow-up

- Sync and archive this change after implementation and review are complete.
