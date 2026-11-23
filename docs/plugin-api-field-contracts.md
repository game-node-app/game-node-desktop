# Plugin API field contracts

The checked [endpoint field policy fixture](../crates/plugin-protocol/contracts/endpoint_field_policy.json)
is derived from [OpenSpec design section 18](../openspec/changes/add-third-party-plugins/design.md#18-per-endpoint-typing-and-extensible-catalogs).
It records each route's rigid request and response fields, extension or opaque-byte allowance,
access model, and protobuf contract status.
The fixture test requires every current `PluginHost`, `ProviderHost`, `ResourceHost`, Saves,
Locations, StorageDiagnostics, Computer, GamePresence, and GamePerformance RPC to appear exactly
once with matching generated request/response types. The formerly planned save-profile, location,
report-query, and aggregate routes now have typed protobuf contracts and checked service catalogs.

The `ProviderHost.Exchange` entry is bidirectional. Its event-family table separately classifies
readiness, account status, catalog chunks, operation results, and runtime-session observations.
Exchange commands/results retain typed operation, account/profile/session context, and correlation
fields. Only explicitly designated descriptive domain records may contain namespaced extensions.
Secrets and login cookies are excluded from generic metadata export. Settings retain their closed
form schema. Configuration-window lifecycle messages carry typed session/profile/window handles
and outcomes.

The matrix is a wire-contract inventory, not a claim that every described API is mounted by the
desktop host. These protobuf contracts do not imply that host-service implementations are
complete or callable.

The runtime-session Exchange family now has typed `RuntimeSessionReport` and
`RuntimeSessionResult` messages with `game.sessions.report` authorization and correlated outcomes.
One plugin may implement adapters for several runtimes; Core consumes only the common lifecycle
reports and does not contain emulator-specific detection. The first report is limited to the
authenticated extension's host-selected game and installation and an approved content/runtime
binding. To report another game or installation from the same running runtime instance, the
extension requests a context using proposed identifiers; the host resolves the current library and
binding, checks the active session/profile/grant and requires a fresh, non-rememberable user
confirmation. Reports for the new context must carry the opaque host-issued context id. It is
session-only, bound to the runtime instance and approved target/revisions, expires after 30 seconds
without an accepted report, and does not survive a session restart. A new context requires fresh
active evidence and confirmation.

Adapters should report at least every 10 seconds. `observed_at` is Unix time in seconds; the host
rejects a report more than 30 seconds old or more than 5 seconds in the future. Report expiry is
measured independently from host monotonic receipt time, so plugin wall clocks cannot extend a
session. Each game transition closes the prior interval at the last accepted observation and starts
the new one at the approved event timestamp; time between them stays unattributed. `GamePresence`
is descriptive metadata: it cannot start, extend or confirm a measured playtime session.
Runtime-session observations are sensitive and may be projected through Data only under the
relevant sharing consent; process handles, credentials and launch tokens are never part of that
projection.

Rust SDK patch preservation is covered by `crates/plugin-sdk/tests/reference_host.rs`.
