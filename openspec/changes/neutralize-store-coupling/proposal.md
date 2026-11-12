# Proposal

## Why

The Epic Games Store plugin moved store logic out of the Rust host, but three pieces of store
coupling are still left, and each would need host edits for the next store:

- The catalog model and the UI treat "Steam Family" as a universal concept. Shared-access text and
  the achievement status messages name Steam directly. Today an Epic entry or an Epic achievement
  failure would be described as Steam.
- The achievement icon cache trusts a static list of Steam CDN hosts in `gamenode-platform`.
- Every build, packaging and install script hardcodes the plugin executable names.

The next store with shared libraries or achievements (GOG, Xbox, itch.io) should not need any of
these edits.

## What Changes

- **Store-neutral shared access.**
  - The catalog's family category becomes a generic *shared* category.
    - The protobuf names change: `CATALOG_CATEGORY_FAMILY` becomes `CATALOG_CATEGORY_SHARED` and
      `family_error` becomes `shared_error`. The wire numbers stay the same.
    - The host model, the snapshots and the UI types use `shared` names.
  - A provider manifest declares shared access with a display label. Steam declares
    `shared = { label = "Steam Family" }`.
  - The host rejects shared catalog data from a provider that declares no shared access.
  - The UI shows shared counts, indicators and the detail-pane access line only for providers that
    declare shared access, using the declared label.
  - The availability filter is renamed "Shared". A layout saved with the old `family` filter key
    still restores.
- **Provider-named achievement and host messages.** Achievement privacy, not-connected and
  sign-in-again messages use the provider's display name instead of "Steam". The startup log no
  longer calls every provider failure a Steam failure.
- **Achievement icon hosts are trusted descriptor data.**
  - A plugin descriptor with the achievements grant declares `achievement_icon_hosts`.
  - The icon cache fetches an icon, and follows its redirects, only for hosts declared by the
    provider whose achievements are shown.
  - `TRUSTED_ICON_HOSTS` and its four boundary-allowlist entries are removed.
- **Plugin binaries are derived from descriptors.**
  - One stdlib Python helper reads `plugins/*.toml` and lists each bundled plugin's executable for a
    platform.
  - Build, run, package and verification scripts use the helper instead of literal names.
  - Packages carry a generated `bundled-plugins.json`. The Windows installer and uninstaller read
    it. The uninstaller also removes the plugins recorded at install time.
  - The boundary checker derives store ids, store hosts and URI schemes from the descriptors and
    manifests.
- **BREAKING (internal only).** Plugin SDK and protocol identifiers are renamed (`Family` to
  `Shared`). All plugins are in-tree, and the wire format is unchanged.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `desktop-providers`:
  - Manifests may declare shared catalog access with a label.
  - The independent-facts requirement covers a generic shared fact instead of a Steam Family fact.
- `desktop-library`:
  - Shared indicators, the detail-pane access line and the availability filter use the provider's
    declared label instead of Steam Family.
  - Older layouts restore.
- `desktop-settings`: the Stores panel shows shared counts only for providers that declare shared
  access, using the declared label.
- `desktop-plugins` (introduced by the unarchived `integrate-steam-plugin-into-main` and
  `add-epic-games-store-plugin`): the packaged plugin set is derived from the trusted descriptors.
- `desktop-achievements` (introduced by the unarchived `add-achievement-tracking`):
  - Icon hosts come from the trusted descriptor of the provider that owns the achievements.
  - Achievement messages name the entry's provider.

## Impact

- **Protocol and SDK:** `crates/plugin-protocol/proto/plugin.proto`, `catalog.rs`, `models.rs`,
  `crates/plugin-sdk`, and the catalog code in `crates/plugin-steam`.
- **Host:**
  - Descriptor and manifest parsing: `crates/app/src/plugin_host.rs`, `crates/app/build.rs`, the
    provider manifest code in `crates/core/src/provider`.
  - Library model: `crates/core/src/library/{store,query,layout}.rs` and the
    `crates/platform/src/library_db.rs` mapping. SQL column names stay as they are.
  - Achievements: `crates/app/src/engine/achievements.rs`,
    `crates/platform/src/achievement_icons.rs`.
  - Logging: `crates/app/src/main.rs`.
- **UI:**
  - Types: `ui/src/api/types.ts`.
  - Library: `GameGrid`, `GameList`, `DetailPane`, `AchievementListModal`, `LibraryToolbar`.
  - Settings: `StoreAccountCard`, `StoresPanel`.
  - Translations: `en.ts`, `pt-BR.ts`.
  - Mocks: `ui/src/mock/host.ts`.
- **Descriptors:** `providers/steam.toml` (shared label) and `plugins/steam.toml` (icon hosts).
- **Scripts and packaging:**
  - Run and build: `run-desktop.sh`, `run-desktop.ps1`, `build-windows-wsl.sh`,
    `dev-windows-wsl.sh`, `dev-windows-wsl.ps1`.
  - Packaging and verification: `package-linux.sh`, `package-windows-wsl.sh`,
    `verify-plugin-package.py`, `check-plugin-boundaries.py` and its allowlist.
  - Installer: `packaging/windows/install.ps1`, `uninstall.ps1`.
  - The tests in `scripts/tests`.
- **Dependencies and data:** no new dependencies and no database migration. Running
  `run-desktop.ps1` on native Windows now requires Python 3.11+, which is documented in
  `docs/development.md`.
