# Spec Delta

## ADDED Requirements

### Requirement: All known Steam games are visible
The library SHALL show one entry per Steam app that is installed on this PC, owned by the connected
Steam account, or shared with it through Steam Family. It SHALL include uninstalled and unplayed
games, SHALL NOT show the same app twice, and SHALL keep saved matches and covers when the account
reconnects.

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
The library SHALL offer an "Installed only" filter that shows only entries installed on this PC.
The filter SHALL compose with the provider selection and the search text, SHALL be keyboard
accessible, and SHALL be persisted as part of the library layout.

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
The grid, list and detail pane SHALL indicate whether an entry is installed on this PC and whether
it is shared through Steam Family. Indicators SHALL use an icon or text and SHALL NOT rely on color
alone. A family-shared game that is also owned SHALL NOT be labelled as shared.

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
A Steam entry that is owned or family-shared but not installed SHALL keep Play and Install through
the provider's declared URIs. Play SHALL ask Steam to run the game, Install SHALL open Steam at the
game, and the application SHALL NOT write to the game folder.

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
When Steam needs re-authentication or cannot be reached, the library SHALL show a banner explaining
that the saved Steam library is shown and offering an action that opens Settings > Stores. The
banner SHALL NOT block Play, Install, search, sort or selection.

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
When a catalog sync adds or removes entries, the library SHALL keep the current provider filter,
search, sort, and selection of entries that still exist. It SHALL NOT duplicate rows or jump back
to the top of the view.

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

## MODIFIED Requirements

### Requirement: Grid and list views
The signed-in main view SHALL show the library as a cover grid or a list, which the user can switch between. The list SHALL show at least the title, provider, match status, and installed or family-sharing indicators. Each entry SHALL show its GameNode cover when one is cached and a titled placeholder otherwise.

#### Scenario: Switch views
- **WHEN** the user switches from grid to list
- **THEN** the same entries, filter, sort and selection are shown as a list

#### Scenario: No cover available
- **WHEN** an entry is unmatched or its cover is not cached
- **THEN** it shows a placeholder with the detected title

#### Scenario: Installed and shared indicators
- **WHEN** the list shows an installed entry and a family-shared entry
- **THEN** each row identifies its installed or family-shared state with text or an icon

### Requirement: Filter, search and sort
A sidebar SHALL list "All games" and each loaded provider with its entry count, and selecting one SHALL filter the view. A search field SHALL filter by title. The user SHALL be able to sort by title, provider or match status, ascending or descending.

#### Scenario: Filter by provider and search
- **WHEN** the user selects Steam in the sidebar and types part of a title
- **THEN** only Steam entries whose detected or matched title contains that text are shown

#### Scenario: Nothing matches
- **WHEN** the filter and search exclude every entry
- **THEN** the view states that nothing matches and offers to clear the search

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

### Requirement: Detail pane
Selecting one entry SHALL show a detail pane with its cover, detected and matched titles, provider, install folder, match status, whether it is running, and the Play, Install, match and Add to GameNode actions. When the entry's provider has no install action (such as DRM-free games), the Install button and its reason SHALL be omitted. Other actions SHALL be disabled with a reason when unavailable, including "Running" for Play. For an entry not installed on this PC, the install folder SHALL be replaced by a not-installed statement. A family-shared entry SHALL identify Steam Family as its access.

#### Scenario: Unmatched game
- **WHEN** the selected game has no GameNode match
- **THEN** the pane offers Match, Play stays available, and Add to GameNode is disabled until a match is chosen

#### Scenario: Running game
- **WHEN** the selected game is running
- **THEN** the pane shows Running, and Play is disabled with a reason saying the game is already running

#### Scenario: DRM-free game details omit Install
- **WHEN** a DRM-free game is selected
- **THEN** the detail pane shows Play and DRM-free actions without rendering an Install button or a "no install action" caption

#### Scenario: Not installed
- **WHEN** an owned or family-shared Steam game is selected and it is not installed
- **THEN** the pane says it is not installed on this PC instead of showing an install folder

#### Scenario: Family-shared details
- **WHEN** a family-shared Steam game is selected
- **THEN** the pane identifies it as shared through Steam Family

### Requirement: Remembered layout
The application SHALL remember the view mode, sidebar selection, Installed filter, sort, the size of the sidebar and detail pane, and whether the detail pane is open, and SHALL restore them on the next start. The search text SHALL NOT be remembered.

#### Scenario: Restart with a changed layout
- **WHEN** the user switches to list view, turns on the Installed filter, sorts by provider, resizes the detail pane and restarts
- **THEN** the library opens in list view, filtered to installed entries, sorted by provider, with the same pane size

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

## REMOVED Requirements

### Requirement: Uninstalled games leave the library
**Reason**: Owned and family-shared games are part of the library even when they are not installed on this PC.
**Migration**: Use "Uninstalled Steam games leave only the installed view" above. It keeps the old behavior for entries with no ownership fact and preserves manual registration behavior.

