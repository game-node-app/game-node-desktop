# Spec Delta

## Purpose

Expose a language-neutral, versioned and permission-scoped host API that independent plugins can implement without a mandatory SDK or internal Core dependency.

## ADDED Requirements

### Requirement: Single SDK-independent wire contract
Third-party clients SHALL be able to use the public protobuf contract and documented local transport directly. SDK clients and independent clients SHALL receive equivalent method availability, permission enforcement and results. No public service SHALL require a private SDK-only handshake.

#### Scenario: Independent client connects
- **WHEN** a compatible non-Rust client authenticates over the documented OS-local transport
- **THEN** it can exchange provider commands/results and call authorized services without a Rust bridge or the GameNode SDK

### Requirement: Existing provider operations remain available
Authorized third-party providers SHALL have documented access to session opening, discovery, command/event exchange, context, account state, private secrets, constrained login, action declarations, catalog, counters and declarative settings. Each operation SHALL preserve existing validation and operation scoping.

#### Scenario: Catalog through the event stream
- **WHEN** a provider receives a catalog command and submits validated chunks plus completion
- **THEN** the host reconciles only that provider's completed category and reports its outcome

#### Scenario: Third-party login policy
- **WHEN** an authorized plugin requests login
- **THEN** the host enforces the approved HTTPS origins and cookie names and does not accept arbitrary privileged-page scripts from the plugin

### Requirement: API and permission introspection
Authenticated enabled plugins SHALL be able to query API versions, feature availability, limits, permission metadata and their own effective grants, and subscribe to permission changes. These control methods SHALL NOT require a user-granted library/data permission or reveal another plugin's private state.

#### Scenario: No optional data grants
- **WHEN** a plugin with no data grants requests its effective permissions
- **THEN** it receives its own denied/unsupported/granted states and can explain disabled features

### Requirement: Plugin-owned configuration lifecycle
The public contract SHALL support user-initiated open/focus/close commands and correlated lifecycle results for a plugin-owned configuration window through optional ui.configure. Windows SHALL be scoped to the current plugin/session/profile, remain independent of the GameNode frontend and confer no additional data permissions.

#### Scenario: Open and focus configuration
- **WHEN** the user activates Configure twice for a plugin with an authorized external configuration mode
- **THEN** the plugin opens one window and the second action focuses that window without duplicating it or blocking provider work

#### Scenario: Window lifecycle loses scope
- **WHEN** its profile changes or ui.configure is revoked
- **THEN** the host requests closure and old window contexts cannot authorize new data operations

#### Scenario: Configuration does not respond
- **WHEN** the plugin does not acknowledge a configuration command within five seconds
- **THEN** the host reports an actionable timeout and unrelated plugin/settings work continues

### Requirement: Declarative configuration remains optional
Plugins SHALL be able to declare external windows, declarative forms, both or neither. External configuration SHALL NOT require a JSON form contract or SDK. A declarative fallback SHALL be offered only when separately declared, supported and authorized; it SHALL preserve existing ui.settings behavior.

#### Scenario: Independent client provides a custom UI
- **WHEN** a direct gRPC client declares only external configuration
- **THEN** it can configure its functionality without implementing a declarative settings form

#### Scenario: Optional external UI is unavailable
- **WHEN** external configuration is unsupported or denied but the plugin also declares an authorized declarative form
- **THEN** the host offers that form without widening any grants

### Requirement: Library and presentation services
The API SHALL provide paginated library list/get/watch, display-metadata updates, favorites list/set, matching search/assignment and artwork read/set. Writes SHALL be limited to caller-owned provider entries and validated through Core rules. Whole-library reads SHALL require a separate grant and omit credentials and raw private snapshots.

#### Scenario: Read another provider
- **WHEN** a plugin has library.read but lacks library.read.all and requests another provider's entries
- **THEN** those records are not disclosed

#### Scenario: Metadata revision conflict
- **WHEN** a plugin updates metadata using an obsolete revision
- **THEN** the host reports a conflict without overwriting the newer value

