# Spec Delta

## MODIFIED Requirements

### Requirement: Filter, search and sort
A sidebar SHALL list "All games" and each loaded provider, and selecting one SHALL filter the view. The toolbar SHALL offer title search, a filter panel with availability, running, match state, play history, release decade, date added and favorites facets, and active-filter chips. The availability facet SHALL offer Installed, Owned and Shared. Shared SHALL be offered only when at least one loaded provider declares shared access. The user SHALL be able to sort by title, provider, match status, release year, playtime, last played or date added, ascending or descending. Sidebar counts SHALL reflect every active filter except the provider selection.

#### Scenario: Filter by provider and search
- **WHEN** the user selects Steam in the sidebar and types part of a title
- **THEN** only Steam entries whose detected or matched title contains that text are shown

#### Scenario: Combine facets
- **WHEN** the user selects Installed and Owned in Availability, Last played: 30 days, and a release decade
- **THEN** only entries matching at least one selected availability fact and every other active facet are shown

#### Scenario: Availability facts are alternatives
- **WHEN** Installed and Shared are both selected
- **THEN** entries that are installed, shared, or both are shown

#### Scenario: No provider declares shared access
- **WHEN** no loaded provider declares shared access, such as on a platform where Steam is not loaded
- **THEN** the availability facet offers only Installed and Owned

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

### Requirement: Detail pane
Selecting one entry SHALL show a detail pane with its cover, detected and matched titles, provider, install folder, match status, whether it is running, active installed tools summary (when any game tool such as OptiScaler is installed), and the Play, Install, Game tools, match and Add to GameNode actions. When the entry's provider has no install action (such as DRM-free games), the Install button and its reason SHALL be omitted. Other actions SHALL be disabled with a reason when unavailable, including "Running" for Play and for mutating game tools. For an entry not installed on this PC, the install folder SHALL be replaced by a not-installed statement. A shared entry SHALL identify its provider's declared shared-access label as its access.

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
- **WHEN** an owned or shared Steam game is selected and it is not installed
- **THEN** the pane says it is not installed on this PC instead of showing an install folder

#### Scenario: Family-shared details
- **WHEN** a shared Steam game is selected
- **THEN** the pane identifies its access as `Steam Family`, the label Steam declares

#### Scenario: Owned Epic game
- **WHEN** an owned Epic game is selected
- **THEN** the pane shows no shared-access line and no text naming another store

### Requirement: Ownership indicators
The grid, list and detail pane SHALL indicate whether an entry is installed on this PC and whether it is shared through its provider's declared shared access. A shared indicator SHALL name the provider's declared label. Indicators SHALL use an icon or text and SHALL NOT rely on color alone. A shared game that is also owned SHALL NOT be labelled as shared.

#### Scenario: Installed game in the full library
- **WHEN** an installed Steam game is shown while "Installed only" is off
- **THEN** its tile or row shows an installed indicator

#### Scenario: Family-shared game
- **WHEN** a shared Steam game is shown or selected
- **THEN** the grid or list and the detail pane identify it as shared through `Steam Family`

#### Scenario: Owned and shared
- **WHEN** a game is both owned and shared through Steam Family
- **THEN** it is presented as owned and not as shared

#### Scenario: Translated indicator text
- **WHEN** the application language is Brazilian Portuguese and a shared Steam game is shown
- **THEN** the surrounding indicator text is translated and the label `Steam Family` is shown unchanged

### Requirement: Remembered layout
The application SHALL remember the view mode, sidebar selection, filters, sort, the size of the sidebar and detail pane, and whether the detail pane is open, and SHALL restore them on the next start. The search text SHALL NOT be remembered.

#### Scenario: Restart with a changed layout
- **WHEN** the user switches to list view, sets filters, sorts by playtime, resizes the detail pane and restarts
- **THEN** the library opens in list view with the same filters and sort, and the same pane size

#### Scenario: Layout saved before filters existed
- **WHEN** a layout saved by an older build with "Installed only" on is loaded
- **THEN** it opens with the Installed availability filter on and every other filter at its default

#### Scenario: Layout saved with the family filter
- **WHEN** a layout saved by an older build with the Family shared availability filter on is loaded
- **THEN** it opens with the Shared availability filter on

#### Scenario: Search is not remembered
- **WHEN** the user searches, closes the application and opens it again
- **THEN** the search box is empty and the saved filters and sort still apply
