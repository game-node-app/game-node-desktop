# Spec Delta

## ADDED Requirements

### Requirement: Plugins declare bounded launch options

An extension SHALL be able to declare launch options in its signed manifest/configuration schema.
Each option SHALL have a stable id, a human label, a type, a default, and either allowed values or
bounds. An option SHALL be plugin-wide or per-game. Options SHALL NOT carry paths, handles,
executables, URIs or shell text. A plugin without options SHALL keep its current behavior.

#### Scenario: Apply a declared option

- **WHEN** a user changes a declared option and starts the game
- **THEN** the plugin receives the effective value, the resulting recipe is reviewed, and the
  summary shows the exact runtime and arguments

#### Scenario: Unsafe or out-of-bounds value

- **WHEN** a value exceeds its declared bounds or contains disallowed text
- **THEN** the host refuses it before the plugin/recipe runs and keeps the previous values

#### Scenario: Plugin without options

- **WHEN** an extension declares no launch options
- **THEN** the editor is absent and every existing recipe behaves as before

### Requirement: Launch options support per-game overrides

The host SHALL apply options in the order default → plugin-wide → per-game and SHALL let the user
reset overrides explicitly. Changing an effective value SHALL invalidate the saved recipe
selection or approved plan so the next Play shows the updated plan.

#### Scenario: Per-game override wins

- **WHEN** a plugin-wide value and a per-game value both exist
- **THEN** the per-game value is applied to that entry only and the other entries keep the
  plugin-wide value

#### Scenario: Changed value requires review

- **WHEN** an option changes after a plan was approved
- **THEN** the previous approval is stale, the next Play shows the updated command, and no launch
  happens without the new approval

#### Scenario: Reset to defaults

- **WHEN** the user resets the options
- **THEN** the defaults apply, overrides are cleared, and the changed plan requires review