### Requirement: Scoped utility services
The API SHALL provide namespaced storage get/put/delete, user-approved filesystem list/read/write/delete, constrained HTTPS fetch and attributed notifications. Services SHALL enforce documented quotas, root/origin scope, timeouts and data bounds. Filesystem roots SHALL be selected by the user and represented by opaque handles.

#### Scenario: Filesystem escape
- **WHEN** a requested relative path escapes its granted root through traversal, symlink or junction replacement
- **THEN** the operation is refused before reading or changing the target

#### Scenario: Network redirect leaves granted origin
- **WHEN** a request redirects to an unapproved origin or a private/local destination
- **THEN** the host refuses the redirected request without forwarding credentials

#### Scenario: Storage quota exceeded
- **WHEN** a plugin writes beyond its documented namespace quota
- **THEN** the host reports resource exhaustion and preserves previously stored values

### Requirement: Host-owned execution and installation services
The API SHALL expose game launch/install requests, backups list/create/restore, tools list/apply/remove and PC ports list/detail/install. These services SHALL reuse host-approved targets and domain workflows, require user intent for execution, preserve confirmations for filesystem changes, and explicitly report unsupported platforms.

#### Scenario: Port install on Linux
- **WHEN** a plugin requests a PC port install on a host where that operation is Windows-only
- **THEN** it receives a feature-unavailable result and no job starts

#### Scenario: Plugin supplies arbitrary installer
- **WHEN** a plugin tries to replace a catalog install target with an arbitrary executable URL
- **THEN** the host rejects the request before downloading or executing it

### Requirement: Jobs and bounded asynchronous work
Long-running requests SHALL return caller-owned job identifiers with get/watch/cancel and typed terminal outcomes. Mutation retries SHALL use idempotency keys. Work SHALL remain cancellable and bounded; blocking IO, hashing, decoding and computation SHALL execute away from the UI thread.

#### Scenario: Retried install request
- **WHEN** a client retries an accepted request with the same idempotency key
- **THEN** the host returns the same operation rather than installing twice

#### Scenario: Another plugin's job
- **WHEN** a plugin requests another identity's job
- **THEN** access is denied without exposing its private progress

### Requirement: Explicit unavailable Core resources
The public API SHALL NOT expose raw SQL, Core credentials, foreign plugin secrets, elevation, arbitrary shell commands, plugin administration or unrestricted frontend execution. Unknown RPCs SHALL fail as unimplemented; unsupported features SHALL NOT be represented as successful no-ops.

#### Scenario: Arbitrary host command
- **WHEN** a client attempts to invoke an internal Tauri command through the plugin API
- **THEN** no generic dispatch route exists and the request fails

### Requirement: Third-party version negotiation
Third-party sessions SHALL negotiate the highest overlapping supported API minor within a compatible major, starting at API 1.4. Required unsupported features or no version overlap SHALL prevent provider work. Optional unsupported features SHALL produce explicit unavailable states permitting documented fallback.

#### Scenario: Supported older client
- **WHEN** an older third-party client offers a supported API minor without newer optional methods
- **THEN** the host selects that minor and the client operates with only its negotiated features

#### Scenario: Newer client with fallback
- **WHEN** a newer client supports an older host minor and only optional features are missing
- **THEN** the shared minor is selected and unavailable optional features remain disabled

#### Scenario: Incompatible major
- **WHEN** a package requires a major without a host compatibility adapter
- **THEN** it remains installed but inactive with an actionable incompatibility reason

### Requirement: Published compatibility window
The host SHALL support third-party minors within the current major for at least twelve months after their successor release and retain at least the last two released minors. It SHALL publish release dates, support floor and deprecations. This retirement policy SHALL NOT remove existing official API 1.1–1.3 behavior.

#### Scenario: Host upgrade retires a minor
- **WHEN** an installed third-party package has no overlap with the published support window
- **THEN** it is marked incompatible without deleting its data, weakening permission checks or preventing official providers from operating

### Requirement: Detected URI and executable targets
Provider discovery SHALL be able to submit typed URI or local executable targets for its own games. Executable targets SHALL include validated installation scope, arguments and working directory and require initial user approval. Registering a target SHALL NOT execute it. Existing official URI declarations SHALL retain their behavior.

