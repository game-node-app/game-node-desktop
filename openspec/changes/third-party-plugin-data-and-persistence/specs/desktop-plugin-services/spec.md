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

#### Scenario: The user previews and confirms a migration
- **WHEN** the user requests a preview for a re-trusted publisher identity
- **THEN** the host reports the old and new fingerprints, the active profile, per-dataset eligible/conflicting/incompatible counts and logical sizes, private entry counts and any rows it cannot copy, without mutating data or exposing values, paths or secrets, and requires explicit confirmation bound to that profile and epoch before copying only the compatible data

#### Scenario: Migration never widens access or deletes the source
- **WHEN** a confirmed migration copies compatible private entries and durable records
- **THEN** the original rows keep their source/version provenance and remain in place, secrets and grants are not migrated, and the new identity needs fresh sharing and retention reviews before a consumer can read the migrated data

#### Scenario: A migration is rolled back
- **WHEN** the user confirms a rollback of one completed migration
- **THEN** only that migration's copies are removed from the new identity, the old identity's data and copies the user protected since stay, and another profile is never affected


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

### Requirement: Private persistence retention is reviewed and never crosses an owner
The host MUST apply one keep-or-delete choice to private persistence at uninstall. Keep makes entries inert with revisions and provenance intact; Delete removes only that plugin/profile namespace. Archived entries MUST be invisible to reads, lists, usage, quota and retention, and reuse MUST need a confirmed, profile-matched host review. No choice may affect game files, saves, backups, other owners or the durable pool.

#### Scenario: Keep at uninstall
- **WHEN** the user uninstalls one plugin or bundle component and chooses Keep
- **THEN** its private entries become invisible to reads, lists and usage and a reinstalled package needs a separate reviewed reuse before those values are readable again

#### Scenario: Delete at uninstall
- **WHEN** the user chooses Delete for one plugin or bundle component
- **THEN** only that plugin/profile private namespace is removed and every other profile, plugin, game file, live save and backup archive is unchanged

#### Scenario: Reuse is unconfirmed or for another profile
- **WHEN** reuse of archived private entries is requested without confirmation or with a different active profile
- **THEN** the host fails closed and changes nothing; only a confirmed, profile-matched review restores the entries

### Requirement: Host-wide cleanup is reviewed and host-only
The host MUST provide a host-only, reviewed cleanup of unused or large plugin data across producers and private stores. A review MUST list candidate owners with sizes, ages, protected and archived counts and effective limits. Deletion MUST require confirmation and a single-use profile-bound token, act only on selected owners, keep protected rows, measure reclaimed bytes and report partial failures. Plugin IPC MUST NOT reach it.

#### Scenario: A candidate is selected without confirmation
- **WHEN** a cleanup apply request omits confirmation, uses an expired or reused token, or names a candidate outside the review
- **THEN** the host rejects the request and deletes nothing

#### Scenario: One selected owner fails
- **WHEN** one selected producer's identity changed or its cleanup fails while another candidate succeeds
- **THEN** the report names the failed candidate and its error, reports the measured reclaimed bytes of the completed candidates, and requires a new review for a retry

#### Scenario: Protected and unselected data
- **WHEN** a reviewed cleanup prunes one owner
- **THEN** protected records and unselected owners, stores and archived rows are unchanged, and the per-namespace automatic retention rules still apply
