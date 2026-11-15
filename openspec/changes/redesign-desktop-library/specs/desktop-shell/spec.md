# Spec Delta

## MODIFIED Requirements

### Requirement: Foundation shell and navigation
The application SHALL provide a native window drawn without the OS title bar, with its own top bar, sidebar, main view and status bar. The top bar SHALL show the GameNode wordmark, a command-search field, the GameNode sync status, the signed-in user's account menu, and minimize, maximize or restore, and close controls; empty top-bar space SHALL move the window and double-clicking it SHALL maximize or restore. The signed-in main view SHALL be the library described by `desktop-library`. Settings/About SHALL open as the modal described by `desktop-settings`, over the main view, without replacing it. The app menu SHALL include Add a game, Rescan, Settings, Keyboard shortcuts, About and licenses, Sign out and Quit. Quit in the app menu SHALL stop the application, while closing the window, including with the top bar's close control, follows `desktop-lifecycle`.

#### Scenario: Signed-in foundation
- **WHEN** a valid session becomes available
- **THEN** the top bar identifies the signed-in user and the main view displays the library of installed games
- **AND** the user can open Settings/About, rescan, open the keyboard shortcuts, and invoke sign-out or Quit through the app menu

#### Scenario: Signed-out foundation
- **WHEN** the application has no usable session
- **THEN** it offers browser sign-in and access to Settings/About
- **AND** it does not display a signed-in user's identity

#### Scenario: Quit versus close
- **WHEN** the user chooses Quit in the app menu on Windows
- **THEN** the application stops, whereas closing the window with the top bar's close control would have kept it running in the background

#### Scenario: Settings keep the main view
- **WHEN** the user opens Settings/About from the sidebar or the app menu while the library is shown
- **THEN** the library stays in place behind the settings modal and is shown unchanged when the modal closes

#### Scenario: Move and maximize the window
- **WHEN** the user drags empty top-bar space, then double-clicks it
- **THEN** the window moves with the pointer, then toggles between maximized and its normal size, and the maximize control's icon and label follow the state

### Requirement: GameNode Version 3.0 theme
The shell SHALL use the Version 3.0 values and source selection recorded in `references/figma-version-3-tokens.json`, bundled Roboto, and GameNode logos, structured through standardized Tailwind utility tokens and Mantine theme configurations shared with `game-node-clients`. The settings modal and shell dialogs SHALL use their own token sets, recorded with the source node or variable from Figma section `4987:10202`. The library, top bar, sidebar, status bar, menus, palette, notifications and their states SHALL follow the desktop library redesign (Figma section `5361:766` and components `5357:677`), whose additional tokens are recorded with their source variables. Theme values SHALL retain source provenance and documented desktop adaptations. Design annotations in the references, such as "new" markers, SHALL NOT be shipped.

#### Scenario: Review the theme against its source
- **WHEN** the sign-in shell, library, menus, and Settings/About are reviewed
- **THEN** their theme values can be traced to a recorded Version 3.0 source node/style, a recorded settings-modal or library-redesign source node/variable, or an explicitly documented desktop adaptation
- **AND** Roboto and brand assets remain available without a network connection

#### Scenario: Reference screens show later or excluded features
- **WHEN** a Version 3.0, settings-modal or library-redesign reference includes social content, premium plans, profile editing, sample data or design annotations
- **THEN** only its applicable visual treatment and the behavior specified by the desktop capabilities are used in the desktop shell

### Requirement: Keyboard access
The shell SHALL provide keyboard access to sign-in, app navigation, Settings controls, sign-out, and Quit with visible focus. It SHALL also provide global shortcuts: Ctrl+K command palette, Ctrl+, Settings, F5 rescan, Ctrl+Q quit, Ctrl+F library search, Ctrl+Shift+F filters, Ctrl+L grid or list, Ctrl+I details panel, Enter play, F favorite, Shift+F10 context menu, Ctrl+A select all, Shift+S sort direction, Esc clear selection, and ? keyboard shortcuts. Shortcuts that type characters SHALL NOT fire while a text field has focus.

#### Scenario: Complete foundation navigation by keyboard
- **WHEN** the user operates the application with the keyboard
- **THEN** they can reach and activate every foundation action and identify the focused control

#### Scenario: Shortcut while typing
- **WHEN** the library search has focus and the user types "f"
- **THEN** the letter is typed and the selected game's favorite does not change

#### Scenario: Toggle the details panel
- **WHEN** the user presses Ctrl+I with a game selected
- **THEN** the details panel closes, and pressing it again opens it

## ADDED Requirements

### Requirement: Status bar
A status bar SHALL show each connected store's state (synced with its relative time, checking, session expired, unreachable), the watched folders' last check, and the running game with its session time. Each store item SHALL use an icon and text, not color alone.

#### Scenario: Healthy and expired stores
- **WHEN** Steam synced two minutes ago and Epic's session expired
- **THEN** the status bar shows "Steam · synced 2 min ago" with a success icon and "Epic · session expired" with a warning icon

#### Scenario: Running game
- **WHEN** a game is running
- **THEN** the status bar shows its title and session time, updated every minute

### Requirement: Command palette
Ctrl+K or the top-bar search SHALL open a command palette that searches games (title), actions (add DRM-free game, rescan, search GameNode, keyboard shortcuts, toggle grid or list) and Settings items. Results SHALL be grouped, navigable with the arrow keys, and run with Enter; on a game, Enter SHALL play or open it and Shift+F10 SHALL open its context menu. The palette SHALL trap focus and close with Escape like other overlays.

#### Scenario: Play from the palette
- **WHEN** the user presses Ctrl+K, types "ember" and presses Enter on an installed game
- **THEN** the palette closes and the game is launched

#### Scenario: Open a setting
- **WHEN** the user types "family" and picks Settings › Stores › Family member
- **THEN** Settings opens on that topic

#### Scenario: Search GameNode from the palette
- **WHEN** no game matches the text and the user picks Search “text” on GameNode
- **THEN** the catalog view opens with the text in its search

### Requirement: Keyboard shortcuts dialog
Pressing ? outside a text field, or choosing Keyboard shortcuts in the app menu or palette, SHALL open a dialog listing the shortcuts by General, Library and Selected game, with their keys.

#### Scenario: Open the shortcuts
- **WHEN** the user presses ? in the library
- **THEN** the keyboard shortcuts dialog opens and Escape closes it with focus restored

### Requirement: Notifications
Notifications SHALL appear in a corner region announced to assistive technology, in success, danger, info or progress tones, with an optional body, up to two actions and a progress bar. Success and info notifications SHALL close by themselves after 6 seconds unless hovered or focused; danger and progress notifications SHALL stay until closed or finished.

#### Scenario: Add with Undo
- **WHEN** an add to GameNode completes
- **THEN** a success notification names how many games were added and offers Undo and View on GameNode

#### Scenario: Launch failure
- **WHEN** a game fails to start because its executable is missing
- **THEN** a danger notification says so with Locate executable and View log, and stays until closed
