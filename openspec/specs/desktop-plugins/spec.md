# desktop-plugins Specification

## Purpose

Run authorized provider plugins through an authenticated public desktop API while preserving the application's existing features and user data on Linux and Windows.

## Requirements

### Requirement: The desktop application hosts the Plugin API
The running desktop application SHALL own the Plugin API and active profile on Linux and Windows. Plugins SHALL connect through authenticated local sessions belonging to the current OS user. A connection SHALL NOT start another profile owner or daemon, and remote login pages SHALL NOT gain frontend command access.

#### Scenario: Plugin connects to the desktop
- **WHEN** the authorized Steam process connects to the running application
- **THEN** its requests use the application's existing profile and domain services

#### Scenario: Desktop application is stopped
- **WHEN** a plugin attempts to connect after the desktop host stops
- **THEN** the connection fails without opening the profile or starting a daemon

### Requirement: Local transports provide equivalent platform protection
Linux sessions SHALL use private Unix sockets with ownership checks and peer-user verification. Windows sessions SHALL use named pipes restricted to the current user and SYSTEM, reject remote clients and protect endpoint ownership. Both transports SHALL authenticate the launched plugin before accepting work.

#### Scenario: Untrusted local connection
- **WHEN** a client lacks the current launch credential or permitted OS identity
- **THEN** the host rejects it without exposing data or modifying the profile

#### Scenario: Windows reconnects after a connection closes
- **WHEN** a legitimate plugin establishes another named-pipe connection
- **THEN** a protected server instance accepts it without relaxing endpoint ownership or remote-client restrictions

### Requirement: Plugin sessions negotiate explicit authority
The host SHALL validate plugin identity, temporary launch credentials, API compatibility and requested capabilities. It SHALL return only approved grants and supported features. Required unsupported operations SHALL fail explicitly. Grants SHALL expire with their session and SHALL NOT expand when an optional protocol feature is added.

#### Scenario: Unauthorized capability
- **WHEN** a plugin requests a grant absent from its trusted descriptor
- **THEN** negotiation fails without exposing that service

#### Scenario: Old discovery-only executable
- **WHEN** an executable lacks the operations required for complete Steam support
- **THEN** the host reports incompatibility rather than treating discovery support as complete integration

### Requirement: Plugins use only the public application boundary
The Steam plugin and public SDK/protocol SHALL build without direct or transitive dependencies on internal application, Core, GameNode HTTP or platform crates. Communication with GameNode SHALL use the versioned Plugin API, excluding raw database access, GameNode credentials and arbitrary internal commands.

#### Scenario: Independent reference host
- **WHEN** the Steam plugin is built against the public SDK and contract
- **THEN** a compatible reference host can exercise its operations without linking application internals

#### Scenario: Internal storage representation changes
- **WHEN** the application changes its database adapters while preserving the public contract
- **THEN** the plugin continues communicating through public messages

### Requirement: Provider work is identified and cancellable
Operations SHALL have host-issued identifiers, typed terminal outcomes and cancellation. The host SHALL reject duplicate, late, cross-provider or superseded results. Login controls SHALL remain responsive while authentication or catalog work is pending.

#### Scenario: Cancel races with successful login
- **WHEN** cancellation invalidates a login before its success is accepted
- **THEN** late credential writes and success events cannot reconnect the account

#### Scenario: Duplicate catalog completion
- **WHEN** a plugin repeats an accepted terminal event
- **THEN** the host does not reconcile or persist the result twice

### Requirement: Plugin lifetime follows the desktop application
The host SHALL supervise one active Steam process per authorized provider session and coordinate concurrent initialization, account and discovery work. Restart SHALL create a fresh session and invalidate obsolete handles. Desktop Quit SHALL stop children and release endpoint ownership within bounded waits.

#### Scenario: Startup restoration and scan overlap
- **WHEN** restoration and discovery target Steam simultaneously
- **THEN** they use one supervised process with separately identified operations

#### Scenario: Linux desktop exits
- **WHEN** the desktop completes a normal Quit
- **THEN** plugin children exit and its plugin and single-instance socket filesystem entries are removed

### Requirement: Plugin failures preserve unrelated application functionality
Endpoint, executable, protocol and process failures SHALL be reported for the affected provider without preventing application startup or creating crash loops. Cached provider facts SHALL remain valid except where authoritative account transitions invalidate ownership. PC ports and other mainline services SHALL remain usable independently of Steam plugin availability.

#### Scenario: Steam executable is missing
- **WHEN** the application starts without its Steam plugin executable
- **THEN** it opens, reports Steam as missing and keeps existing cached entries available

#### Scenario: Steam fails while ports installs
- **WHEN** Steam crashes or cannot authenticate during a PC ports installation on Windows
- **THEN** the installation, its progress and cancellation remain available without depending on a Steam session

### Requirement: Plugin execution and messages are bounded
The host SHALL bound connections, queues, payloads, records, operation durations and shutdown waits. Incomplete catalog transfers SHALL NOT replace successful facts. I/O, decoding, computation, storage and waits SHALL run outside UI callbacks and SHALL preserve a responsive desktop.

#### Scenario: Catalog transfer is interrupted
- **WHEN** a plugin sends only part of a catalog category before disconnecting
- **THEN** the previous complete category remains in use and a categorized failure is reported

#### Scenario: Slow login or credential service
- **WHEN** a provider request waits for network, user input or credential storage
- **THEN** the main interface remains responsive and cancellation or timeout remains bounded

### Requirement: Installed plugin inventory reports trusted metadata
The GUI SHALL display the packaged Steam plugin's status, origin, granted permissions and native-access declarations. Official status SHALL derive from trusted distribution metadata. Native file, network and registry access SHALL be distinguished from host-enforced API grants and SHALL NOT be described as sandboxed.

#### Scenario: Official Steam plugin is packaged
- **WHEN** inventory displays a compatible Steam executable on Linux or Windows
- **THEN** it reports the external official plugin and its actual approved grants

#### Scenario: Plugin claims to be official
- **WHEN** plugin-provided text asserts an origin not established by trusted metadata
- **THEN** that text cannot change the displayed trusted origin

### Requirement: Packaged plugins work in development and releases
Documented development and release workflows SHALL build and locate the host and compatible Steam executable on Linux and Windows without PATH executable lookup or a manually installed Protobuf compiler. Missing or incompatible executables SHALL produce an actionable provider status without an in-process Steam fallback.

#### Scenario: Clean supported environment
- **WHEN** the documented locked workflow runs without a system Protobuf compiler
- **THEN** declared build dependencies produce both the application and Steam executable

#### Scenario: Application is installed under a path with spaces
- **WHEN** the packaged desktop starts from that location
- **THEN** it launches the intended adjacent Steam executable and establishes an authenticated session

### Requirement: Steam integration requires native parity acceptance
Acceptance SHALL preserve existing mainline behavior and data. Native Linux validation and explicit user confirmation SHALL precede native Windows VM acceptance. Build, lint and fixture results SHALL be distinguished from native execution evidence; merge readiness SHALL remain pending until required native scenarios pass on both platforms.

#### Scenario: Linux passes but Windows is untested
- **WHEN** the user confirms Linux behavior and no Windows VM execution has occurred
- **THEN** Windows acceptance and final merge readiness remain pending

#### Scenario: Ports installation is Windows-only
- **WHEN** Linux fixtures and UI mocks pass for PC ports
- **THEN** they do not substitute for Windows installation, tracking and recovery acceptance
