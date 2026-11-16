# Developing a GameNode provider plugin

## Public transport and session lifecycle

Plugins connect to the local GameNode process that started them. The endpoint is an OS-local
Unix socket on Linux or a private named pipe on Windows; it is not a TCP listener. On Linux the
socket is placed in a current-user-only directory and has mode `0600`; the host also checks the
peer UID. Windows uses GameNode's private named-pipe listener. The host limits the server to 16
accepted connections, eight concurrent requests per connection and eight concurrent streams.

The supervisor supplies `GAMENODE_PLUGIN_ENDPOINT` (or the legacy
`GAMENODE_PLUGIN_SOCKET`), `GAMENODE_PLUGIN_SESSION_CREDENTIAL`, and
`GAMENODE_PLUGIN_OPERATION_ID`. `Launch::from_env()` parses these values. The credential is a
32-byte session secret; it authenticates `OpenSession`, the provider stream and service calls.
Treat it as a password: do not print it, include it in diagnostics, persist it or send it to a
remote service. `Launch` intentionally has no `Debug` or serialization implementation. The
operation identifier is host-supplied launch context, not a substitute for host-issued operation
IDs in later commands.

`OpenSession` authenticates the package identity and negotiates the protocol before the plugin
opens `ProviderHost.Exchange`. API 1.1–1.5 clients keep using `api_major` and `api_minor`; an
empty API range in the additive fields means the legacy request, where `api_minor` is the newest
minor the client supports and the host may select any supported minor from the API minimum through
that value. API 1.6 clients may
declare inclusive minimum/maximum major and minor versions plus required or optional feature IDs.
The host selects the highest overlapping version. A required unavailable feature rejects the
session; optional unavailable features are returned as unsupported. The reply includes granted
permissions, selected version, feature statuses and a support window. Unknown or duplicate
permission IDs are rejected, and a requested permission is not a grant unless host policy
authorized it.

The protocol distinguishes `provider` and `extension` sessions. An extension session carries one
host-validated `TargetGameContext` (game, installation, profile and revision) and its
`ServiceContext` repeats that scope on each call. The Rust SDK exposes `Session::open_extension`
and `run_extension` for that lifecycle. The desktop currently mounts the provider and control
services described below; the API 1.6 `ResourceHost` is a contract in the `.proto` and is not yet
mounted by the desktop server. Do not infer that an RPC is available just because generated
client code exists. Third-party package metadata and validation are documented in
[`plugin-package-manifest.md`](plugin-package-manifest.md).

On `Exchange`, the plugin sends `ProviderEvent.Ready` first. It then receives host-issued
`ProviderCommand`s and sends correlated events using the `ServiceContext` included with each
command. A command context binds session credential, provider or target scope, operation ID,
account epoch and profile epoch. Preserve it exactly in the response. Do not invent operation IDs,
reuse an old context, or keep a `HostClient` from a completed command to authorize future work.
The SDK's `HostClient::for_command` enforces this binding.

The existing provider operation families are:

| Command family | `ProviderOperation` values |
| --- | --- |
| Lifecycle and discovery | `Initialize`, `Discover`, `Shutdown`, `Acknowledge` |
| Account and login | `SignIn`, `FocusSignIn`, `CancelSignIn`, `Check`, `SignOut`, `Cancel` |
| Catalog and playtime | `SyncCatalog`, `ReadCounter` |
| Declarative settings (API 1.3, `ui.settings`) | `SettingsDescribeRead`, `SettingsSubmit`, `SettingsCancel` |
| External configuration (API 1.6, `ui.configure`) | `ConfigurationOpen`, `ConfigurationFocus`, `ConfigurationClose` |

Existing `ProviderEvent` families are `Ready`, `AccountStatus`, `CatalogChunk` and `Result`.
`OperationResult` carries the typed discovery, status, catalog completion, counter, login,
settings or configuration result, plus failure, focused-state, account-change, sign-out and
observation-time fields where relevant. Catalogs are chunked at 256 games and validated as a
complete category before reconciliation. A completed empty result is different from a failed,
cancelled or superseded operation; never report a failure as an empty successful snapshot. The old unary
`PluginHost.SubmitDiscovery` remains for discovery-only API 1.1 clients; full providers use the
exchange stream.

Control operations such as login cancellation/focus, settings cancellation and configuration
focus/close are dispatched separately from bounded work so they can progress while another
operation awaits a network or user response. Honor cancellation promptly and stop side effects
that have not committed. The host fences later work with operation, account, profile and grant
revisions. The SDK bounds concurrent work and control dispatch; it calls `shutdown` after the host
sends `Shutdown` or the stream ends, with a three-second shutdown wait.

