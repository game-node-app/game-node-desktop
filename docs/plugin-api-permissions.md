# Plugin API 1.4 method permissions

This page maps the API 1.4 `ResourceHost` methods to their required grants. The
route-level request/response types, caller ownership, streaming marker, and
contract notes are kept in the checked
[`resource_host.json`](../crates/plugin-protocol/contracts/resource_host.json)
catalog. A test compares every catalog entry with the generated protobuf
descriptor. Unknown routes remain `UNIMPLEMENTED`.

| Methods | Permission |
| --- | --- |
| `LibraryList`, `LibraryGet`, `LibraryWatch` | `library.read` (whole-library reads require `library.read.all`) |
| `MetadataUpdate` | `library.metadata.write` |
| `FavoritesList`, `FavoritesSet` | `favorites.read`, `favorites.write` |
| `MatchingSearch`, `MatchingAssign` | `matching.read`, `matching.write` |
| `ArtworkRead`, `ArtworkSet` | `artwork.read`, `artwork.write` |
| `GamesRequestLaunch` | `game.launch` |
| `GamesProposeCustomLaunch`, `GamesRequestCustomLaunch`, `GamesRemoveCustomLaunch` | `game.custom_launch` |
| `GamesRequestInstall` | `games.install` |
| `StorageGet`, `StoragePut`, `StorageDelete` | `storage.read`, `storage.write`, `storage.delete`; durable writes additionally require `storage.persist` |
| `FilesRequestAccess`, `FilesList`, `FilesRead`, `FilesWrite`, `FilesDelete` | corresponding `files.read`, `files.write`, `files.delete` grants, plus explicit game/root approval |
| `NetworkFetch` | `network.fetch` |
| `NotificationsPublish` | `notifications.publish` |
| `BackupsList`, `BackupsRequestCreate`, `BackupsRequestRestore` | `backups.read`, `backups.create`, `backups.restore` |
| `LibrariesList`, `LibrariesGet`, `LibrariesWatch` | `library.roots.read` |
| `LibrariesCreate`, `LibrariesUpdate`, `LibrariesAddRoot`, `LibrariesUpdateRoot` | `library.roots.write` |
| `LibrariesRemove`, `LibrariesRemoveRoot` | `library.roots.delete` |
| `RuntimesList`, `RuntimesGet`, `RuntimesWatch` | `game.runtime.read` |
| `RuntimesRegister`, `RuntimesUpdate` | `game.runtime.write` |
| `RuntimesRemove` | `game.runtime.delete` |
| `GamesRegisterContent` | `library.discovery` |
| `StorageDiagnosticsPublishBenchmark`, `StorageDiagnosticsPublishSmart` | `storage.diagnostics.publish` |
| `ComputerPublishHardware` | `computer.hardware.publish` |
| `GamePresencePublish`, `GamePresenceClear` | `game.presence.publish` |
| `GamePerformancePublishSamples`, `GamePerformanceEndSeries` | `game.performance.publish` |
| `DataPublish` | `data.publish` for declared custom data, or the domain-specific producer grant; durable publication additionally requires `storage.persist` |
| `DataGet`, `DataList`, `DataWatch` | read grant for the selected dataset/category; common records are excluded unless a query explicitly opts in |
| `PersistenceGetUsage`, `PersistenceListEntries`, `PersistenceDeleteEntries`, `PersistenceClear`, `PersistenceSetRetentionPolicy` | `storage.persist`, restricted to the caller's namespace |
| `ToolsList`, `ToolsRequestApply`, `ToolsRequestRemove` | `tools.read`, `tools.apply`, `tools.remove` |
| `PortsList`, `PortsDetail`, `PortsRequestInstall` | `ports.read`, `ports.install` |
| `JobsGet`, `JobsWatch`, `JobsCancel` | permission for the underlying operation and caller-owned job |

`GamesRequestLaunch` has a dedicated `GameLaunchRequest` and `game.launch`
grant. It uses the approved detected target and stored parameters without
per-call overrides. The custom-launch proposal, execution, and removal methods
use `CustomLaunchRequest` where applicable and the independent
`game.custom_launch` grant. Custom recipes require their own user review and
cannot be authorized by `game.launch`.

## Common data is an explicit per-query choice

`ResourceRequest.common_data_categories` is an optional typed `CommonDataSelector`. Its
`categories` list supports game paths, game performance history, runtime
catalogs, and content availability. Omitting the selector or sending an empty
list returns only data visible to the caller in its own scope. A plugin must
name each category on each list/get/watch query where it wants common records.
The host checks the matching `common.*.read` grant and its scope on every
request; activation grants only make those queries eligible and do not trigger
reads or delivery. Secrets and sensitive/custom datasets cannot be selected by
this message and continue to require their separate APIs and sharing review.

The former `include_common` boolean and string `selected_categories` fields
remain deprecated at their original wire numbers for draft-client decoding.
Hosts must ignore them for authorization and data inclusion.

## Host-side authorization audit

The host retains a bounded recent history of denied authorization checks for
diagnostics and permission UI. Each record identifies the acting plugin and
provider, requested permission, denial reason, and grant revision. The history
is available only to host code; it is not exposed through plugin RPCs. Session
credentials, operation identifiers, cookies, and request payloads are omitted.

This audit describes API enforcement. A native plugin process can still access
resources available to its operating-system identity, so API grants do not
constrain direct filesystem, network, or process access. Show that distinction
separately from the host-enforced grants.

## Current contract limitation

The current protobuf exposes the API 1.4 method names and separates ordinary
launch from custom launch, but most resource methods still accept the same
generic `ResourceRequest` and return `ResourceReply`. Arbitrary `payload` bytes,
string outcomes, and a generic event payload do not define stable method-level
request/response fields. In particular, the current `include_common` boolean
and string category list are deprecated; `common_data_categories` is the typed per-query
`CommonDataSelector` contract and preserves private-only behavior when omitted.
Data, libraries,
runtimes, persistence, files, backups, tools, ports, jobs, diagnostics, and
performance also need operation-specific typed payloads and outcome enums.

The generic envelope has fields for revision, cursor, page size, byte limits,
operation ID, idempotency key, and durability, but does not define which methods
require each field, method-specific page/quota bounds, revision-conflict
responses, retry behavior, terminal job outcomes, or operation ownership rules
as wire-level contracts. The JSON catalog records intended ownership and
semantics as guidance; it does not make these missing typed wire contracts
complete. Task 2.2 therefore remains open pending typed method contracts and
their generated-client fixtures.
