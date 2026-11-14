# Plugin API 1.6 method permissions

This page maps the API 1.6 methods to their required grants. `ResourceHost`
routes are listed in the checked
[`resource_host.json`](../crates/plugin-protocol/contracts/resource_host.json)
catalog; Saves, Locations, StorageDiagnostics, Computer, GamePresence and
GamePerformance routes are listed in
[`domain_host.json`](../crates/plugin-protocol/contracts/domain_host.json). Both
catalogs record ownership, permission, pagination, quota, revision, idempotency
and commit-time authorization. Contract tests compare every catalog route to
the generated protobuf descriptors. Unknown routes remain `UNIMPLEMENTED`.

The route tables below describe the API contract, not a claim that every route
is currently served. `PluginApiServer` registers `PluginHost`, `ProviderHost`
and `ResourceHost`. In `ResourceHost` only `LibraryList`, `LibraryGet`,
`LibraryWatch`, `MetadataUpdate`, `FavoritesList` and `FavoritesSet` have
handlers; every other `ResourceHost` method and every
domain service (Saves, Locations, StorageDiagnostics, Computer, GamePresence,
GamePerformance) is not served yet, so those RPCs have no live handler that can
enforce the listed grants. `DescribeApi` lists exactly the served methods. The
persisted `PermissionGrantStore` also has not been wired into RPC sessions: the
Library, metadata and favorites handlers check the grants negotiated for the session. Treat the
per-route checks and common-data selectors below as requirements for the
unserved handlers, not as evidence that an unavailable route is protected by
runtime enforcement. Provider operation permissions, the permission-control
methods and the Library, metadata and favorites services that are mounted are enforced
by the host.

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
scope. Selectors request data; they do not grant authority. The contract
requires the host to check each requested scope on every query. Activation
grants only make queries eligible and do not trigger reads or delivery.
Sensitive/custom datasets still require their separate sharing review, and
secrets remain excluded. The common-data selector RPC handlers are not mounted
yet, as noted above.

The former `include_common` boolean and string `selected_categories` fields
remain deprecated at their original wire numbers for draft-client decoding.
Hosts must ignore them for authorization and data inclusion.

## Host-side authorization audit

The mounted provider API retains a bounded recent history (256 entries) of
authorization denials for diagnostics and permission UI. Each record identifies
the acting plugin and provider, requested permission, denial reason, and grant
revision. The history is available only to host code; it is not exposed through
plugin RPCs. Session credentials, operation identifiers, cookies, and request
payloads are omitted. This is currently an in-memory denial history for the
provider API; it is not yet a unified audit stream for all API 1.6 services.
The separate permission-grant store has a bounded persisted domain-mutation
audit model, but it is not yet connected to live RPC authorization.

## Library reads

`LibraryList`, `LibraryGet` and `LibraryWatch` are plugin-initiated, so they do
not belong to a provider operation. `OpenSession` returns a `resource_context`
for sessions that negotiated API 1.6 or later; every Library request carries it.
It is bound to the session, its kind and target game, and the active profile. It
adds no authority: the session still needs `library.read`, and a context from
before a profile change is stale. A missing account profile is `UNAVAILABLE`, an
unknown or foreign context is denied, and a missing grant is `PERMISSION_DENIED`
with a recorded audit event.

- **Scope.** With `library.read` a plugin sees only entries of its own provider.
  Naming another provider in `provider_id` does not widen that. `library.read.all`
  admits every provider and `provider_id` then narrows the result. A foreign
  installation is reported as `NOT_FOUND`, indistinguishable from an unknown id.
- **Identity.** `installation_id` is 128 random bits assigned and stored by the
  host the first time an entry is projected; it never changes across rescans,
  renames or match changes and reveals neither the store id nor a path. `game_id`
  is the matched GameNode game, or empty when the entry has no single match.
  Several installations may share one `game_id`. An entry is projected only once
  its id is stored, so a failed write hides it instead of issuing a temporary id.
- **Get.** The installation is the target. `game_id` alone works when exactly one
  admitted installation matches it; with several the call fails with
  `FAILED_PRECONDITION` and the caller names an `installation_id`. Naming both
  requires them to agree.
- **Fields.** Entries carry provider, name, install state (`installed`, `owned`,
  `shared`), a revision and the two ids. They carry no store id, install folder,
  scan fields, credentials or account data. `version` is empty.
- **Pagination.** Entries are ordered by `installation_id`. The page size defaults
  to 50 and is capped at 200. A cursor is signed by the host and bound to the
  query, session, profile, grant revision and library revision, and it expires
  after 5 minutes. A cursor from another query or session, an expired one, or one
  issued before the library changed fails with `FAILED_PRECONDITION` and no
  entries; the caller restarts the list. A cursor the host did not issue is
  `INVALID_ARGUMENT`.
