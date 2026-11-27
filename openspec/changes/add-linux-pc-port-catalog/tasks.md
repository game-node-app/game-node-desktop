# Tasks

This change depends on `add-pc-port-catalog`. Do not archive it before that change is archived: its
requirements extend the same `desktop-port-catalog`, `desktop-port-install` and `desktop-shell`
capabilities. Windows behavior and the existing Windows acceptance are not reopened.

Review 2026-11-24: the artifacts were reconciled with the code (see the design's review-findings
table). Tasks 2.2, 2.4 and 2.5 were reopened, 1.7 was superseded, and sections 3 and 4 were added.

## 1. Phase 1 — Linux catalog (Coming Soon state superseded by 2.5)

- [x] 1.1 Derive per-entry platform availability from the selected release's verified assets and
  the recipe's tool requirements, not from `supportedOS` alone. Expose declared/native/blocked and
  the reason in the catalog view model (design D1): `declared` until the lazy release details
  load, then `native` or `blocked`. Pass the platform into `entry_view`'s asset reason instead of
  computing it only on Windows, and enable Linux Install only from `native`. Verify Linux-only,
  Windows-only, mixed and inconsistent upstream claims (an OS listed without an asset) produce the
  expected availability.
  - Review note (2026-11-24): 1.4, 1.5 and 2.5 were completed on top of the declared
    `supportedOS`; their filtering and blocking must be rechecked once this lands.
  - Completion (2026-11-26): `PortAvailability` (declared/native/blocked) is in the catalog view.
    `entry_view` takes `Option<&EntryFacts>`: Linux is `declared` with Install off until the detail
    loads, then `native` or `blocked` from the platform's selected asset; Windows keeps its
    listing-time optimism (D7). The no-Linux-asset reason is its own value, and the Linux filter
    moves an entry declaring Linux without a Linux asset to Windows-only. Recipe/tool blocking
    arrives with 3.8. Tests: `linux_is_declared_until_the_release_details_load`,
    `a_declared_linux_entry_without_a_linux_asset_is_blocked`,
    `windows_stays_optimistic_before_the_details_load`, and the UI's `marks an entry as declared
    until its release details load` plus the platform-filter case.
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
- [x] 1.7 ~~Run a native Linux packaged-host check of the Coming Soon state.~~
  - Superseded (2026-11-24): task 2.5 removed the Coming Soon state, so it can no longer be
    observed. **No packaged evidence of the phase-1 state was recorded.** Packaged Linux evidence
    is now covered only by 2.6.

## 2. Phase 2 — native Linux installs

- [x] 2.1 Select a native Linux release asset (AppImage, tar, zip or bare executable where the
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
  - Completion (2026-11-26): `tar`, `tar.gz` and `tgz` are selected and extracted in process with
    the same traversal, special-entry and 16 GiB cap rules as zip/7z; the Linux install applies
    the recorded tar mode and the runner normalizes the executable. Unsupported package formats
    (`.deb`, `.rpm`) and macOS stay refused. Tests:
    `tar_and_tar_gz_archives_extract`, `a_linux_tar_gz_release_plans_its_archive` and
    `a_linux_tar_gz_port_without_a_recorded_mode_registers`.
- [ ] 2.2 Map `app`, `data`, `config` and `localData` to XDG placement roots, register the result
  as an ordinary DRM-free entry, and validate the Linux launcher/executable before launch.
  - Completion (2026-11-22): `UserDataRoots::host`/`CopyRoots::host` already resolve XDG
    (`dirs::config_dir` and `data_local_dir`), and the Linux plan uses the recipe's
    `preferredExecutables.linux`. `SystemExecutableIdentity` now records the same
    device/inode/mode/timestamp identity `LinuxInstallations::validate_registration` compares, so
    the registration written at the commit point launches after a restart. The end-to-end runner
    test asserts the registration and the executable; the native launch itself is part of 2.6.
  - Reopened (2026-11-24): the mapping is not XDG — `data` and `config` both resolve to
    `~/.config` and only `localData` to `~/.local/share` (design D16, task 3.10). Launch
    validation was never exercised (it moved to 2.6), and the identity it compares breaks on
    in-place updates (D17, task 3.11).
- [x] 2.3 Support Linux guided setup only for overlay recipes explicitly reviewed for Linux. A
  recipe requiring the Windows PS4 PKG extractor is reported unavailable; never fetch or run that
  tool on Linux.
  - Completion (2026-11-26): the `linux_reviewed` mark and the `packages_only` slot rule from 3.8
    block unreviewed and package-only recipes in availability; the plan refuses a package-only
    slot and any `Packages` source on Linux before download, and the setup modal never offers the
    Windows extractor there. No extractor is fetched or run on Linux.
- [x] 2.4 Extend the port job runner for Linux assets and placement: cancellation, crash recovery,
  no partial registration, journaled paths and no game-file deletion. Keep the Windows branch
  unchanged.
  - Completion (2026-11-26): the runner detects AppImage/bin/run as a bare executable, copies it
    into staging, sets `0o755`, and keeps the shared journaling, cancellation and startup
    recovery paths unchanged. `ports::linux_asset` classifies executable versus archive formats.
    The end-to-end test `a_native_linux_appimage_installs_and_becomes_executable` covers download,
    checksum, placement, mode and registration; `beyond_the_pines`-style archive installs reuse the
    existing zip/7z extractor. Windows tests are unchanged.
  - Reopened (2026-11-24): archive installs were never tested end to end and do not work — the
    extractor ignores the zip `unix_mode`, 7z records none, and `linux_identity` rejects a file
    without an execute bit, so a zip/7z Linux port ends in `InvalidTarget` (design D11, task 3.5).
    `.run` is detected here but is not a supported Linux format (task 3.9).
  - Closed (2026-11-26): 3.5 applies the recorded zip mode and the runner sets `0o755` before the
    identity is read; `a_linux_zip_port_without_a_recorded_mode_registers` and
    `a_linux_7z_port_without_a_recorded_mode_registers` cover both archive formats end to end.
    3.9 made `.run` unsupported in all three format lists.
- [x] 2.5 Remove the Coming Soon block for native Linux entries; keep Windows-only entries blocked
  with their own reason. Add en/pt-BR strings, mocks and tests for the mixed catalog.
  - Completion (2026-11-22): `supports_installs` now covers Windows and Linux, so the service
    starts the install runner on Linux. `entry_view` blocks only entries that do not declare the
    host platform, with `PortInstallBlockReason::WindowsOnly` and the Proton note; the Coming Soon
    reason, sidebar badge and catalog banner are gone. The platform filter stays on Linux. The
    plan's `NoLinuxAsset` has its own user-facing message. Tests cover a native Linux entry, a
    Windows-only entry on Linux, the sidebar, the app destination, the filter and the blocked
    install.
  - Reopened (2026-11-24): blocking uses the declared `supportedOS`, not the release (1.1); the
    overlay still drops Linux-only entries, so the native-Linux path mostly sees mixed entries
    (3.1); and Install is enabled while the folder picker cannot work (2.7, 3.2), which regresses
    the phase-1 guarantee of no misleading Install.
  - Closed (2026-11-26): 1.1 derives availability from the selected asset, 3.1 lists Linux-only
    entries, and 3.2 keeps Install off until the folder picker lands. The mixed-catalog strings,
    mocks and tests from the original completion still hold.
- [ ] 2.6 Run focused Linux install fixtures and a packaged Linux acceptance with at least one
  native port built from the user's own files. Record the evidence separately from Windows
  acceptance.

- [x] 2.7 **Linux folder and file pickers (acceptance blocker, documented 2026-11-22).** The setup
  modal's **Escolher pasta** does nothing on Linux, so a native install cannot choose its
  destination and **Iniciar instalação** stays unavailable.
  - Symptom: with `diablo-devilutionx` (Diablo, 1.5.5) and `diablo-iii-reaper-of-souls-...-sanctuaryport`
    (Diablo III, v0.7.0), the Install setup shows **Nenhuma pasta escolhida**; pressing
    **Escolher pasta** opens no dialog. Reproduced with both entries, so it is not entry-specific.
  - Cause: `gamenode_platform::installations::pick_folder_starting_at` is implemented only under
    `#[cfg(windows)]` and returns `LocalError::Unsupported` on every other platform; the command
    turns that into `PortCommandError::UnsupportedPlatform`. The same Windows-only path backs the
    guided-setup file and folder pickers (`pick_file_with_extensions` / `pick_file_starting_at`).
  - Scope: add Linux folder and file pickers (desktop portal/GTK or the Tauri dialog plugin) behind
    `ports_pick_parent` and `ports_pick_supplied`, with a typed-path fallback when no portal is
    available and a clear message when the picker cannot open. Never start an install with an empty
    parent folder.
  - Status: documented only, deliberately not fixed yet. It blocks 2.6 native acceptance.
  - Completion (2026-11-26): the app injects a `PortFilePicker` into `ports_pick_parent`,
    `ports_pick_supplied` and `ports_pick_executable`. On Linux it uses the Tauri dialog plugin
    (desktop portal/native dialog); Windows keeps its system dialogs. The setup modal adds a
    typed-path fallback and a clear message when the picker cannot open, and Start still requires
    a non-empty parent. The temporary "folder picker unavailable" block is gone, so a reviewed
    native entry is installable again. Tests: `accepts a typed parent folder when the picker
    cannot open`, `offers only the extracted folder on Linux`; native dialog behavior belongs to
    the 2.6 packaged run.

