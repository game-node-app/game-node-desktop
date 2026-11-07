# Spec Delta

## ADDED Requirements

### Requirement: Persist favorites per account
The local database SHALL store each account's favorite games and whether each change has been confirmed by GameNode. Signing out SHALL keep them for a later sign-in by the same account. A database without the favorites table SHALL be upgraded by a migration that keeps existing settings, library cache, matches and playtime.

#### Scenario: Favorites survive restart
- **WHEN** the user favorites games, closes the application and opens it again
- **THEN** the same games are favorited before any network request finishes

#### Scenario: Pending change survives restart
- **WHEN** a favorite change was made offline and the application restarts
- **THEN** the change is still applied locally and still pending

#### Scenario: Same account signs in again
- **WHEN** an account signs out and later signs in again on the same PC
- **THEN** its favorites are available again

#### Scenario: Upgrade from the previous schema
- **WHEN** a database from the previous schema is opened
- **THEN** the favorites table is added and the existing data is kept