#### Scenario: Disc-installed game discovered
- **WHEN** an authorized provider identifies a local game's executable and installation directory
- **THEN** GameNode can validate and register that executable for Play after target approval without requiring a store URI

### Requirement: Standard game launch permission
The game.launch permission SHALL authorize launch through the game's current detected URI or executable with its stored parameters unchanged, regardless of detection source, within approved game scope. Calls SHALL bind to the expected target revision and reject custom executable, URI, argument, working-directory or environment overrides.

#### Scenario: Detected default target
- **WHEN** a plugin with game.launch requests an approved game at its current target revision
- **THEN** the host launches the detected URI or executable unchanged through its user-intent workflow

#### Scenario: Attempted custom parameter under standard grant
- **WHEN** a game.launch request includes custom arguments or environment values
- **THEN** the host refuses it explicitly without silently accepting or discarding those overrides

### Requirement: Custom game launch permission
The independent game.custom_launch grant SHALL authorize plugin-owned per-game recipes and their approved execution, including alternate URI/executable, arguments, working directory, runtime wrappers and environment overrides. Recipes SHALL use typed target/resource handles and argument vectors rather than shell command strings. This grant SHALL NOT imply installation or normal-launch permission.

#### Scenario: Runtime wrapper and environment
- **WHEN** the user approves a recipe selecting an installed Proton runtime for an identified game executable with arguments and environment settings
- **THEN** the host can execute that recipe with the approved runtime and values while preserving the game's identity

#### Scenario: Environment is unsupported by target
- **WHEN** a custom request requires environment changes through a URI handler that cannot guarantee them
- **THEN** the host reports unsupported target semantics and does not silently launch without the requested changes

### Requirement: Launch extensions across providers
A plugin SHALL be able to operate as a launch extension without owning a game provider. User-approved target-game contexts SHALL permit standard/custom launching across official, third-party and manual sources. Such access SHALL NOT permit provider impersonation or mutation of another provider's metadata, credentials or discovery facts.

#### Scenario: Extension customizes a Steam game
- **WHEN** an approved extension proposes a custom launch recipe for a selected Steam game
- **THEN** the host stores a plugin-owned launch overlay without modifying the official Steam plugin, launch manifest or account state

#### Scenario: No full-library read grant
- **WHEN** the user selects one game for a launch extension without library.read.all
- **THEN** the extension receives only the approved target context and cannot enumerate other games

### Requirement: Launch approval and revision validation
Launch recipes SHALL bind approval to game/installation/profile, target and recipe revisions, executable/runtime resources and effective parameters. Unchanged approved recipes SHALL run through Play without a repeated confirmation. Plugin-initiated execution SHALL require a host-confirmed user intent. Paths, grants and approval SHALL be revalidated before spawn.

#### Scenario: Repeated user launch
- **WHEN** the user clicks Play with an unchanged approved selected recipe
- **THEN** the host executes it without a second permission prompt and associates process tracking with the original game

#### Scenario: Target or recipe changes
- **WHEN** a selected runtime, executable, arguments, environment, working directory or base-target revision changes
- **THEN** the affected recipe requires renewed approval before execution

#### Scenario: Runtime disappears
- **WHEN** the approved runtime or executable is missing or escapes its approved scope
- **THEN** the host reports an actionable missing/invalid target and does not silently substitute another launch route

#### Scenario: Plugin is revoked while game runs
- **WHEN** the user revokes custom-launch permission after a game has started
- **THEN** later recipe launches are denied and the running game is not killed solely by that revocation

### Requirement: Operational data plane without mandatory Core consumers
The host SHALL provide working publication, validation, authorized storage, query and inter-plugin subscription APIs for the defined data families. Collection, benchmark/SMART tools, hardware scans, performance capture and predictions SHALL remain plugin responsibilities. Core dashboards and online profile publication SHALL NOT be prerequisites for using these contracts.

#### Scenario: Publish without a Core dashboard
- **WHEN** a permitted plugin publishes a supported hardware, diagnostic or game-performance record
- **THEN** the record is available to its authorized consumers even if Core has no presentation or prediction feature for it

