# Spec Delta

## MODIFIED Requirements

### Requirement: Host services require explicit grants
Each host service SHALL require its named grant and validate plugin, session, operation, the applicable provider or runtime-content context and applicable profile/account generation. A denied call SHALL return no protected data and apply no side effect. Private resources of another plugin SHALL remain inaccessible. Explicit public-pool category or selected-record operation grants SHALL authorize only eligible shared resources, never foreign credentials or private namespaces. A selected-record read does not require a category-wide grant; query selectors identify only the authorized categories or records. Public domain deletion is governed by its explicit record/category delete grant and remains distinct from storage.persist cleanup, which addresses only the caller's own private namespace.

#### Scenario: Missing permission
- **WHEN** a session without secret or login grants requests those services
- **THEN** the host rejects it without reading credentials or opening a window

#### Scenario: Cross-plugin handle
- **WHEN** a plugin presents another plugin's resource handle
- **THEN** access is rejected unless an explicit shared-resource operation grant covers that handle; matching permission names alone do not authorize access

#### Scenario: Selected shared record access
- **WHEN** a plugin queries one explicitly authorized published record without a category-wide grant
- **THEN** the host returns only that record and applies the same restriction through domain and Data APIs

#### Scenario: Public deletion and private cleanup remain separate
- **WHEN** a plugin deletes an explicitly authorized published domain record while cleaning persistent data
- **THEN** the domain delete grant applies to that record and storage.persist cleanup remains confined to the caller's own private namespace

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
