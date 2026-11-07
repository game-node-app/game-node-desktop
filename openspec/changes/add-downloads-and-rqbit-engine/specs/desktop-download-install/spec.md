# Spec Delta

## Purpose

Turn a completed transfer into a playable library entry on Windows by detecting an installer or portable game, obtaining explicit consent, running the installer, and registering the result.

## ADDED Requirements

### Requirement: Installer discovery
After a transfer completes, the application SHALL inspect its content for a Windows installer such as `setup.exe` or `install.exe`, excluding uninstallers, updaters and redistributables. Exactly one plausible installer SHALL be proposed directly; several SHALL be offered for the user to choose; none SHALL switch the flow to portable-game detection.

#### Scenario: One installer
- **WHEN** a completed transfer contains exactly one plausible installer
- **THEN** the automatic installation flow proposes that installer

#### Scenario: Several installers
- **WHEN** a completed transfer contains more than one plausible installer
- **THEN** the user chooses which one to run and no installer runs before that choice

#### Scenario: Only maintenance executables
- **WHEN** the transfer contains only uninstallers, updaters or redistributables
- **THEN** they are not proposed as installers and the flow treats the content as a portable or manual case

### Requirement: Per-install confirmation
Before executing any installer, the application SHALL show a confirmation with its path, size, SHA-256, publisher metadata when available, and a warning that it runs with the user's privileges. Execution SHALL begin only after explicit confirmation. Cancelling SHALL change nothing on disk or in the library.

#### Scenario: Confirm the installer
- **WHEN** the user confirms the installer details
- **THEN** the install job starts and its state is shown in Downloads

#### Scenario: Cancel the confirmation
- **WHEN** the user cancels the installer confirmation
- **THEN** the installer does not run, the transfer stays completed and no entry is created

#### Scenario: Details shown
- **WHEN** the confirmation is displayed
- **THEN** the path, size, SHA-256 and available publisher metadata are visible before the user confirms

### Requirement: Installer execution
The application SHALL run the installer as a child process with its working directory inside the transfer, without a shell and without automatic elevation. The installer's own window SHALL remain visible. The application SHALL report the exit status and SHALL NOT claim success before the installer exits with success.

#### Scenario: Successful exit
- **WHEN** the installer exits with success
- **THEN** the job moves to result detection

#### Scenario: Failed exit
- **WHEN** the installer exits with an error or is terminated
- **THEN** the job reports failure, creates no entry and offers the folder action

#### Scenario: Elevation requested
- **WHEN** the installer requests administrator elevation
- **THEN** the operating system owns the prompt and the application does not bypass or suppress it

#### Scenario: Quit during installation
- **WHEN** the user quits while an installer is running
- **THEN** the application asks for confirmation and terminates the install job without creating an entry

### Requirement: Install result detection
After a successful exit, the application SHALL determine the installed folder and launch executable from evidence: the chosen destination, new uninstall registry entries, changed folders and executable metadata, reusing the DRM-free discovery rules. An unambiguous result SHALL be proposed for confirmation; an ambiguous result SHALL ask the user to choose; no result SHALL fall back to the folder action.

#### Scenario: Detected installation
- **WHEN** exactly one installed game folder and executable can be evidenced
- **THEN** the result is proposed prefilled with title, folder and executable

#### Scenario: Ambiguous installation
- **WHEN** several plausible folders or executables remain
- **THEN** the user chooses before anything is added

#### Scenario: No result
- **WHEN** no installed game can be evidenced
- **THEN** no entry is created and the app offers to open the folder or choose an executable manually

### Requirement: Portable game association
When the completed content contains no installer but contains a recognized game folder, the application SHALL offer to add it directly through the existing DRM-free discovery and review flow, without executing anything.

#### Scenario: Recognized portable game
- **WHEN** a completed transfer contains a recognized game folder
- **THEN** the app offers to add it directly and shows the inferred title and executable

#### Scenario: Unrecognized content
- **WHEN** the content has no recognized game
- **THEN** the user can choose an executable manually or open the folder, and nothing is executed

### Requirement: Entry creation and provenance
An accepted install SHALL create one DRM-free entry with a stable manual installation id, the detected executable, provenance naming the transfer info hash, and background matching. Re-adding the same canonical executable SHALL select the existing entry. Removing the entry SHALL NOT delete the transfer or the installed files.

#### Scenario: Entry created
- **WHEN** the user accepts the detected result
- **THEN** one playable entry appears in the library with provenance to the transfer and matching runs in the background

#### Scenario: Duplicate executable
- **WHEN** the detected executable is already registered
- **THEN** the existing entry is selected instead of creating a second installation

#### Scenario: Remove the entry
- **WHEN** the user removes the downloaded game from the library
- **THEN** the transfer files and the installed files remain on disk

### Requirement: Automatic installation platform scope
Automatic installation SHALL be offered only on Windows in this change. On Linux the action SHALL be disabled with a localized explanation that Proton/Wine integration is pending, and Abrir pasta de destino SHALL remain available. No Wine or Proton runtime, prefix, launch registration or entry SHALL be created. Linux automatic installation SHALL be revisited only after the Proton/Wine integration is configured and functional, and not before.

#### Scenario: Linux disabled state
- **WHEN** a transfer completes on Linux
- **THEN** Instalação automática is disabled with a localized explanation and Abrir pasta de destino is available

#### Scenario: No Linux side effects
- **WHEN** the Linux disabled state is shown or the user opens the destination folder
- **THEN** no runtime is probed, no prefix is created and no library entry is added

#### Scenario: Deferred work is not claimed
- **WHEN** Downloads, Settings or the library details are inspected on Linux
- **THEN** no control claims that Proton or Wine installation is supported in this change

### Requirement: Failure fallback leaves the download intact
Any discovery, consent, execution, detection or registration failure SHALL leave the transfer and its files untouched, create no partial entry, and offer Abrir pasta de destino and manual executable selection.

#### Scenario: Installer fails
- **WHEN** the installer exits with an error
- **THEN** the transfer stays completed, its files remain, and the folder action is offered

#### Scenario: Detection fails
- **WHEN** the installer succeeds but no game can be identified
- **THEN** no entry is created and the user can open the folder or choose an executable

#### Scenario: Registration fails
- **WHEN** storing the new entry fails
- **THEN** no partial entry remains and the failure is reported with the folder action

### Requirement: One install job at a time
Only one installer SHALL run at a time. Starting another SHALL be refused with an explanation until the current job finishes or is cancelled. Transfers MAY continue while an installer runs.

#### Scenario: Second install refused
- **WHEN** the user starts automatic installation while another install job is running
- **THEN** the second job is refused with an explanation and the first continues

#### Scenario: Downloads during install
- **WHEN** an installer is running and another transfer is downloading
- **THEN** the download continues to progress
