# desktop-lifecycle Specification

## Purpose

Integrate GameNode Desktop into Windows and Linux as a per-user application with one running instance and reliable delivery of launches and browser authentication callbacks.

## Requirements

### Requirement: Installable Windows and Linux builds
CI SHALL produce unsigned, per-user-installable application artifacts for Windows and Linux. Installation SHALL provide the binary, bundled assets and license notices, an application entry point, and `app.gamenode://` handling without requiring administrator privileges.

#### Scenario: Install a CI artifact
- **WHEN** a user installs the Windows or Linux CI artifact
- **THEN** they can open the application from its registered entry point and the OS can deliver a GameNode scheme URL to it
- **AND** fonts, logos, translations, and About/license information are present in the installed application

### Requirement: One running application instance
The application SHALL permit one running instance per OS user. A later ordinary launch SHALL request that the existing instance show its window and then exit without initializing a second application session.

#### Scenario: Launch twice
- **WHEN** a user launches the installed application while it is already running
- **THEN** the existing window is raised or requests attention under the compositor's rules
- **AND** no second window, database owner, or independent authenticated application session is created

#### Scenario: Concurrent startup
- **WHEN** two launches occur at nearly the same time
- **THEN** exactly one becomes the application owner and the other hands its request to that owner

### Requirement: Deliver GameNode callback URLs
The registered Windows and Linux URI handlers SHALL deliver the complete GameNode callback as data to the owning instance. If no instance is running, the URI handler SHALL start one. Delivery SHALL NOT interpret callback content as shell commands or authenticate without the desktop-auth checks.

#### Scenario: Callback while sign-in is pending
- **WHEN** the browser opens an `app.gamenode://` callback while the application has a pending sign-in
- **THEN** the callback reaches the existing instance for validation and the forwarding process exits
- **AND** the full URL and credential are excluded from application and installer diagnostics

#### Scenario: Callback launches a stopped application
- **WHEN** a scheme URL starts the application with no instance running
- **THEN** the application opens and validates it as an incoming callback
- **AND** it rejects authentication if no matching pending attempt exists

#### Scenario: Invalid or hostile payload
- **WHEN** another process supplies a malformed URL or a payload containing shell syntax
- **THEN** the application rejects unsupported data without executing it or modifying the active session

### Requirement: Exit behavior for the foundation
In step 1, closing the window or choosing Quit SHALL stop the application and release single-instance resources. The application SHALL NOT register autostart or leave a tray/background process running.

#### Scenario: Close and relaunch
- **WHEN** the user closes the window and launches GameNode again
- **THEN** a new application instance starts and restores persisted foundation state
- **AND** no background tracker or autostart task was created
