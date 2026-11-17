# Tasks

Read before starting:

- `proposal.md`, `design.md` (decisions D1–D17) and the specs under `specs/` (the `desktop-settings` delta is
  in Portuguese, like its main spec).
- Everything in `design-reference/`: `README.md` (frame index, rules, out-of-scope list), `screens/*.png`
  (visual targets), `raw/*.txt` (exact sizes; start with `raw/00-shell-nav.txt`), `tokens.md`,
  `components.md`, `icons.md`, `copy.md`. You do not need Figma access.
- Archive order: this change builds on the pending `redesign-desktop-dialogs`,
  `add-linux-package-manager-plugin`, `add-pc-port-catalog` and `neutralize-store-coupling` deltas; their code
  is already on `main`.

Rules for every group:

- Follow AGENTS.md: never block the UI thread; Tauri commands are async and blocking work runs on workers;
  Tailwind token classes only (inline `style` only for runtime values; root font size is 14px, so use token
  keys or arbitrary px); Mantine v9 through `ui/src/components/ui` wrappers; Tabler icons; `FloatingModal` /
  `AppModal` for overlays; covers through `gamenode-asset://` with `<img loading="lazy" decoding="async">`.
- Every user-facing string a task adds goes into both `ui/src/i18n/en.ts` and `ui/src/i18n/pt-BR.ts` in the
  same task, from `copy.md`; `i18n.test.ts` stays green. Never ship the word "NOVO". Never render the
  out-of-scope items (updater, "Jogando agora", backup quota).
- Every new command, snapshot field or view field is mirrored in `ui/src/mock/host.ts` (and its fixtures,
  covered by `ui/src/mock/*.test.ts`) in the same task that adds it to the host, so
  `VITE_UI_MOCK=1 pnpm --dir ui dev` works at every step.
- Keep existing `data-testid`s where the element survives; update tests in the task that changes them.
- No performance tests or benchmarks.
- Verification commands: `cargo test --locked -p gamenode-core -p gamenode-platform -p gamenode-app`,
  `cargo test --locked -p gamenode-plugin-protocol -p gamenode-plugin-steam -p gamenode-plugin-epic`
  (group 6), `pnpm --dir ui test --run`, `pnpm --dir ui build` (runs `tsc -b`).

## 1. Tokens and shared primitives

- [x] 1.1 Add the new tokens from `design-reference/tokens.md` (`--radius-card`, `--spacing-row`,
  `--settings-nav-width`, `--settings-copy-max-width`, `--gradient-settings-fade`, `--gradient-avatar`) to
  `ui/src/styles/theme.css` and `ui/tailwind.config.ts` with provenance comments; verify with a Vitest that
  renders elements using `rounded-card`, `py-row`, `w-settings-nav`, `max-w-settings-copy`, `bg-settings-fade`,
  `bg-avatar` and `pnpm --dir ui build` succeeds.
- [x] 1.2 Adjust `Switch` to the 14px thumb / 3px padding spec (D1) and verify `Switch.test.tsx` plus the
  existing library/dialog tests that render switches pass.
- [x] 1.3 Add `segmentPadding` (`compact` default 6px, `regular` 12px, `filter` 10px) to `SegmentedControl`;
  verify `SegmentedControl.test.tsx` covers the three paddings and library callers are unchanged.

## 2. Settings components

- [x] 2.1 Create `ui/src/features/settings/components/` with `SettingsPanel`, `SettingsCard` (37px header,
  56px header with action, tone border) and `SettingsRow` (label/help/icon/control, `changed` dot with
  accessible "Alterado", `previous` "Antes:" line, compact variant, wrap below 560px) per `components.md`
  §3–5; verify one Vitest each for structure, labelling (`aria-labelledby`/`aria-describedby`) and variants.
- [x] 2.2 Add `IconBox`, `StatTile`, `PathField` and `ScrollFade` (hides at scroll end via scroll +
  `ResizeObserver`) per `components.md` §6–7 and §1; verify one Vitest each, including `PathField` typing,
  title tooltip with the full path and brand border when changed.
- [x] 2.3 Add `UnsavedChangesBar` (count plural, Reverter, Salvar alterações with loading, `role="region"`,
  polite live region) and `SettingsSearch` (placeholder, `type="search"`, Esc clears when non-empty) per
  `components.md` §2 and §9 with copy from `copy.md`; verify Vitest for both, including Esc behavior.

## 3. Modal shell, navigation and search

- [x] 3.1 Extend `SettingsTopic` in `uiStore` with `drmFree` and `general`, add the topic registry
  `features/settings/topics.ts` (groups, icons, title/description keys, availability, searchable row label
  keys) per D2; verify a unit test that the registry lists the eight topics in spec order and hides none.
