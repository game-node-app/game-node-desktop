# desktop-game-tools Specification

## Purpose

Provide a scalable, reversible per-game tools management system that fetches tool releases from fixed sources, resolves each game's main executable directory, warns on anti-cheat or cross-tool conflicts, and installs, updates, or uninstalls tools (starting with OptiScaler, and architected for MFG Unlocker, DLSS / FSR 4 & FSR 4 RDNA 2 Lite Swapper, AMD NR Launcher, and AeonSR) while guaranteeing byte-identical restoration of the game folder.

## Requirements

### Requirement: Declarative tool registry and install strategies
The application SHALL maintain a built-in registry of supported game tools in `core`. Each descriptor SHALL declare its `id`, `name`, SPDX `license`, `source_url`, release channels (`Stable`, `Nightly`), archive format (`.7z`, `.zip`), and an `InstallStrategy` (`ProxyDllInjector`, `SiblingLibrarySwapper`, or `AddonOverlay`) so future tools share the same catalog, conflict/composition checks, and rollback ledger.

#### Scenario: Built-in OptiScaler descriptor is available at startup
- **WHEN** the application starts on Windows
- **THEN** the tools registry includes `optiscaler` with its `GPL-3.0` license, source link (`https://github.com/optiscaler/OptiScaler`), `Stable` (`optiscaler/OptiScaler`) and `Nightly` (`optiscaler/OptiScaler-nightly`) channels, `ProxyDllInjector` strategy, default proxy filename `dxgi.dll`, and wiki-supported fallback filenames (`winmm.dll`, `version.dll`, `d3d12.dll`, `dbghelp.dll`, `wininet.dll`, `winhttp.dll`)

#### Scenario: Future tools share the same catalog and ledger interfaces
- **WHEN** a tool descriptor using `ProxyDllInjector` (e.g., AeonSR, MFG Unlocker, or AMD NR Launcher), `SiblingLibrarySwapper` (e.g., DLSS Swapper or FSR 4 → FSR 4 RDNA 2 Lite DLL swapper), or `AddonOverlay` is registered
- **THEN** the engine uses the same release cache, target executable resolver, conflict/layering checker, stopped-process guard, and rollback ledger without tool-specific database tables

### Requirement: Target executable and deployment directory resolution
Before installing or inspecting tools for any installed library entry (`steam:<id>`, `manual:<id>`, or other providers), the application SHALL resolve candidate Windows executables inside `install_dir` and select a preferred main executable using manual registration settings, Ludusavi launch hints, non-launcher role, 64-bit PE architecture, and path depth. When multiple candidates exist, the user SHALL be able to override the target executable.

#### Scenario: Unreal Engine game with root bootstrap and Shipping binary
- **WHEN** a game's `install_dir` contains both a root `Game.exe` launcher/bootstrap and `Game/Binaries/Win64/Game-Win64-Shipping.exe`
- **THEN** the resolver selects `Game/Binaries/Win64/Game-Win64-Shipping.exe` as the default target executable and lists both candidates so the user can override the choice if needed

#### Scenario: Target path escaping the game folder is rejected
- **WHEN** a candidate executable or its parent directory resolves through a symlink or junction outside the game's canonical `install_dir`
- **THEN** the target is rejected and no tool files are written outside `install_dir`

### Requirement: Anti-cheat detection and explicit warning before install
Before installing any tool into a game, the application SHALL inspect the game's `install_dir` for EasyAntiCheat or BattlEye files/folders and check the bundled anti-cheat blocklist. When anti-cheat is detected, the UI SHALL display a prominent warning explaining the ban risk and SHALL require explicit user confirmation before proceeding with installation.

#### Scenario: Game folder contains EasyAntiCheat or BattlEye
- **WHEN** the user opens the Tools modal or clicks Install on a game whose folder contains `EasyAntiCheat` (`EasyAntiCheat_EOS.exe`, `EasyAntiCheat_x64.dll`) or `BattlEye` (`BEService.exe`, `BEClient_x64.dll`), or is on the blocklist
- **THEN** a prominent anti-cheat warning names the detected anti-cheat system and blocks installation until the user explicitly confirms the risk

#### Scenario: Clean single-player game folder
- **WHEN** a game has no anti-cheat markers in its folder and is not on the blocklist
- **THEN** installation proceeds without an anti-cheat warning dialog

### Requirement: Release channel fetching and content-addressed archive caching
The application SHALL fetch available `Stable` and `Nightly` releases over HTTPS exclusively from the fixed GitHub repositories declared in the tool descriptor. Downloaded `.7z` or `.zip` archives SHALL be cached under `<data_dir>/tools/cache/<tool_id>/<channel>/<version>/`, verified against SHA-256 digests, and extracted in pure Rust for reuse across games and offline installs.

#### Scenario: Select Stable or Nightly version and cache archive
- **WHEN** the user selects `Stable` or `Nightly` for OptiScaler and installs version `v0.7.9`
- **THEN** the `.7z` release asset is downloaded over HTTPS from the declared GitHub repository, cached on disk with its SHA-256 digest, and extracted in pure Rust without invoking external archive executables

#### Scenario: Install an already-cached version on a second game or offline
- **WHEN** a tool version's archive is already present in the local cache and matches its recorded SHA-256 digest
- **THEN** installing that version on another game uses the cached archive immediately without a network download

