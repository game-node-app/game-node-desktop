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

### Requirement: Linux custom launch preserves the reviewed plan
The Linux host MUST execute supported direct-executable and wrapper-based custom plans with their reviewed executable, arguments, working directory and environment changes. It MUST preserve the original GameNode game and installation identity, revalidate approved resources before dispatch, and fail explicitly when an operation or wrapper form is unsupported. It MUST NOT silently fall back to the detected default or omit requested plan fields.

#### Scenario: A direct Linux executable is selected
- **WHEN** a user approves a custom plan whose effective target is an approved Linux executable
- **THEN** the Linux adapter starts that executable with the reviewed arguments, working directory and environment delta under the original game identity

#### Scenario: A Proton-style wrapper is selected
- **WHEN** a user approves a custom plan whose approved wrapper launches a Windows game executable through Proton
- **THEN** the Linux adapter passes the reviewed game executable and arguments using the selected wrapper invocation form and retains the original game identity

#### Scenario: The Linux adapter cannot honor a reviewed field
- **WHEN** the wrapper is missing, its invocation form is unsupported, or a requested environment operation cannot be applied
- **THEN** the host returns an explicit unsupported/failed outcome and does not dispatch the detected default or a partial plan

### Requirement: Custom launch process trees retain host ownership
The host MUST transfer each custom-launch child to a process supervisor and track its descendants. Any attached shared-resource use lease MUST remain held until the complete tree is confirmed exited or explicitly terminated and fenced. Unknown tree state MUST remain busy.

#### Scenario: A wrapper exits while its game process remains active
- **WHEN** a custom runtime wrapper exits but a descendant remains in the supervised launch tree
- **THEN** the host retains process-tree ownership and the shared-resource use lease, and backup/restore remains blocked

#### Scenario: The user explicitly stops a custom launch
- **WHEN** the user requests stop for a supervised launch
- **THEN** the host terminates and fences the complete process tree, waits until exit is confirmed, and only then releases the use lease

#### Scenario: The process tree cannot be inspected
- **WHEN** the supervisor cannot determine whether a process or descendant still uses the resource
- **THEN** it reports the resource busy/unknown and does not release the lease or permit an unsafe backup/restore

### Requirement: Custom launch and exclusive resource operations are serialized
The host MUST acquire a shared-resource use lease before spawning a launch that uses that resource. An exclusive backup/restore lease MUST block the launch. Revoking launch permission MUST block new launches without terminating an already-running game under the current launch policy.

#### Scenario: An exclusive backup or restore is active
- **WHEN** a custom launch requires a resource whose exclusive backup/restore lease is held
- **THEN** the host refuses to spawn the child until the exclusive operation commits or aborts

#### Scenario: Permission is revoked while a custom launch is running
- **WHEN** launch permission is revoked after the process tree has started
- **THEN** the host denies new launches but leaves the existing game running; its use lease remains until the tree exits or is explicitly stopped

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
The host MUST validate plugin runtime-session reports against authenticated identity, current game/runtime grants, session epoch, target binding and timestamp policy before Core uses them for attribution. `observed_at` is Unix time in seconds, accepted no more than 30 seconds stale or 5 seconds in the future; expiry uses host monotonic receipt time.

#### Scenario: A runtime changes to another game in the same process
- **WHEN** an extension requests reporting context for another game or installation on an instance with a fresh active report
- **THEN** the host validates the active plugin identity, reporter session, profile epoch, current `game.sessions.report` grant and current runtime/content binding, obtains host confirmation for the new target, and returns a context id bound to those revisions and that runtime instance; a plugin-supplied target context or token is never authority

#### Scenario: A host-issued runtime context is reused
- **WHEN** a report for the new game includes the context id returned by the host
- **THEN** the host rechecks the context id, instance, target, profile, grant and binding before Core closes the old interval at its last accepted observation and starts the new one at the approved report's event time; a forged, expired, revoked or cross-instance context is refused

#### Scenario: A runtime context request is denied or becomes stale
- **WHEN** the user declines, the profile or grant changes, or the target binding changes before a report is accepted
- **THEN** the host issues no usable context, records no playtime for the new target, and treats the interval after the last fresh report as unknown

#### Scenario: A runtime instance id is reused after PID reuse
- **WHEN** an adapter observes that an operating-system PID now belongs to another runtime process
- **THEN** it starts a new runtime instance id and Core never merges the new process with the prior instance or session

#### Scenario: A report is stale or revoked
- **WHEN** a runtime report arrives after its session or grant is fenced
- **THEN** Core rejects it without changing playtime or session attribution

#### Scenario: Runtime observations stop arriving
- **WHEN** no authorized report is received for 30 seconds
- **THEN** Core closes the interval at its last host monotonic receipt and leaves the later time unattributed until a fresh authorized report arrives

#### Scenario: A context token expires after reports stop
- **WHEN** no report using a host-issued context id is accepted for 30 seconds
- **THEN** the token expires, the runtime instance is no longer eligible to request another game context, and new reporting requires fresh active evidence and user confirmation

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
