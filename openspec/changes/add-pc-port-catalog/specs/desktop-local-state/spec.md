# Spec Delta

## ADDED Requirements

### Requirement: Persist the port catalog cache and overlay
Local storage SHALL keep the last complete port catalog snapshot, the time it was fetched, the newest valid overlay and its version through a new ordered migration. The catalog and overlay SHALL belong to the Windows user, be shared across GameNode accounts, and contain no credentials. A snapshot SHALL be replaced in one transaction.

#### Scenario: Restart offline
- **WHEN** the application restarts without network access after a successful catalog fetch
- **THEN** the cached catalog and overlay are loaded from local storage before any request completes

#### Scenario: Another account
- **WHEN** a different GameNode account signs in under the same Windows user
- **THEN** it sees the same cached catalog and the same port installations

### Requirement: Persist port install jobs and provenance
Local storage SHALL keep each port install job with its catalog id, release, install folder, phase, the paths it created and its result, and SHALL link a completed job to its DRM-free registration. Every created path SHALL be recorded before it is written, so startup recovery can remove partial work. Completed jobs SHALL keep their provenance for as long as the registration exists.

#### Scenario: Crash during an install
- **WHEN** GameNode stops while a job is extracting
- **THEN** on restart the recorded job and its created paths let recovery remove the partial folder and mark the job interrupted

#### Scenario: Registration committed before the crash
- **WHEN** GameNode stops after a job's registration was committed but before the job was marked complete
- **THEN** recovery marks the job complete and keeps the entry and its files

### Requirement: Persist port catalog preferences
Local storage SHALL keep the PC ports view mode, its filters (not the search text) and the last chosen install parent folder across restarts.

#### Scenario: Restart with preferences
- **WHEN** the user chooses list view, enables the guided setup filter, installs into `D:\Ports` and restarts
- **THEN** PC ports opens in list view with the filter enabled and offers `D:\Ports` for the next install

### Requirement: Upgrade preserves data when adding port state
The port tables SHALL be added by a transactional migration that preserves every existing table and value. After the upgrade, the catalog cache, install jobs and preferences SHALL start empty. An older application SHALL refuse the upgraded schema without modifying it.

#### Scenario: Upgrade an existing database
- **WHEN** a database at the favorites schema is upgraded
- **THEN** library entries, manual registrations, tools, backups, favorites, matches and playtime are unchanged and the port state is empty
