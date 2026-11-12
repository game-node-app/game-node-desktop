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
Its session arbitration and accounting behavior remain tracked under task 14.3.

Rust SDK patch preservation is covered by `crates/plugin-sdk/tests/reference_host.rs`; direct Go
protobuf preservation and exact 64-bit field handling are covered by the runnable interoperability
fixture in [`examples/go`](../examples/go/README.md).
