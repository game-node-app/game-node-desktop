# Spec Delta

## MODIFIED Requirements

### Requirement: Close returns to the background
On Windows, while the "Closing the window minimizes to the tray" preference is on (the default), closing the window SHALL hide it and keep the application running with tracking and the tray icon. While it is off, closing the window SHALL end the application as Quit does. Either way it SHALL save window geometry and layout as closing did before. On other platforms in the MVP, closing the window SHALL stop the application as in step 1, and the preference SHALL NOT be offered.

#### Scenario: Close the window during play
- **WHEN** the tray preference is on and the user closes the window while a game is running
- **THEN** the window disappears, the tray icon stays, and the game's play keeps being tracked

#### Scenario: Close with the tray preference off
- **WHEN** the user has turned the tray preference off and closes the window while a game is running
- **THEN** the session ends at that time, state is saved, the tray icon disappears and no GameNode process remains

#### Scenario: Close on Linux
- **WHEN** the user closes the window on Linux
- **THEN** the application stops and releases single-instance resources

### Requirement: Start in the background
Started with `--background`, the application SHALL start tracking and show the tray icon without opening a window. A login start SHALL pass `--background` only while the "Start minimized to the tray" preference is on; while it is off, a login start SHALL open the window. Opening GameNode from the tray, the Start menu or a shortcut SHALL show the window. Starting without `--background` SHALL show the window as before.

#### Scenario: Start at login
- **WHEN** start minimized is on and the user signs in to Windows
- **THEN** GameNode is running with its tray icon and no window
- **AND** the library is scanned and installed games are tracked

#### Scenario: Start at login with the window
- **WHEN** start minimized is off and the user signs in to Windows
- **THEN** GameNode starts with its window open and tracking running

#### Scenario: Open from the Start menu while in the background
- **WHEN** GameNode is running in the background and the user opens it from the Start menu
- **THEN** the existing instance shows its window and no second instance starts

#### Scenario: Second background start
- **WHEN** GameNode is already running and another instance is started with `--background`
- **THEN** the second instance exits without showing the window of the running one

### Requirement: Autostart at login
On Windows, the application SHALL register itself to start at login through a value under the current user's `Run` key while the "Start with Windows" preference is on, which it is by default. The value SHALL carry `--background` only while start minimized is on. It SHALL rewrite the value at each start with its current executable path and current arguments, and SHALL NOT override the user disabling the entry in Windows' startup settings. Turning the preference off SHALL remove the value. Uninstalling SHALL remove the value.

#### Scenario: First start after install
- **WHEN** the installed application starts for the first time
- **THEN** a `Run` value for the current user starts GameNode with `--background` at the next login, without administrator rights

#### Scenario: Disabled in Windows settings
- **WHEN** the user disables GameNode in Windows' Startup apps settings and GameNode starts again
- **THEN** GameNode remains disabled there and does not start at the next login

#### Scenario: Moved installation
- **WHEN** the application is started from a new location
- **THEN** the `Run` value points to the new executable

#### Scenario: Turn autostart off in Settings
- **WHEN** the user turns "Start with Windows" off
- **THEN** the `Run` value is removed immediately, it is not written again at later starts, and turning the preference on writes it again

#### Scenario: Start minimized changes
- **WHEN** the user turns "Start minimized to the tray" off while autostart is on
- **THEN** the `Run` value is rewritten without `--background`

#### Scenario: Uninstall
- **WHEN** the user runs the uninstaller
- **THEN** the `Run` value is removed
