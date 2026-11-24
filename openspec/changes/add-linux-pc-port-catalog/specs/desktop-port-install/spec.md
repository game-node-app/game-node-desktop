# Spec Delta

## Purpose

Install native Linux builds of PC ports on Linux with the same safety model as Windows: verified
downloads, a user-chosen folder, safe extraction, no partial registration and a launchable DRM-free
entry. Windows-only assets and Windows tools are never executed on Linux. This delta modifies the
Windows-only asset, folder and registration requirements delivered by `add-pc-port-catalog`;
Windows behaviour is unchanged.

## MODIFIED Requirements

### Requirement: Only safe releases are installed by default
An install SHALL use the release the upstream marks as verified for the entry, or the overlay's pinned release when it names one. It SHALL never install a withdrawn release. When the asset selected for the running platform has a verified checksum that changed upstream, or a malware scan flagged it, the install SHALL need an explicit confirmation naming the risk. Flags on assets for other platforms SHALL NOT trigger or replace that confirmation. An entry with no verified release SHALL show why Install is unavailable.

#### Scenario: Verified release
- **WHEN** the user installs an entry whose verified release is v0.9.38
- **THEN** that release's asset for the running platform is downloaded even if newer unverified releases exist

#### Scenario: Withdrawn release
- **WHEN** the release the overlay pins has been withdrawn upstream
- **THEN** Install is unavailable and the pane shows the withdrawal reason

#### Scenario: Changed upstream file
- **WHEN** the selected asset has only an unverified checksum
- **THEN** the install asks for confirmation before downloading and checks the file against that checksum

#### Scenario: Flag on another platform's asset
- **WHEN** the application runs on Linux, the release's Windows asset is flagged by the malware scan and its Linux AppImage is clean
- **THEN** the Linux install starts without a malware confirmation

### Requirement: Downloads are verified before use
The host SHALL download release assets and helper tools over HTTPS only. It SHALL choose the asset for the running platform (Windows on Windows, Linux on Linux) by first keeping the assets for that operating system, then applying the entry's asset filter, then keeping only formats the host can install on that platform, and only then ranking by architecture. A release whose best-architecture asset is unsupported SHALL still install a supported asset when one exists. It SHALL verify the SHA-256 checksum before extracting and SHALL discard a file that fails. Verified downloads SHALL be cached so a retry does not download them again.

#### Scenario: Checksum mismatch
- **WHEN** a downloaded asset's SHA-256 differs from the expected checksum
- **THEN** the file is deleted, nothing is extracted and the install fails with a message saying the download could not be verified

#### Scenario: Retry after a later failure
- **WHEN** an install fails after its asset was verified and the user retries
- **THEN** the cached verified asset is reused

#### Scenario: Format is checked before architecture
- **WHEN** a Linux release has `game-x86_64.tar.gz` and `game-x86_64.AppImage` and the host does not install tar archives
- **THEN** the AppImage is selected and the install does not fail with an unsupported format

### Requirement: User chooses the install folder
Before installing, the user SHALL choose a parent folder through the platform's folder picker. On Linux the picker SHALL use the desktop portal or an equivalent native dialog; when no picker can open, the setup SHALL say so and accept a typed absolute path validated by the host. Install SHALL stay disabled until a parent folder is chosen, and the application SHALL NOT offer an enabled Install on a platform where no folder can be chosen. The port SHALL be installed in a subfolder named from the recipe's folder name or the entry name. The application SHALL remember the last parent folder. It SHALL refuse a target that is not empty, lies inside another library entry's game folder, or lacks the free space the release, recipe and supplied packages need, counting a package slot's extraction estimate. A refusal SHALL be reported with a translated reason. The same picker rules SHALL apply to supplied files and folders and to picking an executable after install.

#### Scenario: Choose a folder
- **WHEN** the user installs Bloodborne's port and chooses `D:\Ports`
- **THEN** the port is installed under a new `D:\Ports\<folder name>` subfolder and `D:\Ports` is offered first next time

#### Scenario: Folder is not empty
- **WHEN** the resolved install subfolder already contains files
- **THEN** the install does not start and asks the user to choose another folder

#### Scenario: Not enough space
- **WHEN** the target volume has less free space than the asset, its extracted size and the supplied files need
- **THEN** the install does not start and states how much space is required

#### Scenario: Linux folder picker
- **WHEN** the user presses Choose folder on Linux
- **THEN** a native folder dialog opens, and the chosen folder becomes the parent folder

#### Scenario: No picker available on Linux
- **WHEN** no desktop portal or native dialog can open
- **THEN** the setup explains that the picker is unavailable and accepts a typed absolute path, and Install stays disabled until a valid folder is given

