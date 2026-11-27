# Windows-only inventory

This document inventories every place in the repository that currently treats Windows as the
only supported platform: provider manifests, compile-time gates, runtime platform checks, host
adapters, UI gating, build/packaging scripts, tests and documentation claims. Its purpose is to
make accidental Windows-only gates visible before they become user-facing bugs such as the
`providers/manual.toml` regression, where the DRM-free provider did not exist on Linux and
installed PC ports never appeared in the library.

The inventory reflects the working tree on 2026-11-27, after the fix that changed
`providers/manual.toml` and its registry guard from `["windows"]` to `["windows", "linux"]`.
Treat that provider as the reference example of an accidental gate.

**Keeping this list current.** Re-run these searches after adding a platform adapter or a UI
surface, and add or remove rows here:

```sh
rg -n 'platforms\s*=\s*\[' providers plugins
rg -n 'cfg\((not\()?windows|cfg!\(windows\)|target_os\s*=\s*"windows"' crates
rg -n 'Platform::Windows|PortPlatform::Windows|supports_windows|== "windows"' crates
rg -n "platform === 'windows'|platform !== 'windows'|backgroundSupported" ui/src
rg -n -i 'windows-only|windows only|only on windows' docs scripts packaging .github ui/src crates
```

The categories are:

| Category | Meaning |
| --- | --- |
| `intentional OS API` | Win32, COM, registry, PE/executable identity, job objects, native dialogs, named pipes or WebView2. A Linux adapter exists or the OS call has no Linux equivalent. |
| `intentional product scope` | Windows packaging/acceptance or a feature explicitly scoped to Windows (Epic, tools, Ludusavi backups, login autostart/background). |
| `accidental / needs a Linux decision` | The gate is not justified by an OS API or a stated product decision. This is the interesting category. |
| `test/fixture` | Windows-only tests, ignored tests and Windows path fixtures. |

## Summary

| Category | Count | Short list |
| --- | ---: | --- |
| `intentional OS API` | 23 | Autostart registry, WebView2, named-pipe IPC, job objects, PE identity, volume metadata, plugin environment/registry, file confinement, OS version, XDG/known-folder paths, Steam/Epic registry |
| `intentional product scope` | 26 | Epic provider, tools, Ludusavi backups, background/autostart UI, backups UI, system package scope, Windows packaging and CI cross-check, acceptance docs |
| `accidental / needs a Linux decision` | 18 | Manual provider (2 fixed rows), DRM-free pickers and discovery, DRM-free settings panel, ports cache panel, manifest refresh, realtime discovery, tray/close behavior, stale Windows-only comments |
| `test/fixture` | 18 | Windows-only test modules, ignored Windows discovery test, non-Windows tests that assert unsupported, Windows path fixtures |
| **Total** | **85** | |

The counts are the number of table rows in the sections below. Rows that group several
repetitive `#[cfg(windows)]` blocks in one file count once.

## 1. Provider manifests and registry

| # | Location | What is Windows-only and what it controls | Category | Impact on a Linux build | Recommendation |
| --- | --- | --- | --- | --- | --- |
| P1 | `providers/manual.toml:3`; `crates/core/src/provider/registry.rs:440` | The DRM-free (`manual`) provider's `platforms` list and the registry guard that only accepts the built-in executable provider for an exact platform list. | `accidental / needs a Linux decision` (**fixed**) | Before the fix, Linux never loaded the provider, so manual registrations and PC-port installs were absent from the library. Now it loads on both platforms. | Keep `["windows", "linux"]` and keep the Linux launch/identity tests. |
| P2 | `crates/core/src/provider/registry.rs:296-301` | A `registry` root is rejected unless it is declared under `roots.windows`. | `intentional OS API` | A Linux provider cannot use registry roots; path roots still work. | Keep; the check gives a clear diagnostic instead of silently failing. |
| P3 | `crates/core/src/provider/manifest.rs:16-27`; `crates/app/src/main.rs:170-173` | `Platform::current()` returns only Windows or Linux, and the app refuses to start elsewhere (`GameNode Desktop supports Windows and Linux only`). | `intentional product scope` | macOS and other targets do not start at all. | Keep unless macOS becomes a target; it is a fail-fast guard, not a silent skip. |
| P4 | `providers/epic.toml:6`; `plugins/epic.toml:11` | Epic Games Store is declared Windows-only in both the provider manifest and the trusted plugin descriptor. | `intentional product scope` | Epic never loads on Linux; docs state Heroic/Legendary are out of scope. | Keep by design; revisit only if Epic-on-Linux becomes a product goal. |
| P5 | `crates/core/src/provider/registry/tests.rs:570-580` | The manual provider test now loops over Windows and Linux. | `test/fixture` | Prevents the regression from returning. | Keep. |
| P6 | `crates/core/src/provider/registry/tests.rs:729,790` | Tests assert the Epic descriptor and provider are Windows-only. | `test/fixture` | Fails if someone accidentally adds `linux` to Epic without the rest of the Linux work. | Keep as an intentional-scope guard. |
| P7 | `crates/core/src/provider/registry/tests.rs:590-598` | Every TOML block in `docs/providers.md` is loaded only for `Platform::Windows`. | `test/fixture` | A Linux-only example in the docs could still pass the test even if Linux validation would reject it. | Add a Linux pass (or both platforms) when docs gain Linux examples. |

