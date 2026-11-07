# desktop-local-state Specification

## Purpose

Keep non-secret GameNode Desktop state durable in an application-owned local database, with predictable initialization and migrations that preserve existing user settings.

## Requirements

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
The local database SHALL retain window geometry and library layout across restarts, and SHALL safely ignore any legacy `renderer` column values present in existing databases. It SHALL NOT contain session tokens, Steam session cookies, Steam web API tokens, authentication callback URLs, or pending sign-in secrets. It MAY contain the last signed-in GameNode account's id and display name and the last connected Steam account's id and display name, so that the cached library and account card can open before verification.

#### Scenario: Settings survive restart
- **WHEN** window geometry and library layout have been saved and the application restarts
- **THEN** those saved values are available before window initialization, even when opening an existing database that still contains a legacy `renderer` column

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

### Requirement: Persist Steam catalog facts and sync results
The library schema SHALL store independent installed, owned, and family-shared facts for each entry, and the last owned and family catalog sync attempt per provider with a categorized error. Missing sync state SHALL mean never synced. Unreadable optional values SHALL NOT prevent the library from opening or make a game visible as owned.

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
The Steam session cookie SHALL use a credential slot and storage location separate from the GameNode session. On the Linux private-file fallback it SHALL be a separate file under the 0700 private directory, created with 0600 permissions, and never following a symlink. It SHALL NOT be written to the SQLite database, settings, backups, logs, or snapshots.

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

### Requirement: Persist manual installations and discovery decisions
Local storage SHALL retain stable manual installation ids, approved launch/tracking settings, watched roots, candidates and ignore decisions across restarts. Repair SHALL update an existing registration transactionally. GameNode chosen matches SHALL remain account-specific; local registrations and discovery settings SHALL belong to the Windows user and never contain credentials.

#### Scenario: Restart after add and ignore
- **WHEN** the user adds one game, ignores another candidate and restarts
- **THEN** the added entry returns with its settings and the ignored candidate is not suggested again

#### Scenario: Another GameNode account
- **WHEN** a different GameNode account signs in under the same Windows user
- **THEN** it sees local installations and OS-local save protection but does not inherit the previous account's catalog choices or playtime

### Requirement: Persist evidence separately from user choices
The application SHALL cache inferred title evidence, automatic catalog matches, provenance and the evidence revision used to obtain them. User choices SHALL remain separate and authoritative. Executable or installation changes SHALL invalidate stale automatic work without erasing chosen catalog matches.

#### Scenario: Changed executable
- **WHEN** repair changes an installation's executable
- **THEN** new evidence can be gathered and obsolete inference responses cannot overwrite its current state

### Requirement: Persist save profiles and backup jobs
Local storage SHALL retain per-installation save profiles, revisions, automatic-backup preferences, backup destination, jobs and results. Backup bytes SHALL live outside SQLite under a stable installation/profile namespace independent of catalog titles. Interrupted operations SHALL be reconciled against actual archives before success is recorded or retention is applied.

#### Scenario: Crash after writing an archive
- **WHEN** an archive was committed but GameNode stopped before recording the result
- **THEN** restart discovers that archive and does not blindly repeat the same completed job

#### Scenario: Change save profile
- **WHEN** the user selects a different save profile
- **THEN** prior archives remain attached to their original profile and queued work for the old revision is cancelled

### Requirement: Persist per-game tool installations, backups and release cache metadata
The local database SHALL store installed tool records (`tool_installations`), file/backup manifests (`tool_files`), and release cache metadata (`tool_releases_cache`) via migration `0005_game_tools.sql`. Tool installations SHALL belong to the Windows user (`(entry_key, tool_id)`) across GameNode accounts, with backups under `<data_dir>/private/tool-backups/` and archives under `<data_dir>/tools/cache/`.

#### Scenario: Restart with an installed game tool
- **WHEN** OptiScaler is installed on a game and the application restarts
- **THEN** the game's installed tool version, channel, target executable, proxy DLL filename, installed file digests, and backed-up original file paths are restored from SQLite before any network request completes

#### Scenario: Switch GameNode account with an installed tool
- **WHEN** one GameNode account signs out and another signs in on the same PC
- **THEN** the installed tool state and `Uninstall` capability remain intact for that game installation

#### Scenario: Reconcile interrupted tool operation on startup
- **WHEN** the application starts and finds a tool installation record left in a transient `Installing`, `Updating`, or `Uninstalling` state due to a prior crash
- **THEN** startup recovery rolls back the incomplete operation using the recorded backup manifest so the game folder returns to a clean, known state

### Requirement: Upgrade preserves existing desktop data
The new schema SHALL be applied through ordered transactional migrations that preserve Steam entries, catalog facts, sync results, settings, matches and playtime. An older application SHALL refuse the newer schema without modifying it. Application uninstall SHALL preserve backups and user data under the existing keep-data policy.

#### Scenario: Upgrade from playtime release
- **WHEN** an existing step 3 database is upgraded
- **THEN** all previous Steam and account data remains and the manual registrations, watches and backup history start empty

#### Scenario: Upgrade adds catalog state
- **WHEN** an existing step 5 database is upgraded
- **THEN** installed entries remain visible, owned and family-shared facts start false, catalog sync state starts empty, and matches, covers, settings and playtime are preserved

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
