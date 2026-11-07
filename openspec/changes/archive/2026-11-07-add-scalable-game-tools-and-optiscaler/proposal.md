# Proposal

## Why

Installing upscaling, frame-generation, and rendering tools like OptiScaler manually requires downloading `.7z` archives, locating the game's true main executable (often buried inside `Binaries/Win64` or `bin/x64`), renaming proxy DLLs (`dxgi.dll`, `winmm.dll`, `version.dll`), checking for anti-cheat risks, and manually tracking overwritten files to uninstall cleanly. Rather than building a one-off OptiScaler installer that would need to be duplicated for upcoming tools (such as MFG Unlocker, DLSS / FSR 4 Swapper—including replacing a game's or OptiScaler's FSR 4 DLL with FSR 4 RDNA 2 Lite—AMD NR Launcher, and AeonSR), we need a scalable, transaction-safe Game Tools Engine in `core` and `platform` now—delivering complete OptiScaler support as its first flagship tool while establishing the shared catalog, version caching, executable target resolution, conflict/layering detection, and byte-identical rollback ledger that future tools will plug into.

## What Changes

- **Introduce a Scalable Game Tools Architecture (`core::tools` & `platform::tools`)**:
  - **Declarative Tool Descriptors (`ToolDescriptor`)**: A unified registry defining each tool's metadata (`id`, `name`, `summary`, `license`, `source_url`), release channels (`Stable`, `Nightly`, or custom feeds), install strategy (`ProxyDllInjector`, `SiblingLibrarySwapper`, or `AddonOverlay`—covering proxy injectors like OptiScaler and AeonSR, in-place SDK swappers like DLSS and FSR 4 / FSR 4 RDNA 2 Lite, and host-coupled plugins/overlays like MFG Unlocker running as an OptiScaler `.asi` plugin), supported proxy/target filenames, and conflict/composition rules.
  - **Shared Release Fetcher & Content-Addressed Archive Cache**: An HTTPS release provider (supporting GitHub Releases APIs and pinned sources) and local archive cache under `<data_dir>/tools/cache/<tool_id>/<channel>/<version>/` with SHA-256 verification and pure-Rust `.7z` / `.zip` extraction.
  - **Universal Game Target & Executable Resolver**: Resolves the primary game executable and target deployment directory across any provider entry (`steam:<id>`, `manual:<id>`, and future stores)—Reusing manual registration executables when available or scanning the game's `install_dir` with the 64-bit/shipping/manifest-hint heuristic (`preferred_target`) and allowing user override when multiple candidate executables exist.
  - **Anti-Cheat & Conflict/Composition Guardrails**: Pre-install inspection of the game installation folder for EasyAntiCheat (`EasyAntiCheat`, `EasyAntiCheat_EOS.exe`), BattlEye (`BattlEye`, `BEService*.exe`), and a curated anti-cheat blocklist/game ID list, plus cross-tool DLL slot conflict detection (preventing two tools from silently clobbering the same `dxgi.dll` proxy slot), layered DLL ownership rules so a `SiblingLibrarySwapper` (such as an FSR 4 RDNA 2 Lite DLL swap) can safely replace either a native game SDK binary or an installed `ProxyDllInjector` companion binary (`amd_fidelityfx_upscaler_dx12.dll` / `amd_fidelityfx_dx12.dll`), and `AddonOverlay` host-plugin integration (e.g., deploying an `.asi` plugin into `<exe_dir>/plugins/` and declaratively patching `OptiScaler.ini` `[Plugins] LoadAsiPlugins=true` while preserving user comments and restoring the INI key when the last `.asi` addon is uninstalled).
  - **Transactional Install, Update, and Byte-Identical Rollback Ledger**: A persistent local ledger (`tool_installations` and `tool_backups`) that backs up any pre-existing game files (or prior tool layer files) into `<data_dir>/private/tool-backups/<entry_key>/<tool_id>/` before deployment and records every created file and SHA-256 digest. Uninstalling or updating a tool restores backed-up files and removes only unmodified tool-owned files, leaving the game folder (and any remaining active tools) byte-identical to its prior state.