- **Watch.** `after_revision = 0` starts at the current revision without a
  backlog. Otherwise the host replays up to 256 retained events after that
  revision; an older position is `FAILED_PRECONDITION` (list again and watch from
  its revision) and a future one is `OUT_OF_RANGE`. Events are `upserted` or
  `removed`. Each delivery rechecks the session, profile and grants, and the
  stream ends with the denial when they change. A session holds at most 8 watches,
  and a watcher that falls behind is ended and must list again.
- **Common data.** Selecting a common-data category or record (for example the
  runtime-availability projection) is refused with `UNIMPLEMENTED` rather than
  silently narrowed; that projection is planned with task 14.4.

## Display metadata and favorites

`MetadataUpdate` (`library.metadata.write`), `FavoritesList` (`favorites.read`) and
`FavoritesSet` (`favorites.write`) use the same resource context as the Library reads and only
ever reach the caller's own provider's entries, whatever other grants it holds.

- **Display name.** `display_name` is the only writable field. A request with another key in
  `display_fields`, or with none, is `INVALID_ARGUMENT`. The value is trimmed, at most 256 bytes
  and free of control characters; an empty value clears the override. The override is stored apart
  from the detected facts, so clearing restores the detected name. While set it is the library
  title, ahead of the matched game's name and the detected name, and it is exposed as
  `LibraryEntry.display_name` (empty when unset) next to the unchanged `name`.
- **Target.** The installation is named by `installation_id` (required). A `game_id`, if given,
  must be that installation's current match. A foreign or unknown installation is `NOT_FOUND`,
  even for a plugin that holds `library.read.all`.
- **Revision.** `expected_revision` must be the entry's `metadata_revision`: 0 until the name is
  first written, then the revision of the last change, which only grows (clearing counts). An
  obsolete revision returns `MUTATION_OUTCOME_REVISION_CONFLICT` with the current revision and
  writes nothing. Writing the value the entry already has is `MUTATION_OUTCOME_ALREADY_APPLIED`
  with an unchanged revision; a change is `MUTATION_OUTCOME_APPLIED` with the new revision.
- **Favorites.** A favorite belongs to the GameNode game, never to the entry, and a rematch does
  not move it. `FavoritesList` returns the favorite state of each GameNode game matched by an
  entry of the caller's provider, ordered by game id, with the Library paging and cursor rules
  (its own cursors; a changed library or another query makes one stale). `FavoritesSet` needs an
  own entry currently matched to that game, otherwise `NOT_FOUND`, which is also the answer for a
  game only another provider has. It goes through the same local-first path as the user's own
  star, so syncing to GameNode still follows the user's sync setting. Setting the state the game
  already has is `ALREADY_APPLIED`. `profile_id` must be empty or the active profile's id, or the
  call is `FAILED_PRECONDITION`.
- **No favorite revision.** Favorites carry no revision: `expected_revision` must be 0, replies
  and list entries carry revision 0, and a write is a desired-state, last-write-wins setting. This
  deviates from per-resource revisions on purpose, because the local state has no revision to
  guard and an idempotent boolean has nothing to conflict with.
- **Idempotency.** Every mutation needs an `idempotency_key` of 1 to 128 bytes without control
  characters. The host remembers committed outcomes per plugin and profile for 24 hours, at most
  1024 keys per plugin; a retry of the same key and request returns the original outcome, while
  the same key with another request is `FAILED_PRECONDITION`. Conflicts are not remembered. The
  table is in memory, so a replay after a restart is not recognized; it still fails safely,
  because metadata is guarded by its revision and favorites are idempotent by nature.
- **Commit and ordering.** Mutations run one at a time. Authorization, including the grant
  revision, is checked again where the change commits (under the engine lock for favorites, on
  the storage worker for metadata), so a revocation that wins the race stops the write. A failed
  database write is `UNAVAILABLE`.

## Matching

`MatchingSearch` (`matching.read`) and `MatchingAssign` (`matching.write`) use the same resource
context as the Library reads. They reuse the library's own match policy, so a plugin never opens or
disturbs the match dialog the user may have open.

- **Search.** One bounded page of GameNode catalog games for a title. `query` is trimmed, 1 to 128
  bytes and free of control characters. `page_size` defaults to 20 and is capped at 50; a
  catalog search has no further pages, so `next_cursor` is always empty and a non-empty `cursor`
  is `INVALID_ARGUMENT`. `platform` is reserved and must be empty, because the catalog has no
  platform filter. Each `MatchCandidate` carries `game_id`, `title` and `release_year` (0 when
  unknown); `platform` and `confidence` are empty because the catalog has neither, and the host
  never invents a score. A plugin may start 20 searches per minute (across its sessions in a
  profile); more is `RESOURCE_EXHAUSTED`. Search spends the user's verified GameNode session, so
  without a verified, online session, or when GameNode cannot answer, it is `UNAVAILABLE`: an
  unavailable catalog is never reported as an empty result.
- **Assign.** Matches one of the caller's own entries to a GameNode game. The entry is named by
  `installation_id` (required); `provider_id` and `provider_game_id` are reserved and must be
  empty, and `gamenode_game_id` is required. A foreign or unknown installation is `NOT_FOUND`,
  even with `library.read.all`. The game must be one the host already knows (in the library's game
  records) or one of the last 256 games plugin searches returned; any other game is `NOT_FOUND`,
  so a plugin finds a game with `MatchingSearch` before assigning it.