A third-party plugin runs only while a GameNode account is signed in, and its grants, activation
and private data belong to that account. Treat `UNAVAILABLE` from a profile-scoped service as "no
account session", not as a denial. When the account signs out or changes, the host fences the
profile and stops the process; do not cache data from one account for use after a restart. See
[account-bound plugin profiles](plugin-api-permissions.md#account-bound-plugin-profiles).

## RPC inventory and API 1.6 status

The mounted `PluginHost` service has `OpenSession`, the legacy `SubmitDiscovery`, and API 1.6
control RPCs `DescribeApi`, `GetPermissions` and `WatchPermissionChanges`. The three control
methods authenticate the session and do not require a library/data permission. They return public
API metadata, the caller's own grants, and changes to those grants. Permission denial details use
safe structured fields (reason, permission ID and grant revision); clients should branch on gRPC
status and these fields, never on raw status text.

The mounted `ProviderHost` service has `Exchange`, `GetContext`, `Account`, `Secret`, `Login` and
`DeclareActions`. These are the existing provider service methods. Discovery, catalog, counters,
status and lifecycle results travel through `Exchange`; they are not extra unary RPCs.

The API 1.6 `ResourceHost` and domain-service contracts in `crates/plugin-protocol/proto/plugin.proto`
define typed request and response messages. The checked
[`resource_host.json`](../crates/plugin-protocol/contracts/resource_host.json) and
[`domain_host.json`](../crates/plugin-protocol/contracts/domain_host.json) catalogs record each
method, message, stream, permission, owner, revision, pagination, quota and idempotency rule;
contract tests compare every route to the generated protobuf descriptor. The permission grouping
is in [`plugin-api-permissions.md`](plugin-api-permissions.md). Unknown routes are `UNIMPLEMENTED`.

The desktop mounts `ResourceHost`, but serves only `LibraryList`, `LibraryGet`, `LibraryWatch`,
`MetadataUpdate`, `FavoritesList`, `FavoritesSet`, `MatchingSearch`, `MatchingAssign`,
`ArtworkRead`, `ArtworkSet`, `StorageGet`, `StoragePut`, `StorageDelete`, the five `Files*`
methods, `NetworkFetch`, `NotificationsPublish`, `GamesRequestLaunch`, `GamesRequestInstall`, `JobsGet`,
`JobsWatch`, `JobsCancel`, the three `Backups*` methods, the three `Tools*` methods, the three `Ports*` methods and the nine `Libraries*` methods (grants,
scope, cursors and watch rules are in
[`plugin-api-permissions.md`](plugin-api-permissions.md#library-reads); the writable display name,
revisions, favorites and idempotency keys are in
[`plugin-api-permissions.md`](plugin-api-permissions.md#display-metadata-and-favorites); catalog
search bounds and assignment results are in
[`plugin-api-permissions.md`](plugin-api-permissions.md#matching); the cover override, image limits
and read budget are in
[`plugin-api-permissions.md`](plugin-api-permissions.md#artwork); jobs are in
[`plugin-api-permissions.md`](plugin-api-permissions.md#jobs); `ToolsList`, `ToolsRequestApply`
and `ToolsRequestRemove` use [Core tool review](plugin-api-permissions.md#tools-from-the-host-catalog); logical-library roots and
their standing monitoring consent are in
[`plugin-api-permissions.md`](plugin-api-permissions.md#logical-libraries-and-approved-roots)).
The `Saves` domain service is also served (`ListProfiles`, `GetProfile`, `CreateProfile`,
`UpdateProfile`, `RemoveProfile`; see
[`plugin-api-permissions.md`](plugin-api-permissions.md#save-profiles)) and `Locations.GetSize`,
`GetAccess`, `GetLastModified`, `GetFingerprint`, `Compare`, `Inspect` and `WatchAvailability` (see
[`plugin-api-permissions.md`](plugin-api-permissions.md#inspecting-approved-places)); the Rust SDK
exposes the raw `HostClient::saves_client()` and `HostClient::locations_client()`. The four
`Data*` methods are also served; see the [shared-data producer and consumer guide](plugin-shared-data.md)
for their permissions, projections and current source limitations. Other `ResourceHost` methods
and domain services answer `UNIMPLEMENTED` unless listed as served above. Call `DescribeApi` for
the methods this build serves instead of inferring availability from the protobuf contract.

Resource calls are plugin-initiated and use the `resource_context` that `OpenSession` returns to API
1.6 sessions (`HostClient::resource_context()` in the Rust SDK). Put it in the `context` of each
request, for example `LibraryListRequest.page.context`; the launch operation context is not
accepted. The context grants nothing by itself.

Public-data reads use a typed per-query `CommonDataSelector` on each query that supports shared
projections. Its categories and selected record ids are checked independently; a selected-record
grant does not require category-wide access. Rust plugins can construct one with
`gamenode_plugin_sdk::common_data_selector([CommonDataCategory::GamePerformanceHistory])` and
set it on a served data request, such as `DataListRequest.page` or
`DataGetRequest.common_data_categories`; selected-record clients populate
`selected_record_ids`. Omitting or emptying the selector keeps the query private to the caller's
visible records. Selectors request data only; the host still checks each grant and scope. Secrets
remain excluded, and sensitive/custom datasets require separate sharing review.

For the served `DataPublish`, `DataGet`, `DataList` and `DataWatch` contract, including
session retention, field projection, snapshot-to-watch resumption and safe exclusions, see
[Shared plugin data](plugin-shared-data.md).

The ResourceHost method names, grouped for source/API review, are:

| Area | API 1.6 contract method names |
| --- | --- |
| Library and presentation | `LibraryList`, `LibraryGet`, `LibraryWatch`, `MetadataUpdate`, `FavoritesList`, `FavoritesSet`, `MatchingSearch`, `MatchingAssign`, `ArtworkRead`, `ArtworkSet` |
| Game execution and install | `GamesRequestLaunch`, `GamesProposeCustomLaunch`, `GamesRequestCustomLaunch`, `GamesRemoveCustomLaunch`, `GamesRequestInstall` |
| Storage, files and network | `StorageGet`, `StoragePut`, `StorageDelete`, `FilesRequestAccess`, `FilesList`, `FilesRead`, `FilesWrite`, `FilesDelete`, `NetworkFetch`, `NotificationsPublish` |
| Backups and libraries | `BackupsList`, `BackupsRequestCreate`, `BackupsRequestRestore`, `LibrariesList`, `LibrariesGet`, `LibrariesWatch`, `LibrariesCreate`, `LibrariesUpdate`, `LibrariesRemove`, `LibrariesAddRoot`, `LibrariesUpdateRoot`, `LibrariesRemoveRoot` |
| Runtimes and observations | `RuntimesList`, `RuntimesGet`, `RuntimesWatch`, `RuntimesRegister`, `RuntimesUpdate`, `RuntimesRemove`, `GamesRegisterContent`, `StorageDiagnosticsPublishBenchmark`, `StorageDiagnosticsPublishSmart`, `ComputerPublishHardware`, `GamePresencePublish`, `GamePresenceClear`, `GamePerformancePublishSamples`, `GamePerformanceEndSeries` |
| Data, persistence, tools, ports and jobs | `DataPublish`, `DataGet`, `DataList`, `DataWatch`, `PersistenceGetUsage`, `PersistenceListEntries`, `PersistenceDeleteEntries`, `PersistenceClear`, `PersistenceSetRetentionPolicy`, `ToolsList`, `ToolsRequestApply`, `ToolsRequestRemove`, `PortsList`, `PortsDetail`, `PortsRequestInstall`, `JobsGet`, `JobsWatch`, `JobsCancel` |

Each name above maps to exactly one typed request/response pair in the protobuf service and
catalog. `GamesRequestLaunch` takes `GameLaunchRequest` and has no target/argument override;
`GamesProposeCustomLaunch` and `GamesRequestCustomLaunch` take `CustomLaunchRequest` and require
the independent `game.custom_launch` grant. Their approvals and effects are not interchangeable.

## Library resource helpers and recipes (Rust SDK)

`HostClient::resources()` returns `Resources`, typed calls for the library resources served today,
or `None` for a session that negotiated a minor before API 1.6 (like `resource_context()`). Each
call sends exactly what the raw `resource_client()` would send with the session's resource context
and returns the host's reply, so the two are interchangeable; the desktop's service tests make
every call both ways and compare the answers (a write is compared by replaying it raw under the
SDK's idempotency key, which the host only honors for an identical request). The helpers add no
authority: grants, scope and revisions are still checked by the host. Errors are the host's own
`Status` (`gamenode_plugin_sdk::Status`, `Code`); the codes are in
[`plugin-api-permissions.md`](plugin-api-permissions.md). Go and other clients use the generated
client directly with the same rules.

| `Resources` method | Raw call | Needs |
| --- | --- | --- |
| `library_page`, `library_entries`, `library_snapshot` | `LibraryList` | `library.read` |
| `library_get` | `LibraryGet` | `library.read` |
| `library_watch` | `LibraryWatch` | `library.read` |
| `set_display_name` | `MetadataUpdate` | `library.metadata.write` |
| `favorite_games`, `set_favorite` | `FavoritesList`, `FavoritesSet` | `favorites.read`, `favorites.write` |
| `search_catalog`, `assign_match` | `MatchingSearch`, `MatchingAssign` | `matching.read`, `matching.write` |
| `read_cover`, `set_cover`, `clear_cover` | `ArtworkRead`, `ArtworkSet` | `artwork.read`, `artwork.write` |

`library_entries` and `favorite_games` walk every page; if the library changes during the walk
the list restarts (up to three times), so the result is one consistent snapshot. `library_snapshot`
also returns the revision to resume `library_watch` from.

Writing safely comes down to three rules:

1. **One key per intended change, reused for its retries.** Make it with `idempotency_key()`. After a
   timeout or lost connection, send the same change again with the same key: the host returns
   the original outcome instead of applying it twice. A different change gets a different key.
2. **Write with the revision you read.** Entries carry `metadata_revision` (display name),
   `artwork_revision` (cover) and `revision` (everything, used for matching). A conflict is an
   answer (`Outcome::RevisionConflict`, with the current revision), not an error: read the entry
   again and decide whether the change still applies. Conflicts are not remembered, so the key
   may be reused with the new revision.
3. **Treat refusals as answers about the host's rules.** `NOT_FOUND` is a foreign or unknown
   entry, `FAILED_PRECONDITION` a match the user chose, `INVALID_ARGUMENT` a refused value or
   image (the message says why), `RESOURCE_EXHAUSTED` a rate or byte budget, `UNAVAILABLE` a
   missing session or catalog. Only `UNAVAILABLE` and `DEADLINE_EXCEEDED` are worth resending.

The recipes in
[`crates/plugin-sdk/examples/library_recipes.rs`](../crates/plugin-sdk/examples/library_recipes.rs)
put the rules into code you can copy. They are compiled with the SDK and run against the fixture
host in the desktop's service tests:

| Recipe | What it does |
| --- | --- |
| `send` | Sends one change, resending it with the same key only after a lost connection or timeout. |
| `rename` | Gives an own entry a display name (or restores the detected one), rereading after a conflict. |
| `match_by_title` | Searches the catalog, assigns the entry to the exact title, and leaves a choice the user made alone. |
| `use_cover`, `current_cover` | Sets, clears or reads an entry's cover; a refused image is not retried. |
| (`Resources::set_cover_from_file`) | Sets a cover from a file you may read, by handle, so you never hold the bytes. Needs `files.read` besides `artwork.write`, and a handle of an approval of the same entry (not another game's folder). |
| `star_matched_games` | Stars every game one of the plugin's entries is matched to, once each. |
| `follow_library` | Lists the library, then follows the changes after it, listing again when the host no longer retains the position. |

## Private key/value storage

`StorageGet`, `StoragePut` and `StorageDelete` keep a plugin's own small non-secret data (settings,
caches, bookkeeping) in a namespace the host derives from the plugin's authenticated identity and the
active account profile. The request names only a key; see "Private storage" in
`docs/plugin-api-permissions.md` for the grants, limits and error codes. The practical rules:

- Declare `storage.read`, `storage.write` and `storage.delete` for what you use, and `storage.persist`
  only if the data must outlive the session. The user approves it separately; without it a
  `PERSISTENT` write is `PERMISSION_DENIED`, so handle that by asking for `SESSION_ONLY` explicitly
  rather than expecting a fallback.
- Read the key first: its `revision` (0 when absent) is the `expected_revision` of your next write,
  and a `MUTATION_OUTCOME_REVISION_CONFLICT` reply means someone else changed it, so read again and
  merge. Use one `idempotency_key` per intended change and reuse it only to retry that same change.
- Keep values within 1 MiB and the namespace within 16 MiB per layer; `RESOURCE_EXHAUSTED` changes
  nothing, so free space (delete or let values expire with `ttl_seconds`) and retry.
- Session-only values vanish when your last session ends, durable ones do not: after a restart read
  your state back from storage instead of assuming it is empty or intact.
- Secrets never belong here. Use the provider `Secret` service for tokens and credentials.

The Rust SDK has typed helpers for storage (`storage_get`, `storage_put`, `storage_delete`) and
recipes for them; see "Utility helpers and recipes (Rust SDK)" below. Other clients use the generated
`ResourceHost` client, as the Go example does.

## File access

A plugin never names a path. It asks `FilesRequestAccess` for an installation of one of its own
library entries, says what it needs (`read`, `write`, `delete`) and which folder kind (`installation`,
`save`, `prefix`), and the user chooses and confirms the folder. The reply holds opaque handles; see
"Files" in `docs/plugin-api-permissions.md` for the rules, limits and errors. The practical points:

- Declare `files.read`, `files.write` and `files.delete` only for what you use. They are separate,
  off at installation, and the user approves a folder per game; an approval for `read` does not allow
  `write`, and neither allows `delete`.
- Tell the user where the folder usually is. For `save` and `prefix` you can declare up to 8
  suggestions per kind, most likely first (`declared_locations`; `declared_location` and
  `Resources::files_request_access_with` in the Rust SDK; recipe `open_save_folder`). Use the
  placeholders `{home}`, `{documents}`, `{appdata}`, `{localappdata}` (Windows) and `{xdg_data}`,
  `{xdg_config}` (Linux) or an absolute path, with `/` between names. List every platform in one
  list: what cannot apply on this system is ignored without an error. A suggestion is only shown
  to the user as yours and unverified; it grants nothing, you are not told which exist or which was
  taken, and a malformed one (relative path, `..`, unknown placeholder, a long or multi-line
  description) is `INVALID_ARGUMENT`. Check what is really in the folder through the handle,
  asking for `read` first.
- Handles last for the session. After a restart call `FilesRequestAccess` again: the user is not
  asked again unless you want more operations or the games sharing the folder changed. Treat
  `FAILED_PRECONDITION` ("request access again") and `NOT_FOUND` on a handle as the cue to do that.
- List a directory to get child handles, page with the returned cursor, and read big files in
  chunks (`next_offset`/`eof`, at most 4 MiB per call). Create a file by writing with a directory
  handle and a `name`; the reply's `resource_id` is its handle.
- Use one idempotency key per intended write or delete and reuse it only to retry that same one.
- Do not expect links to be followed, directories to be deleted, or anything inside GameNode's own
  data folder to exist: those are never reachable, and a refused path says so without naming one.
- Your API access is what the user approved; it is not a promise that your own process cannot use
  the operating system directly.

Until the settings UI connects the approval dialog, a new approval is not available; handles for
folders already approved work.

## Network fetch

`NetworkFetch` asks the host to fetch an https URL for you, from an origin the user approved. See
"Network fetch" in `docs/plugin-api-permissions.md` for the rules, limits and errors. The practical
points:

- Declare `network.fetch` and name the exact origins you need (scheme, host and port; no
  wildcards or paths). The user approves them; an origin you did not declare, or the user did not
  approve, is `PERMISSION_DENIED` and nothing is sent.
- The host never connects to loopback, private, link-local or similar addresses, even for an
  approved origin, and it follows redirects only to approved origins. A plugin that must talk to a
  local service does so over its own native IPC, not through this API.
- Treat `PERMISSION_DENIED` after a redirect as "the server sent me somewhere I may not go": the
  error does not say where. Ask the user to approve the extra origin you need and declare it.
- Set `max_response_bytes` and `timeout_ms` for what you expect; a response over the budget is
  `RESOURCE_EXHAUSTED` and nothing is truncated. Response bytes are not decompressed.
- Send your own `Authorization` if your service needs one; the host adds none and never sends any
  GameNode credential.
- At most 4 fetches per plugin run at once; retry later on `RESOURCE_EXHAUSTED`.

## Notifications

`NotificationsPublish` asks Core to show a short plain-text notice from your plugin. See
"Notifications" in `docs/plugin-api-permissions.md` for the rules and limits. The practical points:

- Declare `notifications.publish`. Core shows the notice as "From plugin `<your plugin id>`"; you
  cannot choose the sender.
- Write plain text: no HTML, Markdown or links (they are shown as typed and never interpreted), no
  control characters. Keep the title to one short line and the body to a few.
- Give `translations` for the languages you support. Core picks the user's language, then the
  same language, then your main text, so the main text should be a language everyone can read.
- Notify about what the user needs to know, not about progress ticks: you may send 5 a minute. On
  `RESOURCE_EXHAUSTED` wait the `retry-after-ms` and drop or merge what you wanted to say.
- Notifications are not delivered if no window is open and are not kept; do not use them as the only
  record of something important.

## Jobs

Operations that take a while (installs, backups, restores) answer with a job. Rules and limits are
under "Jobs" in `docs/plugin-api-permissions.md`; what to do about them:

- Keep the `job_id` the operation returned. It is opaque and yours alone: no other plugin can read
  or cancel it, and you do not need to (and cannot) say whose it is.
- Give the operation an idempotency key you can reuse. If your plugin crashes or the call times
  out, retrying with the same key returns the same job instead of starting a second one. If the
  job was forgotten (an hour after it finished) the retry is `FAILED_PRECONDITION`: look at what
  the operation changed before starting it again with a new key.
- Follow a job with `JobsWatch` (events in order; resume with the last `sequence` you saw) or poll
  `JobsGet`. A watch ends after the terminal state. `NOT_FOUND` means it is not yours or it is gone;
  treat both the same.
- Branch on the terminal state and `result_code`, not on `message`, which is for people. A job
  that ends `FAILED` with `permission_denied`, `failed_precondition` or `session_ended` committed
  nothing: the permission or the profile changed, or your session ended before the effect.
- `JobsCancel` is a request. Check the state in the reply: `CANCELLED` means it stopped; `RUNNING`
  means the work cannot stop halfway and will report what it finally did.
- Keep at most 8 jobs running at once; more is `RESOURCE_EXHAUSTED`.

## Launching and installing games

`GamesRequestLaunch` and `GamesRequestInstall` ask the host to start a game or open the provider's
install flow. Rules and limits are under "Launching and installing" in
`docs/plugin-api-permissions.md`. What to do about them:

- Declare `game.launch` and/or `games.install`. Each answers with a job: follow it as described under
  Jobs. A launch needs the `expected_revision` you read in `LibraryEntry.revision`; if it changed,
  read the entry again.
- Send nothing about *how* to start: no executable, URI or arguments. Use `game.custom_launch` and an
  approved recipe for that. If it is denied, you may call the standard launch yourself as a
  fallback; the host will not do it for you.
- Expect the user to be asked. The job is `QUEUED` with the message `awaiting confirmation` while
  the dialog is open, and `FAILED` with `declined` or `timed_out` if the user says no or does not
  answer. Do not retry a decline in a loop; give the user a reason and a button. You can withdraw
  the request with `JobsCancel`.
- The user can choose "don't ask again" for your plugin and one game. That is not a permission you
  can rely on: it ends when the game's launch target changes or when the user revokes it.
- Only your own library entries (or the game the user selected for an extension) can be named.
  Anything else is `NOT_FOUND`.
- `SUCCEEDED` with `opened` means the operating system accepted the link, not that the game is
  running. Playtime is detected by GameNode as for the user's own Play button.

## Host catalog PC ports

Use `PortsList`/`PortsDetail` under `ports.read`; request an installation under the
independent `ports.install` permission with `PortsRequestInstall`. Send a known
catalog id and an idempotency key, then follow the wrapper through `JobsGet/Watch/Cancel`.
Leave `user_intent_id` empty and use revision 0 for a new catalog install. Core opens
its existing guided setup and the user chooses the destination, sources and risk
acknowledgements. Optional `supplied_file_handles` are approved, session-owned Files
candidates requiring `files.read`; the user associates them with recipe requirements.
No plugin-provided installer URL, command or raw path is accepted. See the
[ports API guide](plugin-api-permissions.md#pc-ports-from-the-host-catalog) for source-game
revision scope, platform availability, cancellation and native completion semantics.

## Host catalog tools

Use `ToolsList` with `tools.read` to discover the host catalog and availability.
`ToolsRequestApply`/`ToolsRequestRemove` need their own grants, the current scoped
library-entry revision and an idempotency key in `MutationContext`. They return a
job and open Core's tools window; the user chooses and confirms the installation
settings there. Send only a known tool id and GameNode game id, leave
`user_intent_id` empty and follow `JobsGet/Watch/Cancel`. Installer URLs, payloads and
target paths are not accepted. Cancellation, platform restrictions and filesystem
recovery are documented in the
[tools API guide](plugin-api-permissions.md#tools-from-the-host-catalog).

## Backups

Use `ResourceHost.BackupsList`, `BackupsRequestCreate` and `BackupsRequestRestore`
with an existing approved save profile; see
[backup scopes, effects and availability](plugin-api-permissions.md#backups-from-approved-save-profiles).
Creation and restoration need the current profile revision and an idempotency key,
return a caller-owned job and use the user's host backup destination. Obtain snapshot
ids from history instead of sending paths. Restoration always asks Core to review the
exact snapshot and affected files; it cannot use remembered launch approval.
Cancellation, lost grants and changed profiles fence the worker. The bundled engine
currently runs only on Windows; runtime/shared-emulator profiles remain unavailable
until their resource/activity coordination exists. Do not infer an empty history
or successful restoration from an unavailable/partial result.

## Action services through the SDK

`HostClient::actions()` returns an [`Actions`](../crates/plugin-sdk/src/actions.rs) client for
API 1.6 sessions. It sends exactly the raw `ResourceHost` requests and covers launch/install,
backup list/create/restore, host-catalog tools and ports, and the jobs they return:
`launch_game`, `install_game`, `backup_page`/`backups`, `create_backup`, `restore_backup`,
`tool_page`/`tools`, `apply_tool`, `remove_tool`, `port_page`/`ports`, `port`, `install_port`,
`job`, `watch_job` and `cancel_job`. It adds no authority: the host still checks every grant,
scope, revision, confirmation and job ownership.

Every action answers with a job, and the user may have to confirm it in Core. Until the dialog
closes the job is `QUEUED` with the message `awaiting confirmation`. Follow it with
`watch_job`, falling back to `job` if the stream ends early, and branch on the terminal state and
`result_code`; show `message` as plain text. A decline or timeout ends it `FAILED` without an
effect. If the operation's permission is revoked while the dialog is open, the commit is refused
and the job fails without an effect; a later confirmation cannot revive it. `PERMISSION_DENIED`
means the plugin must ask the user to approve the access again instead of retrying.
`cancel_job` withdraws a request that still waits; a job that already runs may answer `RUNNING`
and report what it finally did.

Reuse one idempotency key for retries of the same request and use a new key for the next change.
Launch needs the entry `revision`; backup create/restore need the save profile revision; tool and
port requests use the source game entry revision (0 for a new catalog port). Copy the runnable
recipes in
[`examples/action_recipes.rs`](../crates/plugin-sdk/examples/action_recipes.rs): `wait_for_job`,
`cancel`, `launch`, `install`, `apply_tool`, `remove_tool`, `install_port`, `create_backup` and
`restore_backup`.

## Save profiles

Use the `Saves` service to tell GameNode where your runtime keeps a game's saves. Rules and limits are
under "Save profiles" in `docs/plugin-api-permissions.md`; what to do about them:

- First get the user's approval for the folder with `FilesRequestAccess` (kind `save` or `prefix`).
  Put the folder handle (the root handle it returned, or a directory handle from `FilesList`) in
  `SaveDirectory.approved_resource_handle`. A profile cannot name a path.
- One profile per runtime and game entry: a ROM that two emulators can load gets two profiles with
  their own `runtime_id` and folders. Pick a name that tells the user which is which.
- Use naming rules to say which files inside the folder are saves (`GC/**/*.gci`) and which are not
  (`!cache/**`). They narrow; they cannot reach outside the approved folder.
- Keep the profile id from the create reply. Update with the revision you last saw, and on
  `REVISION_CONFLICT` read the profile again and decide. A handle in a reply is for the current session
  only; an empty one means the folder's approval is gone and you need to ask the user again.
- A profile is not permission to read, back up or restore anything, and removing it deletes nothing
  of the user's. Describe the format (`format_id`, `format_version`, metadata) only as a hint for
  other tools: GameNode does not parse or convert saves.

## Inspecting approved places

Use `Locations.GetSize`, `GetAccess` and `GetLastModified` to learn about a folder or a save profile
you already have, without reading it. Rules are under "Inspecting approved places" in
`docs/plugin-api-permissions.md`; what to do about them:

- Declare `game.saves.inspect` for save and prefix folders and profiles, `game.storage.inspect` for
  installation folders. You do not need `files.read` for this, and it does not give you any.
- Read `complete` before trusting a total: `false` means the walk was cut or part of the place was
  not reachable. Ask again with a larger `max_entries`, or treat it as "at least".
- Read `availability` before you read a number: `MISSING` or `INACCESSIBLE` mean you learned nothing
  about the contents; do not show "0 bytes", and do not create the folder again to make it go away.
- Do not treat `modified_at` as a hash. A changed time or size is a hint that something changed; a
  same time and size is not proof that nothing did.
- To know whether saves changed, take a fingerprint with `GetFingerprint` and compare it with the
  last one with `Compare`; do not rely on times. Give each request a new idempotency key. If the answer
  has only a `job_id`, wait for the job and ask again with the same key. Only `complete` fingerprints
  have a digest, and only a `changed` or `unchanged` status is a verdict: `indeterminate` means a file
  could not be read or was being written, so try again later and do not assume nothing changed.
- `effective_access` is a snapshot of what the user could do a moment ago, not a promise: the
  file may change, or the system may refuse for another reason. `unknown` is a real answer.

## Utility helpers and recipes (Rust SDK)

`Resources` also has typed calls for the utility services. They follow the same rules as the library
helpers above: each sends exactly what the raw `resource_client()` call would, returns the host's
answer unchanged, and adds no authority. The desktop's service tests make every call both ways (a
write by replaying it raw under the SDK's idempotency key) and compare the answers.

| `Resources` method | Raw call | Needs |
| --- | --- | --- |
| `storage_get`, `storage_put`, `storage_delete` | `StorageGet`, `StoragePut`, `StorageDelete` | `storage.read`, `storage.write` (+ `storage.persist` for `Durability::Persistent`), `storage.delete` |
| `files_request_access` | `FilesRequestAccess` | `files.read`, `files.write`, `files.delete` for the operations asked |
| `files_page`, `files_list` | `FilesList` | `files.read` |
| `files_read`, `files_read_all` | `FilesRead` | `files.read` |
| `files_write`, `files_create` | `FilesWrite` (`name` empty / set) | `files.write` |
| `files_delete` | `FilesDelete` | `files.delete` |
| `network_fetch` (`Fetch`, `Fetched`) | `NetworkFetch` | `network.fetch` and an approved origin |
| `notify` (`Notice`, `Severity`) | `NotificationsPublish` | `notifications.publish` |

What each refusal means, in the order a plugin usually handles it (the limits behind them are in
[`plugin-api-permissions.md`](plugin-api-permissions.md)):

| Status | Storage | Files | Network | Notifications |
| --- | --- | --- | --- | --- |
| `PERMISSION_DENIED` | the grant (or `storage.persist`) is not approved | the user declined the folder, or the operation was not approved | the origin, or a redirect target, is not approved | the grant is not approved |
| `FAILED_PRECONDITION` | the same key was used for another request | no approval dialog yet, or the handle must be requested again | | the same key was used for another notice |
| `NOT_FOUND` | | a handle that is not yours, or is gone | | |
| `INVALID_ARGUMENT` | key or value over its limit, durability unspecified | a name or offset the host refuses | a URL, method or header the host refuses | text with controls or disguising characters |
| `RESOURCE_EXHAUSTED` | the layer's quota (nothing was changed) | a file larger than your limit | too many fetches at once, or a response over your budget | 5 a minute: wait `retry-after-ms` |
| `UNAVAILABLE`, `DEADLINE_EXCEEDED` | resend with the same key | resend with the same key | try later | resend with the same key |

Only the last row is worth resending unchanged. A revision conflict (`Outcome::RevisionConflict`) is
an answer, not an error: read the key again, merge, and write with the new revision.

The recipes in
[`crates/plugin-sdk/examples/utility_recipes.rs`](../crates/plugin-sdk/examples/utility_recipes.rs)
put these rules into code you can copy. They are compiled with the SDK and run against the fixture
host in the desktop's service tests:

| Recipe | What it does |
| --- | --- |
| `send` | Sends one change, resending it with the same key only after a lost connection or timeout. |
| `recall`, `update`, `forget` | Reads a stored value (`None` when absent or expired), changes it from what it was with the revision it read (reading again after a conflict), and deletes it. |
| `open_installation` | Asks for a folder of one of your own entries and returns its root handle; tells a declined approval from one that must be asked again. |
| `open_save_folder` | Asks for read access to the save folder of one of your entries, suggesting where it usually is on each platform; the user takes one or chooses another. |
| `find`, `read_named`, `save_named` | Finds a file by name, reads it whole under a limit you choose, and creates or replaces a file by name. |
| `fetch_bytes` | Fetches an https URL under a size budget and a deadline, and says whether to ask the user to approve an origin or to retry. |
| `tell_user`, `retry_after` | Sends a plain-text notice with a translation, and stops, instead of queueing, when the rate limit says to wait. |

Two of them put the pieces together (they are tested like the rest): keep a counter that survives
restarts, and read a save file of a game.

```rust
pub async fn count_launch(resources: &Resources) -> Result<(), Status> {
    update(resources, "launches", Durability::Persistent, None, |old| {
        let count = old
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .and_then(|text| text.parse::<u64>().ok())
            .unwrap_or(0);
        (count + 1).to_string().into_bytes()
    })
    .await
    .map(|_| ())
}

pub async fn read_slot(
    resources: &Resources,
    installation_id: &str,
) -> Result<Option<Vec<u8>>, Status> {
    let Ok(root) = open_installation(resources, installation_id, &["read"]).await else {
        return Ok(None); // declined or not available yet: carry on without it
    };
    read_named(resources, &root, "slot1.sav", 1 << 20).await
}
```

### What these services do not do

They are an authorization boundary for GameNode's own services, not an operating-system sandbox:

- A plugin is an ordinary process with the user's operating-system permissions. Nothing here
  stops it from opening a file, a socket or a registry key itself, so a user's approval of a folder
  or an origin is not a promise that the plugin cannot reach more. Do not describe the plugin as
  confined, and do not use these grants to hide something the plugin does natively.
- The converse also holds: approved access is not a free pass. The host checks every call against
  the approved scope, and a plugin that names a path, a link or another folder gets a refusal that
  does not say what exists there.
- Store plugins that need native access (their own HTTPS, the store's install files, the Windows
  registry, D-Bus) declare it as documentation under "Host capabilities and native access are
  different" below; those declarations are not enforced by these services either.

## Bounded work, cancellation and failures

The host and SDK cap encoded and decoded gRPC messages at 8 MiB. A provider session has at most
32 in-flight operations; the SDK event queue is 32 entries. Discovery is limited to 50,000
records. Catalog transfer validation allows 50,000 records, at most 256 records per chunk and
32 MiB aggregate catalog data. `GetContext` reports negotiated record/message/chunk limits; use
those values instead of assuming a future host has the same limits.

Unary SDK service calls have a 15-second request timeout. The local connection attempt is bounded
to three seconds. Current host operation deadlines are 615 seconds for sign-in, 300 seconds for
catalog sync, 105 seconds for initialize/check, three seconds for settings cancellation and 90
seconds for other operations. Login policy additionally bounds its window lifetime and page probe
size. These provider-host values describe the current implementation. API 1.6 resource request
and response payloads are capped at 8 MiB; method-specific limits are listed in the route catalog.
Blocking IO and CPU work belongs on worker
threads, and async code must remain cancellable. Resource job state, progress, terminal outcome,
watch sequence and cancellation request fields are defined by `JobReply`, `JobEvent`,
`JobGetRequest`, `JobWatchRequest` and `JobCancelRequest`; the three job calls are served (see
[Jobs](#jobs)), though no operation returns a job yet. API 1.6 method-specific quota values are carried as catalog contract metadata and are not
claims about current desktop enforcement.

The SDK's legacy `Error` keeps its existing coarse variants for source compatibility. New code
can use `DetailedError`, which distinguishes denied, unauthenticated, stale, unsupported, invalid,
unavailable and protocol outcomes and decodes structured denial details when present. Never
include credentials, cookies, secret values, raw request bodies or arbitrary transport messages
in logs.

GameNode's provider API lets a bundled provider process implement store-specific behavior while
the desktop application remains responsible for the library, profile, persistence and GameNode
account. The process communicates with the running desktop through the versioned public crates
`gamenode-plugin-protocol` and `gamenode-plugin-sdk`. Provider plugins must not depend on
`gamenode-core`, `gamenode-app`, `gamenode-api` or `gamenode-platform`; they must not read or write
the GameNode database or GameNode credentials.

## Official plugins and trusted descriptors

Official plugins are compiled-in and trusted. Each one is described by a `plugins/<name>.toml`
descriptor that `crates/app/build.rs` embeds together with the provider manifest it names and the
login policy it references. The host parses and validates the descriptors once at startup; nothing
is read from disk at runtime. Adding a store therefore needs a plugin crate, a
`providers/<provider id>.toml` manifest and a `plugins/<name>.toml` descriptor, not a new Rust
branch in the host.

The bundled plugins are:

- **Steam** (Windows and Linux): installed discovery, account sign-in/check/sign-out, owned and
  Family catalog sync, playtime observations, Play/Install actions and achievement reads.
- **Linux package manager** (Linux): installed package candidates through PackageKit or pacman.
  It has context and discovery grants only.
- **[Epic Games Store](epic-games-store.md)** (Windows): installed discovery, launcher actions,
  owned catalog, playtime totals and an opt-in account.

The manual DRM-free provider is built into Core and is not a plugin.

A descriptor has these fields:

| Field | Meaning |
| --- | --- |
| `provider_id` | Stable provider id. It must be a plain lowercase id (`a-z`, `0-9`, `-`, `_`). |
| `plugin_id` | The plugin's public id, matched against the executable's handshake. |
| `name` | Display name used by inventory, Stores and provider-neutral copy. |
| `platforms` | Platforms where the plugin is available. They must be a subset of the manifest's platforms. |
| `capabilities` | The host-service grants approved for this provider. |
| `executable` | File name of the executable placed next to the desktop host. |
| `provider_manifest` | Provider id of the embedded `providers/<id>.toml` manifest. It must equal `provider_id`. |
| `login_policy` | Optional repo-relative path to an embedded login policy. Present exactly when `auth.webview` is granted. |
| `credential_alias` | Optional existing keyring account and private file for a provider that must keep its historical physical credential. |
| `native_access` | Declarations of OS access the plugin itself needs. They document intent; they are not sandbox rules. |
| `legacy_login_profiles` | Profile directory names from older builds that startup cleanup removes. |

Descriptors are validated before use. A descriptor is skipped, with a diagnostic, when its manifest
is missing or its id differs, it lists a platform its manifest does not, its login policy presence
does not match `auth.webview`, it grants `auth.webview.cookies.read` without a policy that declares
cookie names, it names an unknown or duplicate capability, its provider id duplicates another
descriptor, or its credential alias is not a plain provider-specific name (including the GameNode
session names). Every other descriptor still loads. Inventory, provider backends and test fixtures
all derive from the same validated data.

## Session and operations

The host starts a registered plugin process and provides its local endpoint, a temporary session
credential and an operation identifier through the environment. The plugin opens a session with
`Session::open(Launch::from_env()?, plugin_id, provider_id, version, capabilities)` and implements
the async `ProviderPlugin` trait. `run(session, Arc::new(plugin))` dispatches host commands. Use the
`HostClient` scoped to each command for approved services, and return an `OperationResult` through
the SDK. Do not retain a command's client to authorize later work: operation, account and profile
generations fence stale requests.

The current public API is major 1, minor 6. Minors 1–5 are the official provider baseline negotiated
with the single-minor handshake: minor 5 adds optional last-played counter values, catalog game
fields, the `invalid credential` catalog failure and cookie-free login policies; minor 4 added
achievement reads; minor 3 added typed settings; minor 2 added the full operation set. Minor 6 is
the first third-party API: it adds range negotiation, external configuration and the resource
surface described below. A plugin negotiates its newest supported minor within major 1, and the
host grants the intersection of the requested capabilities with its trusted descriptor; a plugin
cannot grant itself additional access. Unknown or duplicate permissions are rejected in
`gamenode-plugin-protocol`.

## Capabilities

The existing provider grant set (`capability::ALL`) is:

| Capability | What the host service permits |
| --- | --- |
| `provider.context.read` | Read this provider's validated configuration, platform and negotiated operation limits. It does not expose GameNode credentials or another provider's settings. |
| `library.discovery` | Submit installed-game observations for the current provider scan. The host reconciles them with the library; this is not direct library or database write access. |
| `library.catalog` | Submit complete owned and family-sharing catalog outcomes for the current account operation, including categorized failures. This is not access to the PC ports catalog or its installation workflow. |
| `account.state.read` | Read this provider's non-secret account identity and status. |
| `account.state.publish` | Publish validated account transitions and status. Secret values are excluded from account state and GUI snapshots. |
| `auth.webview` | Open, focus, cancel and close this provider's isolated sign-in attempt. The host enforces the packaged navigation policy. |
| `auth.webview.cookies.read` | Read only the approved cookie names for the active login attempt. Required only when the trusted policy declares cookies. |
| `auth.webview.page.read` | Read only the bounded page probe at the exact origin in the trusted login policy. Plugins cannot submit arbitrary scripts or origins. |
| `secrets.read` | Read this provider's logical secret slot. |
| `secrets.write` | Write this provider's logical secret slot. |
| `secrets.delete` | Delete this provider's logical secret slot. |
| `playtime.counter` | Return provider-local total-playtime and last-played observations. Core retains tracking, attribution, deduplication and submission policy. |
| `provider.actions.declare` | Declare approved provider actions that the host validates against trusted URI schemes and templates. It does not authorize arbitrary commands, shells or filesystem changes. |
| `achievements.read` | Return bounded per-game achievement definitions and the connected account's unlock state for this provider. The host rejects unrequested game ids, oversized batches and strings, and results fenced by an account change. Available from API minor 4. |

A provider should request only the capabilities it actually needs. Capability names are not
free-form: a descriptor containing an unknown name is invalid and is not loaded.

## Provider secrets

Every provider sees one logical secret slot named `session`; the host rejects any other key. A
plugin cannot choose a physical keyring account, file name or another provider's slot.

The physical slot is derived from the provider id, unless the trusted descriptor declares an alias:

- **Default keyring account:** `provider.<provider_id>`.
- **Default Linux private file:** `private/provider-<provider_id>.secret` below the application data
  directory.
- **Steam's legacy alias:** `credential_alias = { keyring_account = "steam", private_file = "steam-token" }`.
  Existing Steam sessions keep the same physical credential, with no migration, renaming or
  re-sign-in.

The GameNode account session remains in its own `session` keyring account and
`private/session-token` file. Descriptor aliases may not reuse those names. On Windows the host
uses Credential Manager and has no plaintext fallback. On Linux it uses Secret Service and falls
back to a user-only private file only when no Secret Service exists; a locked or failing service is
reported as an error. Secret read, write and delete are independent grants, and a reported storage
method is not proof that a write was durable.

## Login policies and cookie-free extraction

`auth.webview` opens an isolated window with an ephemeral profile scoped to the provider. URLs,
allowed hosts, the page probe and any cookies come from the trusted login policy. Runtime requests
supply only window handles and operations. External HTTPS links outside the approved hosts open in
the system browser. The probe runs only at the exact `probe_origin`, and a navigation change
invalidates an in-flight result.

A policy may read cookies or only approve a page probe:

- `cookie_urls` is an optional list of URLs to read cookies from. When non-empty,
  `cookie_names` must be non-empty; when empty, `cookie_names` must be empty. Several URLs let a
  policy capture a session whose cookies Steam scopes to different hosts.
- `auth.webview.cookies.read` is valid only with a policy that declares cookie names. A
  cookie-free policy does not grant it.
- A login `Read` on a cookie-free policy skips cookie collection entirely and returns only
  `page_result_json`. This supports authorization-code flows where the plugin receives a one-time
  code instead of a cookie.
- `page_result_json` is bounded JSON and is secret-bearing. The host never logs it and never
  includes it, or any cookie value, in tracing output or GUI snapshots.

Each attempt uses `private/login/<provider>` and the host removes that directory when the attempt
ends, whether it succeeded, was cancelled or timed out. At startup the host also removes the
`legacy_login_profiles` named by trusted descriptors. A newer attempt's profile is never deleted by
a late cleanup from an older attempt.

The protocol also defines the optional `ui.settings` grant (API 1.3), `ui.configure` (API 1.6),
and API 1.6 resource grants for library reads and metadata, favorites, matching, artwork, game
launch/install, storage, files, network, notifications, backups, library roots, runtimes,
diagnostics, computer hardware, game sessions/presence/performance, custom data and ports.
These identifiers are known to the protocol but are not part of the legacy `ALL` grant set.
Knowing an identifier never grants it. The complete identifier list is in
[`capability.rs`](../crates/plugin-protocol/src/capability.rs); method-by-method scopes and
availability remain subject to the pending API 1.6 permission matrix.

The Steam plugin uses the full capability set above to provide installed discovery, account
sign-in/check/sign-out, owned and Family catalog sync, Steam playtime observations and Play/Install
actions. Its trusted descriptor also contains Steam-specific native-access declarations described
below. Optional settings capability `ui.settings` exists in the protocol crate, but is outside the
Steam integration scope and is not part of Steam's grant. There is no plugin-provided catalog
browsing UI or PC ports installation service in this integration.

## Host capabilities and native access are different

Host capabilities authorize calls to explicit GameNode services. They are scoped to the plugin,
provider, process session and operation, and the host checks the grant on each call. For example,
`secrets.read` authorizes a request for the plugin's logical secret; it does not authorize reading
arbitrary files or the operating system's credential database directly.

Native-access declarations describe what the trusted bundled executable needs to access for its
own implementation. A store plugin may need HTTPS access to the store's API, reads of store
library and configuration files (including supported Linux native/Flatpak locations), the store
install root from the Windows registry, or process and D-Bus access. These declarations document
expected access; they are not operating-system sandbox rules. The plugin is an ordinary process
running with the user's OS permissions, so filesystem and network access by that process are not
confined by the host API. Community code is not enabled by these bundled-plugin permissions or
declarations.

## Implementation rules for store plugins

- Keep store parsing, account/session validation, HTTP calls, login interpretation, discovery and
  the store's own playtime counter inside that store's plugin. Use the public host services for
  login windows, provider secrets, account state, discovery/catalog results and action
  declarations.
- Keep GameNode authentication, matching, library reconciliation, favorites, artwork, profile
  persistence, playtime attribution and submission policy in the desktop/Core.
- Store login URLs, allowed hosts, cookies and page probes come from trusted packaged policy data.
  Runtime requests must not replace that policy. Do not log cookies, tokens, authorization codes,
  credentials, raw secret-bearing messages or complete account URLs.
- Distinguish a successful empty result from a failed or incomplete operation. A failure must not
  be reported as an empty catalog or discovery result; the host preserves prior successful facts
  according to the operation's contract.
- Keep blocking file, registry and credential access off the UI thread. Network work and user
  login waits must remain cancellable and must not block the desktop event loop.

The concrete DTOs, RPC messages, capability identifiers and compatibility rules are defined in
[`crates/plugin-protocol`](../crates/plugin-protocol); the async client and provider trait are in
[`crates/plugin-sdk`](../crates/plugin-sdk). Trusted descriptors live in [`plugins`](../plugins),
and provider manifests live in [`providers`](../providers).