- **Deliver Full OptiScaler Support (Flagship Tool #1)**:
  - **Stable & Nightly Channels**: Fetch versions from `optiscaler/OptiScaler` (Stable) and `optiscaler/OptiScaler-nightly` (Nightly), displaying release tags, dates, source links, and the `GPL-3.0` license notice.
  - **Proxy DLL Selection & Deployment**: Extract the release `.7z` archive, rename `OptiScaler.dll` (or legacy `nvngx.dll`) to the selected proxy filename next to the resolved main game executable—defaulting to `dxgi.dll` with fallbacks (`winmm.dll`, `version.dll`, `d3d12.dll`, `dbghelp.dll`, `wininet.dll`, `winhttp.dll`) per the OptiScaler wiki—while preserving `OptiScaler.ini` and bundled support libraries (`fakenvapi`, `dlssg_to_fsr3`, `libxess`, `amd_fidelityfx_*`, `D3D12_OptiScaler`, `streamline`), and skipping interactive `.bat`/`.sh` setup scripts.
  - **One-Click Install, Version Switch / Update, and Reversible Uninstall**: Preserve user edits to `OptiScaler.ini` across version updates (with an option to reset config), verify game processes are stopped before mutating game files, and require explicit confirmation when anti-cheat markers are detected.
- **UI Integration in the Library & Settings**:
  - **Per-Game Tools Modal & Detail Pane Action**: Add a `Game tools…` (or `Tools & OptiScaler…`) action and status summary on the Library detail pane and context menu for installed games, opening a dedicated `GameToolsModal` styled with the `SettingsStyle` card/section system. The modal lists available tools (starting with OptiScaler, and designed to list MFG Unlocker, DLSS / FSR 4 RDNA 2 Lite Swapper, AMD NR Launcher, and AeonSR as they are added), showing installed status, version/channel picker, target executable & proxy DLL selector, anti-cheat warning banner, and `Install` / `Update` / `Uninstall` actions.
  - **Global Tools Cache & Overview in Settings**: Add a `Tools` topic in `SettingsModal` showing cached tool versions on disk, cache size, a `Clear unused cache` action, and third-party license/source attribution for supported tools.

## Capabilities

### New Capabilities

- `desktop-game-tools`: Scalable per-game tools framework (tool registry, release channel fetching, archive caching, executable target resolution, anti-cheat detection, cross-tool DLL conflict checking, transactional file deployment, and byte-identical rollback ledger) and full OptiScaler installation, update, and uninstallation support.

### Modified Capabilities

- `desktop-library`: Expose per-game tool status and the `Game tools…` action in the detail pane and context menu for installed library entries across all providers.
- `desktop-local-state`: Persist installed per-game tool manifests, backed-up original file records, cached tool release metadata, and tool preferences in SQLite through a new ordered schema migration (`0005_game_tools.sql`).
- `desktop-settings`: Add a `Tools` topic in the Settings modal to inspect cached tool downloads, manage cache storage, and view tool source/license notices.

## Impact

- **Core (`crates/core`)**:
  - New `crates/core/src/tools/` module defining `ToolId`, `ToolDescriptor`, `ToolChannel`, `InstallStrategy`, `ProxyDllName`, `AntiCheatReport`, `ToolInstallation`, `FileOperationRecord`, `ToolsView`, and pure state/validation logic.
  - Port traits in `crates/core/src/ports.rs` (or `crates/core/src/tools/ports.rs`): `ToolCatalogApi` (fetching release lists & downloading archives), `ToolInstallerHost` (executable target resolution, anti-cheat scan, transactional install/uninstall/update while game is stopped), and `ToolStore` (SQLite persistence).
- **Platform (`crates/platform`)**:
  - New migration `crates/platform/migrations/0005_game_tools.sql` and `crates/platform/src/tools_db.rs`.
  - New `crates/platform/src/tools.rs` implementing archive extraction (`sevenz-rust2` / `zip`), SHA-256 verification, anti-cheat signature detection (`EasyAntiCheat`, `BattlEye`), executable folder resolution, and atomic rollback-safe file deployment with backup staging under `DataPaths`.
- **API / Release Client (`crates/api` or `crates/platform`)**:
  - GitHub release metadata fetcher and HTTPS asset downloader restricted to fixed, declared GitHub repositories (`optiscaler/OptiScaler`, `optiscaler/OptiScaler-nightly`, and future tool repos).
- **App & UI (`crates/app`)**:
  - New `crates/app/ui/tools.slint` (`GameToolsModal`, `ToolsState`) and updates to `crates/app/ui/library.slint`, `crates/app/ui/settings.slint`, `crates/app/src/engine/`, `crates/app/src/library_ui.rs`, and `i18n/` (`en` and `pt_BR`).
