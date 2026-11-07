# Spec Delta

## ADDED Requirements

### Requirement: Last played reflects the selected source
The library SHALL expose each entry's last-played time from the selected playtime source. With Store selected it SHALL come from that game's store total. With Internal selected it SHALL be the end of the entry's most recent measured session, including the current session while the game runs. When the source has no date, last played SHALL be unknown rather than invented. Account changes SHALL clear it before the new account's values load.

#### Scenario: Store last played
- **WHEN** Store is selected and the store reports a last-played date for a game
- **THEN** that date is used for the entry's Last played sort and filter

#### Scenario: Internal last played
- **WHEN** Internal is selected and a game's latest session ended at 18:00
- **THEN** last played is 18:00, and it follows the current session while the game runs

#### Scenario: No date
- **WHEN** the selected source has no last-played date for an entry
- **THEN** last played is unknown, the entry sorts last for that key, and no last-played window filter counts it

#### Scenario: Account change
- **WHEN** a different account signs in
- **THEN** the previous account's last-played dates are cleared before the new values load
