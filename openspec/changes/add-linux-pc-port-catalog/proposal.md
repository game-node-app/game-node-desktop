# Proposal

## Why

The upstream PC ports catalog already contains entries with native Linux builds, but the
destination was Windows-only: `PortPlatform::supports_installs()` was Windows-only, the sidebar
hid PC ports on other builds, and the overlay dropped entries without Windows support. Linux users
could not discover native ports or understand why a Windows-only entry cannot be installed.

This change makes PC ports a working destination on Linux: it lists host-compatible entries,
derives installability from the selected release rather than the coarse `supportedOS` claim, lets
the user filter native Linux and Windows-only entries, and installs native Linux builds with the
same safety model as Windows. Windows-only entries stay visible but blocked. Proton is a future
proposal and is not part of this change.

The change was first planned in two phases (a read-only "Coming Soon" catalog, then native
installs). Both have partly landed and the Coming Soon state was removed. The artifacts were
reconciled with the code on 2026-11-24 and now describe the final state; the remaining gaps are
tracked as tasks.

## What Changes

- **Catalog on Linux.** A signed-in Linux build shows `EXPLORE → PC ports`. The catalog lists
  entries that support Linux or Windows, including Linux-only entries; macOS stays out of scope.
- **Asset-level availability.** An entry is installable on a platform only when the selected
  release has a supported asset for that platform and the recipe can run there. Because releases
  load lazily, an entry is shown as *declared* from `supportedOS` until its details confirm it as
  *native* or *blocked* with a reason. `supportedOS` is never install authority.
- **Platform filter on Linux.** A persisted filter with **All** (default), **Native Linux** and
  **Windows-only**. Windows keeps its current toolbar.
- **Blocked entries.** Windows-only, no-Linux-asset, unsupported-format, console-extraction and
  not-reviewed-for-Linux entries stay browsable with a translated reason and never start install
  work.
- **Detail pane shows the platform's asset.** Name, size, checksum and malware warning come from
  the asset the Linux install would use, not the Windows one.
- **Native Linux installs.** AppImage, bare executable, zip and 7z assets (format checked before
  architecture); executable bit after extraction; native folder and file pickers with a typed-path
  fallback; per-platform overlay executables; XDG data roots; a launch identity that survives
  in-place updates; guided setup only for recipes reviewed for Linux. The Windows PS4 PKG extractor
  is never used on Linux.
- **No change to Windows behavior.** Windows keeps its catalog, install engine and acceptance.
  Proton, Wine and remote streaming are future work.

## Capabilities

### Modified Capabilities

- `desktop-port-catalog`: host-compatible listing (renamed from Windows-compatible), the detail
  pane's platform asset, asset-level availability, the Linux platform filter and blocked entries.
- `desktop-port-install`: platform asset selection and malware confirmation, folder pickers on
  Linux, per-platform executables, Linux formats and XDG roots, Linux launch identity, reviewed
  Linux recipes and the refusal of Windows-only tools.
- `desktop-shell`: the `EXPLORE` group is shown on Windows and Linux.

## Impact

- **Core (`crates/core/src/ports`)**: overlay platform filtering and per-platform executables,
  availability in the view model, asset selection order.
- **Platform (`crates/platform`)**: Linux pickers, executable bit after extraction, XDG roots,
  Linux launch identity, format detection.
- **App (`crates/app`)**: the PC ports service on Linux, platform-aware detail and availability.
- **UI (`ui/src/features/ports`, sidebar, i18n, mocks)**: platform filter, blocked reasons, typed
  folder fallback and Linux fixtures.
- **Data**: the persisted catalog preferences gain the platform filter; the overlay schema gains
  per-platform executables and a Linux-review mark. Existing snapshots and installs are preserved.
- **Dependency**: `add-pc-port-catalog` must be archived first; this change modifies its
  Windows-only requirements.
- **Out of scope**: Proton/Wine execution, macOS installs, a Linux PS4 package extractor, Linux
  distribution packaging, and any change to the upstream catalog source or the Windows acceptance
  tasks.
