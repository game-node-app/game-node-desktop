# Spec Delta

## MODIFIED Requirements

### Requirement: Shared plugin data is private until explicitly granted
The host MUST keep published datasets private by default and apply explicit category or selected-record grants consistently to queries, projections and subscriptions. Durable records SHALL be filtered by the current grants on every read, including reads that happen while the producer is offline.

#### Scenario: A query omits an authorized category selector
- **WHEN** a consumer reads shared data without selecting an authorized category or record scope
- **THEN** the host excludes that data from the result

#### Scenario: Data list resumes as a watch without a gap
- **WHEN** a consumer starts `DataWatch` using the snapshot revision returned by `DataList`
- **THEN** the host starts the bounded subscription after that exact revision or rejects a stale cursor, without silently skipping intervening records

#### Scenario: Durable record read while the producer is stopped
- **WHEN** a consumer with the applicable grant reads a durable published record and the producer plugin is not running
- **THEN** the record is returned under the same field projection and revision rules as a session record

### Requirement: Published data preserves host provenance
The host MUST assign source, publisher, package/version and record identity and MUST reject plugin-supplied authority or ownership fields. Durable records SHALL keep the assigned provenance and owner identity across restarts and producer sessions.

#### Scenario: Producer forges another owner
- **WHEN** a plugin submits a record with a foreign owner identity
- **THEN** the host rejects or replaces the forged attribution using authenticated identity

#### Scenario: Provenance survives a restart
- **WHEN** the host restarts and a durable record is read again
- **THEN** its producer identity, package version, origin and schema remain the host-assigned values from publication

## ADDED Requirements

### Requirement: Durable shared records are owner-scoped and bounded
Durable shared records SHALL live in a host-owned store with per-owner quotas, TTL, retention and cleanup. A publisher SHALL replace only its own record for the same dataset/resource, and uninstall SHALL offer keep-or-delete for its durable records.

#### Scenario: Quota or retention prunes records
- **WHEN** a producer's durable records exceed its age or byte limit
- **THEN** the host prunes the oldest unprotected records and reports the reclaimed space without touching another owner

#### Scenario: Another producer's record is protected
- **WHEN** a producer publishes a record for a resource already published by another plugin
- **THEN** only its own record is replaced and the other producer's record remains unchanged

#### Scenario: Producer uninstalled
- **WHEN** the producer is uninstalled and the user chooses to keep its durable data
- **THEN** the records remain inert under the original owner and a future consumer requires the normal reviewed access

#### Scenario: No session-only persistence
- **WHEN** a producer publishes SESSION_ONLY data
- **THEN** the record is not written to the durable store and disappears with the producer session
