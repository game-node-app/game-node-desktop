# Spec Delta

## ADDED Requirements

### Requirement: Playtime and background strings are translated
The tray menu and tooltip, the Running indicator, the sync setting and its explanation, the Store/Internal preference and its help, and playtime errors SHALL be available in English and Brazilian Portuguese, using the same language selection as the rest of the shell. Game titles in the tray tooltip SHALL be shown as received.

#### Scenario: Brazilian Portuguese tray
- **WHEN** the application runs in the background with a Brazilian Portuguese OS locale
- **THEN** the tray menu's Open and Quit items and its tooltip are in Brazilian Portuguese, and the running game's title is unchanged

### Requirement: Sync controls are keyboard accessible
The "Sync with GameNode" switch in Settings and the actions of its first-run explanation SHALL be reachable and operable by keyboard with visible focus, and the explanation SHALL trap focus and close with Escape like the other overlays.

#### Scenario: Keep sync on by keyboard
- **WHEN** the first-run explanation is shown and the user presses Tab and Enter
- **THEN** focus moves between its actions visibly and the chosen action is applied

## MODIFIED Requirements

### Requirement: Foundation shell and navigation
The application SHALL provide a native window with an app menu, a signed-in user indicator, the game library, and Settings/About. The signed-in main view SHALL be the library described by `desktop-library`. The app menu SHALL include Rescan. Quit in the app menu SHALL stop the application, while closing the window follows `desktop-lifecycle`.

#### Scenario: Signed-in foundation
- **WHEN** a valid session becomes available
- **THEN** the top bar identifies the signed-in user and the main view displays the library of installed games
- **AND** the user can open Settings/About, rescan, and invoke sign-out or Quit through the app menu

#### Scenario: Signed-out foundation
- **WHEN** the application has no usable session
- **THEN** it offers browser sign-in and access to Settings/About
- **AND** it does not display a signed-in user's identity

#### Scenario: Quit versus close
- **WHEN** the user chooses Quit in the app menu on Windows
- **THEN** the application stops, whereas closing the window would have kept it running in the background

## ADDED Requirements

### Requirement: Playtime source control
Settings SHALL expose the global Store/Internal source choice with visible keyboard focus,
immediate application and explanatory text that games without store counters use Internal.

#### Scenario: Select Internal with keyboard
- **WHEN** the user focuses the source choice and selects Internal by keyboard
- **THEN** the preference is applied and persisted immediately