- [x] 3.2 Rebuild `SettingsModal.tsx` with the title bar (drag handle, title, decorative `Esc` Kbd, close),
  232px grouped navigation (Lojas attention marker with accessible name, Plugins count, version footer),
  scrolling panel with title inside the scroller, `ScrollFade`, and `rounded-dialog border-border-overlay
  bg-canvas shadow-alert`, keeping geometry key, sizes, overlay and focus restoration (D3); verify
  `SettingsModal.test.tsx` covers groups/order, footer version, close by button/Esc/overlay, focus return, and
  ↑/↓ skipping group labels (Conta → ↓↓ → Jogos DRM-free).
- [x] 3.3 Implement settings search (D2): accent/case-insensitive match over translated titles, descriptions
  and row labels; filter nav; select the first match through the discard-aware path; highlight and scroll to
  matching rows; "Nenhuma configuração encontrada"; `Ctrl+F` focuses the field while the modal is open
  without triggering library search; verify Vitest for "bandeja" → Geral, no-result state, clearing, Esc
  order (menu/dialog → clear search → close) and `Ctrl+F`.
- [x] 3.4 Replace `backupDestinationDraft` with the generic `settingsDraft` model (D4), wire
  `UnsavedChangesBar` into the content area (scroller bottom padding while visible), `Ctrl+S` save, and
  `DiscardChangesDialog` with the changed row labels on topic switch/close; verify Vitest for dirty count,
  Reverter, discard on topic switch and close, and that `Ctrl+S` calls save.
- [x] 3.5 Visual check of the shell against `screens/S1-account-sync.png` in `VITE_UI_MOCK=1 pnpm --dir ui dev`
  at 1440×900 (title bar 48px, nav 232px, panel padding 28/32/32/32, card radius 10); record mismatches fixed.

## 4. Core and host: application preferences and lifecycle

- [x] 4.1 Add `AppPreferences` (fields and defaults in D5) to `crates/core/src/settings.rs` with per-key
  persistence in the existing settings storage and a migration; load it with `Settings` before window
  creation; verify core tests for defaults on a fresh/old database, one unparsable value falling back alone,
  round-trip of every field, and independence from sign-out.
- [x] 4.2 Expose `preferences` in the snapshot and add async `set_app_preference(patch)` (tagged enum per
  field, persisted on a blocking worker, snapshot published) in `crates/app/src/commands.rs`; add types in
  `ui/src/api/types.ts`/`commands.ts` and mirror in `ui/src/mock/host.ts`; verify app command tests (valid
  patch persists, invalid value rejected) and `ui/src/mock/host.test.ts`.
- [x] 4.3 Add `register(background: bool)` and `unregister()` to `crates/platform/src/autostart.rs` (installed
  builds only; never touch `StartupApproved`), and make startup and preference changes call them per D6;
  verify platform unit tests for the command line with/without `--background` and `cargo xwin test --workspace
  --target x86_64-pc-windows-msvc --locked` compiles and passes the Windows-only tests.
- [x] 4.4 Make the Windows `CloseRequested` handler read `close_to_tray` from engine state: on → hide as today;
  off → save geometry and run the Quit path (D6); verify an app test for the decision function and that Linux
  close behavior is unchanged.
- [x] 4.5 Effective locale (D7): `AppInfo.locale` = preference or normalized OS locale, add `systemLocale`;
  on change, relabel the tray menu and host-worded native dialog titles; verify Rust tests for effective-locale
  resolution and a UI test that changing the preference calls `setLocale` and re-renders the open modal.

## 5. Core and host: library, sync, DRM-free, backups, tools, plugins, about

- [x] 5.1 Add `CoverSize` to `LibraryLayout` (serde default Medium) and apply `start_scope` once at layout load
  (D8); verify core tests for old layouts → Medium, each start scope, `Last` with a deleted collection → All,
  and that later scope changes persist.
- [x] 5.2 Exclude shared-only, not-installed entries of providers in `shared_excluded` in `library/query.rs`
  before scopes, filters, groups, counts and search, keeping `ProviderCount.sharedCount` from the catalog
  (D8); verify query tests for exclusion, installed shared game kept, owned kept, and re-inclusion without sync.
- [x] 5.3 Add the per-account `SyncSummary` table and record it at the end of each submission pass; add async
  `sync_now()` (rescan + submit held totals, ignored while running, unavailable when sync off/unverified) and
  snapshot `sync.summary`/`sync.running` (D9); mirror in the mock; verify core/app tests for the
  "Sync now and last sync summary" and "Persist the last sync result" scenarios.
