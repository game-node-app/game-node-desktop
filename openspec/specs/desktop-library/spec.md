# desktop-library Specification

## Purpose

Keep one local library of the games installed on this PC, from every loaded provider, and let the user browse, select and act on them in a native desktop view that opens from local data.

## Requirements

### Requirement: Library entries are keyed by provider
Each detected game SHALL be one library entry with the key `<provider>:<id>`. The same game found by two providers SHALL be two entries. A later scan SHALL update an entry with the same key and SHALL NOT duplicate it.

#### Scenario: Rescan an unchanged PC
- **WHEN** a scan finds the same installed games as the previous one
- **THEN** the library shows the same entries, with their matches and covers kept

#### Scenario: Game moved to another library folder
- **WHEN** a Steam game's install folder changes between scans
- **THEN** its entry keeps its key, match and cover, and shows the new folder

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

### Requirement: Grid and list views
The signed-in main view SHALL show the library as a cover grid or a list, which the user can switch between. The list SHALL show at least the title, provider and match status. Each entry SHALL show its GameNode cover when one is cached and a titled placeholder otherwise.

#### Scenario: Switch views
- **WHEN** the user switches from grid to list
- **THEN** the same entries, filter, sort and selection are shown as a list

#### Scenario: No cover available
- **WHEN** an entry is unmatched or its cover is not cached
- **THEN** it shows a placeholder with the detected title

### Requirement: Filter, search and sort
A sidebar SHALL list "All games" and each loaded provider with its count of installed games, and selecting one SHALL filter the view. A search field SHALL filter by title. The user SHALL be able to sort by title, provider or match status, ascending or descending.

#### Scenario: Filter by provider and search
- **WHEN** the user selects Steam in the sidebar and types part of a title
- **THEN** only Steam entries whose detected or matched title contains that text are shown

#### Scenario: Nothing matches
- **WHEN** the filter and search exclude every entry
- **THEN** the view states that nothing matches and offers to clear the search

### Requirement: Empty library guidance
When the scan finds no installed games, the library SHALL say so, name the supported store sources it looked for, and offer Rescan. On Windows it SHALL additionally offer Add game and explain that local DRM-free games can be added from an executable without selecting a GameNode match first.

#### Scenario: No supported store found
- **WHEN** a scan completes with no games from any provider
- **THEN** the library explains that no installed games were found and offers Rescan, plus Add game on Windows

### Requirement: Keyboard navigation and multi-select
The library SHALL support keyboard navigation in both views, opening the selected game's details, Play from the keyboard, and multi-select with Ctrl/Shift and the mouse or keyboard. Focus SHALL always be visible.

#### Scenario: Browse by keyboard
- **WHEN** the user moves through the grid with the arrow keys and presses Enter
- **THEN** the focused game is selected and its detail pane shows it

#### Scenario: Select several games
- **WHEN** the user extends the selection with Shift or Ctrl
- **THEN** several entries are selected and actions that apply to several games are offered for all of them

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
Play and Install SHALL use the provider's launch and install rules. Double-clicking an entry or pressing Enter on a focused/selected entry in grid or list view SHALL select it and trigger Play when available, or open its detail pane when Play is unavailable. Play SHALL show progress until the URI is handed to the OS or the executable start request completes, and a failure SHALL show a localized error. Install SHALL open the store client on the game and SHALL be unavailable for manual entries. A game SHALL be tracked the same way whether or not it was started from GameNode; a successful process start alone SHALL not claim that it is running, which SHALL come from tracking.


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

#### Scenario: Unmatched manual game
- **WHEN** the user presses Play on an unmatched DRM-free game
- **THEN** its executable is started and no metadata dialog blocks play

#### Scenario: Executable fails to start
- **WHEN** Windows refuses to start the executable
- **THEN** launch progress clears with a localized error and no running state or backup is fabricated
### Requirement: Cover cache
Covers SHALL be downloaded from the image URL GameNode returns for the matched game and stored under the per-user data directory. A cached cover SHALL be shown without the network. Only HTTPS image URLs on the GameNode image host SHALL be downloaded. A failed download SHALL fall back to the placeholder and be retried on a later scan.

#### Scenario: Open the library offline
- **WHEN** the application starts without network access after covers were cached
- **THEN** the library shows the cached covers

#### Scenario: Cover URL on an unexpected host
- **WHEN** a cover URL is not HTTPS or not on the expected image host
- **THEN** it is not downloaded and the entry shows the placeholder

#### Scenario: Match changes
- **WHEN** an entry's match changes to another GameNode game
- **THEN** the new game's cover replaces the old one

### Requirement: Remembered layout
The application SHALL remember the view mode, sidebar selection, sort, the size of the sidebar and detail pane, and whether the detail pane is open, and SHALL restore them on the next start. The search text SHALL NOT be remembered.

#### Scenario: Restart with a changed layout
- **WHEN** the user switches to list view, sorts by provider, resizes the detail pane and restarts
- **THEN** the library opens in list view, sorted by provider, with the same pane size

### Requirement: Library opens from local data
The library SHALL be shown from local data before any network request finishes. It SHALL remain usable while GameNode is unreachable. Matching, game search and Add to GameNode SHALL be disabled with an explanation until a verified session and the network are available.

#### Scenario: Offline after a verified session
- **WHEN** GameNode becomes unreachable while the user is signed in
- **THEN** the library, covers, Play and Install keep working, and network actions say that GameNode cannot be reached

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

### Requirement: DRM-free details expose protection and maintenance
A DRM-free game's detail pane and context menu SHALL expose applicable launch editing, Locate executable, Remove, matching and save-protection actions. Details SHALL show the automatic-match provenance, tracking configuration and save-protection status. Controls SHALL be keyboard accessible and all user-facing strings SHALL be available in English and Brazilian Portuguese.

#### Scenario: Known game with saves
- **WHEN** a supported DRM-free game is selected
- **THEN** details show its backup status and history alongside Play and Change match

#### Scenario: Unsupported backup
- **WHEN** the selected game has no known save profile
- **THEN** details explain the missing support and offer save-location setup while Play remains available

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