### Requirement: Multiple libraries and roots
Libraries SHALL support create/update/remove, multiple approved roots, individual watch flags and list/get/watch. Existing monitored roots SHALL be preserved. Removing a library/root SHALL stop its monitoring without deleting game files, saves or history; overlapping/aliased roots SHALL not duplicate game observations.

#### Scenario: Several game folders
- **WHEN** a plugin proposes a library with three user-approved directories
- **THEN** each root can be monitored independently and an unavailable external root does not disable the others

### Requirement: Save profile configuration
The API SHALL support save-profile CRUD for approved games with scoped files/directories, include/exclude rules and save-context identity. Independent operation grants SHALL cover own records or published foreign records in authorized category/record scopes. Scope changes SHALL require host review, preserve history and invalidate obsolete jobs. Configuration SHALL NOT authorize file reads or restore.

#### Scenario: Emulator save context
- **WHEN** a plugin proposes save files for one emulated game and emulator user
- **THEN** the user reviews the actual locations before activation and unrelated profiles/history remain unchanged

#### Scenario: Separate emulator save directories
- **WHEN** two plugins declare different approved save directories and naming rules for the same ROM
- **THEN** both profiles remain associated with the same game and their runtime contexts without assuming either directory is beside the ROM

### Requirement: Runtime save descriptors preserve optional compatibility
Save contracts SHALL support optional versioned descriptors for artifact kind, format/version, content binding, naming and runtime compatibility with attributed evidence. Unknown metadata SHALL remain unknown. Format declarations SHALL NOT authorize conversion or execution. Rich save browsing, platform parsers and converters SHALL remain future capabilities rather than required Core implementations in this delivery.

#### Scenario: Different save artifact kinds
- **WHEN** a plugin describes an in-game save and a save state for the same content
- **THEN** their formats and runtime/version compatibility remain distinct; extension or byte size alone does not establish interchangeability

#### Scenario: Future conversion workflow
- **WHEN** a future negotiated plugin capability proposes converting a save for another runtime
- **THEN** it requires approved input/output scopes, explicit review, backup of originals, result validation and preserved provenance; descriptive metadata alone cannot trigger it

#### Scenario: Future per-runtime selection
- **WHEN** a future supported selector associates a compatible discovered save with a game/runtime
- **THEN** loading follows declared runtime capabilities and required import/naming steps, with no silent copy, rename, overwrite or assumption that every runtime supports arbitrary save selection

### Requirement: Common scoped location inspection
Installation, save and runtime locations SHALL expose size, access, modification dates, fingerprint/comparison and availability through approved resource handles. Metadata SHALL distinguish read-only attributes from effective read/write/create/delete access and unknown states. Inspection SHALL NOT change files or grant writes.

#### Scenario: Directory read-only attribute is insufficient
- **WHEN** the OS cannot establish effective write permission without modifying a resource
- **THEN** the API returns unknown with its observation method rather than declaring write access from the attribute alone

### Requirement: Fresh bounded content fingerprints
Fingerprint inspection SHALL support SHA-256 per file and a versioned deterministic tree digest. Fresh comparisons SHALL read content and distinguish changed/unchanged/indeterminate/incomparable, including file additions/deletions. Partial, unstable or unavailable reads SHALL NOT report a complete unchanged result. Large scans SHALL be cancellable background jobs.

#### Scenario: Same metadata different bytes
- **WHEN** a file changes while preserving its size and modification time
- **THEN** fresh hashing detects the content difference rather than reusing metadata as proof of equality

#### Scenario: Concurrent save write
- **WHEN** a file changes during hashing or part of the set cannot be read
- **THEN** the result identifies incomplete/unstable observations and does not certify an unchanged save set

### Requirement: Storage characteristics and mount availability
Location inspection SHALL separate availability, local/network, internal/external/removable, interface and media characteristics, with evidence and unknown states. Availability watches SHALL notify Core and authorized plugins. Disconnected volumes SHALL preserve registrations/history; a different device at the same mount point SHALL not inherit scope approval.

#### Scenario: External USB HDD
- **WHEN** a location is on an identified external USB hard disk
- **THEN** the result can express external, USB and HDD simultaneously

#### Scenario: Mount disappears
- **WHEN** a monitored save or installation volume disconnects
- **THEN** affected work pauses and records become unavailable rather than being reconciled as empty, deleted or uninstalled

