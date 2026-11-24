## Purpose

Extend the official Epic Games Store plugin to the native Linux launcher preview while preserving
the Windows provider.

## ADDED Requirements

### Requirement: Native Linux Epic launcher discovery

The Linux Epic provider SHALL discover installed games from the native launcher's recorded install
data using a bounded, read-only adapter. It SHALL exclude DLC, incomplete installs and engine builds
like the Windows adapter. A missing launcher SHALL be a successful empty scan; an unknown or changed
launcher layout SHALL fail only the Epic provider with an explicit diagnostic and SHALL preserve
previously discovered records.

#### Scenario: An installed Epic game

- **WHEN** the native launcher records a complete installed game
- **THEN** the provider returns a stable `epic:<appName>` record with its install location and
  expected fields

#### Scenario: The launcher is not installed

- **WHEN** no native launcher or install data exists
- **THEN** the scan succeeds with an empty result and does not clear other providers

#### Scenario: The preview layout changes

- **WHEN** the launcher's install data is present but its layout is not recognized
- **THEN** the Epic provider reports the layout error, keeps its previous records and does not
  invent installations from arbitrary folders

### Requirement: Linux Epic account, catalog and playtime

The Linux provider SHALL use the existing authorization-code WebView sign-in, per-provider secret
slot, refresh-token rotation, owned-library sync, namespace matching and `epicgames` playtime
counter. It SHALL NOT read the launcher's cookies, tokens or private WebView storage.

#### Scenario: Sign in on Linux

- **WHEN** the user signs in through the isolated Epic WebView
- **THEN** the refresh token is stored in the provider's secret slot, the access token remains in
  memory, and the generic account card shows the connected account

#### Scenario: Catalog and playtime after restart

- **WHEN** the app restarts with a stored Epic refresh token
- **THEN** the account is restored, the owned catalog syncs and Epic playtime totals are submitted
  with source `epicgames`, including the unknown-last-played case

### Requirement: Linux Epic actions require a registered launcher URI

Play and Install SHALL use the reviewed `com.epicgames.launcher://` handoff. The host SHALL verify
that the scheme has a desktop handler before opening it. If no handler is registered, the action
SHALL return an actionable integration error and SHALL NOT spawn the AppImage, Wine/Proton or an
arbitrary executable.

#### Scenario: The desktop handler is registered

- **WHEN** Play or Install is requested and `com.epicgames.launcher` resolves to the launcher
- **THEN** the registered launcher opens the reviewed launch/install URI and owns Proton execution

#### Scenario: The desktop handler is missing

- **WHEN** Play or Install is requested without a registered handler
- **THEN** the action is refused with guidance to integrate the AppImage/desktop entry and no
  process is started

### Requirement: Linux discovery is bounded and does not read private launcher state

The Linux adapter SHALL enforce entry, record and file-size bounds and SHALL read only installation
metadata. It SHALL not read cookies, tokens, IndexedDB/localStorage contents or credential files.

#### Scenario: Oversized or malformed install data

- **WHEN** a manifest exceeds the bounds or cannot be parsed
- **THEN** the provider reports the explicit failure without exposing private launcher data

### Requirement: Windows Epic behavior remains unchanged

The Linux provider SHALL NOT change the Windows discovery roots, actions, packaging or acceptance.

#### Scenario: Windows build

- **WHEN** the Epic provider runs on Windows
- **THEN** its registry/ProgramData discovery, URI actions, package layout and existing Windows
  acceptance behavior are unchanged
