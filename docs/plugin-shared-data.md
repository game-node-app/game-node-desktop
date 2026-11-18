# Shared plugin data

`ResourceHost.DataPublish`, `DataGet`, `DataList` and `DataWatch` are the public
wire API for session-retained plugin data. Use the generated
`ResourceHost` client and the session's `resource_context`; no source-private
RPC or Core dashboard is part of this contract. `DataPublish` requires the
`data.publish` capability. Reads require an authenticated plugin session and
host authorization for each record; there is no general `data.read` grant.

## Dataset, method, permission and projection contract

The protobuf service definitions and the checked route catalogs are the wire
contract. A generated client only proves that a method can be called by name;
`DescribeApi` is the runtime availability check. `projection_id` selects a
host-defined projection and never lets the caller request arbitrary fields.
For every list/get/watch path, the host applies the same current authorization
and field projection before returning records or events.

| Dataset or information family | Producer methods | Consumer methods | Permission and sharing scope | Projection and current availability |
| --- | --- | --- | --- | --- |
| Custom shared dataset | `DataPublish` | `DataGet`, `DataList`, `DataWatch` | Producer: `data.publish`. Consumer: authenticated session, owner access, or exact source/recipient/dataset sharing policy. `CommonDataSelector` categories and record IDs only request scopes; current grants authorize each independently. | `DataRecord` exposes host-assigned record ID, dataset ID, publisher ID, revision and the authorized `Struct` fields. Uses the host's custom-dataset projection; no caller-selected field mask. `Data*` is served. |
| Game paths | No shared-data producer adapter | `DataGet`/`DataList`/`DataWatch` selector; typed `Locations` methods are separate | `COMMON_DATA_CATEGORY_GAME_PATHS` plus a selected-record grant, or an explicitly reviewed sensitive projection where applicable. Typed location methods require `game.storage.inspect` or `game.saves.inspect` and approved resource scope. | Common category is a consent scope only. No adapter currently publishes this category to the shared-data broker. Physical managed paths and raw paths are excluded from shared projections. |
| Performance history | `GamePerformancePublishSamples`, `GamePerformanceEndSeries` (typed telemetry); `DataPublish` for an independent custom dataset | `GamePerformance.List`, `Get`, `Watch`, `QuerySeries`, `Aggregate`; `Data*` for custom datasets | Producer: `game.performance.publish` or `data.publish`. Consumer: `game.performance.read` plus explicit common category/record selection or exact sensitive projection policy. | Typed series retain game, installation, computer context, session, owner, samples and revision; queries return samples/coverage and aggregates include unit, count, coverage and provenance. Custom `DataRecord` fields are projected separately. Typed telemetry consumer routes are currently unserved; custom `Data*` is served. |
| Runtime catalog | No shared-data producer adapter | `Data*` selector; typed runtime methods are separate | `COMMON_DATA_CATEGORY_RUNTIME_CATALOG` or selected-record grants; typed runtime methods require `game.runtime.read` for reads. | Category is a consent scope; no shared-broker adapter currently publishes it. Typed runtime routes are not served in this build. |
| Content availability | No shared-data producer adapter | `Data*` selector; typed library/content methods are separate | `COMMON_DATA_CATEGORY_CONTENT_AVAILABILITY` or selected-record grants; typed library reads require `library.read` and may require `library.read.all` for whole-library scope. | No shared-broker adapter currently publishes this category. Use only fields exposed by a served typed method; store identifiers, credentials and install paths are not shared projection fields. |
| Library roots | No shared-data producer adapter | `Data*` selector; typed `Libraries.List/Get/Watch` are separate | `COMMON_DATA_CATEGORY_LIBRARY_ROOTS` or selected-record grants; typed methods require `library.roots.read`. | No shared-broker adapter currently publishes this category. Typed logical root APIs do not expose physical managed-storage paths. |
| Save profiles | No shared-data producer adapter | `Data*` selector; typed `Saves.ListProfiles/GetProfile` are separate | `COMMON_DATA_CATEGORY_SAVE_PROFILES` or selected-record grants; typed reads require `game.saves.read` and approved game/resource scope. | No shared-broker adapter currently publishes this category. Typed profile projection contains logical profile metadata and approved-resource handles, not raw filesystem paths or save bytes. |
| Storage diagnostics | `StorageDiagnosticsPublishBenchmark`, `StorageDiagnosticsPublishSmart` | `StorageDiagnostics.List`, `Get`, `Watch` | Producer: `storage.diagnostics.publish`. Consumer: `storage.diagnostics.read` and the user's explicit sensitive-data projection review. | Report projection contains report kind, host-attributed owner, opaque volume identity token, schema/version, typed measurements, approved extension, observation time and revision. Consumer domain routes are in the protocol/catalog but unserved here. |
| Computer hardware | `ComputerPublishHardware` | `Computer.List`, `Get`, `Watch` | Producer: `computer.hardware.publish`. Consumer: `computer.hardware.read` and explicit sensitive-data projection review. | Projection contains host-attributed owner, schema/version, typed observations, approved extension, observed time and revision; raw serials and absent identifiers stay absent. Consumer domain routes are currently unserved. |
| Game presence | `GamePresencePublish`, `GamePresenceClear` | `GamePresence.List`, `Get`, `Watch` | Producer: `game.presence.publish`. Consumer: `game.presence.read` and explicit sensitive-data projection review. | Projection is scoped by game/session and includes host-attributed owner, sequence, observation/expiry and approved details. It does not trigger online publication or host process-state decisions. Consumer domain routes are currently unserved. |
| Private durable plugin data | `StoragePut` or Persistence write methods | `StorageGet` or Persistence read/list methods | `storage.write`/`storage.read`; durable writes additionally require `storage.persist`. Persistence remains restricted to the authenticated owner's namespace. | Private bytes are returned only to their owner through the private storage API. They are not a shared dataset and cannot be made readable by another plugin through a sharing grant. |