### Requirement: Plugin storage diagnostic publication
Plugins SHALL be able to publish attributed benchmark and SMART reports with device context, tool/version, timestamps, units, test conditions and unavailable/error states. Core SHALL validate and relay these reports without running the diagnostic tools or certifying their findings. Publishing SHALL NOT authorize benchmark writes.

#### Scenario: Third-party SMART collector
- **WHEN** an authorized plugin submits health observations or an access-denied result
- **THEN** the API retains source attribution and makes the approved report available without querying SMART itself

### Requirement: Hardware observation publication
The API SHALL receive versioned plugin-collected CPU/GPU/memory/OS/driver observations with computer context, source and unknown states. Standard records SHALL omit unique hardware identifiers. Hardware data SHALL be available only under explicit resource/field sharing policy and SHALL not imply a Core performance prediction.

#### Scenario: Prediction by another plugin
- **WHEN** the user authorizes a predictor to read selected hardware and performance fields
- **THEN** that plugin can consume those fields while unrelated identifiers and other datasets remain withheld

### Requirement: In-game presence publication
Plugins SHALL be able to publish/clear bounded game-session status fields such as mode, map, character and level. Records SHALL retain source, sequence, time and expiry and become stale on timeout/disconnect. Receiving presence SHALL NOT automatically publish it to profiles/friends or treat it as authoritative process/backup state.

#### Scenario: Ranked game details
- **WHEN** an authorized producer reports ranked mode, character and level for a game session
- **THEN** a separately authorized plugin can subscribe to the selected fields until their TTL expires

### Requirement: Game performance observations and history
The API SHALL accept bounded timestamped FPS, CPU and RAM batches with original game/session/computer identity, source, units, sampling windows and normalization. It SHALL preserve gaps and distinguish measured values from estimates. Historical query/aggregation SHALL require retained samples and current data-sharing authorization.

#### Scenario: Performance history
- **WHEN** the producer is granted persistence and publishes a measured gameplay series
- **THEN** authorized consumers can query retained intervals with source and coverage information

#### Scenario: Missing sample
- **WHEN** a sample is dropped or arrives out of order
- **THEN** the API records the gap/sequence condition without fabricating zero FPS

### Requirement: Paired inter-plugin dataset access
Common-data families SHALL expose projected get/list/watch. Requests SHALL list the common categories to include; an omitted or empty list SHALL exclude common records. Every listed category SHALL require a current user grant. Sensitive/custom datasets SHALL require separate sharing review. Secrets and administrative commands SHALL remain unavailable through common-data permissions.

#### Scenario: Consumer queries another publisher
- **WHEN** a plugin with the shared performance-history read permission explicitly requests common records from another publisher
- **THEN** the host returns the authorized category projection over authenticated API transport without requiring pairwise approval or a public broadcast endpoint

#### Scenario: Query defaults to private data
- **WHEN** a plugin issues a query without selecting common records
- **THEN** the host excludes other plugins' common records while leaving the caller's private data and normal query result available

#### Scenario: Common data permission is absent
- **WHEN** a plugin explicitly requests common records without the applicable user grant
- **THEN** the host denies that common-data query and does not alter or disclose the caller's private data

### Requirement: Host-assigned data provenance
Shared records SHALL carry host-assigned producer identity, declared developer, origin, publisher fingerprint, installed version/package digest, schema, resource and observation/receipt times. A producer SHALL NOT forge another source. Derived records SHALL retain origin lineage and SHALL NOT expose fields outside the applicable category projection when republished.

#### Scenario: Forged producer version
- **WHEN** a plugin submits another producer name/version in provenance fields
- **THEN** the host rejects or replaces the claim with authenticated provenance rather than attributing it to the other plugin

### Requirement: Explicit persistence durability
Durable publication, received-data copies and private writes SHALL require storage.persist independently of category read permissions. Calls SHALL specify session-only or persistent durability and receive the actual mode/expiry. A denied persistent request SHALL fail explicitly rather than silently losing durability.

#### Scenario: Persist received performance samples
- **WHEN** a consumer has data access but lacks storage.persist
- **THEN** its persistent-copy request is denied and session-only consumption remains separately available

