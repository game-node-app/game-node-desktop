# Spec Delta

## MODIFIED Requirements

### Requirement: Shared plugin data is private until explicitly granted
The host MUST keep published datasets private by default and persist common-category and selected-record read grants for the host-authenticated plugin, publisher and profile. Every common-data query MUST explicitly select its category or record, and the same current grants MUST filter all query, projection and subscription paths.

#### Scenario: A query omits an authorized category selector
- **WHEN** a consumer reads shared data without selecting an authorized category or record scope
- **THEN** the host excludes that data from the result

#### Scenario: An activation grant survives restart without triggering a read
- **WHEN** a user selects a common-data category for a plugin and the host later restarts
- **THEN** the grant remains associated only with that plugin's authenticated publisher/profile identity, but the plugin sees common records only when a later query explicitly selects the category

#### Scenario: Data list resumes as a watch without a gap
- **WHEN** a consumer starts `DataWatch` using the snapshot revision returned by `DataList`
- **THEN** the host starts the bounded subscription after that exact revision or rejects a stale cursor, without silently skipping intervening records

## ADDED Requirements

### Requirement: Typed resource extensions remain bounded and optional
The host MUST validate extension envelopes as a bounded set and resolve schemas from integrity-verified package assets bound to the authenticated plugin and publisher. Extensions MUST NOT replace typed authority fields. Unknown optional schemas MUST leave the typed base usable; only schema-validated fields may be projected under current policy.

#### Scenario: Reader encounters an unsupported optional schema
- **WHEN** a valid runtime has a supported typed base and a bounded optional extension whose schema the reader does not support
- **THEN** the reader can use the typed base, the owner record retains the opaque extension, and the extension fields are omitted from the reader's projection

#### Scenario: Resource extension exceeds the aggregate budget
- **WHEN** the combined extensions exceed the namespace, encoded-byte, depth or total-object-key budget
- **THEN** the host rejects the mutation without changing the resource or its revision

#### Scenario: Projection uses declared fields and current grants
- **WHEN** a reader selects fields from a known extension schema
- **THEN** the host returns only fields declared by that schema and permitted by the reader's current policy

### Requirement: Sensitive and custom sharing remains separately scoped
Sensitive/custom datasets MUST use a separate persisted review bound to the exact source, recipient and dataset. The review MUST specify allowed fields and history/live delivery. A common grant MUST NOT widen it; plugin RPC MUST NOT create or widen policy. Neither kind of grant authorizes recipient persistence or retention; durable copies require separate consent.

#### Scenario: Sensitive sharing stays bound to the reviewed source, recipient and fields
- **WHEN** the user approves fields and history/live delivery for one source-to-recipient dataset pair
- **THEN** only that recipient receives those fields through the approved delivery paths; other recipients, datasets and fields remain denied, and the policy does not grant durable retention

### Requirement: Publisher-key changes isolate data and require reviewed migration
A changed publisher key MUST establish a distinct identity. Old data MUST remain isolated until the user explicitly reviews and requests migration of compatible data. The host MUST preserve source provenance and MUST NOT automatically migrate secrets or permissions.

#### Scenario: Publisher key is re-trusted
- **WHEN** the user trusts a changed publisher key
- **THEN** old data and grants remain inaccessible to the new identity until any separate authorized migration or grant is completed