Controls checked and cleared: `providers/steam.toml:4` and `plugins/steam.toml:6` declare both
platforms; `providers/packagekit.toml:4` is Linux-only by design; `crates/app/src/plugin_host/startup.rs:356-360`
maps the two platforms for synthetic plugin manifests.

## 2. Rust compile-time gates

Repetitive `#[cfg(windows)]` blocks are grouped by file and purpose. Except where noted, a Linux
build compiles the `#[cfg(not(windows))]`/Unix branch and returns `Unsupported` or a no-op when no
Linux implementation exists; there is no compile error.

| # | Location | What is Windows-only and what it controls | Category | Impact on a Linux build | Recommendation |
| --- | --- | --- | --- | --- | --- |
| R1 | `crates/platform/src/autostart.rs:1-80`; `crates/platform/src/lib.rs:50`; `crates/app/src/main.rs:65,163`; `crates/app/src/engine/mod.rs:2519-2548` | The per-user `Software\Microsoft\Windows\CurrentVersion\Run` value, startup registration and the `apply_autostart` worker. On Linux the call is a no-op. | `intentional OS API` | Login startup is not registered or changed. | Keep; a Linux XDG-autostart/systemd adapter is separate product work. |
| R2 | `crates/app/src/main.rs:1,91-110,458,840,862,888` | `windows_subsystem`, and the WebView2 `SetIsVisible`/memory-target helper used when hiding/showing the window from the tray and IPC. | `intentional OS API` | Linux uses the default Tauri hide/show path without the WebView2 suspension. | Keep; it is a Windows runtime optimisation, not a feature gate. |
| R3 | `crates/platform/src/host.rs:125-221,333-349` | Registry reads for store roots and the `SystemRoots::registry_string` API; non-Windows returns `None`. | `intentional OS API` | Steam/Epic registry roots resolve to nothing; Steam uses its `~` roots instead. | Keep; the provider manifest declares the platform-appropriate roots. |
| R4 | `crates/platform/src/credentials/mod.rs:156-180`; `crates/platform/src/credentials/keyring.rs:66-90` | Windows Credential Manager store, and the deliberate no-file-fallback `Unavailable` state. | `intentional OS API` | Linux uses Secret Service, then the private-file fallback. | Keep. |
| R5 | `crates/platform/src/ipc/mod.rs:14-26`; `crates/platform/src/bin/instance_probe.rs:36`; `crates/plugin-sdk/src/lib.rs:275`; `crates/app/src/plugin_host.rs:971`; `crates/app/src/main.rs:247` | Named pipes for single-instance IPC and the plugin API, versus Unix domain sockets. | `intentional OS API` | Linux uses `UnixStream`/socket files; both paths exist. | Keep. |
| R6 | `crates/platform/src/processes.rs:4-55`; `crates/app/src/plugin_host/process.rs:222-322,791-793`; `crates/app/src/plugin_host/shared_resources.rs:733-740` | Toolhelp/job objects for process listing and plugin process-tree supervision; Linux uses `/proc`, process groups and pidfd. | `intentional OS API` | Playtime and plugin supervision work through the Linux adapter. | Keep. |
| R7 | `crates/platform/src/installations.rs:312-434,459-619` | Windows executable identity (volume serial + file index), PE header validation and `WindowsInstallations` spawn/validate paths. | `intentional OS API` | Linux uses `LinuxInstallations` with device + inode and the execute bit. | Keep; do not weaken the Windows checks. |
| R8 | `crates/platform/src/volume.rs:151-197`; `crates/platform/src/storage.rs:21-52` | Win32 volume GUID/drive-type/capacity metadata and free-space queries. | `intentional OS API` | Linux reads mountinfo/sysfs/statvfs; unknown dimensions stay unknown. | Keep. |
| R9 | `crates/platform/src/plugin_environment.rs:103,307-460` | Windows version/product/architecture detection for plugin compatibility. | `intentional OS API` | Linux reads `/etc/os-release`, uname and ELF/libc data. | Keep. |
| R10 | `crates/platform/src/plugin_files.rs:28-34,200-204` | Windows confined-file implementation and the `open_probe` access method versus Unix `kernel_access_check`. | `intentional OS API` | Linux uses the Unix module and kernel access checks. | Keep. |
| R11 | `crates/platform/src/plugin_registry.rs:978-1020`; `crates/app/src/plugin_host/grants.rs:995-1030`; `crates/app/src/plugin_host/publisher_trust.rs:457-492` | `MoveFileExW` atomic replacement of pointers/grants on Windows; other platforms use `rename`. | `intentional OS API` | Linux uses `fs::rename` and directory fsync. | Keep. |
| R12 | `crates/platform/src/system.rs:9-70` | OS version from the `CurrentVersion` registry key and Windows profile-path redaction. | `intentional OS API` | Linux reads `/etc/os-release` and redacts with `~`. | Keep. |
| R13 | `crates/platform/src/paths.rs:95-97,212-214` | Windows `%LOCALAPPDATA%\GameNode\Desktop` data/log paths versus Linux `~/.local/share`. | `intentional OS API` | Linux uses XDG paths. | Keep. |
| R14 | `crates/platform/src/plugin_locations.rs:35-43` | `Os::current()` and absolute-path normalization for plugin-declared locations. | `intentional OS API` | Linux expands `~`, XDG variables and absolute Unix paths. | Keep. |
| R15 | `crates/platform/src/backups.rs:123-126,425-426,476,787,922-923` | The bundled Ludusavi helper, `scoped_available()`, backup/restore execution and the ignored `CREATE_NO_WINDOW` flag. | `intentional product scope` | `scoped_available()` is false and every backup entry point returns `LocalError::Unsupported`; the Windows adapter test is skipped. | Keep while the Linux package has no backup assets; remove the `cfg!(windows)` when a Linux helper ships. |
| R16 | `crates/platform/src/ports/extractor.rs:787-795,1093-1299` | Reparse-point checks and job-object child-tree management for port extractors. | `intentional OS API` | Linux uses symlink metadata and process groups; extraction works. | Keep. |
| R17 | `crates/platform/src/ports/job_runner.rs:140-175` | Windows executable identity for a completed port install. | `intentional OS API` | Linux uses device + inode through `linux_identity`. | Keep. |
| R18 | `crates/plugin-steam/src/discovery/registry.rs:1-60`; `crates/plugin-epic/src/discovery/mod.rs:360-390` | Steam and Epic registry readers for store roots; non-Windows returns `None`. | `intentional OS API` | Steam still scans its Linux roots; Epic never loads on Linux. | Keep. |
| R19 | `crates/app/src/commands.rs:114-127` | `GetUserDefaultLocaleName` for the system locale; Linux reads `LC_ALL`/`LC_MESSAGES`/`LANG`. | `intentional OS API` | Locale detection still works through environment variables. | Keep. |
| R20 | `crates/app/src/plugin_host/launch_supervisor.rs:12-30`; `crates/app/src/plugin_host/runtime_access.rs:551-566` | Windows custom-launch supervisor and `.exe`/`.com` executable check; Linux uses the helper and the execute bit. | `intentional OS API` | Both adapters exist; no feature loss. | Keep. |
| R21 | `crates/app/src/engine/tools.rs:90-128`; `crates/app/src/engine/mod.rs:3335-3342`; `crates/app/src/plugin_host/resources/tools.rs:60-95` | Game tools are advertised as unavailable and installation is refused unless `cfg!(windows)`; a plugin request for `platform = "linux"` receives an empty list. | `intentional product scope` | Tools are absent on Linux, with explicit `UNAVAILABLE`/empty results rather than a silent success. | Keep by design; the UI hides the Tools panel sections (see U3). |
| R22 | `crates/app/src/engine/drm_free.rs:2747-2749`; `crates/platform/src/discovery.rs:190-196` | The DRM-free realtime watcher (`start_local_discovery`) and `FolderNotifications` compile to nothing without Windows. | `accidental / needs a Linux decision` | The method exists but does nothing on Linux; no directory events and no periodic watcher are started. | Decide whether Linux DRM-free discovery is in scope; `notify` supports Linux, but the scanner uses `WindowsInstallations` today. |
| R23 | `crates/app/src/plugin_host/resources/libraries.rs:132-136` | Plugin library roots report `discovery_state = "available"` only on Windows and `"unsupported_platform"` elsewhere. | `accidental / needs a Linux decision` | Plugins see Linux roots as unsupported even though the host polls library scans every 30 seconds. | Report a capability based on the actual monitor, or implement Linux filesystem notifications. |

