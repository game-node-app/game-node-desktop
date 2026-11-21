# Spec Delta

## MODIFIED Requirements

### Requirement: Third-party plugin management in Stores
The Stores panel SHALL extend its plugin inventory with separate official and third-party attribution and actions to import, enable, disable, review permissions, check/review updates and remove third-party packages. It SHALL show active version, publisher fingerprint, compatibility and failure status without adding a new navigation topic. A bundle SHALL appear as one entry with one review and one uninstall while activation remains per component.

#### Scenario: Import example plugin
- **WHEN** the user selects Install plugin and chooses a reference package
- **THEN** the panel shows package validation progress and opens a review containing identity, publisher, license and requested permissions before activation

#### Scenario: Plugin inventory while signed out
- **WHEN** the user opens Stores from Settings/About without a signed-in account
- **THEN** installed third-party packages and compatibility are shown read-only, and import, activation, grants, Configure, update and removal controls are unavailable with a localized sign-in prompt

#### Scenario: Official plugin selected
- **WHEN** the user selects Steam in the inventory
- **THEN** its existing official attribution/details remain and third-party permission or update controls cannot alter its grants or bundled binary

#### Scenario: Bundle inventory and uninstall
- **WHEN** a bundle with a discovery provider and a launch extension is installed
- **THEN** one entry shows the bundle name, version, publisher and each component's status, its review groups permissions by component, and one uninstall action removes both after a single retention choice
