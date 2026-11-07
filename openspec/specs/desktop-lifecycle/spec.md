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

### Requirement: Close returns to the background
On Windows, closing the window SHALL hide it and keep the application running with tracking and the tray icon. It SHALL save window geometry and layout as closing did before. On other platforms in the MVP, closing the window SHALL stop the application as in step 1.

#### Scenario: Close the window during play
- **WHEN** the user closes the window while a game is running
- **THEN** the window disappears, the tray icon stays, and the game's play keeps being tracked

#### Scenario: Close on Linux
- **WHEN** the user closes the window on Linux
- **THEN** the application stops and releases single-instance resources

### Requirement: Quit ends the application
Quit in the app menu and in the tray menu SHALL end any open session at that moment, save state, release single-instance resources and stop the process.

#### Scenario: Quit from the tray
- **WHEN** the user chooses Quit in the tray menu while a game is running
- **THEN** the session ends at that time, the tray icon disappears and no GameNode process remains

### Requirement: Start in the background
Started with `--background`, the application SHALL start tracking and show the tray icon without opening a window. Opening GameNode from the tray, the Start menu or a shortcut SHALL show the window. Starting without `--background` SHALL show the window as before.

#### Scenario: Start at login
- **WHEN** the user signs in to Windows
- **THEN** GameNode is running with its tray icon and no window
- **AND** the library is scanned and installed games are tracked

#### Scenario: Open from the Start menu while in the background
- **WHEN** GameNode is running in the background and the user opens it from the Start menu
- **THEN** the existing instance shows its window and no second instance starts

#### Scenario: Second background start
- **WHEN** GameNode is already running and another instance is started with `--background`
- **THEN** the second instance exits without showing the window of the running one

### Requirement: Autostart at login
On Windows, the application SHALL register itself to start at login with `--background` through a value under the current user's `Run` key, on by default. It SHALL rewrite the value at each start with its current executable path and SHALL NOT override the user disabling the entry in Windows' startup settings. Uninstalling SHALL remove the value.

#### Scenario: First start after install
- **WHEN** the installed application starts for the first time
- **THEN** a `Run` value for the current user starts GameNode with `--background` at the next login, without administrator rights

#### Scenario: Disabled in Windows settings
- **WHEN** the user disables GameNode in Windows' Startup apps settings and GameNode starts again
- **THEN** GameNode remains disabled there and does not start at the next login

#### Scenario: Moved installation
- **WHEN** the application is started from a new location
- **THEN** the `Run` value points to the new executable

#### Scenario: Uninstall
- **WHEN** the user runs the uninstaller
- **THEN** the `Run` value is removed

### Requirement: Tray icon
While the application runs on Windows, it SHALL show a tray icon with GameNode's icon. Activating the icon SHALL show the window. Its menu SHALL offer Open GameNode and Quit, and its tooltip SHALL name the running game while one is running.

#### Scenario: Open from the tray
- **WHEN** the user clicks the tray icon while the window is hidden
- **THEN** the window is shown and brought to the front

#### Scenario: Tooltip during play
- **WHEN** a detected game is running
- **THEN** the tray icon's tooltip names that game
