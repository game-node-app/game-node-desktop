# Spec Delta

## ADDED Requirements

### Requirement: Persist library organization per account
The local database SHALL store each account's saved views (name, scope, filters, sort, grouping and view mode) and hidden entries. Signing out SHALL keep them for the same account's next sign-in and SHALL NOT expose them to another account. A database without these tables SHALL be upgraded by a migration that keeps all existing data.

#### Scenario: Views and hidden games survive restart
- **WHEN** the user saves a view, hides a game, closes the application and opens it again
- **THEN** the view is in the sidebar and the game is still hidden before any network request finishes

#### Scenario: Upgrade from the previous schema
- **WHEN** a database from the previous schema is opened
- **THEN** the new tables are added and the existing settings, library cache, matches, favorites, playtime and achievements are kept

### Requirement: Persist the account library cache
The local database SHALL store, per account, the last complete read of the GameNode library (entry id, game, status, collections) and the account's collections, replaced together only after a complete read. Signing out SHALL keep the cache for the same account and SHALL NOT expose it to another account.

#### Scenario: Restart offline
- **WHEN** the application restarts offline after a complete account library read
- **THEN** GameNode statuses and collections are shown from the cache

#### Scenario: Another account
- **WHEN** a different GameNode account signs in
- **THEN** it does not see the first account's cached statuses or collections

### Requirement: Remember the last watched-folder check
The application SHALL remember when the watched DRM-free folders were last checked, so the status bar can show it after a restart.

#### Scenario: Restart
- **WHEN** the watched folders were checked 12 minutes before a restart
- **THEN** the status bar shows the last check as 12 minutes ago after the restart
