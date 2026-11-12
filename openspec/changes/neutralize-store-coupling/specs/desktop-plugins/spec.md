# Spec Delta

## ADDED Requirements

### Requirement: The packaged plugin set is derived from trusted descriptors
The plugin executables that a platform's development and release workflows build, place, verify, install and uninstall SHALL be the ones declared by the bundled trusted descriptors for that platform. Adding, removing or retargeting a bundled plugin SHALL need only a descriptor change. No workflow SHALL keep its own list of plugin executable names.

#### Scenario: Windows package
- **WHEN** the Windows package is built from descriptors that declare Steam and Epic for Windows
- **THEN** it contains the Steam and Epic executables and no Linux-only plugin, and the verifier requires exactly that set

#### Scenario: Linux package
- **WHEN** the Linux package is built from descriptors that declare Steam and the package-manager plugin for Linux
- **THEN** it contains those executables and no Windows-only plugin, and the verifier rejects any Windows-only plugin found in it

#### Scenario: New descriptor
- **WHEN** a new descriptor declares a plugin executable for Windows
- **THEN** the Windows build, package, verifier, installer and uninstaller include that executable without other script edits

#### Scenario: Descriptor that cannot be read
- **WHEN** a descriptor is malformed or names an executable that is not a plain file name
- **THEN** the build or package step fails before producing a package and names the descriptor

### Requirement: Packages carry their plugin index
Each package SHALL contain a generated index of its bundled plugin executables. The Windows installer SHALL copy exactly the files that index names. The installer SHALL record what it installed. The uninstaller SHALL remove the plugins recorded at install time as well as those in the index, so a plugin dropped by a later package is not left behind.

#### Scenario: Install
- **WHEN** a Windows package is installed, including under a path with spaces
- **THEN** every plugin in its index is copied next to the host, and the install record lists them

#### Scenario: Index names a missing file
- **WHEN** the index names a plugin that is absent from the package
- **THEN** installation stops before changing the existing installation and names the missing file

#### Scenario: Uninstall after a plugin was dropped
- **WHEN** an installation whose record lists a plugin is upgraded by a package that no longer bundles it, and the user later uninstalls
- **THEN** the dropped plugin and the current plugins are all removed

### Requirement: Store boundary checks follow the descriptors
The production store-boundary check SHALL take its store provider ids, store host names and store URI schemes from the bundled descriptors and provider manifests. When a store is added, its literals SHALL be checked without editing the checker.

#### Scenario: New store literal in host code
- **WHEN** a descriptor for a new store is added and host production code names its provider id
- **THEN** the boundary check fails on that line unless a reviewed allowlist entry excuses it

#### Scenario: Removed icon-host exceptions
- **WHEN** the boundary check runs after icon hosts move into descriptors
- **THEN** it passes without allowlist entries for store image hosts
