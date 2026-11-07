# Spec Delta

## ADDED Requirements

### Requirement: Persist downloads consent and preferences
The database SHALL store the accepted consent term version and timestamp, whether Downloads is enabled, the destination folder, rate limits and seeding policy. A missing or unreadable consent SHALL be treated as not accepted, so that a corrupt value can never enable transfers.

#### Scenario: Consent survives restart
- **WHEN** the user accepts the term, restarts and opens Downloads
- **THEN** the term is not shown again and the feature is enabled

#### Scenario: Corrupt consent
- **WHEN** the stored consent cannot be read or predates the current term
- **THEN** Downloads is disabled and the consent screen is shown before any engine starts

#### Scenario: Preferences survive
- **WHEN** the user changes the destination, limits or seeding policy and restarts
- **THEN** the saved values are applied before the engine starts

### Requirement: Persist transfer associations and install jobs
The database SHALL retain the association between a transfer info hash and its library entry, plus the phase and outcome of each install job. Engine-owned transfer state SHALL remain with the engine. After a crash or restart, an interrupted install job SHALL be reconciled without automatically re-running an installer.

#### Scenario: Already associated after restart
- **WHEN** a transfer was associated with an entry and the application restarts
- **THEN** the completed transfer still shows its associated entry instead of offering another addition

#### Scenario: Crash during installation
- **WHEN** the application stops while an install job is running
- **THEN** the job is shown as interrupted on the next start, no entry is created, and the user decides whether to retry
