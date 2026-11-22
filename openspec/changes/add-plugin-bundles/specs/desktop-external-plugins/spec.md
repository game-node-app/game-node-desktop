# Spec Delta

## MODIFIED Requirements

### Requirement: Managed package installation
The application SHALL install third-party plugins only through explicit package import into its managed per-user registry. Packages and publisher-key trust SHALL belong to the OS-user registry; activation and enabled state SHALL belong to the signed-in GameNode account. Packages SHALL be validated before execution. Packages SHALL be installed disabled until the user approves activation for that account, except that one explicitly accepted import SHALL both install the reviewed package and enable its compatible reviewed components for the active profile immediately, with the reviewed permission decisions applied and no application restart required. A signed bundle SHALL install its component packages through this same flow, atomically. Arbitrary files dropped outside this flow SHALL NOT be executed as plugins.

#### Scenario: Import a valid package
- **WHEN** a user accepts a valid package review for this platform
- **THEN** its metadata, license and requested permissions were shown before acceptance, no executable ran during validation, and the host installs the package, applies the reviewed permission decisions and enables the compatible components in the same running session

#### Scenario: Reject an unsafe archive
- **WHEN** an archive contains traversal, absolute paths, links, duplicate or case-colliding entries, excessive expansion or an unsupported architecture
- **THEN** installation is rejected without changing the active version or writing outside staging

#### Scenario: Import a bundle
- **WHEN** a user accepts a valid signed bundle review
- **THEN** every component is validated before execution, installed under its own identity, shown in one review without executing package code, and every compatible component is registered and enabled for the active profile immediately

#### Scenario: Component activation fails after an accepted bundle import
- **WHEN** an accepted bundle installs a compatible component that cannot be registered or started
- **THEN** the component and the whole bundle stay installed with their reviewed grants, the import result reports that component as installed but not activated with its error, and the other components keep their own outcome

### Requirement: Independent lifecycle and failure isolation
The user SHALL be able to enable and disable each third-party plugin persistently for the signed-in account and remove its package for the OS user. Third-party plugins SHALL run only while a GameNode account profile is active. Disable SHALL revoke its sessions and stop supervised processes with a bounded grace period. A failed or disabled plugin SHALL NOT erase library history, start new work, or disable official providers. Bundle components SHALL keep independent lifecycle state and failures.

#### Scenario: Disable a running plugin
- **WHEN** the user disables a plugin during a scan
- **THEN** new API work is refused, supervised execution is stopped and the previous successful library facts remain

#### Scenario: Plugin crashes
- **WHEN** a third-party executable exits unexpectedly
- **THEN** its status reports failure without an unbounded restart loop and official providers continue operating

#### Scenario: Sign-out stops third-party plugins
- **WHEN** the user signs out or the session is rejected
- **THEN** third-party sessions are revoked, supervised processes stop with the bounded grace period and each account's enabled state is retained for its next sign-in

#### Scenario: Remove installed plugin
- **WHEN** the user removes a plugin
- **THEN** its sessions, packages and grants are removed, library history is preserved and private configuration/secret deletion is a separate choice

#### Scenario: Disable one bundle component
- **WHEN** the user disables or one component crashes while the other bundle components are active
- **THEN** only that component's work and processes stop, and its siblings keep their own sessions, grants and recorded facts
