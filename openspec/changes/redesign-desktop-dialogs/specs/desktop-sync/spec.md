# Spec Delta

## MODIFIED Requirements

### Requirement: Sync is explained on first use
After the first successful sign-in on this PC, the application SHALL show once what "Sync with GameNode" sends (playtime and sessions, the games detected on this PC and their matches, and favorites), what stays on this PC (save files, install paths and executables), that it is on, and where to turn it off. It SHALL offer keeping sync on and turning it off directly. Until the user dismisses it, it SHALL be shown again the next time the window opens, and neither playtime nor favorites SHALL be sent before it is dismissed.

#### Scenario: First sign-in
- **WHEN** a user signs in for the first time on this PC
- **THEN** the window explains Sync with GameNode, listing what is sent and what stays on this PC, with a way to keep it on or turn it off

#### Scenario: Turn sync off from the explanation
- **WHEN** the user chooses Turn sync off in the explanation
- **THEN** sync is turned off as if from Settings, the explanation is dismissed, and nothing pending is sent

#### Scenario: First start is in the background
- **WHEN** GameNode is installed, signed in, and then starts at login in the background before the explanation was ever shown
- **THEN** playtime is recorded locally but not sent until the explanation has been shown in the window and dismissed

#### Scenario: Already explained
- **WHEN** the user dismissed the explanation and signs in again later, as the same or another account
- **THEN** the explanation is not shown again

#### Scenario: Favorite before the explanation
- **WHEN** the user favorites a game before dismissing the explanation
- **THEN** the favorite is kept locally and sent after the explanation is dismissed