## 3. Runtime platform checks and views

| # | Location | What is Windows-only and what it controls | Category | Impact on a Linux build | Recommendation |
| --- | --- | --- | --- | --- | --- |
| V1 | `crates/app/src/engine/mod.rs:1047-1050` | The "Check when GameNode starts" reconcile runs only when `platform == Windows`. | `accidental / needs a Linux decision` | A Linux start never reconciles watched folders, even if discovery is enabled. | Remove the platform check and let the discovery adapter report unsupported. |
| V2 | `crates/app/src/engine/mod.rs:2571-2578` | Turning realtime discovery back on resumes the watcher and re-scans only on Windows. | `accidental / needs a Linux decision` | The preference persists but has no effect on Linux. | Gate on `start_local_discovery` capability, not platform. |
| V3 | `crates/app/src/engine/drm_free.rs:2990-2993` | The daily Ludusavi manifest refresh returns early unless the provider platform is Windows. | `accidental / needs a Linux decision` | Linux never refreshes the manifest; without the packaged `backup-assets/manifest.yaml`, manifest-dependent DRM-free enrichment and save-profile matching are unavailable too. | Gate on whether the bundled manifest asset exists, not on the OS. |
| V4 | `crates/app/src/engine/drm_free.rs:2217-2228` | When no `DiscoveryHost` is wired, the engine falls back to `WindowsDiscovery`; on Linux that returns `Unsupported`, which is converted to an empty report when installations exist. | `accidental / needs a Linux decision` | In production (`discovery: None`) Linux watched-folder scans can silently find zero candidates instead of reporting that discovery is unavailable. | Wire a Linux discovery host or return the unsupported error; do not convert `Unsupported` to an empty success. |
| V5 | `crates/app/src/commands.rs:283-292` | `AppInfo.background_supported` is `cfg!(windows)`. | `intentional product scope` | The UI hides the startup card and the tray note; no background tracking at login on Linux. | Keep while background tracking is Windows-only; update together with a Linux tray/background decision. |
| V6 | `crates/app/src/engine/pc_ports/view.rs:47-58,168-190` | PC-ports listing and installability: Windows lists Windows entries; Linux lists Linux or Windows entries and blocks Windows-only installs with `WindowsOnly`; macOS lists none. | `intentional product scope` | Windows-only entries stay visible but blocked on Linux, which is intentional. | Keep; this is the platform rule, not a Windows gate. |
| V7 | `crates/core/src/ports/plan.rs:590-599` | `plan_install` refuses a Windows plan for an entry without Windows support and a Linux plan without Linux support. | `intentional product scope` | A Linux install of a Windows-only entry fails with `NotLinux` before any job starts. | Keep; the UI already shows the block reason. |

