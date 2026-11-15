# Spec Delta

## MODIFIED Requirements

### Requirement: Scans run on start, window open and demand
The application SHALL scan all loaded providers when it starts, when the window is opened, when the user asks for a rescan, and every 30 minutes while it runs in the background. Scans SHALL run off the UI thread. Only one scan SHALL run at a time, and a request made during a scan SHALL cause one more scan after it. The UI SHALL show that a scan is running and, for each provider and the DRM-free folders, whether it is waiting, scanning or done and how many games it has found so far.

#### Scenario: Rescan on demand
- **WHEN** the user chooses Rescan from the library header, the app menu, the command palette or its keyboard shortcut
- **THEN** a scan runs while the library stays usable, and new or removed games appear when it ends

#### Scenario: Repeated requests
- **WHEN** several rescans are requested while a scan is running
- **THEN** at most one further scan runs after the current one

#### Scenario: Game installed while GameNode is in the background
- **WHEN** the user installs a Steam game while GameNode runs in the background with its window closed
- **THEN** the game is in the library and tracked after the next periodic scan, without the window being opened

#### Scenario: Scanning progress
- **WHEN** a scan is running with Steam done, Epic scanning and DRM-free folders waiting
- **THEN** the scanning panel shows each source with its state and found count, and the rows already known stay usable below it

### Requirement: Grid and list views
The signed-in main view SHALL show the library as a cover grid or a list, which the user can switch between with the segmented control or Ctrl+L. Both views SHALL virtualize their visible rows using `@tanstack/react-virtual` so that only the entries inside the viewport (plus a bounded overscan) are mounted in the DOM. Grid tiles SHALL have a fixed size and show the cover, title, playtime or install state, and indicators for installed, running, family-shared, favorite and hidden. The list SHALL show a selection checkbox, cover, title with its subtitle, store, GameNode status, match, playtime, last session and favorite, and its column headers SHALL sort by their column. Each entry SHALL show its GameNode cover when one is cached and a titled placeholder otherwise. Cached covers and detail-pane landscape artwork SHALL be served through the custom `gamenode-asset://` protocol and rendered with asynchronous off-thread decoding (`loading="lazy" decoding="async"`) so image loading never blocks the UI thread or retains unbounded uncompressed bitmaps in Rust memory.

#### Scenario: Switch views
- **WHEN** the user switches from grid to list
- **THEN** the same entries, filter, sort, grouping and selection are shown as a virtualized list

#### Scenario: No cover available
- **WHEN** an entry is unmatched or its cover is not cached
- **THEN** it shows a placeholder with the detected title

#### Scenario: Installed and shared indicators
- **WHEN** the list shows an installed entry and a family-shared entry
- **THEN** each row identifies its installed or family-shared state with text or an icon

#### Scenario: Running tile
- **WHEN** a game is running
- **THEN** its tile shows a Running badge with the session time and its row shows Running in place of its last session

#### Scenario: Sort from a column header
- **WHEN** the user clicks the Time column header in the list, then clicks it again
- **THEN** rows sort by playtime most first, then least first, and the header shows the sort direction

#### Scenario: Scroll a large library with cached covers and artwork
- **WHEN** the user scrolls rapidly through hundreds of games in grid or list view or selects a game with high-resolution landscape artwork
- **THEN** only viewport-visible rows are mounted, covers and artwork decode off the UI thread via `gamenode-asset://`, and scrolling remains responsive without unbounded memory growth

### Requirement: Filter, search and sort
The sidebar SHALL offer scopes and store shortcuts as described by "Sidebar scopes and store shortcuts". The toolbar SHALL offer title search, a filter popover with availability, store, shortcuts (favorites only, running only, show hidden), match state, GameNode status, playtime, history, last session, release decade and date added facets, and active-filter chips. Store, match state and GameNode status SHALL allow several choices, which are alternatives within their facet. The user SHALL be able to sort by title, store, match status, GameNode status, release year, playtime, last played or date added, ascending or descending, from the Sort menu, the list headers or Shift+S for the direction.

