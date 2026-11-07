# Spec Delta

## ADDED Requirements

### Requirement: Persist Steam catalog facts and sync results
The library schema SHALL store independent installed, owned, and family-shared facts for each
entry, and the last owned and family catalog sync attempt per provider with a categorized error.
Missing sync state SHALL mean never synced. Unreadable optional values SHALL NOT prevent the
library from opening or make a game visible as owned.

#### Scenario: Restart with a synced catalog
- **WHEN** the application restarts after a successful owned and family sync
- **THEN** the cached owned and family facts are visible before any Steam request finishes

#### Scenario: Catalog failure
- **WHEN** a catalog sync fails
- **THEN** the previous owned or family facts are kept and the failure category is stored for Stores

#### Scenario: Corrupt ownership value
- **WHEN** an entry's owned or family-shared value cannot be read
- **THEN** that fact is treated as false, the problem is logged, and the entry's installed fact still applies

#### Scenario: Never synced
- **WHEN** a provider has no catalog sync record
- **THEN** its installed games are shown and owned and family facts are treated as unknown, not as an empty library

#### Scenario: Steam account change
- **WHEN** a different Steam account connects on the same OS user
- **THEN** the previous ownership facts are replaced while entries, matches, covers, and playtime history are kept

### Requirement: Separate Steam credential storage
The Steam session cookie SHALL use a credential slot and storage location separate from the
GameNode session. On the Linux private-file fallback it SHALL be a separate file under the 0700
private directory, created with 0600 permissions, and never following a symlink. It SHALL NOT be
written to the SQLite database, settings, backups, logs, or snapshots.

#### Scenario: Separate slots
- **WHEN** the user signs out of Steam
- **THEN** the Steam credential is removed and the GameNode session credential is unchanged

#### Scenario: Linux private file
- **WHEN** the Steam credential is stored in the private-file fallback
- **THEN** its parent directory is 0700 and the file is 0600

#### Scenario: Symlinked credential path
- **WHEN** the Steam credential path is replaced by a symbolic link
- **THEN** reading and writing the credential fail instead of following the link

#### Scenario: No database secrets
- **WHEN** the Steam account connects or refreshes
- **THEN** the database contains no Steam cookie or short-lived API token

## MODIFIED Requirements

### Requirement: Persist foundation preferences without credentials
The local database SHALL retain renderer preference, window geometry and library layout across restarts. It SHALL NOT contain session tokens, Steam session cookies, Steam web API tokens, authentication callback URLs, or pending sign-in secrets. It MAY contain the last signed-in GameNode account's id and display name and the last connected Steam account's id and display name, so that the cached library and account card can open before verification.

#### Scenario: Settings survive restart
- **WHEN** a renderer preference, window geometry and library layout have been saved and the application restarts
- **THEN** those saved values are available before window initialization

#### Scenario: Authentication changes
- **WHEN** the user signs in, restores a session, or signs out
- **THEN** the database contains no authentication credential or complete callback URL

#### Scenario: Steam authentication changes
- **WHEN** the user connects, refreshes, or signs out of Steam
- **THEN** the database contains no Steam cookie, API token, or complete Steam login URL

#### Scenario: Steam identity survives restart
- **WHEN** the application restarts with a stored Steam credential
- **THEN** Stores can show the last connected Steam account name while the session is being checked, and the database contains no cookie or token

#### Scenario: Steam sign-out
- **WHEN** the user signs out of Steam
- **THEN** the stored Steam credential is removed and the record of the last GameNode account is unchanged

#### Scenario: Sign-out
- **WHEN** the user signs out of GameNode
- **THEN** the record of the last signed-in account is removed, and the next start without a session shows sign-in rather than a cached library

### Requirement: Persist the library cache per account
The local database SHALL store detected library entries with independent installed, owned, and family-shared facts, the last installed scan and catalog sync results per provider, and matches, and SHALL keep chosen matches separate for each GameNode account. Cached covers SHALL be stored as files under the per-user data directory. Steam ownership facts SHALL belong to the OS user and be shared by GameNode accounts on that OS user. Signing out SHALL keep entries, matches, covers, and playtime for a later sign-in.

#### Scenario: Restart with a scanned library
- **WHEN** the application restarts after a completed scan and catalog sync
- **THEN** the library entries, ownership facts, matches and cover files are available before the next scan finishes

#### Scenario: Upgrade from the foundation schema
- **WHEN** a step 1 database is opened by this version
- **THEN** the library tables are added by a new migration and the existing settings are kept

#### Scenario: Steam catalog and another GameNode account
- **WHEN** a different GameNode account signs in under the same OS user
- **THEN** it sees the same Steam ownership facts but its own chosen matches

#### Scenario: Steam sign-out keeps matches
- **WHEN** the user signs out of Steam
- **THEN** owned and family-shared facts are cleared while entries, matches, covers, and playtime are kept

### Requirement: Upgrade preserves existing desktop data
The new schema SHALL be applied through ordered transactional migrations that preserve Steam entries, catalog facts, sync results, settings, matches and playtime. An older application SHALL refuse the newer schema without modifying it. Application uninstall SHALL preserve backups and user data under the existing keep-data policy.

#### Scenario: Upgrade from playtime release
- **WHEN** an existing step 3 database is upgraded
- **THEN** all previous Steam and account data remains and the manual registrations, watches and backup history start empty

#### Scenario: Upgrade adds catalog state
- **WHEN** an existing step 5 database is upgraded
- **THEN** installed entries remain visible, owned and family-shared facts start false, catalog sync state starts empty, and matches, covers, settings and playtime are preserved
