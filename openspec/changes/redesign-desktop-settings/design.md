# Design

## Context

See proposal.md › Why. The visual target, tokens, component specs, icons and copy are in
`design-reference/` (start with its README). This document covers how the redesign and the new local
behavior fit the current code.

Current state that shapes the approach:

- `ui/src/features/settings/SettingsModal.tsx` is a `FloatingModal` (geometry key `settings`, 960×720,
  min 720×480) with a 216px user block + topic list, a fixed panel header and a scrolling body. Panels
  use the legacy class strings in `settingsClasses.ts` and `--settings-*` variables. Topics are
  `SettingsTopic = 'account' | 'stores' | 'plugins' | 'backups' | 'tools' | 'about'` in `uiStore`
  (not persisted).
- Pending edits exist only for the backup destination: `uiStore.backupDestinationDraft`,
  `discardBackupConfirmOpen`, `pendingSettingsAction`; `DiscardChangesDialog` (redesigned by
  `redesign-desktop-dialogs`) already accepts a list of pending field names.
- `StoresPanel.tsx` mixes store accounts (`StoreAccountCard.tsx`), detected stores (`ProviderCount`),
  rescan, reset hidden games and the DRM-free watched folders. `AccountPanel.tsx` already holds the sync
  switch and the playtime source.
- Preferences today: `crates/core/src/settings.rs` `Settings { window }`; `SyncPreferences` and playtime
  source through the playtime storage port (`crates/core/src/ports/mod.rs`, implemented in
  `crates/platform/src/playtime_db.rs`); `LibraryLayout` through `library/store.rs` `load_layout` /
  `save_layout`; `BackupSettings { automatic, destination, retention }` in `drm_free.rs` with
  `RETENTION_OPTIONS [5, 10, 20, 50]`.
- Lifecycle: `crates/app/src/main.rs` always calls `gamenode_platform::autostart::register()` (installed
  builds only, `"<exe>" --background`), parses `--background`, and on Windows `CloseRequested` always
  saves geometry, `prevent_close()` and hides. There is no unregister.
- Locale: host `AppInfo.locale` = normalized OS locale; `App.tsx` calls `setLocale(hostLocale)`.
- Plugins: `PluginInfo { providerId, pluginId, name, source, status, capabilities, session, nativeAccess }`
  from trusted descriptors (`plugins/*.toml`), no version, no stop/start command.
- Root font size is 14px; Tailwind's rem scale does not map to Figma pixels (see `tokens.md`).

## Goals / Non-Goals

**Goals:**

- Pixel-faithful Settings using shared tokens and a small set of settings components, so every panel is
  composed from the same card/row pieces.
- One preferences model in core for every new PC-wide preference, loaded before the window exists and
  delivered to the UI through the snapshot.
- Every new disk, registry, network or process operation runs off the UI thread through async Tauri
  commands; the UI only renders state and progress.

**Non-Goals:**

- The app updater, update channel, presence/privacy and backup quota (proposal › Out of scope).
- Persisting a stopped plugin across restarts, or disabling a plugin permanently.
- Changing what sync sends; "Sync now" reuses the existing submission rules.
- Linux autostart/tray behavior (Windows-first MVP); Linux keeps today's close/start behavior and hides
  those rows.

## Decisions

### D1 — Settings component layer and tokens

Add the tokens from `tokens.md` (`rounded-card`, `spacing.row`, `w-settings-nav`, `max-w-settings-copy`,
`bg-settings-fade`, `bg-avatar`) to `theme.css` and `tailwind.config.ts`, with provenance comments pointing
to this change's `tokens.md`. Build `ui/src/features/settings/components/` (`SettingsSearch`,
`SettingsPanel`, `SettingsCard`, `SettingsRow`, `IconBox`, `StatTile`, `PathField`, `UnsavedChangesBar`,
`ScrollFade`) exactly as `components.md` specifies, and adjust `Switch` (14px thumb, 3px padding) and
`SegmentedControl` (`segmentPadding`). Delete `settingsClasses.ts` and every `--settings-*` variable with no
remaining reference once all panels are migrated.

