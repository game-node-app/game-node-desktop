# Spec Delta

## Purpose

Expose explicit provider-scoped host services that let store plugins own their integrations while GameNode keeps application authority, protected persistence and platform resource ownership.

## ADDED Requirements

### Requirement: Each provider has its own logical secret slot
The secret services SHALL map each provider's logical slot to a physical credential location derived from its provider id, unless the trusted descriptor names an existing alias. A plugin SHALL NOT choose or address another provider's slot, the GameNode session slot or an arbitrary OS key.

#### Scenario: Epic stores its refresh token
- **WHEN** the Epic plugin writes its logical secret
- **THEN** it lands in the Epic provider's own keyring entry, or its own private file on Linux, and never in Steam's slot or the GameNode session slot

#### Scenario: Steam alias preserved
- **WHEN** the Steam plugin reads its logical secret
- **THEN** the host resolves it to the existing `steam` keyring account or `steam-token` file named by Steam's descriptor alias

#### Scenario: Cross-provider secret request
- **WHEN** a plugin requests a secret key outside its own logical slot
- **THEN** the request is rejected and no credential is read

### Requirement: Login policies may approve page probes without cookies
A trusted login policy SHALL be allowed to approve only a bounded page probe at an exact origin, with no cookie names. When a policy declares no cookies, the host SHALL return no cookies, and the plugin's grant SHALL NOT include cookie reading. Probe results SHALL be treated as secret-bearing data.

#### Scenario: Authorization-code login
- **WHEN** the Epic login reaches the approved redirect page at the exact probe origin
- **THEN** the host returns only the probe's bounded JSON result to the Epic plugin and returns no cookies

#### Scenario: Probe on another origin
- **WHEN** the window is on a different origin or the navigation changed during the probe
- **THEN** no probe result is returned

#### Scenario: Cookie grant without cookie policy
- **WHEN** a descriptor grants cookie reading but its login policy declares no cookie names
- **THEN** the descriptor is rejected as invalid

### Requirement: Login windows use per-provider ephemeral profiles
Each sign-in attempt SHALL use an ephemeral profile directory scoped to its provider. The host SHALL remove that directory when the attempt ends. At startup it SHALL also remove the legacy profile directories listed in trusted descriptors.

#### Scenario: Attempt ends
- **WHEN** an Epic sign-in attempt succeeds, is cancelled or times out
- **THEN** its profile directory is removed and the next attempt starts with no stored web state

### Requirement: Counter observations may omit last played
A counter observation SHALL be able to report the last-played time as unknown. The host SHALL store unknown as unknown, and SHALL keep rejecting negative values.

#### Scenario: Total without date
- **WHEN** a plugin reports 3,600 seconds for a game and an unknown last-played time
- **THEN** the total is saved with no last-played date, and the library shows last played as unknown

#### Scenario: Negative value
- **WHEN** a plugin reports a negative last-played value
- **THEN** the observation is rejected as invalid

### Requirement: Account and catalog services are store-neutral
Account state, catalog failure categories and the generic account commands SHALL be addressed by provider id and SHALL have no store-specific variants. A provider that never reports a family category SHALL have no family status or warning shown.

#### Scenario: Provider-scoped command
- **WHEN** the GUI asks to sign out of provider `epic`
- **THEN** only the Epic account and its credential are affected

#### Scenario: No family category
- **WHEN** Epic completes an owned catalog sync and reports no family outcome
- **THEN** no family error or family timestamp is shown for Epic
