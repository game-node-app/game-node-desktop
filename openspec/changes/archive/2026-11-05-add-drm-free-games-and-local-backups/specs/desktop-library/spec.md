# Spec Delta

## MODIFIED Requirements

### Requirement: Uninstalled games leave the library
An entry that a completed store-provider scan no longer finds SHALL leave the library view. Its match and cover SHALL be kept, so that a reinstalled game returns with them. A provider whose scan failed SHALL keep its previous entries. Approved manual registrations SHALL instead remain visible as unavailable until repaired or explicitly removed, preserving matches, playtime and backup history.

#### Scenario: Game uninstalled
- **WHEN** a game is uninstalled through Steam and the library rescans
- **THEN** the game is no longer listed

#### Scenario: Game reinstalled
- **WHEN** a previously matched game is installed again and the library rescans
- **THEN** it reappears with its previous match and cover

#### Scenario: Scan fails for one provider
- **WHEN** a provider's roots cannot be read during a scan
- **THEN** that provider's entries from the last successful scan remain, and the library shows that its scan failed

#### Scenario: Manual executable missing
- **WHEN** a manually registered executable is unavailable during a scan
- **THEN** its entry stays visible, Play explains why it is unavailable, and Locate executable is offered

### Requirement: Empty library guidance
When the scan finds no installed games, the library SHALL say so, name the supported store sources it looked for, and offer Rescan. On Windows it SHALL additionally offer Add game and explain that local DRM-free games can be added from an executable without selecting a GameNode match first.

#### Scenario: No supported store found
- **WHEN** a scan completes with no games from any provider
- **THEN** the library explains that no installed games were found and offers Rescan, plus Add game on Windows

### Requirement: Play and Install actions
Play and Install SHALL use the provider's launch and install rules. Play SHALL show progress until the URI is handed to the OS or the executable start request completes, and a failure SHALL show a localized error. Install SHALL open the store client on the game and SHALL be unavailable for manual entries. Starting a process SHALL not by itself claim that the game is running; that state SHALL come from tracking.

#### Scenario: Play from the detail pane
- **WHEN** the user presses Play on a Steam game
- **THEN** Steam is asked to run that game and GameNode stays open

#### Scenario: Close the window after Play
- **WHEN** the user presses Play and then closes the GameNode window
- **THEN** the game keeps being tracked while GameNode runs in the background

#### Scenario: Unmatched manual game
- **WHEN** the user presses Play on an unmatched DRM-free game
- **THEN** its executable is started and no metadata dialog blocks play

#### Scenario: Executable fails to start
- **WHEN** Windows refuses to start the executable
- **THEN** launch progress clears with a localized error and no running state or backup is fabricated

## ADDED Requirements

### Requirement: DRM-free details expose protection and maintenance
A DRM-free game's detail pane and context menu SHALL expose applicable launch editing, Locate executable, Remove, matching and save-protection actions. Details SHALL show the automatic-match provenance, tracking configuration and save-protection status. Controls SHALL be keyboard accessible and all user-facing strings SHALL be available in English and Brazilian Portuguese.

#### Scenario: Known game with saves
- **WHEN** a supported DRM-free game is selected
- **THEN** details show its backup status and history alongside Play and Change match

#### Scenario: Unsupported backup
- **WHEN** the selected game has no known save profile
- **THEN** details explain the missing support and offer save-location setup while Play remains available
