# Spec Delta

## MODIFIED Requirements

### Requirement: Persist foundation preferences without credentials
The local database SHALL retain window geometry and library layout across restarts, and SHALL safely ignore any legacy `renderer` column values present in existing databases. It SHALL NOT contain session tokens, authentication callback URLs, or pending sign-in secrets. It MAY contain the last signed-in account's id and display name, so that the cached library can open before the session is verified.

#### Scenario: Settings survive restart
- **WHEN** window geometry and library layout have been saved and the application restarts
- **THEN** those saved values are available before window initialization, even when opening an existing database that still contains a legacy `renderer` column

#### Scenario: Authentication changes
- **WHEN** the user signs in, restores a session, or signs out
- **THEN** the database contains no authentication credential or complete callback URL

#### Scenario: Sign-out
- **WHEN** the user signs out
- **THEN** the record of the last signed-in account is removed, and the next start without a session shows sign-in rather than a cached library