- [x] 5.4 Replace `WatchRoot` in `LocalView.watches` with `WatchView` (count, last check, state,
  unavailable-since), add async `check_local_watch(id)`, and gate the startup reconcile and the filesystem
  watcher on `discovery_on_start`/`discovery_realtime` (real-time on → start watcher + one reconcile) (D11);
  mirror in the mock; verify tests for the "Discovery preferences" and "Watched folder status" scenarios.
- [x] 5.5 Extend `set_local_backup_settings` with `retention` (validated against `RETENTION_OPTIONS`, saved
  atomically with automatic and destination; per-game choice still wins) and add async `local_backup_usage()`
  computed on a blocking worker (D12); mirror in the mock; verify tests for the "Default retention for new
  games" and "Backup space used per game" scenarios.
- [x] 5.6 Add `ToolsView.cacheEntries` (tool, version, channel, bytes, sha256Verified, inUse) from the release
  cache metadata, the `tool_auto_check` scheduler (start + every 24h, off-thread, never downloads) and default
  channel preselection in the game tools dialog (D13); mirror in the mock; verify tests for the "Cached
  versions are listed" and "Tool update preferences" scenarios.
- [x] 5.7 Add optional `version` to plugin descriptors (`plugins/*.toml`, default package version) and
  `PluginInfo.version`; add async `plugin_stop(providerId)` / `plugin_start(providerId)` through the supervisor
  (cancel pending sign-in, bounded shutdown, stable "plugin stopped" error, fresh session on start) (D14);
  mirror in the mock; verify plugin host tests for version trust, stop/start, and provider calls failing with
  the stopped error while stopped.
- [x] 5.8 Add `AppInfo.buildDate`/`osVersion`/`arch` (build.rs env), async `system_info_text()` (redacts
  profile paths, no account data) and `open_gamenode_link(kind)` with the fixed enum including `Profile`
  (D15); mirror in the mock; verify tests that the system info contains no home path, account name or
  credential, and that `Profile` without a session returns `NoSession`.

## 6. Store detection location

- [x] 6.1 Add optional `install_root` and `library_folders` to the discovery result in `plugin-protocol`
  (backward compatible), fill them in `plugin-steam` from the Steam root and `libraryfolders.vdf`, and in
  `plugin-epic` when the launcher root is known; carry them to `ProviderCount.installRoot`/`libraryFolders`
  (D10); mirror in the mock; verify protocol round-trip tests, a Steam test with four library folders, and a
  provider without location keeping only the installed count.

## 7. Panels

- [x] 7.1 Rebuild `AccountPanel` (S1): identity card (avatar image or `bg-avatar`, storage line, Conectado,
  Abrir perfil → `open_gamenode_link('profile')`, Sair), Tempo de jogo e sincronização (sync switch, playtime
  source segmented `regular`, last sync row + Sincronizar agora), O que é sincronizado list; signed-out state;
  verify Vitest for the "Painel Conta" scenarios and the visual check against `S1-account-sync.png`.
- [x] 7.2 Rebuild `StoresPanel` and `StoreAccountCard` (S2): provider-neutral cards with icon map, stat tiles,
  primary action + "Mais ações" menu, warning/danger borders with text, Remover login salvo, shared switch,
  credential storage card (with the Linux private-file warning), Detectadas neste PC with path/libraries,
  Reescanear and Mostrar jogos ocultos; remove the DRM-free section and the playtime-source control from
  Lojas; verify updated `StoresPanel.test.tsx`/`StoreAccountCard.test.tsx` for the "Painel Lojas", "Cartão da
  conta Steam" and "Ações da conta Steam" scenarios and the check against `S2-stores.png`.
- [x] 7.3 Create `DrmFreePanel` (S3): header "Verificar tudo agora", watched folders with count/last
  check/state pills, per-folder Verificar and Parar de monitorar, Adicionar pasta… via picker, Descoberta
  switches, ignored count + reset, add from executable, unsupported state; verify Vitest for the "Painel Jogos
  DRM-free" scenarios and the check against `S3-drm-free.png`.
- [x] 7.4 Rebuild `BackupsPanel` (S4) on the draft model: automatic switch, `PathField` + Escolher pasta…,
  copies select (5/10/20/50), changed dots and "Antes:", unsaved bar, Uso do espaço (total + per-game rows
  with covers, no quota), unsupported state; verify Vitest for the "Painel Backups" and "Edições de texto
  exigem salvar" scenarios and the check against `S4-backups.png`.
