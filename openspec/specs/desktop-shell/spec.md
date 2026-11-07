# desktop-shell Specification

## Purpose

Provide the native GameNode Desktop shell that hosts authentication and foundation settings on Windows and Linux, using the shared brand without importing web product features.

## Requirements

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

### Requirement: Library strings are translated
Every library, detail pane, match and Add to GameNode screen, menu, status and error SHALL be available in English and Brazilian Portuguese, using the same language selection as the rest of the shell. Game titles, collection names and other data from GameNode or the store SHALL be shown as received.

#### Scenario: Brazilian Portuguese library
- **WHEN** the application runs with a Brazilian Portuguese OS locale
- **THEN** the library toolbar, sidebar, detail pane, dialogs and errors use Brazilian Portuguese, and game titles are unchanged

### Requirement: GameNode Version 3.0 theme
The shell SHALL use the Version 3.0 values and source selection recorded in `references/figma-version-3-tokens.json`, bundled Roboto, and GameNode logos, structured through standardized Tailwind utility tokens and Mantine theme configurations shared with `game-node-clients`. The settings modal and shell dialogs SHALL use their own token sets, recorded with the source node or variable from Figma section `4987:10202`. Theme values SHALL retain source provenance and documented desktop adaptations. Figma layouts SHALL guide visual treatment without adding features beyond roadmap step 1.

#### Scenario: Review the theme against its source
- **WHEN** the sign-in shell, empty library, menus, and Settings/About are reviewed
- **THEN** their theme values can be traced to a recorded Version 3.0 source node/style, a recorded settings-modal source node/variable, or an explicitly documented desktop adaptation
- **AND** Roboto and brand assets remain available without a network connection

#### Scenario: Reference screens show later or excluded features
- **WHEN** a Version 3.0 or settings-modal reference includes social content, premium plans, profile editing, or populated game controls
- **THEN** only its applicable visual treatment is used in the desktop shell

### Requirement: Standardized styling tokens and utility classes
The application shell and feature views SHALL define and apply visual presentation using Tailwind CSS utility classes synchronized with the design token configurations of `game-node-clients` (including brand palette, surface/paper tones, font scale, and border radii). Components SHALL NOT rely on ad-hoc inline `style` objects for structural layout, colors, or interactive states, except for dynamic runtime values such as virtualized list offsets and drag handle positions.

#### Scenario: Utility class styling verification
- **WHEN** inspecting the markup of shell, settings, and library components
- **THEN** structural layout and visual styling are applied through standardized utility classes rather than arbitrary inline styles
- **AND** interactive states (`hover`, `active`, `focus-visible`) respond immediately without custom script listeners

### Requirement: Accessible overlay dialogs and primitives
All modal windows, dialogs, and popup overlays (including settings, match confirmation, game tools, local game management, and sync explanations) SHALL use accessible overlay primitives that enforce portal mounting, keyboard focus trapping, and backdrop escape dismissal without obstructing the underlying desktop shell. Bespoke icon assets SHALL use standardized Tabler icon components.

#### Scenario: Opening and navigating overlay dialogs
- **WHEN** an overlay dialog or modal is opened via keyboard or mouse
- **THEN** keyboard focus is trapped within the dialog container
- **AND** pressing Escape dismisses the dialog and restores focus to the triggering element
- **AND** icons within the dialog render consistently with standard vector icon components

### Requirement: Remember window geometry
The application SHALL restore the saved normal window size, maximized state, and position where the window system permits positioning. Invalid or unavailable geometry SHALL fall back to usable geometry on an available display.

#### Scenario: Restart after resizing
- **WHEN** the user resizes or moves the window, exits, and reopens the application
- **THEN** its last normal size and maximized state are restored
- **AND** its position is restored where supported by the active window system

#### Scenario: Saved display is unavailable
- **WHEN** a saved position is outside the available displays or the compositor controls positioning
- **THEN** the application opens a reachable window without failing startup

### Requirement: Non-blocking asynchronous IPC between UI and Engine
The application shell SHALL execute all backend commands (`#[tauri::command]`) asynchronously on background worker threads and SHALL push state updates to the frontend through coalesced backend events (`engine:snapshot`) synchronized with `@tanstack/react-query`. No file I/O, SQLite access, process inspection, network request, or image decoding SHALL run on the UI/main thread.

#### Scenario: Rapid state updates while interacting with the shell
- **WHEN** the backend engine emits several snapshot updates during a library scan or authentication verification
- **THEN** the UI receives coalesced snapshot updates without blocking user input, menu navigation, or window resizing

### Requirement: English and Brazilian Portuguese
The application SHALL provide English and Brazilian Portuguese translations for every foundation screen, app menu, and user-facing status or error. It SHALL select the OS language when supported and otherwise use English.

#### Scenario: Brazilian Portuguese desktop
- **WHEN** the application starts with a Brazilian Portuguese OS locale
- **THEN** sign-in, the empty library, Settings/About, menus, and foundation errors use Brazilian Portuguese

#### Scenario: Unsupported language
- **WHEN** the application starts with an unsupported OS language
- **THEN** user-facing foundation text uses English without exposing translation identifiers

### Requirement: About and credential-storage disclosure
Settings/About SHALL display the application version, the repository's GPL-3.0-only license, and the bundled dependencies' and assets' license notices. Settings SHALL identify the active session credential-storage method, including a clear disclosure when Linux uses a user-only file.

#### Scenario: About is available offline
- **WHEN** the user opens Settings/About without network access
- **THEN** the installed version and bundled license information are available

#### Scenario: Linux fallback is active
- **WHEN** Linux has no Secret Service and session persistence uses the fallback file
- **THEN** Settings explains that the session token is stored in a file readable only by the current user

### Requirement: Keyboard access
The shell SHALL provide keyboard access to sign-in, app navigation, Settings controls, sign-out, and Quit with visible focus.

#### Scenario: Complete foundation navigation by keyboard
- **WHEN** the user operates the application with the keyboard
- **THEN** they can reach and activate every foundation action and identify the focused control

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

### Requirement: Playtime source control
Settings SHALL expose the global Store/Internal source choice with visible keyboard focus,
immediate application and explanatory text that games without store counters use Internal.

#### Scenario: Select Internal with keyboard
- **WHEN** the user focuses the source choice and selects Internal by keyboard
- **THEN** the preference is applied and persisted immediately
