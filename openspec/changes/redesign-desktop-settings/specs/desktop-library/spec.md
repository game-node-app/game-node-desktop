# Spec Delta

## ADDED Requirements

### Requirement: Cover size preference
The grid view SHALL offer three cover sizes, Small, Medium (the default) and Large, chosen with the "Cover size" preference in Settings. The choice SHALL change how many covers fit per row without changing the cover aspect ratio, the entries, the filter, the sort or the selection, and SHALL be remembered with the layout. The list view SHALL NOT change.

#### Scenario: Choose large covers
- **WHEN** the user chooses Large in Settings while the grid is open behind the modal
- **THEN** the grid shows fewer, larger covers per row with the same entries and selection, and still shows Large after a restart

#### Scenario: Layout from an older build
- **WHEN** a layout saved before cover sizes existed is loaded
- **THEN** the grid uses Medium

### Requirement: Default view in Settings
The "Default view" control in Settings SHALL show and change the same remembered view mode as the library's grid/list switch. Changing either SHALL update the other.

#### Scenario: Switch from Settings
- **WHEN** the library is in grid view and the user chooses List in Settings
- **THEN** the library switches to list view and the toolbar switch shows List

### Requirement: Library opens in a chosen scope
The "Open in" preference SHALL choose the sidebar scope the library opens with at start: Last view (the default, the remembered scope), All games, Installed, Favorites or Recent. A fixed choice SHALL apply only at start and SHALL NOT stop the user from changing the scope afterwards; the remembered filters, sort and view still apply.

#### Scenario: Always open in Installed
- **WHEN** "Open in" is Installed and the user closes the application while viewing Favorites
- **THEN** the next start opens the library in Installed with the remembered view, filters and sort

#### Scenario: Last view
- **WHEN** "Open in" is Last view and the user closes the application while viewing a collection
- **THEN** the next start opens that collection

#### Scenario: Remembered scope no longer exists
- **WHEN** "Open in" is Last view and the remembered collection was deleted
- **THEN** the library opens in All games

### Requirement: Shared games can be left out per store
For each store that declares shared access, the "Include shared games" preference SHALL control whether entries that are only shared with the user (not owned and not installed) appear in the library. It SHALL be on by default. While off, those entries SHALL be left out of every scope, filter, group, count and search, while the shared catalog keeps syncing, owned and installed entries stay, and turning it back on restores them without a new sync.

#### Scenario: Leave out family-shared games
- **WHEN** the user turns "Include family-shared games" off for Steam with 11 shared-only entries
- **THEN** those 11 entries disappear from the library and its counts, the Steam card in Settings keeps showing the shared count, and owned Steam games remain

#### Scenario: Shared game is installed
- **WHEN** the preference is off and a shared-only Steam game is installed
- **THEN** that game appears in the library as installed with its shared indicator

#### Scenario: Turn it back on
- **WHEN** the user turns the preference on again
- **THEN** the shared-only entries reappear immediately with their shared indicators
