# Spec Delta

## MODIFIED Requirements

### Requirement: GameNode Version 3.0 theme
The shell SHALL use the Version 3.0 values and source selection recorded in `references/figma-version-3-tokens.json`, bundled Roboto, and GameNode logos, structured through standardized Tailwind utility tokens and Mantine theme configurations shared with `game-node-clients`. The settings modal and shell dialogs SHALL use their own token sets, recorded with the source node or variable from Figma section `4987:10202`. Theme values SHALL retain source provenance and documented desktop adaptations. Figma layouts SHALL guide visual treatment without adding features beyond roadmap step 1.

#### Scenario: Review the theme against its source
- **WHEN** the sign-in shell, empty library, menus, and Settings/About are reviewed
- **THEN** their theme values can be traced to a recorded Version 3.0 source node/style, a recorded settings-modal source node/variable, or an explicitly documented desktop adaptation
- **AND** Roboto and brand assets remain available without a network connection

#### Scenario: Reference screens show later or excluded features
- **WHEN** a Version 3.0 or settings-modal reference includes social content, premium plans, profile editing, or populated game controls
- **THEN** only its applicable visual treatment is used in the desktop shell

## ADDED Requirements

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