- **Revision.** `expected_revision` is the entry's `LibraryEntry.revision`, which changes whenever
  the entry's matched game, name, install state or display metadata changes. A stale revision
  returns `MUTATION_OUTCOME_REVISION_CONFLICT` with the current revision and writes nothing; the
  plugin rereads the entry and retries. A change returns `MUTATION_OUTCOME_APPLIED` with the entry's
  revision afterwards, and `resource_id` is the installation id.
- **Already applied.** An entry that already has an explicit choice of that game, made by the user
  or by a plugin, is `MUTATION_OUTCOME_ALREADY_APPLIED` with its current revision, whatever
  `expected_revision` says, and nothing is written. An entry that is only matched automatically is
  not the same: assigning the same game stores an explicit choice, so later automatic rematching
  keeps the game.
- **The user's choice wins.** The stored choice records who made it. A plugin can set or replace
  its own choices, but when the user chose a game, or chose "no GameNode game", for the entry in the
  match dialog, `MatchingAssign` is `FAILED_PRECONDITION` and changes nothing. The user choosing
  again, including the game a plugin picked, makes the choice theirs; "forget my choice" in the
  library removes it whoever made it. A plugin cannot clear a match.
- **Effects.** The choice is stored through the same path as the user's pick: it is per account,
  survives a restart, and playtime totals are submitted for the new match as they are for the
  user's own. A favorite stays on the game it was set on and is never moved by a rematch.
- **Idempotency and commit.** The idempotency, serialization and commit-time authorization rules of
  the other mutations apply. Conflicts and refusals are not remembered, so a retry with the same
  key and a fresh request is evaluated again.

### Matching limits

The matching limits are constants in `crates/app/src/plugin_host/resources/matching.rs` and
`crates/core/src/library/plugin_match.rs`. They were chosen by reasoning from limits the host
already has, not from measurements, and are open to change after a capacity review.

| Limit | Value | Why |
| --- | --- | --- |
| Search query | 1 to 128 bytes | Titles are short. Half of the 256 characters the user's own match dialog accepts, and the same 128 bytes as idempotency keys and provider ids, so a plugin cannot use the user's session to send large payloads. |
| Results per search | default 20, at most 50, one page | The GameNode search endpoint is asked for 20 results (`SEARCH_LIMIT` in `crates/api/src/game.rs`), so 20 is what the user's own dialog shows and is the practical maximum today. 50 leaves headroom if that changes and stays well under the 200-item page cap of the Library lists. The catalog has no paging, so there is no cursor. |
| Searches per plugin | 20 per 60 seconds, per plugin and profile | One search every 3 seconds on average. That is enough for interactive matching of a screenful of entries and low enough that a plugin cannot turn the user's verified GameNode session into a bulk crawler. The window is a plain sliding minute and only counts accepted attempts. |
| Assignable offered games | 256, oldest dropped first | Bounds the memory a plugin can occupy with search results. Matches the 256 events the library watch retains, and is large enough for several searches of 20 results. Games already in the library's records are always assignable. |
| Installation id in Assign | at most 128 bytes | Host-issued ids are 128 random bits as hex; the bound only rejects abuse before lookup. |
| `release_year` in `MatchCandidate` | additive field, 0 when unknown | Titles alone do not separate remakes and reboots. The catalog already returns the year, and the user's dialog shows it. |

Search spends the user's session, so the rate limit is the one most worth revisiting once there are
real plugins; the others only bound size.

## Account-bound plugin profiles

Third-party plugins run inside the profile of the signed-in GameNode account.
GameNode has no signed-out or local usage mode, so there is no anonymous plugin
profile. The engine reports the verified account, or the cached last account
while a restored session is still being verified offline. Without an account,
`ProviderApi` issues no profile context, refuses to start a third-party process
and answers profile-scoped services with `UNAVAILABLE`. That is a missing
session, not a permission denial, so it records no denial event.

Sign-out, a rejected session and a verified account that differs from the
cached one are profile changes. Each advances the profile epoch, so every
context, lease and namespace resolver issued for the previous profile becomes
stale before its session stops. The host then disconnects the third-party
sessions. Verifying the cached account is not a change and keeps them running.
Official bundled providers are not bound to the profile and keep running across
these changes.

Profile IDs are opaque hashes of the account ID, safe as directory names, and
do not reveal the account in paths or namespace keys. State is layered per OS
user: installed packages (`packages/`) and publisher trust (`trust/`) are
shared by every account of that OS user, while activation, grants,
configuration and private data live in `accounts/<profile id>/`. Another
account on the same OS user sees the installed packages but not the previous
account's activation, grants or data. The layout is modelled by
`PluginStateLayout`; it is not yet bound to an application-data root, because
managed third-party packages are not installed by the desktop yet.

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
