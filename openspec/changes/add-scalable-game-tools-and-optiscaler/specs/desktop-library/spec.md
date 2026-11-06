# Spec Delta

## MODIFIED Requirements

### Requirement: Context menus
Each entry SHALL have a context menu, opened by right-click or the keyboard menu key, with the actions available for that entry: Play, Install (omitted when the provider has no install action, such as DRM-free entries), Game tools (for installed entries on supported platforms), Match or Change match, and Add to GameNode. With several entries selected, the menu SHALL offer only the actions that work on several games.

#### Scenario: Context menu on one game
- **WHEN** the user opens the context menu on a matched, installed Steam game
- **THEN** Play, Game tools, Change match and Add to GameNode are offered

#### Scenario: Context menu on a DRM-free game
- **WHEN** the user opens the context menu on a DRM-free game
- **THEN** Play, Game tools, Game settings, Locate executable, Remove from library, Local save protection, Match or Change match, and Add to GameNode are offered, and Install is not shown

#### Scenario: Context menu on a multi-selection
- **WHEN** several games are selected
- **THEN** single-game actions such as Play, Game tools and Change match are not offered

### Requirement: Detail pane
Selecting one entry SHALL show a detail pane with its cover, detected and matched titles, provider, install folder, match status, whether it is running, active installed tools summary (when any game tool such as OptiScaler is installed), and the Play, Install, Game tools, match and Add to GameNode actions. When the entry's provider has no install action (such as DRM-free games), the Install button and its reason SHALL be omitted. Other actions SHALL be disabled with a reason when unavailable, including "Running" for Play and for mutating game tools.

#### Scenario: Unmatched game
- **WHEN** the selected game has no GameNode match
- **THEN** the pane offers Match, Play and Game tools stay available, and Add to GameNode is disabled until a match is chosen

#### Scenario: Running game
- **WHEN** the selected game is running
- **THEN** the pane shows Running, Play is disabled with a reason saying the game is already running, and opening Game tools disables install, update and uninstall actions until the game exits

#### Scenario: DRM-free game details omit Install
- **WHEN** a DRM-free game is selected
- **THEN** the detail pane shows Play, Game tools and DRM-free actions without rendering an Install button or a "no install action" caption

#### Scenario: Game with OptiScaler installed
- **WHEN** the selected game has OptiScaler installed
- **THEN** the detail pane displays the installed tool summary (such as `OptiScaler v0.7.9 (dxgi.dll)`) and opening `Game tools…` shows the installed state, version switcher, proxy DLL selector, and `Uninstall` action
