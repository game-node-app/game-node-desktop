# Proposal

## Why

The public shared-data pool is session-only in the current host: `DataPublish` rejects any
durability other than `SESSION_ONLY`, and the broker deletes a producer's records when its
session ends. The specs already require durable publication with the separate
`storage.persist` grant, and the Dolphin plugin exposed the gap in practice: a discovered
disc-ID → title → catalog-association table is only useful across sessions, when the producer
may not be running.

Session-only remains correct for live observations and presence. Reference data that consumers
read later needs a host-owned durable pool with explicit authorization, provenance, retention
and cleanup.

## What Changes

- Implement the already-specified durable publication path: `DataPublish` with `PERSISTENT`
  durability requires `data.publish` plus `storage.persist` and writes the record to a
  host-owned SQLite pool instead of the in-memory session broker. `SESSION_ONLY` stays the
  default and its behavior is unchanged.
- Persist host-assigned provenance, schema version, revision, observation/receipt times, TTL and
  the producer's owner identity. A later publish for the same producer/dataset/resource replaces
  that producer's previous value with a new revision; other producers' records are never
  modified.
- Keep publication private until granted. Durable records are readable across sessions under the
  current persisted category/selected-record policy even while the producer is offline;
  revocation, profile change, expiry and retention fence reads and watches immediately.
- Add retention and quota rules for the durable pool: per-owner/category byte and age limits,
  TTL expiry, oldest-first pruning, usage reporting and cleanup that never touches another
  namespace. Producer uninstall offers keep/delete and archived-data reuse follows the existing
  reviewed path.
- Return the actual durability and expiry in publish/get/list records (additive fields) so a
  plugin can distinguish a durable record from a session copy; no silent downgrade.
- Show durable-pool usage and retention in the public-pool settings surface, with en/pt-BR
  labels and no path disclosure.
- Do not make `StoragePut`/`Persistence` public, do not auto-copy foreign records, and do not
  persist session-only publications.
- Update the SDK/protocol helpers, docs and fixtures. The Dolphin plugin's catalog cache is the
  first acceptance consumer.

## Capabilities

### New Capabilities

None. The behavior is already required by the existing public-pool specs; this change implements
it and clarifies cross-session and retention semantics.

### Modified Capabilities

- `desktop-plugin-api`: durable publication, actual-durability responses, cross-session reads and
  revision/retention behavior for published records.
- `desktop-plugin-services`: the shared-data service persists authorized records with host
  provenance and bounded retention instead of dropping every record at producer disconnect.
- `desktop-settings`: the public-pool settings show durable usage and retention, keeping the
  existing publication grants and per-record/category scopes.

## Impact

- Host: shared-data broker/store split (session memory + SQLite), publish/read/watch paths,
  retention worker, quota accounting, uninstall/archived integration, permission checks.
- Protocol/SDK: additive durability fields on publish replies and data records; helper updates;
  no new routes required for the first slice.
- UI: durable-pool usage/retention in settings; existing publication grants stay.
- Tests: cross-session read while the producer is stopped, restart persistence, revocation and
  profile fencing, TTL/quota/cleanup, producer uninstall keep/delete, no session-only leak, no
  foreign mutation.
- Related changes: complements `third-party-plugin-data-and-persistence` (publication/grant
  tasks 1.24/1.25 and the combined flow in 1.22) without reopening its completed persistence
  work. The Dolphin plugin repository is the motivating consumer, not a dependency.
- Out of scope: turning private storage into a public pool, automatic retention of foreign
  records, remote/cloud synchronization, and changing the session-only default.