#### Scenario: Filter by provider and search
- **WHEN** the user selects Steam under Stores in the sidebar and types part of a title
- **THEN** only Steam entries whose detected or matched title contains that text are shown

#### Scenario: Combine facets
- **WHEN** the user selects Installed and In library in Availability, Last session: 30 days, and a release decade
- **THEN** only entries matching at least one selected availability fact and every other active facet are shown

#### Scenario: Availability facts are alternatives
- **WHEN** Installed and Family shared are both selected
- **THEN** entries that are installed, family shared, or both are shown

#### Scenario: Several stores
- **WHEN** Steam and Epic are both checked in the Store facet
- **THEN** entries from either store are shown and no store item in the sidebar is highlighted

#### Scenario: Match-state facets
- **WHEN** the user selects Needs review
- **THEN** only entries whose match is ambiguous are shown, and Marked as not a game remains a separate choice

#### Scenario: GameNode status facet
- **WHEN** the user checks Playing and Not on GameNode
- **THEN** entries whose game is in the GameNode library as Playing and entries not in it (including unmatched entries) are shown

#### Scenario: Play history facets
- **WHEN** the user selects History: Never played and Playtime: Any
- **THEN** only entries whose selected playtime source reports zero are shown, and entries with unknown playtime are not presented as unplayed

#### Scenario: Release decades follow the library
- **WHEN** the library has games from the 2000s and 2020s only
- **THEN** the Release facet offers Any, 2020s and 2000s, plus Before 1990 or Unknown only when they have games

#### Scenario: Sort by a new key
- **WHEN** the user chooses Playtime
- **THEN** rows are ordered by playtime with the most played first by default, and entries with unknown playtime come last

#### Scenario: Sort by GameNode status
- **WHEN** the user sorts by GameNode status
- **THEN** rows order Playing, Ongoing, Planned, Finished, Dropped, in library without status, then not on GameNode, with ties by title

#### Scenario: Choosing a key uses its natural direction
- **WHEN** the user chooses Release year, Last played or Date added
- **THEN** the order starts newest first, while choosing Title or Store starts A → Z

#### Scenario: Sort direction
- **WHEN** the user changes the sort direction
- **THEN** only the leading order flips, the direction control names the resulting order for the chosen key, and ties keep their deterministic order

#### Scenario: Nothing matches
- **WHEN** the filters and search exclude every entry
- **THEN** the view states that nothing matches, says how many filters may be hiding results, and offers Clear filters and Search GameNode

### Requirement: Filter feedback and clearing
The library SHALL show how many entries the active filters and search leave, SHALL show one removable chip per active facet, and SHALL offer a single action that clears all filters. Every filter option SHALL show how many entries it would leave given the scope, the search and every other active facet. The Filters button SHALL show the number of active facets. Clearing filters SHALL NOT clear the search text unless the user chooses the search-specific action.

#### Scenario: Active filters
- **WHEN** several filters are active
- **THEN** each has a chip, the Filters button shows their number, and the header shows the filtered count out of the total

#### Scenario: Remove one filter
- **WHEN** the user removes one chip
- **THEN** only that facet is cleared and the rows update

#### Scenario: Clear all
- **WHEN** the user chooses Clear filters
- **THEN** every filter returns to its default and the search text stays as it was

#### Scenario: Faceted counts
- **WHEN** Store: Steam is active and the user looks at the Match facet
- **THEN** each match option shows how many Steam entries it would leave, and the Store options show counts computed without the Store facet

#### Scenario: Counts never promise hidden rows
- **WHEN** a filter hides most of a store's games
- **THEN** the sidebar count for that store matches the rows the view would show for it

#### Scenario: Search a filter
- **WHEN** the user types "steam" in the popover's filter search
- **THEN** only options and sections whose label contains it stay visible

#### Scenario: No installed games
- **WHEN** Installed is the only availability fact selected and no entry is installed
- **THEN** the view explains that no games are installed for the selected store instead of showing a generic no-match message

