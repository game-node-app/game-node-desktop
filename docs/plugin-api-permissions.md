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
| `JobsGet`, `JobsWatch`, `JobsCancel` | the permission of the operation that created the job, plus ownership of the job (see [Jobs](#jobs)) |

The domain services use these grants (of them, only the `Saves` profile methods and `Locations.GetSize`, `GetAccess` and `GetLastModified` are served by this build; see [Save profiles](#save-profiles) and [Inspecting approved places](#inspecting-approved-places)):

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
cannot be authorized by `game.launch`. `GamesRequestLaunch` and `GamesRequestInstall` are served
and need the user's intent, described under [Launching and installing](#launching-and-installing);
the custom-launch methods are not served yet.

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
  the entry's matched game, name, install state, display metadata or cover changes. A stale revision
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

## Artwork

`ArtworkRead` (`artwork.read`) and `ArtworkSet` (`artwork.write`) use the same resource context as
the Library reads and only ever reach the caller's own provider's entries, whatever other grants
it holds. The one artwork kind is the library **cover**: `artwork_kind` is `"cover"` or empty, and
anything else is `INVALID_ARGUMENT`.

- **Target.** The entry is named by `installation_id` (required, at most 128 bytes). A `game_id`,
  if given, must be that installation's current match. A foreign or unknown installation is
  `NOT_FOUND`, even with `library.read.all`, and `ArtworkSet` does not decode a single byte for it.
- **Set is an override of the entry, not of the game.** The image becomes the cover of that one
  entry. The matched game's own cover, which every entry of the game shares, is never touched, and
  neither are other entries or other providers. The override is stored apart from the detected
  facts and exposed as `LibraryEntry.artwork_revision`; it is the entry's cover in the library
  until it is cleared. An empty `encoded_image` (with an empty `media_type`) clears it and
  restores the game's cover. A `media_type` with no image is `INVALID_ARGUMENT`.
- **The host stores its own picture.** The bytes are untrusted. They are bounded first, then
  decoded, shrunk and encoded again on a blocking worker; what is stored is the host's output, so
  metadata (EXIF, ICC), trailing data and anything else the decoder skipped never reach the disk
  or another reader. See the limits below. Refused images are `INVALID_ARGUMENT` with a message
  naming the reason: not a PNG, JPEG or WebP picture, a `media_type` that does not match the
  bytes, an animation, too many pixels, a damaged file or too many bytes. Nothing is written for
  a refused image.
- **Revision.** `expected_revision` must be the entry's `artwork_revision`: 0 until a cover is
  first set, then the revision of the last change, which only grows (clearing counts). An obsolete
  revision returns `MUTATION_OUTCOME_REVISION_CONFLICT` with the current revision and writes
  nothing. Setting the picture the entry already has (the same bytes after processing) is
  `MUTATION_OUTCOME_ALREADY_APPLIED` with an unchanged revision; a change is
  `MUTATION_OUTCOME_APPLIED` with the new revision, and `resource_id` is the installation id.
  A cover change also changes `LibraryEntry.revision`.
- **Read.** `ArtworkRead` returns the cover the entry shows: the plugin's override if it set one,
  else the matched game's own cover when it is already on this device. Nothing is downloaded for a
  read, so an entry with no cover yet, a missing file or no matched game is `NOT_FOUND`. The reply
  has the encoded image, its `media_type`, its `byte_length` and the entry's `artwork_revision`
  (0 while the cover is the game's own). `max_bytes` is the caller's budget: 0 means the 5 MiB
  maximum, a larger value is capped to it, and an image bigger than the budget is
  `RESOURCE_EXHAUSTED` (nothing is truncated).
- **Resource references.** The image comes inline (`encoded_image`) or from a file the plugin may
  read (`file_handle`, with `media_type` set as for inline bytes). Giving both, or a `file_handle`
  with no `media_type`, is `INVALID_ARGUMENT`. The handle is the only reference there is: there is
  no field that names a path or a URL, and a made-up, edited, expired or other-session handle is
  `NOT_FOUND` like any other. The handle is checked as for `FilesRead`, and more:
  - the plugin needs `files.read` besides `artwork.write` (without it, `PERMISSION_DENIED` and
    nothing is read);
  - it must be a regular-file handle of a current approval of **this same installation**; a handle
    from the approval of another of the plugin's entries is `PERMISSION_DENIED` and a directory is
    `FAILED_PRECONDITION`. A revoked approval or a revoked grant stops it at once;
  - the host reads the file itself, at most 4 MiB plus one byte, on the plugin's namespace worker,
    so a file over the bound is refused as "larger than 4194304 bytes" (the inline wording) without
    being read whole, and the bytes then go through the same worker-side decode, pixel check and
    re-encode as inline bytes. A damaged file or one whose type does not match is refused the same
    way. The plugin never holds the bytes, and the host stores only its own picture;
  - a retry is answered before the file is read again, from the handle and the declared type, so a
    file that has since changed or gone does not change the answer to the same key. A different
    file needs a different key, and `files.read` is checked again at commit.
- **Idempotency and commit.** The idempotency, serialization and commit-time authorization rules of
  the other mutations apply. A retried key is answered before its image is decoded. The file is
  written before the database row and the replaced file is deleted after it, so a crash leaves at
  worst an unreferenced file, never a row that points at nothing.

### Artwork limits

The limits are constants in `crates/platform/src/plugin_artwork.rs` and
`crates/app/src/plugin_host/resources/artwork.rs`. Like the matching limits they were chosen by
reasoning from limits the host already has, not from measurements, and are open to change after a
capacity review. D31 had already fixed the formats (static PNG, JPEG, WebP) and that bytes and
pixels are bounded; the numbers are engineering detail.

| Limit | Value | Why |
| --- | --- | --- |
| Encoded image | at most 4 MiB, inline or read from a file by handle | Plugin messages are capped at 8 MiB; this leaves room for the request around the bytes. The host's own cover downloads are capped at 5 MiB, and a 4 MiB source is far more than a cover needs. |
| Source pixels | at most 4096 x 4096 (16,777,216), no side over 8192 | A cover is shown at a few hundred pixels, so 16 megapixels is generous. The pixel count is read from the file header and checked *before* decoding, so a small file that expands into a huge bitmap is refused without allocating it. The side limit keeps thin strips from bypassing the count. |
| Decode memory | 128 MiB allocation limit; two decodes at once | A 16-megapixel RGBA16 image is 128 MiB. Two concurrent decodes bound what plugin pictures can occupy at any moment to a quarter gigabyte; more wait. |
| Stored size | fits in 600 x 900, aspect kept, never enlarged | The library grid draws covers at roughly 200 to 300 pixels wide; 600 x 900 (2:3, like the game covers) is about twice that for high-density screens. Stored covers are at most a few hundred KiB, so a plugin cannot grow the covers folder much, and each entry keeps at most one override file. |
| Output format | JPEG (quality 88), or PNG when a pixel is transparent | Covers are JPEG everywhere else. Transparency would be lost in JPEG, and a fully opaque alpha channel is not worth a larger PNG. |
| Formats | static PNG, JPEG, WebP | From D31. GIF, BMP, SVG and anything else are refused by signature; animated PNG (APNG) and animated WebP are refused. The signature decides, not the declared `media_type`, which must agree. |
| Read budget | `max_bytes` 0 means 5 MiB, capped at 5 MiB | The largest cover the host stores is a downloaded one at the 5 MiB download cap. |
| Installation id | at most 128 bytes | Same as the other mutations. |

There is no per-plugin rate limit for `ArtworkSet`. Writes are serialized, at most two pictures are
decoded at once and a retried key does no decoding, which bounds CPU and memory but not how often a
plugin may replace its cover. Revisit this with the rate limits once there are real plugins.

## Private storage

`StorageGet` (`storage.read`), `StoragePut` (`storage.write`, plus `storage.persist` for durable
writes) and `StorageDelete` (`storage.delete`) keep a plugin's own small, non-secret data. They use
the same resource context as the other services and need no other grant.

- **The namespace is the plugin, not a request field.** Data belongs to the authenticated plugin id,
  its pinned publisher fingerprint and the active GameNode account profile. A request carries a
  `key` and nothing that could name another owner, a path or another profile, and a key that looks
  like a path or an identity (`../x`, `org.other/k`) is just a key in the caller's own namespace.
  Two plugins, two publishers of the same plugin id and two accounts never see each other's keys,
  usage or quota. Official providers use trusted host adapters instead and get
  `FAILED_PRECONDITION`.
- **Durability is explicit.** `StoragePut.durability` must be `SESSION_ONLY` or `PERSISTENT`;
  `UNSPECIFIED` is `INVALID_ARGUMENT`. A `PERSISTENT` write without `storage.persist` is
  `PERMISSION_DENIED` (the denial detail names `storage.persist`) and stores nothing, in memory or on
  disk: the host never turns it into a volatile success. `SESSION_ONLY` needs only `storage.write`.
  Reads and deletes of values already stored need only `storage.read` and `storage.delete`, so
  revoking `storage.persist` stops new durable writes without hiding or trapping existing data.
- **Two layers, one key space.** A *session-only* value stays in the host's memory and is discarded
  when the plugin's last open session ends or the account profile changes. A *persistent* value
  survives restarts until it is deleted or expires. A key lives in one layer at a time: writing it
  with the other durability moves it (its previous copy is removed), and `StorageGetReply.durability`
  says where it is now.
- **Revisions.** A key's revision is 0 while it does not exist and `expected_revision` of the next
  put or delete must equal it, so a first write sends 0. An obsolete revision returns
  `MUTATION_OUTCOME_REVISION_CONFLICT` with the current revision and changes nothing. Revisions come
  from one counter per namespace that survives deletions, layer moves and restarts, so a key that is
  deleted and created again never repeats a revision a stale writer may still hold. A put always
  writes (`MUTATION_OUTCOME_APPLIED`); deleting a key that is not there is
  `MUTATION_OUTCOME_ALREADY_APPLIED` with revision 0, whatever `expected_revision` says.
- **Lifetime.** `ttl_seconds` 0 means the value does not expire; otherwise it expires that many
  seconds after the write, up to 30 days (`INVALID_ARGUMENT` beyond). `expires_at` (Unix seconds,
  0 for none) is returned by the put and the get. An expired value is a key that does not exist and
  frees its space.
- **Quota.** Each layer of a namespace may hold `QUOTA_BYTES` (16 MiB) of logical data. A write
  that would exceed it is `RESOURCE_EXHAUSTED` ("nothing was changed") and leaves every stored value,
  including the one it would have replaced, as it was: durable writes check the quota and write in
  one transaction. The put reply carries `usage_bytes` and `quota_bytes` of the layer written.
- **Idempotency and commit.** Writes and deletes need an idempotency key like the other mutations. A
  retry with the same request returns the original outcome without writing again; the same key with
  another request is `FAILED_PRECONDITION`; a revision conflict or a failure is not remembered. The
  caller's grants and the grant revision are checked again immediately before the first change, so a
  revocation that wins the race stops the write.
- **No physical paths.** Replies and errors never contain a path or the name of a file. Durable data
  is in a private store the host keeps for the plugin and the account, below its own data folder;
  only the user, through the host's own administration, can see where. A plugin cannot ask for it,
  and the Files service excludes every host persistence folder.
- **Non-secret versus Secret.** Storage is for preferences, caches and bookkeeping. It is not
  encrypted and the host does not treat it as sensitive. Credentials, tokens and anything that must
  not be readable by other software on the machine go through the provider `Secret` service, which
  maps a logical key to the OS credential store (see "Provider secrets" in the development guide).
  Secrets are never kept here.

### Storage limits

The constants are in `crates/platform/src/plugin_storage.rs`. The value size and the 16 MiB quota
come from the design; the rest were chosen by reasoning, not measured, and are open to change.

| Limit | Value | Why |
| --- | --- | --- |
| Key | 1 to 256 bytes, no control characters | Keys are names, not data; the bound keeps the index small. |
| Value | at most 1 MiB | From the design. A value is read and written whole, and plugin messages are capped at 8 MiB. |
| Quota per layer | 16 MiB of logical bytes | From the design ("16 MiB private-data quota"). The 64 MiB managed total of the design also covers data of other services, which are not implemented yet. |
| Per-entry overhead | 64 bytes charged on top of key and value | An empty value must not be free, or a plugin could keep millions of keys without being charged. With it, 16 MiB holds at most about 260,000 entries. |
| Lifetime | `ttl_seconds` 0 (none) to 30 days | A plugin cannot ask for a lifetime it cannot also be cleaned up after; long-lived data uses no expiry. |
| Layers | quota applies to each layer alone | The session-only layer is host memory, so 16 MiB per plugin is the most it can hold; the durable layer has its own 16 MiB. |

The quota counts logical bytes, not the size of the store file, which is larger (SQLite pages and
indexes). There is no per-plugin rate limit on writes; they are serialized per namespace, which
bounds contention but not frequency. The user-facing cleanup, usage screens, uninstall choices and
the `Persistence*` methods are separate tasks (16.x, 17.2).

## Files

`FilesRequestAccess`, `FilesList` and `FilesRead` (`files.read`), `FilesWrite` (`files.write`) and
`FilesDelete` (`files.delete`) let a plugin work on files of a game installation. They are the
only way a plugin touches the user's files through the API, and they are closed by default: the
three grants are separate, off at installation, and even with them a plugin reaches only a folder
**the user approved for that plugin and that one installation**.

- **Approval is the user's, per plugin and installation.** `FilesRequestAccess` names an
  `installation_id` (one of the caller's own library entries; a `game_id`, if given, must be its
  match), the operations wanted (`read`, `write`, `delete`) and the kinds of folder
  (`installation`, `save`, `prefix`; empty means `installation`). It carries no path. The host asks
  the user to choose the folder (starting at the installation's folder for `installation`) and then
  to confirm the final scope: folder, operations and every game that shares it. A refusal is
  `PERMISSION_DENIED`; a build with no approval dialog connected is `FAILED_PRECONDITION`; the
  user has five minutes to answer; only one approval per plugin waits at a time (`ABORTED`).
  Each requested operation needs its grant first, so the user is never asked for something the
  plugin may not even request.
- **A plugin may suggest where the folder is.** `declared_locations` (additive) holds, per kind
  `save` or `prefix`, where that folder usually is: an ordered list (earlier is preferred, at most 8
  per kind), each with a `location` and a one-line `description`. A suggestion is **only shown to the
  user**, who takes one or chooses another folder; it grants nothing, and approval still needs the
  user's confirmation of the full scope.
  - `location` is an absolute path (`/opt/games/x`, `C:\Games\x`) or starts with a placeholder the
    host expands: `{home}`, `{documents}`, `{appdata}` and `{localappdata}` (Windows), `{xdg_data}`
    and `{xdg_config}` (Linux), optionally followed by plain names separated by `/` or `\`.
    The text is at most 1024 bytes without control characters.
  - A well-formed suggestion that cannot apply on this system is **dropped without an error**: a
    placeholder or a drive or share path of the other system, a system folder that does not exist
    (such as `{documents}` with no XDG user directory), a folder inside GameNode's private data, or
    a duplicate. One list can therefore serve every platform.
  - What is wrong with the request itself is `INVALID_ARGUMENT`, before the user is asked: an
    unknown placeholder, a relative path (`saves/x`, `~/x`), `..`, an empty, overlong or
    control-character text, a kind that is not `save` or `prefix` (the host already knows where an
    installation is), more than 8 for one kind, or a description over 120 bytes, over one line or
    with control or disguising characters (the notification text rules). The message never echoes
    the location.
  - Each suggestion reaches the approver labelled as declared by the plugin and not verified, with
    whether the folder exists (only existence is checked: nothing is listed, opened or read). The
    folder to start on is the host's own suggestion (an installation's known folder), else the first
    suggestion that exists. The approver shows the description as plain text.
  - The plugin is told nothing about which suggestions exist, which were shown or which was taken:
    the reply is the same handles as without any suggestion. The suggestions of an approval that
    already stands and covers the request are not looked at, and a read-only approval is not widened
    by a suggestion: asking for `write` asks the user again.
  - The approval records how the folder was chosen: `declared` if it is the same folder (by
    canonical path) as a suggestion of the plugin, `user` otherwise. It is shown to the user, not
    to the plugin.
- **What is remembered.** The approval is stored per plugin and account profile (beside its private
  storage, not charged to its quota) and replaced when the same installation and kind are approved
  again. Asking for the same or fewer operations later returns handles without asking; asking for
  more asks again. It holds only while the set of games sharing the folder is still the one the user
  confirmed: a game added to or removed from the folder, or a changed or replaced folder, stops it
  (`FAILED_PRECONDITION`, "request access again") until it is approved again.
- **Shared folders.** The games that "share" a folder are the library entries, of any provider,
  whose install folder is the root, inside it or contains it (links resolved). Approving a parent
  such as a library folder names every game under it, and grants nothing across them beyond what
  was confirmed and the operations approved: an approved read-only parent does not allow
  writes, and a plugin approved for one game's folder has no way to name a sibling's.
- **Handles are opaque and belong to a session.** `FileHandle.handle` (and each `FileEntry.handle`,
  and the `resource_id` of a write or delete) is a sealed token: it carries the approval and a
  relative path but cannot be read, edited, forged, kept past the session or used by another
  session, and two for the same file differ. A malformed, foreign, edited or revoked handle is
  `NOT_FOUND`, all the same. The reply never contains an absolute path.
- **Every call re-checks.** From the handle the host re-derives: the approval exists and is not
  revoked, it allows that operation, the games sharing the folder are the confirmed set, the folder
  is still the approved one (same volume and node), and the plugin's grant for the operation is held
  now. All of that, and the file work, run under the plugin's namespace lock, and a write or delete
  rechecks the grants right before it changes anything, so a revocation cannot slip in between.
- **Confinement.** Below the root only plain path components exist. `..`, empty or absolute names,
  separators, control characters and names over 255 bytes are refused before the disk is touched.
  Symbolic links, junctions and other reparse points are never followed at any depth (a link is not
  listed and naming one is `PERMISSION_DENIED`), so a folder swapped for a link after a listing is
  refused when used; a directory on another volume is refused; devices, pipes and sockets do not
  exist for the plugin. On Linux every step is an `openat` with `O_NOFOLLOW` from a directory
  handle. On Windows each component is checked as a reparse point and the opened handle's final
  path must be exactly the root plus the requested components.
- **The host's own folders are excluded.** The application data folder (database, private
  plugin storage, packages, credentials) is reserved: it cannot be approved, a root inside it is
  refused, and when an approved parent contains it the folder is not listed and nothing beneath it
  can be reached. This holds even if the user picks a parent that contains it.
- **List.** `FilesList` takes a directory handle and returns entries sorted by name with opaque
  child handles, size and modification time (Unix seconds); directories have size 0. `page_size` 0
  is 100, at most 200. The cursor is bound to the session and the directory; an edited or foreign
  cursor is `INVALID_ARGUMENT`. A directory with more than 20,000 entries is `RESOURCE_EXHAUSTED`
  instead of a partial list.
- **Read.** `FilesRead` returns up to `max_bytes` from `offset` (0 means 1 MiB; at most 4 MiB),
  `next_offset` and `eof`. A directory handle is `FAILED_PRECONDITION`.
- **Write.** With `name` empty, `file_handle` is an existing file: `contents` go at `offset`, which
  may not pass the end of the file (`OUT_OF_RANGE`), and with `truncate` the file ends where the
  write does. With `name` set, `file_handle` is a directory handle and the file `name` is created,
  or overwritten, inside it; a new file can only be written from offset 0, and a refused write
  creates nothing. New names are one plain component that is valid on every platform: no
  separators, `.`/`..`, `< > : " | ? *`, trailing space or dot, or reserved device names
  (`CON`, `NUL`, `COM1`...), else `INVALID_ARGUMENT`. One call carries at most 4 MiB
  (`INVALID_ARGUMENT`, write in several calls) and a file may not grow past 512 MiB
  (`RESOURCE_EXHAUSTED`). The reply's `resource_id` is the handle of the written file.
- **Delete.** `FilesDelete` removes one regular file. Directories are never deleted, and nothing a
  link points to is.
- **No revisions.** Files carry no revision: `expected_revision` must be 0.
- **Idempotency.** Writes and deletes need an idempotency key like the other mutations. A retry of
  the same request returns the original outcome without acting again (so a retried append applies
  once, and a retried delete is not `NOT_FOUND`); the same key for another request is
  `FAILED_PRECONDITION`. `FilesRequestAccess` needs a key for its shape but is naturally
  idempotent: an existing approval is reused.
- **Errors** never contain a path. `PERMISSION_DENIED` is outside the approved access, a missing
  approval for the operation or an operating-system refusal; `NOT_FOUND` a missing file or unusable
  handle; `FAILED_PRECONDITION` the approved folder or its sharing changed, or the wrong kind of
  entry; `UNAVAILABLE` an I/O failure.
- **Not a sandbox.** These rules govern what the *API* does for a plugin. A plugin that runs as the
  user can still use the operating system directly; nothing here confines native code.

### File limits

The constants are in `crates/platform/src/plugin_files.rs` and `crates/app/src/plugin_host/files.rs`.
None of the numbers comes from the design or a measurement: they were chosen by reasoning and are
open to change after a capacity review.

| Limit | Value | Why |
| --- | --- | --- |
| Bytes per read or write call | at most 4 MiB (read default 1 MiB) | Plugin messages are capped at 8 MiB; 4 MiB leaves room for the request and bounds memory per call. Bigger files are moved in pieces. |
| Size of a file a plugin may grow | 512 MiB | Saves and memory cards are small; this keeps a runaway plugin from filling a disk in one chain of calls. It does not bound the total a plugin may write. |
| Entries read from one directory | 20,000 | A listing sorts and pages the whole directory each call, so the work per page is bounded. A larger directory is refused rather than cut short. |
| List page | default 100, at most 200 | Same as the other paged services. |
| Path depth, name length | 64 components, 255 bytes | Common filesystem limits; keeps handles small. |
| Path carried in a handle | at most 2,048 bytes | Entries deeper than that are not listed. |
| Time to answer an approval | 5 minutes | Long enough to find a folder, short enough not to leave a plugin waiting forever. |
| Remembered canonical library folders | 60 seconds | The sharing check runs on every call; this avoids resolving every library folder each time. |

No per-plugin rate limit exists for file calls. Mutations of one plugin are serialized, and its
private-storage calls wait while one of its file calls runs, because both use the plugin's
namespace lock.

### What is not covered yet

- The suggestions of `declared_locations` reach the approver (`RootRequest.declared` and
  `.preselected`), but no real dialog shows them yet: the approval dialog is task 10.9, which also
  shows the "declared by the plugin, not verified" label, the existence marker and a folder chooser.
  The Windows expansion of `{appdata}`, `{localappdata}`, `{documents}` and drive or share paths is
  unit-tested through a stand-in for the system folders and type-checked for Windows, but has not
  run on Windows.
- The approval dialog belongs to the settings UI (section 10). Until a build connects one
  (`FileAccess::set_approver`), `FilesRequestAccess` answers `FAILED_PRECONDITION` for a new
  approval; existing approvals keep working.
- The sharing check knows only the library's install folders. A save or prefix folder outside every
  install folder may be shared with games the host cannot see; the confirmation names the games it
  knows and the user judges the rest.
- Revoking an approval is a store operation the settings UI will call; no plugin API revokes one.
- Approval is limited to the caller's own library entries. Saves and runtime handles that exist
  without a library entry arrive with the save-profile and location services.
- The Windows implementation was type-checked for the Windows target but has not been run; see
  "Limitations" in `docs/development.md` for native acceptance.

## Network fetch

`NetworkFetch` (`network.fetch`) lets a plugin ask the host to fetch an HTTPS URL. The host is the
one on the network, so every rule below is enforced before data reaches the plugin.

- **Two things are needed: the grant and an approved origin.** An origin is exactly scheme, host
  and port (`https://api.example.com`, or `https://api.example.com:8443`); the default port is left
  out and the host is lowercased. There are no wildcards, no paths and no prefixes: `www.` or
  another subdomain, another port, `http`, and `api.example.com.evil.test` are all other origins.
  Nothing is reachable until the user approves one, and the approved set is asked again for every
  fetch and for every redirect, so approving, widening, narrowing or revoking takes effect on the
  next call and stops a fetch already running. A refusal for scope is `PERMISSION_DENIED` with
  the `DENIAL_REASON_SCOPE_DENIED` detail naming `network.fetch`, and is audited.
- **Only public addresses.** The host name is resolved by the client's own resolver, and the
  connection is refused when **any** answer is loopback, private (10/8, 172.16/12, 192.168/16),
  link-local (including the cloud metadata address 169.254.169.254), shared address space
  (100.64/10), unique-local or site-local IPv6, multicast, unspecified, documentation, benchmarking,
  reserved, Teredo, or an IPv4-mapped, NAT64 or 6to4 form that embeds such an IPv4 address. The
  client connects to exactly the addresses that passed, so a name cannot pass the check and then
  point elsewhere. A URL that names an address (`127.0.0.1`, `[::1]`, `2130706433`, `0x7f.1`) is
  judged the same way before connecting. This holds even for an origin the user approved: a
  plugin cannot use the host to reach the user's own machine or network.
- **Redirects** are followed by the host, at most 5, and every target must be an https URL of an
  approved origin, checked before anything is sent to it. A redirect to an unapproved origin ends
  the fetch with `PERMISSION_DENIED`; the plugin is never told where it pointed and the redirect
  response's body is never read. `Authorization`, `Cookie` and `Proxy-Authorization` that the plugin
  sent do not follow a redirect to another origin. 303, and POST after 301/302, continue as a GET
  without a body; 307 and 308 repeat the method and body.
- **No credentials of GameNode.** The host adds none: no cookies, no authorization, no referer,
  no GameNode session or token. The only header it adds is a `User-Agent`
  (`GameNode-Desktop/<version> plugin-fetch`), which the plugin may replace. A plugin may send its own
  `Authorization` or `Cookie` for its own service.
- **Nothing is shared between fetches.** There is no connection pool, no cookie jar and no
  proxy. The system proxy settings are deliberately ignored: a proxy resolves names itself and would
  bypass the address check.
- **Request shape.** `url` is an https URL of at most 2,048 bytes with no user name or password
  (the fragment is dropped); `method` is empty (GET), `GET`, `HEAD`, `POST`, `PUT`, `PATCH` or
  `DELETE`; at most 32 headers, names that are valid tokens (compared in lowercase, no
  repeats), values of at most 4,096 bytes without control characters; the headers the host controls
  (`Host`, `Content-Length`, `Transfer-Encoding`, `Connection`, `Upgrade`, `TE`, `Expect`,
  `Proxy-*`, `Via`, `Forwarded`, `X-Forwarded-*`) cannot be sent. A body is at most 1 MiB and only for
  POST, PUT, PATCH and DELETE. Anything else is `INVALID_ARGUMENT`, and nothing is fetched.
- **Bounds.** `max_response_bytes` 0 means 1 MiB and is capped at 8 MiB. A response over the budget
  is `RESOURCE_EXHAUSTED`: it is refused as soon as the `Content-Length` or the bytes read exceed
  the budget, and nothing is truncated. The budget counts the bytes on the wire: the host does not
  decompress, so a plugin that sends `Accept-Encoding` receives the compressed bytes. `timeout_ms` 0
  or above 30 seconds means 30 seconds, and one deadline covers the whole fetch (every redirect and
  the body); expiry is `DEADLINE_EXCEEDED`.
- **Reply.** `status_code` is the HTTP status of the final response (any status, including 4xx and
  5xx, is a reply, not an error). `headers` are lowercase, repeated headers joined with `, `, at
  most 64 and 32 KiB (the rest are dropped). `body` is the opaque response. `final_origin` is the
  origin of the final response, never a path, an address or the redirect chain.
- **Errors never say where.** `UNAVAILABLE` is "could not be reached" (DNS failure, connection
  failure or a certificate that is not accepted), with nothing about addresses or names;
  `FAILED_PRECONDITION` is too many redirects; `RESOURCE_EXHAUSTED` is also what a plugin gets
  when it is over its allowance of running fetches (4 per plugin, 16 for all plugins; extra
  fetches are not queued).
- **Not a sandbox.** Native plugins can still open their own sockets. This API controls what the
  *host* fetches for them.

### Network limits

The 8 MiB response, 30 s deadline, exact HTTPS origins and the address and redirect checks come
from the design. The rest were chosen by reasoning, not measured, and are open to change.

| Limit | Value | Why |
| --- | --- | --- |
| Response | 8 MiB cap, default 1 MiB | Design cap; the default keeps a careless call small. |
| Deadline | 30 s cap and default | Design cap. |
| Redirects | 5 | Browsers allow 20; legitimate API flows rarely need more than two, and each hop is another origin check and connection. |
| Request body | 1 MiB | Plugin messages are capped at 8 MiB; requests are small API calls. |
| URL, headers | 2,048 bytes; 32 headers of at most 4,096 bytes | Common server limits. |
| Response headers returned | 64, 32 KiB | Keeps a reply small. |
| Concurrent fetches | 4 per plugin, 16 in all | Bounds sockets and memory; over the allowance is `RESOURCE_EXHAUSTED`, not a queue. |

There is no per-plugin rate limit and no total-bytes quota, so a plugin may fetch repeatedly.

### What is not covered yet

- The source of approved origins belongs to plugin activation and the permission store. Until it
  is connected (`NetworkAccess::set_scopes`), no origin is approved and every fetch is denied.
- The client has been tested against local plain-HTTP servers with its address check relaxed, and
  against the real strict client for the destinations it must refuse. A fetch over real TLS to a
  public server has not been run in this environment.
- Proxy-only networks are not supported (system proxies are ignored on purpose).

## Notifications

`NotificationsPublish` (`notifications.publish`) lets a plugin ask Core to show a short notice. It
is plain text, attributed to the plugin, and it can do nothing but be read.

- **Attribution is the host's.** The notice is shown as "From plugin `<plugin id>`", where the id is
  the authenticated plugin of the session. The request has no field for a name, an icon or a
  sender, so a plugin cannot pose as Core or as another plugin.
- **Text.** `title` is required, one line, at most 120 bytes. `body` is optional, at most 512
  bytes and 8 lines. Both are trimmed. Control characters (other than the line breaks of a body)
  and characters that can disguise text are `INVALID_ARGUMENT`: bidirectional overrides and
  isolates, zero-width space, word joiner, byte-order mark, line and paragraph separators,
  interlinear annotation and tag characters. Joiners that real text needs (emoji sequences,
  several scripts) are allowed. `locale` is a language tag of letters, digits and hyphens (such
  as `en`, `pt-BR`) or empty.
- **Severity** is empty (info) or one of `info`, `success`, `warning`, `error`; anything else is
  `INVALID_ARGUMENT`.
- **Localization.** `translations` (at most 8) are other versions of the text, each with its own
  non-empty `locale`; a locale may appear once, the main text's included. Core shows the version
  for the user's language: the exact locale, then the same language (the region-less tag first),
  then the main `title`/`body`. Matching ignores case. A notice with no translation matching is shown
  in the plugin's main text.
- **No markup, links or actions.** The host never interprets the text and passes it on exactly. Core
  renders every field as plain text: `<script>`, HTML, Markdown and `javascript:` strings appear as
  the characters they are, a URL is not a link, and a notification has no buttons, icons or
  actions. The UI removes controls and disguising characters again before showing a notice, so
  its safety does not rest on the host's check alone.
- **Rate limit.** At most 5 notifications per plugin and 20 for all plugins together in any 60
  seconds. A call over the limit is `RESOURCE_EXHAUSTED`, with the wait in the message and in the
  `retry-after-ms` metadata; it counts nothing and is not remembered, so it may be retried with the
  same idempotency key after the wait.
- **Idempotency.** A notification needs an idempotency key. A retry of the same request returns
  the original outcome and shows nothing again; the same key for another request is
  `FAILED_PRECONDITION`. `expected_revision` must be 0. The grants are checked again right before
  a notice is counted and delivered.
- **Delivery is best effort.** A notice is an event, not stored state: it is shown as a toast
  (eight seconds) while a window listens, and is lost if none does. A host build with nothing
  connected answers `UNAVAILABLE`. Core keeps no history of plugin notifications.

### Notification limits

The text bounds and the rate limit are not in the design; they were chosen by reasoning, not
measured, and are open to change.

| Limit | Value | Why |
| --- | --- | --- |
| Title | 120 bytes, one line | A toast is a line of text, not a document. |
| Body | 512 bytes, 8 lines | Enough for a sentence or two without turning a toast into a page. |
| Translations | 8 | The languages GameNode and plugin authors are likely to cover. |
| Rate | 5 per plugin and 20 in all per 60 s | A plugin can report progress and results without being able to bury the window; the shared cap bounds a crowd of plugins. |

There is no per-user mute yet and no notification history.

## Jobs

A long operation (an install, a backup, a restore) answers with a job instead of blocking the call.
`JobsGet`, `JobsWatch` and `JobsCancel` read, follow and stop it. No operation returns a job in this
build yet except `GamesRequestLaunch` and `GamesRequestInstall` (backups, tools and ports come next);
the three calls and the registry behind them serve them all.

- **Ownership.** A job belongs to the authenticated plugin, its pinned publisher and the active
  account profile. The request names the job by its opaque id and nothing else: it has no field for
  an owner, a plugin or a profile. An id that is unknown, another plugin's, from an earlier
  profile, or already forgotten is the same `NOT_FOUND` with the same message, so an id says
  nothing about jobs the caller does not own. A plugin that restarts finds its own jobs again.
- **The permission.** The three calls have no permission of their own. Each one needs the
  permission of the operation that created the job, and a plugin that has lost it gets
  `PERMISSION_DENIED` (details name the permission) on `JobsGet`, `JobsWatch` and `JobsCancel`. A
  watch that was already open ends with that denial the next time it would deliver (or within five
  seconds when the job is quiet).
- **The commit.** The work behind a job checks again right before it commits an effect. If the
  permission was lost, the plugin's grants changed, the profile changed, the session that asked for
  the operation ended (the plugin stopped or restarted) or the job was cancelled in the meantime,
  nothing is committed and the job ends as `FAILED` (codes `permission_denied`,
  `failed_precondition`, `session_ended`) or `CANCELLED`. The authority to commit is the one the
  requesting session had; the job's record outlives that session, so the restarted plugin can read
  the outcome and retry under the same key.
- **Retries.** An operation that returns a job takes an idempotency key. A retry of the same
  request answers with the same job and starts nothing; the same key for a different request is
  `FAILED_PRECONDITION`. Keys are remembered for 24 hours but a finished job is kept for one hour
  (and at most 32 per plugin), so a retry that finds its job forgotten is `FAILED_PRECONDITION`
  ("no longer kept; use a new key"), never a second run.
- **States.** `QUEUED`, `RUNNING`, then one terminal state: `SUCCEEDED`, `FAILED` or `CANCELLED`,
  with a short `result_code` and a `message` (plain text, at most 64 and 512 bytes, no control
  characters; never a path, a credential or a stack trace). `revision` is the sequence number of
  the newest event.
- **Watching.** `JobsWatch` replays the events after `after_sequence` (0: from the start) in
  order and ends after the terminal one. Progress only moves forward (0 to 100), so a job has at
  most 103 events and a position is never "no longer retained"; a position past the newest event
  is `OUT_OF_RANGE`. A session has at most 8 open watches, library watches included.
- **Cancelling.** `JobsCancel` asks the work to stop and waits up to two seconds for it to. A
  finished job answers with how it finished, unchanged, so cancelling twice, or too late, is not an
  error. Work that cannot stop halfway keeps running and the reply says `RUNNING`: the work decides,
  because stopping it in the middle could leave an effect half done, and what it finally did is
  what the job reports (a cancellation does not rewrite a `SUCCEEDED`). A job cancelled before it
  started never runs. The call needs an idempotency key like every mutation; cancelling is
  idempotent by itself, so the key only has to be well formed.
- **A profile switch.** The jobs of the old profile are told to stop and are no longer visible to
  anyone; the new profile starts with none, and a request still carrying the old profile's context
  cannot start one.

### Job limits

Not in the design; chosen by reasoning, not measured, and open to change.

| Limit | Value | Why |
| --- | --- | --- |
| Running jobs | 8 per plugin, 32 in all (`RESOURCE_EXHAUSTED`; a retry of a running job is still answered) | A plugin can run a few operations at once without taking every worker; the shared cap bounds a crowd of plugins. |
| Finished jobs kept | 32 per plugin, one hour | Long enough to read an outcome after a restart; short enough to stay small. |
| Idempotency keys | 1,024 per plugin, 24 hours | The same bound as the other mutations. |
| Result code / message | 64 / 512 bytes | They are for a person to read, not a document. |
| Cancel wait | 2 seconds | The call stays short; a slow job is reported as still running. |

Jobs live in the host's memory. A host restart forgets them and their keys, and the work does not
resume; an operation that must not repeat across a restart has to be idempotent in its own domain
(an install that is already installed is not installed twice).

## Save profiles

The `Saves` service (`ListProfiles`, `GetProfile`, `CreateProfile`, `UpdateProfile`, `RemoveProfile`;
`game.saves.read`, `.create`, `.update` and `.delete`, independent of each other) lets a plugin say
where one runtime keeps a game's saves. A profile is **configuration only**: it grants no access and
reads nothing.

- **What a profile is.** A name, one to eight folders, an optional `runtime_id`, `format_id` and
  `format_version`, and bounded descriptive metadata. Several profiles may exist for one game entry,
  for example one per emulator, each with its own folders. The name is unique per game entry, without
  regard to case. A profile is for one of the plugin's own library entries; the game it was matched to
  when it was made is stored with it and a later catalog rematch does not change it.
- **Folders the user already approved.** Each folder is a handle the plugin got for a folder the
  user approved for the same plugin and the same game entry ([Files](#files)), or for a directory below
  it. Anything else is `NOT_FOUND` (forged, edited, another session's, revoked) or `PERMISSION_DENIED`
  (approved for another entry). The rules can only narrow inside the folder, so a profile never
  reaches further than the approvals do. A new folder needs a new approval; revoking an approval
  leaves the profile and its rules but gives the folder no handle in replies (an empty
  `approved_resource_handle`) until it is approved again.
- **Naming rules.** At most 16 per folder, each at most 256 bytes: a relative path of `/`-separated
  names with `*` and `?` inside a name and `**` as a whole name for any depth. A leading `!` makes it an
  exclusion. No absolute path, `..`, `.`, empty name, drive, backslash, brackets or braces.
- **Ids and limits.** `runtime_id` and `format_id` hold letters, digits, `.`, `_` and `-`, at most 128
  bytes. Names and folder names are one line of plain text, at most 120 bytes. Descriptive metadata is
  a `Struct` of at most 8 KiB, 4 levels and 64 keys and items, with no control characters or non-finite
  numbers (the structural rules of task 12.6 will replace these bounds). At most 16 profiles per game
  entry and 256 per plugin (`RESOURCE_EXHAUSTED`).
- **Revisions.** The profile id is the host's (`profile-…`); `profile_id`, `creator_plugin_id` and
  `revision` in a create request must be empty. Creating takes `expected_revision` 0; updating and
  removing take the profile's current revision. A stale revision is an outcome
  (`REVISION_CONFLICT` with the current revision), not an error, and changes nothing. All revisions of
  a plugin come from one counter, so a removed and recreated profile never repeats one. Idempotency
  keys work as for every mutation.
- **Updates** name the fields they change in `field_mask`: `name`, `directories`, `runtime_id`,
  `format_id`, `format_version`, `descriptive_metadata`. The id, the entry, the game and the creator
  cannot change; a profile that moved would be a different profile.
- **Removing** a profile deletes the configuration only. It never touches a file in the folders and
  never deletes backup archives.
- **Where profiles live.** In the plugin's own store for the active account profile (task 13.1,
  decision S of the pending-decisions document): they survive a plugin restart and signing out and in
  to the same account, a different account sees none of them, and a profile never exists without the
  approvals it rests on.
- **Another plugin's profile** is reached only through a provisional grant the host holds for that
  profile and operation (read, update, delete): without it the profile is `NOT_FOUND`. A reader sees
  the creator in `creator_plugin_id` and no folder handles; an updater may change the name, runtime,
  format and metadata but never the folders, which are the creator's own approved folders; a deleter
  removes the creator's profile. The grant source is a port the application connects
  (`SaveAccess::set_grants`); **nothing is connected in the real app yet**, so today no plugin reaches
  another's profiles. It is provisional until the shared data plane (section 12) and the permissions
  screen define sharing. List and get return another plugin's profile only when its id is named in
  `shared_records.selected_record_ids`; common-data `categories` are `UNIMPLEMENTED`.
- **Not covered.** Backing up and restoring from a profile is not served (`BackupsRequestCreate` and
  `BackupsRequestRestore` stay `UNIMPLEMENTED`), and nothing parses, converts or loads a save. A
  profile says where saves are; it does not make them readable: reading needs `files.read` and the
  approval.

## Inspecting approved places

`Locations.GetSize`, `GetAccess` and `GetLastModified` describe an approved place from its **metadata
alone**. They open no file for its content and change nothing to find out: no permission, no
timestamp, no file. `Inspect`, `GetFingerprint`, `Compare` and `WatchAvailability` are not served yet
(`UNIMPLEMENTED`).

- **What a place is.** A handle the plugin already holds for a folder the user approved ([Files](#files)),
  or for a file or directory below it; or the id of one of the plugin's own [save profiles](#save-profiles),
  which stands for the union of its folders narrowed by their naming rules. A forged, edited, revoked
  or another session's handle, and another plugin's profile id, are `NOT_FOUND`.
- **Permissions.** `game.storage.inspect` for a folder approved as `installation`, `game.saves.inspect`
  for one approved as `save` or `prefix` and for a profile. Neither needs `files.read`, and neither
  gives it: a plugin that lost `files.read` can still ask how big a folder is and still cannot list
  or read it. The permission is checked again after the walk, which may take seconds.
- **`GetSize`** reports logical bytes (the sum of the files' sizes, not allocated disk space), the
  number of selected files and the number of folders visited. For a profile, only the files its
  rules select count, and a file reachable through two of its folders (a folder and one inside it)
  counts once. A rule matches the whole path from the profile's folder, so `*.sav` is the folder's own
  files and `**/*.sav` those at any depth. Links, devices and mount points do not exist for a plugin
  and are not counted.
- **Bounded and honest.** A walk visits at most `max_entries` entries (default 10,000, at most
  100,000) and runs for at most 5 seconds, and it stops when the call is abandoned. If it was cut,
  or a folder inside could not be read, or a folder of a profile has lost its approval, the answer says
  `complete = false`. A place that cannot be inspected at all is reported through `availability`
  (`MISSING` for a folder that is gone, moved or replaced, `INACCESSIBLE` when the system will not let
  GameNode in) with zeros and `complete = false`, never as an empty folder. `UNMOUNTED` and
  `DISCONNECTED` come with the volume metadata of task 13.5; until then a drive that is away looks
  `MISSING`. Nothing is created in place of a missing folder.
- **`GetLastModified`** lists, first, the place's own folder time under the handle that was asked
  about, then up to `max_entries` entries below it (default 100, at most 1,000), each with a handle
  for the next call and its modification time. `aggregate_latest_at` is the newest time among the
  selected entries *below* the folder, apart from the folder's own, which changes when files are
  added or removed. `precision` is `subsecond` when the system reported a fractional part and
  `whole_second` when it did not; it describes what was observed, not a guarantee about the
  filesystem. An entry the system gave no time for is left out, never reported as 1970.
- **`GetAccess`** answers, per entry (the place itself when no `entry_handles` are named; up to 100
  entries of the same approval; a profile lists its folders), two separate things.
  `read_only_state` is metadata (`read_only` or `writable`: the read-only attribute on Windows, no write
  bit on Linux). `effective_access` is what the current operating-system user may do now:
  `read=… write=… create=… delete=… method=…`, each right `allowed`, `denied` or `unknown`, and the
  method naming how it was found out (`kernel_access_check` on Linux, `open_probe` on Windows, `none`
  when the place could not be reached). On Linux it comes from the
  kernel's own access check, which weighs the mode, owner, group, ACLs and a read-only mount; delete
  depends on the entry's folder, and `create` and `delete` are `unknown` where they do not apply or
  cannot be told (a file's `create`, the approved root's `delete`, a sticky folder). On Windows each
  right is probed by opening the entry with that access and closing it at once, nothing written;
  a sharing violation or any other error is `unknown`. **The Windows probe has only been
  type-checked, never run on Windows.**
- **A profile's lost folder** (its approval revoked or changed) is reported as `MISSING` in
  `GetAccess`, as a partial total in `GetSize`, and is left out of `GetLastModified`.
- **Not covered.** Hashes and comparisons (13.4), volume identity, local-versus-network and media
  hints, and availability events (13.5), and runtime resources (there are none yet).

## Launching and installing

`GamesRequestLaunch` (`game.launch`) starts a game's detected target unchanged, and
`GamesRequestInstall` (`games.install`) opens the provider's own install flow (for a store game, the
store's install page, where the store asks the user to confirm). Neither starts anything without the
**user's intent**, whatever the plugin's grants. The answer to both is a job (see [Jobs](#jobs)).

- **No parameters.** The request names the game (`installation_id`, optionally `game_id`) and, for a
  launch, the `expected_revision` of the library entry the plugin read (a different revision is
  `FAILED_PRECONDITION`: read it again). It has no field for an executable, a URI, arguments, a
  working directory or an environment; what is started is decided by the host. A plugin that wants
  parameters needs `game.custom_launch` and an approved recipe; without it, a custom launch is
  denied explicitly and never turned into a standard launch by the host. A plugin may, by its own
  choice, then make a standard launch request without parameters. A message in the custom-launch
  shape sent to `GamesRequestLaunch` has its extra fields dropped by protobuf and nothing applies
  them; the protocol does not detect that, so use the SDK's parameterless call.
- **Scope per game.** A provider session names only its own library entries; an extension session
  names only the game the user selected for it. `library.read.all` does not widen this. Anything
  else is `NOT_FOUND`, the same as an unknown id.
- **The user's intent** is host state, never a claim the plugin makes. It comes in two forms.
  - *On request.* Without a `user_intent_id`, the job stays `QUEUED` with the message
    `awaiting confirmation` while Core asks the user: which plugin (with its publisher), which game
    and what will happen. Approving runs the job; declining ends it `FAILED` with `declined`; no
    answer within 5 minutes ends it `FAILED` with `timed_out`; `JobsCancel` withdraws the request.
    One confirmation per game and operation may wait at once (`FAILED_PRECONDITION` for another),
    and at most 4 per plugin (`RESOURCE_EXHAUSTED`). A build with no dialog answers
    `FAILED_PRECONDITION` at once and starts no job.
  - *Pre-issued.* The host creates an opaque, unguessable id from an explicit user action in Core and
    gives it to the plugin it concerns. It is bound to the plugin and publisher, the profile, the game
    and installation, the operation and (for a launch) the game's detected target; it is single
    use and valid for 60 seconds. An id that is missing, expired, spent, forged or for anything
    else is refused with the same `FAILED_PRECONDITION`, without saying which; a refusal spends
    nothing. A retry with the same idempotency key does not spend a second intent. Delivery of the
    id to a plugin arrives with launch-extension sessions; the host side exists.
- **Remembering.** The dialog offers "don't ask again for this plugin and this game", off by
  default and only with an approval. It is remembered per plugin, profile, game and operation, and
  for a launch it is bound to the game's detected target (the launch URI, or the approved
  executable, arguments and working directory): when the target changes, the user is asked again. It
  stays in the plugin's own store, is listed under the plugin in the settings and can be revoked
  there; the next request then asks again. The call still needs the current permission.
- **Checked again at the start.** After the user answers (it may take minutes) the host checks the
  permission and the grants, that the game is still the same and, for a launch, that its target is
  the one approved, and only then starts it. A permission lost, a changed target or a cancellation
  in the meantime start nothing (`FAILED` with `permission_denied`, `failed_precondition` or
  `target_changed`, or `CANCELLED`).
- **Outcomes.** `SUCCEEDED` with `opened` means the link was handed to the operating system, not
  that the game finished loading or installing. `FAILED` with `unsupported` (the provider has no
  such action, for example installing a manually added game), `unavailable` (the store application
  is missing, the game is already running or being opened, or data is missing) or `failed`. The
  starting uses the same path as the user's own Play button, so the launch status, the "already
  running" guard and playtime detection behave as before.

What this does not do: the dialog is the user's consent for the request, not a certificate for the
programs that run. GameNode checked the scope; the programs may do anything the user can. The dialog
shows the plugin's id, its publisher and the game's name as plain text; the game's name comes from
the provider and is not trusted.

### Launch and install limits

Chosen by reasoning, not measured, and open to change.

| Limit | Value | Why |
| --- | --- | --- |
| Pre-issued intent | 60 seconds, single use, 256 held at once | Long enough to be handed over and used, too short to keep. |
| Dialog wait | 5 minutes | The same as folder approvals. |
| Waiting confirmations | 1 per game and operation, 4 per plugin | The user is never asked twice for the same thing, nor buried by one plugin. |

## Utility services are not a sandbox

Storage, Files, Network and Notifications decide what a plugin may ask GameNode to do. They do not
confine the plugin's own process: it runs with the user's operating-system permissions and can open
files and connect to hosts directly, outside this API and without these limits. Consequences for
authors and for what the interface tells the user:

- An approval (a folder, an origin, durable storage) states what GameNode will do for the plugin. It
  is not evidence that the plugin cannot do more by itself, and the interface must not suggest it.
- The limits of this section (quotas, byte budgets, address checks, rate limits) apply to calls made
  through the API. A plugin that bypasses it is not limited by them, and a user who does not trust
  a plugin should not install it; OS-level isolation is outside what the host provides.
- What the API guarantees regardless of the plugin's behavior is the other direction: it never gives
  a plugin an absolute path, a Core credential, another plugin's data, a host data folder or an
  address on the user's network.

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
