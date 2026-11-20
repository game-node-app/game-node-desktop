# Spec Delta

## MODIFIED Requirements

### Requirement: Backups retain useful version history
The application SHALL offer Backup now when stopped and a history with timestamps, result and size. It SHALL retain a user-chosen number of successful changed snapshots per installation/profile (5, 10, 20 or 50; ten by default), avoid new versions for unchanged data, and keep histories separate for distinct installations. Failed backups SHALL not cause retention pruning of usable snapshots. Removing an entry SHALL not delete its archives.

#### Scenario: Unchanged save
- **WHEN** Backup now runs and the protected data has not changed
- **THEN** history reports unchanged without adding a redundant version

#### Scenario: Successful eleventh version
- **WHEN** an eleventh changed automatic backup succeeds with the default retention
- **THEN** the newest ten ordinary successful versions remain, with protected recovery snapshots retained separately

#### Scenario: Lower the retention
- **WHEN** the user changes retention from 20 to 5 for a game with twelve ordinary versions
- **THEN** nothing is deleted immediately, and the next successful changed backup keeps the newest five ordinary versions and every recovery snapshot

#### Scenario: Retention is per game
- **WHEN** the user sets retention to 50 for one game
- **THEN** other games keep their own retention

## ADDED Requirements

### Requirement: Each backup version says why it exists
Each version in a game's backup history SHALL show when it was made and why: after a play session with that session's length, manual (Backup now), or recovery (made before a restore). A version whose origin the application did not record SHALL show only its time and size.

#### Scenario: After a play session
- **WHEN** an automatic backup runs after a 42-minute session and succeeds
- **THEN** its history row reads After a play session · 42 min with its time and size

#### Scenario: Recovery backup
- **WHEN** a restore creates a recovery backup of the current saves
- **THEN** that version is listed as Recovery backup with a distinct icon

#### Scenario: Unknown origin
- **WHEN** the history lists a version created before this information was recorded
- **THEN** the row shows its time and size and no reason

### Requirement: Backup history shows the space it uses
The save protection dialog SHALL show the total size of the game's listed backup versions, recovery snapshots included, and update it after each backup, restore or pruning.

#### Scenario: Space used
- **WHEN** a game's history lists twelve versions totalling 84 MB
- **THEN** the dialog shows 84 MB used in 12 copies
