# Design

## Context

`DataPublish` currently accepts only `SESSION_ONLY` and the broker removes a producer's rows on
disconnect. The public-pool specs already require durable publication with `storage.persist`
(`desktop-plugin-api` and `desktop-plugin-permissions`), so this change closes an implementation
gap rather than inventing a new permission model. The private durable stores
(`StoragePut(PERSISTENT)`, `Persistence*`) remain separate and are not reused as a public pool.

## Goals / Non-Goals

**Goals:**

- Persist authorized shared records across producer sessions and host restarts.
- Keep the existing publication/read grants and field projections exactly as they are.
- Bound growth with quotas, TTL, retention and cleanup.
- Keep session-only behavior unchanged and never silently persist it.
- Let the Dolphin catalog cache be read without the producer running.

**Non-Goals:**

- Making private storage public or automatically copying foreign records.
- A remote/cloud pool, cross-device sync or federation.
- New update/delete RPCs beyond the existing publish/resource replacement.
- Changing the grant model or the default privacy.

## Decisions

### Separate durable store

Add a host-owned SQLite database (`plugin-shared-data.gnpersistence`) with:
`shared_records` (owner key, dataset, record/resource identity, schema, revision, payload,
provenance, observed/received/expiry times, updated time), `shared_state` (monotonic pool
sequence) and `shared_retention` (per-owner age/byte limits). A separate database keeps the
public pool distinct from private `plugin-storage`/`plugin-persistence` files and lets the
broker stay in memory for session-only rows. Writes run on the storage worker, never the UI
thread.

### Explicit durability, same grants

`DataPublish(PERSISTENT)` requires `data.publish` and `storage.persist`; `SESSION_ONLY` keeps
today's behavior and grants. A missing grant returns an explicit failure, never a session
downgrade. The publish reply and record gain an additive `durability` field so the caller sees
the actual mode; no new routes are added.

### Record identity and revisions

A publish with a non-empty `resource_id` replaces that producer's previous record for the same
owner/dataset/resource and increments the record revision; a publish without one creates a new
host record id (append-only) subject to TTL/retention. The pool revision is a persisted
monotonic sequence shared by list/watch, so a consumer can resume after restart without
replaying old events. Reads merge durable and session rows, with durable rows first by
`received_at` and the existing page/cursor rules.

### Authorization and cross-session reads

Durable reads use the persisted `SharedSharingPolicies` exactly like session reads: category or
selected-record selectors, field projections, owner-private default. The producer does not need
to be connected; the consumer's own session and current policy revision are checked. Revocation
fences queued watches and cursors through the policy revision, not through producer liveness.

### Retention, quota and cleanup

Per-owner byte and age limits apply to durable records, with TTL as a per-record upper bound.
A bounded worker prunes expired then oldest unprotected records when a quota is crossed. Usage is
reported in the public-pool settings (size, count, limit) and a user deletion path removes the
retained data without uninstalling the plugin. No physical path is exposed to plugins or the UI.

### Uninstall and archived reuse

The existing uninstall keep/delete choice extends to durable records. Kept records remain inert
under the original owner; a future consumer or reinstall requires the existing reviewed
archived-data access. Deleting the plugin data removes its durable rows in the same operation.

## Risks / Trade-offs

- **Unbounded growth** → quotas, TTL and oldest-first pruning; usage surfaced to the user.
- **Stale records read as current** → provenance and received/observed timestamps stay on every
  record; the UI/consumer can distinguish age. Producers replace their own resource records.
- **Revocation races** → check the policy revision immediately before returning a page/event,
  as session reads already do.
- **Cursor/replay gaps after restart** → persist the pool sequence and reject stale cursors.
- **Privacy regression** → durable rows are still filtered by the same grants and projections;
  no automatic copy; tests for foreign/profile isolation.
- **Old builds** → they ignore the new database; the feature is opt-in per publish. A newer
  database is refused by the existing schema guard, not silently downgraded.
- **Dolphin-specific temptation to cache in private storage** → the acceptance fixture reads the
  durable record through the public API with the producer stopped.

## Migration Plan

1. Add the database and schema guard; no backfill, session rows are not copied.
2. Enable `PERSISTENT` publication behind the existing request field and grants.
3. Extend uninstall/settings/UI and docs.
4. Rollback: stop durable publication and keep the database on disk; session-only behavior is
   unchanged. The Dolphin plugin can republish; no private data is touched.