## 3. Review fixes (2026-11-24)

Each task names the decision in `design.md` and needs a regression test; Windows tests stay
unchanged (D7).

- [x] 3.1 Remove the `CatalogEntry::supports_windows` filter from `Overlay::apply`
  (`crates/core/src/ports/overlay.rs`) and apply the platform rule in the view (D13). Test: a
  Linux-only entry is listed on Linux and not on Windows; the UI mock's Linux-only entry comes
  through the overlay like real data.
  - Completion (2026-11-26): `Overlay::apply` now only merges and drops hidden entries;
    `lists_entry` in `crates/app/src/engine/pc_ports/view.rs` applies D13 (Windows lists Windows
    entries, Linux lists Linux or Windows entries, macOS none) and `build` filters with it. Tests:
    the overlay keeps every platform in order, the catalog worker keeps the Linux-only entry after
    the overlay, and the view lists it on Linux but not on Windows.
- [x] 3.2 Until 2.7 lands, disable Linux Install with a translated "folder picker unavailable"
  reason (D9), in en and pt-BR. Test: no enabled Install on Linux without a working picker.
  - Completion (2026-11-26): `PortInstallBlockReason::FolderPickerUnavailable` turns Install off
    on every Linux entry, with the pane and the setup modal using the new en/pt-BR text. The
    `?platform=linux` mock mirrors the reason. Tests: `linux_install_waits_for_the_folder_picker`
    and the UI's `blocks Install on Linux until the folder picker exists`.
