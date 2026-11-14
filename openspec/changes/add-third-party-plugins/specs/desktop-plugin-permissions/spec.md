# Spec Delta

## Purpose

Give users enforceable control over third-party access to host APIs and accurately disclose the limits of native executable permissions.

## ADDED Requirements

### Requirement: Explicit persistent scoped grants
The host SHALL deny third-party data permissions by default and persist each GameNode account's permission and scope decisions independently. Effective grants SHALL be limited by manifest requests, host support, user decisions and current resource/session/profile scope. A plugin SHALL NOT grant itself permissions.

#### Scenario: First activation
- **WHEN** a plugin requests required and optional permissions
- **THEN** the user can review each permission by data category and scope, grant or deny it during activation, understand that a grant permits but does not trigger common-data queries, required permissions gate dependent work and denied optional permissions remain denied

#### Scenario: Save fails
- **WHEN** persisting a permission toggle fails
- **THEN** the previous saved grant remains effective and the UI restores its actual state with an error

### Requirement: Stable permission catalog
Every public method/event SHALL declare its permission, API version, scope and effects. Runtime, library-root and save-profile create, update and delete grants are independent. `storage.persist` permits cleanup only in the caller's private namespace. Unknown required permissions block activation; unknown optional permissions remain unsupported and denied.

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
The host SHALL scope third-party resources to authenticated plugin, pinned publisher and active profile identity. Ownership SHALL come from the host context, and stale profile contexts SHALL NOT access a new profile.

#### Scenario: Private storage and job ownership
- **WHEN** a plugin supplies a foreign plugin/profile owner or uses another plugin's job ID
- **THEN** the host denies the request without revealing whether the namespace or job exists, and no data or job effect is exposed

#### Scenario: Profile switch fences queued work
- **WHEN** the active GameNode profile changes while a plugin operation is queued
- **THEN** the old profile context is invalidated and the operation cannot read, mutate or commit data in the new profile

### Requirement: Account-bound plugin profile
The active plugin profile SHALL be the signed-in GameNode account. The host SHALL NOT create a local, anonymous or signed-out plugin profile. During an unverified restored session, the cached last signed-in account SHALL be the active profile.

#### Scenario: Restored session awaiting verification
- **WHEN** the application starts offline with a stored credential of the last signed-in account
- **THEN** enabled plugins run with that cached account's profile and grants, consistent with the cached library

#### Scenario: Verified account differs from the cached one
- **WHEN** verification identifies a different account than the cached profile
- **THEN** the cached profile's plugin contexts are fenced and sessions stop, and the new account starts only with its own grants, activation state and data

### Requirement: No plugin profile without an account session
Without a usable account session the host SHALL issue no profile context, SHALL NOT start third-party plugin processes and SHALL report profile-scoped services as unavailable, distinct from permission denial.

#### Scenario: No signed-in account
- **WHEN** GameNode has never been signed in, or the user signed out, or the restored session was rejected
- **THEN** no third-party plugin process is started and profile-scoped requests receive an unavailable status without exposing any account's data

### Requirement: Account-scoped plugin state
Grants, activation state, configuration, secrets and private data SHALL be scoped to the account and retained for its next sign-in without automatic migration to another account. Installed packages and publisher trust SHALL remain per OS user.

#### Scenario: Data does not follow account changes
- **WHEN** another account signs in on the same OS user
- **THEN** it sees installed packages but not the previous account's grants, configuration, secrets or private data

### Requirement: Third-party credential isolation
Secret slots SHALL be dedicated to the authenticated plugin, pinned publisher, provider sub-identity and profile. Requests SHALL use logical keys; they SHALL NOT select physical aliases. Core and foreign credentials SHALL remain inaccessible.

#### Scenario: Credential alias isolation
- **WHEN** a third-party plugin requests another plugin's secret or names a Core, GameNode or official-provider credential alias
- **THEN** the host denies the request without disclosing whether the value exists

### Requirement: Private storage and job ownership
The host SHALL assign storage and job ownership from the authenticated plugin/profile context. Domain-resource grants SHALL NOT expose private namespaces or foreign jobs.

#### Scenario: Foreign storage or job selector
- **WHEN** a plugin supplies a foreign plugin/profile owner or uses another plugin's job ID
- **THEN** the host denies the request without revealing whether the namespace or job exists, and no data or job effect is exposed

### Requirement: Shared domain grants preserve private isolation
Mutating another plugin's shared domain records SHALL require explicit resource-and-operation consent and immutable creator provenance. Broader library reads SHALL require a separate grant. A domain grant SHALL NOT transfer account, credential, storage or job ownership. Foreign provider account/discovery facts SHALL NOT be mutated through domain-resource grants.

