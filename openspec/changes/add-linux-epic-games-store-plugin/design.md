# Design

## Context

The Windows Epic plugin is a catalog/account/playtime provider plus launcher-action URI templates.
Its discovery adapter reads the Windows launcher's manifests under the registry-resolved
`ProgramData` tree; the rest of the plugin uses Epic's HTTP endpoints and the host's generic
provider services. The provider manifest is `platforms = ["windows"]`, the Linux scripts exclude
the executable, and the Windows change's Linux regression asserts that no Epic UI appears.

A native Linux launcher preview exists as an AppImage (observed `0.120.0`). It runs the WebView
natively, stores its WebView state under `~/.local/share/com.epicgames.eglv2`, and its `.desktop`
entry declares `com.epicgames.launcher` with `Exec=EpicGamesLauncher %u`. The AppImage does not
install that desktop entry automatically. Windows games are run by the launcher through Proton;
GameNode does not need a Wine/Proton implementation to hand off a launch or install.

## Goals / Non-Goals

**Goals**

- Deliver the official Epic provider on Linux with the same identity, account, catalog and
  playtime behavior as Windows.
- Discover installed Epic games from the native launcher with bounded, read-only parsing.
- Hand off Play/Install to the registered launcher URI and fail clearly when integration is
  missing.
- Keep Windows behavior and acceptance unchanged.
- Make the Linux adapter resilient to the preview's evolving layout.

**Non-Goals**

- Managing, launching or configuring Proton/Wine, or running the game process from GameNode.
- Bundling, installing or redistributing the Epic launcher/AppImage.
- Registering the URI handler automatically; desktop integration remains a user/reviewer action.
- macOS support or a second Epic provider identity.
- Changing the Epic API source, account model or the Windows discovery/acceptance behavior.

## Decisions

### D1. One provider, platform-specific roots

Keep provider id `epic`, plugin id `org.gamenode.plugin.epic` and the `epicgames` source on both
platforms. `providers/epic.toml` gains `linux` in `platforms` and Linux `[[roots.linux]]` entries;
the Windows registry roots stay byte-for-byte compatible. The host already selects roots by
platform and forwards the manifest to the plugin, so no store-specific host branch is added.

### D2. Discover from the native launcher's files, not its private WebView state

The discovery adapter must identify where the AppImage preview records installed games. A spike is
required before implementation: inspect the launcher's install manifests/install metadata and
record the exact paths and schema for the observed preview version. Prefer a documented filesystem
manifest or the launcher's installed-game index; do not scrape cookies, tokens or arbitrary
IndexedDB/localStorage content. The adapter is read-only, bounded by entry/record/size limits, and
reports a successful empty scan when the launcher or its data directory is absent. If the preview
layout is unknown or changes, fail only the Epic provider, preserve its previous records and expose
a diagnostic; never guess game installations from arbitrary folders.

### D3. Hand off through the registered launcher URI

Linux Play/Install use the same `com.epicgames.launcher://...` templates as Windows. The host's URI
opener sends them to the desktop handler. The plugin/host checks whether the scheme has a handler
(for example through the desktop entry/xdg MIME query) and returns an actionable failure if it does
not: integrate the AppImage/desktop entry first. It does not spawn the AppImage path, a Wine
command or an arbitrary executable; that would bypass the host's executable-approval boundary.

### D4. Reuse the account, catalog and playtime paths

The Linux build reuses the authorization-code WebView flow, per-provider secret slot, refresh
rotation, owned-library sync, namespace matching and `epicgames` playtime counter. The generic
provider commands and account card already handle any catalog provider; Linux only changes the
descriptor platform, roots, discovery adapter and packaging.

### D5. Keep Windows unchanged

The Windows adapter, registry/ProgramData roots, package layout and Windows acceptance remain as
delivered by `add-epic-games-store-plugin`. Shared code may be split by platform, but the Windows
branch must keep its current tests and behavior.

### D6. Treat the launcher preview as versioned, fail-closed input

The launcher is a preview. Record the observed launcher version and data layout in tests/docs, and
keep root discovery/parsing version-tolerant. An unknown layout is an explicit provider error, not
an empty successful result, so the host does not clear known records or silently hide an installed
library. A launcher update that moves data must be handled by a bounded adapter change, not by a
guess.

### D7. Packaging and the Windows-change regression

Add the plugin to Linux development/release layouts and package verification; keep it required on
Windows and present on Linux. The base change's 10.2 "no Epic UI on Linux" expectation is
superseded only after this change is implemented and accepted; the base change must be archived
first. The Linux acceptance then verifies sign-in, library/catalog, install/play through an
integrated AppImage (Proton handled by Epic), playtime and the missing-handler failure.

## Risks / Trade-offs

- **Preview data is undocumented and may move** → spike first, bounded adapters, version-tolerant
  roots, fail closed with previous records preserved.
- **AppImage not integrated** → explicit actionable failure and docs; do not auto-spawn it.
- **No games installed during development** → fixture manifests plus a packaged Linux acceptance
  with at least one installed game supplied by the user.
- **Handler registration differs by desktop** → detect the scheme/handler and document
  AppImageLauncher/manual desktop-entry integration; never assume `xdg-open` works.
- **Windows regression** → keep platform adapters separate and run the existing Windows tests.
- **Credential isolation** → the Linux plugin uses the same logical provider secret slot; do not
  read the launcher's WebView cookies/tokens.

## Migration Plan

1. Archive `add-epic-games-store-plugin` and preserve its Windows evidence.
2. Spike the native launcher layout and record paths/schema/version.
3. Add Linux platform/roots/descriptor and the discovery adapter; reuse account/catalog/playtime.
4. Add URI handler detection and the Linux packaging/tests.
5. Run packaged Linux acceptance with an integrated AppImage and one installed game; update the
   superseded Linux regression note in the archived base change.
6. Rollback: remove Linux from the descriptor/roots and packaging; Windows behavior and existing
   Epic data are unaffected.
