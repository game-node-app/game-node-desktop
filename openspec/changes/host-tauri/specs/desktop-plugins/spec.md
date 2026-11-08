# Spec Delta

## Purpose

Lets separately launched plugins provide authorized observations and capabilities to the running GameNode Desktop application through a local, versioned API hosted by its Tauri process.

## ADDED Requirements

### Requirement: The desktop application hosts the Plugin API
On Linux, the running desktop application SHALL host the local Plugin API in the same process that owns the Engine and profile. The Plugin API SHALL be distinct from the Tauri IPC used by the React WebView, and SHALL accept connections only from the current OS user.

#### Scenario: Plugin connects while the desktop application is running
- **WHEN** an authorized plugin connects to the local Plugin API
- **THEN** the running desktop host accepts the plugin session without starting another Engine or opening another profile owner

#### Scenario: Plugin attempts to connect while the application is stopped
- **WHEN** a plugin connects after the desktop host has stopped
- **THEN** the connection fails and no background Core or daemon process is started

### Requirement: Plugin sessions authenticate and negotiate compatibility
On Linux, the host SHALL authenticate each plugin process session, verify the plugin identity and declared capabilities, and reject incompatible protocol versions before accepting work. Session credentials SHALL be temporary, scoped to one plugin launch, and invalidated when that process exits or the host stops.

#### Scenario: Authorized compatible plugin completes handshake
- **WHEN** a plugin presents its launch credential, identity, declared capabilities, and a supported API version
- **THEN** the host establishes a session limited to the capabilities authorized for that plugin

#### Scenario: Invalid identity, credential, capability, or API version
- **WHEN** a plugin presents an invalid session credential, undeclared capability, or incompatible API version
- **THEN** the host rejects the request without applying application state changes

### Requirement: Plugin requests are mediated by the desktop host
On Linux, plugins SHALL submit typed requests or observations through the Plugin API. The host SHALL validate each request and route accepted domain work through the Engine; plugins SHALL NOT write application persistence directly or receive GameNode session credentials.

#### Scenario: Plugin submits a discovery observation
- **WHEN** an authorized discovery plugin submits records for an accepted scan operation
- **THEN** the host validates and passes those records through the existing provider reconciliation and persistence flow

#### Scenario: Plugin attempts an ungranted or malformed operation
- **WHEN** a plugin submits data outside its authorized capability or outside the operation's contract
- **THEN** the host rejects the request and preserves the current application state

### Requirement: Plugin failures preserve the last successful provider data
On Linux, the host SHALL report plugin startup, protocol, timeout, and execution failures as provider failures. A failed plugin run SHALL preserve that provider's previously persisted scan data and SHALL NOT be restarted automatically immediately.

#### Scenario: Plugin fails during discovery
- **WHEN** a plugin fails to start, complete its handshake, or finish an accepted discovery operation
- **THEN** the host reports the provider failure and retains its previous successful records

#### Scenario: Discovery is requested again after failure
- **WHEN** a later user-requested scan occurs after a plugin failure
- **THEN** the host may launch a fresh plugin process for that operation

### Requirement: Plugin lifetime follows the desktop application
On Linux, the host SHALL launch plugins only for authorized work, SHALL avoid concurrent duplicate processes for the same plugin capability, and SHALL stop plugin processes and invalidate their sessions when the desktop application exits.

#### Scenario: Concurrent requests target the same plugin
- **WHEN** overlapping scans request the same plugin capability
- **THEN** the host preserves existing scan coordination and does not create duplicate plugin processes for the same operation

#### Scenario: Desktop host exits
- **WHEN** the desktop application shuts down
- **THEN** its plugin processes are stopped and their local API sessions become unusable

### Requirement: Steam discovery can run through the plugin host on Linux
On Linux, Steam discovery SHALL be supplied by the external Steam plugin while preserving the existing provider records, scan coordination, and library persistence behavior.

#### Scenario: Steam scan returns installed games
- **WHEN** the user requests a library rescan and the Steam plugin completes successfully
- **THEN** Steam installations appear with the same provider-local ids, names, and install directories as the existing scanner

#### Scenario: Steam is unavailable or has no installed games
- **WHEN** Steam is not installed or the plugin completes with no records
- **THEN** the scan completes without inventing Steam installations

#### Scenario: One Steam manifest is unreadable
- **WHEN** the plugin encounters an unreadable or malformed game manifest
- **THEN** it skips that entry, reports diagnostics without file contents, and returns other valid records

### Requirement: Discovery plugins are registered by provider
The desktop host SHALL select a discovery plugin from host-owned provider metadata and SHALL authorize only the provider capability for the requested scan operation.

#### Scenario: A registered provider is scanned
- **WHEN** the Engine requests discovery for a provider with an available registered plugin
- **THEN** the host launches that plugin with only its provider-scoped discovery capability

#### Scenario: An unknown provider is requested
- **WHEN** a scan requests a provider without a registered plugin
- **THEN** the host reports that provider as unavailable without launching another plugin

### Requirement: Discovery results identify their provider
An authorized discovery plugin SHALL return typed observations associated with its registered provider, and the host SHALL reject results that do not match the active provider and operation.

#### Scenario: Plugin returns valid provider observations
- **WHEN** a plugin completes an authorized operation with typed records for its registered provider
- **THEN** the host routes those records through the Engine's existing provider reconciliation and persistence flow

#### Scenario: Plugin returns mismatched observations
- **WHEN** a plugin submits records for another provider or operation
- **THEN** the host rejects the result and preserves application state

### Requirement: Scanner plugins use a stable discovery contract
Official scanner plugins SHALL depend on the versioned plugin contract and shared discovery data types rather than directly on application-internal Engine or platform crates.

#### Scenario: Scanner implementation is built as an external plugin
- **WHEN** an official discovery plugin is compiled independently of the desktop application
- **THEN** it can submit the shared typed result contract without linking `gamenode-core` or `gamenode-platform`

### Requirement: Linux package-manager discovery reports game candidates
On Linux, the official package-manager discovery plugin SHALL use the native pacman query interface on Arch-based distributions and PackageKit on other distributions. It SHALL report installed packages as game candidates only when installed desktop application metadata identifies them as games.

#### Scenario: SuperTux is installed and identified as a game
- **WHEN** SuperTux is installed on Arch Linux and its installed desktop entry identifies it as a game
- **THEN** the plugin returns a SuperTux candidate with its package identifier and available launch metadata

#### Scenario: PackageKit is used on a non-Arch distribution
- **WHEN** the host distribution is not Arch-based and PackageKit reports installed ownership for a game desktop entry
- **THEN** the plugin returns the corresponding package candidate with its available launch metadata

#### Scenario: Selected package manager is unavailable
- **WHEN** the native pacman query on an Arch-based distribution or PackageKit on another distribution is missing or cannot be queried
- **THEN** the plugin reports a provider failure without blocking other discovery providers or replacing prior successful data

#### Scenario: Installed package is not identified as a game
- **WHEN** an installed package has no game classification in desktop application metadata
- **THEN** the plugin omits it from game candidates
