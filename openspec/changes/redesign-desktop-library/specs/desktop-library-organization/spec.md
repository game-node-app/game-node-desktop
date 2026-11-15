# Spec Delta

## Purpose

Let the user organize the local library beyond filters: group rows into collapsible sections, save filter/sort/group/view combinations as named views in the sidebar, and hide games from the library without removing them.

## ADDED Requirements

### Requirement: Group the library
The toolbar SHALL offer Group by with None, Store, GameNode status, Match, Installation and Release decade. Grouping SHALL apply to the grid and the list, keep the active sort inside each group, and order groups by a fixed order for each grouping. Each group SHALL have a header that names it and shows its row count.

#### Scenario: Group by GameNode status
- **WHEN** the user chooses Group by › GameNode status
- **THEN** rows appear under Playing, Finished, Planned, Ongoing, Dropped, In library without status and Not on GameNode headers, each with its count, and rows inside a group keep the chosen sort

#### Scenario: Empty groups are omitted
- **WHEN** no visible row belongs to a group
- **THEN** that group's header is not shown

#### Scenario: Group by release decade
- **WHEN** the user groups by Release decade
- **THEN** groups are the decades that have rows, newest first, then Before 1990, then Unknown release

### Requirement: Collapse groups
Each group header SHALL collapse or expand its group by mouse or keyboard, and the Group menu SHALL offer Collapse all and Expand all. Collapsed groups SHALL keep their header and count and SHALL hide their rows from the view, keyboard navigation and Select all. The collapsed set SHALL be remembered with the layout per grouping.

#### Scenario: Collapse one group
- **WHEN** the user activates the Steam group header while grouped by store
- **THEN** Steam rows disappear, the header stays with its count, and arrow keys skip from the header to the next group

#### Scenario: Restart with collapsed groups
- **WHEN** the user collapses a group and restarts the application
- **THEN** the library opens with the same grouping and that group collapsed

#### Scenario: Selected row in a collapsed group
- **WHEN** the user collapses a group that contains selected rows
- **THEN** those rows leave the selection

### Requirement: Save the current view
With any filter, sort, grouping or view mode different from the default, or a scope other than All games, the user SHALL be able to save the combination as a named view from the filter chips row or the Collections and views header. The dialog SHALL show the name field and what the view includes. Names SHALL be required, trimmed, and unique per account.

#### Scenario: Save a view
- **WHEN** the user sets Store: Steam and Playtime: 10 h+, sorts by playtime, chooses Save as view, names it "Steam 10 h+" and confirms
- **THEN** "Steam 10 h+" appears under Collections and views and a notification confirms the save

#### Scenario: Duplicate name
- **WHEN** the user enters the name of an existing view
- **THEN** Save is disabled and the dialog says that a view with this name already exists

### Requirement: Apply, rename and delete saved views
Selecting a saved view SHALL apply its scope, filters, sort, grouping and view mode and mark it active until any of those change. Each view SHALL show the count of rows it would show now. Its context menu SHALL offer Rename and Delete; Delete SHALL ask for confirmation. Saved views SHALL belong to the signed-in account.

#### Scenario: Apply a view
- **WHEN** the user selects a saved view
- **THEN** the library shows that view's rows, the header title is the view name, and the view is highlighted in the sidebar

#### Scenario: Changing a facet leaves the view
- **WHEN** a view is active and the user changes the sort
- **THEN** the view is no longer highlighted and the saved view itself is unchanged

#### Scenario: Delete a view
- **WHEN** the user confirms Delete on a saved view
- **THEN** it disappears from the sidebar and the current filters stay as they are

#### Scenario: Another account
- **WHEN** a different GameNode account signs in
- **THEN** it does not see the first account's saved views

### Requirement: Hide games from the library
The context menu SHALL offer Hide from library for one or several entries. Hidden entries SHALL be left out of every scope, count, group, Select all and the command palette until Show hidden is on. Hiding SHALL NOT remove matches, playtime, favorites or GameNode data, and SHALL be stored per account.

#### Scenario: Hide a game
- **WHEN** the user chooses Hide from library on a tool that is not a game
- **THEN** it leaves the view and the counts, and a notification offers Show again

#### Scenario: Undo from the notification
- **WHEN** the user activates Show again on that notification
- **THEN** the entry is visible again

#### Scenario: Hidden game is still tracked
- **WHEN** a hidden game is launched outside GameNode
- **THEN** its playtime is still recorded and the status bar still shows it running

### Requirement: Reveal hidden games
The Shortcuts filter group SHALL offer Show hidden. While it is on, hidden entries SHALL be shown dimmed with a hidden indicator that does not rely on opacity alone, counts SHALL include them, and their context menu SHALL offer Show in library.

#### Scenario: Show hidden
- **WHEN** the user turns on Show hidden
- **THEN** hidden entries appear dimmed with an eye-off indicator and a chip shows that Show hidden is active

#### Scenario: Unhide
- **WHEN** the user chooses Show in library on a hidden entry
- **THEN** it stays visible after Show hidden is turned off
