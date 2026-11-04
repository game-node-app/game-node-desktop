# Spec Delta

## MODIFIED Requirements

### Requirement: Persist foundation preferences without credentials
The local database SHALL retain renderer preference, window geometry and library layout across restarts. It SHALL NOT contain session tokens, authentication callback URLs, or pending sign-in secrets. It MAY contain the last signed-in account's id and display name, so that the cached library can open before the session is verified.

#### Scenario: Settings survive restart
- **WHEN** a renderer preference, window geometry and library layout have been saved and the application restarts
- **THEN** those saved values are available before window initialization

#### Scenario: Authentication changes
- **WHEN** the user signs in, restores a session, or signs out
- **THEN** the database contains no authentication credential or complete callback URL

#### Scenario: Sign-out
- **WHEN** the user signs out
- **THEN** the record of the last signed-in account is removed, and the next start without a session shows sign-in rather than a cached library

## ADDED Requirements

### Requirement: Persist the library cache per account
The local database SHALL store detected library entries, the last scan result per provider, and matches, and SHALL keep chosen matches separate for each GameNode account. Cached covers SHALL be stored as files under the per-user data directory. Signing out SHALL keep this data for a later sign-in by the same account.

#### Scenario: Restart with a scanned library
- **WHEN** the application restarts after a completed scan
- **THEN** the library entries, matches and cover files from that scan are available before the next scan finishes

#### Scenario: Upgrade from the foundation schema
- **WHEN** a step 1 database is opened by this version
- **THEN** the library tables are added by a new migration and the existing settings are kept