- [x] 3.3 Pass `PortPlatform` to `build_detail`, `release_view` and `no_asset_reason`
  (`crates/app/src/engine/pc_ports/view.rs`) and use `select_asset_for`, so the pane and the
  `PortSetupModal` malware gate evaluate the platform's asset (D10). Test: Windows asset flagged
  and AppImage clean → no confirmation on Linux, and the reverse.
  - Completion (2026-11-26): `build_detail`, `release_view`, `no_asset_reason` and
    `update_available` take the `PortPlatform` and call `select_asset_for`; `PortsService`
    passes `config.platform`. Test `the_detail_reads_the_asset_of_its_platform` covers a flagged
    Windows file with a clean AppImage and the reverse.
- [x] 3.4 Filter by supported format before ranking architecture in `select_asset_for`
  (`crates/core/src/ports/plan.rs`) (D8). Test: `x86_64.tar.gz` + `x86_64.AppImage` selects the
  AppImage; Windows selection tests unchanged.
  - Completion (2026-11-26): selection partitions by installable format before
    `best_by_architecture`; when nothing is installable it still reports the problem on the
    best-ranked file, so the existing error messages and Windows tests are unchanged. Test:
    `an_installable_format_wins_over_the_best_ranked_one`.
- [x] 3.5 After zip/7z extraction on Linux apply the recorded zip mode and set `0o755` on the
  registered executable (D11). Test: end-to-end runner install of a zip and a 7z Linux port whose
  binary has no recorded execute bit.
  - Completion (2026-11-26): `extract_port_asset_to_dir` applies a zip entry's recorded Unix mode
    (masked to `0o777`, no setuid/setgid/sticky) during a Linux port install; the shared
    extractor keeps the platform default. The runner sets `0o755` on the executable it registers,
    covering the preferred, picked and fallback paths. 7z records no mode, so it gets the default
    plus the registration bit. Tests: `a_port_zip_keeps_its_recorded_unix_mode`,
    `a_linux_zip_port_without_a_recorded_mode_registers` and
    `a_linux_7z_port_without_a_recorded_mode_registers` (both with `SystemExecutableIdentity`).
- [ ] 3.6 Match preferred executables case-insensitively on Linux and fall back to the single
  installed AppImage/bare executable (D12, confirm first). Test: a versioned, mixed-case AppImage
  name registers without `AwaitingExecutable`.
- [ ] 3.7 Make the overlay's `launcher_executable` and `gameplay_executables` per platform, reading
  the old scalar fields as Windows (D13, confirm the schema first). Test: an overlay with
  `Lantern.exe` does not affect a Linux install or its playtime tracking.