#### Scenario: Corrupted or mismatched download is rejected
- **WHEN** a downloaded archive fails SHA-256 verification (when a release digest is published) or fails archive integrity checks
- **THEN** the partial/corrupt file is deleted, no files are written to the game folder, and a localized error is shown

### Requirement: Stopped-game check, cross-tool slot conflict detection, and layered library composition
The application SHALL verify via host process listing that no process runs inside `install_dir` before mutating tool files. If another `ProxyDllInjector` claims the same proxy DLL filename, the application SHALL warn the user to pick an alternate proxy filename, replace the tool, or attach as an `AddonOverlay` plugin. When a `SiblingLibrarySwapper` or `AddonOverlay` layers onto a native or host-tool file/INI, the ledger SHALL record prior ownership and INI values for reversible uninstall.

#### Scenario: Attempt to install or uninstall while game is running
- **WHEN** the user triggers tool install, update, or uninstall while the game is running
- **THEN** the action is refused with a reason stating that the game must be closed first, and no files in the game folder are modified

#### Scenario: Proxy DLL slot collision between two tools
- **WHEN** Tool A (`OptiScaler`) is already installed as `dxgi.dll` in the game's executable directory and the user attempts to install Tool B (`AeonSR` or another proxy injector) with `dxgi.dll`
- **THEN** the engine detects the slot collision and prompts the user to select an alternate supported proxy filename or uninstall Tool A first

#### Scenario: Sibling library swapper layers over native game DLL or OptiScaler companion DLL
- **WHEN** a `SiblingLibrarySwapper` tool (e.g., replacing an FSR 4 DLL with FSR 4 RDNA 2 Lite) is installed on a game that either ships with a native FSR 4 DLL or has OptiScaler installed with `amd_fidelityfx_dx12.dll` / `amd_fidelityfx_upscaler_dx12.dll`
- **THEN** the swapper backs up the prior DLL with its `prior_owner_tool_id` (`NULL` for native game file, or `optiscaler` for OptiScaler's companion file), deploys the swapped DLL, and uninstalling either tool in any order restores the remaining tool's DLL or the original unmodded game state byte-for-byte

#### Scenario: AddonOverlay installs as an OptiScaler ASI plugin and enables LoadAsiPlugins
- **WHEN** an `AddonOverlay` tool (such as MFG Unlocker in OptiScaler plugin mode) is installed on a game where OptiScaler is active
- **THEN** the engine places the plugin into `<exe_dir>/plugins/` (as `.asi`), records the prior `[Plugins] LoadAsiPlugins` setting from `OptiScaler.ini` in the ledger, sets `LoadAsiPlugins=true` in `OptiScaler.ini` without losing user comments or edits, and upon uninstalling the plugin removes the `.asi` file (and `plugins/` if empty) and restores `LoadAsiPlugins` to its prior value

### Requirement: Transactional installation and byte-identical uninstallation
Tool installation SHALL back up any overwritten game or lower-layer tool files to `<data_dir>/private/tool-backups/<entry_key>/<tool_id>/` before deployment, place the tool's main DLL as the chosen proxy filename (`dxgi.dll` default, or wiki fallback) alongside `OptiScaler.ini` and support files (excluding `.bat`/`.sh` scripts), and record SHA-256 digests for byte-identical uninstall or rollback.

#### Scenario: Install then uninstall OptiScaler leaves game folder byte-identical
- **WHEN** OptiScaler is installed into a game folder (including when the game folder already had an existing `dxgi.dll` or `amd_fidelityfx_dx12.dll`) and is then uninstalled
- **THEN** every newly added OptiScaler file is removed, every pre-existing file is restored from backup with identical SHA-256 digests, and the game folder is byte-identical to its state before installation

#### Scenario: Rollback on mid-install failure
- **WHEN** extracting or writing a file fails partway through tool installation
- **THEN** all files written during that attempt are removed, any overwritten pre-existing files are restored from backup, and no partial installation record is committed

### Requirement: Updating or switching tool versions preserves user configuration
When updating an installed tool, switching between `Stable` and `Nightly`, or changing its proxy DLL filename, the application SHALL remove obsolete files from the prior tool version while preserving user-modified configuration files (`OptiScaler.ini`) by default and retaining the original pre-tool game file backups for future uninstallation.

#### Scenario: Update OptiScaler version while keeping edited OptiScaler.ini
- **WHEN** the user has edited `OptiScaler.ini` after installing OptiScaler `v0.7.7` and updates to a newer `Stable` or `Nightly` version
- **THEN** the proxy DLL and bundled libraries are updated to the new release, obsolete files from `v0.7.7` are removed, the user's edited `OptiScaler.ini` is preserved, and the original pre-OptiScaler game file backups remain intact for future uninstallation

#### Scenario: Switch proxy DLL filename on an installed game
- **WHEN** OptiScaler is installed as `dxgi.dll` and the user switches the proxy filename to `winmm.dll`
- **THEN** `dxgi.dll` is removed (or any pre-existing original `dxgi.dll` is restored), any pre-existing `winmm.dll` is backed up, and OptiScaler's main binary is placed as `winmm.dll`