## 4. Host integrations

| # | Location | What is Windows-only and what it controls | Category | Impact on a Linux build | Recommendation |
| --- | --- | --- | --- | --- | --- |
| H1 | `crates/platform/src/installations.rs:719-982`; `crates/platform/src/covers.rs:66-74`; `crates/app/src/engine/drm_free.rs:1068-1082` | `GetOpenFileNameW`, `SHBrowseForFolderW`/COM and the `pick_file`/`pick_folder`/`pick_executable` helpers; used by **Add game**, **Locate executable**, **Change cover** and the executable picker in repair flows. Non-Windows returns `LocalError::Unsupported`. | `accidental / needs a Linux decision` | Add game / Locate executable / Change cover fail with "this operation is unavailable on this platform" even though the manual provider now loads on Linux. | Route these through the existing Tauri `PortFilePicker` (`crates/app/src/commands.rs:2830-2884`) or add a Linux dialog implementation. |
| H2 | `crates/app/src/engine/drm_free.rs:2898-2926` | `pick_local_watch_folder` and `pick_local_saves` call `pick_save_location`, which is Win32-only. | `accidental / needs a Linux decision` | Once the DRM-free panel is shown on Linux, adding a watched folder or choosing a save folder fails. | Use the Tauri dialog, as PC ports already does. |
| H3 | `crates/app/src/commands.rs:2447-2485`; `crates/platform/src/installations.rs:875-880` | `pick_save_destination` uses the Win32 folder picker. | `intentional product scope` | Backups are Windows-only, so the command is unreachable on Linux. | Keep until Linux backups exist; switch to the Tauri dialog at the same time. |

Controls checked and cleared: `LinuxInstallations` (`crates/platform/src/installations.rs:105-310`)
implements inspect, validate, launch, launch-with-environment, custom launch and evidence;
`SystemExecutableIdentity` has a Linux branch (`crates/platform/src/ports/job_runner.rs:140-175`);
the credential, registry and process adapters have Linux implementations (R3-R6).

## 5. Tauri, UI and lifecycle