#### Scenario: Shared record grant does not transfer private ownership
- **WHEN** a plugin receives a category-wide or selected-record grant for another plugin's published domain record
- **THEN** it can access only the authorized record and operation, and cannot read the publisher's account identity, secrets, private storage or jobs

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

#### Scenario: Plugin-declared save locations
- **WHEN** a plugin declares an ordered list of default folders where a game's saves live
- **THEN** Core shows them in that order as declared by the plugin and not verified, pre-selects the first that exists, and the declarations alone grant nothing: the user confirms one or chooses another folder before any approval

#### Scenario: Verify contents before asking to write
- **WHEN** a plugin approved read-only for a folder inspects its files and then asks for write
- **THEN** the read-only approval gives no write, and Core asks again, showing what is already approved and what is added, before any write is allowed

#### Scenario: Approval requires the Core dialog
- **WHEN** a plugin asks for access to a folder
- **THEN** Core asks the user in its own dialog naming the plugin, game, folder, operations and every known affected game, and nothing is granted until the user confirms

### Requirement: Default-deny common-data category sharing
Common-data read permissions SHALL start denied and be selectable during activation. Granting one SHALL permit explicit per-operation access to all plugins' published records in that category within its declared scope, without pairwise source/recipient approval. Sensitive/custom datasets SHALL retain explicit sharing review; secrets and authentication material SHALL never be included.

#### Scenario: Grant performance history access
- **WHEN** the user grants a plugin the shared performance-history read permission during activation and the plugin requests common records in a query
- **THEN** it can query persisted performance history published by any plugin under that category, subject to the granted scope and separate persistence rules

#### Scenario: Grant does not cause automatic use
- **WHEN** a plugin has a common-data read permission but makes a query that excludes common records
- **THEN** the host returns no other plugin's common data and the grant remains available for later explicit queries

#### Scenario: Sensitive custom data remains reviewed
- **WHEN** a plugin holds a common-data category permission and requests a custom or sensitive dataset
- **THEN** the common-data permission does not grant access and the separate sharing review remains required

#### Scenario: Secret data is excluded
- **WHEN** a plugin holds any shared-data category permission
- **THEN** it cannot read credentials, cookies, tokens, secret configuration or other authentication material through that permission

### Requirement: Revocable recipient projections
The host SHALL enforce sharing policy on queries, subscriptions, queued delivery and persistence of received data. Revocation SHALL stop future access and invalidate affected tokens. It SHALL disclose that copies already delivered to unsandboxed processes cannot be recalled.

#### Scenario: Revoke live FPS sharing
- **WHEN** the user revokes one consumer while a performance stream is queued
- **THEN** new queries and queued delivery to that consumer are denied without stopping separately authorized consumers

### Requirement: Persistence does not imply other access
storage.persist SHALL default disabled and authorize retention/own-namespace cleanup only. It SHALL NOT imply reading another dataset, modifying game files, deleting foreign private data or disclosing physical managed paths. Recipient persistence SHALL remain independent of category read permission and SHALL follow the user's retention policy.

#### Scenario: Own cleanup without general delete
- **WHEN** a plugin has storage.persist but lacks storage.delete and requests authorized cleanup through Persistence
- **THEN** it can clean its own eligible persisted entries within user limits, while Storage.Delete still requires storage.delete and no foreign namespace becomes accessible

#### Scenario: Reused archived producer data
- **WHEN** a plugin is granted the applicable category read permission and requests records retained after the original producer was uninstalled
- **THEN** access follows that category permission and its resource scope, while any durable recipient copy still requires storage.persist and the configured retention policy

### Requirement: API enforcement disclosure
Third-party permission UI SHALL describe restrictions as controls enforced on GameNode API calls. It SHALL NOT promise that independent native processes obey them for direct OS activity, hide their physical environment or are sandboxed. User consent SHALL identify the plugin and the scope of host authority being granted.

#### Scenario: Read-only API grant
- **WHEN** the user grants API read access to a native plugin
- **THEN** the UI does not claim that the process is unable to perform direct filesystem writes using its OS privileges

### Requirement: Common catalog read follows category or record grants
Common domain reads SHALL require an explicit user grant at activation or in settings. A category grant covers eligible published records in scope; a selected-record grant covers only named records and needs no category grant. Query selectors identify data but confer no authority and default to private/own records. Reads do not grant mutation; secrets and private/executable content remain excluded, with provenance preserved.

