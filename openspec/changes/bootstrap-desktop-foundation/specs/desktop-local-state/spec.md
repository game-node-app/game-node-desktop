# Spec Delta

## Purpose

Keep non-secret GameNode Desktop state durable in an application-owned local database, with predictable initialization and migrations that preserve existing user settings.

## ADDED Requirements

### Requirement: Initialize persistent local state
The application SHALL create its SQLite database in the current user's application-data location and initialize it with ordered, versioned migrations before normal application use. A fresh database SHALL start with foundation defaults and no game records.

#### Scenario: First launch
- **WHEN** the application starts without an existing local database
- **THEN** it creates a database at the per-user application-data location and applies all current migrations
- **AND** the shell can open without downloading library data

### Requirement: Apply each migration once
The application SHALL record completed schema versions and apply each pending migration transactionally and in order. Reopening a current database SHALL NOT reapply completed migrations or reset settings.

#### Scenario: Reopen current database
- **WHEN** the application opens a database that already has the current schema
- **THEN** it preserves settings and migration history without rerunning completed migrations

#### Scenario: Upgrade an older schema
- **WHEN** the application opens a supported older database
- **THEN** pending migrations run in order and preserve existing settings

### Requirement: Fail safely on unusable local data
If database access or a migration fails, the application SHALL report the local-storage problem without deleting or replacing the existing database. It SHALL reject a schema newer than it supports without modifying it.

#### Scenario: Migration fails
- **WHEN** a migration encounters an error
- **THEN** its changes are rolled back, earlier committed data remains intact, and normal startup does not proceed against a partially migrated schema

#### Scenario: Binary is older than the database
- **WHEN** the application opens a database with an unsupported newer schema
- **THEN** it reports that incompatibility and leaves the database unchanged

### Requirement: Persist foundation preferences without credentials
The local database SHALL retain renderer preference and window geometry across restarts. It SHALL NOT contain session tokens, authentication callback URLs, or pending sign-in secrets.

#### Scenario: Settings survive restart
- **WHEN** a renderer preference and window geometry have been saved and the application restarts
- **THEN** those saved values are available before window initialization

#### Scenario: Authentication changes
- **WHEN** the user signs in, restores a session, or signs out
- **THEN** the database contains no authentication credential or complete callback URL
