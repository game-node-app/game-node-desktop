# Spec Delta

## MODIFIED Requirements

### Requirement: Bundled provider manifests
The application SHALL embed trusted bundled TOML provider manifests at build time and load them only on declared platforms. Plugin-backed manifests SHALL identify the packaged plugin and its supported operations. Bundled identities SHALL NOT be loaded or overridden from user-writable locations. Separate third-party identities SHALL load only from validated, installed, enabled and compatible managed packages.

#### Scenario: Steam manifest on a supported platform
- **WHEN** the packaged application starts on Windows or Linux
- **THEN** the bundled Steam provider and its compatible external plugin are available without manual plugin installation

#### Scenario: Manifest for another platform
- **WHEN** a bundled manifest does not list the current platform
- **THEN** that provider is not loaded and not shown as a filter

#### Scenario: Declared plugin is missing
- **WHEN** the Steam plugin executable is absent or incompatible
- **THEN** the desktop reports its unavailability, preserves cached data and does not run an in-process Steam substitute

#### Scenario: Third-party package registers a distinct provider
- **WHEN** an enabled managed package contains a validated provider manifest with an unreserved unique identity
- **THEN** the host registers that provider through the third-party registry without replacing bundled manifest data

### Requirement: Manifest validation isolates failures
The application SHALL validate every manifest when it loads. Validation SHALL reject unknown fields, missing required fields, an unknown native scanner, an unknown catalog backend, an unknown playtime counter, a duplicate provider id, malformed placeholders, and invalid action declarations. Registered plugin operations SHALL be valid scanner, catalog and counter backends only within their approved descriptor. Executable launch SHALL be restricted to the bundled manual provider or an explicitly approved managed game target; unapproved providers SHALL retain declared URI actions. An invalid manifest SHALL be skipped and logged, and the other manifests SHALL still load.

#### Scenario: One manifest is invalid
- **WHEN** one bundled manifest fails validation
- **THEN** it is skipped with a diagnostic naming the provider and the problem
- **AND** every valid provider still scans and launches

#### Scenario: Unknown native scanner
- **WHEN** a manifest names a native scanner the application does not have
- **THEN** that manifest is rejected and no scan runs for it

#### Scenario: Unknown catalog backend
- **WHEN** a manifest names a catalog backend that is neither available natively nor authorized through its registered plugin
- **THEN** that manifest is rejected with a diagnostic naming the backend and other providers remain usable

#### Scenario: Unknown playtime counter
- **WHEN** a manifest names a playtime counter the application does not have
- **THEN** that manifest is rejected with a diagnostic naming the counter

#### Scenario: Undeclared executable action
- **WHEN** a non-manual provider declares an executable action without an approved managed target
- **THEN** it is rejected and no executable can be launched through that declaration

#### Scenario: Valid plugin backend
- **WHEN** a manifest selects a scanner, catalog or counter capability from its registered plugin descriptor
- **THEN** the host accepts that operation without requiring a Steam-specific native backend

#### Scenario: Plugin capability exceeds its descriptor
- **WHEN** the manifest requests an operation or permission not approved for that plugin
- **THEN** the host rejects it and other valid providers remain usable

### Requirement: Approved URI and executable game launch
URI Play and Install SHALL open only a filled URI whose scheme is the scheme of the manifest's own template, passed as one argument without a shell. The bundled manual provider and approved managed game targets SHALL additionally permit Play through a user-approved executable inside its registered game folder, with a stored argument list and working directory. Explicitly approved custom recipes SHALL also permit alternate targets and runtime wrappers in their separately approved resource scopes. Manual entries SHALL have no Install action. Executable targets and custom recipes SHALL be revalidated before each launch; custom overlays SHALL preserve original provider data and game identity.

#### Scenario: Play a Steam game
- **WHEN** the user plays a detected Steam game
- **THEN** the OS opens `steam://rungameid/<id>` with the registered Steam handler

#### Scenario: Install through the store client
- **WHEN** the user chooses Install for a Steam game
- **THEN** the OS opens `steam://nav/games/details/<id>` and the application writes nothing to game folders

#### Scenario: Handler is missing
- **WHEN** the OS has no handler for the scheme or opening fails
- **THEN** the application shows a localized error naming the store and the library is unchanged

#### Scenario: Direct executable launch
- **WHEN** the user plays an available DRM-free entry
- **THEN** its approved executable is started with its stored argument list and working directory without invoking a shell

#### Scenario: Escaping the registered folder
- **WHEN** an executable resolves through a replaced junction outside its registered game folder
- **THEN** launch is refused and the entry offers repair

## ADDED Requirements

### Requirement: Reserved official identities
Third-party registration SHALL reject reserved official plugin/provider identities and duplicate installed provider ids. Publisher-controlled metadata SHALL NOT establish official status or select a bundled credential alias. Invalid packages SHALL fail independently without changing official registration.

