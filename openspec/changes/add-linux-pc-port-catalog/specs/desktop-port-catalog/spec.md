## Purpose

Expose the PC ports catalog on Linux for discovery before native installs exist, with explicit
platform availability and blocked Windows-only entries. This adds requirements to the catalog
delivered by `add-pc-port-catalog`; it does not replace that change.

## ADDED Requirements

### Requirement: Linux catalog is visible with a Coming Soon label

A signed-in Linux build SHALL show the PC ports destination and load the catalog read-only. The
destination and the catalog header SHALL carry an “Em breve”/“Coming Soon” label. Search, filters,
grid/list and the detail pane SHALL remain usable without starting an install.

#### Scenario: Signed-in Linux host opens PC ports

- **WHEN** a signed-in Linux build selects PC ports
- **THEN** the catalog opens with the Coming Soon label and the entries are browsable, while every
  Install action is disabled

#### Scenario: Signed-out or non-Linux build

- **WHEN** the build is signed out or is not Linux/Windows
- **THEN** the destination follows the existing visibility rules and no unsupported install path is
  offered

### Requirement: Platform availability comes from the selected release

The host SHALL derive per-entry platform availability from the selected release's verified assets
and the recipe's tool requirements. `supportedOS` SHALL be a discovery hint only. An entry with no
asset for the running platform SHALL be reported blocked with a reason instead of being treated as
installable.

#### Scenario: Upstream claims Linux but the release has no Linux asset

- **WHEN** an entry lists Linux in `supportedOS` but the selected release contains only Windows
  assets
- **THEN** the catalog marks it Windows-only/blocked on Linux and never offers its Windows asset
  for installation

#### Scenario: Release has a native Linux asset

- **WHEN** the selected release contains a supported Linux asset and the recipe has no
  Linux-unavailable tool requirement
- **THEN** the catalog marks the entry as native Linux and eligible for the phase-2 install path

### Requirement: Linux platform filter separates native and Windows-only entries

The Linux catalog SHALL provide a persisted platform filter with **All** (the default),
**Native Linux** and **Windows-only**. An unknown persisted value SHALL fall back to All. The filter
SHALL NOT change which entries the upstream returns; it only narrows the displayed list.

#### Scenario: Filter native Linux

- **WHEN** the user selects Native Linux
- **THEN** only entries with a Linux asset and a Linux-compatible recipe remain visible, and the
  selection persists across restarts

#### Scenario: Filter Windows-only

- **WHEN** the user selects Windows-only on Linux
- **THEN** entries without a Linux asset remain visible and blocked, with the Windows-only/Proton
  explanation

### Requirement: Blocked entries remain browsable and never install

A blocked entry SHALL remain selectable. Its detail pane SHALL show the platforms it supports and
why Install is unavailable: the Coming Soon note in phase 1, or the Windows-only/Proton note in
phase 2. Selecting, searching, filtering or opening the detail of a blocked entry SHALL NOT start a
download, extraction, process launch or library registration.

#### Scenario: Install is blocked in phase 1

- **WHEN** the user opens any entry on Linux in phase 1 and attempts to install
- **THEN** the action is disabled with the Coming Soon reason and no install work starts

#### Scenario: Windows-only entry in phase 2

- **WHEN** the user opens an entry with no Linux asset after native Linux installs are enabled
- **THEN** the entry remains visible and blocked, and the detail explains that Proton is future
  work
