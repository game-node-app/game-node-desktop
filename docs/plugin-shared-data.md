# Shared plugin data

`ResourceHost.DataPublish`, `DataGet`, `DataList` and `DataWatch` are the public
wire API for session-retained and durable plugin data. Use the generated
`ResourceHost` client and the session's `resource_context`; no source-private
RPC or Core dashboard is part of this contract. `DataPublish` requires the
`data.publish` capability, and `PERSISTENT` publication additionally requires
the independent `storage.persist` grant. Reads require an authenticated plugin
session and host authorization for each record; there is no general
`data.read` grant, and a durable record is filtered by the same grants while
its producer is offline.

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
the six common categories into the broker. Two domain families serve category
reads directly instead: the runtime catalog through `RuntimesList`,
`RuntimesGet` and `RuntimesWatch`, and content availability through
`LibraryList`, `LibraryGet` and `LibraryWatch`.

| Dataset or information family | Write methods and grants | Read methods and grants | Returned projection and scope |
| --- | --- | --- | --- |
| Custom shared dataset | `DataPublish` with `data.publish`; `PERSISTENT` also requires `storage.persist` | `DataGet`, `DataList`, `DataWatch` with owner access or exact source/recipient/dataset review | `DataRecord` has a host-assigned ID and revision, dataset/schema, observation and expiry metadata, the actual `durability`, and host-attributed provenance. The payload has the reviewed top-level `Struct` fields or the whole opaque payload when explicitly approved. A durable record remains readable while its producer is offline. |
| Approved game paths and volumes (`GAME_PATHS`) | Location inspection is read-only | `Locations.Inspect`, `WatchAvailability`, `GetSize`, `GetAccess`, `GetLastModified`, `GetFingerprint`, `Compare`. Use `game.storage.inspect` for approved installation/library resources and `game.saves.inspect` for approved save/prefix resources or own save profiles. | Inspection and availability return scoped volume/attachment metadata and opaque volume tokens. Size, access, timestamps, fingerprints and comparison results describe the approved scope. Location details, handles and relative names stay in that scoped API. |
| Runtime catalog (`RUNTIME_CATALOG`) | `RuntimesRegister`, `RuntimesUpdate`, `RuntimesRemove` with `game.runtime.create`, `game.runtime.update`, `game.runtime.delete` respectively. Register/update replies also require `files.read`, and the executable must be approved. | `RuntimesList`, `RuntimesGet`, `RuntimesWatch` with `game.runtime.read` and `files.read`. An explicit `RUNTIME_CATALOG` selector returns other plugins' records after the recipient's current category grant, without a source-to-recipient policy. | The caller's own runtime ID, name, version, formats, platform, revision, observation time and argument template, with an approved executable handle bound to the session. A shared projection adds host-recorded producer provenance and validated extension namespaces but always has an empty executable handle; shared records cannot be updated, removed or launched. |
| Content availability (`CONTENT_AVAILABILITY`) | `GamesRegisterContent` with `library.discovery`, `files.read` and an approved content handle | `LibraryList`, `LibraryGet`, `LibraryWatch` with `library.read` and `library.read.all` for whole-library scope | Registration binds content to a scoped game/installation and the caller's runtime. Library reads return logical game/installation and display metadata. `LibraryEntry.runtime_availability` lists safe content/runtime identities for the caller's own bindings by default; an explicit `CONTENT_AVAILABILITY` selector adds other plugins' bindings after the current category grant, with no path, handle or byte payload. Other categories remain `UNIMPLEMENTED` on Library reads. |
| Logical library roots (`LIBRARY_ROOTS`) | `LibrariesCreate`, `LibrariesAddRoot` with `library.roots.create`. `LibrariesUpdate`, `LibrariesUpdateRoot` with `library.roots.update`. `LibrariesRemove`, `LibrariesRemoveRoot` with `library.roots.delete`. Adding a root requires folder approval. | `LibrariesList`, `LibrariesGet`, `LibrariesWatch` with `library.roots.read` | The caller's logical library IDs, names, descriptions and revisions. Roots include IDs, names, availability, watching/discovery state and approved session-bound handles. Common-data selectors return `UNIMPLEMENTED`. |
| Save profiles (`SAVE_PROFILES`) | `Saves.CreateProfile`, `Saves.UpdateProfile`, `Saves.RemoveProfile` with `game.saves.create`, `game.saves.update`, `game.saves.delete` respectively | `Saves.ListProfiles`, `Saves.GetProfile` with `game.saves.read` and game/resource scope | Logical profile metadata, runtime/format, revision, naming rules and handles for currently approved folders. Current desktop reads are owner-only. Category selectors on these routes return `UNIMPLEMENTED`. |
| Storage diagnostics | `StorageDiagnosticsPublishBenchmark`, `StorageDiagnosticsPublishSmart` with `storage.diagnostics.publish` | `StorageDiagnostics.List`, `Get`, `Watch` with `storage.diagnostics.read` and the current sensitive-data projection policy | Host-attributed benchmark/SMART report, schema/version, observation time, revision, projected measurements/attributes and the opaque volume token only when approved. Reports are session-retained. |
| Computer hardware observations | `ComputerPublishHardware` with `computer.hardware.publish` | `Computer.List`, `Get`, `Watch` with `computer.hardware.read` and exact sensitive source/recipient field review | The host assigns the active computer context and producer identity. Observations carry a schema/version and host-validated timestamp; caller-supplied computer IDs and common unique-identifier fields are rejected. Reports are session-retained and removed when the producer session closes. GameNode does not collect hardware. |
| Game presence | `GamePresencePublish`, `GamePresenceClear` with `game.presence.publish` | `GamePresence.List`, `Get`, `Watch` with `game.presence.read` and exact sensitive source/recipient field review | The producer explicitly publishes bounded details for a numeric GameNode game ID and safe session ID. The host assigns sequence and observation time, caps TTL at five minutes, replaces prior state for the same producer/game/session, and removes it on clear, expiry or producer-session close. Presence is descriptive and does not set online status or influence process/playtime decisions. |
| Game performance history (`GAME_PERFORMANCE_HISTORY`) | `GamePerformancePublishSamples`, `GamePerformanceEndSeries` with `game.performance.publish` | `GamePerformance.List`, `Get`, `Watch`, `QuerySeries`, `Aggregate` with `game.performance.read`; foreign common history additionally requires explicitly selecting the category or an authorized record | Each bounded series is tied to the host-selected game/installation, local computer context, plugin session and optional runtime recipe revision. Samples identify source/tool/version, Unix-millisecond observation time, sample interval, explicit FPS/CPU/RAM units and semantics, measurement quality and sequence. Only actual retained observations are queried or aggregated. Missing sequence numbers remain gaps; no values are interpolated. History is session-retained and removed when the producer session closes. |
| Private plugin storage | `StoragePut` with `storage.write` and additionally `storage.persist` for `PERSISTENT`. `StorageDelete` with `storage.delete`. | `StorageGet` with `storage.read` | Private bytes, revision, durability and expiration in the authenticated plugin/publisher/profile namespace. Shared-data grants cannot address these keys. |

