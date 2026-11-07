# Spec Delta

## MODIFIED Requirements

### Requirement: Foundation shell and navigation
The application SHALL provide a native window with an app menu, a signed-in user indicator, two top-level sections named Biblioteca and Downloads, and Settings/About. The signed-in application SHALL open on Biblioteca, which contains the library described by `desktop-library`. Downloads SHALL contain the transfers described by `desktop-downloads` and, until the feature is enabled, its consent screen. Settings/About SHALL open as the modal described by `desktop-settings`, over the active section, without replacing it. The app menu SHALL include Rescan. Quit in the app menu SHALL stop the application, while closing the window follows `desktop-lifecycle`.

#### Scenario: Signed-in foundation
- **WHEN** a valid session becomes available
- **THEN** the top bar identifies the signed-in user and the main view opens on Biblioteca
- **AND** the sidebar exposes Biblioteca and Downloads, and the user can open Settings/About, rescan, and invoke sign-out or Quit through the app menu

#### Scenario: Signed-out foundation
- **WHEN** the application has no usable session
- **THEN** it offers browser sign-in and access to Settings/About
- **AND** it does not display a signed-in user's identity

#### Scenario: Quit versus close
- **WHEN** the user chooses Quit in the app menu on Windows
- **THEN** the application stops, whereas closing the window would have kept it running in the background

#### Scenario: Settings keep the main view
- **WHEN** the user opens Settings/About from the sidebar or the app menu while Biblioteca or Downloads is shown
- **THEN** the active section stays in place behind the settings modal and is shown unchanged when the modal closes

#### Scenario: Downloads before consent
- **WHEN** the user opens Downloads while the feature has no accepted consent
- **THEN** Downloads shows the consent screen described by `desktop-downloads` instead of a transfer list

## ADDED Requirements

### Requirement: Section navigation and activity badge
The sidebar SHALL present Biblioteca and Downloads as sibling destinations with exactly one active at a time. The Downloads item SHALL show a badge counting active transfers plus completed transfers whose handoff was not handled. Selecting a destination SHALL replace the content area without opening a new window. Both destinations SHALL be reachable and activatable by keyboard with visible focus.

#### Scenario: Switch sections
- **WHEN** the user selects Downloads and later Biblioteca
- **THEN** the content area changes to that section and the sidebar marks it active

#### Scenario: Active transfer badge
- **WHEN** one transfer is downloading and another completed transfer has not been handled
- **THEN** the Downloads sidebar item shows a badge with the count two

#### Scenario: Keyboard navigation
- **WHEN** the user reaches the section navigation with the keyboard
- **THEN** both destinations are focusable and activatable with visible focus

#### Scenario: Single window
- **WHEN** the user switches between Biblioteca and Downloads
- **THEN** the main window and the Settings modal state are preserved
