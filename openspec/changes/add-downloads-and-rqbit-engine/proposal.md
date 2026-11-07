# Proposal

## Why

Today a DRM-free game acquired through a magnet link forces the user out of GameNode: install and configure a separate BitTorrent client, wait for the download, find the folder, run the installer by hand, and only then add the game to the library. GameNode already owns detection, matching, launch, playtime and save protection, but it has no transfer surface. Bundling a BitTorrent engine closes that gap and removes the qBittorrent dependency entirely.

The feature is deliberately opt-in. GameNode never searches, indexes, recommends or hosts content; it only accepts a link the user already has. Downloads and automatic installation are off until the user accepts a versioned consent term.

## What Changes

- **New top-level navigation**: the signed-in shell is split into **Biblioteca** and **Downloads**. Sidebar shows both sections, the Downloads item carries a badge for active and completed-unhandled transfers, and keyboard navigation reaches both.
- **Consent before anything else**: opening Downloads for the first time, or trying to add a magnet/torrent from anywhere, shows a full consent screen. Accepting stores the term version and enables the feature; declining returns to the Library and leaves everything disabled. No engine process runs while the feature is off.
- **Bundled rqbit sidecar**: `rqbit` (Apache-2.0) ships as a Tauri sidecar for Windows and Linux, with no qBittorrent integration. The host owns its lifecycle: spawn on first use after consent, health check, authenticated loopback HTTP API, crash reporting, restart, and graceful stop on disable or Quit. Disabling the feature stops the process and opens no ports.
- **Downloads section**: empty state, add magnet or `.torrent`, list with state/progress/size/speed/ETA/peers, detail pane with files and destination, pause/resume, forget keeping files, and open folder. Progress updates arrive on a dedicated event channel without republishing the library snapshot.
- **Completion handoff**: a completed transfer offers exactly two primary actions — **Instalação automática** and **Abrir pasta de destino**.
- **Automatic installation (Windows)**: detect the installer, ask for a second explicit confirmation showing path, size, SHA-256 and publisher, run it as a child process without a shell, detect the installed folder and executable, and create a DRM-free library entry. Portable game folders are detected and added without an installer.
- **Linux/Proton automatic installation is deferred**: this change does not implement Proton/Wine detection, prefix creation, Linux installer execution, or Wine/Proton-backed entries. On Linux, Downloads and the bundled engine still work, but automatic installation is disabled with a localized explanation and the folder action remains. Linux automatic installation SHALL be revisited only after the Proton/Wine integration is configured and functional — not before.
- **Settings → Downloads**: engine status and version, download folder, rate limits, seeding policy, feature disable, and rqbit license notices.
- **Local state**: persist the consent term version and timestamp, download preferences, torrent-to-entry associations, and install jobs across restarts.
- **Background behavior**: transfers continue while the window is hidden on Windows; Quit stops the engine gracefully and leaves resumable state.

**Out of scope and deferred**: torrent search, indexers, RSS, recommendations, bundled content, qBittorrent support, independent engine auto-updates, deleting downloaded files from the app, and macOS. Linux automatic installation and the Proton/Wine integration are deferred and SHALL NOT be revisited before that integration is configured and functional; consequently, Wine/Proton entries, their playtime tracking and their save backups are also out of scope for this change.

## Capabilities

### New Capabilities

- `desktop-downloads`: opt-in consent, bundled rqbit sidecar lifecycle, Downloads navigation and list/detail surfaces, magnet and torrent intake, progress and transfer actions, completion handoff.
- `desktop-download-install`: installer discovery, per-install confirmation, Windows installer execution and result detection, portable-game detection, and creation of DRM-free entries from completed downloads. Linux automatic installation is explicitly deferred.

### Modified Capabilities

- `desktop-shell`: signed-in navigation becomes Biblioteca plus Downloads, with badge, keyboard access and translated strings.
- `desktop-settings`: new Downloads topic covering engine status, destination, limits, seeding, disable and license notices.
- `desktop-local-state`: persist consent version, download preferences, torrent associations and install records through ordered migrations.
- `desktop-drm-free-games`: local registrations gain download provenance and automatic-install output on Windows.
- `desktop-lifecycle`: transfers continue in the background on Windows, and Quit stops the bundled engine without losing resumable state.

## Impact

- **Core (`crates/core`)**: new `downloads` domain types and ports for the transfer engine, installer jobs and file-manager handoff; `TorrentClient` port kept engine-agnostic.
- **App (`crates/app`)**: sidecar spawn/health/shutdown, authenticated loopback client, transfer event channel, Tauri commands, Windows installer runner, and command/capability scoping for the new surfaces.
- **Platform (`crates/platform`)**: SQLite migrations for consent, preferences, associations and install jobs; process spawning with job-object containment; OS file-manager reveal.
- **UI (`ui/src`)**: shell navigation split, `features/downloads` (consent, list, detail, add dialog, install dialog), Settings Downloads panel, i18n in English and Brazilian Portuguese, Vitest coverage.
- **Build/packaging**: `bundle.externalBin` for rqbit per target triple, CI fetch/verify of pinned rqbit releases, Apache-2.0 notice, code signing considerations for the sidecar.
- **Dependencies**: `rqbit` sidecar binary; no qBittorrent dependency.
- **Docs**: new `docs/downloads.md` covering consent, engine lifecycle, Windows installer execution, and the explicitly deferred Linux/Proton installation.
