# Spec Delta

## ADDED Requirements

### Requirement: Custom launch is an explicitly reviewed immutable plan
The host MUST resolve custom launch requests into immutable typed plans and bind remembered approval to the exact effective target, arguments, working directory, environment changes, component identities and relevant revisions.

#### Scenario: A recipe changes after approval
- **WHEN** a launch request differs from the approved plan or a bound resource/grant revision changes
- **THEN** the old approval is not reused and the host requires a new decision

#### Scenario: The unchanged approved plan is played again
- **WHEN** the user selects Play for the same recipe and the canonical plan, package, source, target, resource and grant revisions are unchanged
- **THEN** the host may reuse the remembered approval without repeating the review

#### Scenario: A selected recipe is removed or revoked
- **WHEN** the selected recipe is missing, disabled, revised or no longer authorized
- **THEN** the host refuses to dispatch it, keeps it visibly unavailable, and offers an explicit choice of another recipe or the detected default

### Requirement: Per-game launch choices are explicit
The host MUST keep one active launch recipe per library entry and profile. No active custom recipe means the detected default is selected; returning to that default requires an explicit user choice. User-initiated launches MUST use the shared host review before dispatch.

#### Scenario: The user selects a custom recipe
- **WHEN** the user selects an available recipe for a matched game
- **THEN** the host binds the selection to its authenticated plugin identity and exact recipe and target revisions, then routes Play through the shared launch-plan review

#### Scenario: The selected custom recipe becomes unavailable
- **WHEN** Play is requested while the selected recipe or its base target has changed or been revoked
- **THEN** the host does not silently launch the detected default and instead presents an actionable choice to repair the recipe or explicitly return to the default

### Requirement: Runtime resources preserve game identity
The host MUST bind runtime and content resources to existing game and installation identities without duplicating game history, saves or ownership.

#### Scenario: One content supports multiple runtimes
- **WHEN** multiple compatible runtimes are registered for one content item
- **THEN** the library retains one game identity and presents selectable runtime bindings

### Requirement: Runtime reports are scoped evidence
The host MUST validate plugin runtime-session reports against the authenticated plugin, game/runtime grants, session epoch and freshness policy before Core uses them for attribution.

#### Scenario: A report is stale or revoked
- **WHEN** a runtime report arrives after its session or grant is fenced
- **THEN** Core rejects it without changing playtime or session attribution

### Requirement: Provider-free extensions use their own supervised session lifecycle
The host MUST run an activated extension as a supervised process bound to its authenticated plugin identity, active account profile and user-selected target-game context. An extension MUST NOT receive a provider id or provider-owned service context. The host MUST fence the session and stop its process tree when activation, profile, target context or required grants become invalid, and MUST apply bounded readiness and package-integrity checks before committing an activation or update.

#### Scenario: Extension starts for a selected game
- **WHEN** the user enables a compatible extension with a valid target-game context and current grants
- **THEN** the host opens an extension-kind session for only that target and the extension cannot invoke provider-only operations

#### Scenario: Extension context is unavailable or revoked
- **WHEN** the target context is missing/foreign, the account changes, or a required grant is revoked
- **THEN** activation fails closed or the active session is fenced and its process tree is stopped without falling back to provider identity

#### Scenario: A running emulator switches games without restarting
- **WHEN** one runtime process changes from game A to game B and the host validates B against its own current authorized context
- **THEN** the host closes A attribution and starts B attribution at the confirmed boundary without requiring a new process; unauthorized or uncertain intervals remain unknown

#### Scenario: Runtime reports stop arriving
- **WHEN** no fresh authorized report arrives within the initial 30-second host timeout
- **THEN** the host expires the evidence and does not attribute the resulting unknown gap to a game
