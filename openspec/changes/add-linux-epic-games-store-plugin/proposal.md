# Proposal

## Why

A native Linux build of the Epic Games Launcher is now available as an AppImage preview. It runs on
Linux and launches Windows games through Proton itself. The existing `add-epic-games-store-plugin`
change deliberately delivers Epic as a Windows-only provider: its discovery roots are Windows
registry/`%ProgramData%` paths, its packaging excludes Linux, and its Linux regression explicitly
verifies that no Epic UI appears.

That means a Linux user with the native launcher can sign in and install/play games outside
GameNode, but GameNode cannot show the Epic library, track its playtime or hand off Play/Install to
the launcher. This change adds Linux support to the same official Epic plugin while leaving the
Windows engine and acceptance unchanged.

The native launcher preview observed on Arch Linux stores its WebView data under
`~/.local/share/com.epicgames.eglv2` and its desktop entry declares
`MimeType=x-scheme-handler/com.epicgames.launcher` with `Exec=EpicGamesLauncher %u`. The AppImage
does not install that desktop entry by itself, so the URI handler must be registered by the user or
an AppImage integration tool before Play/Install can work.

## What Changes

- **Linux platform.** Build and package `gamenode-plugin-epic` on Linux and add Linux to the
  provider descriptor/platform data. Keep the same plugin and provider identity (`epic`) and the
  same account/catalog/playtime semantics.
- **Native launcher discovery.** Locate the preview launcher's installed-game data and parse it
  with a bounded read-only adapter. Exclude DLC, incomplete installs and engine builds exactly as
  the Windows adapter does. A missing launcher is an empty successful scan; an unknown or changed
  preview layout fails only the Epic provider and preserves its previous records.
- **Account, catalog and playtime.** Reuse the existing authorization-code login, per-provider
  secret slot, refresh token rotation, owned-library sync and `epicgames` playtime totals on Linux.
  The generic store account card renders Epic on Linux when the plugin is present.
- **Play and Install.** Hand off through `com.epicgames.launcher://` on Linux. The handler must be
  registered by the user's desktop integration; if it is missing, report an actionable
  "integrate the Epic Games Launcher AppImage" failure instead of spawning the AppImage or an
  arbitrary executable. The launcher, not GameNode, decides how Proton runs the game.
- **Packaging and tests.** Include the plugin in Linux development/release layouts and package
  verification; keep the Windows package unchanged. Add Linux discovery/action fixtures, a missing
  launcher/handler test, and a packaged Linux acceptance run.
- **Reconcile the Windows change.** `add-epic-games-store-plugin` task 10.2 currently expects no
  Epic UI on Linux. This change supersedes that expectation once implemented; the base change must
  be archived first, and the Linux regression must be rewritten to expect the Linux provider.

## Capabilities

### Modified Capabilities

- `desktop-epic-games`: Linux native launcher discovery, Linux account/catalog/playtime and URI
  actions; platform-specific roots and fail-closed behavior for preview changes.
- `desktop-plugins`: the Epic plugin is packaged on Linux as well as Windows.
- `desktop-providers`: provider discovery roots and launcher actions may be platform-specific; the
  same provider id can have per-platform roots without host store code.

## Impact

- **`crates/plugin-epic`**: Linux target configuration, native launcher root resolution and a
  Linux discovery adapter; shared HTTP/account/catalog/playtime code reused.
- **`providers/epic.toml`**: `platforms` gains `linux` and Linux roots/action behavior; Windows
  roots remain unchanged.
- **App/plugin host**: the trusted Epic descriptor supports Linux; packaging and package
  verification include the plugin; no Core store-specific code.
- **UI**: the existing provider-neutral account card and store views show Epic on Linux; no new
  store-specific UI is required.
- **Scripts/docs**: Linux build/package/install/uninstall scripts; `docs/epic-games-store.md` and
  the plugin packaging docs gain the Linux flow and the AppImage handler requirement.
- **Dependency**: `add-epic-games-store-plugin` must be archived first. This change adds Linux
  requirements to the same capabilities and does not reimplement the Windows engine.
- **Out of scope**: Wine/Proton configuration or process management, bundling or redistributing the
  Epic launcher, automatic AppImage desktop integration, macOS support, changing the Epic catalog
  source or any Windows acceptance task.
