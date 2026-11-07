# Spec Delta

## MODIFIED Requirements

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

## ADDED Requirements

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