| # | Location | What is Windows-only and what it controls | Category | Impact on a Linux build | Recommendation |
| --- | --- | --- | --- | --- | --- |
| U1 | `ui/src/features/settings/panels/DrmFreePanel.tsx:35-36,97-107` | `localFeaturesSupported()` returns true only for an unreported platform or `windows`; otherwise the whole DRM-free settings panel is replaced by "Local discovery is available in the Windows build." | `accidental / needs a Linux decision` | Linux cannot add watched folders, change discovery preferences, add an executable from Settings or reset ignored discoveries. PC-port installs still create `manual:` library entries, so the entries exist but this panel is hidden. | Show the panel on Linux once H1/H2 have Linux implementations, and gate only the features that remain unavailable. |
| U2 | `ui/src/features/settings/panels/BackupsPanel.tsx:31-32,104-114` | The same helper hides local save backups on non-Windows. | `intentional product scope` | Backups report unavailable on Linux, matching the Win32-only Ludusavi engine. | Keep while backups are Windows-only; update with R15/H3. |
| U3 | `ui/src/features/settings/panels/ToolsPanel.tsx:77,82,87,429` | `showPorts = platform === 'windows'` hides the "helper tools" and "ports cache" cards, and disables the `portsToolsCache` query. | `accidental / needs a Linux decision` | Linux PC-port installs work, but users cannot inspect or clear the port download/extractor cache. | Show the ports cache on Linux too (`platform !== 'macos'` or the same predicate as the sidebar). |
| U4 | `ui/src/features/settings/panels/GeneralPanel.tsx:65-66,215-281` | The startup card (`Start with Windows`, `Start minimized`, `Closing the window minimizes to the tray`) is shown only when `platform === 'windows' && backgroundSupported`. | `intentional product scope` | Linux has no login-startup or close-to-tray controls. | Keep while background/tray behavior is Windows-only. |
| U5 | `ui/src/features/shell/SyncExplanationModal.tsx:101,132` | The "closing the window keeps GameNode running" note is shown only when `backgroundSupported`. | `intentional product scope` | Linux does not promise background tracking. | Keep consistent with V5. |
| U6 | `crates/app/src/main.rs:812-871,947-1000` | The tray icon and menu are built on every platform, but the `CloseRequested` decision (`close_action` -> hide-to-tray or quit) is compiled only for Windows. | `accidental / needs a Linux decision` | Linux attempts to show a tray icon but ignores `close_to_tray`; closing the window follows the default Tauri path instead of the engine's graceful quit path. Docs claim Linux has no tray. | Either gate tray creation to Windows, or implement the same close-to-tray/quit decision on Linux and update the docs. |
| U7 | `ui/src/features/ports/PortsView.tsx:51,69,149`; `ui/src/features/ports/catalog/PortsToolbar.tsx:26-27,180-199` | The platform filter (`All`, `Native Linux`, `Windows-only`) is shown only on Linux; Windows forces the stored filter back to `All`. | `intentional product scope` | Windows-only entries are hidden from the control on Windows; on Linux they remain visible and blocked. | Keep; it is a Linux-catalog affordance, not a Windows gate. |
| U8 | `ui/src/features/ports/setup/PortSetupModal.tsx:509,927-934` | Console-package extraction mode is disabled when `appInfo.platform === 'linux'`, because the pinned extractor is a Windows build. | `intentional OS API` | Linux users must supply the extracted folder directly. | Keep until a Linux extractor is pinned; the reason is shown in the UI. |
| U9 | `ui/src/api/types.ts:2277-2283` | The asset protocol base uses `http://gamenode-asset.localhost` on Windows WebView2 and the bare custom scheme elsewhere. | `intentional OS API` | Linux uses `gamenode-asset://localhost`. | Keep. |
| U10 | `ui/src/api/ports.ts:36,358`; `ui/src/features/settings/panels/ToolsPanel.tsx:65,77`; `ui/src/App.tsx:165`; `ui/src/features/ports/catalog/PortsToolbar.tsx:26`; `crates/app/src/commands.rs:26,2746` | Comments still say "PC port installs are Windows-only", "filtered to Windows entries", "read-only Linux phase 1", etc. | `accidental / needs a Linux decision` | No runtime effect, but the comments mislead the next developer into re-adding a gate. | Update the comments to the current Windows+Linux behavior. |

Controls checked and cleared: `ui/src/App.tsx:168-174` and `ui/src/features/shell/Sidebar.tsx:143-149`
show the ports destination on both Windows and Linux; `crates/app/capabilities/default.json:5` and
`crates/app/tauri.conf.json:14` use the Tauri `windows` key for **webview window labels**, not the
Windows OS, and are not gates.

## 6. Build, packaging and CI

| # | Location | What is Windows-only and what it controls | Category | Impact on a Linux build | Recommendation |
| --- | --- | --- | --- | --- | --- |
| B1 | `.github/workflows/plugin-boundaries.yml:17-65` | CI runs UI tests, Linux workspace tests, Clippy and a `cargo xwin check` Windows cross-compile. There is no native Windows test or packaging run. | `intentional product scope` | Windows-only code is type-checked but Windows-only tests are skipped on the Linux runner. | Keep the cross-check; add a native Windows job when Windows acceptance becomes a release gate. |
| B2 | `scripts/build-windows-wsl.sh:1-57`; `scripts/package-windows-wsl.sh:1-77`; `scripts/install-windows-wsl.sh:1-20` | Windows MSVC cross-build, ZIP layout, bundled plugin index, backup assets, PowerShell install and WSL interoperability. | `intentional product scope` | These scripts are not used for Linux packages. | Keep; they are the documented Windows route. |
| B3 | `scripts/dev-windows-wsl.sh:1-146`; `scripts/dev-windows-wsl.ps1`; `scripts/run-desktop.ps1:1-40` | Windows HMR launcher and native PowerShell runner. | `intentional product scope` | Linux uses `scripts/run-desktop.sh`. | Keep. |
| B4 | `scripts/run-desktop.sh:7-11` | Chooses `linux` or `windows` from `uname`, and notes that Epic is Windows-only so the Linux plugin list omits it. | `intentional product scope` | The script builds the host plus the platform's declared plugins. | Keep; the descriptor list remains the source of truth. |
| B5 | `scripts/prepare-backup-assets.py:1-60`; `packaging/windows/backup-inputs.json` | Downloads and verifies the pinned Windows Ludusavi executable and manifest. | `intentional product scope` | Linux packages carry no `backup-assets/`, which is why R15/V3 return unsupported. | Keep while backups are Windows-only. |
| B6 | `scripts/tests/test_plugin_boundaries.py:77-84` | A test rejects public-crate dependencies on `gamenode-platform` behind `cfg(windows)`. | `test/fixture` | Protects the plugin boundary from Windows-only internal edges. | Keep. |