### Durable publication, retention and cross-session reads

`DataPublishRequest.durability` selects the storage mode. `SESSION_ONLY` is the
default and unchanged: the record disappears when the producer session closes.
`PERSISTENT` requires both `data.publish` and `storage.persist`, writes the
validated record to the host-owned public pool and returns the actual mode and
expiry in `DataMutationReply.durability` and `.expires_at`. A missing
`storage.persist` is an explicit `PERMISSION_DENIED`; the host never downgrades
the request to a session copy. `UNSPECIFIED` is `INVALID_ARGUMENT`.

Every returned `DataRecord` carries the additive `durability` field, so a
consumer can distinguish a durable record from a session copy without guessing
from the producer's state. `StorageDurability` values are
`SESSION_DURABILITY_UNSPECIFIED`, `SESSION_ONLY` and `PERSISTENT`; the field is
additive and older clients ignore it.

A durable publication is private until the same category or selected-record
review grants read access. The producer may be stopped: `DataGet`, `DataList`
and `DataWatch` merge durable rows with live session rows under the current
policy revision, field projection and owner-private default. Revocation,
profile change and expiry fence reads and queued watches immediately, without
requiring the producer to reconnect. The persisted pool sequence is handed to
`DataList` and `DataWatch`, so a restart can resume after an exact revision and
a stale cursor is rejected instead of silently skipping records.

Durable limits and retention (all values are logical, never a path):

