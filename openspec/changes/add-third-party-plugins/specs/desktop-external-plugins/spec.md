# Spec Delta

## Purpose

Manage third-party native plugin packages, publisher identity, execution and updates independently from bundled official providers.

## ADDED Requirements

### Requirement: Managed package installation
The application SHALL install third-party plugins only through explicit package import into its managed per-user registry. Packages SHALL be validated before execution and installed disabled until the user approves activation. Arbitrary files dropped outside this flow SHALL NOT be executed as plugins.

#### Scenario: Import a valid package
- **WHEN** a user imports a valid package for this platform
- **THEN** its metadata, license and requested permissions are shown before activation and no executable runs during validation

#### Scenario: Reject an unsafe archive
- **WHEN** an archive contains traversal, absolute paths, links, duplicate or case-colliding entries, excessive expansion or an unsupported architecture
- **THEN** installation is rejected without changing the active version or writing outside staging

### Requirement: Versioned manifest and integrity contract
Packages SHALL declare stable plugin/provider identity, publisher key, SemVer version, API range, target entrypoints, required/optional scoped permissions, native access and bundled license notices. The host SHALL verify a publisher signature and hashes covering all payload files and reject unsupported schema versions or unlisted payloads.

#### Scenario: Modified executable
- **WHEN** any packaged executable or manifest differs from its signed integrity metadata
- **THEN** installation or launch is refused with an integrity failure

#### Scenario: Missing executable
- **WHEN** the package has no verified entrypoint for the active OS and architecture
- **THEN** the plugin is reported incompatible and no substitute binary is executed

### Requirement: Publisher identity and trust
The host SHALL require explicit initial trust of the publisher key fingerprint and pin it to the installed plugin identity. A signature SHALL NOT imply official status, safe code or legal certification. Publisher-key changes SHALL require separate user re-trust before activation.

#### Scenario: A different key signs an update
- **WHEN** a package with an installed plugin id is signed by an unpinned key
- **THEN** automatic replacement is refused and the current verified version remains active

### Requirement: Independent lifecycle and failure isolation
The user SHALL be able to enable, disable and remove each third-party plugin persistently. Disable SHALL revoke its sessions and stop supervised processes with a bounded grace period. A failed or disabled plugin SHALL NOT erase library history, start new work, or disable official providers.

#### Scenario: Disable a running plugin
- **WHEN** the user disables a plugin during a scan
- **THEN** new API work is refused, supervised execution is stopped and the previous successful library facts remain

#### Scenario: Plugin crashes
- **WHEN** a third-party executable exits unexpectedly
- **THEN** its status reports failure without an unbounded restart loop and official providers continue operating

#### Scenario: Remove installed plugin
- **WHEN** the user removes a plugin
- **THEN** its sessions, packages and grants are removed, library history is preserved and private configuration/secret deletion is a separate choice

### Requirement: Verified consented updates
The host SHALL check a plugin's configured HTTPS update feed using its pinned publisher key, metadata expiry and anti-replay sequence. Updates SHALL require user approval, platform/API compatibility and full package validation. New permissions or wider scopes SHALL NOT be granted implicitly.

#### Scenario: Update requests additional access
- **WHEN** an update requests a new permission or wider resource scope
- **THEN** the review shows the difference and activation requires approval of any newly required access

#### Scenario: Expired or replayed feed
- **WHEN** update metadata is expired, has an older sequence or has an invalid signature
- **THEN** it is rejected while the installed version remains usable

#### Scenario: Offline update check
- **WHEN** the update feed cannot be reached
- **THEN** the user sees an update-check failure and the active plugin is not disabled solely for that failure

### Requirement: Atomic activation and rollback
Installation and update SHALL activate only a fully validated version. Interrupted activation SHALL recover deterministically. A failed readiness check SHALL restore the previous verified version with current grants intersected with its own manifest, never older broader grants. Package updates SHALL NOT execute installer hooks.

#### Scenario: Updated process fails readiness
- **WHEN** the candidate process fails activation
- **THEN** the previous verified version is restored with current permission restrictions and an update failure is displayed

#### Scenario: Application stops during activation
- **WHEN** startup detects an incomplete package transaction
- **THEN** it selects a fully verified committed version or leaves the plugin inactive, without executing partially extracted content

### Requirement: License disclosure and bounded compliance checks
Each package SHALL provide an SPDX expression or a documented custom LicenseRef, local license texts and applicable third-party notices. The host SHALL reject missing or malformed required license metadata, display notices offline and allow proprietary third-party licenses at the public process/API boundary without claiming automatic legal compliance certification.

#### Scenario: Proprietary plugin with notices
- **WHEN** an otherwise valid independently distributed plugin declares a proprietary LicenseRef with included terms
- **THEN** its terms can be reviewed offline and it is not rejected solely because the Core uses GPL

#### Scenario: License file absent
- **WHEN** a manifest references a missing license or notice file
- **THEN** package validation fails with an actionable license-metadata error

### Requirement: Provider and extension package kinds
A managed package SHALL declare whether it owns a provider or acts as an extension without one. Extensions SHALL use their own plugin identity and approved resource contexts. They SHALL be able to contribute custom launch recipes without creating a store, reusing an official provider id or altering official package metadata.

#### Scenario: Install a launch-only extension
- **WHEN** a valid package declares extension kind and requests game.custom_launch
- **THEN** it can be installed without a provider manifest and remains subject to normal identity, permission, signature and activation checks

### Requirement: Attributed persistence budgets
The host SHALL account managed persistent data by owning plugin, dataset/category, logical/physical usage, quota, last use and retention. Plugin callers SHALL receive only their own logical accounting. Exact local paths and global accounting SHALL be visible only through host-owned user administration.

#### Scenario: Performance history consumes quota
- **WHEN** a plugin persists performance samples until its approved quota is reached
- **THEN** additional writes report resource exhaustion or follow the approved retention policy without deleting another plugin data

### Requirement: Uninstall offers persistence retention
Uninstall SHALL offer keeping or deleting the plugin managed persisted data, showing the user its size, location and consumer dependencies. Kept data SHALL remain inert and attributed to its original producer/version. Reinstall or future consumers SHALL require normal authorization; uninstall SHALL not delete games, saves or archives.

#### Scenario: Keep for future reuse
- **WHEN** the user uninstalls a plugin and chooses to retain its data
- **THEN** executable code and grants are removed while archived data remains available only through newly authorized API access

### Requirement: User-controlled cross-plugin cleanup
Host administration SHALL offer reviewed cleanup of unused/uninstalled or large plugin datasets with size/path/dependency previews and protection controls. Global automatic cleanup SHALL default off. Plugin-initiated cleanup SHALL remain limited to its own namespace regardless of user game-file grants.

#### Scenario: Reclaim unused data
- **WHEN** the user selects an uninstalled plugin dataset in cleanup
- **THEN** only the reviewed managed data is deleted and actual reclaimed space or failure is reported
