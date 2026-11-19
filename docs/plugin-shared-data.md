# Shared plugin data

`ResourceHost.DataPublish`, `DataGet`, `DataList` and `DataWatch` are the public
wire API for session-retained plugin data. Use the generated
`ResourceHost` client and the session's `resource_context`; no source-private
RPC or Core dashboard is part of this contract. `DataPublish` requires the
`data.publish` capability. Reads require an authenticated plugin session and
host authorization for each record; there is no general `data.read` grant.

## Dataset, method, permission and projection matrix

Check method availability by service and method name in `DescribeApi`. The
checked protocol catalogs include reserved routes. This build advertises some
Data methods twice and can show `data.publish` on read entries. The read
handlers accept an authenticated consumer with a matching host sharing policy
even when it has no `data.publish` grant.
[Protobuf definitions](../crates/plugin-protocol/proto/plugin.proto),
[ResourceHost catalog](../crates/plugin-protocol/contracts/resource_host.json),
[domain catalog](../crates/plugin-protocol/contracts/domain_host.json),
[host availability and Data handlers](../crates/app/src/plugin_host/resources.rs).

### Served routes

Methods without a service prefix belong to `ResourceHost`. The typed domain
routes below use their own resource and permission checks. `DataPublish` only
creates custom records, and this build has no producer adapter feeding any of
the six common categories into the broker.

| Dataset or information family | Write methods and grants | Read methods and grants | Returned projection and scope |
| --- | --- | --- | --- |
| Custom shared dataset | `DataPublish` with `data.publish` | `DataGet`, `DataList`, `DataWatch` with owner access or exact source/recipient/dataset review | `DataRecord` has a host-assigned ID and revision, dataset/schema, observation and expiry metadata, and host-attributed provenance. The payload has the reviewed top-level `Struct` fields or the whole opaque payload when explicitly approved. |
| Approved game paths and volumes (`GAME_PATHS`) | Location inspection is read-only | `Locations.Inspect`, `WatchAvailability`, `GetSize`, `GetAccess`, `GetLastModified`, `GetFingerprint`, `Compare`. Use `game.storage.inspect` for approved installation/library resources and `game.saves.inspect` for approved save/prefix resources or own save profiles. | Inspection and availability return scoped volume/attachment metadata and opaque volume tokens. Size, access, timestamps, fingerprints and comparison results describe the approved scope. Location details, handles and relative names stay in that scoped API. |
| Runtime catalog (`RUNTIME_CATALOG`) | `RuntimesRegister`, `RuntimesUpdate`, `RuntimesRemove` with `game.runtime.create`, `game.runtime.update`, `game.runtime.delete` respectively. Register/update replies also require `files.read`, and the executable must be approved. | `RuntimesList`, `RuntimesGet`, `RuntimesWatch` with `game.runtime.read` and `files.read` | The caller's own runtime ID, name, version, formats, platform, revision, observation time and argument template, with an approved executable handle bound to the session. Cross-plugin catalog projections are `UNIMPLEMENTED`. |
| Content availability (`CONTENT_AVAILABILITY`) | `GamesRegisterContent` with `library.discovery`, `files.read` and an approved content handle | `LibraryList`, `LibraryGet`, `LibraryWatch` with `library.read` and `library.read.all` for whole-library scope | Registration binds content to a scoped game/installation and the caller's runtime. Library reads return logical game/installation and display metadata. `LibraryEntry.runtime_availability` is empty in this build. Common-data selectors on these reads return `UNIMPLEMENTED`. |
| Logical library roots (`LIBRARY_ROOTS`) | `LibrariesCreate`, `LibrariesAddRoot` with `library.roots.create`. `LibrariesUpdate`, `LibrariesUpdateRoot` with `library.roots.update`. `LibrariesRemove`, `LibrariesRemoveRoot` with `library.roots.delete`. Adding a root requires folder approval. | `LibrariesList`, `LibrariesGet`, `LibrariesWatch` with `library.roots.read` | The caller's logical library IDs, names, descriptions and revisions. Roots include IDs, names, availability, watching/discovery state and approved session-bound handles. Common-data selectors return `UNIMPLEMENTED`. |
| Save profiles (`SAVE_PROFILES`) | `Saves.CreateProfile`, `Saves.UpdateProfile`, `Saves.RemoveProfile` with `game.saves.create`, `game.saves.update`, `game.saves.delete` respectively | `Saves.ListProfiles`, `Saves.GetProfile` with `game.saves.read` and game/resource scope | Logical profile metadata, runtime/format, revision, naming rules and handles for currently approved folders. Current desktop reads are owner-only. Category selectors on these routes return `UNIMPLEMENTED`. |
| Private plugin storage | `StoragePut` with `storage.write` and additionally `storage.persist` for `PERSISTENT`. `StorageDelete` with `storage.delete`. | `StorageGet` with `storage.read` | Private bytes, revision, durability and expiration in the authenticated plugin/publisher/profile namespace. Shared-data grants cannot address these keys. |

