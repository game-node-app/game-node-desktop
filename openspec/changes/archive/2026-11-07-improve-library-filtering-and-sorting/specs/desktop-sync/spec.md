# Spec Delta

## MODIFIED Requirements

### Requirement: What leaves the PC while sync is off
While sync is off, the application SHALL NOT send playtime totals, sessions, detected games or favorite changes to GameNode. It SHALL still send the requests that sign-in, matching, game search, covers and Add to GameNode need. Playtime SHALL keep being recorded locally and favorites SHALL keep being stored locally.

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

### Requirement: Sync is explained on first use
After the first successful sign-in on this PC, the application SHALL show once what "Sync with GameNode" sends, including playtime and favorites, that it is on, and where to turn it off. Until the user dismisses it, it SHALL be shown again the next time the window opens, and neither playtime nor favorites SHALL be sent before it is dismissed.

#### Scenario: First sign-in
- **WHEN** a user signs in for the first time on this PC
- **THEN** the window explains Sync with GameNode with a way to keep it on or open its setting

#### Scenario: First start is in the background
- **WHEN** GameNode is installed, signed in, and then starts at login in the background before the explanation was ever shown
- **THEN** playtime is recorded locally but not sent until the explanation has been shown in the window and dismissed

#### Scenario: Already explained
- **WHEN** the user dismissed the explanation and signs in again later, as the same or another account
- **THEN** the explanation is not shown again

#### Scenario: Favorite before the explanation
- **WHEN** the user favorites a game before dismissing the explanation
- **THEN** the favorite is kept locally and sent after the explanation is dismissed
