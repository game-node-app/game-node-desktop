# Spec Delta

## MODIFIED Requirements

### Requirement: Explicit persistence durability
Durable publication, received-data copies and private writes SHALL require storage.persist independently of category read permissions. Calls SHALL specify session-only or persistent durability and receive the actual mode/expiry. A denied persistent request SHALL fail explicitly rather than silently losing durability. A durable publication SHALL survive the producer session and remain readable under the current grants until its expiry, retention limit or removal.

#### Scenario: Persist received performance samples
- **WHEN** a consumer has data access but lacks storage.persist
- **THEN** its persistent-copy request is denied and session-only consumption remains separately available

#### Scenario: Revoke access after a copy was retained
- **WHEN** the user revokes a category read permission after a consumer has durably retained an authorized copy
- **THEN** future source queries are denied, the retained copy follows its separately authorized retention policy, and the UI explains that already delivered data cannot be recalled automatically

#### Scenario: Publish a durable record
- **WHEN** a producer with data.publish and storage.persist publishes a custom dataset record as PERSISTENT
- **THEN** the host stores it with host-assigned provenance and returns PERSISTENT and its expiry, and a consumer with the applicable grant can still read it after the producer session ends

#### Scenario: Persistent publication without its grant
- **WHEN** a producer requests PERSISTENT publication but lacks storage.persist
- **THEN** the request fails explicitly and the host does not silently store a session-only copy

#### Scenario: Revocation while the producer is offline
- **WHEN** the user revokes the consumer's grant after a durable record was published and the producer is not running
- **THEN** new reads and queued watches are denied or fenced without requiring the producer to reconnect
