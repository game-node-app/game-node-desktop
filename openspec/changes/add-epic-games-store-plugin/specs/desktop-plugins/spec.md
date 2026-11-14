# Spec Delta

## Purpose

Host official provider plugins through an authenticated local runtime with an independent public boundary, a supervised lifecycle, trusted inventory and packaging, and isolated failure behavior.

## ADDED Requirements

### Requirement: Trusted plugin descriptors are bundled data
Each official plugin SHALL be described by a bundled descriptor validated at startup. The descriptor SHALL carry provider and plugin ids, name, platforms, grants, executable, provider manifest, login policy, credential alias, native-access declarations and legacy login profiles. Host code SHALL NOT hold per-plugin constants or branches.

#### Scenario: Steam, package manager and Epic descriptors
- **WHEN** the desktop starts
- **THEN** it loads the bundled descriptors that apply to the current platform, and inventory, backends and grants come only from them

#### Scenario: Invalid descriptor
- **WHEN** a bundled descriptor fails validation, for example because its manifest id differs, it requests unknown grants or its login policy does not match its login grant
- **THEN** that plugin is skipped with a diagnostic and every other plugin still loads

#### Scenario: Descriptor not on this platform
- **WHEN** a descriptor does not list the current platform
- **THEN** its provider, executable and inventory entry are absent

### Requirement: Multiple store plugins run independently
The host SHALL supervise each official plugin as its own process and session. One plugin's crash, timeout, missing executable or account failure SHALL NOT affect the operations, credentials or account state of another plugin.

#### Scenario: Epic crashes during sync
- **WHEN** the Epic plugin exits while syncing its catalog
- **THEN** Epic reports the failure and restarts under supervision, while Steam discovery, sign-in and playtime continue unaffected

#### Scenario: Concurrent sign-in attempts
- **WHEN** Steam and Epic sign-in windows are open at the same time
- **THEN** each window and its result are bound to its own provider, and closing one does not cancel the other

### Requirement: Packaged Epic plugin on Windows
The Windows development and release workflows SHALL build the Epic executable and place it next to the host, using the same discovery rules as other official plugins. Linux packages SHALL NOT include it.

#### Scenario: Windows release
- **WHEN** the Windows package is installed, including under a path with spaces
- **THEN** the host launches the adjacent Epic executable and establishes an authenticated session

#### Scenario: Linux release
- **WHEN** the Linux package is built
- **THEN** it contains no Epic executable and the package verifier does not require one
