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

When Core starts an extension from a game's **Start with extension** action, it also supplies
`GAMENODE_PLUGIN_USER_INTENT_ID`. Read it with `Launch::user_intent_id()` and pass it unchanged to
`Actions::launch_game` or `Actions::install_game`. It is single-use, expires after 60 seconds and
is bound to that extension, account profile, game, installation, operation and launch target.
Core also supplies `GAMENODE_PLUGIN_TARGET_GAME_ID`,
`GAMENODE_PLUGIN_TARGET_INSTALLATION_ID`, `GAMENODE_PLUGIN_TARGET_PROFILE_ID` and
`GAMENODE_PLUGIN_TARGET_REVISION`. Use `Launch::selected_target()` or
`Session::open_extension_from_launch()` to open the session for that host-selected target. The host
checks the submitted target exactly; an extension cannot substitute another installation.
Provider sessions and extensions started without this Core action receive neither the target nor
the intent environment values.

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

## Management lifecycle, configuration and plugin-owned UI

GameNode manages package trust and installation, permission grants, per-account activation,
process startup and shutdown, update recovery and removal. Activation answers whether the host may
run a registered plugin for the current account; it does not mean that the plugin has completed its
own setup, signed in to its service, or configured a feature. Functional settings and configuration
are separate provider operations (`ui.settings` or `ui.configure`) and belong to the plugin's
feature lifecycle. Plugins own the meaning and persistence of those values. Do not use enable or
disable as a substitute for a functional configuration operation.

When a plugin offers configuration UI, it owns and runs that UI outside the GameNode frontend,
such as in a window or web surface created by the plugin. The host may dispatch typed configuration
requests and present host-owned activation and permission controls, but it does not load plugin
HTML, JavaScript, or other plugin code into the GameNode React/Tauri frontend. Plugins should keep
their private data in their own account-scoped storage and treat its retention as separate from
activation, grants and package removal. Removing a plugin removes its packages and account
activation/grants; it does not implicitly erase the plugin's private data. Uninstall offers one
keep-or-delete choice for durable shared records: keep archives them under the original owner for
a separate reviewed reuse, and delete removes only that owner's records.

Process supervision is lifecycle management, not an OS sandbox. On Linux the host supervises a
process group; on Windows it assigns the process to a Job Object configured to terminate processes
when the job closes. Shutdown asks the plugin to stop, waits for a bounded period, then terminates
the supervised tree. These measures limit leftover host-managed processes, but a native plugin runs
with the user's OS identity and can access resources available to that identity directly. API grants
do not restrict direct filesystem, network or OS access, and process-tree supervision is not a
security boundary against a malicious native executable.

The lifecycle fixtures keep these concerns distinct:
`managed_lifecycle_refuses_disabled_plugins_and_requires_an_active_profile` checks activation and
profile requirements, while
`activation_persists_per_account_and_remove_stops_unregisters_and_deletes_packages` checks account
switching, restart, disable and removal. Settings/configuration have separate typed operation
contracts. The GameNode UI contains host-owned activation and permission surfaces; it does not
embed or execute plugin frontend code.

## Bundling a provider and an extension

