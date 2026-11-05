# Spec Delta

## ADDED Requirements

### Requirement: Running state in the library
An entry whose game is running SHALL show a "Running" indicator in the grid, in the list and in the detail pane, and the indicator SHALL clear when the game stops. The state SHALL come from tracking, whichever way the game was started.

#### Scenario: Game started from Steam
- **WHEN** the user starts a game from Steam while the GameNode window is open
- **THEN** its entry shows Running in the current view and in the detail pane

#### Scenario: Game exits
- **WHEN** the running game's last process exits
- **THEN** the Running indicator disappears from its entry

#### Scenario: Tracking unavailable
- **WHEN** tracking is unavailable on this platform
- **THEN** no entry shows Running and nothing indicates an error

## MODIFIED Requirements

### Requirement: Scans run on start, window open and demand
The application SHALL scan all loaded providers when it starts, when the window is opened, when the user asks for a rescan, and every 30 minutes while it runs in the background. Scans SHALL run off the UI thread. Only one scan SHALL run at a time, and a request made during a scan SHALL cause one more scan after it. The UI SHALL show that a scan is running.

#### Scenario: Rescan on demand
- **WHEN** the user chooses Rescan from the library toolbar, the app menu or its keyboard shortcut
- **THEN** a scan runs while the library stays usable, and new or removed games appear when it ends

#### Scenario: Repeated requests
- **WHEN** several rescans are requested while a scan is running
- **THEN** at most one further scan runs after the current one

#### Scenario: Game installed while GameNode is in the background
- **WHEN** the user installs a Steam game while GameNode runs in the background with its window closed
- **THEN** the game is in the library and tracked after the next periodic scan, without the window being opened

### Requirement: Detail pane
Selecting one entry SHALL show a detail pane with its cover, detected and matched titles, provider, install folder, match status, whether it is running, and the Play, Install, match and Add to GameNode actions. Actions SHALL be disabled with a reason when they are unavailable. Play SHALL be disabled with the reason "Running" while the game runs.

#### Scenario: Unmatched game
- **WHEN** the selected game has no GameNode match
- **THEN** the pane offers Match, Play stays available, and Add to GameNode is disabled until a match is chosen

#### Scenario: Running game
- **WHEN** the selected game is running
- **THEN** the pane shows Running, and Play is disabled with a reason saying the game is already running

### Requirement: Play and Install actions
Play and Install SHALL use the provider's launch and install rules. Play SHALL show progress until the URI is handed to the OS, and a failure SHALL show a localized error. Install SHALL open the store client on the game. Play SHALL NOT be required for tracking: a game is tracked the same way whether or not it was started from GameNode.

#### Scenario: Play from the detail pane
- **WHEN** the user presses Play on a Steam game
- **THEN** Steam is asked to run that game and GameNode stays open

#### Scenario: Close the window after Play
- **WHEN** the user presses Play and then closes the GameNode window
- **THEN** the game keeps being tracked while GameNode runs in the background

## ADDED Requirements

### Requirement: Visible playtime
Each installed game SHALL show its selected source's playtime in the grid, list and
detail pane. Details SHALL identify the source. Internal totals SHALL include the
current session and refresh while running. Missing store data SHALL appear as unknown,
not zero. Account changes SHALL clear old display values before loading the new account.

#### Scenario: Imported Steam total
- **WHEN** Steam records 754 minutes for a game and Store is selected
- **THEN** grid, list and details show 12 h 34 min, and details identify Steam

#### Scenario: Internal source and active session
- **WHEN** Internal is selected and a game is running
- **THEN** the view shows internal history plus elapsed session time without adding store totals

#### Scenario: Counter unavailable
- **WHEN** a store counter has not supplied a total for a game
- **THEN** the view shows an unavailable value rather than a fabricated zero
