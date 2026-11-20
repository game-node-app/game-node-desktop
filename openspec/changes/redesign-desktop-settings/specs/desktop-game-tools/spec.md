# Spec Delta

## ADDED Requirements

### Requirement: Tool update preferences
"Check for updates automatically" (on by default) SHALL let the application refresh each tool's available releases in the background at start and at most once a day, and tell the user when a newer stable release exists for an installed tool; it SHALL NOT download or install anything. "Default channel" (Stable by default) SHALL preselect the channel when installing a tool in a game for the first time.

#### Scenario: New stable release
- **WHEN** automatic checks are on and a newer stable OptiScaler release is published than the one installed in a game
- **THEN** the game's tool view shows that an update is available, and nothing is downloaded until the user confirms

#### Scenario: Automatic checks off
- **WHEN** automatic checks are off
- **THEN** release lists are fetched only when the user opens a tool view or asks to check

#### Scenario: Default channel Nightly
- **WHEN** the default channel is Nightly and the user opens Install for a game without the tool
- **THEN** Nightly is preselected and the user can still choose Stable

### Requirement: Cached versions are listed
The application SHALL list each cached tool archive with its tool, version, channel, size, whether its SHA-256 digest was verified, and whether any game uses it, together with the cache total. Clearing the cache SHALL keep its existing guarantees for installed tools and rollback backups.

#### Scenario: List the cache
- **WHEN** the cache holds OptiScaler 0.7.4 (installed in a game), 0.7.3 and a nightly
- **THEN** the list shows three versions with their sizes, marks 0.7.4 in use and the nightly as Nightly, and the total equals the sum

#### Scenario: Unverified archive
- **WHEN** a cached archive's release did not publish a digest
- **THEN** its entry does not claim SHA-256 verification
