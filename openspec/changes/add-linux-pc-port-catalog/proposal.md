# Proposal

## Why

The PC ports catalog is useful on Linux even before native installs exist: the upstream catalog
already contains entries with native Linux builds, and hiding the whole destination prevents Linux
users from discovering them or understanding why a Windows-only entry cannot be installed. Today
`PortPlatform::supports_installs()` is Windows-only and the sidebar hides PC ports on non-Windows
builds, so the restriction is partly a presentation decision rather than a catalog limitation.

This change exposes the catalog on Linux, distinguishes native Linux assets from Windows-only
entries using the selected release rather than the coarse `supportedOS` claim alone, and lets the
user filter for native Linux entries. Windows-only entries stay visible but blocked, with an
explicit explanation. Native Linux installation is delivered in a second phase; Proton is a future
proposal and is not part of this change.

## What Changes

- **Catalog visibility on Linux, labelled Coming Soon.** A signed-in Linux build shows
  `EXPLORE → PC ports` with an “Em breve”/“Coming Soon” label on the destination and in the
  catalog header, and can browse, search, filter and open the detail pane read-only. Every entry
  remains blocked for installation on Linux in phase 1; listing is the delivered value.
- **Asset-level platform availability.** An entry is installable on a platform only when the
  selected release has a verified asset for that platform and the recipe does not require an
  unavailable tool. `supportedOS` remains a discovery hint, not install authority.
- **Platform filter.** The toolbar gains a persisted platform filter with **All**, **Native
  Linux** and **Windows-only** on Linux (and the equivalent Windows view on Windows). The default
  is All: Windows-only cards are visible but blocked.
- **Blocked Windows-only cards.** On Linux, Windows-only entries show a “Windows-only” badge, an
  Install action that is disabled, and a detail explanation that Proton is not supported yet.
- **Native Linux installation (phase 2).** Phase 1 keeps every Install action disabled with the
  Coming Soon reason. Phase 2 selects native Linux assets, installs under XDG roots, registers
  ordinary DRM-free entries and validates them before launch; Windows-only entries stay blocked
  with a Proton note. The Windows PS4 PKG extractor is never used on Linux; recipes that require
  it are reported unavailable.
- **No change to Windows behavior.** Windows keeps its existing catalog, install engine and
  acceptance. macOS remains out of scope. Proton, Wine and remote streaming are future work and
  are not promised by this change.

## Capabilities

### Modified Capabilities

- `desktop-port-catalog`: Linux catalog visibility, asset-level platform availability, the
  platform filter and blocked Windows-only entries.
- `desktop-port-install`: native Linux asset selection, XDG placement, Linux launch validation and
  the explicit refusal of Windows-only or Windows-extractor recipes on Linux.
- `desktop-shell`: the PC ports destination is visible on signed-in Linux builds.

## Impact

- **Core (`crates/core/src/ports`)**: platform availability on the catalog view, the platform
  filter in the view model and Linux install-plan semantics.
- **Platform (`crates/platform/src/ports`)**: asset selection by OS, Linux placement roots and
  launch file identity; the extractor remains Windows-only and is excluded for Linux recipes.
- **App (`crates/app`)**: the PC ports service and commands run on Linux in read-only catalog mode
  in phase 1 and expose native installs in phase 2.
- **UI (`ui/src/features/ports`, sidebar, i18n, mocks)**: platform filter, blocked cards, detail
  explanations and Linux fixtures.
- **Data**: the persisted catalog preferences gain the platform filter; existing snapshots and
  installed entries are preserved. No game files are migrated.
- **Dependency**: `add-pc-port-catalog` must be archived first. This change adds Linux
  requirements to the same capabilities and does not reimplement the Windows engine.
- **Out of scope**: Proton/Wine execution, macOS installs, Linux distribution packaging, and any
  change to the upstream catalog source or the Windows acceptance tasks.
