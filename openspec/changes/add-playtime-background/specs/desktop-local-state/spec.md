# Spec Delta

## ADDED Requirements

### Requirement: Persist playtime per account
The local database SHALL store recorded sessions, open sessions with their last checkpoint, and the last accepted store total per game, each tied to the GameNode account it belongs to. Signing out SHALL keep them for a later sign-in by the same account. The database SHALL NOT store process lists or command lines.

#### Scenario: Restart with queued sessions
- **WHEN** the application restarts with sessions in the queue
- **THEN** the same sessions, with the same ids and times, are still queued

#### Scenario: Same account signs in again
- **WHEN** an account signs out and later signs in again on the same PC
- **THEN** its queued sessions and accepted totals are available again

#### Scenario: Upgrade from the step 2 schema
- **WHEN** a step 2 database is opened by this version
- **THEN** the playtime tables are added by a new migration and the existing settings, library cache and matches are kept

### Requirement: Persist sync preferences
The local database SHALL keep the "Sync with GameNode" switch and whether its explanation was dismissed. A database without these values SHALL behave as sync on and explanation not yet dismissed. A corrupt sync value SHALL be treated as off, so that a user's opt-out is never lost to corruption.

#### Scenario: Sync setting survives restart
- **WHEN** the user turns sync off and restarts the application
- **THEN** sync is off before any playtime could be submitted

#### Scenario: Upgraded database
- **WHEN** a step 2 database without sync values is opened
- **THEN** sync is on and the explanation has not been dismissed

#### Scenario: Corrupt value
- **WHEN** the stored sync value cannot be read
- **THEN** sync is treated as off and the explanation is shown again

### Requirement: Persist playtime source
The database SHALL persist the PC-wide `playtime_source` preference across restarts and accounts.
Missing or invalid values SHALL select Store.

#### Scenario: Internal survives restart
- **WHEN** the user selects Internal and restarts
- **THEN** Internal is selected before tracking or submitting starts

#### Scenario: Missing or invalid source
- **WHEN** the source setting is missing or invalid
- **THEN** Store is selected
