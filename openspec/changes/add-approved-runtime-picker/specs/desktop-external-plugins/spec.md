# Spec Delta

## ADDED Requirements

### Requirement: Approved emulator runtime selection

The runtime picker SHALL suggest candidates only from host-owned installed-program sources —
PackageKit on Linux and the registry/Start Menu program list on Windows — matched against the
extension's text hints. It SHALL always offer manual Browse. The user SHALL see the requesting
plugin/publisher, runtime name and exact executable path before approval. Third-party providers
SHALL NOT contribute candidates. A changed executable SHALL require fresh approval without
falling back to another path.

#### Scenario: PackageKit suggests Dolphin

- **WHEN** the Dolphin extension requests a runtime with the hint `dolphin-emu` and the official
  PackageKit inventory has that executable
- **THEN** the picker shows Dolphin Emulator with the exact `/usr/bin/dolphin-emu` path and the
  package id, and the user can approve it or browse for another file

#### Scenario: Windows program list

- **WHEN** the same extension requests a runtime on Windows and the registry/Start Menu inventory
  has a matching program
- **THEN** the picker shows that program with its exact executable path and manual Browse remains
  available

#### Scenario: Third-party candidate rejected

- **WHEN** a third-party provider reports a path that matches the extension's hint
- **THEN** that path is not offered as a suggestion and only the host-owned inventory or a manual
  user choice can be approved

#### Scenario: Executable changed after approval

- **WHEN** the approved executable is replaced, moved or its digest changes
- **THEN** the runtime requires a fresh approval and the host does not launch another path