Controls checked and cleared: `scripts/bundled_plugins.py:26-30,54-56` defines
`KNOWN_PLATFORMS = ("linux", "windows")` and appends `.exe` only for Windows; this is platform
vocabulary, not a gate. `scripts/package-linux.sh` is the Linux packaging counterpart.

## 7. Tests and fixtures

| # | Location | What is Windows-only and what it controls | Category | Impact on a Linux build | Recommendation |
| --- | --- | --- | --- | --- | --- |
| T1 | `crates/platform/src/backups.rs:922-923` | `adapter_windows_tests` requires the packaged `ludusavi.exe` and native Windows execution. | `test/fixture` | The test is compiled out on Linux. | Keep; record native Windows runs separately. |
| T2 | `crates/app/src/engine/tests/drm_free.rs:1179-1182` | `windows_watched_folder_produces_review_candidates` is Windows-only and `#[ignore]`d; it needs `GAMENODE_DISCOVERY_ROOT`. | `test/fixture` | It never runs in CI. | Keep as a manual acceptance fixture; add a Linux equivalent if discovery is enabled. |
| T3 | `crates/app/src/engine/tests/drm_free.rs:1092-1177` | Two `#[cfg(not(windows))]` tests assert that a watched-folder check ends with `"this operation is unavailable on this platform"`. | `test/fixture` | These tests encode the accidental gate: enabling Linux discovery will correctly fail them. | Update them when V1/V2/R22 are addressed. |
| T4 | `crates/platform/src/plugin_locations/tests.rs:95-106` | Windows-only drive-letter and UNC normalization assertions. | `test/fixture` | Skipped on Linux; the Linux cases above still run. | Keep. |
| T5 | `crates/platform/src/ipc/tests.rs:1,14-26,61-72,197`; `crates/platform/tests/instance_processes.rs:54-66,134` | Per-platform endpoint fixtures: named pipes on Windows, temp-directory Unix sockets on Linux. | `test/fixture` | The Linux branch runs; the Windows branch is skipped. | Keep. |
| T6 | `crates/platform/src/ports/job_runner/tests.rs:559,2081-2098` | Windows `PortPlatform::Windows` install cases, `.exe` filenames and a per-platform symlink branch. | `test/fixture` | Most cases run on Linux because platform is an argument; the Windows symlink branch is skipped. | Keep; add Linux-native cases next to the Windows ones where behavior differs. |
| T7 | `crates/app/src/engine/tests/drm_free.rs` (many: `load_bundled(Platform::Windows)` and `C:\...` fixtures) | Provider and path fixtures exercise the Windows manual provider and Windows paths. | `test/fixture` | They pass on Linux because paths are opaque strings, but they do not prove Linux path behavior. | Add at least one Linux-path manual registration/lifecycle test (some Linux-only tests already exist at lines 3211-3521). |
| T8 | `crates/app/src/engine/tests/tools.rs:211-212,266` | Tool tests use `C:/Games/...` and `.exe` paths. | `test/fixture` | Tools are Windows-only, so the fixture matches the feature scope. | Keep until tools support Linux. |
| T9 | `crates/app/src/commands.rs:3202-3223` | Diagnostics fixture uses `C:\Users\cassi\Steam` and asserts `%USERPROFILE%` on Windows or `~` elsewhere. | `test/fixture` | The non-Windows branch asserts `~`; the Windows branch is skipped. | Keep. |
| T10 | `crates/platform/src/installations.rs:1062-1145` | `unsupported_on_linux` and other non-Windows tests assert the Win32 adapters return `Unsupported`. | `test/fixture` | Documents the current Windows-only dialog/identity behavior. | Keep until H1 lands, then extend for Linux pickers. |
| T11 | `ui/src/mock/host.ts:2517,4884,5305-5315,5928-5932` | The UI mock defaults to a Windows signed-in build, Windows plugin inventory and Windows paths. | `test/fixture` | `?platform=linux` exercises the Linux branch, but most tests see Windows defaults. | Keep; add Linux-default coverage for DRM-free and ports-cache panels when U1/U3 change. |
| T12 | `crates/plugin-steam/src/discovery/tests.rs:56-165`; `crates/plugin-steam/src/discovery/fixtures/libraryfolders_windows.vdf` | Windows Steam path and `libraryfolders.vdf` fixtures, alongside Linux fixtures. | `test/fixture` | Both platform fixture sets exist and run. | Keep. |
| T13 | `ui/src/mock/host.ts:405`; `ui/src/features/library/DetailPane.test.tsx:188,496`; `ui/src/features/settings/StoresPanel.test.tsx:83,196`; other `C:\`/`%APPDATA%` fixtures | UI tests use Windows install roots and `%APPDATA%` file lists. | `test/fixture` | The UI renders paths as opaque strings; no Linux path rendering is asserted. | Add a Linux-path rendering case where the UI formats or truncates paths. |
| T14 | `crates/app/src/plugin_host/resources/tests/tools.rs:100-109` | A test asserts a `platform = "linux"` tools request returns an empty list. | `test/fixture` | Locks in R21's Windows-only tools behavior. | Update when tools gain Linux support. |

## 8. Documentation claims

These are the places a reader is told that something is Windows-only. Cross-reference them when
changing the corresponding gate.

| # | Location | What it claims | Category | Impact on a Linux build | Recommendation |
| --- | --- | --- | --- | --- | --- |
| D1 | `docs/mvp-roadmap.md:81` | "New platform features are Windows-only" until the Linux milestone; Linux returns "unsupported" or compiles the feature out. | `accidental / needs a Linux decision` | This policy is the root cause of the manual-provider class of bug. It is stale for features already ported (manual provider, PC ports, package discovery). | Replace the blanket rule with a capability checklist and update this inventory when a Linux adapter lands. |
| D2 | `docs/mvp-roadmap.md:128,151,228` | Example manifests use `platforms = ["windows"]`, and the assumptions table says Linux is not in the MVP. | `intentional product scope` | Historical MVP scope; does not match the current Linux work. | Keep as history if desired, but add a note that the Linux milestone has started. |
| D3 | `docs/development.md:287` | The `?platform=linux` mock hides the startup card, Epic card and Windows-only Tools/ports surfaces, and makes DRM-free/Backups unavailable. | `intentional product scope` | Describes the current UI fixture, including the accidental U1/U3 gates. | Update when U1/U3 are fixed so the documented fixture behavior matches. |
| D4 | `docs/development.md:617-620` | Native-only checks include the Windows tray, Credential Manager, native file dialogs, Windows process/parent tracking and directory notifications. | `intentional OS API` | Accurately lists Windows adapters that Vite cannot exercise. | Keep; extend with Linux-native acceptance items. |
| D5 | `docs/drm-free-games.md:3,29` | "On Windows, choose Add game" and "Identification reads bounded PE metadata". | `accidental / needs a Linux decision` | The first sentence is stale now that the manual provider loads on Linux; PE metadata is genuinely Windows-specific. | Reword the opening to "Add game" without "On Windows"; keep the PE sentence scoped to Windows. |
| D6 | `docs/epic-games-store.md:3,18,200,222` | Epic is Windows-only; the plugin compiles on Linux but its descriptor lists `windows` only. | `intentional product scope` | Accurate. | Keep. |
| D7 | `docs/plugin-api-permissions.md:843,926,1004` | The port installer, tools production route and bundled backup engine are Windows-only. | `intentional product scope` | Accurate for tools/backups; the port-installer sentence is stale for Linux native installs. | Update the port-installer sentence; keep the tools/backups statements until Linux equivalents exist. |
| D8 | `docs/plugin-development.md:700` | Runtimes/tools "currently runs only on Windows". | `intentional product scope` | Accurate for tools; verify before extending to runtimes. | Keep, or link to this inventory. |
| D9 | `docs/pc-ports.md:10,310,653-672` | Native Linux installs are in progress; the leftover recipe paths and console extractor are Windows-specific; the platform filter is Linux-only. | `intentional product scope` | Accurate description of the Linux scope and the intentional Windows-only extractor. | Keep; update the scope when the remaining Linux tasks close. |
| D10 | `docs/steam-plugin-integration.md:29` | The PC-ports regression row says "installation is Windows-only". | `accidental / needs a Linux decision` | Stale: Linux native installs are implemented. | Update to "native Windows installs; Linux installs in progress". |
| D11 | `docs/local-state.md:243,303,320` | Tables added by migrations are described as "Windows-user-owned". | `accidental / needs a Linux decision` | The wording implies the data is Windows-only, but the tables are in the cross-platform SQLite database. | Reword to "user-owned" or "per-OS-user". |
| D12 | `docs/providers.md:126-149` | The DRM-free section is now titled "Windows and Linux" and the manifest example lists both platforms. | `accidental / needs a Linux decision` (**fixed**) | Matches the uncommitted manual-provider fix. | Keep consistent with P1 and this inventory. |
| D13 | `docs/windows-verification.md:1-140`; `docs/linux-verification.md:1-210` | Windows acceptance evidence and Linux foundation/limits, including "no tray backend or tray icon" for Linux. | `intentional product scope` | The Linux doc's "no tray" claim appears to conflict with the unconditional tray builder (U6). | Update the Linux doc or gate the tray; record native runs in the matching file. |
| D14 | `.github/ISSUE_TEMPLATE/bug_report.yml:68-70`; `.github/ISSUE_TEMPLATE/feature_request.yml:13,56` | The MVP prioritises Windows; macOS is unsupported. | `intentional product scope` | Sets reporter expectations, not a runtime gate. | Keep, or update when Linux becomes a supported release target. |
| D15 | `packaging/linux/README.md:34` | "Linux packages never include a Windows-only plugin." | `intentional product scope` | Accurate: `bundled_plugins.py` filters by platform. | Keep. |

## 9. Not Windows-only (checked and cleared)

These look like gates at first glance but are not. Do not "fix" them without a separate decision.

- `crates/app/tauri.conf.json:14` (`"windows": []`) and `crates/app/capabilities/default.json:5`
  (`"windows": ["main"]`) use Tauri's webview-window key, not the Windows OS.
- `Platform` and `PortPlatform` enums, `Platform::current()` and `PortPlatform::current()` are
  platform discriminators. Windows is the `#[default]` variant in `PortPlatform`, but
  `current()` selects the running OS and `supports_installs()` covers Windows and Linux.
