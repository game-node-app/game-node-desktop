# Spec Delta

## Purpose

Let the user add detected games to their GameNode library, with a status and collections they choose, through the API the web uses, without changing entries they already have.

## ADDED Requirements

### Requirement: Show GameNode library membership
For a selected matched entry, the detail pane SHALL show whether the game is already in the user's GameNode library and, if so, its status. The application SHALL read this from GameNode when the entry is selected and SHALL NOT assume membership from local data alone.

#### Scenario: Game already in the GameNode library
- **WHEN** the user selects a matched game that is in their GameNode library as Finished
- **THEN** the detail pane shows "In your GameNode library: Finished" and does not offer Add to GameNode

#### Scenario: Membership cannot be read
- **WHEN** GameNode cannot be reached while the detail pane loads membership
- **THEN** Add to GameNode is disabled with an explanation, and nothing is sent

### Requirement: Add with a status and collections
Add to GameNode SHALL open a dialog where the user picks one status (Playing, Finished, Planned, Dropped or Ongoing) and any of their own GameNode collections. Confirming SHALL create the entry with platform PC through GameNode's existing collection-entry API. Cancelling SHALL send nothing.

#### Scenario: Add a game as Playing to a collection
- **WHEN** the user picks Playing and one of their collections and confirms
- **THEN** the game is in their GameNode library as Playing, on PC, in that collection
- **AND** the detail pane shows the new status

#### Scenario: Collection sets the status
- **WHEN** a selected collection has a default status
- **THEN** the dialog shows that status, as GameNode will apply it, before the user confirms

#### Scenario: Request fails
- **WHEN** the create request fails or is rejected
- **THEN** a localized error is shown, the dialog keeps the user's choices, and the user can retry

### Requirement: Never overwrite an existing entry
The application SHALL check that the game is not already in the user's GameNode library immediately before creating an entry, and SHALL NOT send a create request for a game that is. The application SHALL NOT edit, move or remove existing GameNode library entries.

#### Scenario: Added elsewhere while the dialog was open
- **WHEN** the game was added on the web after the dialog opened and the user confirms
- **THEN** the application does not send the create request and shows the existing status

### Requirement: Add several games at once
With several matched entries selected, Add to GameNode SHALL apply one status and set of collections to every selected game that is not already in the user's GameNode library. The result SHALL say how many games were added, already present, or failed.

#### Scenario: Mixed multi-selection
- **WHEN** the user adds five selected games, one of which is already in their GameNode library and one unmatched
- **THEN** the three other games are added, the existing entry is unchanged, the unmatched game is skipped, and the summary reports each outcome

### Requirement: Never add automatically
The application SHALL add a game to the user's GameNode library only after the user confirms the Add to GameNode dialog. Scanning, matching, launching and signing in SHALL NOT add games.

#### Scenario: Scan a new library
- **WHEN** a first scan detects and matches many games
- **THEN** none of them are added to the user's GameNode library
