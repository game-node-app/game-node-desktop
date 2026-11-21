# Tasks

## 1. Durable shared store

- [ ] 1.1 Add the host-owned shared-data database with schema versioning, owner key, record identity, provenance, payload, timestamps, retention and a persisted pool sequence; verify a fresh open and an upgrade preserve no session rows and expose only logical fields.
- [ ] 1.2 Implement owner-scoped write/read/prune operations on a storage worker: replace a producer's resource record, append a record without a resource id, read by record id and list by dataset with a stable order. Verify foreign owners, profile epochs and revision conflicts fail closed.
- [ ] 1.3 Add quota/retention accounting and a bounded prune (expired, then oldest unprotected) with usage output. Verify a producer crossing its byte/age limit loses only its own oldest records and another owner is untouched.

## 2. Publication path

- [ ] 2.1 Accept `DataPublish(PERSISTENT)` with `data.publish` plus `storage.persist`: validate the payload/provenance exactly as session publication, persist the record and return the actual durability and expiry. Verify a missing `storage.persist` fails explicitly and no session copy is written.
- [ ] 2.2 Keep `SESSION_ONLY` unchanged and never persist it. Verify session rows disappear at producer disconnect while durable rows survive.
- [ ] 2.3 Add the additive durability field to the publish reply and data record, update the Rust/Go wire fixtures and SDK helpers, and verify old clients ignore it and new clients read the actual mode.

## 3. Reads, lists and watches

- [ ] 3.1 Merge durable and session records in `DataGet`, `DataList` and `DataWatch` under the existing category/selected-record projections. Verify a consumer reads a durable record while the producer is stopped and receives the same fields as before.
- [ ] 3.2 Persist and resume the pool revision across restarts; verify list-to-watch handoff, stale cursors, revocation and profile changes fence reads/events without the producer reconnecting.
- [ ] 3.3 Verify owner-private defaults: a consumer without a grant sees nothing durable, a selected-record grant returns only that record, and no foreign private storage or path is reachable.

## 4. Settings and cleanup

- [ ] 4.1 Expose durable-pool usage (size, count, retention limit) per producer in the Stores plugin settings with en/pt-BR labels and mocks; verify no path is shown and unrelated UI stays usable during a slow read.
- [ ] 4.2 Add the user deletion path for a producer's durable records without uninstalling, with confirmation and actual reclaimed bytes; verify another producer's records and private storage remain untouched.

## 5. Uninstall and archived data

- [ ] 5.1 Extend the uninstall keep/delete choice to durable shared records; verify keep leaves inert owner-attributed rows, delete removes them, and component/bundle uninstall keeps the same semantics.
- [ ] 5.2 Connect archived durable data to the existing reviewed reuse path; verify a reinstall or new consumer needs a fresh review and cannot silently read old records.

## 6. Documentation and acceptance

- [ ] 6.1 Update `docs/plugin-shared-data.md`, `docs/plugin-api-permissions.md` and `docs/plugin-development.md` with durability, retention, cross-session reads, uninstall and API fields; verify the documented commands and examples run.
- [ ] 6.2 Add a public RPC fixture with two plugins: producer publishes a durable catalog record, consumer reads it after the producer stops and after a host restart, revocation fences it, and quota/cleanup leaves the other owner intact.
- [ ] 6.3 Package the Dolphin provider with a durable disc-ID/title/catalog record and verify a consumer reads it with Dolphin stopped; record the Linux evidence and cross-build the fixture for Windows.

## Workflow follow-up

- Archive this change after the review and native acceptance requirements are satisfied.
- Synchronize the modified main specs during archive.
- Reconcile the related `third-party-plugin-data-and-persistence` tasks 1.22/1.24/1.25 with this change's publication/retention decisions.
