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
opens `ProviderHost.Exchange`. API 1.1–1.3 clients keep using `api_major` and `api_minor`; an
empty API range in the additive fields means the legacy request, where `api_minor` is the newest
minor the client supports and the host may select any supported minor from the API minimum through
that value. API 1.4 clients may
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
services described below; the API 1.4 `ResourceHost` is a contract in the `.proto` and is not yet
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
| External configuration (API 1.4, `ui.configure`) | `ConfigurationOpen`, `ConfigurationFocus`, `ConfigurationClose` |

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

## RPC inventory and API 1.4 status

The mounted `PluginHost` service has `OpenSession`, the legacy `SubmitDiscovery`, and API 1.4
control RPCs `DescribeApi`, `GetPermissions` and `WatchPermissionChanges`. The three control
methods authenticate the session and do not require a library/data permission. They return public
API metadata, the caller's own grants, and changes to those grants. Permission denial details use
safe structured fields (reason, permission ID and grant revision); clients should branch on gRPC
status and these fields, never on raw status text.

The mounted `ProviderHost` service has `Exchange`, `GetContext`, `Account`, `Secret`, `Login` and
`DeclareActions`. These are the existing provider service methods. Discovery, catalog, counters,
status and lifecycle results travel through `Exchange`; they are not extra unary RPCs.

The API 1.4 `ResourceHost` and domain-service contracts in `crates/plugin-protocol/proto/plugin.proto`
define typed request and response messages. The checked
[`resource_host.json`](../crates/plugin-protocol/contracts/resource_host.json) and
[`domain_host.json`](../crates/plugin-protocol/contracts/domain_host.json) catalogs record each
method, message, stream, permission, owner, revision, pagination, quota and idempotency rule;
contract tests compare every route to the generated protobuf descriptor. The permission grouping
is in [`plugin-api-permissions.md`](plugin-api-permissions.md). Unknown routes are `UNIMPLEMENTED`.

These API 1.4 resource and domain methods remain contract-only: the desktop does not mount their
services, so generated clients cannot call them successfully yet. Provider and control services
described above are mounted. Do not infer runtime availability from the protobuf contract.

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

| Area | API 1.4 contract method names |
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
size. These provider-host values describe the current implementation. API 1.4 resource request
and response payloads are capped at 8 MiB; method-specific limits are listed in the route catalog.
Blocking IO and CPU work belongs on worker
threads, and async code must remain cancellable. Resource job state, progress, terminal outcome,
watch sequence and cancellation request fields are defined by `JobReply`, `JobEvent`,
`JobGetRequest`, `JobWatchRequest` and `JobCancelRequest`; service implementation is not mounted
yet. API 1.4 method-specific quota values are carried as catalog contract metadata and are not
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

## Session and operations

The host starts a registered plugin process and provides its local endpoint, a temporary session
credential and an operation identifier through the environment. The plugin opens a session with
`Session::open(Launch::from_env()?, plugin_id, provider_id, version, capabilities)` and implements
the async `ProviderPlugin` trait. `run(session, Arc::new(plugin))` dispatches host commands. Use the
`HostClient` scoped to each command for approved services, and return an `OperationResult` through
the SDK. Do not retain a command's client to authorize later work: operation, account and profile
generations fence stale requests.

The existing provider API defines discovery, account, catalog, login, secret, counter and action
operations. A provider should request only the capabilities it actually needs. The host compares
the request with trusted packaged metadata and grants the intersection; a plugin cannot grant
itself additional access. API major/minor negotiation and unknown or duplicate permission checks
are implemented in `gamenode-plugin-protocol`.

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
| `auth.webview.cookies.read` | Read only the approved cookie names for the active login attempt. |
| `auth.webview.page.read` | Read only the bounded page probe at the exact origin in trusted login policy. Plugins cannot submit arbitrary scripts or origins. |
| `secrets.read` | Read this provider's logical secret slot. |
| `secrets.write` | Write this provider's logical secret slot. |
| `secrets.delete` | Delete this provider's logical secret slot. |
| `playtime.counter` | Return provider-local total-playtime and last-played observations. Core retains tracking, attribution, deduplication and submission policy. |
| `provider.actions.declare` | Declare approved provider actions that the host validates against trusted URI schemes and templates. It does not authorize arbitrary commands, shells or filesystem changes. |

Secret read, write and delete are independent grants. The host uses Windows Credential Manager on
Windows and the existing Secret Service/private-file policy on Linux. For Steam, the logical
`session` slot is mapped by trusted host metadata to the existing physical `CredentialSlot::Steam`
alias (`steam` in the keyring and `steam-token` in the Linux private-file fallback). The GameNode
session remains in its separate `Session` slot. Windows has no plaintext fallback. A plugin must
not treat a reported storage method as proof that a secret has been persisted successfully.

The protocol also defines the optional `ui.settings` grant (API 1.3), `ui.configure` (API 1.4),
and API 1.4 resource grants for library reads and metadata, favorites, matching, artwork, game
launch/install, storage, files, network, notifications, backups, library roots, runtimes,
diagnostics, computer hardware, game sessions/presence/performance, custom data, tools and ports.
These identifiers are known to the protocol but are not part of the legacy `ALL` grant set.
Knowing an identifier never grants it. The complete identifier list is in
[`capability.rs`](../crates/plugin-protocol/src/capability.rs); method-by-method scopes and
availability remain subject to the pending API 1.4 permission matrix.

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
own implementation. Steam needs HTTPS access to Steam's store/API, reads of Steam library and
configuration files (including supported Linux native/Flatpak locations), and the Steam install
root from the current user's Windows registry. These declarations document expected access; they
are not operating-system sandbox rules. The plugin is an ordinary process running with the user's
OS permissions, so filesystem and network access by that process are not confined by the host API.
Community code is not enabled by these bundled-plugin permissions or declarations.

## Steam-specific implementation rules

- Keep Steam parsing, account/session validation, HTTP calls, login interpretation, discovery and
  Steam's own playtime counter inside the Steam plugin. Use the public host services for login
  windows, provider secrets, account state, discovery/catalog results and action declarations.
- Keep GameNode authentication, matching, library reconciliation, favorites, artwork, profile
  persistence, playtime attribution and submission policy in the desktop/Core.
- Steam login URLs, allowed hosts, cookies and page probe come from trusted packaged policy data.
  Runtime requests must not replace that policy. Do not log cookies, tokens, credentials, raw
  secret-bearing messages or complete account URLs.
- Distinguish a successful empty result from a failed or incomplete operation. A failure must not
  be reported as an empty catalog or discovery result; the host preserves prior successful facts
  according to the operation's contract.
- Keep blocking file, registry and credential access off the UI thread. Network work and user
  login waits must remain cancellable and must not block the desktop event loop.

The concrete DTOs, RPC messages, capability identifiers and compatibility rules are defined in
[`crates/plugin-protocol`](../crates/plugin-protocol); the async client and provider trait are in
[`crates/plugin-sdk`](../crates/plugin-sdk). Trusted Steam policy and packaging details are
maintained alongside the integration implementation.
