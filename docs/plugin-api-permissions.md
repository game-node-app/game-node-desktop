# Plugin API 1.4 method permissions

This page maps the API 1.4 methods to their required grants. `ResourceHost`
routes are listed in the checked
[`resource_host.json`](../crates/plugin-protocol/contracts/resource_host.json)
catalog; Saves, Locations, StorageDiagnostics, Computer, GamePresence and
GamePerformance routes are listed in
[`domain_host.json`](../crates/plugin-protocol/contracts/domain_host.json). Both
catalogs record ownership, permission, pagination, quota, revision, idempotency
and commit-time authorization. Contract tests compare every catalog route to
the generated protobuf descriptors. Unknown routes remain `UNIMPLEMENTED`.

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
| `LibrariesCreate`, `LibrariesAddRoot` | `library.roots.create` |
| `LibrariesUpdate`, `LibrariesUpdateRoot` | `library.roots.update` |
| `LibrariesRemove`, `LibrariesRemoveRoot` | `library.roots.delete` |
| `RuntimesList`, `RuntimesGet`, `RuntimesWatch` | `game.runtime.read` |
| `RuntimesRegister` | `game.runtime.create` |
| `RuntimesUpdate` | `game.runtime.update` |
| `RuntimesRemove` | `game.runtime.delete` |
| `GamesRegisterContent` | `library.discovery` |
| `StorageDiagnosticsPublishBenchmark`, `StorageDiagnosticsPublishSmart` | `storage.diagnostics.publish` |
| `ComputerPublishHardware` | `computer.hardware.publish` |
| `GamePresencePublish`, `GamePresenceClear` | `game.presence.publish` |
| `GamePerformancePublishSamples`, `GamePerformanceEndSeries` | `game.performance.publish` |
| `DataPublish` | `data.publish` for declared custom data, or the domain-specific producer grant; durable publication additionally requires `storage.persist` |
| `DataGet`, `DataList`, `DataWatch` | read grant for the selected dataset/category or record; public records are excluded unless a query explicitly opts in |
| `PersistenceGetUsage`, `PersistenceListEntries`, `PersistenceDeleteEntries`, `PersistenceClear`, `PersistenceSetRetentionPolicy` | `storage.persist`, restricted to the caller's namespace |
| `ToolsList`, `ToolsRequestApply`, `ToolsRequestRemove` | `tools.read`, `tools.apply`, `tools.remove` |
| `PortsList`, `PortsDetail`, `PortsRequestInstall` | `ports.read`, `ports.install` |
| `JobsGet`, `JobsWatch`, `JobsCancel` | permission for the underlying operation and caller-owned job |

The domain services use these grants:

| Service methods | Permission |
| --- | --- |
| `Saves.ListProfiles`, `Saves.GetProfile` | `game.saves.read` |
| `Saves.CreateProfile` | `game.saves.create` |
| `Saves.UpdateProfile` | `game.saves.update` |
| `Saves.RemoveProfile` | `game.saves.delete` |
| `Locations.Inspect`, `GetSize`, `GetAccess`, `GetLastModified`, `GetFingerprint`, `Compare`, `WatchAvailability` | `game.storage.inspect` or `game.saves.inspect`, according to the approved resource |
| `StorageDiagnostics.List`, `Get`, `Watch` | `storage.diagnostics.read` plus the user's explicit sensitive-data projection review |
| `Computer.List`, `Get`, `Watch` | `computer.hardware.read` plus the user's explicit sensitive-data projection review |
| `GamePresence.List`, `Get`, `Watch` | `game.presence.read` plus the user's explicit sensitive-data projection review |
| `GamePerformance.List`, `Get`, `Watch`, `QuerySeries`, `Aggregate` | `game.performance.read` and either the selected common category/record grant or the explicit sensitive-data projection review |

`GamesRequestLaunch` has a dedicated `GameLaunchRequest` and `game.launch`
grant. It uses the approved detected target and stored parameters without
per-call overrides. The custom-launch proposal, execution, and removal methods
use `CustomLaunchRequest` where applicable and the independent
`game.custom_launch` grant. Custom recipes require their own user review and
cannot be authorized by `game.launch`.

## Common data is an explicit per-query choice

List/get/watch and relevant data queries use an optional typed
`CommonDataSelector`. Its `categories` list supports game paths, game
performance history, runtime catalogs, content availability, library roots and
save profiles; `selected_record_ids` names individual published records. A
category selector requires its category grant, while each selected id requires
its record grant and does not require category-wide access. Omitting the
selector or sending it empty returns only data visible to the caller in its own
scope. Selectors request data; they do not grant authority. The host checks each
requested scope on every query. Activation grants only make queries eligible
and do not trigger reads or delivery. Sensitive/custom datasets still require
their separate sharing review, and secrets remain excluded.

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

## Typed operation contracts

Every `ResourceHost` and domain-service route has an operation-specific request
and response in `plugin.proto`; the checked route catalogs record that type
pair, required permission, ownership, streaming behavior, and operation semantics. Shared
`MutationContext` carries the host-issued operation ID, client idempotency key,
and expected revision. Read/list/watch requests use typed selectors and opaque
cursors, while mutation results use `MutationOutcome` instead of a free-form
string. Long-running operations return `JobReply` and stream typed `JobEvent`
states.

Ordinary launch uses `GameLaunchRequest`, which has no target or argument
override fields. Custom launch uses `CustomLaunchRequest` and a separate
`game.custom_launch` permission. This keeps user-approved customization from
being smuggled through the ordinary `game.launch` contract.

`ResourceRequest`, `ResourceReply`, and `ResourceEvent` remain as deprecated
draft compatibility messages only; no current service route uses them. Generated
descriptor fixtures check every route against its catalog, and contract tests
reject generic request/response envelopes on published services.