- `crates/core/src/ports/plan.rs:128-145`, `crates/app/src/engine/pc_ports/view.rs:47-58` and
  `crates/core/src/ports/catalog.rs:163-230` implement per-platform path rules, asset selection
  and executable roles; Windows is one branch, not the only branch.
- `crates/app/src/engine/playtime.rs:70` passes `platform == Platform::Windows` to the tracker for
  case-insensitive path comparison; Linux tracking still runs.
- `crates/platform/src/ports/placement.rs:75-93` uses the roaming/local split for every non-Linux
  platform; this is a user-data-root rule, not a Windows feature gate.
- `crates/app/src/plugin_host/managed_packages.rs:587` selects `msvc` for Windows fixtures and
  `gnu` otherwise; it is test-fixture metadata.
- The Linux adapters for processes, credentials, volume metadata, plugin files, plugin
  environment, IPC, `LinuxInstallations`, `SystemExecutableIdentity` and port extraction all
  exist and are exercised by Linux tests. A `#[cfg(not(windows))]` branch next to them is a
  fallback, not a missing feature.
- Plugin notifications (`crates/app/src/plugin_host/notifications.rs:1-45`) and the tray/app menu
  items (`crates/app/src/main.rs:817-827`) use platform-neutral Tauri events and menu APIs. Only
  the tray lifecycle/close decision is Windows-gated (U6).