A `.gnbundle` distributes several signed `.gnplugin` packages as one download, one publisher
review and one Stores entry. Use it when one product ships more than one component, such as a
discovery provider plus a launch extension. The archive layout, signature domain and limits are in
[the package manifest](plugin-package-manifest.md#plugin-bundles); a bundle never merges the
components' identities, permissions, API ranges or lifecycle.

Build the components first, then the envelope (the paths below are the plugin repository's own):

```sh
python3 scripts/plugin-package.py keygen --development --private-key /tmp/plugin-keys/dev.pem
python3 scripts/plugin-package.py pack --source provider/package \
  --private-key /tmp/plugin-keys/dev.pem --output dist/provider.gnplugin
python3 scripts/plugin-package.py pack --source launch/package \
  --private-key /tmp/plugin-keys/dev.pem --output dist/launch.gnplugin
python3 scripts/plugin-package.py bundle \
  --component dist/provider.gnplugin \
  --component dist/launch.gnplugin \
  --id org.example.product.bundle \
  --name "Example product" \
  --version 0.1.0 \
  --private-key /tmp/plugin-keys/dev.pem \
  --output dist/product-0.1.0-linux-x64.gnbundle
python3 scripts/plugin-package.py bundle-verify dist/product-0.1.0-linux-x64.gnbundle
sha256sum dist/*.gnplugin dist/*.gnbundle
```

Every component must be signed by the bundle publisher key; mixed publishers, duplicate plugin
ids, nested bundles and oversized or case-colliding entries fail before any registry write.
`keygen --development` marks the artifacts as development builds; it is not a production
publisher identity. Keep the private key outside Git and publish the recorded SHA-256 values with
the download.

Import the `.gnbundle` from the Stores settings. The host stages and validates every component and
shows one aggregated review: the bundle identity, one publisher fingerprint, grouped licenses,
and each component's compatibility, permissions and reasons. Confirming the fingerprint grants
nothing. Install activates every compatible component atomically; a platform- or API-incompatible
component is installed unavailable without blocking its siblings. The inventory then shows one
bundle row with each component's status and its own enable/disable control. One uninstall action
stops and removes every component and applies one keep-or-delete choice to each component's
private data and durable shared records.

### Dolphin acceptance (Linux x86_64, 2026-11-21)

The sibling `gamenode-plugin-dolphin-emu` repository delivers the real acceptance artifact; its
`README.md` and `TASKS.md` own the plugin-side build, signing and remaining launch work. The
desktop acceptance for the signed bundle is the host test
`plugin_host::bundles::tests::the_real_dolphin_bundle_is_one_review_one_inventory_entry_and_one_uninstall`:

```sh
DOLPHIN_BUNDLE_PATH=/path/to/gamenode-plugin-dolphin-emu/dist/dolphin-emu-0.1.0-linux-x64.gnbundle \
CARGO_TARGET_DIR=/data/DADOS_LINUX/geral/area_troca_rust/target-dolphin-acceptance \
CARGO_NET_OFFLINE=true CARGO_INCREMENTAL=0 \
cargo test --locked -p gamenode-app --lib -- the_real_dolphin_bundle
```

| Item | Accepted value |
| --- | --- |
| Host revision | `971a8a8` (test commit `8cff197`) |
| Dolphin revision | `83c3d94` |
| Bundle | `dev.richmont.gamenode.dolphin-emu.bundle` 0.1.0 |
| Bundle SHA-256 | `2718d7fd4a42359e63b03f46622758570cdf2acb6e57096acaaf1ca85b0070b3` |
| Provider | `dev.richmont.gamenode.dolphin-emu` 0.1.0 (`e7243b68…f14`) |
| Extension | `dev.richmont.gamenode.dolphin-emu-launch` 0.1.0 (`56670c46…f8e`) |
| Publisher | `richmont-dolphin-emu-development` (`SHA256:27a81cf6a123d11e8e1438f1ceb4d8f7b706aeb4841a120e39b09d246389b24c`) |

The test passes with one aggregated review for both components and one publisher confirmation,
one install of both components, one inventory row reporting the provider installed and the
extension installed with its runtime available, and one uninstall that stops and removes both
components. Keep leaves the signed-in profile's private namespace archived and inert; a reinstall
followed by Delete removes only that profile's namespace. The provider's own
`DOLPHIN_PLUGIN_ROOTS` discovery was exercised at the host level and reported the expected
six-character disc record.

Durable catalog acceptance (`add-durable-shared-data` 6.3 / `add-plugin-bundles` 7.2, Linux) is
the host test
`plugin_host::runtime::tests::the_real_dolphin_provider_publishes_a_durable_catalog_read_with_it_stopped`:

```sh
DOLPHIN_BUNDLE_PATH=/path/to/gamenode-plugin-dolphin-emu/dist/dolphin-emu-0.1.0-linux-x64.gnbundle \
CARGO_TARGET_DIR=/data/DADOS_LINUX/geral/area_troca_rust/target-dolphin-acceptance \
CARGO_NET_OFFLINE=true CARGO_INCREMENTAL=0 \
cargo test --locked -p gamenode-app --lib -- the_real_dolphin_provider_publishes
```

It installs the same signed artifact, grants `provider.context.read`, `library.discovery`,
`data.publish` and `storage.persist`, and runs the provider through the production
`HostedProviders::bundled` registration with `DOLPHIN_PLUGIN_ROOTS` pointing at a temporary
collection. `Discover` lists `GZLE01` / `The Legend of Zelda` / `gamecube`, and the provider's own
`PERSISTENT` `DataPublish` writes the `dev.richmont.gamenode.dolphin-emu.catalog.v1` row
(`disc_id`/`title`/`platform`/`content_id`). Provenance names a 32-byte `package_digest` equal to
`installed_package_digest` (SHA-256 of the installed signed `integrity.json`) and the active
profile. A consumer under the exact source/recipient/dataset policy reads only those fields with
`PERSISTENT` durability while the provider process is stopped, a fresh broker over the reopened
pool reads the same record, and revoking the policy fences the watch and reads. No path or reserved
field is projected.

The Dolphin repository still has no Windows target (both manifests declare only `linux/x86_64`),
so there is no Windows bundle to cross-build; native Windows acceptance remains the Lamarck
(GameNode Core) handoff tracked as task 7.2-win. `add-plugin-bundles` 7.2 therefore stays open on
that clause with the complete Linux evidence recorded in its task note.

### Installed providers join the library

An accepted import makes a provider package part of the running library, not only of the plugin
runtime. At startup the desktop composes the official manifests and then appends every active
third-party package whose signed manifest has `kind: "provider"` and declares `library.discovery`,
provided its publisher key is pinned, the package is signed and intact, and one target is
compatible with this machine. The synthetic provider manifest keeps the verified id and name,
uses `source = "community"` and `[scan] plugin = <plugin_id>`, so scans go through the same plugin
`Discover` operation as an official plugin-backed provider. Official providers keep their order
and behavior. After `plugin_import_install` or `plugin_bundle_import_install` the command re-reads
the installed packages and adds/replaces the provider in the existing library, then requests a
scan; no restart and no second `HostedProviders` is involved. Ineligible packages are skipped with
a logged diagnostic: untrusted or changed publisher keys, incompatible targets, extension-only
packages, packages without `library.discovery`, reserved provider ids and malformed manifests.

The always-running regression fixtures are in `plugin_host::startup::tests`:

```sh
CARGO_TARGET_DIR=/data/DADOS_LINUX/geral/area_troca_rust/target-provider-discovery \
CARGO_NET_OFFLINE=true CARGO_INCREMENTAL=0 \
cargo test --locked -p gamenode-app --lib -- plugin_host::startup::tests
```

They install real signed fixture packages into a temporary registry, run the binary's own
composition, drive the library scan effects through a provider runtime and assert the entries and
the `provider_scans` success row. The real signed Dolphin end-to-end acceptance is gated because it
needs the sibling repository artifact; it imports the bundle into an already composed host, points
`DOLPHIN_PLUGIN_ROOTS` at a temporary collection with `The Legend of Zelda.iso` (`GZLE01`), and
asserts that `dolphin-emu` joins the library provider list, the record reaches `library_entries`,
and `provider_scans` records success:

```sh
DOLPHIN_BUNDLE_PATH=/path/to/gamenode-plugin-dolphin-emu/dist/dolphin-emu-0.1.0-linux-x64.gnbundle \
CARGO_TARGET_DIR=/data/DADOS_LINUX/geral/area_troca_rust/target-provider-discovery \
CARGO_NET_OFFLINE=true CARGO_INCREMENTAL=0 \
cargo test --locked -p gamenode-app --lib -- \
  plugin_host::startup::tests::the_real_dolphin_bundle_joins_the_running_library
```

The fixture variant always runs; the Dolphin variant is the end-to-end acceptance evidence and is
not part of the regular CI job.

### The plugin lifecycle is live

The running desktop owns every lifecycle change. Restarting GameNode is recovery from a crash, not
a step the user takes to finish an operation.

| Operation | Effect in the running desktop |
| --- | --- |
| Import | Installs the reviewed package, applies the decisions, registers the process and enables it; a provider joins the library and a scan starts. |
| Enable | Starts the supervised process and requests a scan; cached rows stay visible until the new scan replaces or marks them. |
| Disable | Stops the process with the bounded grace period, revokes its sessions and publishes the stopped availability; cached library facts and rows remain. |
| Update / rollback | Replaces the immutable process configuration, restarts the process and requests a scan; added or wider permissions still need their normal review. |
| Uninstall | Stops and unregisters the plugin, drops its process slot, and removes the provider and its live rows from the library. Persisted rows stay hidden for a later reinstall. |
| Reinstall | Registers the provider again, adds it back to the library and scans it in the same session. |

A provider removed during a scan is dropped from that scan's pending set; a late discovery answer
for it is ignored instead of recreating rows. `LibraryMachine::remove_provider` removes the live
rows and `visible_entries` hides cached rows whose provider is not loaded, which also covers a
restart after an uninstall. Enable/disable, update/rollback and uninstall/reinstall are wired in
`crates/app/src/commands.rs`; no second `HostedProviders` or engine rebuild is involved.

The regression coverage is split between the core library machine
(`removing_a_provider_drops_its_live_rows_and_a_reinstall_restores_them`,
`cached_entries_of_a_provider_that_is_not_loaded_stay_hidden`), the live host
(`a_live_signed_provider_can_update_disable_remove_and_reinstall_without_a_restart`) and the
library/UI lifecycle acceptance tracked by task 18.11.

## RPC inventory and API 1.6 status

An endpoint is homologated only after a real signed bundle crosses it; fixtures and examples do not
count. [`plugin-endpoint-acceptance.md`](plugin-endpoint-acceptance.md) records each family, its
current consumer, the evidence and the cold paths that still have no real consumer.

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
session retention, durable `PERSISTENT` publication, field projection, snapshot-to-watch
resumption, retention and uninstall choices, and safe exclusions, see
[Shared plugin data](plugin-shared-data.md).

The ResourceHost method names, grouped for source/API review, are:

| Area | API 1.6 contract method names |
| --- | --- |
| Library and presentation | `LibraryList`, `LibraryGet`, `LibraryWatch`, `MetadataUpdate`, `FavoritesList`, `FavoritesSet`, `MatchingSearch`, `MatchingAssign`, `ArtworkRead`, `ArtworkSet` |
| Game execution and install | `GamesRequestLaunch`, `GamesProposeCustomLaunch`, `GamesRequestCustomLaunch`, `GamesRemoveCustomLaunch`, `GamesRequestInstall` |
| Storage, files and network | `StorageGet`, `StoragePut`, `StorageDelete`, `FilesRequestAccess`, `FilesList`, `FilesRead`, `FilesWrite`, `FilesDelete`, `NetworkFetch`, `NotificationsPublish` |
| Backups and libraries | `BackupsList`, `BackupsRequestCreate`, `BackupsRequestRestore`, `LibrariesList`, `LibrariesGet`, `LibrariesWatch`, `LibrariesCreate`, `LibrariesUpdate`, `LibrariesRemove`, `LibrariesAddRoot`, `LibrariesUpdateRoot`, `LibrariesRemoveRoot` |
| Runtimes and observations | `RuntimesList`, `RuntimesGet`, `RuntimesWatch`, `RuntimesRequestAccess`, `RuntimesRegister`, `RuntimesUpdate`, `RuntimesRemove`, `GamesRegisterContent`, `StorageDiagnosticsPublishBenchmark`, `StorageDiagnosticsPublishSmart`, `ComputerPublishHardware`, `GamePresencePublish`, `GamePresenceClear`, `GamePerformancePublishSamples`, `GamePerformanceEndSeries` |
| Data, persistence, tools, ports and jobs | `DataPublish`, `DataGet`, `DataList`, `DataWatch`, `PersistenceGetUsage`, `PersistenceListEntries`, `PersistenceDeleteEntries`, `PersistenceClear`, `PersistenceSetRetentionPolicy`, `ToolsList`, `ToolsRequestApply`, `ToolsRequestRemove`, `PortsList`, `PortsDetail`, `PortsRequestInstall`, `JobsGet`, `JobsWatch`, `JobsCancel` |

Each name above maps to exactly one typed request/response pair in the protobuf service and
catalog. `GamesRequestLaunch` takes `GameLaunchRequest` and has no target/argument override;
`GamesProposeCustomLaunch` and `GamesRequestCustomLaunch` take `CustomLaunchRequest` and require
the independent `game.custom_launch` grant. Their approvals and effects are not interchangeable.
A proposal may declare ordered mod layers (`CustomLaunchModLayer`); their argv and environment
changes are part of the host review. An execution request may reference only the stored recipe
revision and cannot supply or add layers or parameters.

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

## Shared durable data

A plugin that owns reference data consumers read later publishes it with `DataPublish` and
`durability = PERSISTENT`. The host stores it in its public pool, not in your private storage:

- Declare `data.publish` and ask the user for the separate `storage.persist` grant. Without it the
  publish is `PERMISSION_DENIED`; do not retry it as `SESSION_ONLY` unless that is the behavior you
  want. `SESSION_ONLY` remains correct for live observations and presence.
- Check the reply, not the request: `DataMutationReply.durability` and `DataRecord.durability`
  report the mode actually stored, and `expires_at` reports the expiry. `ttl_seconds` 0 means no
  expiry, capped at 30 days; one record is at most 1 MiB.
- Publish reference data with a stable `resource_id` to replace your own record; a publish without
  one appends a new record. A consumer reads the record through its reviewed category or
  selected-record grant even while you are stopped. Revocation, profile changes, expiry and user
  retention fence reads without requiring your session.
- Respect the owner quota and retention: 64 MiB logical bytes per owner by default, an optional
  user age limit, and bounded oldest-first pruning. A full quota returns `RESOURCE_EXHAUSTED` and
  changes nothing; free space or let values expire instead of assuming a silent overwrite.
- Uninstall can keep or delete your durable rows. Keep archives them under your owner identity:
  they are inert until the user approves the separate reuse review, so a reinstall cannot read or
  replace them automatically. Delete removes only your owner's rows. Never cache public reference
  data in private storage as a workaround: private bytes are not readable by consumers.

### Publishing and reading shared data with the Rust SDK

`HostClient::resources()` returns `Resources` bound to the session's `resource_context`. Its data
helpers fill only the fields the host owns, so the caller writes the publication itself and leaves
`mutation` empty; `data_publish` replaces it with the given key, `expected_revision` and the
session context:

```rust
use gamenode_plugin_sdk::{idempotency_key, proto::*};

let resources = host.resources().expect("an API 1.6 session");
let reply = resources
    .data_publish(
        &idempotency_key(),
        0, // publication is append-only, so expected_revision is always 0
        DataPublishRequest {
            dataset_id: "org.example.catalog.v1".into(),
            schema_id: "org.example.catalog.v1".into(),
            schema_version: 1,
            value: Some(catalog_entry), // a prost_types::Struct with your schema's fields
            durability: StorageDurability::Persistent as i32,
            ttl_seconds: 0,
            resource_id: "catalog-primary".into(),
            observed_at: observed_at,
            sequence: sequence,
            source_observation_id: observation_id,
            ..Default::default()
        },
    )
    .await?;
// The reply, not the request, says what was stored:
assert_eq!(reply.durability, StorageDurability::Persistent as i32);

// One page at a snapshot revision; carry `next_cursor` back to continue.
let page = resources
    .data_list("org.example.catalog.v1", "", None, 50)
    .await?;
let record = resources
    .data_get("org.example.catalog.v1", &reply.record_id, None)
    .await?;
```

`data_get`, `data_list` and `data_watch` take an optional `CommonDataSelector` for another
plugin's reviewed projection; without it they read only what is visible to your plugin. Pass a
list reply's `revision` to `data_watch`, and its `next_cursor` back unchanged to continue a list.
The helpers add no authority and no retries: the host still enforces `data.publish`, plus
`storage.persist` for `PERSISTENT`, and a missing grant is `PERMISSION_DENIED` rather than a
session-only fallback. Use `SESSION_ONLY` only when you really want the record to end with the
producing session.

The focused fixture commands are in
[Shared plugin data](plugin-shared-data.md#durable-publication-retention-and-cross-session-reads).
The Rust SDK's `Durability` helper and the generated `StorageDurability` enum both map
`PERSISTENT` to the same wire value.

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

- Declare `network.fetch` with a comma-separated list of exact HTTPS origins in the permission's
  `scope` string, for example `https://api.example.com, https://cdn.example.com:8443`. Each origin
  is a separate choice in the account's permission review. Requests to undeclared or unapproved
  origins return `PERMISSION_DENIED` before anything is sent.
- The host negotiates the `network.fetch` grant when the plugin starts. New origin decisions take
  effect immediately in sessions that already hold the grant. A package that changes its network
  declaration requires a new review of its origins.
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
  read the entry again. A Core-started extension uses the numeric revision in its host-selected
  `TargetGameContext` instead; it is bound to the launch target and does not require whole-library
  read permission.
- Send nothing about *how* to start: no executable, URI or arguments. Use `game.custom_launch` and an
  approved recipe for that. If it is denied, you may call the standard launch yourself as a
  fallback; the host will not do it for you.
- Expect the user to be asked. The job is `QUEUED` with the message `awaiting confirmation` while
  the dialog is open, and `FAILED` with `declined` or `timed_out` if the user says no or does not
  answer. Do not retry a decline in a loop; give the user a reason and a button. You can withdraw
  the request with `JobsCancel`.
- For a user-started extension action, pass the intent from `Launch::user_intent_id()` for the
  matching Play or Install request. Without a valid intent Core asks the user as usual.
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

## Approved runtimes and the host picker

An emulator extension does not receive a path, a folder handle or process-spawning authority.
Under `game.runtime.create`, call `RuntimesRequestAccess` with the runtime spec and text hints; the
host lets the user approve one exact executable and creates the owned runtime.

1. Send `RuntimeRequestAccessRequest` with a `RuntimeRecord` whose `executable_handle` is empty,
   plus `RuntimeAccessHints`. `executable_names` are base names such as `dolphin-emu`;
   `desktop_ids` are desktop ids such as `dolphin-emu.desktop`. They are text, never paths. Put a
   fresh idempotency key in `MutationContext`; reuse it for retries of that request.
2. The host looks for an owned runtime with the same name, platform and supported formats. If its
   approved executable still matches its recorded identity, size and SHA-256, the host returns that
   runtime with `ALREADY_APPLIED` and opens no picker.
3. Otherwise Core shows the picker with the requesting plugin/publisher and the runtime. It offers
   only host-owned installed-program suggestions — the official PackageKit inventory on Linux; the
   Windows registry/Start Menu source is not implemented in this build — and always offers
   **Browse…**. Third-party providers never contribute candidates. The user sees the exact program,
   package id/manager/version when known, and can cancel.
4. The host validates the chosen regular executable (no link in the final component, executable
   permissions, not inside GameNode's private data), records its volume/node identity, size and
   SHA-256, stores a durable approval and creates the runtime. A decline or cancel creates nothing.
   The reply is an ordinary `RuntimeMutationReply`; its runtime record has an empty
   `executable_handle` and never contains a path.
5. Bind content with `GamesRegisterContent`, then propose or request your recipe with the runtime
   id as usual. The host resolves the approved program only at launch, after rechecking the
   approval's ownership, profile, runtime revision, identity, size and digest. If the program was
   replaced or changed, the launch is refused and the next `RuntimesRequestAccess` shows the picker
   again. There is no fallback to another program.

Use the Rust SDK's `Resources::runtime_request_access` with `RuntimeExecutableHints`; it refuses a
`RuntimeSpec` that already has an `executable_handle`. `RuntimesRegister` remains for runtimes whose
executable comes from an already approved installation, prefix or library folder. For an emulator
program, prefer the host picker: it approves one file instead of a folder.

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
| `secrets.read` | Read one of this provider's host-owned logical secret slots. |
| `secrets.write` | Write one of this provider's host-owned logical secret slots. |
| `secrets.delete` | Delete one of this provider's host-owned logical secret slots. |
| `playtime.counter` | Return provider-local total-playtime and last-played observations. Core retains tracking, attribution, deduplication and submission policy. |
| `provider.actions.declare` | Declare approved provider actions that the host validates against trusted URI schemes and templates. It does not authorize arbitrary commands, shells or filesystem changes. |
| `achievements.read` | Return bounded per-game achievement definitions and the connected account's unlock state for this provider. The host rejects unrequested game ids, oversized batches and strings, and results fenced by an account change. Available from API minor 4. |

A provider should request only the capabilities it actually needs. Capability names are not
free-form: a descriptor containing an unknown name is invalid and is not loaded.

## Provider secrets

An official bundled provider sees its trusted logical `session` slot; the host rejects other keys.
An authenticated third-party provider may use logical keys from 1 to 128 ASCII letters, digits,
periods, underscores or hyphens. The host derives each physical slot from the authenticated plugin
id, pinned publisher, active account profile, provider id and logical key. A plugin cannot choose a
physical keyring account, file name, owner identity or another plugin's/profile's slot.

Official physical slots are derived from the provider id, unless the trusted descriptor declares an alias:

- **Default keyring account:** `provider.<provider_id>`.
- **Default Linux private file:** `private/provider-<provider_id>.secret` below the application data
  directory.
- **Steam's legacy alias:** `credential_alias = { keyring_account = "steam", private_file = "steam-token" }`.
  Existing Steam sessions keep the same physical credential, with no migration, renaming or
  re-sign-in.

The GameNode account session remains in its own `session` keyring account and
`private/session-token` file. Descriptor aliases may not reuse those names. Third-party aliases use
a separate host-generated prefix and digest, so they cannot select the GameNode session or trusted
Steam alias. Their non-secret account records use a separate persistence adapter keyed by the same
authenticated namespace and never enter Core's official account table. On Windows the host
uses Credential Manager and has no plaintext fallback. On Linux it uses Secret Service and falls
back to a user-only private file only when no Secret Service exists; a locked or failing service is
reported as an error. Secret read, write and delete are independent grants, and a reported storage
method is not proof that a write was durable.

## Login policies and cookie-free extraction

`auth.webview` opens an isolated window with an ephemeral profile scoped to the provider. URLs,
allowed hosts, the page probe and any cookies come from host-approved login policy. Runtime requests
supply only window handles and operations. External HTTPS links outside the approved hosts open in
the system browser. The probe runs only at the exact `probe_origin`, and a navigation change
invalidates an in-flight result.

Third-party login metadata names a supported `probe_id`; it cannot contain JavaScript. The current
`oauth_authorization_code` probe returns only a non-empty `code` query value as
`page_result_json`. Unknown probe ids, publisher-provided `probe_script` fields and invalid
non-HTTPS origins are rejected. Cookie reads are limited to the declared names and URLs in the
approved policy.

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

## Conformance checks for examples and the public surface

Before changing the public protocol or an example, run:

```sh
python3 scripts/check-plugin-conformance.py
python3 -m unittest scripts.tests.test_plugin_conformance
```

The checker verifies:

- both checked-in Go binding trees (`examples/plugins/go-grpc/proto`, `examples/go/proto`) match
  `plugin.proto`. It compares the embedded file descriptor and, when the pinned generators are on
  `PATH`, performs a full regeneration diff. Install libprotoc 36.1 and the pinned `protoc-gen-go`
  and `protoc-gen-go-grpc` versions recorded in `generate-proto.sh`.
- the examples keep their public dependency boundary: the Rust example depends on the exact
  published `gamenode-plugin-sdk` and no internal crate, and the Go modules import no workspace
  package and carry no `replace` directive that escapes the module.
- each example manifest's plugin/provider identity, SemVer version and `api.min`/`api.max` match
  its source constants and the host's supported window (`MIN_API_MINOR` through `API_MINOR`). The
  host's legacy 1.1–1.5 fallback and feature gating are covered by the
  `crates/plugin-protocol/src/capability.rs` tests; the checker keeps the examples aligned with
  those constants.
- `DescribeApi` advertises each served route exactly once, every advertised route exists in
  `plugin.proto`, and the three session methods are present. Routes present in the protocol but
  absent from the served tables are deferred: they must answer `UNIMPLEMENTED` and must never be
  advertised.

Contribution expectations: regenerate bindings with the pinned tools and include them in the same
change as the proto edit; never import Core, platform or application crates into an example; keep a
manifest in step with its identity and API constants; implement a route before adding it to a
`SERVED` table, and remove it from `DescribeApi` if it becomes unavailable. On the host side,
`deferred_plugin_routes_are_unadvertised_and_unimplemented` and the exact `DescribeApi`
route test in `plugin_host::resources::tests` enforce the same boundary.

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
