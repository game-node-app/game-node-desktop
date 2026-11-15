# Spec Delta

## Purpose

Let a signed-in user search the GameNode catalog from inside the desktop app and add games that are not on this PC to their GameNode library, without leaving for the website.

## ADDED Requirements

### Requirement: Open the catalog view
Signed-in users SHALL be able to open "Explore GameNode" from the sidebar, from the no-results state with the library search prefilled, and from the command palette with the typed text prefilled. The view SHALL replace the library content while the top bar, sidebar and status bar stay. Selecting a library sidebar item SHALL return to the library.

#### Scenario: Open from no results
- **WHEN** the library search "hades" matches nothing and the user chooses Search GameNode
- **THEN** the catalog view opens with "hades" in its search and results load

#### Scenario: Signed out
- **WHEN** no session is available
- **THEN** the sidebar does not offer Explore GameNode

### Requirement: Search the catalog
The catalog view SHALL search GameNode's game search after the user stops typing, once the query has at least 2 characters. Results SHALL show cover, title and release year. A newer query SHALL replace an older one's results, even if the older answer arrives later. Incomplete answers SHALL say that results may be incomplete.

#### Scenario: Search results
- **WHEN** the user types "celeste"
- **THEN** matching GameNode games appear as tiles with their covers, titles and years

#### Scenario: Short query
- **WHEN** the query has fewer than 2 characters
- **THEN** no request is sent and the view asks for at least 2 characters

#### Scenario: Out-of-order answers
- **WHEN** the answer for "cel" arrives after the answer for "celeste"
- **THEN** the results for "celeste" stay on screen

### Requirement: Catalog result state
Each result SHALL show whether the game is already in the user's GameNode library and whether a local library entry on this PC is matched to it. The detail pane SHALL offer Add to GameNode for games not in the library, View on GameNode for games already in it, and Show in library for games on this PC.

#### Scenario: Add from the catalog
- **WHEN** the user selects a result that is not in their library, chooses Add to GameNode, picks Planned and confirms
- **THEN** the game is added through the same dialog and rules as library adds, and the result then shows In your library

#### Scenario: Game on this PC
- **WHEN** a result is matched to a local entry and the user chooses Show in library
- **THEN** the library opens with that entry selected and scrolled into view

### Requirement: Catalog errors and offline
When the search fails, the view SHALL show a localized error with Try again. When GameNode cannot be reached or the session is unverified, the view SHALL say that search needs a connection and SHALL NOT send requests until retried.

#### Scenario: Search fails
- **WHEN** the search request fails
- **THEN** the view shows "Couldn't search GameNode" with Try again, and the previous query text is kept

#### Scenario: Offline
- **WHEN** GameNode is unreachable
- **THEN** the view explains that search needs a connection
