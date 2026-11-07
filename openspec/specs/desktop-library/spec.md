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

### Requirement: Grid and list views
The signed-in main view SHALL show the library as a cover grid or a list, which the user can switch between. Both the grid view and the list view SHALL virtualize their visible rows using `@tanstack/react-virtual` so that only the entries inside the viewport (plus a bounded overscan) are mounted in the DOM. The list SHALL show at least the title, provider, match status, and installed or family-sharing indicators. Each entry SHALL show its GameNode cover when one is cached and a titled placeholder otherwise. Cached covers and detail-pane landscape artwork SHALL be served through the custom `gamenode-asset://` protocol and rendered with asynchronous off-thread decoding (`loading="lazy" decoding="async"`) so image loading never blocks the UI thread or retains unbounded uncompressed bitmaps in Rust memory.

#### Scenario: Switch views
- **WHEN** the user switches from grid to list
- **THEN** the same entries, filter, sort and selection are shown as a virtualized list

#### Scenario: No cover available
- **WHEN** an entry is unmatched or its cover is not cached
- **THEN** it shows a placeholder with the detected title

#### Scenario: Installed and shared indicators
- **WHEN** the list shows an installed entry and a family-shared entry
- **THEN** each row identifies its installed or family-shared state with text or an icon

#### Scenario: Scroll a large library with cached covers and artwork
- **WHEN** the user scrolls rapidly through hundreds of games in grid or list view or selects a game with high-resolution landscape artwork
- **THEN** only viewport-visible rows are mounted, covers and artwork decode off the UI thread via `gamenode-asset://`, and scrolling remains responsive without unbounded memory growth

### Requirement: Filter, search and sort
A sidebar SHALL list "All games" and each loaded provider, and selecting one SHALL filter the view. The toolbar SHALL offer title search, a filter panel with availability, running, match state, play history, release decade, date added and favorites facets, and active-filter chips. The user SHALL be able to sort by title, provider, match status, release year, playtime, last played or date added, ascending or descending. Sidebar counts SHALL reflect every active filter except the provider selection.

#### Scenario: Filter by provider and search
- **WHEN** the user selects Steam in the sidebar and types part of a title
- **THEN** only Steam entries whose detected or matched title contains that text are shown

#### Scenario: Combine facets
- **WHEN** the user selects Installed and Owned in Availability, Last played: 30 days, and a release decade
- **THEN** only entries matching at least one selected availability fact and every other active facet are shown

#### Scenario: Availability facts are alternatives
- **WHEN** Installed and Family shared are both selected
- **THEN** entries that are installed, family shared, or both are shown

#### Scenario: Match-state facets
- **WHEN** the user selects Needs review
- **THEN** only entries whose match is ambiguous are shown, and Marked as no game remains a separate choice

#### Scenario: Play history facets
- **WHEN** the user selects Unplayed
- **THEN** only entries whose selected playtime source reports zero are shown, and entries with unknown playtime are not presented as unplayed

#### Scenario: Sort by a new key
- **WHEN** the user chooses Playtime
- **THEN** rows are ordered by playtime with the most played first by default, and entries with unknown playtime come last

#### Scenario: Choosing a key uses its natural direction
- **WHEN** the user chooses Release year, Last played or Date added
- **THEN** the order starts newest first, while choosing Title or Store starts A → Z

#### Scenario: Sort direction
- **WHEN** the user changes the sort direction
- **THEN** only the leading order flips, the direction control names the resulting order for the chosen key, and ties keep their deterministic order

#### Scenario: Nothing matches
- **WHEN** the filters and search exclude every entry
- **THEN** the view states that nothing matches and offers to clear the search and the filters

### Requirement: Filter feedback and clearing
The library SHALL show how many entries the active filters and search leave, SHALL show one removable chip per active filter, and SHALL offer a single action that clears all filters. Clearing filters SHALL NOT clear the search text unless the user chooses the search-specific action.

#### Scenario: Active filters
- **WHEN** several filters are active
- **THEN** each has a chip and the view shows the filtered count out of the total

#### Scenario: Remove one filter
- **WHEN** the user removes one chip
- **THEN** only that filter is cleared and the rows update

