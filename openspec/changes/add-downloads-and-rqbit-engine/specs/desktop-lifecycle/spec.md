# Spec Delta

## MODIFIED Requirements

### Requirement: Close returns to the background
On Windows, closing the window SHALL hide it and keep the application running with tracking, active downloads and the tray icon. It SHALL save window geometry and layout as closing did before. On other platforms in the MVP, closing the window SHALL stop the application as in step 1, stopping active transfers while preserving their resumable state.

#### Scenario: Close the window during play
- **WHEN** the user closes the window while a game is running
- **THEN** the window disappears, the tray icon stays, and the game's play keeps being tracked

#### Scenario: Close during a download on Windows
- **WHEN** the user closes the window while a transfer is downloading
- **THEN** the window disappears, the transfer keeps downloading, and the tray remains available

#### Scenario: Close on Linux
- **WHEN** the user closes the window on Linux
- **THEN** the application stops, releases single-instance resources, and active transfers keep their resumable state for the next start

### Requirement: Quit ends the application
Quit in the app menu and in the tray menu SHALL end any open session at that moment, save state, stop the bundled transfer engine gracefully after preserving resumable state, release single-instance resources and stop the process. If an installer is running where automatic installation is available, Quit SHALL ask for confirmation and terminate the install job without creating an entry.

#### Scenario: Quit from the tray
- **WHEN** the user chooses Quit in the tray menu while a game is running
- **THEN** the session ends at that time, the tray icon disappears and no GameNode process remains

#### Scenario: Quit during a download
- **WHEN** the user quits while a transfer is downloading or seeding
- **THEN** the engine stops without corrupting data and the transfer resumes from its persisted state on the next start

#### Scenario: Quit during an installation
- **WHEN** the user quits on Windows while an installer is running
- **THEN** a confirmation explains that the install job will be terminated, and confirming leaves no entry and no GameNode process
