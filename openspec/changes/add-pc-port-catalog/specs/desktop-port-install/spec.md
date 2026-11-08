# Spec Delta

## Purpose

Install a community native PC port on Windows from the user's own game files, verify what is downloaded, place or extract the files the port needs, and leave a launchable DRM-free library entry. GameNode never supplies game content.

## ADDED Requirements

### Requirement: Only safe releases are installed by default
An install SHALL use the release the upstream marks as verified for the entry, or the overlay's pinned release when it names one. It SHALL never install a withdrawn release. Files whose verified checksum changed upstream, or that a malware scan flagged, SHALL need an explicit confirmation naming the risk. An entry with no verified release SHALL show why Install is unavailable.

#### Scenario: Verified release
- **WHEN** the user installs an entry whose verified release is v0.9.38
- **THEN** that release's Windows asset is downloaded even if newer unverified releases exist

#### Scenario: Withdrawn release
- **WHEN** the release the overlay pins has been withdrawn upstream
- **THEN** Install is unavailable and the pane shows the withdrawal reason

#### Scenario: Changed upstream file
- **WHEN** the selected asset has only an unverified checksum
- **THEN** the install asks for confirmation before downloading and checks the file against that checksum

### Requirement: Downloads are verified before use
The host SHALL download release assets and helper tools over HTTPS only. It SHALL choose the Windows asset through the entry's asset filter and the asset's operating system and architecture. It SHALL verify the SHA-256 checksum before extracting and SHALL discard a file that fails. Verified downloads SHALL be cached so a retry does not download them again.

#### Scenario: Checksum mismatch
- **WHEN** a downloaded asset's SHA-256 differs from the expected checksum
- **THEN** the file is deleted, nothing is extracted and the install fails with a message saying the download could not be verified

#### Scenario: Retry after a later failure
- **WHEN** an install fails after its asset was verified and the user retries
- **THEN** the cached verified asset is reused

### Requirement: User chooses the install folder
Before installing, the user SHALL choose a parent folder. The port SHALL be installed in a subfolder named from the recipe's folder name or the entry name. The application SHALL remember the last parent folder. It SHALL refuse a target that is not empty, lies inside another library entry's game folder, or lacks the free space the release and recipe need.

#### Scenario: Choose a folder
- **WHEN** the user installs Bloodborne's port and chooses `D:\Ports`
- **THEN** the port is installed under a new `D:\Ports\<folder name>` subfolder and `D:\Ports` is offered first next time

#### Scenario: Folder is not empty
- **WHEN** the resolved install subfolder already contains files
- **THEN** the install does not start and asks the user to choose another folder

#### Scenario: Not enough space
- **WHEN** the target volume has less free space than the asset, its extracted size and the supplied files need
- **THEN** the install does not start and states how much space is required

### Requirement: Release archives are extracted safely
The host SHALL extract release archives in-process into the install folder. It SHALL reject entries with absolute paths, parent traversal, links that escape the folder, or a total size above a fixed limit. It SHALL apply the recipe's files to add after extraction. A failed extraction SHALL remove everything it wrote.

#### Scenario: Path traversal
- **WHEN** an archive contains an entry named `..\..\Windows\evil.dll`
- **THEN** extraction stops, the partial install is removed and the error is reported

#### Scenario: Portable marker
- **WHEN** the recipe lists `portable.txt` in its files to add
- **THEN** an empty `portable.txt` exists in the install folder after extraction

### Requirement: Guided setup asks for the user's own files
For each setup file in the merged recipe, the install SHALL show its label and prompt and ask the user to choose a file or folder of the declared kind and extensions. Required slots SHALL block the install until filled. Optional slots SHALL be skippable. The application SHALL NOT download, search for or suggest sources of game content.

#### Scenario: Required folder
- **WHEN** a recipe requires a game folder and the user has not chosen one
- **THEN** Install stays disabled with a caption naming the missing folder

#### Scenario: Optional file
- **WHEN** a recipe slot is marked not required
- **THEN** the user can continue without choosing it

### Requirement: Supplied files are validated
A supplied file SHALL be accepted when its SHA-1 or SHA-256 matches any accepted hash, or when the slot lists no hashes. A supplied folder SHALL be accepted when it contains every declared marker file and its marker hashes match when given. Hashing SHALL run in the background with progress. A rejected choice SHALL name the accepted versions.

#### Scenario: Wrong file for a recipe that lists hashes
- **WHEN** a recipe lists accepted hashes for a slot and the user picks a file that matches none of them
- **THEN** the choice is rejected and the message names the accepted versions

#### Scenario: Port decides which edition it runs
- **WHEN** a folder slot lists marker files and no hashes, and the user picks a folder that has every marker
- **THEN** the choice is accepted without checking the game's edition or version