### Requirement: A completed install becomes a library entry
A completed install SHALL register one DRM-free entry whose executable is the overlay's launch executable for the running platform or the first preferred executable for the running platform that exists. On Linux the preferred-executable match SHALL ignore case, and when no preferred executable matches and the installed asset is a single AppImage or bare executable, that file SHALL be the executable. Its arguments SHALL come from the recipe, and its gameplay and launcher executables from the overlay entries for the running platform; Windows executable names SHALL NOT be used on Linux. The entry SHALL be selected in the library on request. Installing the same entry again into another folder SHALL create a separate installation.

#### Scenario: Lantern Engine install
- **WHEN** Lantern Engine finishes installing on Windows
- **THEN** the library has a Bloodborne DRM-free entry whose launcher is the Lantern Engine launcher and whose gameplay executable is `Bloodborne.exe`

#### Scenario: Preferred executable is missing
- **WHEN** none of the preferred executables exists after extraction of an archive
- **THEN** the install completes, asks the user to pick the executable inside the install folder, and registers the entry with that choice

#### Scenario: Versioned AppImage on Linux
- **WHEN** the recipe prefers `devilutionx.appimage` and the installed file is `devilutionx-1.5.5-x86_64.AppImage`
- **THEN** the AppImage is registered as the executable without asking the user

#### Scenario: Overlay names only Windows executables
- **WHEN** the overlay names `Lantern.exe` as launcher for Windows and gives no Linux launcher
- **THEN** a Linux install does not look for `Lantern.exe` and does not track it for playtime

## ADDED Requirements

### Requirement: Native Linux installs use Linux assets
On Linux the host SHALL install AppImages, bare executables, zip and 7z archives, SHALL reject other formats with a translated reason until they are supported, and SHALL reject Windows-only assets before download. After copying or extracting the asset, the host SHALL make the registered executable executable before validation. The result SHALL follow the same provenance, playtime, cancellation, recovery and no-partial-registration rules as Windows.

#### Scenario: Linux AppImage release
- **WHEN** the selected release contains a verified Linux AppImage and the recipe has no Linux-unavailable requirement
- **THEN** the host installs it in the chosen folder, makes it executable, validates it and registers the DRM-free entry

#### Scenario: Linux zip release
- **WHEN** the selected Linux asset is a zip whose game binary has no execute permission recorded or whose archive format records none
- **THEN** after extraction the registered executable is executable and the install registers it instead of failing validation

#### Scenario: Windows-only release
- **WHEN** the selected release contains only Windows assets
- **THEN** the host refuses the Linux install before downloading and reports the Windows-only reason

### Requirement: Linux recipe roots follow XDG
On Linux the recipe roots SHALL map `app` to the chosen install folder, `config` to `$XDG_CONFIG_HOME` (default `~/.config`), and `data` and `localData` to `$XDG_DATA_HOME` (default `~/.local/share`). Windows keeps its roaming and local application-data roots.

#### Scenario: Data root
- **WHEN** a recipe copies a supplied file to the `data` root on Linux
- **THEN** the file is placed under `$XDG_DATA_HOME`, or `~/.local/share` when it is unset

### Requirement: Linux launch validation tolerates in-place updates
The launch identity recorded for a Linux port SHALL identify the executable file without depending on its size, timestamps or permission bits, so that a port rewriting its own executable in place, a `touch` or a `chmod` keeps the entry launchable. A replaced executable (a different file) SHALL be handled by the existing repair flow.

#### Scenario: Port updates its executable in place
- **WHEN** a Linux port rewrites its executable in place and the user plays the entry
- **THEN** the launch is not refused as a changed target

### Requirement: Linux guided setup runs only reviewed recipes
On Linux, guided setup SHALL run only for overlay recipes marked as reviewed for Linux. An entry whose recipe has required setup files and no Linux review SHALL be blocked with the not-reviewed reason before any install starts. A release-only entry with no setup files SHALL need no review.

#### Scenario: Windows-authored recipe on Linux
- **WHEN** an entry's recipe requires a game folder and is not marked as reviewed for Linux
- **THEN** the entry is blocked on Linux with the not-reviewed reason and no setup modal opens

### Requirement: Windows-only recipes are never executed on Linux
A Linux install SHALL NOT fetch or execute the Windows PS4 PKG extractor, start Wine/Proton, or substitute a Windows executable. A required slot that can only be filled through the Windows extractor SHALL make the entry unavailable on Linux until a reviewed Linux tool and recipe exist; a slot that also accepts an extracted folder SHALL offer only the folder choice on Linux.

#### Scenario: Recipe requires console extraction
- **WHEN** a Linux catalog entry's only recipe requires the Windows PS4 PKG extractor
- **THEN** the entry remains blocked with an explicit unavailable reason and no extractor is fetched or run

#### Scenario: Package-capable slot on Linux
- **WHEN** a reviewed Linux recipe has a folder slot that can also be produced from packages
- **THEN** the Linux setup offers only the extracted-folder choice

#### Scenario: Future Proton path
- **WHEN** Proton/Wine support is proposed later
- **THEN** it is delivered by a separate change; this change does not install or launch Windows executables on Linux
