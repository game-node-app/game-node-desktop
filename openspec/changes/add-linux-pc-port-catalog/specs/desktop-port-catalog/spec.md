# Spec Delta

## Purpose

Make the PC ports catalog a working destination on Linux: list the entries that have a native
Linux build, derive installability from the release files rather than the upstream's `supportedOS`
claim, let the user filter native Linux and Windows-only entries, and keep blocked entries
browsable with an explicit reason. This delta describes the final state after native Linux
installs; it modifies the Windows-only requirements delivered by `add-pc-port-catalog`.

## RENAMED Requirements

- FROM: `### Requirement: Windows-compatible entries are listed`
- TO: `### Requirement: Host-compatible entries are listed`

## MODIFIED Requirements

### Requirement: Host-compatible entries are listed
PC ports SHALL list every merged entry that is not hidden and that supports the running platform, including entries without a setup recipe. On Windows an entry supports the platform when it supports Windows. On Linux an entry is listed when it supports Linux or Windows, so Linux-only entries are listed and Windows-only entries stay visible as blocked entries. macOS builds SHALL NOT list the catalog. The overlay SHALL NOT drop an entry for lacking Windows support before this platform rule is applied. Each entry SHALL show its name, original game, source console, project, and whether it offers guided setup, has a verified release, is pre-release only, or is already installed.

#### Scenario: Entry without a recipe
- **WHEN** an entry supports the running platform and has no setup files in the upstream or the overlay
- **THEN** it is listed without the guided-setup label and can still be installed

#### Scenario: Linux-only entry on Linux
- **WHEN** an entry supports only Linux and the application runs on Linux
- **THEN** it is listed and appears under the Native Linux filter

#### Scenario: Linux-only entry on Windows
- **WHEN** an entry supports only Linux and the application runs on Windows
- **THEN** it is not listed

### Requirement: Detail pane explains what an install needs
Selecting an entry SHALL show a detail pane with artwork, description, original game, developer, project source link, the licence when the overlay records one, the release that would be installed with its date and verification state, and each file or folder the user must supply with any accepted versions its recipe lists. The release file shown, including its name, size, checksum state and malware-scan state, SHALL be the asset that the install would select for the running platform, using the same selection the install plan uses. When no asset can be selected, the pane SHALL show the reason for the running platform. The pane SHALL list the platforms the entry supports and SHALL offer Install, or Open in library when the entry is installed.

#### Scenario: Inspect a guided entry
- **WHEN** the user selects an entry whose recipe requires a PS4 game folder
- **THEN** the pane lists that requirement, any accepted versions its recipe lists, and whether a console package can be used instead

#### Scenario: Installed entry
- **WHEN** the selected entry was installed by GameNode
- **THEN** the pane shows the installed version and folder and offers Open in library instead of Install

#### Scenario: Open the source
- **WHEN** the user activates the project source link
- **THEN** it opens in the system browser

#### Scenario: Linux shows the Linux asset
- **WHEN** the application runs on Linux and the selected release has `game-win64.zip` flagged by the malware scan and a clean `game-x86_64.AppImage`
- **THEN** the pane shows the AppImage's name, size and checksum state, shows no malware warning, and Install asks for no malware confirmation

## ADDED Requirements

### Requirement: Platform availability comes from the selected release
The host SHALL derive each entry's availability on the running platform from the selected release's assets and the recipe's requirements, using the same asset selection as the install plan. `supportedOS` SHALL only decide whether an entry is listed and how it is labelled before its release is known. An entry with no usable asset for the running platform SHALL be reported blocked with a reason instead of being treated as installable.

#### Scenario: Upstream claims Linux but the release has no Linux asset
- **WHEN** an entry lists Linux in `supportedOS` and its selected release contains only Windows assets
- **THEN** once the release details load, the entry is blocked on Linux with the no-Linux-asset reason, its Windows asset is never offered, and Install stays disabled

#### Scenario: Release has a native Linux asset
- **WHEN** the selected release contains a supported Linux asset and the recipe has no Linux-unavailable requirement
- **THEN** the entry is confirmed native on Linux and Install is enabled

#### Scenario: Linux asset only in an unsupported format
- **WHEN** the release's only Linux asset is a `.deb` or another format the host does not install
- **THEN** the entry is blocked on Linux with the unsupported-format reason

### Requirement: Availability is declared until release details load
Because the catalog listing carries no release assets, availability SHALL have three states: *declared* from `supportedOS` before the release details load, *native* when a supported asset exists and the recipe can run on the running platform, and *blocked* with a reason. The UI SHALL distinguish declared from native, SHALL enable Install only for native entries, and SHALL re-evaluate an entry when its release details arrive.

#### Scenario: Details not loaded yet
- **WHEN** a Linux entry declares Linux support and its release details have not loaded
- **THEN** the entry is labelled as declared for Linux and Install is not enabled until the details confirm a native asset

### Requirement: Linux platform filter separates native and Windows-only entries
The Linux catalog SHALL provide a persisted platform filter with **All** (the default), **Native Linux** and **Windows-only**. Native Linux SHALL show entries declared or native on Linux; Windows-only SHALL show entries without Linux support or blocked for lacking a Linux asset. An unknown stored value SHALL fall back to All. The filter SHALL only narrow the displayed list. Windows builds SHALL NOT show it.

#### Scenario: Filter native Linux
- **WHEN** the user selects Native Linux
- **THEN** only entries that support Linux remain visible, and the selection persists across restarts

#### Scenario: Filter Windows-only
- **WHEN** the user selects Windows-only on Linux
- **THEN** entries without Linux support remain visible and blocked, with the Windows-only explanation

#### Scenario: Unknown stored value
- **WHEN** the stored platform filter has a value this build does not know
- **THEN** the catalog opens with All selected

### Requirement: Blocked entries remain browsable and never install
A blocked entry SHALL remain selectable. Its detail pane SHALL show the platforms it supports and a translated reason Install is unavailable: Windows-only (Proton is not supported), no Linux asset, unsupported Linux format, a recipe that needs the Windows console-package tool, or a recipe not reviewed for Linux. Browsing a blocked entry SHALL NOT start a download, extraction, process launch or library registration, and the host SHALL refuse an install request for it before any such work.

#### Scenario: Windows-only entry on Linux
- **WHEN** the user opens an entry with no Linux support on Linux
- **THEN** the entry is visible, Install is disabled, and the pane explains that it is Windows-only and Proton is not supported

#### Scenario: Recipe requires console extraction
- **WHEN** an entry's only way to fill a required slot on Linux is the Windows console-package extraction tool
- **THEN** the entry is blocked with the console-extraction reason before any install starts