#### Scenario: Clear all
- **WHEN** the user chooses Clear all filters
- **THEN** every filter returns to its default and the search text stays as it was

#### Scenario: Counts never promise hidden rows
- **WHEN** a filter hides most of a provider's games
- **THEN** the sidebar count for that provider matches the rows the view would show for it

#### Scenario: No installed games
- **WHEN** Installed is the only availability fact selected and no entry is installed
- **THEN** the view explains that no games are installed for the selected provider instead of showing a generic no-match message

### Requirement: Empty library guidance
When the installed scan and the catalog find no games, the library SHALL say so, name the supported store sources it looked for, and offer Rescan. When the Installed filter hides every entry but the library has games, the view SHALL offer to show all games. On Windows the empty library SHALL additionally offer Add game and explain that local DRM-free games can be added from an executable without selecting a GameNode match first.

#### Scenario: No supported store found
- **WHEN** a scan completes with no installed or catalog games from any provider
- **THEN** the library explains that no games were found and offers Rescan, plus Add game on Windows

#### Scenario: No installed games but owned games
- **WHEN** the Installed filter is on and the library has owned or family-shared games but none installed
- **THEN** the library explains that nothing is installed and offers to show all games

#### Scenario: Empty Steam library
- **WHEN** Steam is connected and the account owns and shares no games
- **THEN** the library says that the Steam library is empty and offers Rescan

### Requirement: Keyboard navigation and multi-select
The library SHALL support keyboard navigation in both views, opening the selected game's details, Play from the keyboard, and multi-select with Ctrl/Shift and the mouse or keyboard. Focus SHALL always be visible.

#### Scenario: Browse by keyboard
- **WHEN** the user moves through the grid with the arrow keys and presses Enter
- **THEN** the focused game is selected and its detail pane shows it

#### Scenario: Select several games
- **WHEN** the user extends the selection with Shift or Ctrl
- **THEN** several entries are selected and actions that apply to several games are offered for all of them

### Requirement: Context menus
Each entry SHALL have a context menu, opened by right-click or the keyboard menu key, with the actions available for that entry: Play, Install (omitted when the provider has no install action, such as DRM-free entries), Game tools (for installed entries on supported platforms), Match or Change match, and Add to GameNode. With several entries selected, the menu SHALL offer only the actions that work on several games.

#### Scenario: Context menu on one game
- **WHEN** the user opens the context menu on a matched, installed Steam game
- **THEN** Play, Game tools, Change match and Add to GameNode are offered

#### Scenario: Context menu on a DRM-free game
- **WHEN** the user opens the context menu on a DRM-free game
- **THEN** Play, Game tools, Game settings, Locate executable, Remove from library, Local save protection, Match or Change match, and Add to GameNode are offered, and Install is not shown

#### Scenario: Context menu on a multi-selection
- **WHEN** several games are selected
- **THEN** single-game actions such as Play, Game tools and Change match are not offered

### Requirement: Detail pane
Selecting one entry SHALL show a detail pane with its cover, detected and matched titles, provider, install folder, match status, whether it is running, active installed tools summary (when any game tool such as OptiScaler is installed), and the Play, Install, Game tools, match and Add to GameNode actions. When the entry's provider has no install action (such as DRM-free games), the Install button and its reason SHALL be omitted. Other actions SHALL be disabled with a reason when unavailable, including "Running" for Play and for mutating game tools. For an entry not installed on this PC, the install folder SHALL be replaced by a not-installed statement. A family-shared entry SHALL identify Steam Family as its access.

#### Scenario: Unmatched game
- **WHEN** the selected game has no GameNode match
- **THEN** the pane offers Match, Play and Game tools stay available, and Add to GameNode is disabled until a match is chosen

#### Scenario: Running game
- **WHEN** the selected game is running
- **THEN** the pane shows Running, Play is disabled with a reason saying the game is already running, and opening Game tools disables install, update and uninstall actions until the game exits

#### Scenario: DRM-free game details omit Install
- **WHEN** a DRM-free game is selected
- **THEN** the detail pane shows Play, Game tools and DRM-free actions without rendering an Install button or a "no install action" caption