#### Scenario: Revoke access after a copy was retained
- **WHEN** the user revokes a category read permission after a consumer has durably retained an authorized copy
- **THEN** future source queries are denied, the retained copy follows its separately authorized retention policy, and the UI explains that already delivered data cannot be recalled automatically

### Requirement: API-only isolated persistence
Plugins SHALL address managed persistence exclusively by their own logical identifiers through the API. No plugin-facing response, error, event or selector SHALL expose or accept physical persistence paths. Authentication SHALL determine namespace ownership; cleanup and policy calls SHALL never address another namespace, including through generic Files APIs.

#### Scenario: Foreign entry id
- **WHEN** a plugin supplies another namespace entry/cursor or a traversal-like selector to deletion
- **THEN** the API refuses the request and changes no data outside the authenticated owner namespace

### Requirement: Own persistence cleanup and retention
storage.persist SHALL permit a plugin to configure automatic retention and request deletion/clear of its own managed data within user limits. Cleanup SHALL preserve protected records, report reclaimed bytes and coordinate with active jobs. Global cleanup and revealing physical storage locations SHALL be host user administration actions only.

#### Scenario: Own cleanup policy
- **WHEN** a permitted plugin requests pruning its unprotected performance samples older than its approved retention limit
- **THEN** the host removes only eligible entries in that plugin namespace and reports the result without disclosing physical paths

### Requirement: Shared domain catalog lookup
A plugin SHALL be able to register an owned common-data domain resource and another plugin SHALL discover it through domain get/list/watch methods under a user-granted category read permission and its declared resource scope. Such lookup SHALL NOT require Data APIs or a second source-to-recipient agreement. Sensitive/custom projections SHALL retain explicit sharing review; read access SHALL not confer mutation or access to excluded private/secret fields.

#### Scenario: A registers B reads
- **WHEN** A registers a runtime and the user grants B the applicable runtime category read permission
- **THEN** B finds the resource through Runtimes.List/Get/Watch with A provenance and cannot update it

#### Scenario: Emulator discovers same-console ROM availability
- **WHEN** an emulator plugin with the common content-catalog permission requests that category through Library.List/Get/Watch for its console
- **THEN** it receives the authorized ROM/content identities and the runtimes that report them available, including other publishers, without receiving ROM bytes or filesystem paths

### Requirement: Typed authority and operation fields
Every RPC SHALL keep identity, provenance, permissions, scopes, handles, executable targets, arguments, paths, quotas, revisions, filters, outcomes and standard metric semantics in typed validated fields. A Struct or opaque payload SHALL NOT override these values, introduce a host operation or grant new authority.

#### Scenario: Executable hidden in metadata
- **WHEN** a runtime metadata extension contains an executable path or grant-like key
- **THEN** Core does not use it as an executable target or permission and launches only through approved typed fields

### Requirement: Versioned namespaced resource extensions
Designated resources/reports SHALL support typed stable fields plus namespaced versioned Struct metadata. Extension envelopes SHALL identify schema and version and retain authenticated provenance. Core SHALL validate structure and bounds without requiring a Core-specific field for each plugin annotation or interpreting unknown metadata as behavior.

#### Scenario: Custom runtime annotations
- **WHEN** A registers a declared runtime extension understood by B but not semantically modeled by Core
- **THEN** authorized B can read it through the regular runtime API without changing the GameNode protobuf schema

### Requirement: Opaque content inside typed envelopes
Files, images, HTTP bodies, private storage and secret contents SHALL use bounded byte payloads with typed context/metadata. Core SHALL NOT deserialize arbitrary blobs into commands or generic permissions. Generic custom datasets SHALL use a typed dataset/schema/resource envelope with a declared Struct payload.

#### Scenario: Custom dataset is not runtime registration
- **WHEN** a client publishes custom JSON describing a runtime through Data.Publish
- **THEN** the data can be retained within its authorized custom dataset but cannot register a runtime or bypass Runtimes validation

### Requirement: Bounded declared extension schemas
Extension schemas SHALL be versioned declarative package assets with field classification and no executable hooks or external schema retrieval. The host SHALL enforce size/depth/key limits and schema shape. Unsupported optional metadata SHALL not invalidate usable base fields or acquire visibility by default.

