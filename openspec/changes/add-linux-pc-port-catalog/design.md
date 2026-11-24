# Design

## Context

`add-pc-port-catalog` delivers the Windows-first catalog and installer. Its catalog model already
carries `supportedOS`, a per-release asset list and per-platform preferred executables. The
upstream catalog contains native Linux builds (AppImages, zips, tarballs), while the original
install engine was Windows-specific: `%APPDATA%`/`%LOCALAPPDATA%` placement, `.exe` identity,
`CREATE_NO_WINDOW`, job objects and a pinned Windows PS4 PKG extractor.

This change was planned in two phases: a read-only "Coming Soon" catalog on Linux, then native
Linux installs. Both phases have partly landed (commits `ac0c981` to `41bfd36`): the service and
the install runner start on Linux, the Coming Soon block is gone, and an AppImage installs end to
end in a runner test. A review on 2026-11-24 compared the artifacts with the code and found gaps
in both directions (see [Review findings](#review-findings-2026-11-24)). The specs now describe the
**final state** only; the phase-1 Coming Soon requirements were removed because the code no longer
has that state and the main specs must not require it after archiving.

## Goals / Non-Goals

**Goals**

- List host-compatible entries on Linux, including Linux-only entries.
- Derive installability from the selected release's assets and the recipe, not from `supportedOS`.
- Filter native Linux versus Windows-only entries; keep blocked entries visible with a reason.
- Install native Linux releases with the same safety model as Windows.
- Keep the existing Windows engine, catalog cache, overlay behavior and acceptance intact.

**Non-Goals**

- Proton, Wine, containers, remote streaming or any Windows-on-Linux execution.
- macOS catalog or installs.
- Re-hosting upstream releases or changing the Quiver source/credit model.
- A Linux PS4 PKG extractor, or running the Windows one.
- Reopening the Windows acceptance tasks in `add-pc-port-catalog`.

## Decisions

### D1. Asset-level availability, with a declared state before release details load

`supportedOS` decides listing and the initial label only. Installability comes from the same
asset selection the plan uses (`select_asset_for`) plus the recipe checks (D13, D14). The catalog
listing carries no release assets — releases are fetched lazily when an entry's details load (see
the lazy-release divergence in `docs/pc-ports.md`). Availability therefore has three states:

| State | Source | UI |
| --- | --- | --- |
| `declared` | `supportedOS` only, details not loaded | platform label, Install enabled only after details confirm |
| `native` | details loaded, a supported asset for the host exists, recipe can run | Install enabled |
| `blocked(reason)` | Windows-only, no Linux asset, unsupported format, console extraction, recipe not reviewed | Install disabled with reason |

`entry_view` must stop using the declared platform as install authority on Linux; today the
`asset_reason` is only computed for Windows (`view.rs`).

### D2. Superseded: phase-1 Coming Soon catalog

The read-only phase shipped and was replaced by native installs in task 2.5. Its requirements are
removed from the deltas. There is no recorded packaged evidence of the phase-1 state (task 1.7 is
marked superseded). The guarantee it gave — no enabled Install that cannot work — is kept by D9.

### D3. Platform filter

The persisted port filters gain a platform field with **All** (default), **Native Linux** and
**Windows-only**, stored in `port_preferences.filters_json`; an unknown value reads as All. The
filter is shown on Linux only. Windows keeps its current toolbar; the proposal's mention of an
"equivalent Windows view" is dropped. Native Linux shows `declared` and `native` Linux entries;
Windows-only shows entries without Linux support and entries blocked for lacking a Linux asset.

### D4. Native Linux install

The existing job runner gains a Linux strategy: Linux asset selection (D8), executable bit (D11),
XDG roots (D16), launch identity (D17), registration as an ordinary DRM-free entry. Cancellation,
journaling, crash recovery and no-partial-registration stay shared. Guided setup runs only for
recipes reviewed for Linux (D14).

### D5. The Windows extractor is not a Linux dependency

The pinned `ps4-pkg` extractor is a Windows `.exe`. It is never fetched, started or emulated on
Linux. A required slot that can only be produced from packages blocks the entry on Linux; a slot
that also accepts an extracted folder offers only the folder on Linux. Today `plan_install`
refuses only a `Packages` source at plan time (`plan.rs`), so the block is not visible in the
catalog — D13 moves it into availability.

### D6. Proton is future work

Unchanged. The blocked reason and the availability are separate so a later change can add a
Proton strategy without migrating data.

### D7. Windows behavior is frozen

Unchanged. Every Linux fix must keep the Windows tests; `select_asset` stays the Windows entry
point, and the format-before-architecture fix (D8) must keep Windows' current choices in its tests.

### D8. Asset selection: OS, filter, format, then architecture

`select_asset_for` currently ranks by architecture before checking the format, so a release with
`x86_64.tar.gz` and `x86_64.AppImage` can pick the tar and answer `UnsupportedFormat`. The order
becomes: OS → recipe asset filter → supported format → architecture. Supported Linux formats are
AppImage, bare executable, zip and 7z. `.run` is either added to `SUPPORTED_LINUX_FORMATS` and
`linux_asset::kind` or removed from `detect_asset_type`; the three lists must agree. "Bare
executable" must cover names without a known extension (for example Godot's `game.x86_64`), with a
rule that does not misread arbitrary files. Tar support stays open (task 2.1); until then tar is
rejected explicitly, and the design no longer promises it.

### D9. No enabled Install without a working folder choice

Folder and file pickers are Windows-only today (`pick_folder_starting_at` returns
`Unsupported`), so the Linux setup modal shows Install but can never start. Until task 2.7 lands,
Linux Install is disabled with a translated "folder picker unavailable" reason (or 2.7 lands
first). 2.7 adds an XDG desktop portal / native dialog picker (the Tauri dialog plugin is
acceptable) behind `ports_pick_parent`, `ports_pick_supplied` and `ports_pick_executable`, plus a
typed absolute-path fallback validated by the host.

### D10. The detail pane uses the platform's asset

`build_detail`, `release_view` and `no_asset_reason` call the Windows `select_asset` and receive no
platform. On Linux they show the Windows file's name, size, checksum and malware warning, and the
`PortSetupModal` malware gate evaluates the wrong file. They take the `PortPlatform` and call
`select_asset_for`, so the pane, the modal and the plan agree.

### D11. Executable bit after extraction

The extractor does not apply the zip `unix_mode`, and 7z records none, while `linux_identity`
rejects a file without an execute bit — so a zip/7z Linux port ends in `InvalidTarget`. After
extraction on Linux the runner applies the zip's recorded mode where present and always sets
`0o755` on the executable it registers (preferred, picked or fallback).

### D12. Executable discovery on Linux

`find_executable` compares the preferred name exactly and case-sensitively; upstream AppImages
carry version and case in the name. On Linux the match ignores case, and when the installed asset
is a single AppImage or bare executable and nothing matches, that file is the executable without
asking. Archives keep the existing "pick the executable" fallback, which needs D9's picker.
*Proposed; needs confirmation.*

### D13. Overlay is platform-aware and does not filter by Windows

`Overlay::apply` filters merged entries with `CatalogEntry::supports_windows`, so Linux-only
entries never reach the Linux catalog and the Windows-only branch of `entry_view` is never really
exercised. The platform rule moves to the view (Windows lists Windows entries; Linux lists Linux or
Windows entries). `launcher_executable` and `gameplay_executables` gain per-platform values
(*proposed shape*: `launcher_executable: { windows, linux }`, `gameplay_executables: { windows,
linux }`, with the old scalar fields read as Windows). Linux never uses a Windows name.

### D14. Linux guided setup requires a reviewed recipe

Overlay recipes are Windows-authored. A recipe with required setup files runs on Linux only when
the overlay marks it reviewed for Linux (*proposed field*: `linux_reviewed: true`); otherwise the
entry is blocked with the not-reviewed reason in availability (D1). Release-only entries need no
review. The final overlay schema is decided in task 3.8.

### D15. Rollback switch

Forcing the platform filter to Windows-only does not block installs after native installs exist.
Rollback is: make `PortPlatform::supports_installs()` false on Linux (the service then serves the
catalog read-only and refuses installs, as task 1.2 tested), or hide the destination. Installed
Linux entries are ordinary DRM-free entries and keep working either way.

### D16. Linux data roots follow XDG

`CopyRoots::host` maps `data` and `config` to `dirs::config_dir()` and `localData` to
`dirs::data_local_dir()`, which copies the Windows `%APPDATA%`/`%LOCALAPPDATA%` split. On Linux
that puts `data` under `~/.config`, where native ports such as devilutionX do not look. Proposed
Linux mapping: `config` → `$XDG_CONFIG_HOME`, `data` and `localData` → `$XDG_DATA_HOME`. Windows
keeps its mapping. *Proposed; needs confirmation (task 3.10).*

### D17. Linux launch identity

`linux_identity` records device, inode, mode, size, mtime and ctime, so any in-place self-update,
`touch` or `chmod` refuses the launch. That breaks the base requirement "Self-updating ports are
left alone" more than Windows does (Windows uses volume + file index only), and contradicts the
`docs/pc-ports.md` claim that in-place rewrites keep working. Recommended: compare device + inode
only and check the execute bit separately at launch; a replaced file goes through the existing
repair flow. *Recommended; needs confirmation (task 3.11). The install spec states the behavior.*

## Review findings (2026-11-24)

| # | Finding | Decision | Task |
| --- | --- | --- | --- |
| 1 | Overlay drops Linux-only entries (`supports_windows` filter) | D13 | 3.1 |
| 2 | zip/7z Linux ports fail validation (no execute bit) | D11 | 3.5 |
| 3 | Detail pane and malware gate use the Windows asset | D10 | 3.3 |
| 4 | Architecture ranked before format | D8 | 3.4 |
| 5 | Install enabled on Linux with no folder picker | D9 | 3.2, 2.7 |
| 6 | Availability from `supportedOS`, not assets (1.1 open) | D1 | 1.1 |
| 7 | Windows-authored recipes run on Linux | D14, D5 | 2.3, 3.8 |
| 8 | Overlay launcher/gameplay not per platform | D13 | 3.7 |
| 9 | Exact, case-sensitive executable match | D12 | 3.6 |
| 10 | `data` mapped to `~/.config`, not XDG data | D16 | 3.10 |
| 11 | Linux identity includes size and timestamps | D17 | 3.11 |
| 12 | Format lists disagree (`.run`, bare binaries, tar, 7z) | D8 | 3.9, 2.1 |
| 13 | Deltas added requirements contradicting the base | specs use MODIFIED/RENAMED | 4.1 |
| 14 | Specs described the removed Coming Soon state | D2 | 4.1 |
| 15 | Task 1.7 no longer verifiable; 2.2 closed without launch | D2 | 1.7, 2.2 |
| 16 | Proposal vs. D3 on a Windows platform filter | D3 | 4.2 |
| 17 | Rollback did not block installs | D15 | 4.2 |
| 18 | Base change not archived | dependency | 4.3 |
| 19 | `docs/pc-ports.md` and `docs/linux-verification.md` stale | docs | 4.2, 3.12 |
| 20 | Duplicate `cfg(not(any(windows, linux)))` block breaks non-Windows/non-Linux builds | — | 3.12 |

## Risks / Trade-offs

- **Upstream inconsistency** (`supportedOS` case or an OS claim without an asset) → D1's declared
  state is shown until details confirm; Install is never enabled from the claim alone.
- **Linux assets vary** → install only the formats in D8, reject others explicitly.
- **Windows-authored overlay recipes** → D14; unreviewed recipes stay blocked.
- **Picker availability differs across desktops** → portal first, typed-path fallback (D9).
- **Identity relaxation** (D17) accepts any in-place change to the executable; the user still
  launches a file they installed, and replacement still needs repair.
- **Windows regression** → D7; the Windows tests stay as they are.
- **No packaged Linux acceptance yet** → task 2.6 records it separately; fixtures alone are not
  native acceptance.

## Migration Plan

1. Archive `add-pc-port-catalog` first (its 10.1 and 10.2 are still open), then this change.
2. Fix the blockers before calling Linux installs usable: D13 listing, D1 availability, D9 picker
   (or a disabled Install), D10 detail asset.
3. Install correctness: D8, D11, D12; then D14, D13 overlay fields, D16, D17.
4. Packaged Linux acceptance (2.6), recorded in `docs/linux-verification.md`.
5. Rollback: D15.

## Open Questions

- D16 data mapping and D17 identity — confirm before tasks 3.10 and 3.11.
- D12 single-file fallback and the D13/D14 overlay field shapes — confirm before 3.6–3.8.
- Tar support (2.1): in this change or a follow-up.
