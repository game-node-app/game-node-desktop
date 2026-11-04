# Proposal

## Why

GameNode Desktop currently has a roadmap but no application. Step 1 establishes the native Windows and Linux foundation so later changes can add a unified game launcher without rebuilding authentication, persistence, theming, or OS integration.

## What Changes

- Create the Rust workspace with `core`, `platform`, `api`, and `app` crates, a pinned toolchain, formatting/lint configuration, and Windows/Linux build and test CI.
- Add a Slint shell with Skia by default, a persisted software-rendering option, an app menu, remembered window geometry, and an empty library after sign-in.
- Copy the colors, typography, spacing, and radii from the supplied Figma file's **Version 3.0** designs into a local Slint theme; bundle Roboto and GameNode brand assets. The imported [token reference](references/figma-version-3.md), [machine-readable snapshot](references/figma-version-3-tokens.json), and exact exported wordmark supply values and provenance. Figma supplies visual guidance only and does not expand roadmap scope.
- Register `app.gamenode://` on Windows and Linux and forward second launches and authentication callbacks to one running instance.
- Reuse the server's mobile browser sign-in contract, persist sessions in the OS keyring, restore them after restart, support sign-out, and show the signed-in user in the top bar. On Linux without Secret Service, use a user-only token file and identify that storage in Settings.
- Introduce SQLite migrations and persisted foundation settings, plus Settings/About with version and license information.
- Wire Slint `@tr()` and gettext catalogs for English and Brazilian Portuguese.
- Use **GPL-3.0-only** for the repository and select Slint's GPLv3 option, as chosen by the user.

This change implements **only step 1** of `docs/mvp-roadmap.md`. Game discovery, provider manifests/runtime, scanning, matching, launch actions, populated library layouts, tracking, tray/background mode, autostart, reporting, Proton, OptiScaler, release signing, updates, AppImage, and Flatpak distribution belong to later steps. Figma screens do not introduce social, feed, collection, or other web features into desktop.

## Capabilities

### New Capabilities

- `desktop-shell`: The native themed shell, signed-in empty library, app navigation, window geometry, renderer selection, localization, and Settings/About.
- `desktop-auth`: Browser sign-in, callback validation, session restoration, protected credential storage, and sign-out.
- `desktop-local-state`: An application-owned SQLite database with ordered migrations and durable non-secret foundation settings.
- `desktop-lifecycle`: Per-user installation integration, one running instance, and delivery of launches and the GameNode URI scheme on Windows and Linux.

### Modified Capabilities

None. The project has no existing capability specs.

## Impact

- New application sources and configuration under `crates/`, `i18n/`, workspace/toolchain files, installation support, CI, license notices, and development/verification documentation. These are implementation targets; this proposal changes planning files only.
- Runtime/build dependencies include Slint/Slint build tooling, Tokio, rusqlite, keyring, and a Rust HTTP client; Windows and Linux need their native windowing and credential integrations.
- The only server API surface needed here is existing GameNode authentication/session APIs. No new server endpoints, TypeScript code reuse, desktop store connections, or importer integration are proposed.
- Figma MCP access confirmed that `3994:677` is the Version 3.0 page in file `ikcThguwowTdynGz0L2YkS`. The planning snapshot imports shared GameNode semantic/dimension variables, source aliases/modes, focused settings/type measurements, and Top Bar/primary-button guidance. Its explicit selection policy preserves source differences without introducing Figma-only features.
