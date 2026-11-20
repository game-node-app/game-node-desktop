# Proposal

## Why

The library and every dialog now follow the 2026 desktop design (`redesign-desktop-library`,
`redesign-desktop-dialogs`), but Settings still uses the earlier layout: one long Stores page that mixes
store accounts, playtime and watched folders, a plain plugin list, no place for startup or library
preferences, and no way to see what sync did or how much space backups use. Figma section `5373:3196`
"03 · Configurações" redesigns the modal into eight topics and adds the information and preferences users
ask for. This change implements it, including the local core, host and platform work behind the new
controls, so Settings matches the rest of the app.

## What Changes

- **Modal shell.** A title bar (drag handle, title, `Esc` hint, close), a 232px navigation grouped as
  Conta / Biblioteca / Aplicativo with a **settings search**, panels built from shared card and row
  primitives, a bottom scroll fade, and a pinned **unsaved-changes bar** with per-row "changed" dots and the
  previous value. Settings switches to the shared tokens; `settingsClasses.ts` and unused `--settings-*`
  variables are removed. All tokens and specs are extracted to `design-reference/`.
- **Topics.** Conta e sincronização · Lojas · **Jogos DRM-free** (moved out of Lojas into its own topic) ·
  Backups · **Geral** (new) · Ferramentas · Plugins · Sobre.
- **Conta e sincronização.** Identity card with avatar, session storage line, "Abrir perfil" and Sair; Sync
  switch and playtime source (moved here from Lojas); **last sync summary and "Sincronizar agora"**; a static
  "O que é sincronizado" list that matches `desktop-sync`.
- **Lojas.** One card per store account with stat tiles, a primary action and an overflow menu; an expired
  account gets a warning border and "Remover login salvo"; a **per-store switch to include shared games**
  (Steam Family) in the library; a credential-storage card; and **"Detectadas neste PC"** with each store's
  install location and library-folder count.
- **Jogos DRM-free.** Watched folders with **per-folder game count, last check, availability and "Verificar"**,
  picker-based "Adicionar pasta…", "Verificar tudo agora", two new discovery preferences (**check at startup**,
  **real-time watching**), ignored-discovery count with reset, and add-from-executable.
- **Backups.** Automatic protection, destination and **default copies kept per game** saved together through
  the unsaved-changes bar; **space used per game**.
- **Ferramentas.** OptiScaler card with "Em N jogos" and "Ver jogos", **automatic tool-update checks** and
  **default channel** preferences, and the download cache listed **per cached version** (in use, nightly,
  SHA-256 verified). The PC-ports helper tools and cache stay, restyled.
- **Plugins.** Filter by origin with counts, one card per plugin with **version**, copyable id, session
  state and **Stop/Start**, native-access permission pills, expandable details (API grants vs native
  access), a warning strip when the plugin's store session expired, and a community note.
- **Sobre.** Version card with build date and OS, diagnostics (recent log, log folder, **copy system
  information**), license notices and website links.
- **Geral (new).** **Start with Windows**, **start minimized to the tray**, **closing the window minimizes to the
  tray**, default library view, **cover size**, **open the library in** a chosen scope, **show the status bar**,
  **interface language** (system / English / Português (Brasil)) and a shortcut to the keyboard shortcuts dialog.
- **BREAKING (behavior):** on Windows, closing the window can now quit GameNode when the user turns the tray
  preference off; autostart can now be turned off from GameNode. Defaults keep today's behavior.
- **Out of scope** (decided with the user): the app updater ("Atualizado", "Procurar atualizações",
  update channel), the "Mostrar “Jogando agora” no perfil" privacy card, and a backup space limit/quota.

## Capabilities

### New Capabilities

<!-- None: every behavior belongs to an existing capability. -->

### Modified Capabilities

- `desktop-settings`: new shell (title bar, grouped navigation, search, unsaved-changes bar), topic list,
  and the content of every panel; Jogos DRM-free becomes its own topic; new Geral and Plugins panel
  requirements; Backups saves as a group; the visual-system reference moves to section `5373:3196`.
- `desktop-lifecycle`: closing to the tray, autostart and starting minimized become user preferences
  (on by default where they exist today).
- `desktop-shell`: the interface language can be chosen instead of always following the OS; the status
  bar can be hidden.
- `desktop-library`: cover size and the scope the library opens in are preferences; shared-only games of a
  store can be left out of the library.
- `desktop-local-state`: persist the new application preferences on this PC.
- `desktop-sync`: "Sync now" and a last-sync summary.
- `desktop-providers`: store detection reports the install location and the number of library folders.
- `desktop-drm-free-games`: discovery-at-startup and real-time watching preferences; per-folder status,
  count and check.
- `desktop-game-tools`: automatic update-check and default-channel preferences; cached versions listed
  with their state.
- `desktop-save-backups`: a default retention for games without their own choice, and space used per game.

## Impact

- **UI** (`ui/src/features/settings/**`): rewrite `SettingsModal.tsx` and every panel; new `DrmFreePanel`,
  `GeneralPanel`, `PluginsPanel`; new settings components (`SettingsSearch`, `SettingsPanel`,
  `SettingsCard`, `SettingsRow`, `IconBox`, `StatTile`, `PathField`, `UnsavedChangesBar`); `Switch` and
  `SegmentedControl` adjustments; `uiStore` topic ids and pending-changes state; library grid (cover size),
  shell (`StatusBar` visibility, startup scope, locale), i18n `en.ts`/`pt-BR.ts`, mock host
  (`ui/src/mock/host`), Tailwind/theme tokens.
- **Core** (`crates/core`): new `AppPreferences` (lifecycle, library, locale, discovery, tools, shared
  inclusion) persisted locally; layout `coverSize`/startup scope; sync summary and sync-now; per-watch
  status/counts; default backup retention; backup usage per game; tool cache listing and preferences.
- **Host** (`crates/app`): new async commands (preferences, sync now, plugin stop/start, system info,
  backup usage, per-folder check), close-request and `--background` handling read the preferences,
  `PluginInfo.version`, locale override in `AppInfo`.
- **Platform** (`crates/platform`): autostart unregister and enabled state on Windows.
- **Plugins** (`crates/plugin-protocol`, `crates/plugin-steam`, `crates/plugin-epic`): optional install root and
  library-folder count in discovery results.
- **Dependencies on pending changes.** This change builds on, and must be archived after,
  `redesign-desktop-dialogs`, `add-linux-package-manager-plugin`, `add-pc-port-catalog` and
  `neutralize-store-coupling`, whose `desktop-settings` deltas it supersedes (`Navegação por tópicos`,
  `Painel Lojas`, `Painel Ferramentas (Tools)`); plugin requirements here assume the plugin inventory from
  `integrate-steam-plugin-into-main` and `add-epic-games-store-plugin`.
- No new third-party dependencies; no GameNode server changes.
