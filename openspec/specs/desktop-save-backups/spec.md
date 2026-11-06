# desktop-save-backups Specification

## Purpose

Protect local DRM-free game progress with automatic versioned backups and explicit, recoverable restores, using maintained save-location metadata without requiring a separate application setup.

## Requirements

### Requirement: Backup support works after installation
The Windows package SHALL include a pinned compatible backup engine and baseline save manifest. Supported backup operations SHALL work offline without a separate Ludusavi installation. Missing or unusable backup components SHALL disable backup actions with a recoverable explanation while leaving play and tracking available.

#### Scenario: First use offline
- **WHEN** the packaged application is installed without internet access and a known game is added
- **THEN** its bundled save metadata and backup engine can be used without downloading executable code

#### Scenario: Engine failure
- **WHEN** the helper is missing, incompatible or cannot start
- **THEN** save protection shows an actionable error and the game remains launchable

### Requirement: Save manifests update without losing usable data
Save metadata updates SHALL use a fixed HTTPS source, validate before activation, and retain the last usable snapshot on failure. Updates SHALL run outside launch and backup critical paths. Cached metadata SHALL remain usable offline and a failed update SHALL not clear user save-profile choices.

#### Scenario: Invalid update
- **WHEN** a manifest update is malformed or cannot be downloaded
- **THEN** the previous valid manifest remains active and existing backup support is preserved

### Requirement: Save identity is independent of catalog matching
Each installation SHALL have a save profile distinct from its GameNode match. A unique strong manifest identity SHALL enable a supported profile automatically; ambiguity SHALL require user selection. The UI SHALL distinguish supported, needs selection, unsupported and no saves found. A GameNode rematch SHALL not silently retarget existing save protection or history.

#### Scenario: Catalog unavailable
- **WHEN** the manifest confidently identifies an added game while GameNode cannot be reached
- **THEN** local save protection works independently of catalog matching

#### Scenario: Correct the GameNode match
- **WHEN** the user changes an entry's GameNode game
- **THEN** its existing save profile and backup history stay unchanged and any alternative save profile requires a separate choice

#### Scenario: Multiple editions
- **WHEN** several manifest profiles plausibly describe an installation
- **THEN** automatic backup waits for profile selection and the UI presents the alternatives

### Requirement: Save locations can be corrected
The application SHALL let the user change or disable a save profile and configure explicit save files or folders when manifest support is absent or incorrect. Such changes SHALL preview the affected locations, invalidate pending jobs using old locations, and preserve previous backups under their original profile identity.

#### Scenario: Unsupported game
- **WHEN** a game has no manifest entry and the user selects its save folder
- **THEN** the application can protect that folder without requiring a GameNode match or edits to YAML

### Requirement: Local save protection has independent controls
Automatic local backup SHALL default to enabled for supported, unambiguous DRM-free profiles and SHALL be explained in game details. Settings SHALL expose the local backup destination and global automatic-backup control, with a per-game override. These controls SHALL be independent of Sync with GameNode. The default destination SHALL be under GameNode's local application-data folder.

#### Scenario: Sync disabled
- **WHEN** Sync with GameNode is off and a supported game exits
- **THEN** automatic local backup can still run and no save data is uploaded

#### Scenario: Backup disabled
- **WHEN** the user disables automatic backup globally or for one game
- **THEN** no new automatic job is run for affected games and existing backups remain available

### Requirement: Real game exits trigger automatic backup
After an observed DRM-free game finishes, the application SHALL schedule one backup after a short settling interval and recheck that it is stopped. External launches SHALL receive the same protection. Checkpoints, session splitting, source changes, sign-out and application shutdown SHALL not masquerade as game exits. Failed or unobserved game starts SHALL not trigger backup.

#### Scenario: Desktop shortcut launch
- **WHEN** an added game started from a desktop shortcut is observed running and then exits
- **THEN** a backup is scheduled even if the GameNode window was never shown

#### Scenario: Immediate restart
- **WHEN** the game restarts before its queued backup begins
- **THEN** that backup is deferred until the next stable stopped state

#### Scenario: Session split
- **WHEN** a running game's playtime session is split at 24 hours
- **THEN** no backup starts while the game remains running

### Requirement: Backup jobs are durable and report accurate outcomes
Backup work SHALL be serialized, cancellable and bounded, with retries for transient failures. A restart SHALL reconcile interrupted jobs and on-disk history without claiming success or creating duplicate snapshots for completed work. The UI SHALL show queued, running, unchanged, successful, partial and failed outcomes. A timeout, malformed output or per-file failure SHALL not be reported as full success.

#### Scenario: Disk full
- **WHEN** backup fails because its destination is full or unavailable
- **THEN** the previous usable history remains available and the UI offers retry after the problem is fixed

#### Scenario: Application interrupted
- **WHEN** GameNode stops during a backup and restarts
- **THEN** it reconciles that job with stored backups and retries only once the game is known to be stopped

### Requirement: Backups retain useful version history
The application SHALL offer Backup now when stopped and a history with timestamps, result and size. It SHALL retain ten successful changed snapshots per installation/profile by default, avoid new versions for unchanged data, and keep histories separate for distinct installations. Failed backups SHALL not cause retention pruning of usable snapshots. Removing an entry SHALL not delete its archives.

#### Scenario: Unchanged save
- **WHEN** Backup now runs and the protected data has not changed
- **THEN** history reports unchanged without adding a redundant version

#### Scenario: Successful eleventh version
- **WHEN** an eleventh changed automatic backup succeeds
- **THEN** the newest ten ordinary successful versions remain, with protected recovery snapshots retained separately

### Requirement: Restore is explicit and recoverable
Restore SHALL require the user to select a backup, preview its target files and registry keys, and confirm replacement. It SHALL require the game to be stopped, create and verify a recovery backup of existing affected data, and abort if that recovery step fails. Automatic restore SHALL not occur on launch. Restore SHALL preserve unrelated files and report partial failures accurately.

#### Scenario: Restore an older save
- **WHEN** the user selects a historical snapshot and confirms its preview while the game is stopped
- **THEN** the current affected saves are backed up before replacement and the recovery snapshot remains available

#### Scenario: Recovery backup fails
- **WHEN** current saves cannot be preserved before restore
- **THEN** restore does not begin and existing saves are left untouched

#### Scenario: Game starts during restore preparation
- **WHEN** the game becomes running before the restore write begins
- **THEN** restore is cancelled without replacing saves

### Requirement: Backup scope is local and specific
Backup and restore SHALL use only the selected installation's save profile and destination, including applicable Windows registry save data. They SHALL never request all games implicitly, enable cloud synchronization, alter a standalone Ludusavi configuration, or derive executable commands from manifest content. The destination SHALL not overlap live save sources or watched game installations.

#### Scenario: Registry saves
- **WHEN** a selected profile includes a Windows registry save key
- **THEN** its data is included in backup and explicit restore without a manual export

#### Scenario: Unsafe destination
- **WHEN** a chosen backup destination overlaps a protected save source or installation
- **THEN** it is refused with a localized explanation before copying data

### Requirement: Local manifests are not implicit authority
A manifest found inside a game folder SHALL not be executed or automatically trusted to change backup/restore scope. Locally supplied save definitions SHALL require explicit location review before activation. Unknown manifest fields SHALL not break otherwise valid supported entries.

#### Scenario: Unexpected secondary manifest
- **WHEN** discovery finds `.ludusavi.yaml` inside a game installation
- **THEN** it cannot silently expand the selected save profile's backup or restore locations
