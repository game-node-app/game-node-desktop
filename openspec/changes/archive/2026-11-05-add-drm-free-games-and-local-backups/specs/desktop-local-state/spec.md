# Spec Delta

## ADDED Requirements

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

### Requirement: Upgrade preserves existing desktop data
The new schema SHALL be applied through ordered transactional migrations that preserve Steam entries, settings, matches and playtime. An older application SHALL refuse the newer schema without modifying it. Application uninstall SHALL preserve backups and user data under the existing keep-data policy.

#### Scenario: Upgrade from playtime release
- **WHEN** an existing step 3 database is upgraded
- **THEN** all previous Steam and account data remains and the manual registrations, watches and backup history start empty