#### Scenario: Unknown optional namespace
- **WHEN** a reader does not support an optional metadata namespace on an approved resource
- **THEN** it can use the typed base while the unknown namespace remains unexecuted and subject to its own projection

### Requirement: Revision-safe extension patches
Resource updates SHALL require expected revisions and explicit field/namespace patches within creator scope or an explicit delegated domain-write grant. Unselected or unsupported extension namespaces SHALL be retained. Stale updates, unauthorized namespace changes and creator-provenance forgery SHALL fail without overwriting existing data.

#### Scenario: Older writer preserves unfamiliar metadata
- **WHEN** an authorized older client patches one typed field without selecting an extension namespace
- **THEN** the host preserves that extension instead of deleting it through full-object replacement

### Requirement: Endpoint field-policy coverage
Every public RPC and stream operation SHALL document which fields are typed, opaque or extension-bearing and its catalog/private/sensitive access model. SDK and direct gRPC clients SHALL observe equivalent validation, unknown-extension behavior and compatibility. Existing official API contracts SHALL remain compatible.

#### Scenario: New RPC without classification
- **WHEN** a proposed method lacks an operation/schema/access classification
- **THEN** contract conformance rejects its publication until that policy is specified

### Requirement: Immutable host launch plans
Customized execution SHALL resolve to a typed immutable host launch plan covering game/profile, source/version, effective executable/URI, runtime/mod components, argv, cwd, environment and current approval/resource revisions. Approval SHALL reference that exact plan. Dispatch SHALL revalidate it and refuse changed/replayed/expired plans rather than accept substituted execution fields.

#### Scenario: Plan substitution
- **WHEN** a plugin obtains approval for one plan and submits different execution parameters
- **THEN** the host refuses dispatch under the old approval

#### Scenario: Resource changes before dispatch
- **WHEN** a reviewed executable/runtime/mod component or grant changes before execution
- **THEN** the old approval is invalidated and the host requests a fresh review or reports an actionable error

### Requirement: Uniform execution customization consent
The same typed launch-plan approval contract SHALL cover runtime/emulator, mod-loader/layer and custom target/parameter changes mediated by GameNode. Native programs SHALL not be certified safe by approval. Launch approval SHALL not authorize mod installation, game-file mutation or otherwise forbidden API behavior.

#### Scenario: Both client implementations
- **WHEN** SDK and direct gRPC clients propose equivalent modded/runtime custom launches
- **THEN** both use the same host plan, user intent and refusal semantics rather than their own confirmation UI

### Requirement: Common runtime content-session contract
The API SHALL accept negotiated typed runtime-session observations under default-deny game.sessions.report scoped to approved games and runtime instances. Events SHALL carry content, instance, session correlation, sequence, time and lifecycle state with explicit acceptance or refusal. Plugin adapters SHALL own runtime-specific detection; a plugin SHALL be allowed to support several runtimes. Descriptive presence SHALL NOT establish gameplay sessions.

#### Scenario: Several emulator adapters in one plugin
- **WHEN** an authorized plugin supports several emulators using different observation mechanisms
- **THEN** each adapter reports through the same session contract without emulator-specific detection logic in Core

#### Scenario: Unapproved game
- **WHEN** a plugin reports gameplay for a game outside its approved reporting scope
- **THEN** the host explicitly refuses the observation without changing accounting

#### Scenario: Shared session observations
- **WHEN** another plugin queries runtime-session observations through Data.Get/List/Watch
- **THEN** only its explicitly consented sensitive-data projection is returned without process handles, credentials or launch tokens

### Requirement: Consistent public pool authorization
Domain APIs and Data projections SHALL enforce the same publication, operation and resource-scope grants. Category-wide and selected-record grants SHALL be supported without exposing private fields. Shared record updates SHALL require expected revisions and preserve creator provenance while recording the acting plugin. Read selectors SHALL NOT confer write authority.

#### Scenario: Alternate route cannot widen a grant
- **WHEN** a plugin authorized to update one public record submits a mutation through another API alias
- **THEN** the same selected-record scope and revision checks apply, and other records remain protected
