# Spec Delta

## Purpose

Provide one per-user owner for GameNode's Core state and a secure local API through which the Linux GUI and CLI can observe and control it.

## ADDED Requirements

### Requirement: One daemon owns the user's Core state
On Linux, the system SHALL run at most one daemon for a user's GameNode profile. The daemon SHALL own the Engine, credentials, database, trackers, and mutable application state used by all local clients.

#### Scenario: GUI connects to a daemon started by the CLI
- **WHEN** the CLI has started the daemon and the user opens the Tauri application
- **THEN** the GUI observes the same authentication, settings, library, and tracking state without creating a second Engine or database owner

#### Scenario: GUI and CLI start together
- **WHEN** the GUI and CLI both try to start the daemon for the same user
- **THEN** only one daemon becomes the profile owner and both clients connect to it

### Requirement: Local clients are restricted to the current user
On Linux, the daemon SHALL expose its gRPC API through a private local Unix socket and SHALL reject connections from a different user. The API SHALL report its version and reject requests from an incompatible protocol client before changing application state.

#### Scenario: Same-user client connects
- **WHEN** the installed GUI or CLI connects using the current user's local endpoint and a supported API version
- **THEN** the daemon accepts the request and serves the shared application state

#### Scenario: Foreign user or incompatible client connects
- **WHEN** a process from another user or a client with an unsupported API version connects
- **THEN** the daemon rejects the connection or request without exposing state or applying a command

### Requirement: Clients can observe a complete snapshot and recover updates
The daemon SHALL provide a complete credential-free application snapshot and ordered state revisions to local clients. A client reconnecting after a stream interruption SHALL be able to replace stale state with a current full snapshot.

#### Scenario: GUI subscribes while state changes
- **WHEN** a client subscribes while Core state changes
- **THEN** it receives a full current snapshot and subsequent revisions without ending in an older state than the daemon

#### Scenario: GUI reconnects after daemon restart
- **WHEN** the gRPC stream ends and the GUI reconnects to the daemon
- **THEN** the GUI receives the current full snapshot before continuing incremental updates

### Requirement: Daemon shutdown preserves committed state
The daemon SHALL flush queued storage work before a successful stop and SHALL refuse a stop while a library scan is active. Clients SHALL receive an explicit result when shutdown is refused.

#### Scenario: Stop after an update
- **WHEN** a client requests daemon shutdown after a state-changing operation
- **THEN** the daemon persists queued storage work before releasing the profile and exiting

#### Scenario: Stop during a scan
- **WHEN** a client requests shutdown while a scan is active
- **THEN** the daemon remains available and reports that shutdown was refused
