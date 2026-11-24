# Design

## Context

`add-pc-port-catalog` delivers the Windows-first catalog and installer. Its catalog model already
carries `supportedOS`, a per-release asset list and per-platform preferred executables, but the
host service only starts on Windows and the sidebar hides the destination elsewhere. The upstream
catalog contains native Linux builds (for example entries with Linux AppImages or tarballs), while
the install engine is Windows-specific: `%APPDATA%`/`%LOCALAPPDATA%` placement, `.exe` identity,
`CREATE_NO_WINDOW`, job objects and a pinned Windows PS4 PKG extractor.

The product decision is to make discovery available on Linux without pretending that every entry
is installable there. Windows acceptance remains unchanged.

## Goals / Non-Goals

**Goals**

- Show the catalog on signed-in Linux builds without enabling unsafe or non-functional installs.
- Derive installability from the selected release assets and recipe requirements.
- Filter native Linux versus Windows-only entries; keep Windows-only entries visible and blocked.
- Install native Linux releases in phase 2 under XDG roots with Linux launch validation.
- Keep the existing Windows engine, catalog cache and overlay behavior intact.

**Non-Goals**

- Proton, Wine, containers, remote streaming or any Windows-on-Linux execution.
- macOS catalog or installs.
- Re-hosting upstream releases or changing the Quiver source/credit model.
- Replacing the Windows PS4 PKG extractor or inventing game content.
- Reopening the Windows acceptance tasks in `add-pc-port-catalog`.

## Decisions

### D1. Asset-level availability, not `supportedOS` alone

`supportedOS` is a discovery hint. An entry is eligible for installation on a platform only when
the selected release contains a verified asset for that platform (matching the recipe's format
filters) and the recipe does not require a tool that is unavailable on that platform. The view
model exposes a per-entry `PlatformAvailability` with `native`/`blocked`/`unknown` and the reason.
This keeps an inconsistent upstream claim from turning into an install attempt.

### D2. Phase 1 is a read-only “Coming Soon” catalog on Linux

In phase 1 the host starts the catalog worker on Linux, but the install service remains
Windows-only. The sidebar shows PC ports on signed-in Linux builds with an “Em breve”/“Coming
Soon” label, and the catalog header repeats the phase note. The catalog, search, filters and detail
pane are fully browsable, but the Install action is disabled for every entry on Linux in phase 1,
native or not, with the Coming Soon reason. This delivers discovery without a half-implemented
install engine or a misleading install button.

Phase 2 removes the Coming Soon block for native Linux entries and leaves Windows-only entries
blocked with their own reason (Proton is future work). The sidebar label changes to a normal
destination; the catalog header keeps a note only for entries that remain blocked.

### D3. Platform filter and blocked cards

The existing persisted port filters gain a platform field:

- **All** (default): every entry the catalog returns. In phase 1 every card is blocked on Linux
  with the Coming Soon reason; in phase 2 only Windows-only cards stay blocked.
- **Native Linux**: only entries with a Linux asset and a Linux-compatible recipe. In phase 1 the
  cards are still browsable but blocked with Coming Soon; in phase 2 they become installable.
- **Windows-only**: only entries without a Linux asset, so the user can see what is not available
  yet without mistaking it for a broken catalog (Proton is future work).

The filter is persisted in `port_preferences` with the existing filter revision; an unknown value
falls back to All. Windows keeps its existing catalog and filters; this change adds the platform
filter to Linux only, so Windows behavior and acceptance remain unchanged.

A blocked card remains selectable. Its detail pane shows the available platforms and why Install is
disabled: the Coming Soon note in phase 1, or the Windows-only/Proton note in phase 2. Install
never starts a download or extractor for a blocked entry.

### D4. Native Linux install phase

Phase 2 uses the existing job runner with a Linux platform strategy:

- select a Linux asset (AppImage, tar, zip or a bare executable; reject Windows-only assets);
- normalize the executable bit and validate the executable/launcher before registration;
- map `app`, `data`, `config` and `localData` to XDG placement roots instead of `%APPDATA%`/
  `%LOCALAPPDATA%`;
- register the result as an ordinary DRM-free entry with the same provenance and playtime model;
- keep the release-only path first. Guided setup is available only when the overlay recipe is
  explicitly Linux-compatible. A recipe requiring `from_package` with the Windows PS4 extractor is
  reported unavailable, not run under Wine.

### D5. The Windows extractor is not a Linux dependency

The pinned `ps4-pkg` extractor is a Windows `.exe` with `CREATE_NO_WINDOW` and a Windows job
object. It must not be fetched, started or emulated on Linux. Linux entries that need a console
package extraction require a separate reviewed Linux tool and recipe in a future change; until
then those entries remain blocked with an explicit reason.

### D6. Proton is future work

Proton/Wine execution is outside this change. The model keeps the blocked reason and the platform
availability separate so a later change can add a `Proton` installation strategy without
migrating catalog data or changing the Windows engine. No Proton setting, dependency or launch
recipe is added here.

### D7. Windows behavior is frozen

The Windows catalog, filters, install engine and acceptance stay as delivered by
`add-pc-port-catalog`. Linux changes must not alter Windows asset selection, placement or launch
semantics. Shared code can grow a platform strategy, but the Windows branch must keep its current
tests and acceptance evidence.

## Risks / Trade-offs

- **Upstream inconsistency** (`supportedOS` values/case; an OS claim without an asset) → derive
  availability from the release and test the mismatch cases.
- **Linux assets vary** (AppImage, tar, zip, scripts) → start with the formats the upstream
  actually publishes; reject unsupported formats explicitly instead of extracting arbitrary
  archives.
- **Overlay recipes are Windows-authored** → phase 2 must not assume a `setup_file` recipe works
  on Linux; unverified recipes remain blocked.
- **Read-only phase 1 looks like a bug** → make the disabled Install state and the phase note
  explicit in the card and detail pane; add translations and tests.
- **Windows regression** → keep the platform strategy behind a boundary and run the existing
  Windows tests plus the Linux catalog tests.
- **No Linux packaged acceptance available yet** → phase 2 ends with fixtures and a documented
  packaged-Linux acceptance handoff, not a claim of native execution from fixtures alone.

## Migration Plan

1. Land the base `add-pc-port-catalog` change and archive it.
2. Phase 1: add platform availability to the catalog view, start the read-only catalog service on
   Linux, add the filter and blocked cards, preserve existing Windows behavior.
3. Phase 2: add the Linux install strategy, XDG placement and launch validation; keep Windows
   blocked Linux-only entries symmetrical.
4. Rollback: hide the destination or force the platform filter back to Windows-only; the Windows
   install path and existing Linux library entries are unaffected either way.
