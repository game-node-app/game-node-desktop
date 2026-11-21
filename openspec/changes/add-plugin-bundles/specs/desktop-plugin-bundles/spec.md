# Spec Delta

## Purpose

Provide one signed, easy-to-install distribution unit for plugins that need more than one package
(provider plus extension) without merging their identities, processes or permission grants.

## ADDED Requirements

### Requirement: Signed bundle envelope
A plugin bundle SHALL be one ZIP archive containing a canonical `bundle.json` and one or more
existing package archives under `components/`. `bundle.json` SHALL declare a bundle id, name,
SemVer version, publisher key and, per component, a package-relative path, SHA-256, plugin id and
kind. The archive SHALL be signed over `bundle.json`; nested bundles SHALL be rejected.

#### Scenario: Valid bundle
- **WHEN** the user imports a bundle whose signature, component hashes and component package signatures verify
- **THEN** the host accepts it for review without executing package code

#### Scenario: Tampered or mixed publisher
- **WHEN** a component hash differs, a component is missing, a component is signed by another publisher or the archive contains a nested bundle
- **THEN** the import is rejected without installing any component

### Requirement: Atomic structural install and per-component compatibility
A structurally valid bundle SHALL install all components atomically; a failure SHALL leave the
previous installation unchanged. A component that is only incompatible with the current platform
or API SHALL be installed unavailable and SHALL NOT block its siblings.

#### Scenario: Missing component
- **WHEN** a bundle references a component that is absent, unreadable or fails package validation
- **THEN** no component is installed or activated and the previous installation remains unchanged

#### Scenario: One component is incompatible
- **WHEN** the platform or API check rejects one component but the bundle's other components are compatible
- **THEN** the compatible components are installed and the rejected component is reported unavailable without failing the bundle

### Requirement: Bundle components keep their own identity and grants
Each component SHALL keep its own plugin id, kind, API range, permissions and process lifecycle.
A bundle SHALL NOT merge session kinds, share grants between components or change what an
individual component may request.

#### Scenario: Provider and extension in one bundle
- **WHEN** a bundle contains a discovery provider and a launch extension
- **THEN** each component is registered, granted and supervised under its own identity as if imported alone

### Requirement: One bundle review and publisher decision
Import SHALL present one review for the bundle with its publisher fingerprint and each component's
identity, version, compatibility, license and requested permissions grouped by component.
Confirming the bundle SHALL pin one publisher fingerprint and SHALL NOT grant any permission.

#### Scenario: Single review for two components
- **WHEN** the user imports a valid bundle
- **THEN** one review shows the bundle and both components, and one confirmation pins the publisher without granting permission

#### Scenario: One component's permission is denied
- **WHEN** the user denies a permission requested by one component
- **THEN** the bundle remains installed, the other components keep their own grants, and the denied component operates only within what it was granted

### Requirement: Grouped inventory and cascade uninstall
The inventory SHALL show one entry per bundle with the bundle name, version, publisher and each
component's status. Enable and disable SHALL remain per component. One uninstall SHALL stop and
remove every component and apply one persistence-retention choice.

#### Scenario: Bundle entry with an unavailable component
- **WHEN** a compatible provider and an extension whose runtime support is unavailable are installed from one bundle
- **THEN** the single inventory entry reports the provider as usable and the extension as unavailable

#### Scenario: Uninstall a bundle
- **WHEN** the user uninstalls the bundle
- **THEN** every component's sessions and processes stop, all component packages are removed, and one retention choice applies to their managed data

### Requirement: Atomic bundle update and rollback
A bundle update SHALL replace all components together or leave the previous set active. The review
SHALL show each component's old and new version and permission differences. Rollback SHALL restore
the previous bundle set.

#### Scenario: Update fails after staging
- **WHEN** a bundle update fails validation or activation after staging
- **THEN** the previous component set stays active and usable

#### Scenario: Roll back a bundle
- **WHEN** the user approves rollback of the previous bundle version
- **THEN** every component returns to the previous bundle set and their permission grants are revalidated rather than widened

### Requirement: Bundle tooling and bounded archives
Repository tooling SHALL pack and verify bundles with the pinned key, a canonical signature and a
development-key marker. The host and tooling SHALL bound the component count, archive and expanded
sizes, reject traversal, absolute paths, links, duplicate ids and case collisions, and SHALL NOT
extract recursively.

#### Scenario: Tool round trip
- **WHEN** the packing tool creates a bundle and the verify command checks it
- **THEN** verification succeeds and any modified component or manifest fails

#### Scenario: Unsafe or oversized bundle
- **WHEN** a bundle exceeds the component or size limits or contains an unsafe path
- **THEN** import is rejected with an actionable error and nothing is written outside staging
