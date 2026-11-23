# Tasks

This change depends on `add-pc-port-catalog`. Do not archive it before that change is archived: its
requirements extend the same `desktop-port-catalog`, `desktop-port-install` and `desktop-shell`
capabilities. Windows behavior and the existing Windows acceptance are not reopened.

## 1. Phase 1 — Coming Soon catalog on Linux

- [ ] 1.1 Derive per-entry platform availability from the selected release's verified assets and
  the recipe's tool requirements, not from `supportedOS` alone. Expose native/blocked/unknown and
  the reason in the catalog view model. Verify Linux-only, Windows-only, mixed and inconsistent
  upstream claims (an OS listed without an asset) produce the expected availability.
- [x] 1.2 Start the PC ports catalog service on signed-in Linux builds in read-only mode while the
  install service stays Windows-only. Keep the existing Windows catalog path unchanged.
  - Completion (2026-11-22): `main.rs` now starts `PortsService` on every platform and builds
    `InstallDeps` only where `supports_installs()` is true, so Linux gets the catalog worker with no
    install runner. `install`, `prepare_install`, `pick_executable` and `clear_cache` still return
    `Unsupported`; the plugin routes report `UNAVAILABLE` before any job starts. Windows keeps the
    same construction. Regression: `engine::tests::pc_ports::linux_serves_the_catalog_read_only_and_refuses_installs`.
- [x] 1.3 Show `EXPLORE → PC ports` on signed-in Linux builds with an “Em breve”/“Coming Soon”
  label on the destination and in the catalog header. Verify signed-out and non-Linux builds keep
  their existing visibility.
  - Completion (2026-11-22): the sidebar shows PC ports for `windows` and `linux`, with a
    `sidebar-ports-coming-soon` badge on Linux; `App.tsx` renders `PortsView` on both platforms
    (the original gate hid the destination after the click). The catalog header carries
    `banner-ports-coming-soon`. Signed-out and macOS builds keep the previous visibility. Tests:
    `Sidebar.test.tsx`, `App.integration.test.tsx` (`opens the read-only PC ports catalog on Linux`).
- [x] 1.4 Add the persisted platform filter **All** (default), **Native Linux** and
  **Windows-only** on Linux. Verify the filter survives restart, an unknown value falls back to
  All, and Windows filters are unchanged.
  - Completion (2026-11-22): `PortPlatformFilter` is stored in `PortPreferences` and inside the
    existing `port_preferences.filters_json`, with an unknown value reading as `All`; the toolbar
    select exists only on Linux (the read-only phase) and Windows keeps its previous controls.
    Filtering uses the entry's declared `supportedOS`; installs stay blocked. Tests: platform
    store round-trip plus legacy/unknown fallback and the UI platform filter.
- [x] 1.5 Keep every entry browsable on Linux in phase 1, but disable Install for all of them with
  the Coming Soon reason. The detail pane shows available platforms and the phase note. Verify no
  download, extractor, launch or library registration is started from a blocked card; Windows-only
  cards explain that Proton is future work.
  - Completion (2026-11-22): `PortInstallBlockReason::ComingSoon` overrides every other reason on a
    platform without an install runner, so a verified Windows-capable entry is still blocked; the
    detail pane shows a Platforms row and the translated note. No setup, download, extractor or
    registration starts because the button is disabled and the service refuses the request.
    Untested here: the Proton note stays phase 2, when native installs exist.
- [x] 1.6 Add en and pt-BR strings, `VITE_UI_MOCK=1` fixtures for native Linux, Windows-only and
  mixed entries, and component tests for the Coming Soon label, filter, blocked card and detail
  note.
  - Completion (2026-11-22): en/pt-BR cover the label, banner, platform filter and platform names.
    The mock catalog has a mixed entry (`Aster Vale`), a native Linux entry (`Beyond the Pines`) and
    Windows-only entries, reachable with `VITE_UI_MOCK=1` plus `?platform=linux`. Component tests
    cover the label, banner, filter, blocked install and Platforms row; the i18n parity test allows
    the proper-noun platform names.