#### Scenario: Game path reader
- **WHEN** a plugin has the shared game-path read permission
- **THEN** it can read game paths published by any plugin within the permission's declared scope, while private storage and executable contents remain inaccessible

#### Scenario: Selected record without category grant
- **WHEN** a plugin has read permission for one published runtime record but no category-wide runtime-catalog grant
- **THEN** a query selecting that record returns only that record, while other records in the category remain inaccessible

### Requirement: Metadata never supplies authority
Unknown metadata and extension schemas SHALL not define permissions, executable policy, ownership or persistence selectors. Private/unclassified extensions SHALL remain hidden until authorized. Alternate domain/Data query paths SHALL enforce the same projection and cannot recover withheld fields.

#### Scenario: Projection bypass attempt
- **WHEN** B requests an excluded runtime extension through the Data alias
- **THEN** the host still withholds or denies that extension

### Requirement: Launch confirmation is not policy escalation
User launch confirmation SHALL approve only the exact supported plan within existing API grants and per-game scope. It SHALL not override prohibited operations, grant writes/install permissions or promise native code confinement. Remembered approval SHALL be bound to plan/source/component revisions and remain revocable.

#### Scenario: Install permission absent
- **WHEN** a plugin has an approved modded launch plan but lacks installation/write grants
- **THEN** its request to install or modify mod files remains explicitly denied

### Requirement: Independent creator-scoped domain permissions
Runtime, library-root and save-profile read/create/update/delete grants SHALL be independent and default denied. Own-record grants cover only records created by the authenticated plugin. Create assigns host provenance; update applies only to own or authorized published records. Public grants cover only their category or selected ids. No grant implies other operations, file access, execution or foreign records. `storage.persist` permits own-private cleanup.

#### Scenario: Dolphin changes its own installation
- **WHEN** Dolphin's plugin has own-record write/delete grants and replaces its external-disk runtime registration with an SSD registration
- **THEN** it can change its own registrations but cannot modify or remove RetroArch's runtime or delete game files, saves or history

#### Scenario: Create without update or delete
- **WHEN** a plugin has game.runtime.create but lacks game.runtime.update and game.runtime.delete
- **THEN** it can register a new runtime in its authorized category but cannot change or remove an existing runtime

### Requirement: Explicit foreign domain resource grants
Users SHALL grant read/create/update/delete on published runtime, library-root and save-profile records by category or selected record. Create is category-scoped; other operations may use either scope. Grants bind recipient, operation, scope and revision; mutations check revisions and preserve creator/actor provenance. Domain deletion follows its delete grant; `storage.persist` cleanup is caller-private only. Foreign secrets and provider-owned facts remain excluded.

#### Scenario: Only selected foreign runtime authorized
- **WHEN** the user grants a migration plugin read/write on one DeSmuME runtime but not delete or access to a RetroArch runtime
- **THEN** it can perform the approved operations on that record, cannot delete it and cannot access RetroArch outside separately authorized scope

#### Scenario: Foreign grant revoked
- **WHEN** the user revokes resource access while a mutation is queued
- **THEN** the host refuses the mutation at commit and preserves both the record and original creator provenance

### Requirement: Authorized local public pool publication
The public pool SHALL be local shared data distinct from private plugin storage. A producer SHALL request publication permission and the user SHALL authorize it before records become shared. Private records SHALL NOT become public merely because another plugin holds category access. Durable publication SHALL additionally require storage.persist.

#### Scenario: Producer publication denied
- **WHEN** a producer requests publication but the user denies permission
- **THEN** its private records remain private and other plugins cannot retrieve them through public domain or Data queries

### Requirement: Granular public pool operations and scopes
Public-pool read, create, update and delete SHALL require independent user-selected permissions at installation or in settings, scoped to a category or selected records. A category grant SHALL cover eligible published records in that category; a record grant SHALL cover only selected records. Publication and persistence SHALL NOT imply consumer mutation rights.

#### Scenario: Category mutation authorization
- **WHEN** the user grants update on a published category but denies delete
- **THEN** supported revision-checked updates can affect eligible records from other producers in that category, preserve creator provenance and record the acting plugin, while deletion is denied

#### Scenario: Private namespace remains isolated
- **WHEN** a plugin has every public-pool operation for a category
- **THEN** foreign private storage, credentials and provider-owned account/discovery facts remain inaccessible to mutation through the pool

#### Scenario: Published record deletion is distinct from private cleanup
- **WHEN** a plugin has delete permission for one published runtime record but only storage.persist for its own namespace
- **THEN** it may delete that authorized public record, may clean its own persisted entries, and cannot address another plugin's private namespace
