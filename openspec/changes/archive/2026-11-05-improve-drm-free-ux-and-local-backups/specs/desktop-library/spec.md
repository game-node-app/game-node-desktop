# Spec Delta

## MODIFIED Requirements

### Requirement: Context menus
Each entry SHALL have a context menu, opened by right-click or the keyboard menu key, with the actions available for that entry: Play, Install (omitted when the provider has no install action, such as DRM-free entries), Match or Change match, and Add to GameNode. With several entries selected, the menu SHALL offer only the actions that work on several games.

#### Scenario: Context menu on one game
- **WHEN** the user opens the context menu on a matched, installed Steam game
- **THEN** Play, Change match and Add to GameNode are offered

#### Scenario: Context menu on a DRM-free game
- **WHEN** the user opens the context menu on a DRM-free game
- **THEN** Play, Game settings, Locate executable, Remove from library, Local save protection, Match or Change match, and Add to GameNode are offered, and Install is not shown

#### Scenario: Context menu on a multi-selection
- **WHEN** several games are selected
- **THEN** single-game actions such as Play and Change match are not offered

### Requirement: Detail pane
Selecting one entry SHALL show a detail pane with its cover, detected and matched titles, provider, install folder, match status, whether it is running, and the Play, Install, match and Add to GameNode actions. When the entry's provider has no install action (such as DRM-free games), the Install button and its reason SHALL be omitted. Other actions SHALL be disabled with a reason when unavailable, including "Running" for Play.

#### Scenario: Unmatched game
- **WHEN** the selected game has no GameNode match
- **THEN** the pane offers Match, Play stays available, and Add to GameNode is disabled until a match is chosen

#### Scenario: Running game
- **WHEN** the selected game is running
- **THEN** the pane shows Running, and Play is disabled with a reason saying the game is already running

#### Scenario: DRM-free game details omit Install
- **WHEN** a DRM-free game is selected
- **THEN** the detail pane shows Play and DRM-free actions without rendering an Install button or a "no install action" caption

### Requirement: Play and Install actions
Play and Install SHALL use the provider's launch and install rules. Double-clicking an entry or pressing Enter on a focused/selected entry in grid or list view SHALL select it and trigger Play when available, or open its detail pane when Play is unavailable. Play SHALL show progress until handed to the OS, and a failure SHALL show a localized error. Install SHALL open the store client. A game is tracked the same way whether or not it was started from GameNode.

#### Scenario: Play from the detail pane
- **WHEN** the user presses Play on a Steam game
- **THEN** Steam is asked to run that game and GameNode stays open

#### Scenario: Double-click or press Enter on a playable game in the library
- **WHEN** the user double-clicks a playable game tile in grid view or row in list view, or focuses it and presses Enter
- **THEN** that game is selected and launched immediately

#### Scenario: Double-click or press Enter on an unplayable game in the library
- **WHEN** the user double-clicks or presses Enter on a game whose Play action is unavailable (such as a running game or a DRM-free entry with a missing executable)
- **THEN** that game is selected and its detail pane opens showing why Play is unavailable

#### Scenario: Close the window after Play
- **WHEN** the user presses Play and then closes the GameNode window
- **THEN** the game keeps being tracked while GameNode runs in the background

## ADDED Requirements

### Requirement: Automatic watched-folder discovery and library notice
Recognized DRM-free installations found in watched folders SHALL be added to the library automatically with a preferred launch executable and matched in the background. When watched-folder scans add new entries, the library view SHALL show a dismissible notice banner identifying the newly added games. Removing a DRM-free entry SHALL suppress automatic re-addition until ignored discoveries are reset in Settings.

#### Scenario: New game discovered in a watched folder
- **WHEN** a watched folder scan finds a recognized DRM-free installation that is not already registered or ignored
- **THEN** the installation is automatically added to the library, background GameNode matching and save-profile resolution begin, and a notice banner appears in the library view

#### Scenario: Discovered folder has multiple executables
- **WHEN** a recognized installation in a watched folder contains both 32-bit and 64-bit game executables
- **THEN** discovery selects the preferred executable automatically so the entry is added immediately without blocking on a manual Settings checklist

#### Scenario: Removed game is not re-added on rescan
- **WHEN** the user removes a DRM-free entry from the library and the watched folder is rescanned
- **THEN** the removed installation is not re-added until the user resets ignored discoveries in Settings or adds its executable manually

### Requirement: Switching among detected launch executables
When a DRM-free installation folder contains multiple candidate game executables, the game settings dialog SHALL list those detected executables and allow the user to select any of them as the launch target with one action.

#### Scenario: Switch executable from Game settings
- **WHEN** a DRM-free game with multiple detected executables in its folder is opened in Game settings
- **THEN** the dialog lists the detected executables and choosing one updates the executable field

### Requirement: Per-game local save protection dialog
Opening local save protection for a DRM-free game SHALL automatically load its backup history and display the active save profile title, the exact save folders, files, and registry keys being backed up, and the backup destination folder. Snapshots SHALL display human-readable sizes and offer a restore preview showing the targets to be replaced before confirmation.

#### Scenario: View configured save protection and folders
- **WHEN** the user opens Local save protection for a DRM-free game with a resolved save profile
- **THEN** the modal displays the profile name, the resolved save file or folder paths and registry keys to be backed up, the backup archive destination, and automatically loads existing backup snapshots

#### Scenario: Preview and confirm restore
- **WHEN** the user selects a backup snapshot to restore
- **THEN** the modal shows the target paths and registry keys that will be replaced after creating a recovery backup, and offers Cancel and Confirm restore