### Reserved routes

Both the writes and reads in this table are `UNIMPLEMENTED`. Grants and
projections here describe the published contract for the follow-up tasks.
Where a request has `projection_id`, it selects a host-defined projection.

| Dataset or information family | Write methods and grants | Read methods and grants | Contracted projection |
| --- | --- | --- | --- |
| Storage diagnostics (1.9) | `StorageDiagnosticsPublishBenchmark`, `StorageDiagnosticsPublishSmart` with `storage.diagnostics.publish` | `StorageDiagnostics.List`, `Get`, `Watch` with `storage.diagnostics.read` and explicit sensitive-data review | Report ID and kind, host-attributed owner, opaque volume token, schema/version, typed measurements, approved extension, observation time and revision. |
| Computer hardware (1.10) | `ComputerPublishHardware` with `computer.hardware.publish` | `Computer.List`, `Get`, `Watch` with `computer.hardware.read` and explicit sensitive-data review | Record ID and computer context, host-attributed owner, schema/version, typed observations, approved extension, observation time and revision. Exclude raw serials and leave missing identifiers absent. |
| Game presence (1.10) | `GamePresencePublish`, `GamePresenceClear` with `game.presence.publish` | `GamePresence.List`, `Get`, `Watch` with `game.presence.read` and explicit sensitive-data review | Presence ID, game/session scope, host-attributed owner, sequence, observation/expiry, approved details and revision. Presence is an observation and grants no authority over online status or host process state. |
| Performance history (`GAME_PERFORMANCE_HISTORY`, 1.11) | `GamePerformancePublishSamples`, `GamePerformanceEndSeries` with `game.performance.publish` | `GamePerformance.List`, `Get`, `Watch`, `QuerySeries`, `Aggregate` with `game.performance.read` and explicit common category/record selection backed by a grant, or exact sensitive projection review | Series ID, game/install/computer/session context, host-attributed owner, revision and typed samples (FPS, CPU percent, memory bytes). Queries include coverage and dropped samples. Aggregates include units, count, coverage and provenance. |
| Private persistence administration (1.14–1.17) | `PersistenceDeleteEntries`, `PersistenceClear`, `PersistenceSetRetentionPolicy` with `storage.persist` | `PersistenceGetUsage`, `PersistenceListEntries` with `storage.persist` | Owner-scoped usage and quota counts, logical IDs/keys, categories, sizes and last-used times. Physical paths remain host-only. |

## Shared Data requests

| Method | Request and result | Permission and behavior |
| --- | --- | --- |
| `DataPublish` | `DataPublishRequest` → `DataMutationReply` | Requires `data.publish`. Appends a custom dataset record and returns its host-assigned ID and revision. Set `durability` to `SESSION_ONLY` and `mutation.expected_revision` to `0`. For private durable values, use `StoragePut` with `PERSISTENT` and the independent `storage.persist` grant. |
| `DataGet` | `DataGetRequest` → `DataGetReply` | Requires the session context and an exact record ID. For a common record owned by another plugin, also select its category or record ID through `CommonDataSelector`. For a foreign custom or sensitive record, the exact policy authorizes its reviewed fields by ID independently of the history/live settings. Hidden and missing records return `NOT_FOUND`. |
| `DataList` | `DataListRequest` → `DataListReply` | Requires a `PageRequest` with session context. Pages one dataset with history access. Leave `filter` empty. Supply the common selector on the request or on `page`. If both are present, they must agree. The opaque cursor is bound to the session and query, expires after five minutes and is fenced by changes to the snapshot or sharing policy. |
| `DataWatch` | `DataWatchRequest` → stream of `DataEvent` | Requires session context and filters each event with current live-sharing authorization. `after_revision = 0` starts at the current broker revision without replay. Any nonzero position must match the current revision. If policy revocation or queue overflow ends the stream, list and subscribe again. |

