# Spec Delta

## MODIFIED Requirements

### Requirement: What leaves the PC while sync is off
While sync is off, the application SHALL NOT send playtime totals, sessions, detected games or favorite changes to GameNode. It SHALL still send the requests that sign-in, matching, match suggestions, game and catalog search, covers, reading the user's GameNode library and collections, and the actions the user explicitly confirms (Add to GameNode, Edit, Undo of an add, Change status and Add to collection) need. Playtime SHALL keep being recorded locally and favorites SHALL keep being stored locally.

#### Scenario: Play with sync off
- **WHEN** the user plays a matched Steam game while sync is off
- **THEN** no playtime request is sent, and the game's total is kept locally

#### Scenario: Matching still works
- **WHEN** sync is off and a scan finds a new game
- **THEN** GameNode is still asked for its match and cover

#### Scenario: Turn sync back on
- **WHEN** the user turns sync on after playing with it off
- **THEN** totals that changed meanwhile are submitted

#### Scenario: Favorite with sync off
- **WHEN** the user favorites a game while sync is off
- **THEN** no favorite request is sent and the star stays filled locally

#### Scenario: Turn sync on after favoriting
- **WHEN** the user turns sync on after favoriting games with it off
- **THEN** the pending favorite changes are sent

#### Scenario: Status column with sync off
- **WHEN** sync is off and the session is verified
- **THEN** the user's GameNode library is still read so statuses and collections are shown, and a batch status change the user confirms is sent
