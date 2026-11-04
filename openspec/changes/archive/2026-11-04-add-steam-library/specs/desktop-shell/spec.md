# Spec Delta

## MODIFIED Requirements

### Requirement: Foundation shell and navigation
The application SHALL provide a native window with an app menu, a signed-in user indicator, the game library, and Settings/About. The signed-in main view SHALL be the library described by `desktop-library`. The app menu SHALL include Rescan.

#### Scenario: Signed-in foundation
- **WHEN** a valid session becomes available
- **THEN** the top bar identifies the signed-in user and the main view displays the library of installed games
- **AND** the user can open Settings/About, rescan, and invoke sign-out or Quit through the app menu

#### Scenario: Signed-out foundation
- **WHEN** the application has no usable session
- **THEN** it offers browser sign-in and access to Settings/About
- **AND** it does not display a signed-in user's identity or library

## ADDED Requirements

### Requirement: Library strings are translated
Every library, detail pane, match and Add to GameNode screen, menu, status and error SHALL be available in English and Brazilian Portuguese, using the same language selection as the rest of the shell. Game titles, collection names and other data from GameNode or the store SHALL be shown as received.

#### Scenario: Brazilian Portuguese library
- **WHEN** the application runs with a Brazilian Portuguese OS locale
- **THEN** the library toolbar, sidebar, detail pane, dialogs and errors use Brazilian Portuguese, and game titles are unchanged
