# Shared plugin data

`ResourceHost.DataPublish`, `DataGet`, `DataList` and `DataWatch` are the public
wire API for session-retained plugin data. Use the generated
`ResourceHost` client and the session's `resource_context`; no source-private
RPC or Core dashboard is part of this contract. `DataPublish` requires the
`data.publish` capability. Reads require an authenticated plugin session and
host authorization for each record; there is no general `data.read` grant.

## What this build serves

| Method | Request and result | Permission and behavior |
| --- | --- | --- |
| `DataPublish` | `DataPublishRequest` → `DataMutationReply` | Requires `data.publish`; appends a custom dataset record and returns its host-assigned ID and revision. Publication is session-retained, so `durability` must be `SESSION_ONLY`. Use Persistence for private durable values. |
| `DataGet` | `DataGetRequest` → `DataGetReply` | Requires the session context and an exact record ID. The owner can read its own record; a different plugin must pass the host's current record or dataset sharing policy. |
| `DataList` | `DataListRequest` → `DataListReply` | Requires a `PageRequest` with session context. Pages one dataset; `filter` must be empty. The opaque cursor is session-bound and expires after five minutes. |
| `DataWatch` | `DataWatchRequest` → stream of `DataEvent` | Requires session context and current live-sharing authorization. Queue capacity is bounded; revocation or falling behind ends the stream, so list and watch again. |

`DataPublishRequest` carries `dataset_id`, `schema_id`, `schema_version`, a
protobuf `Struct`, `durability`, and `ttl_seconds`. Keep dataset and schema IDs
stable and version the schema when its shape changes. The host assigns record
identity, publisher and revision. `ttl_seconds = 0` means no expiry; positive
values set an expiry. The current implementation bounds a record to 1 MiB, a
Struct to depth 32 and 4096 values, and the in-memory broker to 16,384 records
and 64 MiB total. `DataPublish` is append-only and uses the mutation idempotency
key for safe retries.

## Producer and consumer flow

1. The producer declares `data.publish`, opens a plugin session, and sends
   `DataPublish` with that session's resource context, a fresh idempotency key,
   its dataset/schema identifiers, a `Struct`, `SESSION_ONLY`, and an optional
   TTL. The reply contains the record ID and broker revision.
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
