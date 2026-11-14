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
`ArtworkRead` and `ArtworkSet` (grants,
scope, cursors and watch rules are in
[`plugin-api-permissions.md`](plugin-api-permissions.md#library-reads); the writable display name,
revisions, favorites and idempotency keys are in
[`plugin-api-permissions.md`](plugin-api-permissions.md#display-metadata-and-favorites); catalog
search bounds and assignment results are in
[`plugin-api-permissions.md`](plugin-api-permissions.md#matching); the cover override, image limits
and read budget are in
[`plugin-api-permissions.md`](plugin-api-permissions.md#artwork)).
Every other `ResourceHost` method and all domain services answer `UNIMPLEMENTED`, so generated
clients cannot call them successfully yet. Call `DescribeApi` for the methods this build serves
instead of inferring availability from the protobuf contract.

Resource calls are plugin-initiated and use the `resource_context` that `OpenSession` returns to API
1.6 sessions (`HostClient::resource_context()` in the Rust SDK). Put it in the `context` of each
request, for example `LibraryListRequest.page.context`; the launch operation context is not
accepted. The context grants nothing by itself.

Public-data reads use a typed per-query `CommonDataSelector` on each query that supports shared
projections. Its categories and selected record ids are checked independently; a selected-record
grant does not require category-wide access. Rust plugins can construct one with
`gamenode_plugin_sdk::common_data_selector([CommonDataCategory::GamePerformanceHistory])` and
set it on the operation-specific request, such as `LibraryListRequest.page` or
`DataGetRequest.common_data_categories`; selected-record clients populate
`selected_record_ids`. Omitting or emptying the selector keeps the query private to the caller's
visible records. Selectors request data only; the host still checks each grant and scope. Secrets
remain excluded, and sensitive/custom datasets require separate sharing review.

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

The Rust SDK has no typed storage helpers yet (they arrive with the utility-service examples of task
6.5); use the generated `ResourceHost` client as the Go example does.

## File access

A plugin never names a path. It asks `FilesRequestAccess` for an installation of one of its own
library entries, says what it needs (`read`, `write`, `delete`) and which folder kind (`installation`,
`save`, `prefix`), and the user chooses and confirms the folder. The reply holds opaque handles; see
"Files" in `docs/plugin-api-permissions.md` for the rules, limits and errors. The practical points:

- Declare `files.read`, `files.write` and `files.delete` only for what you use. They are separate,
  off at installation, and the user approves a folder per game; an approval for `read` does not allow
  `write`, and neither allows `delete`.
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
`JobGetRequest`, `JobWatchRequest` and `JobCancelRequest`; service implementation is not mounted
yet. API 1.6 method-specific quota values are carried as catalog contract metadata and are not
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
diagnostics, computer hardware, game sessions/presence/performance, custom data, tools and ports.
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