- [ ] 1.7 Run a native Linux packaged-host check: open PC ports, see Coming Soon, list the cached
  catalog, filter native Linux and Windows-only, and confirm no install/network download starts.
  Record the evidence as a packaging check.

## 2. Phase 2 — native Linux installs

- [ ] 2.1 Select a native Linux release asset (AppImage, tar, zip or bare executable where the
  recipe allows it), verify its checksum and normalize the executable bit. Reject Windows-only
  assets and unsupported formats explicitly.
  - Progress (2026-11-22): `select_asset_for(platform, ...)` in `crates/core/src/ports/plan.rs`
    picks a Linux asset (AppImage, zip, 7z or a bare binary), applies the recipe asset filter,
    ranks architecture, and refuses Windows-only releases (`NoLinuxAsset`), unsupported formats
    (`UnsupportedFormat`, including `.deb` and tar until the in-process tar extractor exists) and
    macOS (`UnsupportedPlatform`); `select_asset` stays the Windows entry point, so its tests and
    behavior are unchanged. The Linux plan selects its own preferred executable, rejects console
    packages, and carries its platform. The job runner copies an AppImage/bare binary into the
    install folder and normalizes its executable bit, and `SystemExecutableIdentity` records the
    Linux identity the launch validation compares. An end-to-end runner test installs an AppImage
    and asserts the file, mode, and registration. Remaining for the task: tar support. Phase 2.2
    and 2.5 (enabling the Linux service and per-entry availability in the UI) are still open.
- [x] 2.2 Map `app`, `data`, `config` and `localData` to XDG placement roots, register the result
  as an ordinary DRM-free entry, and validate the Linux launcher/executable before launch.
  - Completion (2026-11-22): `UserDataRoots::host`/`CopyRoots::host` already resolve XDG
    (`dirs::config_dir` and `data_local_dir`), and the Linux plan uses the recipe's
    `preferredExecutables.linux`. `SystemExecutableIdentity` now records the same
    device/inode/mode/timestamp identity `LinuxInstallations::validate_registration` compares, so
    the registration written at the commit point launches after a restart. The end-to-end runner
    test asserts the registration and the executable; the native launch itself is part of 2.6.
- [ ] 2.3 Support Linux guided setup only for overlay recipes explicitly reviewed for Linux. A
  recipe requiring the Windows PS4 PKG extractor is reported unavailable; never fetch or run that
  tool on Linux.
- [x] 2.4 Extend the port job runner for Linux assets and placement: cancellation, crash recovery,
  no partial registration, journaled paths and no game-file deletion. Keep the Windows branch
  unchanged.
  - Completion (2026-11-22): the runner detects AppImage/bin/run as a bare executable, copies it
    into staging, sets `0o755`, and keeps the shared journaling, cancellation and startup
    recovery paths unchanged. `ports::linux_asset` classifies executable versus archive formats.
    The end-to-end test `a_native_linux_appimage_installs_and_becomes_executable` covers download,
    checksum, placement, mode and registration; `beyond_the_pines`-style archive installs reuse the
    existing zip/7z extractor. Windows tests are unchanged.
- [x] 2.5 Remove the Coming Soon block for native Linux entries; keep Windows-only entries blocked
  with their own reason. Add en/pt-BR strings, mocks and tests for the mixed catalog.
  - Completion (2026-11-22): `supports_installs` now covers Windows and Linux, so the service
    starts the install runner on Linux. `entry_view` blocks only entries that do not declare the
    host platform, with `PortInstallBlockReason::WindowsOnly` and the Proton note; the Coming Soon
    reason, sidebar badge and catalog banner are gone. The platform filter stays on Linux. The
    plan's `NoLinuxAsset` has its own user-facing message. Tests cover a native Linux entry, a
    Windows-only entry on Linux, the sidebar, the app destination, the filter and the blocked
    install.
- [ ] 2.6 Run focused Linux install fixtures and a packaged Linux acceptance with at least one
  native port built from the user's own files. Record the evidence separately from Windows
  acceptance.

## 3. Future

- Proton/Wine execution, Windows-only installs on Linux, macOS catalog/installs and a Linux PS4
  package extractor are future proposals. They are not tasks in this change.
