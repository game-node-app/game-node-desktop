# Spec Delta

## MODIFIED Requirements

### Requirement: Foundation shell and navigation
The application SHALL provide a native window with an app menu, a signed-in user indicator, the game library, and Settings/About. The signed-in main view SHALL be the library described by `desktop-library`. Settings/About SHALL open as the modal described by `desktop-settings`, over the main view, without replacing it. The app menu SHALL include Rescan. Quit in the app menu SHALL stop the application, while closing the window follows `desktop-lifecycle`.

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

#### Scenario: Settings keep the main view
- **WHEN** the user opens Settings/About from the sidebar or the app menu while the library is shown
- **THEN** the library stays in place behind the settings modal and is shown unchanged when the modal closes

### Requirement: GameNode Version 3.0 theme
The shell SHALL use the Version 3.0 values and source selection recorded in `references/figma-version-3-tokens.json`, bundled Roboto, and GameNode logos. The settings modal SHALL use its own token set, recorded with the source node or variable from Figma section `4987:10202`. Theme values SHALL retain source provenance and documented desktop adaptations. Figma layouts SHALL guide visual treatment without adding features beyond roadmap step 1.

#### Scenario: Review the theme against its source
- **WHEN** the sign-in shell, empty library, menus, and Settings/About are reviewed
- **THEN** their theme values can be traced to a recorded Version 3.0 source node/style, a recorded settings-modal source node/variable, or an explicitly documented desktop adaptation
- **AND** Roboto and brand assets remain available without a network connection

#### Scenario: Reference screens show later or excluded features
- **WHEN** a Version 3.0 or settings-modal reference includes social content, premium plans, profile editing, or populated game controls
- **THEN** only its applicable visual treatment is used in the desktop shell
- **AND** those features are absent from the application
