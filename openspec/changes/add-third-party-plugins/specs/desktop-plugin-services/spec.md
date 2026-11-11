# Spec Delta

## MODIFIED Requirements

### Requirement: Host services require explicit grants
Each host service SHALL require its named grant and validate plugin, session, operation, the applicable provider or runtime-content context and applicable profile/account generation. A denied call SHALL return no protected data and apply no side effect. Private resources of another plugin SHALL remain inaccessible. Explicit public-pool category or selected-record operation grants SHALL authorize only eligible shared resources, never foreign credentials or private namespaces.

#### Scenario: Missing permission
- **WHEN** a session without secret or login grants requests those services
- **THEN** the host rejects it without reading credentials or opening a window

#### Scenario: Cross-plugin handle
- **WHEN** a plugin presents another plugin's resource handle
- **THEN** access is rejected unless an explicit shared-resource operation grant covers that handle; matching permission names alone do not authorize access

### Requirement: Discovery permission submits installed observations
`library.discovery` SHALL accept installed-game observations only for the current authenticated provider operation or a host-issued scoped runtime-content operation. Success with no games SHALL be distinct from failure. Validated observations SHALL enter existing reconciliation rather than granting direct library mutation. Runtime-content contexts SHALL bind plugin identity and approved roots without provider impersonation or authority over foreign provider facts.

#### Scenario: Steam is absent
- **WHEN** discovery succeeds without finding a Steam installation
- **THEN** the host applies a successful empty installed result

#### Scenario: Discovery fails
- **WHEN** the plugin reports a scan-level failure
- **THEN** the previous installed facts remain and a categorized error is shown

#### Scenario: Extension discovers runtime content
- **WHEN** an authorized extension without a provider submits content observations under a current host-issued runtime-content operation and approved roots
- **THEN** Core reconciles content and candidate runtime bindings without granting a provider account context or executing content

#### Scenario: Discovery context missing
- **WHEN** discovery is submitted without either a valid provider operation or a valid scoped runtime-content operation
- **THEN** the host rejects it without modifying successful discovery facts
