# Spec Delta

## ADDED Requirements

### Requirement: Shared plugin data is private until explicitly granted
The host MUST keep published datasets private by default and apply explicit category or selected-record grants consistently to queries, projections and subscriptions.

#### Scenario: A query omits an authorized category selector
- **WHEN** a consumer reads shared data without selecting an authorized category or record scope
- **THEN** the host excludes that data from the result

#### Scenario: Data list resumes as a watch without a gap
- **WHEN** a consumer starts `DataWatch` using the snapshot revision returned by `DataList`
- **THEN** the host starts the bounded subscription after that exact revision or rejects a stale cursor, without silently skipping intervening records

### Requirement: Published data preserves host provenance
The host MUST assign source, publisher, package/version and record identity and MUST reject plugin-supplied authority or ownership fields.

#### Scenario: Producer forges another owner
- **WHEN** a plugin submits a record with a foreign owner identity
- **THEN** the host rejects or replaces the forged attribution using authenticated identity

### Requirement: Managed persistence is owner-scoped and bounded
The host MUST expose durable plugin data only through logical owner-scoped APIs with explicit retention, quotas and cleanup rules.

#### Scenario: Cleanup targets a foreign namespace
- **WHEN** a plugin requests deletion by a foreign namespace selector
- **THEN** the host rejects the operation without disclosing physical paths or foreign records

### Requirement: Downgrades and publisher changes require explicit review
The host MUST reject downgrade selections from automated update feeds. A user-requested downgrade MUST use the normal verified-package path, present the source and target versions with a warning, and require explicit confirmation before activation. A publisher-key change MUST have a separate re-trust review bound to the exact new fingerprint.

#### Scenario: An update feed advertises a lower version
- **WHEN** an automated feed offers a release below the active plugin version
- **THEN** the host does not activate it or silently change the active package

#### Scenario: The user requests a downgrade
- **WHEN** the user selects a verified lower version for downgrade
- **THEN** the host shows the active and target versions and requires explicit confirmation before changing the active package

#### Scenario: The package changes publisher key
- **WHEN** an update package is signed by a key different from the pinned publisher key
- **THEN** the host requires a separate confirmation of the exact new fingerprint before activation

### Requirement: Private plugin data remains recoverable across package rollback
The host MUST keep the authenticated plugin/publisher/profile owner namespace stable across package versions. Plugins MUST use versioned logical keys or schema identifiers when private data changes shape. The host MUST NOT automatically destroy older schema versions during update or downgrade, and rollback MUST retain the current grants intersected with the selected package's manifest.

#### Scenario: A plugin writes a new private-data schema
- **WHEN** a plugin version changes the shape of private data
- **THEN** the new data uses a versioned logical key or schema identifier and older values remain available for a compatible rollback

#### Scenario: A package is rolled back
- **WHEN** the host restores the last-known-good package after failed activation or an explicitly confirmed downgrade
- **THEN** the owner namespace remains stable, older compatible values are retained, and permissions are limited to the current grants intersected with that package's manifest

Archived data reuse after uninstall remains subject to its separate host review and does not authorize a new consumer.
