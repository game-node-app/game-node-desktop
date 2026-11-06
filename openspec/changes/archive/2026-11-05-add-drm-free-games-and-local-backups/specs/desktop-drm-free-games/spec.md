# Spec Delta

## Purpose

Make locally installed DRM-free Windows games easy to add, identify, launch and maintain, with reviewed discovery that keeps unrelated programs out of the library.

## ADDED Requirements

### Requirement: Add a game from one executable selection
The application SHALL offer Add game in the library toolbar and empty state. Selecting a valid Windows game executable SHALL create a playable DRM-free entry without requiring a title, GameNode match or save profile. Enrichment SHALL run in the background. Cancelling SHALL create nothing.

#### Scenario: Quick add
- **WHEN** the user chooses Add game and selects `F:\NonSteamLibrary\Celeste\Celeste.exe`
- **THEN** the entry is persisted and playable without a second metadata selection
- **AND** title inference, matching and save support are resolved asynchronously

#### Scenario: Add offline
- **WHEN** a signed-in user with a cached library selects an executable while offline
- **THEN** the entry is added and playable with a local title and matching is retried when access is verified

#### Scenario: Invalid selection
- **WHEN** the selection is missing, is a directory, or is not a supported executable
- **THEN** no entry is created and the picker or add flow explains the problem

### Requirement: Installation identity survives repairs
Each manual installation SHALL have a stable provider-local identity independent of its title, executable path and GameNode match. Re-adding the same canonical executable SHALL select its existing entry. Different installations SHALL remain separate even when they match the same GameNode game.

#### Scenario: Duplicate selection
- **WHEN** an executable already added is selected through a different path spelling or junction
- **THEN** the application selects the existing entry without duplicating it

#### Scenario: Two copies
- **WHEN** two different installations of the same game are added
- **THEN** each has its own launch settings, identity and backup history

### Requirement: Editable executable and tracking settings
A DRM-free entry SHALL expose its title, game folder, executable, working directory, argument list and tracked game executables for editing. Simple adds SHALL infer usable defaults. Ambiguous game-folder or executable choices SHALL remain visible and correctable; a watched library root SHALL NOT become a game's tracking folder.

#### Scenario: Nested executable
- **WHEN** a game is added from `F:\NonSteamLibrary\Foo\Binaries\Win64\Foo.exe`
- **THEN** the application proposes the evidenced game root when available and otherwise uses a conservative folder
- **AND** the user can correct the root and working directory without recreating the entry

#### Scenario: Launcher needs configuration
- **WHEN** the selected executable starts another game executable that cannot be identified reliably
- **THEN** the entry stays launchable and exposes a way to select the actual game executable for tracking

### Requirement: Watched folders require reviewed discovery
Folder monitoring SHALL be opt-in. Newly discovered installations SHALL appear as candidates outside the library until the user approves them. The review SHALL show the inferred title, folder, executable, recognition evidence and ambiguities, and SHALL support Add selected, Ignore and executable correction. Discovery SHALL never execute candidates.

#### Scenario: Review a new folder
- **WHEN** the user watches `F:\NonSteamLibrary` and discovery finds two plausible games
- **THEN** a review list offers both candidates and neither is tracked, launched or backed up before approval

#### Scenario: Background discovery
- **WHEN** a plausible game is copied into a watched folder while the window is hidden
- **THEN** it becomes a pending review item without interrupting play or opening the window

### Requirement: Discovery filters non-game executables
Discovery SHALL require positive game evidence beyond an `.exe` extension or fuzzy folder-name similarity. Known setup, uninstall, updater, crash reporter, redistributable and support-tool candidates SHALL be excluded as primary games. Several possible launch targets in one installation SHALL form one unresolved candidate rather than separate games.

#### Scenario: Installer beside the game
- **WHEN** a recognized game folder contains a game executable, `unins000.exe` and a redistributable installer
- **THEN** review offers the game and does not offer the maintenance executables as games

#### Scenario: Unknown folder
- **WHEN** a folder contains only unidentified executables and no positive game evidence
- **THEN** it is omitted from game candidates and Add game remains available for explicit selection

#### Scenario: Several launch targets
- **WHEN** a recognized installation has both 32-bit and 64-bit game executables with no unique preferred target
- **THEN** one review item asks the user to choose a launch target before adding

### Requirement: Folder monitoring is bounded and resilient
Monitoring SHALL process only selected roots, coalesce filesystem events and reconcile missed events on startup and rescan. Work SHALL remain off the UI thread with bounded traversal and cancellation. Overlapping roots SHALL not duplicate candidates. Inaccessible folders and links outside selected roots SHALL not trigger unbounded scans or deletion of existing entries.

#### Scenario: Large copy operation
- **WHEN** hundreds of files arrive in a watched folder
- **THEN** discovery coalesces events while the library remains responsive and each installation appears at most once

#### Scenario: Drive unplugged
- **WHEN** a watched drive becomes unavailable
- **THEN** the folder is shown as unavailable, entries and review decisions are retained, and discovery resumes after reconnection

### Requirement: Repair and remove local entries
Unavailable manual entries SHALL remain visible with Locate executable and Remove actions. Repair SHALL retain identity, matches, playtime and backup history. Removal SHALL only remove the registration and suppress rediscovery until explicitly reconsidered; it SHALL NOT delete installed game files, live saves or backup archives. Removing a watch SHALL stop discovery without removing approved games.

#### Scenario: Move a game
- **WHEN** an executable has moved and the user locates its new path
- **THEN** the same entry becomes playable with its existing history and matches

#### Scenario: Remove a game registration
- **WHEN** the user removes a DRM-free entry and the watched folder rescans
- **THEN** the entry is absent and is not suggested again until the user resets its ignored state or explicitly adds it
- **AND** the installation, saves and backup archives remain on disk