#### Scenario: Game with OptiScaler installed
- **WHEN** the selected game has OptiScaler installed
- **THEN** the detail pane displays the installed tool summary (such as `OptiScaler v0.7.9 (dxgi.dll)`) and opening `Game tools…` shows the installed state, version switcher, proxy DLL selector, and `Uninstall` action

#### Scenario: Not installed
- **WHEN** an owned or family-shared Steam game is selected and it is not installed
- **THEN** the pane says it is not installed on this PC instead of showing an install folder

#### Scenario: Family-shared details
- **WHEN** a family-shared Steam game is selected
- **THEN** the pane identifies it as shared through Steam Family

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
The application SHALL remember the view mode, sidebar selection, filters, sort, the size of the sidebar and detail pane, and whether the detail pane is open, and SHALL restore them on the next start. The search text SHALL NOT be remembered.

#### Scenario: Restart with a changed layout
- **WHEN** the user switches to list view, sets filters, sorts by playtime, resizes the detail pane and restarts
- **THEN** the library opens in list view with the same filters and sort, and the same pane size

#### Scenario: Layout saved before filters existed
- **WHEN** a layout saved by an older build with "Installed only" on is loaded
- **THEN** it opens with the Installed availability filter on and every other filter at its default

#### Scenario: Search is not remembered
- **WHEN** the user searches, closes the application and opens it again
- **THEN** the search box is empty and the saved filters and sort still apply

### Requirement: Library opens from local data
The library SHALL be shown from local data before any network request finishes. It SHALL remain usable while GameNode or Steam is unreachable. Matching, game search and Add to GameNode SHALL be disabled with an explanation until a verified session and the network are available. Cached installed, owned and family facts SHALL remain visible when a Steam request fails.

#### Scenario: Offline after a verified session
- **WHEN** GameNode becomes unreachable while the user is signed in
- **THEN** the library, covers, Play and Install keep working, and network actions say that GameNode cannot be reached

#### Scenario: Steam offline
- **WHEN** Steam cannot be reached while the cached Steam catalog exists
- **THEN** the cached Steam entries, covers, Play and Install keep working and the library shows a connection banner

#### Scenario: No cached catalog
- **WHEN** the application starts offline and Steam was never synced
- **THEN** only installed games are shown and Steam's account state explains that it could not be checked

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

### Requirement: All known Steam games are visible
The library SHALL show one entry per Steam app that is installed on this PC, owned by the connected Steam account, or shared with it through Steam Family. It SHALL include uninstalled and unplayed games, SHALL NOT show the same app twice, and SHALL keep saved matches and covers when the account reconnects.

#### Scenario: Owned and installed library
- **WHEN** the account owns 320 games and 18 of them are installed
- **THEN** the library shows 320 Steam entries and 18 of them are marked installed

#### Scenario: Family-shared games
- **WHEN** the account has access to games through Steam Family that it does not own
- **THEN** those games appear as Steam entries marked as family-shared

#### Scenario: Same app in two facts
- **WHEN** an app is both owned and installed, or both owned and family-shared
- **THEN** it appears once with the strongest ownership fact and the installed fact

#### Scenario: Reconnect
- **WHEN** the user signs in to Steam again after signing out
- **THEN** the previous entries return with their saved matches and covers

### Requirement: Installed filter
The library SHALL offer an "Installed only" filter that shows only entries installed on this PC. The filter SHALL compose with the provider selection and the search text, SHALL be keyboard accessible, and SHALL be persisted as part of the library layout.

#### Scenario: Turn the filter on
- **WHEN** the user turns on "Installed only"
- **THEN** the view shows only installed entries and the filter is visibly active

#### Scenario: Provider and filter together
- **WHEN** Steam is selected in the sidebar and "Installed only" is on
- **THEN** only installed Steam entries are shown

#### Scenario: Counts follow the filter
- **WHEN** "Installed only" changes
- **THEN** the All games and provider counts show the number of entries visible under the new filter

#### Scenario: Nothing installed
- **WHEN** the filter hides every entry but the library has owned games
- **THEN** the view says that nothing is installed and offers to show all games

#### Scenario: Restart
- **WHEN** the user restarts with "Installed only" on
- **THEN** the library opens with the same filter active

