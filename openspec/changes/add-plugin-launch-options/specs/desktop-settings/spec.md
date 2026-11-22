# Spec Delta

## ADDED Requirements

### Requirement: Launch option editor

Settings › Plugins and the selected game's launch method SHALL render the extension's declared
launch options with their labels, defaults, bounds and impact text. The editor SHALL distinguish
plugin-wide values from per-game overrides, expose an explicit reset, and keep en/pt-BR, keyboard
and focus behavior.

#### Scenario: Edit a plugin-wide option

- **WHEN** the user opens Configure for an extension with declared launch options
- **THEN** the editor shows each option with its default and impact, and saving records the value
  for the active profile

#### Scenario: Add a per-game override

- **WHEN** the user opens the launch method for one installed game
- **THEN** the editor shows the effective value and lets the user override only that game

#### Scenario: Invalid input

- **WHEN** a value is outside its declared bounds or missing a required selection
- **THEN** the editor reports the exact option and does not save a partial configuration

#### Scenario: Reset

- **WHEN** the user resets the options
- **THEN** plugin-wide and per-game overrides are cleared, the defaults are shown, and the changed
  launch plan requires review
