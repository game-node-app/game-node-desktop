# Spec Delta

## MODIFIED Requirements

### Requirement: Show GameNode library membership
For a selected matched entry, the detail pane SHALL show whether the game is already in the user's GameNode library and, if so, its status and collections. The application SHALL show the cached account library state immediately and SHALL refresh that game's entry from GameNode when the entry is selected, so membership is never assumed from local data alone when GameNode can be reached.

#### Scenario: Game already in the GameNode library
- **WHEN** the user selects a matched game that is in their GameNode library as Finished
- **THEN** the detail pane shows Finished on GameNode with its collections and offers Edit on GameNode

#### Scenario: Membership cannot be read
- **WHEN** GameNode cannot be reached while the detail pane loads membership
- **THEN** the cached state, if any, stays visible, Add to GameNode is disabled with an explanation, and nothing is sent

#### Scenario: Changed on the web
- **WHEN** the cache says Planned, the user changed the game to Playing on the web, and then selects it
- **THEN** the pane updates to Playing once the refresh answers, and the cache is updated

### Requirement: Explicit editing of a detected game
The application SHALL offer Edit for a selected store-detected matched game already in the account library, prefilled with its status and collections. Confirming SHALL save those choices while preserving owned platforms and passing through the completion date under the server’s date rules. Cancelling SHALL send no mutation. The full GameNode account library SHALL be read only to show the state of games that have local entries and to mark catalog results; games that exist only in the account library SHALL NOT be shown as library entries.

#### Scenario: Edit an existing detected game
- **WHEN** the user changes a detected game’s status and collections and confirms Edit
- **THEN** those choices are saved and the game remains editable
- **AND** its platforms and completion date are preserved according to the server’s status rules

#### Scenario: Edit save fails
- **WHEN** saving an edit fails
- **THEN** the choices are retained for retry

#### Scenario: Existing collections cannot be loaded
- **WHEN** the dialog cannot load all existing collection choices
- **THEN** saving is disabled until the collections can be loaded

#### Scenario: Account-only games stay out of the library
- **WHEN** the account library contains a game that no local entry is matched to
- **THEN** it is not listed in the library, and it appears as In your library only in catalog search results

### Requirement: Add several games at once
With several matched entries selected, Add to GameNode SHALL apply one status and set of collections to every selected game that is not already in the user's GameNode library. The result SHALL be shown as a notification saying how many games were added, already present, skipped or failed, with Undo and View on GameNode when at least one game was added.

#### Scenario: Mixed multi-selection
- **WHEN** the user adds five selected games, one of which is already in their GameNode library and one unmatched
- **THEN** the three other games are added, the existing entry is unchanged, the unmatched game is skipped, and the summary reports each outcome

## ADDED Requirements

### Requirement: Cache the account library
With a verified session, the application SHALL read every entry of the user's GameNode library, with its status and collections, at sign-in verification and at each rescan, and SHALL keep the last complete read per account. A failed or partial read SHALL keep the previous cache. After any add, edit, status change, collection change or undo, the affected games SHALL be refreshed.

#### Scenario: Offline after a read
- **WHEN** the application starts offline after a complete read
- **THEN** the GameNode status column, status filter, collection scopes and detail pane show the cached state

#### Scenario: Partial read
- **WHEN** reading the account library fails on its third page
- **THEN** the previous cache stays unchanged and the read is retried at the next rescan

### Requirement: Undo an add
After an add creates entries, its notification SHALL offer Undo while shown. Undo SHALL delete exactly the entries that add created, never entries that were already present, then refresh those games and report the result.

#### Scenario: Undo a multi-add
- **WHEN** an add creates three entries, skips one already present, and the user chooses Undo
- **THEN** the three created entries are deleted from the GameNode library, the existing one is untouched, and a notification confirms the undo

#### Scenario: Undo fails
- **WHEN** a delete request fails during Undo
- **THEN** a danger notification says the add could not be fully undone and the games that are still in the library show their state

### Requirement: Change GameNode status in batch
Change status, from the batch bar, the multi-selection detail pane or the context menu, SHALL set one status on every selected game already in the user's GameNode library, preserving collections, platforms and completion date under the server's rules. Games not in the library SHALL be skipped and counted in the result.

#### Scenario: Mixed selection
- **WHEN** three games are selected, two in the library, and the user chooses Change status › Finished
- **THEN** the two games become Finished, and the notification says the status changed for 2 games and 1 not on GameNode was skipped

### Requirement: Add to a collection
Add to collection, from the context menu or the multi-selection detail pane, SHALL list the user's collections and add every selected game already in the GameNode library to the chosen one, keeping its status and existing collections. It SHALL be disabled with "Add to GameNode first" when no selected game is in the library.

#### Scenario: Add one game to a collection
- **WHEN** the user chooses Add to collection › Coop no sofá on a game in the library
- **THEN** the game is in that collection, its status is unchanged, and a notification confirms it

#### Scenario: Game not in the library
- **WHEN** the user opens Add to collection on a game not in the GameNode library
- **THEN** the item is disabled with "Add to GameNode first"

### Requirement: Open GameNode pages
Edit on GameNode SHALL open the game's page on the GameNode website, and View on GameNode SHALL open the user's library page, in the default browser. The website address SHALL be the official one unless overridden for development.

#### Scenario: Edit on GameNode
- **WHEN** the user chooses Edit on GameNode for a matched game
- **THEN** the browser opens that game's GameNode page

#### Scenario: Browser fails
- **WHEN** no browser can be opened
- **THEN** a localized error is shown and nothing else changes
