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

### Requirement: Game performance batches and history are bounded and explicitly attributed
The host MUST accept bounded FPS/CPU/RAM batches from the authenticated producer and bind series to the selected game/installation and host computer/session. Samples MUST identify source, units, windows and FPS/CPU/memory semantics. Preserve gaps, reject invalid sequences/values, enforce quotas with explicit backpressure, retain only for the producer session, and query retained samples under current sharing authorization without interpolation or prediction.

#### Scenario: A plugin reports a measured series for its selected game
- **WHEN** a plugin with `game.performance.publish` submits valid FPS, CPU and RAM samples matching its host-selected game/installation
- **THEN** the host stores the bounded series with its source, units, sampling semantics, runtime context, host identity, provenance and sequence coverage

#### Scenario: Missing or out-of-order samples arrive
- **WHEN** sequence values contain a gap or a batch duplicates/regresses a previously accepted sequence
- **THEN** the host reports retained coverage and dropped counts for the gap and rejects the duplicate/regressed batch without fabricating zero values

#### Scenario: A batch exceeds a rate or retention bound
- **WHEN** a producer exceeds the batch interval or a batch/series would exceed its byte/sample quota
- **THEN** the host returns an explicit backpressure outcome with retry and dropped-sample information and preserves the previously accepted series

#### Scenario: A consumer queries a shared performance series
- **WHEN** a consumer has `game.performance.read` and explicitly selects the common performance-history category or an authorized record
- **THEN** List/Get/Watch/QuerySeries/Aggregate return only records allowed by the current sharing policy, with units, source and coverage; omitted selectors leave foreign common records private

#### Scenario: Aggregate uses only available observations
- **WHEN** a consumer requests average, min, max or a percentile over an authorized time interval
- **THEN** the host computes from retained samples and reports units, count, provenance and gaps without interpolation or prediction

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

### Requirement: Shared save-resource scope requires explicit review
The host MUST bind each shared emulator save resource to the authenticated plugin, publisher and profile and its host-resolved backing handle. The host MUST resolve the complete affected game/installation set and require explicit user consent bound to that exact set, resource and revisions before creating or changing the scope.

#### Scenario: The user reviews the full affected-game scope
- **WHEN** a plugin proposes to create or widen a shared memory-card resource used by multiple games or installations
- **THEN** the host presents the complete host-resolved affected set and approved backing location for explicit consent, and records the decision against the exact authenticated identity, resource and revisions

#### Scenario: A plugin attempts to widen an approved scope
- **WHEN** a plugin changes the backing handle, runtime binding or affected-game set after review
- **THEN** the host refuses the mutation or requires a fresh user review before applying it; existing grants do not silently widen

### Requirement: Backup and restore coordinate with host-observed resource use
The host MUST coordinate shared-resource access with host-issued use leases for supervised runtime process trees and platform checks against approved runtime executable identities. Unsafe backup/restore MUST be denied while use is active or unknown. Plugin-reported activity MUST NOT establish that a resource is idle.

#### Scenario: Restore races with a supervised emulator launch
- **WHEN** a restore holds the exclusive resource lease while a supervised runtime attempts to use the same resource
- **THEN** the restore and launch are serialized, and the runtime cannot use the resource until the restore commits or aborts

#### Scenario: An emulator is active or its state is unknown
- **WHEN** a backup/restore operation targets a resource with an active supervised lease, a matching externally observed approved runtime process, or indeterminate process state
- **THEN** the host refuses the unsafe operation with an actionable busy/unknown result; plugin-reported inactivity cannot override the refusal