- [x] 7.5 Rebuild `ToolsPanel` (S5): OptiScaler card (IconBox accent, summary, license/source meta, "Em N
  jogos", Ver jogos menu that closes Settings and selects the game), auto-check switch, default channel
  segmented, Cache de downloads with per-version rows and Limpar cache, restyled Windows-only ports cards;
  verify Vitest for the "Painel Ferramentas (Tools)" scenarios and the check against `S5-tools.png`.
- [x] 7.6 Create `PluginsPanel` (S6) replacing the list/detail of `PluginInventory.tsx`: origin filter with
  counts, cards with version, origin pill, copy id, session pill, Parar/Iniciar, native-access pills,
  Detalhes toggle (grants vs native access, `aria-expanded`, selected style), expired-store strip linking to
  Lojas, community note; verify updated `PluginInventory.test.tsx` (or `PluginsPanel.test.tsx`) for the "Painel
  Plugins" and existing "Inventário selecionável de plugins" scenarios and the check against `S6-plugins.png`.
- [x] 7.7 Rebuild `AboutPanel` (S7): wordmark, version, "Build {date} · {os} {arch}" (no channel/updater),
  Diagnóstico (recent log, log folder, Copiar system info with toast), Licenças, footer links via
  `open_gamenode_link`; verify Vitest for the "Painel Sobre" scenarios and the check against `S7-about.png`.
- [x] 7.8 Create `GeneralPanel` (S8): Inicialização (Windows only), Biblioteca (default view ↔ layout view,
  cover size, Abrir em, status bar), Idioma e atalhos (language select, Kbd "?" + Ver atalhos opening
  `ShortcutsModal` above the modal with focus return); verify Vitest for the "Painel Geral" scenarios
  (Windows and Linux) and the check against `S8-general.png`.

## 8. Library and shell consumers

- [x] 8.1 Make `GameGrid`/`libraryGroups.ts` tile size depend on `layout.coverSize` (Small 140 / Medium 172 /
  Large 216, aspect kept) and keep virtualization; verify grid tests for column counts per size and the
  "Cover size preference" scenarios.
- [x] 8.2 Render `StatusBar` only when `preferences.showStatusBar`, and apply the default-view sync between
  Settings and the toolbar switch; verify shell/library tests for "Status bar visibility preference" and
  "Default view in Settings".

## 9. Cleanup and integration

- [x] 9.1 Delete `ui/src/features/settings/settingsClasses.ts` and every `--settings-*` variable with no
  remaining reference (keep or rename `--settings-toggle-track-off` / `--settings-accent-hover` with all
  consumers per `tokens.md`); verify `grep -rn "settingsClasses\|--settings-" ui/src` shows only the kept
  variables and `pnpm --dir ui build` passes.
- [x] 9.2 Update `docs/development.md` with the new mock scenarios (expired store, unavailable folder, pending
  backup changes, stopped community plugin, non-Windows mode); verify the documented `VITE_UI_MOCK=1` steps
  reach each state.
- [x] 9.3 Run `cargo test --locked -p gamenode-core -p gamenode-platform -p gamenode-app`, the plugin crate
  tests, `pnpm --dir ui test --run` and `openspec validate redesign-desktop-settings --strict`; all pass.
- [ ] 9.4 Native Windows acceptance with `scripts/install-windows-wsl.sh`: autostart off removes the `Run` value
  and on rewrites it (with and without `--background`); Windows Startup-apps disable still wins; close with
  the tray preference off quits and ends a running session; language switch relabels the tray menu; record
  results in the change.
  Native acceptance, 2026-11-16 (package built and installed with `scripts/install-windows-wsl.sh`, UI driven
  through WebView2 remote debugging against the installed `gamenode.exe`; registry read with PowerShell):
  - Verified: first start writes `Run` with `--background`; autostart off removes the value and it stays absent
    after a restart; on writes it again; start minimized off rewrites it without `--background` and on restores it;
    a stale path in `Run` is rewritten at start; a disabled `StartupApproved\Run` entry is never touched or
    cleared by any of those writes; close with the tray preference on keeps one process and a relaunch reuses
    the window; close with it off exits the process; 12 window resizes interleaved with 12 preference writes
    persist the last geometry and the last preference across a restart.
  - Not observed natively: the tray menu relabel on a language switch (the native tray menu is not reachable
    through WebView2; covered by `tray_labels_follow_the_effective_locale`) and close ending a running tracked
    session (no tracked game available on this PC). Task stays open for those two checks.

## Workflow follow-up

- Archive `redesign-desktop-dialogs`, `add-linux-package-manager-plugin`, `add-pc-port-catalog` and
  `neutralize-store-coupling` before archiving this change.
- Archive this change after review, then verify the merged `desktop-settings` spec reads correctly.