#### Scenario: Package claims Steam identity
- **WHEN** a third-party package declares Steam's provider/plugin identity or an official reserved namespace
- **THEN** it is rejected before execution and Steam's descriptor, grants and credentials remain unchanged

### Requirement: Approved third-party backend registration
Only enabled compatible third-party packages SHALL register runtime backends. Their discovery/catalog/counter/action declarations SHALL be validated against negotiated capabilities and existing record/action contracts. Disabling SHALL preserve saved facts and history while making new provider work and actions unavailable.

#### Scenario: Disabled third-party source
- **WHEN** a previously scanned provider is disabled
- **THEN** its historical data remains intact, no new provider scan starts and its unavailable actions are explained to the user

### Requirement: Official plugin compatibility baseline
Adding third-party support SHALL preserve existing official plugin implementations, manifests, permissions, credential mappings and package layouts. Third-party failures, permission decisions and compatibility policies SHALL NOT change official provider operation.

#### Scenario: No third-party packages
- **WHEN** the host starts with an empty external registry
- **THEN** Steam and the platform's other official providers operate with their existing behavior and grants

### Requirement: Managed local targets and launch overlays
Third-party provider discoveries SHALL support approved local executable targets as well as URI targets. Independent extensions SHALL contribute user-approved per-game custom launch overlays for any provider without replacing provider identity or facts. An executable target SHALL NOT become executable solely because it appeared in an unreviewed scan.

#### Scenario: Register a disc-installed game
- **WHEN** a provider discovers an executable for a local installation and the user approves its validated target
- **THEN** Play can start that target with its stored parameters without a store launcher

#### Scenario: Custom route for an official provider
- **WHEN** an extension's approved recipe is selected for an official provider game
- **THEN** Play uses that overlay while the original provider's detected target and plugin implementation remain intact

### Requirement: Individual content identities for shared runtimes
An approved emulator/interpreter/engine runtime SHALL be reusable by multiple individually identified game contents. Game entries SHALL retain distinct logical/installation identities, saves and history and reference structured runtime launch recipes. Runtime path/PID alone SHALL NOT identify every hosted game as one game.

#### Scenario: One engine several games
- **WHEN** a provider discovers several content items executed by the same runtime
- **THEN** each content appears as a distinct game with its own launch arguments and history

#### Scenario: Runtime updated
- **WHEN** the shared runtime version/path changes
- **THEN** game identities remain distinct and affected launch approvals are revalidated without merging their entries

### Requirement: One game with selectable runtime bindings
Runtime-hosted content SHALL reference its GameNode game identity independently of runtime and discovering plugin. Several approved runtimes SHALL be selectable for the same game/content without duplicating the game or resetting history. Content resources SHALL retain separate file/variant identities. Removing a runtime binding SHALL NOT delete games, ROMs, saves or history.

#### Scenario: One ROM with two emulators
- **WHEN** authorized ZSNES and bsnes discoveries identify the same registered ROM and GameNode game
- **THEN** the library retains one game with two selectable runtime bindings and preserves its playtime and saves

#### Scenario: Migrate from DeSmuME to melonDS
- **WHEN** the user authorizes reading the existing game/content references and selects a newly registered melonDS runtime
- **THEN** only the approved runtime binding changes, the original game identity/history remain and saves are not silently converted or overwritten

### Requirement: Runtime plugin discovery is separately authorized
Runtime plugins SHALL be able to request library.discovery for approved content roots, including negotiated runtime-content contexts without provider impersonation. Core SHALL reconcile observations into game/content identities rather than using runtime ownership as game identity. Discovery SHALL NOT imply runtime mutation, launch, file writes or broader library reads.

#### Scenario: Discovery by a runtime extension
- **WHEN** an authorized emulator extension submits scoped content observations without owning a store provider
- **THEN** Core reconciles those observations and candidate runtime bindings without granting a foreign provider context or executing the content

### Requirement: Runtime metadata preserves typed launch contracts
Runtime/content records SHALL permit optional versioned descriptive plugin metadata while keeping validated executable/content references and launch parameters in typed fields. Reading metadata SHALL not authorize execution. Unknown descriptive extensions SHALL not merge identities or alter approved launch semantics.

#### Scenario: Recommendation becomes a launch proposal
- **WHEN** a plugin reads preferred runtime information in an extension
- **THEN** it must submit a typed custom-launch recipe for approval before that recommendation affects execution

### Requirement: Launch overlays share one review boundary
Approved runtime and mod-related overlays SHALL preserve original game/provider identity and use the common host launch-plan review. Unchanged official default launching SHALL keep its baseline behavior. Known/declared components SHALL not be presented as an exhaustive inventory of everything the native program can execute.

#### Scenario: Overlay without official plugin changes
- **WHEN** a mod-loader/runtime customization is selected for an official provider game
- **THEN** the host reviews the overlay plan while leaving that official provider implementation and original detected launch target intact

## RENAMED Requirements

- FROM: `### Requirement: Launch and install are restricted to declared URIs`
- TO: `### Requirement: Approved URI and executable game launch`
