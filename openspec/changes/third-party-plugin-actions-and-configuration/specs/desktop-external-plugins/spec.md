# Spec Delta

## ADDED Requirements

### Requirement: Plugin game actions require host-issued user intent
The host MUST bind plugin launch and installation requests to a valid, short-lived user intent or an explicit confirmation, and MUST reject stale, reused, foreign or target-revision-mismatched intents.

#### Scenario: Missing intent waits for confirmation
- **WHEN** a plugin requests launch or installation without a valid intent
- **THEN** the host does not perform the action and exposes an explicit confirmation job

#### Scenario: Core starts an extension for a selected game
- **WHEN** the user selects **Start with extension** for a game and chooses an installed, trusted, enabled extension and Play or Install
- **THEN** Core resolves the installation and target, starts a supervised one-shot `Extension` session scoped to that game, and passes a single-use intent bound to the plugin, publisher, active profile epoch, installation, operation and current target revision as `GAMENODE_PLUGIN_USER_INTENT_ID`

#### Scenario: Extension action is restricted to the Core-selected target
- **WHEN** an extension spends the startup intent with a different plugin, publisher, profile, game, installation, operation or target revision
- **THEN** the intent is rejected and the host follows the explicit-confirmation flow; the extension cannot substitute a target supplied in its own session request

#### Scenario: No eligible extension is available
- **WHEN** the selected game has no installed, trusted and enabled extension with the requested action grant
- **THEN** Core does not start an extension process or issue an intent

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
