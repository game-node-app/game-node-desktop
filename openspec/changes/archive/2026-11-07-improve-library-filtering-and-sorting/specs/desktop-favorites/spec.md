# Spec Delta

## Purpose

Let the user mark matched games as favorites, browse them as a filter, and keep that choice in step with their GameNode account when sync is active.

## ADDED Requirements

### Requirement: Favorite matched games
The library SHALL let the user mark a matched entry as a favorite and remove it again. The favorite SHALL belong to the GameNode game, so every entry matched to the same game shows the same state. An entry without a GameNode match SHALL NOT be favoritable and SHALL explain why. The change SHALL be visible immediately.

#### Scenario: Add a favorite
- **WHEN** the user chooses Add to favorites on a matched entry
- **THEN** its star becomes filled immediately, and the Favorites filter includes it

#### Scenario: Remove a favorite
- **WHEN** the user chooses Remove from favorites on a favorited entry
- **THEN** its star clears immediately, and the Favorites filter excludes it

#### Scenario: Same game from two providers
- **WHEN** a Steam entry and a DRM-free entry are matched to the same GameNode game and one is favorited
- **THEN** both entries show the favorite state

#### Scenario: Unmatched entry
- **WHEN** the user opens the favorite action on an entry with no GameNode match
- **THEN** the action is disabled with a reason saying the game must be matched first

### Requirement: Favorites are local-first and per account
Favorites SHALL be stored locally for the signed-in GameNode account and SHALL remain usable while signed out, offline, with sync off, or while the session is unverified. Signing out SHALL keep the account's favorites for its next sign-in and SHALL NOT expose them to another account.

#### Scenario: Favorite with sync off
- **WHEN** the user favorites a game while sync is off
- **THEN** the star stays filled and no favorite request is sent

#### Scenario: Sign out and back in
- **WHEN** the user favorites a game, signs out, and signs in again with the same account
- **THEN** the game is still favorited

#### Scenario: Another account
- **WHEN** a different GameNode account signs in on the same PC
- **THEN** it starts with its own favorite set and the first account's favorites stay hidden

### Requirement: Favorites sync with GameNode
When sync is active — the switch is on, its explanation has been dismissed, and the session is verified — the application SHALL read the account's favorite games from GameNode and SHALL send each local favorite change. A local change that GameNode has not confirmed SHALL take precedence over the server value. A completed read SHALL make the local set match the server for games with no pending change. A failed or incomplete read SHALL change nothing.

#### Scenario: Server favorite appears locally
- **WHEN** sync becomes active and GameNode reports a game as favorite
- **THEN** its local entries show the favorite state without a user action

#### Scenario: Local favorite is sent
- **WHEN** the user favorites a matched game while sync is active
- **THEN** GameNode receives the new favorite state for that game

#### Scenario: Local change wins over a stale server value
- **WHEN** the user removes a favorite locally and a read still returns it as favorite before the removal is confirmed
- **THEN** the local removal stays and is sent again

#### Scenario: Server removal appears locally
- **WHEN** a read completes and a game is no longer a favorite on GameNode, with no pending local change
- **THEN** its local entries stop showing the favorite state

#### Scenario: Read fails
- **WHEN** the favorite read fails or returns only some pages
- **THEN** local favorites are unchanged and the read is retried later

### Requirement: Pending favorites retry
A favorite change that cannot be sent SHALL stay applied locally and SHALL be retried when sync is active again or at the next library sync. A game that is not in the GameNode library SHALL stay pending until the game is added, then sync without a further user action.

#### Scenario: Offline change
- **WHEN** the user favorites a game while GameNode is unreachable
- **THEN** the star stays filled locally and the change is retried later

#### Scenario: Sync turned on later
- **WHEN** the user turns sync on after favoriting games while it was off
- **THEN** the pending favorites are sent

#### Scenario: Game not in the GameNode library
- **WHEN** the user favorites a matched game that is not in their GameNode library
- **THEN** the star stays filled locally and the change is kept pending

#### Scenario: Game added later
- **WHEN** the user adds that game to their GameNode library
- **THEN** its pending favorite is sent without the user toggling it again

### Requirement: Favorites in the library view
The library SHALL show a favorite indicator in grid and list, offer Add to favorites or Remove from favorites for a single game in the context menu and detail pane, and provide a Favorites filter that shows only favorited entries. The actions and the filter SHALL be keyboard accessible and available in English and Brazilian Portuguese.

#### Scenario: Indicator
- **WHEN** a favorited game is visible in grid or list
- **THEN** its tile or row shows a star indicator that is announced to assistive technology

#### Scenario: Context menu
- **WHEN** the user opens the context menu on a matched entry
- **THEN** the menu offers Add to favorites or Remove from favorites according to its current state

#### Scenario: Favorites filter
- **WHEN** the user turns on the Favorites filter
- **THEN** only favorited entries remain, and the active-filter chip can remove the filter

#### Scenario: Multi-selection
- **WHEN** several entries are selected
- **THEN** the multi-selection context menu does not offer a favorite action

#### Scenario: Keyboard
- **WHEN** the user reaches the favorite action or the Favorites filter with the keyboard
- **THEN** it can be activated with Enter or Space and its state is exposed