`DataPublishRequest` carries `dataset_id`, `schema_id`, `schema_version`,
`durability`, and `ttl_seconds`, plus one payload chosen from a non-empty
protobuf `Struct` and non-empty `opaque_payload` bytes. Use a namespaced
dataset ID. Set `schema_id` to that ID or a dot-qualified child and set a
positive `schema_version`. Sharing opaque bytes requires an explicit
`opaque_payload` field grant and delivers the entire payload. The
producer also supplies `resource_id`, non-negative `observed_at`, a positive
per-resource `sequence`, and a unique `source_observation_id`. Sequence streams
are scoped to the authenticated plugin, publisher key, profile, dataset, and
resource. A repeated observation with the same content returns its existing
record. Conflicting or out-of-order observations return `REVISION_CONFLICT`.

The host assigns record identity, revision, received time, and provenance from
the authenticated session and signed package. Session-only records are removed
when their producer session closes. `ttl_seconds = 0` means no expiry before
session close; positive values set an expiry and are capped at 30 days. The
broker bounds a record to 1 MiB, a `Struct` to depth 32 and 4096 values, and
retains at most 16,384 records and 64 MiB total. `DataPublish` is append-only;
its idempotency key is scoped to the authenticated producer session and covers
the complete payload and observation metadata, so a retry returns the original
result, key reuse for changed content is rejected, and a later session can safely
reuse a key after the earlier session's data has been removed.

## Producer and consumer flow

1. Open separate authenticated producer and consumer sessions in the same
   profile. The producer declares and receives `data.publish`. Publish
   `org.example.metrics.v1` with the producer's `mutation.context`, a unique
   idempotency key, the same ID as `schema_id`, `schema_version = 1`,
   `SESSION_ONLY`, `resource_id = "runtime-primary"`, `sequence = 1`, an
   observation time and a unique `source_observation_id`. The fixture sends
   a `Struct` with `fps` and a dummy `device_id` to exercise projection.
   Keep the returned record ID.
2. In the host's plugin inventory, review the exact producer, recipient and
   dataset. Approve only `fps`, with history for list and live delivery for watch.
   The host binds the policy to the verified plugin IDs, publisher fingerprints
   and active profile. A plugin cannot create or widen this policy through RPC.
3. Call `DataList` with the consumer's context in `page.context` and the same
   `dataset_id`. The record has only `fps` in its payload and retains the
   producer's host-assigned provenance. Call `DataGet` with the consumer's
   context, dataset and record ID to retrieve the same projection.
4. Call `DataWatch` with the same dataset and consumer context, setting
   `after_revision` to `DataListReply.revision`. Once subscribed, have the
   producer publish another observation with a higher sequence and fresh keys.
   The consumer receives a `published` event with the same field projection.
   If an intervening change makes the position stale, the host replies
   `FAILED_PRECONDITION`. List again and retry the handoff.
5. Revoke the policy in the host. A queued watch event and a pre-revocation
   list cursor are fenced, a fresh list filters the revoked records, and a
   fresh get for the revoked record returns `NOT_FOUND`.

The 1.27 fixtures exercise this exchange through generated clients and a local
gRPC server. `persisted_custom_policy_filters_authenticated_public_exchange_and_revokes_access`
reopens the host policy store and verifies the projected list, get and watch,
independent history/live choices, and revocation.
`persisted_common_policy_requires_explicit_selectors_on_public_reads` seeds
common records at the broker boundary and checks the public selector paths.
[ResourceHost fixtures](../crates/app/src/plugin_host/resources/tests/data.rs).

Six category values are available for consent, as named in the matrix.
For common records, put the approved category in
`CommonDataSelector.categories` or specific record IDs in
`CommonDataSelector.selected_record_ids` on each query. Each scope requires
its matching grant. Without either selector, the common-data path returns
only the caller's own records.

## Non-shareable information

Exclude credentials, tokens, session identifiers, private plugin storage, raw
filesystem paths, physical managed-storage locations, executable or file
handles, raw device serials, private store/account identifiers and save bytes
from shared payloads. Use dataset-local opaque IDs for resource and observation
metadata. Keep payloads and provenance free of secrets. The host rejects
reserved top-level keys such as `owner`, `provenance`, `permission`, `path`
and `executable_handle`. Producers remain responsible for excluding sensitive
values inside nested objects, under other keys or inside opaque bytes.
Custom fields carry data and cannot grant execution, ownership or access.

Shared reads grant access to the authorized projection for the current query
or delivery. Durable recipient copies require separate retention consent
(task 1.13). Use the private `Storage*` API for the owner's private bytes.

The Rust SDK exposes the generated client through
`HostClient::resource_client()`. Pass `HostClient::resource_context()` in each
request. The Rust SDK provides `common_data_selector` for categories,
`selected_record_selector` for record IDs and `common_data_selector_with_records`
for both. Other languages use the generated protobuf `ResourceHost` client
with the same request fields and host-side authorization rules.
