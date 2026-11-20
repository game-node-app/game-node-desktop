# Spec Delta

## ADDED Requirements

### Requirement: Sync now and last sync summary
Settings SHALL show the last completed sync: how long ago it ran, how many games have a total accepted by GameNode, how many stores contributed, and how many totals are still held or "no errors". "Sync now" SHALL rescan the library and submit every held total at once, under the same rules as automatic submission. It SHALL be unavailable while sync is off, without a verified session, or while a pass is running.

#### Scenario: Sync now
- **WHEN** sync is on, the session is verified, two Steam totals are held after an offline period, and the user activates "Sync now"
- **THEN** the library is rescanned, both totals are submitted, the button shows progress until the pass ends, and the summary updates to the new time with no pending totals

#### Scenario: Sync is off
- **WHEN** "Sync with GameNode" is off
- **THEN** the last sync row says sync is off, "Sync now" is disabled, and nothing is sent

#### Scenario: Partial failure
- **WHEN** GameNode rejects one of the held totals during "Sync now"
- **THEN** the summary shows one pending total, the accepted totals are recorded, and the rejected total is retried at the next opportunity

#### Scenario: Never synced
- **WHEN** no pass has completed for the signed-in account
- **THEN** the last sync row says "Never synced"