The method-level permission catalog in [Plugin API 1.6 method
permissions](plugin-api-permissions.md) is authoritative for required grants.
The matrix above also distinguishes API shape from runtime availability: typed
producer/consumer service routes listed as unserved must not be treated as
available merely because generated bindings exist.

## What this build serves

| Method | Request and result | Permission and behavior |
| --- | --- | --- |
| `DataPublish` | `DataPublishRequest` → `DataMutationReply` | Requires `data.publish`; appends a custom dataset record and returns its host-assigned ID and revision. Publication is session-retained, so `durability` must be `SESSION_ONLY`. Use Persistence for private durable values. |
| `DataGet` | `DataGetRequest` → `DataGetReply` | Requires the session context and an exact record ID. The owner can read its own record; a different plugin must pass the host's current record or dataset sharing policy. |
| `DataList` | `DataListRequest` → `DataListReply` | Requires a `PageRequest` with session context. Pages one dataset; `filter` must be empty. The opaque cursor is session-bound and expires after five minutes. |
| `DataWatch` | `DataWatchRequest` → stream of `DataEvent` | Requires session context and current live-sharing authorization. Queue capacity is bounded; revocation or falling behind ends the stream, so list and watch again. |

`DataPublishRequest` carries `dataset_id`, `schema_id`, `schema_version`,
`durability`, and `ttl_seconds`, plus exactly one payload: a non-empty protobuf
`Struct` or non-empty `opaque_payload` bytes. Empty `Struct` values are rejected
because structured records are projected by their authorized fields. Keep dataset and schema IDs stable and
version the schema when its shape changes. Opaque payload sharing requires an
explicit `opaque_payload` field grant; it is never partially projected. The
producer also supplies `resource_id`, non-negative `observed_at`, a positive
per-resource `sequence`, and a unique `source_observation_id`. Sequence streams
are scoped to the authenticated plugin, publisher key, profile, dataset, and
resource. A repeated observation with the same content returns its existing
record; conflicting or out-of-order observations return `REVISION_CONFLICT`.

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

1. The producer declares `data.publish`, opens a plugin session, and sends
   `DataPublish` with that session's resource context, an idempotency key, its
   dataset/schema identifiers, exactly one non-empty `Struct` or opaque payload,
   observation metadata, `SESSION_ONLY`, and an optional TTL. The reply
   contains the record ID and broker revision.
2. A consumer opens its own session and calls `DataList` for the same dataset.
   Put its resource context in `page.context`. To request common data, set a
   `CommonDataSelector`; a selector is a request, not a grant. Without one, the
   result contains only records the caller owns.
3. To fetch one record, call `DataGet` with its dataset and record IDs. To follow
   new publications without a list-to-watch gap, call `DataWatch` with the
   `revision` from `DataListReply`. A stale position fails closed; repeat the
   list and watch handoff.

The consumer never decides which fields are authorized. The host projects the
allowed fields before returning a record or sending a watch event. Category
grants are selected for an authenticated recipient during activation and only
make a category eligible for explicit queries; they do not trigger reads.
Selected record IDs are checked independently and do not imply category-wide
access. Sensitive and custom datasets use a source-to-recipient policy scoped
to the exact dataset, with separate field, history, and live-delivery choices.
Revocation changes the policy revision and fences cursors and subscriptions.
Plugins cannot create or broaden these policies through RPC. The inventory has
a sharing review surface, but its selections are currently held only in UI
state and are not connected to the persisted policy store. As a result, the
default-deny installation can publish and read its own records but cannot
complete an interactive cross-plugin grant. The consumer flow below describes
the public request contract after a host policy has approved the recipient;
persisting the review choices and enforcing them through the policy store remain
an integration gap beyond the UI controls covered by task 1.18.

## Dataset categories and exclusions

`CommonDataCategory` currently defines these policy scopes:

| Category | Intended information |
| --- | --- |
| `GAME_PATHS` | Approved game path information exposed by a host domain adapter |
| `GAME_PERFORMANCE_HISTORY` | Performance history exposed by a host domain adapter |
| `RUNTIME_CATALOG` | Runtime catalog records exposed by a host domain adapter |
| `CONTENT_AVAILABILITY` | Content availability exposed by a host domain adapter |
| `LIBRARY_ROOTS` | Logical library roots exposed by a host domain adapter |
| `SAVE_PROFILES` | Save profile records exposed by a host domain adapter |

These enum values describe consent scopes; they do not promise that a host data
source is populated. In this build, `DataPublish` creates custom-class records,
and there is not yet a producer adapter that publishes the listed domain
categories into the shared-data broker. Domain methods not listed as served by
`DescribeApi` remain `UNIMPLEMENTED`. Plugins should use a currently served
domain method for its typed data, or the public `Data*` methods for their own
custom dataset; they must not depend on a private host RPC to bridge the two.

Never publish credentials, tokens, session identifiers, private plugin storage,
physical managed-storage paths, executable handles, or fields that claim host
ownership, publisher, permission, or grant authority. A `Struct` is data, not
authority: the host rejects reserved authority fields, assigns provenance, and
does not infer execution behavior from custom values. Opaque private bytes
belong in private `Storage` or, for durable values, `Persistence`; neither is
automatically shareable.

The Rust SDK exposes the generated client through
`HostClient::resource_client()`. Pass `HostClient::resource_context()` in each
request and use `gamenode_plugin_sdk::common_data_selector` to construct typed
selectors. Other languages use the generated protobuf `ResourceHost` client
with the same request fields and host-side authorization rules.
