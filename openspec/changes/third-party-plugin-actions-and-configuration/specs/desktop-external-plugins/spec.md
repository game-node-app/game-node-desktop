# Spec Delta

## ADDED Requirements

### Requirement: Plugin game actions require host-issued user intent
The host MUST bind plugin launch and installation requests to a valid, short-lived user intent or an explicit confirmation, and MUST reject stale, reused, foreign or target-revision-mismatched intents.

#### Scenario: Missing intent waits for confirmation
- **WHEN** a plugin requests launch or installation without a valid intent
- **THEN** the host does not perform the action and exposes an explicit confirmation job

### Requirement: Filesystem scope is approved by the host
The host MUST collect explicit user approval for a plugin's selected folder and operations, persist only the approved scope, and fence access when approval is revoked.

#### Scenario: Scope is revoked during use
- **WHEN** a user revokes a folder approval
- **THEN** new access is denied and existing handles or queued operations are fenced

### Requirement: Plugin configuration remains isolated
The host MUST route Configure to an isolated plugin-owned window or an explicitly authorized closed-schema settings form, scoped to the active plugin and account profile.

#### Scenario: Account profile changes
- **WHEN** the active account changes while a plugin window is open
- **THEN** the old window context is fenced before another profile is exposed