| Limit | Value |
| --- | --- |
| One durable record | 1 MiB, one non-empty `Struct` or non-empty `opaque_payload` |
| Per-owner default byte limit | 64 MiB; a user policy may only lower it |
| Per-owner default age limit | none; a user policy is optional and bounded to 365 days |
| Producer TTL | 0 (no expiry) or up to 30 days, capped by the host |
| Bounded prune | expired rows first, then the owner's oldest unprotected rows, at most 1024 rows per pass |

A write for the same producer, dataset and `resource_id` replaces that
producer's record in place and raises its record revision; another producer's
record is never replaced. A publication without `resource_id` appends a new
host id. Quota and retention prune only the publishing owner's oldest
unprotected rows and report the reclaimed bytes. `set_user_protected` is
host-only; a plugin cannot mark its own rows or name another owner.

### Uninstall, kept records and reviewed reuse

Uninstall offers one keep-or-delete choice for a producer's durable records:

- **Keep** archives every row under the original owner. The rows stay on disk
  and keep their host-assigned provenance, but ordinary reads, lists, watches,
  usage and automatic pruning skip them. A reinstalled package that shares the
  owner identity cannot replace an archived resource record and cannot read the
  old rows. A separate host review (`plugin_durable_reuse`, confirmed by the
  user) restores them; only then do the consumer's normal grants apply again.
- **Delete** removes only that producer's rows and reports the bytes actually
  reclaimed. Another owner's durable rows, the private `plugin-data` stores,
  grants and library history are untouched.

Bundle and component uninstall use the same choice for every component package
they actually removed. A partial removal reports the components that remain;
retention is applied only to the components whose packages were deleted. The
pool is host-owned and separate from private `StoragePut(PERSISTENT)` data, so
uninstalling a producer never turns private storage into a public pool.

The fixture below proves the whole path with two plugins and real RPCs:
`two_plugin_durable_catalog_fixture_survives_stop_restart_revocation_and_cleanup`
in `crates/app/src/plugin_host/resources/tests/data.rs` publishes a durable
catalog record from a game-bound extension producer, reads it from the consumer
after the producer stops, restarts the broker and pool, revokes the grant and
reclaims only the producer's owner. The same flow also refuses a performance
write for a game the host did not select, includes one game's history only when
the consumer explicitly selects the granted category, keeps both the category
and the exact custom dataset grant separate, archives the producer's rows for an
uninstall keep, restores them through the reviewed reuse step, and shows that a
private cleanup or another owner's deletion never crosses a game or namespace.

```sh
cargo test -p gamenode-app --lib --locked -- two_plugin_durable_catalog_fixture
cargo test -p gamenode-app --lib --locked -- uninstall_keep_archives
cargo test -p gamenode-app --lib --locked -- reinstalling_requires_a_reviewed_reuse
cargo test -p gamenode-platform plugin_shared_data --locked
```

### Storage diagnostics

Storage diagnostics reports are session-retained producer claims. The host derives the volume
identity token from the caller's approved file handle, validates report structure, and never
starts benchmark or SMART tools. `projection_id` is `benchmark.v1` or `smart.v1`; the matching
dataset policy selects the report fields visible to another plugin. The opaque volume token is
itself a projected field, so a report reader receives it only when that field is approved. A new
volume token is a distinct resource binding and does not match reports from a replaced disk.

### Game performance history

The performance publisher accepts at most 1000 samples and 1 MiB per batch, limits each
authenticated plugin to one accepted batch per second, and requires the declared sample interval
to be 100–60000 ms. Each session-retained series is bounded to 1 MiB. A rejected rate or storage
limit returns `MUTATION_OUTCOME_BACKPRESSURE`, `dropped_samples` and `retry_after_ms`; the last
accepted series remains unchanged. A repeated latest batch is idempotent. Sequence gaps are
reported as missing samples; a repeated or decreasing sequence is rejected.

`PerformanceSample.observed_at` is Unix milliseconds. FPS uses `frames/second` and states whether
it is instantaneous, frame-count-window or frame-time-derived, with instantaneous/average/minimum/
maximum/percentile statistic and measured/estimated quality. CPU uses `percent`, identifies
process/system scope and whole-machine (0–100) or per-core normalization. Memory uses `bytes`,
identifies working-set or resident memory, and names the collection source. Frame-window samples
must state a 100–600000 ms window; instantaneous samples use a zero window. Source/tool/version,
runtime and recipe revision are descriptive metadata. The host binds the report to the game and
installation selected for the authenticated session and supplies `active-host` as the local
computer context. It does not launch a collector or verify that a claimed runtime collected it.

