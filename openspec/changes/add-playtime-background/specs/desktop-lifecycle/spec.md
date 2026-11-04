# Spec Delta

## ADDED Requirements

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

## REMOVED Requirements

### Requirement: Exit behavior for the foundation
**Reason**: Step 3 adds background mode. On Windows, closing the window no longer stops the application, and the application registers autostart and keeps a tray icon while it runs.
**Migration**: Use "Close returns to the background", "Quit ends the application", "Start in the background", "Autostart at login" and "Tray icon". Quit in the app or tray menu stops the application; Linux keeps step 1's close-to-quit behavior until the Linux milestone.
