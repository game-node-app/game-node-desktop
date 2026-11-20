# Spec Delta

## ADDED Requirements

### Requirement: Collection sizes in the add dialog
The add and edit dialog SHALL show, next to each of the user's collections, how many games it holds, counted from the last complete read of the user's GameNode library. When no complete read exists, counts SHALL be hidden rather than shown as zero.

#### Scenario: Counts from the cached library
- **WHEN** the user opens Add to GameNode and the cached library has nine entries in "Backlog curto"
- **THEN** "Backlog curto" shows 9 games

#### Scenario: No complete read
- **WHEN** the account library has never been read completely on this PC
- **THEN** the collections are listed without counts

### Requirement: Create a collection from the add dialog
The add and edit dialog SHALL let the user create a new collection by name through GameNode's collection API without closing the dialog. The new collection SHALL appear selected in the list. A failed creation SHALL keep the typed name and the dialog's other choices and show a localized error. Creating a collection SHALL NOT add or edit any game until the user confirms the dialog.

#### Scenario: Create and select
- **WHEN** the user chooses New collection, types "Coop no sofá" and confirms the name
- **THEN** the collection is created on GameNode, appears in the list checked with no games, and no game has been added yet

#### Scenario: Creation fails
- **WHEN** creating the collection fails or GameNode cannot be reached
- **THEN** an error is shown, the typed name is kept for retry, and the chosen status and collections are unchanged

#### Scenario: Collection limit reached
- **WHEN** GameNode refuses the new collection because the account reached its collection limit
- **THEN** the error says the collection limit was reached, and the dialog's choices are unchanged

#### Scenario: Blank name
- **WHEN** the name is empty or only spaces
- **THEN** the create action is disabled

### Requirement: Batch add shows per-game progress
Adding several games SHALL show each selected game with its outcome as it happens (waiting, adding, added, already in the library, skipped with no GameNode match, or failed) and overall progress. Unmatched games SHALL be listed as skipped and SHALL NOT stop the others. Hiding the dialog SHALL NOT cancel the run; the remaining games SHALL keep being added and the final result SHALL be reported as a notification.

#### Scenario: Progress with a skipped game
- **WHEN** the user adds three selected games and one has no GameNode match
- **THEN** the dialog shows 2 of 3 progress, the matched games as added or adding, and the unmatched one as skipped

#### Scenario: Hide during the run
- **WHEN** the user chooses Hide while games are still being added
- **THEN** the dialog closes, the remaining games are still added, and a notification reports the outcome when the run ends

#### Scenario: Sign-out during a hidden run
- **WHEN** the account signs out or changes while a hidden run is still adding games
- **THEN** no further create request is sent and the notification reports the games that were not added
