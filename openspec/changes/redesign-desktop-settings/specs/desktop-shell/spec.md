# Spec Delta

## MODIFIED Requirements

### Requirement: English and Brazilian Portuguese
The application SHALL provide English and Brazilian Portuguese translations for every foundation screen, app menu, and user-facing status or error. The interface language SHALL follow the "Language" preference: System language (the default) selects the OS language when supported and otherwise English; English or Português (Brasil) selects that language regardless of the OS. Changing the preference SHALL switch the interface immediately without restarting the application or interrupting running games, and SHALL be kept across restarts.

#### Scenario: Brazilian Portuguese desktop
- **WHEN** the application starts with a Brazilian Portuguese OS locale and the language preference at System language
- **THEN** sign-in, the empty library, Settings/About, menus, and foundation errors use Brazilian Portuguese

#### Scenario: Unsupported language
- **WHEN** the application starts with an unsupported OS language and the language preference at System language
- **THEN** user-facing foundation text uses English without exposing translation identifiers

#### Scenario: Choose a language
- **WHEN** the OS is in English and the user chooses Português (Brasil) in Settings while a game is running
- **THEN** the open Settings modal, the library and the menus switch to Brazilian Portuguese at once, the game keeps being tracked, and the next start also uses Brazilian Portuguese

## ADDED Requirements

### Requirement: Status bar visibility preference
The user SHALL be able to hide or show the status bar with the "Show status bar" preference, which is on by default and kept across restarts. While it is off, the status bar SHALL NOT be rendered and the library SHALL use the freed height.

#### Scenario: Hide the status bar
- **WHEN** the user turns "Show status bar" off in Settings
- **THEN** the status bar disappears immediately and stays hidden after a restart

#### Scenario: Show it again
- **WHEN** the user turns "Show status bar" on
- **THEN** the status bar appears with its current information