### Requirement: Empty library guidance
When the installed scan and the catalog find no games, the library SHALL show a first-run guide with three steps (sign in to GameNode, connect a store, add a DRM-free folder), each showing whether it is done and offering its action, and SHALL offer Rescan. When the Installed filter hides every entry but the library has games, the view SHALL offer to show all games. On Windows the empty library SHALL explain that local DRM-free games can be added from a folder or an executable without selecting a GameNode match first.

#### Scenario: No supported store found
- **WHEN** a scan completes with no installed or catalog games from any provider
- **THEN** the library shows the first-run guide with sign-in marked done for a signed-in user and offers Connect and Choose folder

#### Scenario: No installed games but owned games
- **WHEN** the Installed filter is on and the library has owned or family-shared games but none installed
- **THEN** the library explains that nothing is installed and offers to show all games

#### Scenario: Empty Steam library
- **WHEN** Steam is connected and the account owns and shares no games
- **THEN** the library says that the Steam library is empty and offers Rescan

### Requirement: Keyboard navigation and multi-select
The library SHALL support keyboard navigation in both views, opening the selected game's details, Play from the keyboard, and multi-select with Ctrl/Shift and the mouse or keyboard, the list checkboxes, Ctrl+A to select every visible row and Esc to clear the selection. Focus SHALL always be visible.

#### Scenario: Browse by keyboard
- **WHEN** the user moves through the grid with the arrow keys and presses Enter
- **THEN** the focused game is selected and launched when Play is available, or its detail pane opens

#### Scenario: Select several games
- **WHEN** the user extends the selection with Shift or Ctrl
- **THEN** several entries are selected, the batch bar appears, and actions that apply to several games are offered for all of them

#### Scenario: Select all and clear
- **WHEN** the user presses Ctrl+A and then Esc
- **THEN** every visible row of expanded groups is selected, then the selection is cleared

### Requirement: Context menus
Each entry SHALL have a context menu, opened by right-click or the keyboard menu key, grouped into play, GameNode, organization, tools and danger sections, with the actions available for that entry: Play, Install (omitted when the provider has no install action, such as DRM-free entries), Open install folder (installed entries), Add to GameNode or Edit on GameNode, Match or Change match, Add to or Remove from favorites, Add to collection, Game tools, Local save protection, View achievements, Hide from library or Show in library, Game settings and Locate executable (DRM-free), and Remove from library (DRM-free). With several entries selected, the menu SHALL offer only the actions that work on several games.

#### Scenario: Context menu on one game
- **WHEN** the user opens the context menu on a matched, installed Steam game that is in the GameNode library
- **THEN** Play, Open install folder, Edit on GameNode, Change match, the favorite action, Add to collection, Game tools, View achievements and Hide from library are offered

#### Scenario: Context menu on a DRM-free game
- **WHEN** the user opens the context menu on a DRM-free game
- **THEN** Play, Game tools, Game settings, Locate executable, Remove from library, Local save protection, Match or Change match, and Add to GameNode are offered, and Install is not shown

#### Scenario: Context menu on a multi-selection
- **WHEN** several games are selected
- **THEN** single-game actions such as Play, Game tools and Change match are not offered, while Add to GameNode, Change status, favorites, Add to collection and Hide from library are

### Requirement: Detail pane
Selecting one entry SHALL show a detail pane with a hero image, title, store and release year, the primary Play or Install action, a favorite toggle, a more-actions menu, state pills (installed or not, match, DRM-free, saves protected), stat cards (playtime with its source, last session, achievements or saves), and sections for On GameNode (status and collections), Match, Achievements (when the provider has an achievement backend), On this PC (install folder and playtime counter) and Game tools. When the entry's provider has no install action (such as DRM-free games), the Install button and its reason SHALL be omitted. Other actions SHALL be disabled with a reason when unavailable, including "Running" for Play and for mutating game tools. For an entry not installed on this PC, the install folder SHALL be replaced by a not-installed statement. A family-shared entry SHALL identify Steam Family as its access.

