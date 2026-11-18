# Spec Delta

## ADDED Requirements

### Requirement: Shared plugin data is private until explicitly granted
The host MUST keep published datasets private by default and apply explicit category or selected-record grants consistently to queries, projections and subscriptions.

#### Scenario: A query omits an authorized category selector
- **WHEN** a consumer reads shared data without selecting an authorized category or record scope
- **THEN** the host excludes that data from the result

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

### Requirement: Publisher-key changes isolate data and require reviewed migration
A changed publisher key MUST establish a distinct identity. Old data MUST remain isolated until the user explicitly reviews and requests migration of compatible data. The host MUST preserve source provenance and MUST NOT automatically migrate secrets or permissions.

#### Scenario: Publisher key is re-trusted
- **WHEN** the user trusts a changed publisher key
- **THEN** old data and grants remain inaccessible to the new identity until any separate authorized migration or grant is completed