`List` returns series in the requested game/computer scope. `QuerySeries` selects the time window,
page and optional resolution; resolution selects actual samples and never creates interpolated
points. `Aggregate` accepts `fps`, `cpu_percent` or `memory_bytes` with `average`, `min`, `max` or
nearest-rank `p0` through `p100`, and reports units, sample count, source provenance and sequence
coverage. Common history remains private unless the query explicitly selects its category or an
authorized record and the current sharing grant permits it. `series_id` is the stable owner-facing
series token. `record_id` is the current broker identity and may be used as `series_id` for a
selected-record read; a replacement creates a new record identity, so category-scoped consent is
the appropriate choice for an evolving history. Producer-session close removes history; durable
retention requires the separate `storage.persist` workflow.

#### Collection belongs to the publisher; transport belongs to GameNode

The plugin decides whether it has a legitimate source for an observation and collects it using its
own implementation or an explicitly selected external tool. GameNode does not bundle or start a
SMART/benchmark scanner, hardware inventory collector, game instrumentation plugin, FPS overlay or
prediction service. The host authenticates the publisher, checks permissions and bounds, binds the
report to the active profile/session and approved context, stores it for the declared lifetime, and
projects it to readers only under current consent. A report is a publisher claim; runtime IDs and
collector names do not prove that a process was inspected or that Core collected the value.

| Family | Publisher supplies | GameNode transports and validates |
| --- | --- | --- |
| Storage diagnostics | Benchmark/SMART measurements, tool/version, units, conditions and unavailable/error states | Approved volume identity, report schema, bounds, session retention and exact sensitive-field projection |
| Computer and presence | Hardware observations or explicit game/session presence updates | Host computer context, safe field names, timestamps/TTL, session cleanup and exact sensitive-field projection |
| Game performance | FPS/CPU/memory samples, measurement semantics, source/tool and sample sequence | Selected game/installation, local computer context, units/bounds, sequence coverage, backpressure and explicitly selected shared history |

For example, a producer's local payload may resemble the following before the host adds authenticated
ownership, selected-game context, revision and session provenance:

```json
{
  "schema_id": "game.performance.history.v1",
  "schema_version": 1,
  "source": "overlay-frame-counter",
  "tool": {"name": "Example Collector", "version": "2.0"},
  "sample_interval_ms": 100,
  "units": {"fps": "frames/second", "cpu": "percent", "memory": "bytes"},
  "samples": [
    {
      "sequence": 1,
      "observed_at": 1795219200123,
      "fps": {"value": 60.0, "method": "instantaneous", "quality": "measured"},
      "cpu": {"value": 34.5, "scope": "process", "normalization": "whole_machine_percent"},
      "memory": {"value": 536870912, "kind": "working_set_bytes", "source": "example-tool"}
    }
  ]
}
```

This is a documentation shape, not a second wire format: clients use the generated
`PerformanceSamplesRequest` and `PerformanceSample` messages. `observed_at` is Unix milliseconds;
the host stores only validated actual observations and does not fill gaps or derive measurements.

Benchmark fields include tool/version, units, named measurements, per-measurement units and
conditions. SMART fields carry model/firmware and named attributes with units. Both accept
explicit `available`, `unavailable`, `unsupported`, `access_denied` and `error` states; missing
measurements are not encoded as zero. Reports remain in the authenticated session and disappear
when it closes; this does not grant durable storage.

### Private persistence administration

`PersistenceGetUsage` reports the caller's durable logical and quota bytes plus an
estimated physical allocation; `PersistenceListEntries` pages the caller's
persistent entries by opaque entry ID and logical key, category, size, last
use and user-protection state. `PersistenceDeleteEntries`, `PersistenceClear` and
`PersistenceSetRetentionPolicy` require the exact current namespace revision and
apply atomically. Session-only values are not listed or counted. A retention
policy (private category, maximum age and maximum bytes) removes entries beyond
the selected bounds immediately, oldest use first. The namespace, identity,
profile and cursors come from the authenticated session; values, physical paths
and request-supplied plugin or path selectors are never exposed.