#### Scenario: Unmatched game
- **WHEN** the selected game has no GameNode match
- **THEN** the pane shows the no-match notice with Match and Not a game, Play and Game tools stay available, and Add to GameNode is disabled until a match is chosen

#### Scenario: Running game
- **WHEN** the selected game is running
- **THEN** the pane shows Running with the session time in place of Play, Play is unavailable with a reason saying the game is already running, and opening Game tools disables install, update and uninstall actions until the game exits

#### Scenario: DRM-free game details omit Install
- **WHEN** a DRM-free game is selected
- **THEN** the detail pane shows Play, Game tools and DRM-free actions, its launch configuration and save protection, without rendering an Install button or a "no install action" caption

#### Scenario: Game with OptiScaler installed
- **WHEN** the selected game has OptiScaler installed
- **THEN** the detail pane displays the installed tool summary (such as `OptiScaler v0.7.9 (dxgi.dll)`) and opening `Game tools…` shows the installed state, version switcher, proxy DLL selector, and `Uninstall` action

#### Scenario: Not installed
- **WHEN** an owned or family-shared Steam game is selected and it is not installed
- **THEN** the pane offers Install as its primary action and says it is not installed on this PC instead of showing an install folder

#### Scenario: Family-shared details
- **WHEN** a family-shared Steam game is selected
- **THEN** the pane identifies it as shared through Steam Family and explains that playtime and achievements are the user's own

#### Scenario: Change status from the pane
- **WHEN** the selected game is in the GameNode library and the user picks Finished in its status select
- **THEN** the entry's GameNode status is updated without opening the add dialog, and its list row shows Finished

### Requirement: Remembered layout
The application SHALL remember the view mode, sidebar scope, filters, sort, grouping and collapsed groups, the size of the sidebar and detail pane, and whether the detail pane and sidebar are open, and SHALL restore them on the next start. The search text SHALL NOT be remembered. A layout saved by an older build SHALL load with its provider selection turned into the equivalent store filter and its single match state, playtime and release choices turned into their new equivalents.

#### Scenario: Restart with a changed layout
- **WHEN** the user switches to list view, sets filters, sorts by playtime, groups by store, resizes the detail pane and restarts
- **THEN** the library opens in list view with the same filters, sort and grouping, and the same pane size

#### Scenario: Layout saved before filters existed
- **WHEN** a layout saved by an older build with "Installed only" on is loaded
- **THEN** it opens with the Installed availability filter on and every other filter at its default

#### Scenario: Layout saved with a provider selection
- **WHEN** a layout saved with Steam selected in the old sidebar is loaded
- **THEN** it opens on All games with Store: Steam active and the Steam store item highlighted

#### Scenario: Search is not remembered
- **WHEN** the user searches, closes the application and opens it again
- **THEN** the search box is empty and the saved filters and sort still apply

### Requirement: Installed filter
The library SHALL offer an Installed scope in the sidebar and an "Installed on this PC" availability option in the filter popover, both showing only entries installed on this PC. They SHALL compose with the store filter and the search text, SHALL be keyboard accessible, and SHALL be persisted as part of the library layout.

#### Scenario: Turn the filter on
- **WHEN** the user selects Installed in the sidebar
- **THEN** the view shows only installed entries and Installed is highlighted

#### Scenario: Provider and filter together
- **WHEN** Store: Steam is active and Installed on this PC is checked
- **THEN** only installed Steam entries are shown

#### Scenario: Counts follow the filter
- **WHEN** the Installed availability option changes
- **THEN** the scope and store counts show the number of entries visible under the new filter

#### Scenario: Nothing installed
- **WHEN** the filter hides every entry but the library has owned games
- **THEN** the view says that nothing is installed and offers to show all games

#### Scenario: Restart
- **WHEN** the user restarts with the Installed scope selected
- **THEN** the library opens with the same scope active

### Requirement: Steam account attention in the library
When a store account needs re-authentication or cannot be reached, or GameNode is offline, the library SHALL show a banner in the matching tone explaining that the saved library is shown, with an action: Sign in again (opens Settings > Stores), Try now (retries the sync), or none when offline. The affected store SHALL show an attention icon in the sidebar and its state in the status bar. Banners SHALL NOT block Play, Install, search, sort or selection, and SHALL be dismissible for the session.

