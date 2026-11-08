# Spec Delta

## MODIFIED Requirements

### Requirement: Foundation shell and navigation
The application SHALL provide a native window with an app menu, a signed-in user indicator, the game library, the PC ports catalog, and Settings/About. The signed-in application SHALL open on the library described by `desktop-library`. PC ports SHALL show the catalog described by `desktop-port-catalog` in the same content area. Settings/About SHALL open as the modal described by `desktop-settings`, over the active view, without replacing it. The app menu SHALL include Rescan. Quit in the app menu SHALL stop the application, while closing the window follows `desktop-lifecycle`.

#### Scenario: Signed-in foundation
- **WHEN** a valid session becomes available
- **THEN** the top bar identifies the signed-in user and the main view displays the library of installed games
- **AND** the user can open PC ports from the sidebar, open Settings/About, rescan, and invoke sign-out or Quit through the app menu

#### Scenario: Signed-out foundation
- **WHEN** the application has no usable session
- **THEN** it offers browser sign-in and access to Settings/About
- **AND** it does not display a signed-in user's identity or the PC ports destination

#### Scenario: Quit versus close
- **WHEN** the user chooses Quit in the app menu on Windows
- **THEN** the application stops, whereas closing the window would have kept it running in the background

#### Scenario: Settings keep the main view
- **WHEN** the user opens Settings/About from the sidebar or the app menu while the library or PC ports is shown
- **THEN** that view stays in place behind the settings modal and is shown unchanged when the modal closes

### Requirement: Keyboard access
The shell SHALL provide keyboard access to sign-in, app navigation, including the sidebar destinations, Settings controls, sign-out, and Quit with visible focus.

#### Scenario: Complete foundation navigation by keyboard
- **WHEN** the user operates the application with the keyboard
- **THEN** they can reach and activate every foundation action and identify the focused control

#### Scenario: Reach PC ports by keyboard
- **WHEN** the user moves focus through the sidebar with the keyboard
- **THEN** All games, each provider and PC ports are focusable in order and activatable with Enter or Space

## ADDED Requirements

### Requirement: Explore group in the sidebar
The signed-in sidebar SHALL show an `EXPLORE` group below `YOUR SPACE` containing the PC ports destination. Exactly one sidebar destination SHALL be active at a time. Selecting PC ports SHALL show the catalog in the content area, and selecting All games or a provider SHALL return to the library with that filter. The active destination SHALL NOT be restored after restart. The group SHALL be shown only on Windows.

#### Scenario: Switch to PC ports
- **WHEN** the user selects PC ports in the sidebar
- **THEN** the content area shows the port catalog, PC ports is marked active, and All games and the providers are not

#### Scenario: Return to the library
- **WHEN** PC ports is active and the user selects Steam in the sidebar
- **THEN** the library is shown filtered to Steam and the library's previous view, sort and selection are preserved

#### Scenario: Restart
- **WHEN** the user quits while PC ports is active and starts the application again
- **THEN** it opens on the library

#### Scenario: Linux build
- **WHEN** the application runs on Linux
- **THEN** the sidebar shows no `EXPLORE` group, because port installs are Windows-only in this release

### Requirement: Port install activity badge
The PC ports sidebar item SHALL show a badge counting running port installs plus installs that finished, failed or were interrupted and that the user has not seen yet. Opening the affected entry in PC ports SHALL mark its result as seen. The badge SHALL have an accessible label stating the count.

#### Scenario: One running, one failed
- **WHEN** one install is running and another failed while the user was in the library
- **THEN** the PC ports item shows a badge with the count two

#### Scenario: Result seen
- **WHEN** the user opens PC ports and selects the failed entry
- **THEN** the badge count drops by one
