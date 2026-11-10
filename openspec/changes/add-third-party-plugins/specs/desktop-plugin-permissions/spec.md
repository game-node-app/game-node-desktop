# Spec Delta

## Purpose

Give users enforceable control over third-party access to host APIs and accurately disclose the limits of native executable permissions.

## ADDED Requirements

### Requirement: Explicit persistent scoped grants
The host SHALL deny third-party data permissions by default and persist each user's permission and scope decisions independently. Effective grants SHALL be limited by manifest requests, host support, user decisions and current resource/session/profile scope. A plugin SHALL NOT grant itself permissions.

#### Scenario: First activation
- **WHEN** a plugin requests required and optional permissions
- **THEN** the user can review each scope, required permissions gate dependent work and denied optional permissions remain denied

#### Scenario: Save fails
- **WHEN** persisting a permission toggle fails
- **THEN** the previous saved grant remains effective and the UI restores its actual state with an error

### Requirement: Stable permission catalog
Every public data method or event SHALL declare its permission, supported API version, resource scope and read/write/execute effects. Read, write and delete permissions SHALL be independent. Unknown required permissions SHALL prevent activation; unknown optional permissions SHALL be reported unsupported and never granted.

#### Scenario: Future unknown permission
- **WHEN** an older host sees an unknown optional permission in an otherwise compatible package
- **THEN** it reports that permission unsupported and permits only the remaining approved features

#### Scenario: Read without write
- **WHEN** a plugin has storage.read but not storage.write
- **THEN** its scoped read can succeed and its write is explicitly denied without modification

### Requirement: Explicit machine-readable refusal
Unauthorized calls SHALL return PERMISSION_DENIED with a stable reason, permission identifier and grant revision. Authentication, stale context, unsupported features and invalid input SHALL remain distinguishable. Rejected stream submissions SHALL receive a correlated failure without committing their data or reporting successful empty results.

#### Scenario: Permission withheld
- **WHEN** a plugin calls a method without its required grant
- **THEN** it receives a not_granted denial naming the permission and can present an actionable error without parsing prose

#### Scenario: Stream event denied
- **WHEN** a plugin submits catalog data without library.catalog
- **THEN** the operation fails explicitly and the previous successful catalog facts remain unchanged

### Requirement: Revocation fences pending work
Permission changes SHALL increment the effective grant revision and notify active plugins. The host SHALL recheck authorization before effects commit, cancel pending work affected by revocation and deny later calls explicitly. Already committed effects SHALL NOT be represented as undone by revocation.

#### Scenario: Permission revoked while confirmation is open
- **WHEN** the user revokes an install permission while an install request awaits confirmation
- **THEN** accepting that old confirmation cannot start the install and the plugin receives a revoked denial

#### Scenario: Permission revoked after a write
- **WHEN** a permitted write has already committed before revocation
- **THEN** its success remains accurate, subsequent writes are refused and no automatic undo is claimed

### Requirement: Plugin and profile isolation
The host SHALL isolate third-party credentials, private storage, jobs and provider results by validated identity and current profile. Plugins SHALL NOT access Core credentials, other plugins' secrets or another provider's mutable records. Broader library reads SHALL require an explicit separate grant.

#### Scenario: Foreign secret slot
- **WHEN** a plugin requests another plugin's secret or a GameNode/Steam credential alias
- **THEN** the host rejects the request without disclosing whether the value exists

#### Scenario: Profile changes
- **WHEN** a profile changes while a plugin holds a scoped operation context
- **THEN** stale requests cannot mutate the new profile

### Requirement: Honest native-code disclosure
Before activation the application SHALL explain that third-party plugins are independent unsandboxed executables with the user's OS privileges. API grants SHALL be distinguished from self-declared native access. The UI SHALL state that direct OS activity is not confined by GameNode and disabling a plugin cannot undo changes or guarantee termination of escaped processes.

#### Scenario: Filesystem API permission denied
- **WHEN** the user denies files.write to a plugin declaring native filesystem access
- **THEN** the UI explains that host-mediated writes are denied but the executable may still write through direct OS access