Usage also carries a per-category breakdown: record count, logical bytes and how
many records the user protected. The live private store exposes the `private`
category today, the only category charged against `quota_bytes`. The physical
figure is the SQLite page allocation of the whole namespace database and is
labelled namespace-wide, because SQLite metadata and free pages are shared across
categories and cannot be attributed per category.

User protection is a host-only decision; no plugin request field can set or
clear it. A protected record survives age/byte retention, cleanup, plugin
overwrites, `StorageDelete` and `PersistenceClear`. Naming a protected record in
`PersistenceDeleteEntries` fails with `FAILED_PRECONDITION` instead of deleting
part of the selection. Host cleanup (`PluginStorage::cleanup_persistence`) also
reclaims only the named owner's expired and policy-exceeding records and advances
that namespace's revision so stale cursors cannot resume across the change.
Further host-wide cleanup and uninstall retention remain tasks 1.16–1.17.

### Other reserved routes

The remaining writes and reads in this table are `UNIMPLEMENTED`. Grants and
projections here describe the published contract for the follow-up tasks. Where a
request has `projection_id`, it selects a host-defined projection.

| Dataset or information family | Write methods and grants | Read methods and grants | Contracted projection |
| --- | --- | --- | --- |

## Shared Data requests

| Method | Request and result | Permission and behavior |
| --- | --- | --- |
| `DataPublish` | `DataPublishRequest` → `DataMutationReply` | Requires `data.publish`; `PERSISTENT` additionally requires `storage.persist`. Appends or replaces a custom dataset record and returns its host-assigned ID, revision, actual `durability` and `expires_at`. Set `mutation.expected_revision` to `0` for an append. For private durable values, use `StoragePut` with `PERSISTENT` and its separate grant; the public pool never reuses private storage. |
| `DataGet` | `DataGetRequest` → `DataGetReply` | Requires the session context and an exact record ID. For a common record owned by another plugin, also select its category or record ID through `CommonDataSelector`. For a foreign custom or sensitive record, the exact policy authorizes its reviewed fields by ID independently of the history/live settings. Hidden and missing records return `NOT_FOUND`. |
| `DataList` | `DataListRequest` → `DataListReply` | Requires a `PageRequest` with session context. Pages one dataset with history access. Leave `filter` empty. Supply the common selector on the request or on `page`. If both are present, they must agree. The opaque cursor is bound to the session and query, expires after five minutes and is fenced by changes to the snapshot or sharing policy. |
| `DataWatch` | `DataWatchRequest` → stream of `DataEvent` | Requires session context and filters each event with current live-sharing authorization. `after_revision = 0` starts at the current broker revision without replay. Any nonzero position must match the current revision. If policy revocation or queue overflow ends the stream, list and subscribe again. |

There are two different durable outcomes. A producer that owns the reference
data can publish it as `PERSISTENT` under `data.publish` plus `storage.persist`;
the host stores it in the public pool with host provenance and later applies the
uninstall/retention rules above. A consumer that only read a record through a
grant can explicitly copy the returned projection into its own private
namespace with `StoragePut`. That copy requires the independent `storage.write`
and `storage.persist` grants and the logical key/expiry are the consumer's.
These stores never overlap: publishing does not write a consumer's private
storage, reading does not create a durable public copy, a retention grant does
not expand which source fields the consumer can read, and the stored copy
remains owned by the consumer's authenticated plugin/publisher/profile
identity.

`DataPublishRequest` carries `dataset_id`, `schema_id`, `schema_version`,
`durability`, and `ttl_seconds`, plus one payload chosen from a non-empty
protobuf `Struct` and non-empty `opaque_payload` bytes. `SESSION_ONLY` needs
only `data.publish`; `PERSISTENT` needs `data.publish` plus `storage.persist`.
Use a namespaced dataset ID. Set `schema_id` to that ID or a dot-qualified child and set a
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
   Keep the returned record ID. For reference data that must outlive the
   producer, send `PERSISTENT` with `storage.persist` and keep
   `mutation.expected_revision = 0`; the reply reports `PERSISTENT` and the
   expiry actually stored.
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

## Runnable producer and consumer examples

Two example clients exercise this contract end to end, and both read the same checked-in fixture
([`data_exchange_fixtures.json`](../crates/plugin-protocol/contracts/data_exchange_fixtures.json)),
so identities, units and expected outcomes cannot drift apart:

- **Rust** — [`examples/data_exchange_recipes.rs`](../crates/plugin-sdk/examples/data_exchange_recipes.rs)
  and [`examples/extension_data_recipes.rs`](../crates/plugin-sdk/examples/extension_data_recipes.rs)
  are compiled with the SDK and run against the desktop's real fixture host by
  `crates/app/src/plugin_host/resources/tests/sdk_data_exchange.rs` and `sdk_extension_data.rs`.
- **Go** — [`examples/go/data_exchange.go`](../examples/go/data_exchange.go) and
  [`extension_data.go`](../examples/go/extension_data.go) use the generated clients. Their tests
  run a local gRPC fixture host and a message-level parity check against the same contract.

```sh
cargo test -p gamenode-app --lib -- plugin_host::resources::tests::sdk_data_exchange
cargo test -p gamenode-app --lib -- plugin_host::resources::tests::sdk_extension_data
cargo test -p gamenode-plugin-sdk --examples
(cd examples/go && GOPROXY=off go test -count=1 ./...)
```

The scenarios cover performance history (publish, explicit selector, aggregate, revocation,
end-series cleanup), approved save/location inspection, save-profile ownership, one shared runtime
with several content identities, and hybrid/custom/opaque envelopes. They start no FPS, SMART or
hardware collector, open no Core dashboard, and no reply names the approved folder path; volume metadata is scoped and opaque.

### Which values Core interprets

| Value in a request or reply | Who decides it | Example behavior |
| --- | --- | --- |
| Selected game, installation and `active-host` computer context | Host, from the authenticated session | A batch for another game is refused; the consumer sees the same host-bound context |
| Sample units, interval, sequence, FPS method/statistic/quality, CPU scope/normalization, memory kind | Producer supplies; host validates and retains | `frames/second`, `percent`, `bytes`; a gap stays a gap |
| `source`, `tool_name`, `tool_version`, runtime and recipe IDs | Producer claim, descriptive only | Recorded as provenance; they do not prove a collector ran |
| `series_id`/`record_id` | Host-assigned owner token and current broker record | Category-scoped consent follows a replaced series |
| Approved folder handle and volume token | Host, from the user's approval | Inspection returns an opaque token; `mount_point`/`relative_path` never name the folder |
| Save-profile ownership and directories | Host, authenticated plugin/publisher/profile | A common selector is `UNIMPLEMENTED`; another plugin's profile is `NOT_FOUND` |
| Shared runtime provenance and extensions | Host | Executable handle is empty; the known namespace is validated, an unknown namespace stays a bounded opaque envelope |
| Custom dataset/schema identity and fields | Host enforces identity/limits; Core never interprets field meaning | Only reviewed fields, or the whole `opaque_payload`, reach a consumer |
| Extension `schema_id`, `schema_version`, `schema_digest` | Compared against signed package assets when a descriptor exists | A v9 payload is not validated as v1; without a descriptor the envelope is preserved, not projected as typed data |

Two common-data outcomes differ deliberately: a performance-history selector without a current grant
returns an empty list (history is absent, not an error), while a runtime-catalog selector without its
grant returns `PERMISSION_DENIED`. Save-profile routes answer `UNIMPLEMENTED` for any common selector
and stay owner-only. All three flows are asserted by both examples.

Shared extension namespaces are carried to another plugin as bounded opaque envelopes: the host
re-validates their structure and budget, but Core interprets no field inside a namespace the
recipient has no descriptor for. A consumer must not treat those values as identity, permission or
execution authority.

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
or delivery. A durable producer publication requires the separate
`storage.persist` grant; a consumer that copies a projection into its own
private namespace needs `storage.write` plus `storage.persist`. Uninstall Keep
does not grant a new consumer access to archived rows: the host must first apply
the reviewed reuse decision described above. Use the private `Storage*` API for
an owner's private bytes.

The Rust SDK exposes the generated client through
`HostClient::resource_client()`. Pass `HostClient::resource_context()` in each
request. The Rust SDK provides `common_data_selector` for categories,
`selected_record_selector` for record IDs and `common_data_selector_with_records`
for both. Other languages use the generated protobuf `ResourceHost` client
with the same request fields and host-side authorization rules.