- [x] 3.8 Add the Linux-review mark to overlay recipes and block unreviewed guided recipes and
  package-only slots in availability, not only at plan time (D14, D5; closes 2.3 with the
  extractor rule). Test: a Windows-authored recipe is blocked in the catalog on Linux with the
  not-reviewed reason; a package-only slot is blocked with the console-extraction reason.
  - Completion (2026-11-26): the overlay entry gains `linux_reviewed` and an overlay setup slot
    gains `packages_only` (schema 1, both default false). Availability reports `notReviewed` for a
    required guided recipe without the mark and `consoleExtraction` for a required package-only
    slot; the plan refuses the package-only slot on Linux and the setup modal hides package mode
    there. Tests: `the_overlay_marks_linux_review_and_package_only_slots`,
    `a_guided_linux_recipe_needs_a_review_and_a_package_only_slot_is_blocked`,
    `linux_refuses_a_package_only_slot` and the UI's `offers only the extracted folder on Linux`.
- [x] 3.9 Make `detect_asset_type`, `SUPPORTED_LINUX_FORMATS` and `linux_asset::kind` agree on
  `.run`, and recognize extensionless and `.x86_64` bare executables (D8). Test per format.
  - Completion (2026-11-26): `BARE_EXECUTABLE_SUFFIXES` (core) maps architecture suffixes such as
    `x86_64` to `bin` for selection; an extensionless asset is a bare executable only when the
    recipe names it as a Linux preferred executable. `detect_asset_type` rejects `.run`, accepts
    the same suffixes and sniffs ELF/PE/Mach-O for names without a known extension;
    `linux_asset::kind` accepts the suffixes and still refuses `.run`. Tests:
    `linux_bare_executable_names_need_no_declared_format`,
    `native_linux_binaries_are_bare_executables` and
    `linux_formats_split_into_executables_and_archives`.
- [ ] 3.10 **Decision needed (D16).** Map Linux `config` to `$XDG_CONFIG_HOME` and `data`/
  `localData` to `$XDG_DATA_HOME` in `CopyRoots::host` and `UserDataRoots::host`; Windows
  unchanged. Test with the `XDG_*` variables set and unset.
- [ ] 3.11 **Decision needed (D17).** Compare only device + inode in the Linux launch identity and
  check the execute bit separately; update the identity sentence in `docs/pc-ports.md`. Test: an
  in-place rewrite, `touch` and `chmod` keep the entry launchable; a replaced file asks for repair.
- [x] 3.12 Remove the duplicated `#[cfg(not(any(windows, target_os = "linux")))]` block in
  `SystemExecutableIdentity::identity` (`crates/platform/src/ports/job_runner.rs`) and record the
  Linux PC ports scope in `docs/linux-verification.md`. Check: `cargo check` for a target that is
  neither Windows nor Linux compiles that function.
  - Completion (2026-11-26): the duplicate block is gone and
    `docs/linux-verification.md` records what the automated suites cover and what the native
    packaged run (2.6/2.7) still owes. The full crate cannot be checked for a third target on this
    host (`aws-lc-sys` compiles its C sources against the host headers and fails for FreeBSD), so
    the exact cfg shape was compiled standalone with `rustc --target x86_64-unknown-freebsd`; it
    emits metadata without errors.

## 4. Artifacts and archive

- [x] 4.1 Rewrite the spec deltas to describe the final state: MODIFIED/RENAMED where the base
  change's Windows-only requirements are replaced, no Coming Soon requirements (review items 13
  and 14). `openspec validate add-linux-pc-port-catalog --strict` passes.
- [x] 4.2 Align `proposal.md`, `design.md` (D2, D3, D15–D17, review-findings table) and the
  header and "Linux scope" section of `docs/pc-ports.md` with the code and the review.
- [ ] 4.3 Archive `add-pc-port-catalog` first (its 10.1 and 10.2 are open), then archive this
  change once sections 1–3 and 2.6 are complete. Until the base is archived, the validator warns
  that the MODIFIED/RENAMED requirements have no main spec to apply to; that is expected.

## 5. Future

- Proton/Wine execution, Windows-only installs on Linux, macOS catalog/installs and a Linux PS4
  package extractor are future proposals. They are not tasks in this change.
- Malware-gate hardening (warning verdicts, scan links, unknown-verdict badges, local scanning)
  is recorded in `docs/pc-ports.md` under "Malware gate: possible hardening". It needs its own
  change and is not a task here.
- Relocating the port download cache (Settings or the install parent's volume) is recorded in
  `docs/pc-ports.md` under "Download cache: possible relocation". It needs its own change and is
  not a task here.