### Requirement: Resource effects and confirmations
Filesystem grants SHALL name user-approved roots; network grants SHALL name exact origins; data grants SHALL name their resource scope. Host-mediated launch SHALL require approved targets and explicit user intent; unchanged recipes selected for Play SHALL not need repeated confirmation. Install, tool modification and restore requests SHALL retain host previews, validations and user confirmation regardless of plugin grants.

#### Scenario: Wider scope requested by update
- **WHEN** an updated plugin requests another filesystem root or network origin
- **THEN** the new scope is denied until the user approves it explicitly

#### Scenario: Destructive request is granted
- **WHEN** a plugin with backups.restore requests a valid restore
- **THEN** the host presents the affected game/files and requires user confirmation before replacement

### Requirement: Separate standard and custom launch grants
The host SHALL expose independent game.launch and game.custom_launch permissions with per-game scope. Standard launch SHALL not permit parameter changes. Custom launch SHALL expose approved runtime/executable, arguments, working directory and environment effects. Neither grant SHALL authorize provider impersonation, broad library reads, runtime installation or Core credential access.

#### Scenario: Only standard launch granted
- **WHEN** a plugin with game.launch but without game.custom_launch proposes a runtime or environment override
- **THEN** it receives an explicit game.custom_launch denial and no custom recipe is executed

#### Scenario: Only custom launch granted
- **WHEN** a plugin holds game.custom_launch and the user approves its recipe for one game
- **THEN** that recipe can be launched through user intent without granting unrelated standard-launch calls or access to other games

#### Scenario: Custom process environment
- **WHEN** the host starts an approved custom game/runtime process
- **THEN** it applies validated approved environment overrides without exposing host/plugin session credentials

### Requirement: Explicit per-plugin per-game filesystem scope
Every third-party permission SHALL start disabled at installation. File read/write/delete grants SHALL bind one authenticated plugin to explicitly approved game/resource roots and operations; no global access SHALL be implied. Shared files affecting several games SHALL require explicit approval of their full affected scope.

#### Scenario: One game authorized
- **WHEN** the user grants one plugin read/write access for game A only
- **THEN** the API allows those operations inside approved A resources and denies game B or broader parent directories

#### Scenario: Shared emulator memory card
- **WHEN** writing one selected resource would affect several games
- **THEN** the full shared scope must be reviewed and authorized before the host permits the write

### Requirement: Default-deny field-level sharing
Data read and subscription capabilities SHALL not grant automatic access to another source. Users SHALL explicitly select source/destination identity and version, dataset/game/resources, fields, delivery, history and retention. Newly added fields and changed package versions SHALL remain unshared until reviewed.

#### Scenario: Install a new consumer
- **WHEN** a new plugin requests hardware or presence data
- **THEN** all sharing is off until the user selects the source and fields and approves that recipient/version

### Requirement: Revocable recipient projections
The host SHALL enforce sharing policy on queries, subscriptions, queued delivery and persistence of received data. Revocation SHALL stop future access and invalidate affected tokens. It SHALL disclose that copies already delivered to unsandboxed processes cannot be recalled.

#### Scenario: Revoke live FPS sharing
- **WHEN** the user revokes one consumer while a performance stream is queued
- **THEN** new queries and queued delivery to that consumer are denied without stopping separately authorized consumers

### Requirement: Persistence does not imply other access
storage.persist SHALL default disabled and authorize retention/own-namespace cleanup only. It SHALL NOT imply reading another dataset, modifying game files, deleting foreign plugin data or disclosing physical managed paths. Recipient persistence SHALL also require explicit sharing-retention consent.

#### Scenario: Reused archived producer data
- **WHEN** a new plugin requests records retained after the original producer was uninstalled
- **THEN** access and persistence are denied until the user separately approves that archived source and recipient

### Requirement: API enforcement disclosure
Third-party permission UI SHALL describe restrictions as controls enforced on GameNode API calls. It SHALL NOT promise that independent native processes obey them for direct OS activity, hide their physical environment or are sandboxed. User consent SHALL identify the plugin and the scope of host authority being granted.

#### Scenario: Read-only API grant
- **WHEN** the user grants API read access to a native plugin
- **THEN** the UI does not claim that the process is unable to perform direct filesystem writes using its OS privileges
