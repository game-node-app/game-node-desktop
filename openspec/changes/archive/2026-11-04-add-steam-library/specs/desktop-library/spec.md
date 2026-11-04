# Spec Delta

## Purpose

Keep one local library of the games installed on this PC, from every loaded provider, and let the user browse, select and act on them in a native desktop view that opens from local data.

## ADDED Requirements

### Requirement: Library entries are keyed by provider
Each detected game SHALL be one library entry with the key `<provider>:<id>`. The same game found by two providers SHALL be two entries. A later scan SHALL update an entry with the same key and SHALL NOT duplicate it.

#### Scenario: Rescan an unchanged PC
- **WHEN** a scan finds the same installed games as the previous one
- **THEN** the library shows the same entries, with their matches and covers kept

#### Scenario: Game moved to another library folder
- **WHEN** a Steam game's install folder changes between scans
- **THEN** its entry keeps its key, match and cover, and shows the new folder

### Requirement: Scans run on start, window open and demand
The application SHALL scan all loaded providers when it starts, when the window is opened, and when the user asks for a rescan. Scans SHALL run off the UI thread. Only one scan SHALL run at a time, and a request made during a scan SHALL cause one more scan after it. The UI SHALL show that a scan is running.

#### Scenario: Rescan on demand
- **WHEN** the user chooses Rescan from the library toolbar, the app menu or its keyboard shortcut
- **THEN** a scan runs while the library stays usable, and new or removed games appear when it ends

#### Scenario: Repeated requests
- **WHEN** several rescans are requested while a scan is running
- **THEN** at most one further scan runs after the current one

### Requirement: Uninstalled games leave the library
An entry that a completed scan of its provider no longer finds SHALL leave the library view. Its match and cover SHALL be kept, so that a reinstalled game returns with them. A provider whose scan failed SHALL keep its previous entries.

#### Scenario: Game uninstalled
- **WHEN** a game is uninstalled through Steam and the library rescans
- **THEN** the game is no longer listed

#### Scenario: Game reinstalled
- **WHEN** a previously matched game is installed again and the library rescans
- **THEN** it reappears with its previous match and cover

#### Scenario: Scan fails for one provider
- **WHEN** a provider's roots cannot be read during a scan
- **THEN** that provider's entries from the last successful scan remain, and the library shows that its scan failed

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
When the scan finds no installed games, the library SHALL say so, name the stores it looked for, and offer Rescan.

#### Scenario: No supported store found
- **WHEN** a scan completes with no games from any provider
- **THEN** the library explains that no installed Steam games were found and offers Rescan

### Requirement: Keyboard navigation and multi-select
The library SHALL support keyboard navigation in both views, opening the selected game's details, Play from the keyboard, and multi-select with Ctrl/Shift and the mouse or keyboard. Focus SHALL always be visible.

#### Scenario: Browse by keyboard
- **WHEN** the user moves through the grid with the arrow keys and presses Enter
- **THEN** the focused game is selected and its detail pane shows it

#### Scenario: Select several games
- **WHEN** the user extends the selection with Shift or Ctrl
- **THEN** several entries are selected and actions that apply to several games are offered for all of them

### Requirement: Context menus
Each entry SHALL have a context menu, opened by right-click or the keyboard menu key, with the actions available for that entry: Play, Install, Match or Change match, and Add to GameNode. With several entries selected, the menu SHALL offer only the actions that work on several games.

#### Scenario: Context menu on one game
- **WHEN** the user opens the context menu on a matched, installed Steam game
- **THEN** Play, Change match and Add to GameNode are offered

#### Scenario: Context menu on a multi-selection
- **WHEN** several games are selected
- **THEN** single-game actions such as Play and Change match are not offered

### Requirement: Detail pane
Selecting one entry SHALL show a detail pane with its cover, detected and matched titles, provider, install folder, match status, and the Play, Install, match and Add to GameNode actions. Actions SHALL be disabled with a reason when they are unavailable.

#### Scenario: Unmatched game
- **WHEN** the selected game has no GameNode match
- **THEN** the pane offers Match, Play stays available, and Add to GameNode is disabled until a match is chosen

### Requirement: Play and Install actions
Play and Install SHALL use the provider's launch and install rules. Play SHALL show progress until the URI is handed to the OS, and a failure SHALL show a localized error. Install SHALL open the store client on the game.

#### Scenario: Play from the detail pane
- **WHEN** the user presses Play on a Steam game
- **THEN** Steam is asked to run that game and GameNode stays open

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