### Requirement: Ownership indicators
The grid, list and detail pane SHALL indicate whether an entry is installed on this PC and whether it is shared through Steam Family. Indicators SHALL use an icon or text and SHALL NOT rely on color alone. A family-shared game that is also owned SHALL NOT be labelled as shared.

#### Scenario: Installed game in the full library
- **WHEN** an installed Steam game is shown while "Installed only" is off
- **THEN** its tile or row shows an installed indicator

#### Scenario: Family-shared game
- **WHEN** a family-shared game is shown or selected
- **THEN** the grid or list and the detail pane identify it as shared through Steam Family

#### Scenario: Owned and shared
- **WHEN** a game is both owned and shared through Steam Family
- **THEN** it is presented as owned and not as shared

### Requirement: Owned-only Steam actions
A Steam entry that is owned or family-shared but not installed SHALL keep Play and Install through the provider's declared URIs. Play SHALL ask Steam to run the game, Install SHALL open Steam at the game, and the application SHALL NOT write to the game folder.

#### Scenario: Play an owned game that is not installed
- **WHEN** the user chooses Play on an owned Steam game that is not installed
- **THEN** Steam is asked to run that app and the application stays open

#### Scenario: Install an owned game
- **WHEN** the user chooses Install on an owned Steam game that is not installed
- **THEN** Steam opens at that app and the application does not install files itself

#### Scenario: Store client unavailable
- **WHEN** the OS has no Steam handler
- **THEN** Play and Install show a localized error and the library is unchanged

### Requirement: Steam account attention in the library
When Steam needs re-authentication or cannot be reached, the library SHALL show a banner explaining that the saved Steam library is shown and offering an action that opens Settings > Stores. The banner SHALL NOT block Play, Install, search, sort or selection.

#### Scenario: Expired session
- **WHEN** Steam rejects the stored session
- **THEN** the library keeps the cached Steam entries and shows a banner to sign in again

#### Scenario: Steam unreachable
- **WHEN** Steam cannot be reached during a sync
- **THEN** the library keeps the cached Steam entries and shows a connection banner

#### Scenario: Action opens Stores
- **WHEN** the user activates the banner action
- **THEN** Settings opens on the Stores topic with the Steam account card visible

#### Scenario: Recovered
- **WHEN** the session is restored or a later sync succeeds
- **THEN** the banner disappears without reloading the library

### Requirement: Catalog updates preserve the view
When a catalog sync adds or removes entries, the library SHALL keep the current provider filter, search, sort, and selection of entries that still exist. It SHALL NOT duplicate rows or jump back to the top of the view.

#### Scenario: Sync adds games
- **WHEN** a sync adds owned games while the user is browsing
- **THEN** the current view stays in place and the new entries join it under the active filter and sort

#### Scenario: Sync removes games
- **WHEN** a sync removes a game that is not selected
- **THEN** the remaining entries keep their selection and the view does not jump to the top

### Requirement: Uninstalled Steam games leave only the installed view
An entry that a completed installed scan no longer finds SHALL lose its installed fact. It SHALL leave the Installed view and SHALL remain visible if it is owned or family-shared. An entry with no remaining fact SHALL become invisible while keeping its match and cover. A failed scan SHALL keep its previous installed entries. Manual registrations SHALL remain visible as unavailable until repaired or removed, preserving matches, playtime and backup history.

#### Scenario: Uninstalled but owned
- **WHEN** an owned Steam game is uninstalled and the library rescans
- **THEN** the game leaves the Installed view and remains in the full library as owned

#### Scenario: Uninstalled and not owned
- **WHEN** an installed Steam game with no ownership fact is uninstalled and the library rescans
- **THEN** it becomes invisible and keeps its match and cover

#### Scenario: Game reinstalled
- **WHEN** a previously matched game is installed again and the library rescans
- **THEN** it reappears as installed with its previous match and cover

#### Scenario: Scan fails for one provider
- **WHEN** a provider's roots cannot be read during a scan
- **THEN** that provider's installed entries from the last successful scan remain, and the library shows that its scan failed

#### Scenario: Manual executable missing
- **WHEN** a manually registered executable is unavailable during a scan
- **THEN** its entry stays visible, Play explains why it is unavailable, and Locate executable is offered
