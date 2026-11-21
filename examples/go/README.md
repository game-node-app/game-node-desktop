# Go protocol interoperability and configuration example

The Go fixture uses generated API protobuf messages directly, without a GameNode SDK or host-side
dynamic RPC dispatcher. It checks exact revisions/timestamps, preservation of unknown extension
namespaces, typed runtime platform/argv fields, and stable content IDs. Launch-consent fixtures cover
emulator/runtime, known mod-layer and environment-only recipes, keep execution bound to the reviewed
recipe reference, and check that installation and file writes use separate ResourceHost requests.
The `cmd/configuration` provider is a minimal raw-gRPC client: it negotiates API 1.6, exchanges
provider commands, opens/focuses/closes a native preference window, and saves its non-secret
preference to host-owned persistent storage.

`extension_data.go` builds a hybrid typed `RuntimeRecord`, keeps an unknown versioned namespace
unchanged during a local patch, demonstrates consumer-side field selection, validates a known schema
identity, and constructs custom dataset and opaque-byte wire shapes. A future schema version is a new
identity: `ValidateKnownSchema` refuses to reinterpret it and callers must carry the bounded envelope
opaquely.

`data_exchange.go` is a runnable producer/consumer client for performance history (publish, explicit
selector, aggregate, end-series cleanup), approved save/location inspection, save-profile ownership,
one shared runtime with several content identities, and custom/opaque datasets. Its tests start a
local gRPC fixture host and assert default-deny, granted and revoked outcomes without a real
collector, a Core dashboard or a filesystem path. Both Go and Rust examples read
`crates/plugin-protocol/contracts/data_exchange_fixtures.json`, so their identities and expected
outcomes stay aligned.

Run it with:

```sh
GOPROXY=off go test -count=1 ./...
```

Build the plugin executable for the native host OS with `go build -o <package-entrypoint>
./cmd/configuration`. The Windows build uses the private named pipe transport; Linux uses the
supervisor's Unix socket. See [`../plugins/README.md`](../plugins/README.md) for native dialog
dependencies, permission grants and the concurrent-provider walkthrough.

The message bindings in `proto/` are generated from `crates/plugin-protocol/proto/plugin.proto` with
`protoc-gen-go` and `protoc-gen-go-grpc`; regenerate them when the public wire contract changes. This
fixture covers wire compatibility only. It checks the declared ResourceHost wire contract and does
not claim that every method is served by the desktop.

Values interpreted by Core: typed identity, ownership, handles, revision, observation time, the
selected game/installation and `active-host` context, schema identity when a signed descriptor
exists, and every grant. Extension `Struct` values, custom field meanings, source/tool names and
collector claims are declarative data only: a consumer must rely on host-enforced authorization and
projection, and a local field filter does not grant access. Private `StoragePut` carries opaque bytes;
those bytes are not cross-plugin data.
