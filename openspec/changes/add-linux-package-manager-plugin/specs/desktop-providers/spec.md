# Spec Delta

## ADDED Requirements

### Requirement: Linux package-manager game discovery
On Linux, GameNode SHALL expose installed packages as game-provider records only when an installed desktop entry describes a visible application in the Game category and its executable resolves to an existing file. Each record SHALL use a stable package-local id, the desktop entry's display name, the executable's parent directory as its install directory, and package-manager metadata.

#### Scenario: Discover a game installed by pacman
- **WHEN** an Arch-family Linux system has a visible Game desktop entry owned by an installed pacman package and its executable exists
- **THEN** the Linux package provider returns one record for that package with the desktop name, executable path, and install directory

#### Scenario: Ignore non-game or hidden applications
- **WHEN** a desktop entry is not an Application in the Game category, has `NoDisplay=true`, or has no resolvable executable
- **THEN** it is not returned as a game candidate

#### Scenario: No matching package candidates
- **WHEN** the package manager query succeeds but no installed packages own eligible game desktop entries
- **THEN** the provider reports a successful empty scan

### Requirement: Linux package-manager backend selection
The Linux package provider SHALL use pacman on Arch-family distributions and PackageKit on other Linux distributions. A selected backend's missing service or failed query SHALL fail only that provider scan; GameNode SHALL retain the provider's last successful records and keep the rest of the application usable.

#### Scenario: Select pacman on Arch-family systems
- **WHEN** the distribution ID or ID_LIKE identifies Arch Linux
- **THEN** discovery queries pacman package ownership and does not require PackageKit

#### Scenario: Select PackageKit on other distributions
- **WHEN** the Linux distribution is not identified as Arch-family
- **THEN** discovery queries installed package ownership through PackageKit

#### Scenario: Backend scan fails
- **WHEN** the selected package-manager backend is unavailable or returns a query error
- **THEN** the package provider reports a scan failure, the last successful package records remain unchanged, and other providers continue to work

### Requirement: Linux-only package provider delivery
The official Linux package provider SHALL be registered only on Linux and SHALL use the shared `provider.context.read` and `library.discovery` plugin capabilities. Its executable SHALL be delivered alongside the Linux desktop host; it SHALL NOT be required, built, or included by Windows packaging.

#### Scenario: Linux package includes provider executable
- **WHEN** the Linux desktop package is built
- **THEN** it contains the host and Linux package provider executables, and the package verifier accepts both binaries

#### Scenario: Windows package excludes Linux provider
- **WHEN** the Windows desktop package is built
- **THEN** it contains no Linux package provider executable and the application does not require that plugin