#### Scenario: Large file hashing
- **WHEN** the user picks a multi-gigabyte disc image
- **THEN** hashing shows progress and the window stays responsive

### Requirement: Supplied files are placed or passed as the recipe says
An accepted supplied file or folder SHALL be copied to the recipe's destination under the declared root (the port folder, roaming data, settings or local data), or passed to the port by flag, positional argument or environment variable. The user's original files SHALL never be moved or modified. Copies SHALL be verified after writing.

#### Scenario: Copy to a destination
- **WHEN** a recipe places the game folder at `CUSA03173` under the port folder
- **THEN** the folder is copied there with progress, verified, and the user's source folder is unchanged

#### Scenario: Pass by flag
- **WHEN** a recipe passes a disc image with the flag `--iso`
- **THEN** the library entry's arguments include `--iso` followed by the image path and no copy is made

### Requirement: Console packages can be extracted with a pinned tool
When the overlay lets a folder slot be produced from console packages, the user SHALL be able to choose the base package and, where required, its update. The host SHALL download the pinned tool from its fixed GitHub release, verify its SHA-256 and run it without a shell, with arguments built only from validated paths. When the template has no `{update}`, the host SHALL run it for each package and merge the update over the base. The merged result SHALL be validated as a supplied folder.

#### Scenario: Extract a PS4 package
- **WHEN** the user chooses a base `.pkg` and its update `.pkg` for a PS4 game slot
- **THEN** the tool extracts the base into the staging folder and the update into a separate temporary folder, the update files replace or join the base at the same relative paths, the temporary folder is removed, and the merged folder is checked against the slot's markers and any hashes it lists and placed as the recipe says

#### Scenario: Update extraction escapes the game folder
- **WHEN** the update tree contains a symbolic link, a reparse point or a path that resolves outside the game folder
- **THEN** the merge is refused, the staging folder and temporary update folder are removed, and the install fails

#### Scenario: Tool fails
- **WHEN** the extraction tool exits with an error or the merged folder fails validation
- **THEN** the staging folder and temporary update folder are removed and the install fails with the tool's last output available in details

### Requirement: Installs run as cancellable background jobs
Each install SHALL run as a background job with phases (download, verify, extract, supplied files, register), a progress figure and a cancel action. Jobs SHALL continue while the window is hidden. Cancelling or failing SHALL remove the install folder and staging data that the job created and SHALL leave no library entry.

#### Scenario: Cancel during copy
- **WHEN** the user cancels while a dump is being copied
- **THEN** copying stops, the files the job created are removed and no library entry is added

#### Scenario: Close the window during an install
- **WHEN** the user closes the window on Windows while an install runs
- **THEN** the install continues and its result is shown when the window opens again

### Requirement: Interrupted installs are recovered on start
On startup, the host SHALL find install jobs left unfinished by a crash or Quit. It SHALL remove their partial files and staging data, keep verified cached downloads, and mark them as interrupted with a Retry action. An install whose library registration had already been committed SHALL be kept as completed.

#### Scenario: Quit during extraction
- **WHEN** the application quits while a release is being extracted and starts again
- **THEN** the partial install folder is removed and PC ports shows the install as interrupted with Retry

### Requirement: A completed install becomes a library entry
A completed install SHALL register one DRM-free entry whose executable is the overlay's launch executable or the first preferred Windows executable that exists. Its arguments SHALL come from the recipe, and its gameplay and launcher executables from the overlay. The entry SHALL be selected in the library on request. Installing the same entry again into another folder SHALL create a separate installation.

#### Scenario: Lantern Engine install
- **WHEN** Lantern Engine finishes installing
- **THEN** the library has a Bloodborne DRM-free entry whose launcher is the Lantern Engine launcher and whose gameplay executable is `Bloodborne.exe`

#### Scenario: Preferred executable is missing
- **WHEN** none of the preferred executables exists after extraction
- **THEN** the install completes, asks the user to pick the executable inside the install folder, and registers the entry with that choice

### Requirement: Self-updating ports are left alone
GameNode SHALL NOT update, overwrite or verify the files of an installed port after the install completes. A port that updates itself SHALL keep its registration, and repair SHALL handle an executable that moved. PC ports SHALL show when a newer verified release exists and SHALL offer the project page to update it.

#### Scenario: Port updated itself
- **WHEN** Lantern Engine replaces its own files through its launcher
- **THEN** GameNode does not restore or flag the changed files, and the library entry still launches it

#### Scenario: Newer release exists
- **WHEN** the catalog's verified release is newer than the installed version
- **THEN** the detail pane says an update is available and links to the project instead of reinstalling over the folder
