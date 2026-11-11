# desktop-plugin-development Specification

## Purpose
Let third-party authors build, test and package independent clients using public documentation, an optional Rust SDK and practical examples in two languages.

## Requirements

### Requirement: Independently consumable MIT development assets
The SDK, public protocol definitions and examples SHALL carry explicit MIT terms and copyright notices, subject to verified licensing authority. Public distributions SHALL be usable outside the GameNode workspace without internal Core/app/platform dependencies. Core and host SHALL retain GPL-3.0-only.

#### Scenario: Build outside the repository
- **WHEN** an author obtains a versioned public SDK or protocol distribution
- **THEN** they can build a plugin without private workspace paths or GPL Core implementation dependencies

#### Scenario: Unresolved contributor rights
- **WHEN** a public asset contains material without authority for MIT distribution
- **THEN** it is not released as MIT until permission is obtained or the affected material is replaced with appropriately licensed code

### Requirement: Complete independent-client documentation
Documentation SHALL define wire messages and RPCs, OS-local transport, session authentication, version negotiation, epochs, command/event ordering, cancellation, deadlines, limits and structured errors. It SHALL distinguish existing calls from additions and describe standalone client generation, manifest/signing, updates, licensing and native-access limitations.

#### Scenario: Implement without the SDK
- **WHEN** an author follows the public protocol guide using a supported gRPC language
- **THEN** all required transport and lifecycle details are available without inspecting host internals or SDK implementation

### Requirement: Rust SDK reference plugin
A runnable Rust example SHALL use the public SDK and a separate example identity. It SHALL demonstrate session opening, host commands and results, fictional discovery, catalog/counter reporting, a protected service request, explicit denial, feature fallback and cancellation/shutdown. It SHALL include build, package, install and run instructions.

#### Scenario: Rust plugin denied a permission
- **WHEN** a user withholds the example's optional data permission
- **THEN** the example reports the typed denial and continues the features that remain authorized

### Requirement: Direct gRPC Go reference plugin
A runnable Go example SHALL use generated public protobuf/gRPC bindings without the GameNode Rust SDK or a Rust bridge. It SHALL implement the same session, provider and denial/fallback scenarios as the Rust example, including native Linux Unix-socket and Windows named-pipe transports.

#### Scenario: Go speaks directly to the host
- **WHEN** the Go example is launched by the host on either supported OS
- **THEN** it authenticates on the host-issued local endpoint, receives a command and returns a valid correlated result without an intermediary TCP service

### Requirement: Practical service recipes and safe fixtures
Both examples SHALL include runnable recipes or deterministic fixture scenarios for every existing public RPC, settings operation and provider event family, plus an added Core read and confirmed write. Examples SHALL use fictional data and demonstrate requests, responses and errors without real Steam credentials or destructive defaults.

#### Scenario: Compare SDK and direct calls
- **WHEN** the same fixture scenario runs through the Rust SDK example and the Go direct client
- **THEN** both receive equivalent wire outcomes and the docs map their code to the same methods/messages

#### Scenario: Secret and login recipes
- **WHEN** the developer runs the documented fixture-host recipes
- **THEN** account/secret/login/action paths are exercised with fake data and no secret values appear in logs

### Requirement: Reference package and compatibility validation
Each example SHALL include a versioned manifest, license notices and reproducible package/signing instructions with development keys clearly identified. Conformance checks SHALL detect stale generated clients, unauthorized internal dependencies, malformed outcomes and incompatible required features.

#### Scenario: Public contract changes
- **WHEN** a compatible API addition is made
- **THEN** both examples remain buildable and compatibility fixtures verify older supported feature behavior

#### Scenario: Example package is installed
- **WHEN** a developer imports either packaged example
- **THEN** the same signature, permission, activation and license checks as other third-party packages apply

### Requirement: Target compatibility authoring and fixtures
The manifest reference SHALL define target constraints, comparison/selection rules, tested environments and host verdicts, including interpreter/native dependency requirements. Deterministic evaluator fixtures SHALL cover Windows builds/products, Linux distribution/libc/version bounds, omitted allowlists, tested coverage, unknown facts, malformed/ambiguous targets and environment changes. Real detection SHALL receive separate native acceptance.