## 10. Suspicious gates to review

Ordered by user-visible risk. The manual provider is the fixed reference example.

1. **DRM-free executable and cover pickers are still Win32-only.** `crates/platform/src/installations.rs:719-982`,
   `crates/platform/src/covers.rs:66-74`, `crates/app/src/engine/drm_free.rs:1068-1082`. Add game,
   Locate executable and Change cover fail on Linux with "this operation is unavailable on this
   platform", even though the manual provider and PC-port entries now exist there. Reuse the Tauri
   `PortFilePicker` or add a Linux dialog implementation.
2. **DRM-free discovery is compiled out or reports unsupported on Linux.** `crates/app/src/engine/drm_free.rs:2747-2749`,
   `crates/app/src/engine/mod.rs:1047-1050,2571-2578`, `crates/platform/src/discovery.rs:7,190-196`,
   `crates/app/src/engine/drm_free.rs:2217-2228`. Watched folders either never reconcile or silently
   produce zero candidates. Decide whether Linux discovery is in scope and implement it with
   `notify` plus `LinuxInstallations`, or return an explicit unsupported error.
3. **The DRM-free settings panel is hidden on Linux.** `ui/src/features/settings/panels/DrmFreePanel.tsx:35-36,97-107`.
   Linux cannot add watched folders, change discovery preferences, add an executable from Settings
   or reset ignored discoveries, although PC ports creates `manual:` entries there. Show the panel
   on Linux once the pickers (item 1) and discovery (item 2) have Linux implementations, gating only
   the operations that remain unavailable.
4. **The ports cache card is hidden on Linux.** `ui/src/features/settings/panels/ToolsPanel.tsx:77,82,87,429`.
   Linux installs ports but cannot inspect or clear the download/extractor cache. Show the card
   wherever PC ports is available.
5. **The Ludusavi manifest refresh is gated on Windows.** `crates/app/src/engine/drm_free.rs:2990-2993`.
   Linux never refreshes the manifest, and without the packaged manifest asset, manifest-dependent
   DRM-free enrichment and save-profile matching are unavailable too. Gate on the presence of the
   bundled manifest asset instead of `Platform::Windows`.
6. **A tray icon is created on Linux, but close-to-tray is Windows-only.**
   `crates/app/src/main.rs:812-871,947-1000`. Linux attempts to show a tray icon but ignores
   `close_to_tray` and can take the default Tauri close path rather than the engine's graceful
   shutdown. Either gate tray creation to Windows or implement the same decision on Linux.
7. **Plugin library roots report `unsupported_platform` on Linux.** `crates/app/src/plugin_host/resources/libraries.rs:132-136`.
   The host polls library scans on Linux, so the advertised capability is misleading.
8. **Stale "Windows-only" comments in PC-ports and tools code.** `ui/src/api/ports.ts:36,358`,
   `ui/src/features/settings/panels/ToolsPanel.tsx:65,77`, `ui/src/App.tsx:165`,
   `crates/app/src/commands.rs:26,2746`. They do not affect behavior but invite new accidental
   gates. Update them with the code.

Items deliberately left Windows-only: Epic (P4), game tools (R21), Ludusavi backups (R15), login
autostart/background (R1, V5, U4), the Win32 console-package extractor (U8) and the Windows
packaging/CI route (B1-B5). Each has a stated product or OS-API reason; revisit them only with an
explicit scope decision.
