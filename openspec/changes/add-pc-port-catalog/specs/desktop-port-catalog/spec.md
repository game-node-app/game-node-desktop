# Spec Delta

## Purpose

Let people browse the community native PC ports and decompilation projects that run on Windows. Show which ones GameNode can install and what each needs from the user, and keep that information available offline, credited to its source and corrected by GameNode where the source is incomplete.

## ADDED Requirements

### Requirement: Catalog comes from the public upstream API
The host SHALL read the port catalog from the public Quiver Catalog API using only unauthenticated read-only GET requests over HTTPS. It SHALL page through the full listing, read an entry's releases and withdrawals when they are needed, and send no account, device or GameNode identifiers. It SHALL NOT call any upstream endpoint that writes, reports or reviews.

#### Scenario: First catalog fetch
- **WHEN** a signed-in user opens PC ports and no cached catalog exists
- **THEN** the host pages through the upstream listing in the background and shows entries once the first complete snapshot is stored
- **AND** the requests carry no authorization header, cookie or GameNode identity

#### Scenario: No write calls
- **WHEN** the user installs, cancels or fails an install
- **THEN** the application does not send a check, download-problem, review or other write request to the upstream

### Requirement: Catalog is cached and refreshed in the background
The host SHALL store the last complete catalog snapshot locally and show it immediately on later visits, including offline. It SHALL refresh automatically at most once per day while the application runs, and on demand through a Refresh action. A refresh SHALL replace the cached snapshot only after it completes and validates. Refreshing SHALL NOT block the UI.

#### Scenario: Offline visit
- **WHEN** the user opens PC ports without network access after a successful earlier fetch
- **THEN** the cached catalog is shown with the time it was last updated

#### Scenario: Refresh fails halfway
- **WHEN** a refresh loses its connection after some pages
- **THEN** the previous snapshot remains in use unchanged and the view reports that the refresh failed

#### Scenario: Daily refresh
- **WHEN** the cached snapshot is older than one day and the application is running
- **THEN** the host refreshes it in the background without user action

### Requirement: Degrade safely when the upstream changes or is unavailable
The host SHALL validate upstream responses against the fields it relies on. It SHALL skip individual entries it cannot interpret and SHALL treat an empty, unparseable or rejected listing as a failed refresh. When no usable snapshot exists, PC ports SHALL explain that the catalog is unavailable and offer Retry. Installed ports and their library entries SHALL keep working regardless.

#### Scenario: Unknown fields or entry shape
- **WHEN** the upstream adds fields or returns one entry missing its releases
- **THEN** unknown fields are ignored, that one entry is omitted, and the rest of the catalog updates

#### Scenario: Upstream blocks access
- **WHEN** the upstream answers every request with an error or an unexpected document
- **THEN** the cached snapshot stays in use, or the view shows the unavailable state with Retry if none exists
- **AND** installed ports remain launchable from the library

### Requirement: GameNode overlay corrects and extends entries
The application SHALL merge a GameNode overlay into the catalog, matched by the upstream's stable catalog id. The overlay SHALL be able to add or replace setup recipes, executable roles, extraction steps, GameNode game ids and edition notes, and hide an entry. A bundled overlay SHALL always be available, and a newer one from the GameNode repository SHALL replace it when it validates.

#### Scenario: Recipe added by the overlay
- **WHEN** the upstream entry for Lantern Engine has no setup files and the overlay defines a required game folder
- **THEN** the merged entry shows guided setup and asks for that folder during install

#### Scenario: Hidden entry
- **WHEN** the overlay marks a catalog id as hidden
- **THEN** that entry is not listed and cannot be installed, while an existing installation stays in the library

#### Scenario: Remote overlay is invalid
- **WHEN** the fetched overlay fails to parse or validate
- **THEN** the application keeps using the newest valid overlay it already has

### Requirement: Windows-compatible entries are listed
PC ports SHALL list every merged entry that supports Windows and is not hidden, including entries without a setup recipe. Each entry SHALL show its name, original game, source console, project, and whether it offers guided setup, has a verified release, is pre-release only, or is already installed.

#### Scenario: Entry without a recipe
- **WHEN** an entry supports Windows and has no setup files in the upstream or the overlay
- **THEN** it is listed without the guided-setup label and can still be installed

#### Scenario: Linux-only entry
- **WHEN** an entry supports only Linux
- **THEN** it is not listed

### Requirement: Browse with grid, list, search and filters
PC ports SHALL offer grid and list views that are virtualized, keyboard-navigable and use the library's tile and selection treatment. It SHALL offer text search over names and game titles and filters for source console, guided setup, verified release and installed. Missing art SHALL fall back to a titled placeholder. The view and filters SHALL be remembered.

#### Scenario: Find a port
- **WHEN** the user types `bloodborne` in the search field
- **THEN** only entries whose name or game title matches are shown, without blocking input

#### Scenario: Filter to guided setup
- **WHEN** the user enables the guided setup filter and selects the PlayStation 4 console filter
- **THEN** only PlayStation 4 entries with a setup recipe are shown, with a visible way to clear the filters

#### Scenario: Restart keeps the view
- **WHEN** the user switches PC ports to list view, applies a filter and restarts the application
- **THEN** PC ports opens in list view with the same filter

### Requirement: Detail pane explains what an install needs
Selecting an entry SHALL show a detail pane with artwork, description, original game, developer, project source link, the licence when the overlay records one, the release that would be installed with its date and verification state, and each file or folder the user must supply with any accepted versions its recipe lists. The pane SHALL offer Install, or Open in library when the entry is installed.

#### Scenario: Inspect a guided entry
- **WHEN** the user selects an entry whose recipe requires a PS4 game folder
- **THEN** the pane lists that requirement, any accepted versions its recipe lists, and whether a console package can be used instead

#### Scenario: Installed entry
- **WHEN** the selected entry was installed by GameNode
- **THEN** the pane shows the installed version and folder and offers Open in library instead of Install

#### Scenario: Open the source
- **WHEN** the user activates the project source link
- **THEN** it opens in the system browser

### Requirement: Catalog source is credited as text
PC ports SHALL show the text "Port catalog by quiverlauncher.com" in the catalog view and in the detail pane, with a link that opens the site in the system browser. The application SHALL NOT display Quiver logos or icons or suggest that Quiver endorses GameNode. Each entry SHALL credit its port project and author as the upstream names them.

#### Scenario: Credit is visible
- **WHEN** the user opens PC ports
- **THEN** the credit text and link are visible without scrolling, and no Quiver logo is shown

### Requirement: Port catalog strings are translated
Every PC ports screen, filter, status, guided-setup step and error SHALL be available in English and Brazilian Portuguese. Port names, game titles, descriptions, recipe labels and release notes from the upstream SHALL be shown as received.

#### Scenario: Brazilian Portuguese catalog
- **WHEN** the application runs with a Brazilian Portuguese OS locale
- **THEN** PC ports controls and messages use Brazilian Portuguese, and entry descriptions and recipe labels are unchanged
