# Spec Delta

## ADDED Requirements

### Requirement: Custom launch is an explicitly reviewed immutable plan
The host MUST resolve custom launch requests into immutable typed plans and bind remembered approval to the exact effective target, arguments, working directory, environment changes, component identities and relevant revisions.

#### Scenario: A recipe changes after approval
- **WHEN** a launch request differs from the approved plan or a bound resource/grant revision changes
- **THEN** the old approval is not reused and the host requires a new decision

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
