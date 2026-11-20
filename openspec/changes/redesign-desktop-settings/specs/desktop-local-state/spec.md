# Spec Delta

## ADDED Requirements

### Requirement: Persist application preferences
The local database SHALL keep, per PC and independent of the signed-in account, the Settings preferences added for startup (start with Windows, start minimized, close to tray), interface language, status bar, library start scope, discovery (check at startup, real-time watching), tools (automatic update checks, default channel) and each store's shared-games inclusion. Cover size SHALL be kept with the library layout. Each preference SHALL have a default and SHALL load before the window opens.

#### Scenario: Preferences survive restart
- **WHEN** the user turns close-to-tray off, chooses English and Large covers, and restarts
- **THEN** closing the window quits, the interface is in English and the grid uses Large covers from the first frame

#### Scenario: Upgrade from a build without these preferences
- **WHEN** an existing database without these preferences is opened
- **THEN** every preference takes its default (autostart on, start minimized on, close to tray on, System language, status bar on, Last view, discovery at startup on, real-time watching on, automatic tool checks on, Stable channel, shared games included) and existing data is unchanged

#### Scenario: Unreadable preference value
- **WHEN** a stored preference value cannot be parsed
- **THEN** that preference uses its default and the others keep their stored values

#### Scenario: Sign-out keeps PC preferences
- **WHEN** the user signs out of GameNode and signs in with another account
- **THEN** the application preferences are unchanged

### Requirement: Persist the last sync result
The local database SHALL keep, per account, the time of the last completed playtime submission pass and its counts (games with an accepted total, stores that contributed, totals still held), so Settings can show the last sync before the next pass runs. It SHALL NOT contain any credential or server response body.

#### Scenario: Last sync after restart
- **WHEN** a submission pass completed and the application restarts
- **THEN** Settings shows that pass's time and counts before any new pass

#### Scenario: Another account
- **WHEN** a different GameNode account signs in on the same PC
- **THEN** Settings shows "Never synced" until a pass completes for that account