*Alternative:* restyle the existing panels in place with more class strings. Rejected: the eight panels
share one card/row grammar, and the old strings are exactly what drifted from the design.

### D2 — Topic model, navigation and search

`SettingsTopic` becomes `'account' | 'stores' | 'drmFree' | 'backups' | 'general' | 'tools' | 'plugins' |
'about'`. A static registry (`features/settings/topics.ts`) holds, per topic: group, icon, title/description
i18n keys, availability predicate (`AppInfo.platform`, DRM-free/backup support) and the i18n keys of its
searchable row labels. Navigation renders groups from the registry; ↑/↓ move across visible topics;
`Ctrl+F` focuses the search while the modal is open (the global library shortcut is already inert behind
the modal's focus trap; the modal handles the key itself).

Search normalizes text (NFD, strip diacritics, lowercase) over the translated strings of the current
locale, so it matches what the user reads. Matching topics stay visible; the first match is selected through
the same `selectTopic` path (so the discard confirmation still applies). Matching rows get
`data-search-match` and a brand underline on their label; the panel scrolls the first match into view.
Search text lives in component state and clears when the modal closes.

*Alternative:* search the settings by backend metadata. Rejected: all searchable text is UI copy.

### D3 — Shell layout

`FloatingModal.Header` becomes the 48px title bar (drag handle); `FloatingModal.Title` is "Configurações".
The panel title and description scroll with the content (Figma puts them inside the scroller), so the
spec's fixed parts are title bar, navigation and the unsaved bar. `ScrollFade` observes the scroller
(`scroll` + `ResizeObserver`) and hides at the end. The unsaved bar is absolutely positioned at the bottom
of the content area and the scroller gets matching bottom padding while it shows. The nav footer reads the
version from `AppInfo`. The 960×720 default and saved geometry stay.

### D4 — Pending changes model (Backups)

Replace `backupDestinationDraft` with a generic `settingsDraft: { topic: 'backups'; values: Partial<{
automatic: boolean; destination: string; retention: number }> } | null` in `uiStore`. The baseline is the
snapshot's `BackupSettings`; a field is "changed" when the draft differs from the baseline. `SettingsRow`
gets `changed` and `previous` (shown for text values). `UnsavedChangesBar` shows the count; Reverter clears
the draft; Salvar calls one extended command `set_local_backup_settings(automatic, destination, retention)`
that validates the destination on a blocking worker and saves all three atomically. On failure the draft
stays and the destination row shows the error. Switching topic or closing with a draft keeps using
`pendingSettingsAction` + `DiscardChangesDialog`, now listing the changed row labels. `Ctrl+S` while the
bar is visible triggers save.

All other panels keep apply-immediately semantics with optimistic UI and rollback on command error
(existing pattern from `AccountPanel`).

### D5 — Application preferences in core

Add `AppPreferences` to `crates/core/src/settings.rs`:

```
start_with_windows: bool = true      start_minimized: bool = true      close_to_tray: bool = true
locale: Locale { System, En, PtBr } = System                      show_status_bar: bool = true
start_scope: StartScope { Last, All, Installed, Favorites, Recent } = Last
discovery_on_start: bool = true      discovery_realtime: bool = true
tool_auto_check: bool = true         tool_default_channel: ToolChannel = Stable
shared_excluded: BTreeSet<String> (provider ids whose shared-only entries are left out; empty = all included)
```

It is stored per PC in the existing settings storage (one key-value row per field, so an unparsable value
falls back to its own default) and loaded with `Settings` before window creation. The snapshot carries it as
`preferences`. One async command `set_app_preference(patch: AppPreferencePatch)` (tagged enum, one variant
per field) validates, persists on a blocking worker, applies side effects (D6–D8) and publishes a snapshot.

`start_minimized` defaults to **on** so login starts stay windowless as today; the Figma "Off" is fixture
data. `cover_size` lives in `LibraryLayout` (it is view state that already persists per layout) as
`CoverSize { Small, Medium, Large }` with serde default `Medium`.

*Alternative:* one JSON blob. Rejected: per-key rows match how sync and playtime source are stored and keep
the "one bad value" fallback simple.

### D6 — Lifecycle preferences (Windows)

- **Autostart.** `gamenode_platform::autostart` gains `register(background: bool)` and `unregister()`. At
  start, the host calls `register(prefs.start_minimized)` only when `start_with_windows` is on, otherwise
  `unregister()`. Toggling either preference re-runs the same function on a blocking worker. Windows'
  own enable/disable lives in `StartupApproved\Run`, which GameNode never writes, so a user's Windows-side
  disable still wins. Non-installed builds keep returning `Ok(false)`; the row stays visible but shows
  that autostart applies only to installed builds.
- **Close to tray.** The `CloseRequested` handler reads the current preference from the engine's shared
  state (cheap read, no I/O on the event loop). On: today's path. Off: save geometry, then run the existing
  Quit path (ends sessions, saves state, releases single-instance resources).
- **Start minimized** has no runtime effect beyond the `Run` value (D6 autostart); manual launches always
  show the window.
- The Inicialização card renders only when `AppInfo.platform === 'windows'` and `backgroundSupported`.

### D7 — Language

`AppInfo.locale` becomes the effective locale (preference, or normalized OS locale for System) and gains
`systemLocale`. Changing the preference sends the new effective locale; the UI calls `setLocale` at once.
The host rebuilds the tray menu labels (Open/Quit) and any host-worded strings (native dialog titles) with
the new locale. No reload of the webview is needed; the copy "Reinicia a interface…" means the UI re-renders.

### D8 — Library preferences

- **Cover size**: `GRID_TILE_WIDTH` becomes a function of `CoverSize` (Small 140, Medium 172, Large 216;
  heights keep the current cover aspect + caption height). `GameGrid` recomputes columns from the measured
  width; virtualization is unchanged.
- **Default view**: Settings writes `layout.view` through the existing layout update path, so the toolbar
  switch and Settings stay in sync.
- **Start scope**: applied once by the engine when it loads the layout at start (`Last` keeps the stored
  scope; a fixed choice replaces `scope` in memory, and the user's later changes persist normally). A
  remembered collection/saved view that no longer exists already falls back to All.
- **Status bar**: `App.tsx` renders `StatusBar` only when `preferences.showStatusBar`.
- **Shared exclusion**: `library/query.rs` drops entries whose provider is in `shared_excluded` and that are
  shared-only (not owned, not installed) before scopes, filters, grouping, counts and search. Catalog sync
  is untouched. `ProviderCount.sharedCount` keeps counting from the catalog so the Lojas tile stays accurate.

### D9 — Sync now and summary

Add `SyncSummary { completed_at, accepted_games, stores, held }` per account in the playtime storage
(new table, migration). The submitter records it at the end of every pass. `sync_now()` in the engine
rescans the library, then runs a submission pass over all held totals with the existing rules (sync on,
verified session); a second call while running is ignored. The snapshot's `sync` gains `summary` and
`running`. Settings shows relative time with the locale formatter.

### D10 — Stores panel

One `StoreAccountCard` per `StoreAccountView` (provider-neutral; icon from a provider-id → Tabler icon map,
default `IconDeviceGamepad2`). Primary action = first applicable action for the state; the rest go to a
Mantine `Menu` ("Mais ações") via `MenuSurface`. "Remover login salvo" calls the existing
`provider_sign_out` (it already clears the credential). Stats: `ownedCount`/`count`, `sharedCount` (only
with `sharedLabel`), `installedCount`. Expired / credential error / persist failed → `border-warning`
(danger for credential error) plus the pill text, so state is not color-only. The shared switch writes
`shared_excluded`.

**Detected stores**: `ProviderCount` gains optional `installRoot` and `libraryFolders`, filled from the
scan result. `plugin-protocol`'s discovery result gets two optional fields; `plugin-steam` fills them from the
Steam root and `libraryfolders.vdf` entry count; `plugin-epic` fills the launcher install root when it is
known, otherwise leaves them empty. Paths are shown as received. Rescan moves to this card's header action;
"Mostrar jogos ocultos" (existing `libraryInput('resetExcluded')`) is a row in the same card. The
credential-storage card reads `snapshot.storage`; the existing private-file warning (Linux) renders inside it.

### D11 — DRM-free panel

Move the watched-folder UI to `panels/DrmFreePanel.tsx`. `LocalView.watches` becomes `WatchView { id, path,
enabled, entryCount, lastCheckedAt, state: 'idle' | 'checking' | 'unavailable', unavailableSince }`; the
engine already tracks availability for "Drive unplugged", and counts come from registrations per root.
New command `check_local_watch(id)` reconciles one root on the existing bounded discovery worker. The
startup reconcile and the filesystem watcher are gated by `discovery_on_start` / `discovery_realtime`;
turning real-time on starts the watcher and schedules one reconcile. Add folder uses the existing
`pick_local_watch_folder` (the free-text path input goes away). The ignored count comes from the persisted
ignore decisions; Redefinir uses `reset_local_ignores`. "Escolher executável…" uses the existing add flow.

### D12 — Backups panel

Default retention is the existing `BackupSettings.retention`, used for games without their own choice
(the per-game choice from `redesign-desktop-dialogs` wins). Usage comes from a new query command
`local_backup_usage()` that sums archive sizes per installation on a blocking worker, returns
`{ totalBytes, totalVersions, games: [{ entryKey, title, coverFile, bytes, versions }] }` sorted by bytes,
and is fetched with react-query, invalidated on backup job, restore and prune events in the snapshot.
Covers render as 18×24 `<img loading="lazy" decoding="async">` over `gamenode-asset://localhost/cover/<file>`.

### D13 — Tools panel

`ToolsView` gains `cacheEntries: [{ toolId, version, channel, bytes, sha256Verified, inUse }]` from the
release cache metadata already persisted (`desktop-local-state` › tool cache metadata) and the per-game
installations (`inUse` = any installation references that version). "Em N jogos" counts
`installedByEntry`; "Ver jogos" opens a menu of those games, and choosing one closes Settings and selects the
entry (existing library selection action). License and source stay as a meta line with `open_tool_source`.
`tool_auto_check` schedules `refresh_tool_versions` at start and every 24h while running (engine timer,
off-thread); the existing per-game "update available" state is what the user sees. `tool_default_channel`
preselects the channel in the game tools dialog when the game has no installation. The Windows-only
ports helper-tools and ports-cache cards are restyled with `SettingsCard` and keep their commands.

### D14 — Plugins panel

`PluginInfo` gains `version` from the trusted descriptor: an optional `version` in `plugins/*.toml`,
defaulting to the build's package version for bundled plugins (plugin-provided text never overrides it).
New async commands `plugin_stop(providerId)` and `plugin_start(providerId)` go through the plugin host
supervisor: stop cancels a pending sign-in, ends the process within the existing bounded wait and marks the
session `stopped` for this run; provider operations then return a stable "plugin stopped" error that the UI
words. Start creates a fresh session (existing restart path) and restores the account. Cards are filtered by
origin with a `SegmentedControl` (`segmentPadding="filter"`). "Detalhes" toggles an in-card section with the
API grants (`capabilities`) and native access; the toggled card is the "selected" plugin, keeping the
selectable-inventory requirement from `add-linux-package-manager-plugin` (`aria-expanded`, selected style).
Native-access pills follow `icons.md`. The id copies with `navigator.clipboard.writeText` (pattern already in
`RecentLogModal`).

### D15 — About panel

Build date comes from a `build.rs` env var (`SOURCE_DATE_EPOCH` when set, otherwise the compile date);
`AppInfo` gains `buildDate`, `osVersion` and `arch` (read once at start on a worker). "Copiar" calls a host
command `system_info_text()` that assembles version, build, OS/arch, effective locale, plugins (name,
origin, version, status) and detected stores, replacing any path under the user's profile with `%USERPROFILE%`
(`~` on Linux), and never including account names, ids or credentials; the UI writes it to the clipboard.
Footer links and "Abrir perfil" use one host command `open_gamenode_link(kind)` with a fixed enum
(`Website`, `Terms`, `Privacy`, `ReportIssue`, `Profile`) so the UI never passes URLs; `Profile` needs a
verified session like `GameNodePage::Library`.

### D16 — Mock host parity (`VITE_UI_MOCK=1`)

`ui/src/mock/host` implements every new command and snapshot field with fixtures that cover the Figma
states: expired Epic account, unavailable watched folder, pending backup changes, three plugins (one
community, stopped), cache with a nightly, and a non-Windows mode toggle to hide the Inicialização card.

### D17 — Testing

Vitest per new component and per panel (states above, keyboard, search, pending changes, a11y names);
update `SettingsModal.test.tsx`, `StoresPanel.test.tsx`, `StoreAccountCard.test.tsx`,
`PluginInventory.test.tsx`. Rust unit tests for preference parsing/defaults, layout `coverSize` default, start
scope, shared exclusion in `query.rs`, sync summary recording, watch status, backup usage, cache listing,
autostart command line, and command validation. Native Windows acceptance for autostart, close-to-tray and
tray relabeling. No performance tests or benchmarks.

## Risks / Trade-offs

- [Close-to-tray off stops tracking when the window closes] → the row's help text says so when off; default
  stays on.
- [Autostart toggled while Windows has it disabled in Startup apps] → GameNode only writes `Run`, never
  `StartupApproved`, so Windows' choice still wins; the help text does not promise it will start.
- [Overlap with pending changes' `desktop-settings` deltas] → the MODIFIED blocks are written on top of the
  pending versions; archive `redesign-desktop-dialogs`, `add-linux-package-manager-plugin`,
  `add-pc-port-catalog` and `neutralize-store-coupling` first (proposal › Impact).
- [Stopping a plugin during a scan or sign-in] → stop cancels pending sign-in first and uses the supervisor's
  bounded shutdown; the scan reports the provider as failed for that pass.
- [Shared exclusion hides entries the user expects in a collection] → only shared-only, not installed entries
  are excluded, and the switch label states it; turning it on restores them without a sync.
- [Search over translated copy differs per locale] → intentional: users search in the language they read.
- [Live language switch leaves stale host strings] → the host relabels the tray and native dialogs on change;
  anything else host-worded is re-requested by the UI.

## Migration Plan

1. Storage migration: add the `AppPreferences` keys (absent = default) and the per-account sync summary table;
   `LibraryLayout.coverSize` uses a serde default, so older layouts load as Medium.
2. First start after upgrade keeps today's behavior: autostart on with `--background`, close to tray on,
   System language.
3. Rollback: older builds ignore unknown settings keys and the unknown layout field; the sync summary table is
   unused by them. A user who turned autostart off and downgrades gets the `Run` value rewritten by the older
   build at its next start (documented limitation).

## Open Questions

- Exact GameNode URLs for Terms, Privacy, Report a problem and the profile page: constants in the host's
  `open_gamenode_link`; they do not change the specs or tasks.
- Epic launcher install-root source (registry vs. `LauncherInstalled.dat`): the field is optional, so the
  card works without it.