#### Scenario: Portable Python source with native dependencies
- **WHEN** an author packages Python source with a platform-specific interpreter or extension library
- **THEN** the guide requires target requirements for that execution environment rather than declaring compatibility from the source language alone

#### Scenario: No provisioning implied
- **WHEN** target compatibility support is implemented
- **THEN** it introduces neither Python environment provisioning nor executable installation hooks or a required detector plugin

#### Scenario: Portable evaluator fixtures
- **WHEN** pure compatibility fixtures run on a developer machine
- **THEN** injected environment facts determine results independently of the machine running the tests

### Requirement: Configuration examples and ownership documentation
Both reference plugins SHALL demonstrate a working plugin-owned configuration window, focus/close lifecycle and saving one non-secret preference without altering API grants. Documentation SHALL distinguish GameNode-managed administration from plugin-owned functional configuration and include an optional declarative-form recipe.

#### Scenario: Configure either reference plugin
- **WHEN** the user opens its configuration, changes the example preference and saves
- **THEN** that plugin retains the preference through its documented storage path and a second Configure action focuses its existing window

#### Scenario: Configuration attempts permission escalation
- **WHEN** configuration code requests an ungranted host capability
- **THEN** the normal explicit API denial is returned and the GameNode permission state remains unchanged

### Requirement: Standard and custom launch recipes in both examples
Both Go and Rust examples SHALL demonstrate game.launch for detected URI and executable targets and game.custom_launch for an alternate executable/runtime, arguments, working directory and environment. Examples SHALL include cross-provider per-game approval, denied access and unsupported/missing target cases using safe fixture processes and opt-in real execution.

#### Scenario: Same custom launch through SDK and raw gRPC
- **WHEN** each example submits the same approved fixture recipe
- **THEN** the fixture process receives equivalent argument vectors, working directory and environment without shell interpretation

#### Scenario: Example extension has no provider
- **WHEN** either example runs its launch-extension fixture profile
- **THEN** it can propose a recipe for a user-selected foreign-provider game without registering a fake provider or using that provider's credential context

### Requirement: Data producer consumer and persistence examples
Both language examples SHALL demonstrate common and sensitive/custom producer-consumer flows, explicit access controls, separate persistence controls and logical data handles without exposing physical paths.

#### Scenario: Two plugins exchange a series
- **WHEN** a user grants the consumer the performance-history category permission during activation
- **THEN** the example first queries private data by default, can explicitly request matching published series across producers, and retains recipient copies only when separately allowed by storage.persist and the retention policy

### Requirement: Location and save inspection examples
Examples SHALL demonstrate scoped save profile proposals and installation/save inspection including size, access, times, fresh hashes, partial reads and unavailable volumes. Diagnostic fixtures SHALL submit reports rather than require installed benchmark/SMART tools.

#### Scenario: Hash content changes
- **WHEN** the fixture changes bytes without changing file size
- **THEN** both SDK and direct gRPC comparison report the content change under authorized scope

### Requirement: Hybrid contract and catalog examples
Both language examples SHALL demonstrate typed resource registration with a declared versioned Struct extension and authorized lookup by the other example through the domain catalog. Guides SHALL identify typed execution/security fields, opaque content, optional metadata and the distinct sensitive-data sharing flow.

#### Scenario: No Core change for custom runtime metadata
- **WHEN** the examples exchange a declared plugin-specific runtime annotation through normal catalog methods
- **THEN** the same public protobuf schema supports the exchange and standard action validation remains unchanged

#### Scenario: Emulator checks ROM availability across runtimes
- **WHEN** the emulator example explicitly requests common content-catalog records for its console
- **THEN** it receives the matching ROM/content identities and reporting runtimes without receiving file paths or ROM bytes

### Requirement: Launch consent examples
Both reference clients SHALL demonstrate host-generated launch review for runtime/emulator, mod-loader and custom argument/environment recipes, with run-once and remembered approval behavior. Fixtures SHALL cover substituted plans, changed components, revoked scope and forbidden requests without performing destructive real launches.

#### Scenario: Malicious substitution fixture
- **WHEN** either example changes a fixture argument after user approval
- **THEN** the old host plan token cannot authorize the altered execution