#### Scenario: Expired session
- **WHEN** Steam rejects the stored session
- **THEN** the library keeps the cached Steam entries, shows a warning banner to sign in again, and the Steam sidebar item shows an attention icon

#### Scenario: Steam unreachable
- **WHEN** Steam cannot be reached during a sync
- **THEN** the library keeps the cached Steam entries and shows a connection banner with Try now

#### Scenario: Action opens Stores
- **WHEN** the user activates Sign in again
- **THEN** Settings opens on the Stores topic with that store's account card visible

#### Scenario: Recovered
- **WHEN** the session is restored or a later sync succeeds
- **THEN** the banner and attention icon disappear without reloading the library

#### Scenario: Dismissed
- **WHEN** the user dismisses a banner
- **THEN** it stays hidden until the application restarts or the same problem appears again after recovering

## ADDED Requirements

### Requirement: Sidebar scopes and store shortcuts
The sidebar SHALL offer the scopes All games, Installed, Favorites, Recently played (played in the last 14 days) and Running, each with its count, and one item per store that sets the Store filter to that store alone. A store item SHALL be highlighted only when the Store filter holds exactly that store. The header title SHALL name the active scope, store, collection or saved view.

#### Scenario: Recently played
- **WHEN** the user selects Recently played
- **THEN** only entries last played in the last 14 days are shown in the current sort, and the header says Recently played

#### Scenario: Store shortcut
- **WHEN** the user selects Epic under Stores
- **THEN** the scope becomes All games, the Store filter is Epic, a Store: Epic chip appears and Epic is highlighted

#### Scenario: Store counts ignore the store facet
- **WHEN** Store: Steam is active
- **THEN** the Epic sidebar count still shows how many Epic entries the other facets leave

### Requirement: Collection scopes
When the account library is available, Collections and views SHALL list the user's GameNode collections with the number of local entries in each. Selecting one SHALL show the local entries whose GameNode entry is in that collection, with the other filters applied.

#### Scenario: Select a collection
- **WHEN** the user selects the collection "Backlog curto"
- **THEN** only local entries in that GameNode collection are shown and the header title is "Backlog curto"

#### Scenario: No account library yet
- **WHEN** the account library has never been read
- **THEN** no collection items are listed and saved views still are

### Requirement: Batch bar for multi-selection
While two or more entries are selected, a floating bar SHALL show the count and offer Add to GameNode, Change status, Favorite, Match and Clear. Actions that do not apply to any selected entry SHALL be disabled with a reason. Match SHALL open the match dialog for each selected unmatched or ambiguous entry in turn.

#### Scenario: Batch bar
- **WHEN** the user selects three games
- **THEN** the bar shows "3 games selected" with its actions, above the bottom of the list

#### Scenario: Match several
- **WHEN** two unmatched games are selected and the user chooses Match
- **THEN** the match dialog opens for the first, then the second after it is confirmed or skipped

### Requirement: Multi-selection detail panel
With several entries selected, the detail pane SHALL show a stacked cover preview, the count with installed and store summaries, total playtime across stores, how many are in the GameNode library, the batch actions, and the selected entries with a way to remove each from the selection.

#### Scenario: Three selected
- **WHEN** three games are selected, two of them in the GameNode library
- **THEN** the pane says "3 games selected", shows "2 of 3" on GameNode with "1 not on GameNode", and lists the three titles

#### Scenario: Remove from selection
- **WHEN** the user removes one title from the pane's selection list
- **THEN** it is deselected and the pane updates

### Requirement: Discovered games entry
The sidebar SHALL show Discovered games with the number of discovered DRM-free candidates awaiting review, when there are any. Selecting it SHALL open the discovery review. The discovered-games banner SHALL offer the same review.

#### Scenario: Pending discoveries
- **WHEN** watched folders found three candidates that need review
- **THEN** the sidebar shows Discovered games with 3 and selecting it opens the review
