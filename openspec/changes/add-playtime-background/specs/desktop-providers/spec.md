# Spec Delta

## ADDED Requirements

### Requirement: Manifest playtime counter
A manifest MAY declare `playtime = { counter = "<name>" }`, naming a store counter built into the application. A provider with a counter SHALL get its playtime from that counter, and a provider without one SHALL get measured sessions. A counter that cannot read a game's total SHALL leave that game's total unchanged, not set it to zero.

#### Scenario: Provider without a counter
- **WHEN** a manifest has no `playtime` field
- **THEN** its games' play is recorded as measured sessions

#### Scenario: Counter cannot read a game
- **WHEN** the counter's file is missing, unreadable or has no entry for a game
- **THEN** no total is submitted for that game and the failure is logged without file contents

### Requirement: Steam playtime counter
When the Steam manifest declares the `steam` counter, the application SHALL read each Steam account's per-game total and last-played time from that account's `localconfig.vdf` under every scanned Steam root. A game's total SHALL come from the Steam account that played it most recently. Units and fields SHALL be the ones recorded in the Steam counter spike notes in `docs/`.

#### Scenario: One Steam account
- **WHEN** one Steam account on the PC has 754 minutes recorded for app `620`
- **THEN** the total for `steam:620` is 45240 seconds, with that account's last-played time

#### Scenario: Two Steam accounts played the same game
- **WHEN** two Steam accounts on the PC have totals for app `620`
- **THEN** the total from the account with the later last-played time is used

#### Scenario: Malformed config
- **WHEN** one account's `localconfig.vdf` is malformed
- **THEN** that account is skipped and logged, and the other accounts' totals are still read

## MODIFIED Requirements

### Requirement: Manifest validation isolates failures
The application SHALL validate every manifest when it loads. Validation SHALL reject unknown fields, missing required fields, an unknown native scanner, an unknown playtime counter, a duplicate provider id, malformed placeholders, and launch or install templates that are not URIs. An invalid manifest SHALL be skipped and logged, and the other manifests SHALL still load.

#### Scenario: One manifest is invalid
- **WHEN** one bundled manifest fails validation
- **THEN** it is skipped with a diagnostic naming the provider and the problem
- **AND** every valid provider still scans and launches

#### Scenario: Unknown native scanner
- **WHEN** a manifest names a native scanner the application does not have
- **THEN** that manifest is rejected and no scan runs for it

#### Scenario: Unknown playtime counter
- **WHEN** a manifest names a playtime counter the application does not have
- **THEN** that manifest is rejected with a diagnostic naming the counter
