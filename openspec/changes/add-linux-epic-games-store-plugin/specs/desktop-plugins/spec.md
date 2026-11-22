## ADDED Requirements

### Requirement: Epic plugin is packaged on Linux

The Linux development and release layouts SHALL build and stage `gamenode-plugin-epic` beside the
desktop, and Linux package verification SHALL require it. The Windows package requirements and the
existing Epic Windows layout SHALL remain unchanged.

#### Scenario: Linux package layout

- **WHEN** the Linux desktop package is built
- **THEN** the Epic plugin executable is present beside the host and passes package verification

#### Scenario: Windows package layout

- **WHEN** the Windows package is built
- **THEN** the existing Epic Windows executable and package requirements are unchanged
